//! Shared native exports, with platform-specific loading confined to the module handle.
use super::ScratchError;

#[cfg(any(windows, target_os = "linux"))]
use super::{ABI_VERSION, NativeApiDescriptor, NativePendingTick, NativeProgramDescriptor, cache, command, memory};
#[cfg(any(windows, target_os = "linux"))]
use std::{ffi::c_void, path::Path};

#[cfg(any(windows, target_os = "linux"))]
pub(super) struct Library {
    pub(super) api_descriptor: unsafe extern "C" fn() -> NativeApiDescriptor,
    pub(super) state_size: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32, u32) -> usize,
    pub(super) create: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32, *mut c_void, usize, u32) -> u32,
    pub(super) import_tick: unsafe extern "C" fn(*mut c_void, u32, u32, u8) -> u32,
    pub(super) destroy: unsafe extern "C" fn(*mut c_void),
    pub(super) tick: unsafe extern "C" fn(*mut c_void) -> u32,
    #[cfg(feature = "plot-bench")]
    pub(super) benchmark_tick_batch: Option<unsafe extern "C" fn(*mut c_void, u32, u32) -> u32>,
    pub(super) use_control: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    pub(super) set_input: unsafe extern "C" fn(*mut c_void, u32, u8) -> u32,
    pub(super) read_state: unsafe extern "C" fn(*const c_void, *mut u8, u32) -> u32,
    pub(super) read_components: unsafe extern "C" fn(*const c_void, *const u32, *mut u8, u32) -> u32,
    pub(super) note_count: unsafe extern "C" fn(*const c_void) -> u32,
    pub(super) read_notes: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
    pub(super) pending_count: unsafe extern "C" fn(*const c_void) -> u32,
    pub(super) read_pending: unsafe extern "C" fn(*const c_void, *mut NativePendingTick, u32) -> u32,
    pub(super) work_count: unsafe extern "C" fn(*const c_void) -> u64,
    pub(super) tick_count: unsafe extern "C" fn(*const c_void) -> u64,
    pub(super) snapshot_words: unsafe extern "C" fn(*const c_void) -> u32,
    pub(super) read_snapshot: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
    pub(super) ack_effects: unsafe extern "C" fn(*mut c_void),
    pub(super) commands: command::Api,
    _module: Module,
}

#[cfg(any(windows, target_os = "linux"))]
impl Library {
    /// Load the native executor and resolve its required API.
    ///
    /// Inputs: The selected cache artifact or explicit library path.
    ///
    /// Open the module under an owning handle, resolve every required export and return a library value
    /// whose drop keeps unload ownership well defined.
    pub(super) fn load() -> Result<Self, ScratchError> {
        let path = cache::module_path(None).map_err(|error| {
            tracing::warn!(%error, "LLVM module preparation failed");
            ScratchError {
                position: None,
                reason: "could not prepare LLVM module; see server log for details",
            }
        })?;
        let module = Module::open(&path)?;
        if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_some() {
            let module_bytes = std::fs::metadata(&*path).map_or(0, |file| file.len());
            eprintln!("LLVM module: path={}, bytes={module_bytes}", path.display());
        }
        // Module ownership also releases the handle if any required export is missing.
        // SAFETY: The trusted native exports match these field ABIs; all names end in NUL and _module keeps
        // their code loaded.
        unsafe {
            Ok(Self {
                api_descriptor: module.symbol(b"ne_api_descriptor\0")?,
                state_size: module.symbol(b"ne_state_size\0")?,
                create: module.symbol(b"ne_create\0")?,
                import_tick: module.symbol(b"ne_import_tick\0")?,
                destroy: module.symbol(b"ne_destroy\0")?,
                tick: module.symbol(b"ne_tick\0")?,
                #[cfg(feature = "plot-bench")]
                benchmark_tick_batch: module.symbol(b"ne_benchmark_tick_batch\0").ok(),
                use_control: module.symbol(b"ne_use_control\0")?,
                set_input: module.symbol(b"ne_set_input\0")?,
                read_state: module.symbol(b"ne_read_state\0")?,
                read_components: module.symbol(b"ne_read_components\0")?,
                note_count: module.symbol(b"ne_note_count\0")?,
                read_notes: module.symbol(b"ne_read_notes\0")?,
                pending_count: module.symbol(b"ne_pending_count\0")?,
                read_pending: module.symbol(b"ne_read_pending\0")?,
                work_count: module.symbol(b"ne_work_count\0")?,
                tick_count: module.symbol(b"ne_tick_count\0")?,
                snapshot_words: module.symbol(b"ne_snapshot_words\0")?,
                read_snapshot: module.symbol(b"ne_read_snapshot\0")?,
                ack_effects: module.symbol(b"ne_ack_effects\0")?,
                commands: command::Api::load(&module)?,
                _module: module,
            })
        }
    }

