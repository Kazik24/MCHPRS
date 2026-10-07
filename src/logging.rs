use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tracing_subscriber::fmt::MakeWriter;

// Two files, 16 MiB each. Rotation also bounds command-spam logs.
pub struct Log(Mutex<Files>);
struct Files {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl Log {
    pub fn open(path: &Path, limit: u64) -> io::Result<Self> {
        assert!(limit > 0);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self(Mutex::new(Files {
            path: path.into(),
            file: Some(file),
            bytes,
            limit,
        })))
    }
}

pub struct Writer<'a>(MutexGuard<'a, Files>);
impl<'a> MakeWriter<'a> for Log {
    type Writer = Writer<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        Writer(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let files = &mut *self.0;
        // Even a single huge debug event cannot exceed the file limit.
        let mut remaining = bytes;
        while !remaining.is_empty() {
            if files.bytes >= files.limit {
                files.file.take(); // Close before rename on Windows.
                let backup = files.path.with_extension("log.1");
                match fs::remove_file(&backup) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
                fs::rename(&files.path, backup)?;
                files.bytes = 0;
            }
            if files.file.is_none() {
                files.file = Some(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&files.path)?,
                );
            }
            let count = remaining.len().min((files.limit - files.bytes) as usize);
            files
                .file
                .as_mut()
                .unwrap()
                .write_all(&remaining[..count])?;
            files.bytes += count as u64;
            remaining = &remaining[count..];
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &mut self.0.file {
            file.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spam_and_restart_keep_only_two_bounded_files() {
        let path = std::env::temp_dir().join(format!("mroww-log-{}.log", std::process::id()));
        let backup = path.with_extension("log.1");
        let log = Log::open(&path, 16).unwrap();
        log.make_writer().write_all(&[b'x'; 100]).unwrap();
        drop(log);
        let log = Log::open(&path, 16).unwrap();
        log.make_writer().write_all(&[b'y'; 100]).unwrap();
        drop(log);
        assert!(fs::metadata(&path).unwrap().len() <= 16);
        assert!(fs::metadata(&backup).unwrap().len() <= 16);
        fs::remove_file(path).unwrap();
        fs::remove_file(backup).unwrap();
    }
}
