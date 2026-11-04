//! Process-owned compiler artifacts. Only the marked child directory is reset at startup.
use crate::redpiler::TaskMonitor;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, MutexGuard, OnceLock, TryLockError},
    time::{Duration, Instant},
};

const OWNER: &[u8] = b"MROWW native compiler cache v1\n";
const CHILD: &str = "mroww-native-cache";
static CACHE: OnceLock<Result<Cache, String>> = OnceLock::new();

struct Cache {
    path: PathBuf,
    _lock: File,
    module: Mutex<ModuleState>,
}

#[derive(Default)]
struct ModuleState {
    generation: u64,
    path: Option<PathBuf>,
}

pub(super) struct ModulePath {
    path: PathBuf,
    // Keep clear from deleting the file between path lookup and library loading.
    _guard: Option<MutexGuard<'static, ModuleState>>,
}

impl std::ops::Deref for ModulePath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

fn validate_owned_root(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || path.canonicalize()? != path {
        return Err(io::Error::other("LLVM cache child must be a real directory inside the work volume"));
    }
    let marker = path.join(".owner");
    let metadata = fs::symlink_metadata(&marker)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || fs::read(marker)? != OWNER {
        return Err(io::Error::other("refusing to remove an unmarked LLVM cache directory"));
    }
    Ok(())
}

fn build_directory(cache: &Cache, key: &str, generation: u64) -> PathBuf {
    cache.path.join(format!("{key}-{generation}"))
}

fn build_name(name: &str) -> bool {
    let (key, generation) = name.split_once('-').map_or((name, None), |(key, generation)| (key, Some(generation)));
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()) && generation.is_none_or(|generation| generation.parse::<u64>().is_ok())
}

fn validate_build_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || path.canonicalize()? != path {
        return Err(io::Error::other("refusing a redirected LLVM artifact directory"));
    }
    for file in fs::read_dir(path)? {
        let metadata = file?.file_type()?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(io::Error::other("LLVM artifact directory contains redirected or nested entries"));
        }
    }
    Ok(())
}

impl Cache {
    fn clear(&self) -> Result<String, String> {
        let mut module = self.module.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => "LLVM module build or load is busy; retry clearing after it finishes".to_owned(),
            TryLockError::Poisoned(_) => "LLVM cache lock poisoned".to_owned(),
        })?;
        validate_owned_root(&self.path).map_err(|error| format!("cannot clear LLVM cache: {error}"))?;
        let mut directories = Vec::new();
        let mut deferred = 0;
        for entry in fs::read_dir(&self.path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_name() == ".owner" {
                continue;
            }
            let path = entry.path();
            if !entry.file_name().to_str().is_some_and(build_name) {
                deferred += 1;
                continue;
            }
            validate_build_directory(&path).map_err(|error| error.to_string())?;
            directories.push(path);
        }
        module.generation = module.generation.checked_add(1).ok_or("LLVM cache generation limit exceeded")?;
        module.path = None;
        let mut removed = 0;
        for path in directories {
            // The marked root, canonical child and its flat files were validated above.
            match fs::remove_dir_all(&path) {
                Ok(()) => removed += 1,
                Err(error) => {
                    deferred += 1;
                    tracing::warn!(path = %path.display(), %error, "LLVM artifact removal deferred; the library may still be in use");
                }
            }
        }
        Ok(format!(
            "LLVM cache invalidated: {removed} artifact directories removed, {deferred} entries deferred (in use or unrecognized). New modules will rebuild; custom MCHPRS_LLVM_DLL files are untouched."
        ))
    }
}

fn reset_root(root: &Path) -> io::Result<Cache> {
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".mroww-native.lock"))?;
    lock.try_lock()
        .map_err(|error| io::Error::other(format!("LLVM cache is already in use: {error}")))?;
    let path = root.join(CHILD);
    if fs::symlink_metadata(&path).is_ok() {
        validate_owned_root(&path)?;
        // Both the resolved target and its ownership marker were checked above.
        fs::remove_dir_all(&path)?;
    }
    fs::create_dir(&path)?;
    fs::write(path.join(".owner"), OWNER)?;
    Ok(Cache {
        path,
        _lock: lock,
        module: Mutex::new(ModuleState::default()),
    })
}