    pub(super) fn validate_abi(&self) -> Result<(), ScratchError> {
        // SAFETY: ne_api_descriptor takes no arguments and returns the repr(C) descriptor from abi.h; self owns
        // the module.
        let descriptor = unsafe { (self.api_descriptor)() };
        if descriptor.abi_version != ABI_VERSION || descriptor.descriptor_bytes != 16 || descriptor.semantic_version != 27 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM DLL ABI/version mismatch",
            });
        }
        Ok(())
    }

    /// Allocate and initialize native execution memory for an encoded program.
    ///
    /// Inputs: The native descriptor, aligned program blob and imported-tick count.
    ///
    /// Ask the library for required storage, enforce the size bound, allocate fallibly and invoke native
    /// initialization; reject any reported status before returning ownership.
    pub(super) fn create_state(&self, descriptor: &NativeProgramDescriptor, blob: &[u64], initial_tick_count: u32) -> Result<Vec<u64>, ScratchError> {
        if blob
            .len()
            .checked_mul(std::mem::size_of::<u64>())
            .is_none_or(|bytes| bytes < descriptor.total_bytes as usize)
        {
            return Err(ScratchError {
                position: None,
                reason: "invalid native Program descriptor",
            });
        }
        let blob_pointer = blob.as_ptr().cast();
        // SAFETY: The live repr(C) descriptor and u64-aligned blob cover total_bytes (checked above); C
        // validates contents before indexing.
        let bytes = unsafe { (self.state_size)(descriptor, blob_pointer, descriptor.total_bytes, initial_tick_count) };
        if bytes == 0 || bytes > 256 * 1024 * 1024 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM state size rejected",
            });
        }
        let mut state = memory::zeroed::<u64>(bytes.div_ceil(8))?;
        // SAFETY: The u64 storage covers the queried byte count; inputs stay live for this call and the caller
        // retains the blob afterward.
        let status = unsafe {
            (self.create)(
                descriptor,
                blob_pointer,
                descriptor.total_bytes,
                state.as_mut_ptr().cast(),
                state.len() * 8,
                initial_tick_count,
            )
        };
        if status != 0 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM rejected the sampled Program",
            });
        }
        Ok(state)
    }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn native_state_creation_rejects_a_truncated_blob_before_calling_c() {
        let library = Library::load().unwrap();
        let descriptor = NativeProgramDescriptor {
            total_bytes: 64,
            ..Default::default()
        };
        let error = library.create_state(&descriptor, &[], 0).unwrap_err();
        assert_eq!(error.reason, "invalid native Program descriptor");
    }

    #[test]
    fn missing_export_does_not_invalidate_the_owned_module() {
        let path = cache::module_path(None).unwrap();
        let module = Module::open(&path).unwrap();
        // SAFETY: This NUL-terminated name is absent in the generated module, so symbol returns Err without
        // constructing a pointer.
        let missing = unsafe { module.symbol::<unsafe extern "C" fn()>(b"ne_missing_export_for_test\0") };
        assert!(missing.is_err());
        // SAFETY: The NUL-terminated name and extern C signature match abi.h; module stays loaded through
        // export().
        let descriptor = unsafe {
            let export = module
                .symbol::<unsafe extern "C" fn() -> NativeApiDescriptor>(b"ne_api_descriptor\0")
                .unwrap();
            export()
        };
        assert_eq!(descriptor.abi_version, ABI_VERSION);
        drop(module);
        drop(path);
        Library::load().unwrap().validate_abi().unwrap();
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) struct Module {
    handle: *mut c_void,
}

