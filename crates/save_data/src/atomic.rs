//! Write a complete file beside the destination, sync, then replace it.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
pub fn backup(path: &Path) -> io::Result<PathBuf> {
    let mut n = 0;
    loop {
        let suffix = if n == 0 {
            "bak".to_owned()
        } else {
            format!("bak.{n}")
        };
        let target = path.with_extension(suffix);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut out) => {
                let mut input = fs::File::open(path)?;
                io::copy(&mut input, &mut out)?;
                out.sync_all()?;
                return Ok(target);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let temp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        out.write_all(bytes)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
