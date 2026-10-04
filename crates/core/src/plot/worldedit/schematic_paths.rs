//! Schematic paths stay within their library; the shared rf folder is read-only.
use anyhow::{bail, ensure, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn relative_path(name: &str) -> Result<PathBuf> {
    let name = name.replace('\\', "/");
    ensure!(
        !name.starts_with('/') && !name.ends_with('/'),
        "Use a relative schematic filename."
    );
    let mut path = PathBuf::new();
    for part in name
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        ensure!(
            part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || ":*?\"<>|".contains(c)),
            "Invalid schematic path."
        );
        path.push(part);
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    ensure!(
        extension.eq_ignore_ascii_case("schem") || extension.eq_ignore_ascii_case("schematic"),
        "Include a .schem or .schematic extension."
    );
    Ok(path)
}

fn in_rf(path: &Path) -> bool {
    path.components().next().is_some_and(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("rf")
    })
}

fn within_root(root: &Path, path: &Path) -> Result<PathBuf> {
    let resolved = path.canonicalize()?;
    ensure!(
        resolved.starts_with(root),
        "Schematic path leaves the schematic folder."
    );
    Ok(resolved)
}

pub(super) fn load_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = relative_path(name)?;
    let root = root.canonicalize()?;
    let exact = root.join(&relative);
    match fs::symlink_metadata(&exact) {
        Ok(_) => return within_root(&root, &exact),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // An explicit subfolder is exact. A bare filename searches the library.
    if relative.components().count() != 1 {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    let mut folders = vec![root.clone()];
    let mut matches = Vec::new();
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            // Do not follow directory links: recursive searches must not loop.
            if kind.is_dir() {
                folders.push(entry.path());
            } else if entry.file_name() == relative.as_os_str() {
                matches.push(within_root(&root, &entry.path())?);
            }
        }
    }
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => Err(std::io::Error::from(std::io::ErrorKind::NotFound).into()),
        1 => Ok(matches.pop().unwrap()),
        _ => bail!(
            "More than one schematic named {name}. Use a subfolder path, such as {}.",
            matches[0].strip_prefix(&root)?.display()
        ),
    }
}

pub(super) fn save_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = relative_path(name)?;
    ensure!(
        !in_rf(&relative),
        "The rf schematic folder is read-only. Save outside rf."
    );
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let mut ancestor = root.join(&relative);
    let mut suffix = Vec::new();
    // Resolve existing ancestors before creating directories, including links.
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(
                    ancestor
                        .file_name()
                        .context("Invalid schematic path")?
                        .to_owned(),
                );
                ensure!(ancestor.pop(), "Invalid schematic path");
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut resolved = within_root(&root, &ancestor)?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    ensure!(
        !in_rf(resolved.strip_prefix(&root)?),
        "The rf schematic folder is read-only. Save outside rf."
    );
    // A link named otherwise may still point into the shared library.
    if let Ok(shared) = root.join("rf").canonicalize() {
        ensure!(
            !resolved.starts_with(shared),
            "The rf schematic folder is read-only. Save outside rf."
        );
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Library(PathBuf);
    impl Library {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("mchprs-schematic-paths-{}", rand::random::<u64>()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, name: &str) {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"original").unwrap();
        }
    }
    impl Drop for Library {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recursive_loading_requires_an_unambiguous_name() {
        let library = Library::new();
        library.file("rf/deep/circuit.schem");
        let expected = library
            .0
            .join("rf/deep/circuit.schem")
            .canonicalize()
            .unwrap();
        assert_eq!(load_path(&library.0, "circuit.schem").unwrap(), expected);
        assert_eq!(
            load_path(&library.0, "./rf/deep/circuit.schem").unwrap(),
            expected
        );
        assert_eq!(
            load_path(&library.0, "rf\\deep\\circuit.schem").unwrap(),
            expected
        );
        library.file("other/circuit.schem");
        assert!(load_path(&library.0, "circuit.schem")
            .unwrap_err()
            .to_string()
            .contains("More than one"));
        library.file("circuit.schem");
        assert_eq!(
            load_path(&library.0, "circuit.schem").unwrap(),
            library.0.join("circuit.schem").canonicalize().unwrap()
        );
        assert!(load_path(&library.0, "missing.schem").is_err());
        assert!(load_path(&library.0, "missing/circuit.schem").is_err());
    }

    #[test]
    fn shared_schematics_cannot_be_created_or_overwritten() {
        let library = Library::new();
        library.file("rf/original.schem");
        for name in [
            "rf/original.schem",
            "./rf/new.schem",
            "RF/deep/new.schem",
            "rf\\new.schem",
        ] {
            assert!(save_path(&library.0, name)
                .unwrap_err()
                .to_string()
                .contains("read-only"));
        }
        assert_eq!(
            fs::read(library.0.join("rf/original.schem")).unwrap(),
            b"original"
        );
        assert!(!library.0.join("rf/new.schem").exists());
        assert!(!library.0.join("rf/deep").exists());
        assert!(save_path(&library.0, "my_builds/new.schem")
            .unwrap()
            .ends_with("my_builds/new.schem"));
        for name in [
            "../outside.schem",
            "rf/../new.schem",
            "/absolute.schem",
            "F:\\outside.schem",
            "file.schem.txt",
            "rf./new.schem",
        ] {
            assert!(save_path(&library.0, name).is_err(), "{name}");
            assert!(load_path(&library.0, name).is_err(), "{name}");
        }
    }

    #[test]
    fn links_cannot_bypass_library_boundaries() {
        fn link(target: &Path, path: &Path) {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, path).unwrap();
            #[cfg(windows)]
            assert!(std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(path)
                .arg(target)
                .output()
                .unwrap()
                .status
                .success());
        }
        let library = Library::new();
        let outside = Library::new();
        library.file("rf/original.schem");
        outside.file("outside.schem");
        link(&library.0.join("rf"), &library.0.join("alias"));
        link(&outside.0, &library.0.join("escape"));
        link(&library.0, &library.0.join("loop"));
        assert!(save_path(&library.0, "alias/original.schem").is_err());
        assert!(save_path(&library.0, "alias/new.schem").is_err());
        assert!(save_path(&library.0, "escape/new.schem").is_err());
        assert!(load_path(&library.0, "escape/outside.schem").is_err());
        assert!(load_path(&library.0, "original.schem").is_ok());
    }
}