// SAFETY: Windows/Linux loader references can be transferred and released on another thread. The
// generated module has no thread-affine state; ownership moves between workers and Module is not Sync.
#[cfg(any(windows, target_os = "linux"))]
unsafe impl Send for Module {}

#[cfg(any(windows, target_os = "linux"))]
impl Module {
    /// Open a native module with an owning OS handle.
    ///
    /// Inputs: The module filesystem path.
    fn open(path: &Path) -> Result<Self, ScratchError> {
        #[cfg(windows)]
        let handle = {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: wide is a live NUL-terminated UTF-16 buffer; the loaded module and its initializers are
            // trusted.
            unsafe { LoadLibraryW(wide.as_ptr()) }
        };
        #[cfg(target_os = "linux")]
        let handle = {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};
            let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ScratchError {
                position: None,
                reason: "invalid LLVM module path",
            })?;
            // SAFETY: CString supplies a live NUL-terminated path; 2 is RTLD_NOW, and the module and its
            // initializers are trusted.
            unsafe { dlopen(path.as_ptr(), 2) }
        };
        if handle.is_null() {
            #[cfg(windows)]
            let reason = "could not load LLVM DLL (set MCHPRS_LLVM_DLL)";
            #[cfg(target_os = "linux")]
            let reason = "could not load LLVM module";
            return Err(ScratchError { position: None, reason });
        }
        Ok(Self { handle })
    }

    /// Resolve a function exported by this module.
    ///
    /// # Safety
    /// `name` must be NUL-terminated, and `T` must match the export's exact function-pointer ABI.
    /// The returned pointer may only be called while this module remains loaded.
    ///
    /// Inputs: The owning module, a NUL-terminated export name and the caller-selected pointer type.
    pub(super) unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, ScratchError> {
        #[cfg(windows)]
        // SAFETY: Self owns a live loader handle; the caller guarantees name is NUL-terminated for the lookup.
        let address = unsafe { GetProcAddress(self.handle, name.as_ptr().cast()) };
        #[cfg(target_os = "linux")]
        // SAFETY: Self owns a live loader handle; the caller guarantees name is NUL-terminated for the lookup.
        let address = unsafe { dlsym(self.handle, name.as_ptr().cast()) };
        if address.is_null() {
            #[cfg(windows)]
            let reason = "LLVM DLL is missing a required export";
            #[cfg(target_os = "linux")]
            let reason = "LLVM module is missing a required export";
            return Err(ScratchError { position: None, reason });
        }
        // SAFETY: The address is non-null; the caller promises the exact function-pointer type, with loader-
        // address size on Windows/Linux.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl Drop for Module {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: The handle owns one loader reference, released exactly once here after the last use of its
        // exports.
        unsafe {
            FreeLibrary(self.handle);
        }
        #[cfg(target_os = "linux")]
        // SAFETY: The handle owns one loader reference, released exactly once here after the last use of its
        // exports.
        unsafe {
            dlclose(self.handle);
        }
    }
}

#[cfg(target_os = "linux")]
#[link(name = "dl")]
// SAFETY: These signatures match the Linux libdl ABI; callers provide live handles and C strings.
unsafe extern "C" {
    fn dlopen(filename: *const std::ffi::c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
// SAFETY: These signatures match the Win32 loader ABI, including the system calling convention.
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) struct Library;

#[cfg(not(any(windows, target_os = "linux")))]
impl Library {
    pub(super) fn load() -> Result<Self, ScratchError> {
        Err(ScratchError {
            position: None,
            reason: "LLVM runtime is supported on Windows and Linux only",
        })
    }
}
