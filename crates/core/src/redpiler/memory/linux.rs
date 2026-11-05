//! Read host and cgroup memory limits without confusing bytes with /proc's KiB.
use std::{fs, io, path::Path};

pub(super) fn available_memory() -> io::Result<usize> {
    let mut available_bytes = parse_mem_available(&fs::read_to_string("/proc/meminfo")?)?;
    let memberships = fs::read_to_string("/proc/self/cgroup")?;
    for membership in memberships.lines() {
        let mut fields = membership.splitn(3, ':');
        let _hierarchy_id = fields.next();
        let controllers = fields.next().unwrap_or("");
        let relative_path = fields.next().unwrap_or("").trim_start_matches('/');
        if let Some(controller) = MemoryController::from_controllers(controllers) {
            available_bytes = available_bytes.min(controller.available_in_hierarchy(relative_path)?);
        }
    }
    Ok(available_bytes)
}

fn parse_mem_available(meminfo: &str) -> io::Result<usize> {
    meminfo
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemAvailable:")?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()?
                .checked_mul(1024)
        })
        .ok_or_else(|| io::Error::other("MemAvailable is missing or invalid"))
}

struct MemoryController<'a> {
    root: &'a Path,
    limit_file: &'static str,
    usage_file: &'static str,
}

impl MemoryController<'static> {
    fn from_controllers(controllers: &str) -> Option<Self> {
        if controllers.is_empty() {
            Some(Self {
                root: Path::new("/sys/fs/cgroup"),
                limit_file: "memory.max",
                usage_file: "memory.current",
            })
        } else if controllers.split(',').any(|controller| controller == "memory") {
            Some(Self {
                root: Path::new("/sys/fs/cgroup/memory"),
                limit_file: "memory.limit_in_bytes",
                usage_file: "memory.usage_in_bytes",
            })
        } else {
            None
        }
    }
}

impl MemoryController<'_> {
    fn available_in_hierarchy(&self, relative_path: &str) -> io::Result<usize> {
        let mut available_bytes = usize::MAX;
        let mut directory = self.root.join(relative_path);
        loop {
            let limit_path = directory.join(self.limit_file);
            if limit_path.exists() {
                let limit = fs::read_to_string(limit_path)?;
                let usage = fs::read_to_string(directory.join(self.usage_file))?;
                available_bytes = available_bytes.min(parse_cgroup_available(&limit, &usage)?);
            }
            // Private cgroup namespaces expose their own limit at the mount root.
            if directory == self.root || !directory.pop() || !directory.starts_with(self.root) {
                break;
            }
        }
        Ok(available_bytes)
    }
}

fn parse_cgroup_available(limit: &str, usage: &str) -> io::Result<usize> {
    let parse_bytes = |value: &str| value.trim().parse::<usize>().map_err(io::Error::other);
    if limit.trim() == "max" {
        Ok(usize::MAX)
    } else {
        Ok(parse_bytes(limit)?.saturating_sub(parse_bytes(usage)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_memory_requires_a_valid_available_value_in_kib() {
        assert_eq!(parse_mem_available("MemTotal: 100 kB\nMemAvailable: 42 kB\n").unwrap(), 42 * 1024);
        for invalid in ["", "MemAvailable: bad kB", "MemAvailable: 18446744073709551615 kB"] {
            assert!(parse_mem_available(invalid).is_err());
        }
    }

    #[test]
    fn cgroup_limits_handle_unlimited_exhausted_and_invalid_values() {
        assert_eq!(parse_cgroup_available("6442450944\n", "5368709120\n").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_cgroup_available("max\n", "100\n").unwrap(), usize::MAX);
        assert_eq!(parse_cgroup_available("100", "101").unwrap(), 0);
        assert!(parse_cgroup_available("garbage", "0").is_err());
        assert!(parse_cgroup_available("100", "garbage").is_err());
    }

    #[test]
    fn both_cgroup_versions_use_the_tightest_visible_ancestor() {
        let root = std::env::temp_dir().join(format!("redpiler-memory-{}-{:032x}", std::process::id(), rand::random::<u128>()));
        let child = root.join("plot");
        fs::create_dir_all(&child).unwrap();
        for controllers in ["", "cpu,memory"] {
            let layout = MemoryController::from_controllers(controllers).unwrap();
            let controller = MemoryController { root: &root, ..layout };
            fs::write(root.join(controller.limit_file), "1000").unwrap();
            fs::write(root.join(controller.usage_file), "900").unwrap();
            fs::write(child.join(controller.limit_file), "500").unwrap();
            fs::write(child.join(controller.usage_file), "100").unwrap();
            assert_eq!(controller.available_in_hierarchy("plot").unwrap(), 100);
            assert_eq!(controller.available_in_hierarchy("").unwrap(), 100);
            fs::write(child.join(controller.usage_file), "450").unwrap();
            assert_eq!(controller.available_in_hierarchy("plot").unwrap(), 50);
        }
        assert!(MemoryController::from_controllers("cpu,pids").is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