fn cache() -> Result<&'static Cache, String> {
    CACHE
        .get_or_init(|| {
            let root = std::env::var_os("MCHPRS_LLVM_WORK_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./work/llvm"));
            reset_root(&root).map_err(|error| format!("cannot initialize LLVM cache {}: {error}", root.display()))
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn initialize() -> Result<(), String> {
    let cache = cache()?;
    tracing::info!(path = %cache.path.display(), "LLVM compiler cache reset for this process");
    Ok(())
}

pub(crate) fn clear() -> Result<String, String> {
    cache()?.clear()
}

fn absolute_executable(path: PathBuf) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().map_err(|error| error.to_string())?.join(path)
    };
    if !path.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    Ok(path)
}

fn executable(name: &str, configured: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(configured) {
        return absolute_executable(PathBuf::from(path)).map_err(|error| format!("{configured}: {error}"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let path = directory.join(name);
            if path.is_file() {
                return absolute_executable(path);
            }
        }
    }
    #[cfg(windows)]
    {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/llvm-23.1.4/bin").join(name);
        if path.is_file() {
            return absolute_executable(path);
        }
    }
    Err(format!("{name} is not installed; set {configured} to its path"))
}

fn hash_file(hash: &mut Sha256, path: &Path) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        hash.update(&buffer[..read]);
    }
}

