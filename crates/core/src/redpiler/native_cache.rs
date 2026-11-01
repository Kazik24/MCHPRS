//! Process-owned compiler artifacts. Only the marked child directory is reset at startup.
use super::TaskMonitor;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const OWNER: &[u8] = b"MROWW native compiler cache v1\n";
const CHILD: &str = "mroww-native-cache";
static CACHE: OnceLock<Result<Cache, String>> = OnceLock::new();

struct Cache {
    path: PathBuf,
    _lock: File,
    module: Mutex<Option<PathBuf>>,
}

fn reset_root(root: &Path) -> io::Result<Cache> {
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(root.join(".mroww-native.lock"))?;
    lock.try_lock().map_err(|error| io::Error::other(format!("native cache is already in use: {error}")))?;
    let path = root.join(CHILD);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() || path.canonicalize()? != path {
            return Err(io::Error::other("native cache child must be a real directory inside the work volume"));
        }
        if fs::read(path.join(".owner")).ok().as_deref() != Some(OWNER) {
            return Err(io::Error::other("refusing to remove an unmarked native cache directory"));
        }
        // Both the resolved target and its ownership marker were checked above.
        fs::remove_dir_all(&path)?;
    }
    fs::create_dir(&path)?;
    fs::write(path.join(".owner"), OWNER)?;
    Ok(Cache { path, _lock: lock, module: Mutex::new(None) })
}

fn cache() -> Result<&'static Cache, String> {
    CACHE.get_or_init(|| {
        let root = std::env::var_os("MCHPRS_LLVM_WORK_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("./work/llvm"));
        reset_root(&root).map_err(|error| format!("cannot initialize native cache {}: {error}", root.display()))
    }).as_ref().map_err(Clone::clone)
}

pub(crate) fn initialize() -> Result<(), String> {
    let cache = cache()?;
    tracing::info!(path = %cache.path.display(), "native compiler cache reset for this process");
    Ok(())
}

fn executable(name: &str, configured: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(configured) {
        return PathBuf::from(path).canonicalize().map_err(|error| format!("{configured}: {error}"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let path = directory.join(name);
            if path.is_file() { return path.canonicalize().map_err(|error| error.to_string()); }
        }
    }
    #[cfg(windows)]
    {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/llvm-23.1.4/bin").join(name);
        if path.is_file() { return path.canonicalize().map_err(|error| error.to_string()); }
    }
    Err(format!("{name} is not installed; set {configured} to its path"))
}

fn hash_file(hash: &mut Sha256, path: &Path) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop { let read = file.read(&mut buffer)?; if read == 0 { return Ok(()); } hash.update(&buffer[..read]); }
}

fn run_tool(tool: &Path, args: &[OsString], directory: &Path, log: &str, monitor: Option<&TaskMonitor>) -> Result<(), String> {
    let path = directory.join(log);
    let mut command = Command::new(tool);
    command.args(args).current_dir(directory).stdin(Stdio::null()).stdout(Stdio::null()).stderr(File::create(&path).map_err(|e| e.to_string())?);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn().map_err(|error| format!("cannot run {}: {error}", tool.display()))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut text = String::new();
                if let Ok(file) = File::open(&path) { let _ = file.take(65536).read_to_string(&mut text); }
                return Err(format!("{} failed ({status}); {}: {text}", tool.display(), path.display()));
            }
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error.to_string()); }
            Ok(None) => {}
        }
        if monitor.is_some_and(TaskMonitor::cancelled) {
            let _ = child.kill(); let _ = child.wait(); return Err("native compilation cancelled".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn module_path(monitor: Option<&TaskMonitor>) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("MCHPRS_LLVM_DLL") {
        return PathBuf::from(path).canonicalize().map_err(|error| format!("MCHPRS_LLVM_DLL: {error}"));
    }
    let cache = cache()?;
    let mut module = cache.module.lock().map_err(|_| "native cache lock poisoned".to_owned())?;
    if let Some(path) = module.as_ref().filter(|path| path.is_file()) { return Ok(path.clone()); }
    #[cfg(windows)] let clang = executable("clang.exe", "MCHPRS_LLVM_CLANG")?;
    #[cfg(not(windows))] let clang = executable("clang", "MCHPRS_LLVM_CLANG")?;
    #[cfg(windows)] let linker = if std::env::var_os("MCHPRS_LLVM_LINKER").is_some() {
        executable("lld-link.exe", "MCHPRS_LLVM_LINKER")?
    } else { clang.with_file_name("lld-link.exe").canonicalize().map_err(|e| e.to_string())? };
    #[cfg(not(windows))] let linker = executable("ld.lld", "MCHPRS_LLVM_LINKER")?;
    let sources: [(&str, &[u8]); 3] = [
        ("native_engine.c", include_bytes!("sampled/native_engine.c")),
        ("native_movement.c", include_bytes!("sampled/native_movement.c")),
        ("native_abi.h", include_bytes!("sampled/native_abi.h")),
    ];
    let flags = ["-std=c11", "-O3", "-ffreestanding", "-fno-builtin", "-Werror"];
    let mut hash = Sha256::new();
    for (name, data) in sources { hash.update(name); hash.update(data); }
    for flag in flags { hash.update(flag); }
    hash.update(std::env::consts::ARCH); hash.update(std::env::consts::OS);
    hash_file(&mut hash, &clang).map_err(|e| e.to_string())?;
    hash_file(&mut hash, &linker).map_err(|e| e.to_string())?;
    let key = format!("{:x}", hash.finalize());
    let directory = cache.path.join(&key);
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    for (name, data) in sources { fs::write(directory.join(name), data).map_err(|e| e.to_string())?; }
    let mut args: Vec<OsString> = flags.iter().map(OsString::from).collect();
    let started = Instant::now();
    #[cfg(windows)]
    let filename = {
        args.extend(["--target=x86_64-pc-windows-msvc", "-c", "native_engine.c", "-o", "engine.obj"].map(OsString::from));
        run_tool(&clang, &args, &directory, "compile.log", monitor)?;
        let args = ["/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/out:engine.pending.dll", "engine.obj"].map(OsString::from);
        run_tool(&linker, &args, &directory, "link.log", monitor)?;
        fs::rename(directory.join("engine.pending.dll"), directory.join("engine.dll")).map_err(|e| e.to_string())?;
        "engine.dll"
    };
    #[cfg(not(windows))]
    let filename = {
        args.extend(["-fPIC", "-shared", "native_engine.c", "-o", "engine.pending.so"].map(OsString::from));
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
    fs::write(directory.join("build.json"), serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    tracing::info!(path=%path.display(),build_ms=started.elapsed().as_secs_f64()*1000.0,"native module compiled");
    *module = Some(path.clone());
    Ok(path)
}
