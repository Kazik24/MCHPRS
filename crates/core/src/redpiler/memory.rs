//! Linux headroom checks for compiler buffers and long-running probes.
// shortcut: concurrent allocations can outpace these checks; use worker processes for hard OOM/native-fault isolation.
use super::scratch::ScratchError;

pub(super) const LOW_MEMORY: &str = "Redpiler compilation stopped: insufficient server memory; free memory or compile a smaller circuit";
#[cfg(any(target_os = "linux", test))]
const HEADROOM: usize = 512 * 1024 * 1024;

#[cfg(any(target_os = "linux", test))]
fn admit(available: usize, additional: usize) -> Result<(), ScratchError> {
    if additional.checked_add(HEADROOM).is_none_or(|needed| needed > available) {
        Err(ScratchError {
            position: None,
            reason: LOW_MEMORY,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn check(additional: usize) -> Result<(), ScratchError> {
    #[cfg(target_os = "linux")]
    {
        let available = available().map_err(|error| {
            tracing::warn!(%error, "cannot read Redpiler memory limits");
            ScratchError {
                position: None,
                reason: "Redpiler cannot read server memory limits; see server log",
            }
        })?;
        return admit(available, additional);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = additional;
        Ok(())
    }
}

pub(super) fn zeroed<T: Default + Clone>(length: usize) -> Result<Vec<T>, ScratchError> {
    let error = || ScratchError {
        position: None,
        reason: LOW_MEMORY,
    };
    check(length.checked_mul(std::mem::size_of::<T>()).ok_or_else(error)?)?;
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| error())?;
    values.resize(length, T::default());
    Ok(values)
}

#[cfg(target_os = "linux")]
pub(super) fn check_periodic() -> Result<(), ScratchError> {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };
    static LAST: Mutex<Option<(Instant, Result<(), ScratchError>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((at, result)) = &*last
        && at.elapsed() < Duration::from_millis(250)
    {
        return result.clone();
    }
    let result = check(0);
    *last = Some((Instant::now(), result.clone()));
    result
}

#[cfg(not(target_os = "linux"))]
pub(super) fn check_periodic() -> Result<(), ScratchError> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn available() -> std::io::Result<usize> {
    use std::{fs, io, path::Path};
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let mut available = meminfo
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemAvailable:")?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()?
                .checked_mul(1024)
        })
        .ok_or_else(|| io::Error::other("MemAvailable is missing"))?;
    let cgroups = fs::read_to_string("/proc/self/cgroup")?;
    for line in cgroups.lines() {
        let mut fields = line.splitn(3, ':');
        let _ = fields.next();
        let controllers = fields.next().unwrap_or("");
        let relative = fields.next().unwrap_or("").trim_start_matches('/');
        let (root, limit, usage) = if controllers.is_empty() {
            (Path::new("/sys/fs/cgroup"), "memory.max", "memory.current")
        } else if controllers.split(',').any(|name| name == "memory") {
            (Path::new("/sys/fs/cgroup/memory"), "memory.limit_in_bytes", "memory.usage_in_bytes")
        } else {
            continue;
        };
        // A private cgroup namespace exposes its own limit at the mount root.
        let mut directory = root.join(relative);
        loop {
            let path = directory.join(limit);
            if path.exists() {
                let limit = fs::read_to_string(path)?;
                let usage = fs::read_to_string(directory.join(usage))?;
                available = available.min(cgroup_available(&limit, &usage)?);
            }
            if directory == root || !directory.pop() || !directory.starts_with(root) {
                break;
            }
        }
    }
    Ok(available)
}

#[cfg(any(target_os = "linux", test))]
fn cgroup_available(limit: &str, usage: &str) -> std::io::Result<usize> {
    let parse = |value: &str| value.trim().parse::<usize>().map_err(std::io::Error::other);
    Ok(if limit.trim() == "max" {
        usize::MAX
    } else {
        parse(limit)?.saturating_sub(parse(usage)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_keeps_headroom_and_rejects_overflow() {
        assert!(admit(HEADROOM + 100, 100).is_ok());
        assert_eq!(admit(HEADROOM + 99, 100).unwrap_err().reason, LOW_MEMORY);
        assert!(admit(usize::MAX, usize::MAX).is_err());
        assert_eq!(cgroup_available("6442450944\n", "5368709120\n").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(cgroup_available("max\n", "100\n").unwrap(), usize::MAX);
        assert_eq!(cgroup_available("100", "101").unwrap(), 0);
        assert!(cgroup_available("garbage", "0").is_err());
    }
}