fn run_tool(tool: &Path, args: &[OsString], directory: &Path, log: &str, monitor: Option<&TaskMonitor>) -> Result<(), String> {
    let path = directory.join(log);
    let mut command = Command::new(tool);
    command
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(File::create(&path).map_err(|e| e.to_string())?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|error| format!("cannot run {}: {error}", tool.display()))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut text = String::new();
                if let Ok(file) = File::open(&path) {
                    let _ = file.take(65536).read_to_string(&mut text);
                }
                return Err(format!("{} failed ({status}); {}: {text}", tool.display(), path.display()));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
            Ok(None) => {}
        }
        if monitor.is_some_and(TaskMonitor::cancelled) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("LLVM compilation cancelled".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn module_path(monitor: Option<&TaskMonitor>) -> Result<ModulePath, String> {
    if let Some(path) = std::env::var_os("MCHPRS_LLVM_DLL") {
        let path = PathBuf::from(path).canonicalize().map_err(|error| format!("MCHPRS_LLVM_DLL: {error}"))?;
        return Ok(ModulePath { path, _guard: None });
    }
    let cache = cache()?;
    let mut module = cache.module.lock().map_err(|_| "LLVM cache lock poisoned".to_owned())?;
    validate_owned_root(&cache.path).map_err(|error| error.to_string())?;
    if let Some(path) = module.path.as_ref().filter(|path| path.is_file()) {
        validate_build_directory(path.parent().unwrap()).map_err(|error| error.to_string())?;
        return Ok(ModulePath {
            path: path.clone(),
            _guard: Some(module),
        });
    }
    #[cfg(windows)]
    let clang = executable("clang.exe", "MCHPRS_LLVM_CLANG")?;
    #[cfg(not(windows))]
    let clang = executable("clang", "MCHPRS_LLVM_CLANG")?;
    #[cfg(windows)]
    let linker = if std::env::var_os("MCHPRS_LLVM_LINKER").is_some() {
        executable("lld-link.exe", "MCHPRS_LLVM_LINKER")?
    } else {
        let adjacent = clang.with_file_name("lld-link.exe");
        if adjacent.is_file() {
            adjacent
        } else {
            executable("lld-link.exe", "MCHPRS_LLVM_LINKER")?
        }
    };
    #[cfg(not(windows))]
    let linker = executable("ld.lld", "MCHPRS_LLVM_LINKER")?;
    let sources: [(&str, &[u8]); 4] = [
        ("engine.c", include_bytes!("engine.c")),
        ("llvm_movement.c", include_bytes!("llvm_movement.c")),
        ("command_engine.c", include_bytes!("command_engine.c")),
        ("abi.h", include_bytes!("abi.h")),
    ];
    let flags = ["-std=c11", "-O3", "-ffreestanding", "-fno-builtin", "-Werror"];
    let mut hash = Sha256::new();
    for (name, data) in sources {
        hash.update(name);
        hash.update(data);
    }
    for flag in flags {
        hash.update(flag);
    }
    hash.update(std::env::consts::ARCH);
    hash.update(std::env::consts::OS);
    hash_file(&mut hash, &clang).map_err(|e| e.to_string())?;
    hash_file(&mut hash, &linker).map_err(|e| e.to_string())?;
    let key = format!("{:x}", hash.finalize());
    let directory = build_directory(cache, &key, module.generation);
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    validate_build_directory(&directory).map_err(|error| error.to_string())?;
    for (name, data) in sources {
        fs::write(directory.join(name), data).map_err(|e| e.to_string())?;
    }
    let mut args: Vec<OsString> = flags.iter().map(OsString::from).collect();
    let started = Instant::now();
    #[cfg(windows)]
    let filename = {
        args.extend(["--target=x86_64-pc-windows-msvc", "-c", "engine.c", "-o", "engine.obj"].map(OsString::from));
        run_tool(&clang, &args, &directory, "compile.log", monitor)?;
        let args = [
            "/dll",
            "/noentry",
            "/nodefaultlib",
            "/machine:x64",
            "/out:engine.pending.dll",
            "engine.obj",
        ]
        .map(OsString::from);
        run_tool(&linker, &args, &directory, "link.log", monitor)?;
        fs::rename(directory.join("engine.pending.dll"), directory.join("engine.dll")).map_err(|e| e.to_string())?;
        "engine.dll"
    };
    #[cfg(not(windows))]
    let filename = {
        args.extend(["-fPIC", "-shared", "engine.c", "-o", "engine.pending.so"].map(OsString::from));
        args.push(OsString::from(format!("--ld-path={}", linker.display())));
        args.extend(["-Wl,-z,defs", "-Wl,-z,relro", "-Wl,-z,now"].map(OsString::from));
        run_tool(&clang, &args, &directory, "compile.log", monitor)?;
        fs::rename(directory.join("engine.pending.so"), directory.join("engine.so")).map_err(|e| e.to_string())?;
        "engine.so"
    };
    let path = directory.join(filename);
    let version = Command::new(&clang).arg("--version").output().map_err(|e| e.to_string())?;
    let manifest = serde_json::json!({"key":key,"clang":clang,"linker":linker,"flags":args,"target_arch":std::env::consts::ARCH,
        "target_os":std::env::consts::OS,"compiler_version":String::from_utf8_lossy(&version.stdout),
        "build_ms":started.elapsed().as_secs_f64()*1000.0,"module_bytes":fs::metadata(&path).map_err(|e|e.to_string())?.len()});
    fs::write(
        directory.join("build.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    tracing::info!(path=%path.display(),build_ms=started.elapsed().as_secs_f64()*1000.0,"LLVM module compiled");
    module.path = Some(path.clone());
    Ok(ModulePath { path, _guard: Some(module) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mroww-cache-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn clear_invalidates_module_and_uses_a_fresh_build_path_without_deleting_unowned_files() {
        let root = temp_root("clear");
        let cache = reset_root(&root).unwrap();
        let key = "a".repeat(64);
        let directory = build_directory(&cache, &key, 0);
        fs::create_dir(&directory).unwrap();
        let module_path = directory.join("engine.dll");
        fs::write(&module_path, b"old module").unwrap();
        cache.module.lock().unwrap().path = Some(module_path);
        let custom = root.join("custom.dll");
        fs::write(&custom, b"custom module").unwrap();
        fs::write(cache.path.join("keep"), b"unrecognized").unwrap();
        let summary = cache.clear().unwrap();
        assert!(summary.contains("1 artifact directories removed, 1 entries deferred"));
        let state = cache.module.lock().unwrap();
        assert!(state.path.is_none());
        assert_ne!(build_directory(&cache, &key, state.generation), directory);
        assert_eq!(state.generation, 1);
        assert!(!directory.exists());
        assert_eq!(fs::read(custom).unwrap(), b"custom module");
        assert!(cache.path.join("keep").exists());
        drop(state);
        drop(cache);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_refuses_busy_builds_and_changed_ownership_markers() {
        let root = temp_root("busy");
        let cache = reset_root(&root).unwrap();
        let state = cache.module.lock().unwrap();
        assert!(cache.clear().unwrap_err().contains("busy"));
        drop(state);
        fs::write(cache.path.join(".owner"), b"not ours").unwrap();
        assert!(cache.clear().is_err());
        assert_eq!(cache.module.lock().unwrap().generation, 0);
        drop(cache);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn clear_defers_locked_artifacts_but_invalidates_the_module() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp_root("locked");
        let cache = reset_root(&root).unwrap();
        let directory = build_directory(&cache, &"b".repeat(64), 0);
        fs::create_dir(&directory).unwrap();
        let module = directory.join("engine.dll");
        fs::write(&module, b"loaded module").unwrap();
        cache.module.lock().unwrap().path = Some(module.clone());
        let locked = OpenOptions::new().read(true).share_mode(0).open(&module).unwrap();
        let summary = cache.clear().unwrap();
        assert!(summary.contains("0 artifact directories removed, 1 entries deferred"));
        assert!(cache.module.lock().unwrap().path.is_none());
        assert_eq!(cache.module.lock().unwrap().generation, 1);
        drop(locked);
        drop(cache);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn clear_rejects_symlinked_artifact_directories_and_files() {
        use std::os::unix::fs::symlink;
        let root = temp_root("links");
        let cache = reset_root(&root).unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"keep").unwrap();
        let directory = build_directory(&cache, &"c".repeat(64), 0);
        symlink(&outside, &directory).unwrap();
        assert!(cache.clear().is_err());
        fs::remove_file(&directory).unwrap();
        fs::create_dir(&directory).unwrap();
        symlink(outside.join("keep"), directory.join("engine.so")).unwrap();
        assert!(cache.clear().is_err());
        assert_eq!(cache.module.lock().unwrap().generation, 0);
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
        drop(cache);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reset_removes_only_owned_cache_and_obeys_lock() {
        let root = std::env::temp_dir().join(format!(
            "mroww-cache-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("keep"), b"keep").unwrap();
        #[cfg(unix)]
        {
            let link = root.join("ld.lld");
            std::os::unix::fs::symlink(root.join("keep"), &link).unwrap();
            assert_eq!(absolute_executable(link.clone()).unwrap(), link);
        }
        let child = root.join(CHILD);
        fs::create_dir(&child).unwrap();
        fs::write(child.join("keep"), b"unowned").unwrap();
        assert!(reset_root(&root).is_err());
        assert!(child.join("keep").exists());
        fs::write(child.join(".owner"), OWNER).unwrap();
        let cache = reset_root(&root).unwrap();
        assert!(!child.join("keep").exists());
        assert!(reset_root(&root).is_err());
        fs::write(child.join("artifact"), b"old").unwrap();
        drop(cache);
        let cache = reset_root(&root).unwrap();
        assert!(!child.join("artifact").exists());
        assert_eq!(fs::read(root.join("keep")).unwrap(), b"keep");
        drop(cache);
        fs::remove_dir_all(root).unwrap();
    }
}
