//! Versioned, pointer-free input format for the experimental native runner.
//!
//! The payload is encoded field by field in little endian. Rust allocation layouts never cross
//! the DLL boundary, and future descriptor versions must not reinterpret existing blobs.
use super::{
    ScratchError,
    program::{Operation, Program},
};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_world::{TickEntry, TickPriority};
use std::ffi::{c_char, c_void};
mod movement;
mod cache;
use movement::encode_movement;

pub(super) const ABI_VERSION: u32 = 3;
const AUX_TABLES: usize = 33;
const MAGIC: [u8; 4] = *b"MCHN";
const HEADER_BYTES: usize = 16;
const OP_BYTES: usize = 8;
const LINK_BYTES: usize = 12;
const COMPONENT_BYTES: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativePendingTick {
    component: u32,
    ticks_left: u32,
    priority: u8,
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeApiDescriptor {
    pub(super) abi_version: u32,
    pub(super) descriptor_bytes: u32,
    pub(super) semantic_version: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeProgramDescriptor {
    pub(super) abi_version: u32,
    pub(super) header_bytes: u32,
    pub(super) total_bytes: u32,
    pub(super) operation_offset: u32,
    pub(super) operation_count: u32,
    pub(super) link_offset: u32,
    pub(super) link_count: u32,
    pub(super) component_offset: u32,
    pub(super) component_count: u32,
    pub(super) source_offset: u32,
    pub(super) source_count: u32,
    pub(super) arrangement_link_count: u32,
    pub(super) watched_wire_count: u32,
}

pub(super) struct NativeProgram {
    pub(super) descriptor: NativeProgramDescriptor,
    pub(super) blob: Vec<u8>,
}

#[cfg(any(windows, target_os = "linux"))]
pub(in crate::redpiler) struct NativeRuntime {
    library: Library,
    state: Vec<u64>,
    descriptor: NativeProgramDescriptor,
    blob: Vec<u8>,
    positions: Vec<BlockPos>,
    initial: Vec<Block>,
    sources: Vec<(BlockPos, u32)>,
}

#[cfg(any(windows, target_os = "linux"))]
impl NativeRuntime {
    pub(in crate::redpiler) fn new(program: Program, ticks: &[TickEntry]) -> Result<Self, ScratchError> {
        if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_some() {
            let mut kinds = std::collections::BTreeMap::<String, usize>::new();
            for block in &program.initial {
                let name = format!("{block:?}").split([' ', '{']).next().unwrap().to_owned();
                *kinds.entry(name).or_default() += 1;
            }
            eprintln!("LLVM Program component kinds: {kinds:?}");
        }
        if !supported(&program) {
            return Err(ScratchError {
                position: None,
                reason: "LLVM does not yet support this Program's component or movement features",
            });
        }
        if ticks.len() > program.component_count().checked_add(64).ok_or_else(overflow)? {
            return Err(ScratchError {
                position: None,
                reason: "LLVM initial scheduled tick capacity exceeded",
            });
        }
        let mut initial_ticks = Vec::with_capacity(ticks.len());
        for tick in ticks {
            let Some(&component) = program.component_indices.get(&tick.pos) else {
                return Err(ScratchError {
                    position: Some(tick.pos),
                    reason: "LLVM cannot import a tick outside its components",
                });
            };
            let block = program.initial[component];
            if !matches!(
                block,
                Block::StoneButton { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::Observer { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            ) || tick.block_type != Some(block.registry_id())
                || tick.ticks_left >= 32
                || (tick.ticks_left == 0 && tick.tick_priority != TickPriority::NanoTick)
            {
                return Err(ScratchError {
                    position: Some(tick.pos),
                    reason: "LLVM cannot import this scheduled tick",
                });
            }
            initial_ticks.push((
                u32::try_from(component).map_err(|_| overflow())?,
                tick.ticks_left,
                tick.tick_priority as u8,
            ));
        }
        let encoded = NativeProgram::encode(&program)?;
        if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_some() {
            let path = std::env::var_os("MCHPRS_LLVM_DLL")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native_engine.dll"));
            let dll_bytes = std::fs::metadata(path).map_or(0, |file| file.len());
            eprintln!(
                "LLVM native: per-plot generated C bytes=0, Program blob bytes={}, DLL bytes={dll_bytes}",
                encoded.blob.len()
            );
        }
        let library = Library::load()?;
        let api = unsafe { (library.api_descriptor)() };
        if api.abi_version != ABI_VERSION || api.descriptor_bytes != 16 || api.semantic_version != 8 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM DLL ABI/version mismatch",
            });
        }
        let bytes = unsafe { (library.state_size)(&encoded.descriptor, encoded.blob.as_ptr(), encoded.blob.len() as u32) };
        if bytes == 0 || bytes > 256 * 1024 * 1024 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM state size rejected",
            });
        }
        let mut state = vec![0u64; bytes.div_ceil(8)];
        let status = unsafe {
            (library.create)(
                &encoded.descriptor,
                encoded.blob.as_ptr(),
                encoded.blob.len() as u32,
                state.as_mut_ptr().cast(),
                state.len() * 8,
            )
        };
        if status != 0 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM rejected the sampled Program",
            });
        }
        for (component, delay, priority) in initial_ticks {
            if unsafe { (library.import_tick)(state.as_mut_ptr().cast(), component, delay, priority) } != 0 {
                unsafe {
                    (library.destroy)(state.as_mut_ptr().cast());
                }
                return Err(ScratchError {
                    position: None,
                    reason: "LLVM rejected an initial scheduled tick",
                });
            }
        }
        let sources = program
            .sources
            .iter()
            .enumerate()
            .map(|(index, &source)| (program.positions[source], index as u32))
            .collect();
        Ok(Self {
            library,
            state,
            descriptor: encoded.descriptor,
            blob: encoded.blob,
            positions: program.positions,
            initial: program.initial,
            sources,
        })
    }

    pub(in crate::redpiler) fn tick(&mut self) -> Result<(), ScratchError> {
        let status = unsafe { (self.library.tick)(self.state.as_mut_ptr().cast()) };
        self.call_status(status)
    }

    pub(in crate::redpiler) fn use_control(&mut self, pos: BlockPos) -> Result<(), ScratchError> {
        let Some((_, source)) = self.sources.iter().find(|(source_pos, _)| *source_pos == pos) else {
            return Err(ScratchError {
                position: Some(pos),
                reason: "LLVM control is not a supported input",
            });
        };
        let status = unsafe { (self.library.use_control)(self.state.as_mut_ptr().cast(), *source) };
        self.call_status(status)
    }

    pub(in crate::redpiler) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.sources.iter().any(|(source, _)| *source == pos)
    }

    pub(in crate::redpiler) fn publish<W: crate::world::World>(&mut self, world: &mut W, io_only: bool) -> Result<(), ScratchError> {
        let mut states = vec![0u8; self.positions.len()];
        let status = unsafe { (self.library.read_state)(self.state.as_ptr().cast(), states.as_mut_ptr(), states.len() as u32) };
        self.call_status(status)?;
        for ((&pos, &initial), state) in self.positions.iter().zip(&self.initial).zip(states) {
            if io_only && !matches!(initial, Block::Lever { .. } | Block::StoneButton { .. } | Block::RedstoneLamp { .. }) {
                continue;
            }
            let block = match initial {
                Block::Lever { mut lever } => {
                    lever.powered = state != 0;
                    Block::Lever { lever }
                }
                Block::StoneButton { mut button } => {
                    button.powered = state != 0;
                    Block::StoneButton { button }
                }
                Block::RedstoneLamp { .. } => Block::RedstoneLamp { lit: state != 0 },
                Block::RedstoneRepeater { mut repeater } => {
                    repeater.powered = state & 1 != 0;
                    repeater.locked = state & 2 != 0;
                    Block::RedstoneRepeater { repeater }
                }
                Block::RedstoneWire { mut wire } => {
                    wire.power = state;
                    Block::RedstoneWire { wire }
                }
                Block::Observer { mut observer } => {
                    observer.powered = state != 0;
                    Block::Observer { observer }
                }
                Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit: state != 0 },
                Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { facing, lit: state != 0 },
                other => other,
            };
            if world.get_block(pos) != block {
                world.set_block(pos, block);
            }
        }
        Ok(())
    }

    pub(in crate::redpiler) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
        let count = unsafe { (self.library.pending_count)(self.state.as_ptr().cast()) };
        if count > self.descriptor.component_count.saturating_mul(2).saturating_add(64) {
            return Err(ScratchError {
                position: None,
                reason: "LLVM pending tick count is invalid",
            });
        }
        let mut pending = vec![NativePendingTick::default(); count as usize];
        let status = unsafe { (self.library.read_pending)(self.state.as_ptr().cast(), pending.as_mut_ptr(), count) };
        self.call_status(status)?;
        pending
            .into_iter()
            .map(|entry| {
                let pos = *self.positions.get(entry.component as usize).ok_or(ScratchError {
                    position: None,
                    reason: "LLVM pending tick component is invalid",
                })?;
                let tick_priority = match entry.priority {
                    0 => TickPriority::Highest,
                    1 => TickPriority::Higher,
                    2 => TickPriority::High,
                    3 => TickPriority::Normal,
                    _ => {
                        return Err(ScratchError {
                            position: Some(pos),
                            reason: "LLVM pending tick priority is invalid",
                        });
                    }
                };
                Ok(TickEntry {
                    ticks_left: entry.ticks_left,
                    tick_priority,
                    pos,
                    block_type: Some(self.initial[entry.component as usize].registry_id()),
                })
            })
            .collect()
    }

    fn call_status(&self, status: u32) -> Result<(), ScratchError> {
        if status == 0 {
            Ok(())
        } else {
            Err(ScratchError {
                position: None,
                reason: "LLVM executor rejected operation or exceeded its operation limit",
            })
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl Drop for NativeRuntime {
    fn drop(&mut self) {
        if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_some() {
            let count = unsafe { (self.library.work_count)(self.state.as_ptr().cast()) };
            eprintln!("LLVM native: executed operations and scheduled callbacks={count}");
        }
        unsafe {
            (self.library.destroy)(self.state.as_mut_ptr().cast());
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(in crate::redpiler) struct NativeRuntime;

#[cfg(not(any(windows, target_os = "linux")))]
impl NativeRuntime {
    pub(in crate::redpiler) fn new(_: Program, _: &[TickEntry]) -> Result<Self, ScratchError> {
        Library::load().map(|_| Self)
    }
    pub(in crate::redpiler) fn sync_projection(&self, _: &mut super::Runtime) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn tick(&mut self) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn use_control(&mut self, _: BlockPos) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn accepts_control(&self, _: BlockPos) -> bool {
        false
    }
    pub(in crate::redpiler) fn publish<W: crate::world::World>(&mut self, _: &mut W, _: bool) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
        unreachable!()
    }
}

fn supported(program: &Program) -> bool {
    program.routing.is_none()
        && !program.has_reactive
        && program.command_blocks.is_empty()
        && program.command_output_state.is_none()
        && program.initial.iter().all(|block| {
            matches!(
                block,
                Block::Lever { .. }
                    | Block::StoneButton { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::RedstoneBlock
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneWire { .. }
                    | Block::Observer { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::Piston { .. }
            )
        })
        && program.links.iter().all(|link| {
            matches!(
                program.initial[link.source],
                Block::Lever { .. }
                    | Block::StoneButton { .. }
                    | Block::RedstoneBlock
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneWire { .. }
                    | Block::Observer { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::Piston { .. }
            ) && matches!(
                program.initial[link.target],
                Block::RedstoneLamp { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneWire { .. }
                    | Block::Observer { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::Piston { .. }
            )
        })
        && program.operations.iter().all(|op| {
            matches!(
                op,
                Operation::Contribution(_)
                    | Operation::UpdateLamp(_)
                    | Operation::UpdateRepeater(_)
                    | Operation::RefreshWire(_)
                    | Operation::CheckWatchedWire(_)
                    | Operation::UpdateTorch(_)
                    | Operation::ArrangementContribution(_)
                    | Operation::UpdatePiston(_)
            )
        })
}

#[cfg(windows)]
struct Library {
    handle: *mut c_void,
    api_descriptor: unsafe extern "C" fn() -> NativeApiDescriptor,
    state_size: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32) -> usize,
    create: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32, *mut c_void, usize) -> u32,
    import_tick: unsafe extern "C" fn(*mut c_void, u32, u32, u8) -> u32,
    destroy: unsafe extern "C" fn(*mut c_void),
    tick: unsafe extern "C" fn(*mut c_void) -> u32,
    use_control: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    read_state: unsafe extern "C" fn(*const c_void, *mut u8, u32) -> u32,
    pending_count: unsafe extern "C" fn(*const c_void) -> u32,
    read_pending: unsafe extern "C" fn(*const c_void, *mut NativePendingTick, u32) -> u32,
    work_count: unsafe extern "C" fn(*const c_void) -> u64,
    snapshot_words: unsafe extern "C" fn(*const c_void) -> u32,
    read_snapshot: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
}

// The compiler moves its exclusive native runtime between the compile and tick workers.
#[cfg(windows)]
unsafe impl Send for Library {}

#[cfg(windows)]
impl Library {
    fn load() -> Result<Self, ScratchError> {
        let path = std::env::var_os("MCHPRS_LLVM_DLL")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native_engine.dll"));
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            return Err(ScratchError {
                position: None,
                reason: "could not load LLVM DLL (set MCHPRS_LLVM_DLL)",
            });
        }
        unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &'static [u8]) -> Result<T, ScratchError> {
            let address = unsafe { GetProcAddress(handle, name.as_ptr().cast()) };
            if address.is_null() {
                return Err(ScratchError {
                    position: None,
                    reason: "LLVM DLL is missing a required export",
                });
            }
            Ok(unsafe { std::mem::transmute_copy(&address) })
        }
        let result = (|| unsafe {
            Ok(Self {
                handle,
                api_descriptor: symbol(handle, b"ne_api_descriptor\0")?,
                state_size: symbol(handle, b"ne_state_size\0")?,
                create: symbol(handle, b"ne_create\0")?,
                import_tick: symbol(handle, b"ne_import_tick\0")?,
                destroy: symbol(handle, b"ne_destroy\0")?,
                tick: symbol(handle, b"ne_tick\0")?,
                use_control: symbol(handle, b"ne_use_control\0")?,
                read_state: symbol(handle, b"ne_read_state\0")?,
                pending_count: symbol(handle, b"ne_pending_count\0")?,
                read_pending: symbol(handle, b"ne_read_pending\0")?,
                work_count: symbol(handle, b"ne_work_count\0")?,
                snapshot_words: symbol(handle, b"ne_snapshot_words\0")?,
                read_snapshot: symbol(handle, b"ne_read_snapshot\0")?,
            })
        })();
        if result.is_err() {
            unsafe {
                FreeLibrary(handle);
            }
        }
        result
    }
}

#[cfg(windows)]
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.handle);
        }
    }
}

#[cfg(target_os = "linux")]
struct Library {
    handle: *mut c_void,
    api_descriptor: unsafe extern "C" fn() -> NativeApiDescriptor,
    state_size: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32) -> usize,
    create: unsafe extern "C" fn(*const NativeProgramDescriptor, *const u8, u32, *mut c_void, usize) -> u32,
    import_tick: unsafe extern "C" fn(*mut c_void, u32, u32, u8) -> u32,
    destroy: unsafe extern "C" fn(*mut c_void),
    tick: unsafe extern "C" fn(*mut c_void) -> u32,
    use_control: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    read_state: unsafe extern "C" fn(*const c_void, *mut u8, u32) -> u32,
    pending_count: unsafe extern "C" fn(*const c_void) -> u32,
    read_pending: unsafe extern "C" fn(*const c_void, *mut NativePendingTick, u32) -> u32,
    work_count: unsafe extern "C" fn(*const c_void) -> u64,
    snapshot_words: unsafe extern "C" fn(*const c_void) -> u32,
    read_snapshot: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
}

#[cfg(target_os = "linux")]
unsafe impl Send for Library {}

#[cfg(target_os = "linux")]
impl Library {
    fn load() -> Result<Self, ScratchError> {
        use std::ffi::CString;
        let path = cache::module_path(None).map_err(|_| ScratchError { position: None, reason: "could not build LLVM native module" })?;
        let path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| ScratchError { position: None, reason: "invalid LLVM module path" })?;
        let handle = unsafe { dlopen(path.as_ptr(), 2) };
        if handle.is_null() { return Err(ScratchError { position: None, reason: "could not load LLVM native module" }); }
        unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &'static [u8]) -> Result<T, ScratchError> {
            let address = unsafe { dlsym(handle, name.as_ptr().cast()) };
            if address.is_null() { return Err(ScratchError { position: None, reason: "LLVM native module is missing a required export" }); }
            Ok(unsafe { std::mem::transmute_copy(&address) })
        }
        let result = (|| unsafe {
            Ok(Self {
                handle, api_descriptor: symbol(handle, b"ne_api_descriptor\0")?, state_size: symbol(handle, b"ne_state_size\0")?,
                create: symbol(handle, b"ne_create\0")?, import_tick: symbol(handle, b"ne_import_tick\0")?,
                destroy: symbol(handle, b"ne_destroy\0")?, tick: symbol(handle, b"ne_tick\0")?,
                use_control: symbol(handle, b"ne_use_control\0")?, read_state: symbol(handle, b"ne_read_state\0")?,
                pending_count: symbol(handle, b"ne_pending_count\0")?, read_pending: symbol(handle, b"ne_read_pending\0")?,
                work_count: symbol(handle, b"ne_work_count\0")?, snapshot_words: symbol(handle, b"ne_snapshot_words\0")?,
                read_snapshot: symbol(handle, b"ne_read_snapshot\0")?,
            })
        })();
        if result.is_err() { unsafe { dlclose(handle); } }
        result
    }
}

#[cfg(target_os = "linux")]
impl Drop for Library { fn drop(&mut self) { unsafe { dlclose(self.handle); } } }

#[cfg(target_os = "linux")]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const std::ffi::c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}

#[cfg(not(any(windows, target_os = "linux")))]
struct Library;

#[cfg(not(any(windows, target_os = "linux")))]
impl Library {
    fn load() -> Result<Self, ScratchError> {
        Err(ScratchError {
            position: None,
            reason: "LLVM native runtime is supported on Windows and Linux only",
        })
    }
}

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

impl NativeProgram {
    pub(super) fn encode(program: &Program) -> Result<Self, ScratchError> {
        let mut blob = Vec::with_capacity(
            HEADER_BYTES
                .checked_add(program.operations.len().checked_mul(OP_BYTES).ok_or_else(overflow)?)
                .and_then(|size| size.checked_add(program.links.len().checked_mul(LINK_BYTES)?))
                .and_then(|size| size.checked_add(program.positions.len().checked_mul(COMPONENT_BYTES)?))
                .and_then(|size| size.checked_add(program.sources.len().checked_mul(4)?))
                .ok_or_else(overflow)?,
        );
        blob.extend_from_slice(&MAGIC);
        put_u32(&mut blob, ABI_VERSION)?;
        put_u32(&mut blob, u32::try_from(HEADER_BYTES).map_err(|_| overflow())?)?;
        // Filled after table serialization.
        put_u32(&mut blob, 0)?;
        let operation_offset = u32::try_from(blob.len()).map_err(|_| overflow())?;
        for operation in &program.operations {
            let (opcode, operand) = encode_operation(*operation)?;
            blob.push(opcode);
            blob.extend_from_slice(&[0; 3]);
            put_u32(&mut blob, operand)?;
        }
        let link_offset = u32::try_from(blob.len()).map_err(|_| overflow())?;
        for (index, link) in program.links.iter().enumerate() {
            put_u32(&mut blob, u32::try_from(link.source).map_err(|_| overflow())?)?;
            put_u32(&mut blob, u32::try_from(link.target).map_err(|_| overflow())?)?;
            blob.push(link.attenuation);
            blob.push(u8::from(program.effective_links[index]));
            blob.extend_from_slice(&[0; 2]);
        }
        let component_offset = u32::try_from(blob.len()).map_err(|_| overflow())?;
        for (index, (&position, block)) in program.positions.iter().zip(&program.initial).enumerate() {
            blob.extend_from_slice(&position.x.to_le_bytes());
            blob.extend_from_slice(&position.y.to_le_bytes());
            blob.extend_from_slice(&position.z.to_le_bytes());
            let (kind, state) = component_state(*block);
            let (delay, flags) = if let Block::RedstoneRepeater { repeater } = block {
                let diode = program.repeater_by_component[index].is_some_and(|record| program.repeaters[record.get() - 1].output_is_diode);
                let dynamic_lock =
                    program.repeater_by_component[index].is_some_and(|record| program.repeaters[record.get() - 1].side_sources.is_some());
                (repeater.delay, (u8::from(diode) << 1) | (u8::from(dynamic_lock) << 2))
            } else {
                (0, 0)
            };
            blob.extend_from_slice(&[kind, state, delay, flags]);
            for range in [&program.output_programs[index][1], &program.output_programs[index][0]] {
                put_u32(&mut blob, u32::try_from(range.start).map_err(|_| overflow())?)?;
                put_u32(&mut blob, u32::try_from(range.len()).map_err(|_| overflow())?)?;
            }
        }
        let source_offset = u32::try_from(blob.len()).map_err(|_| overflow())?;
        for &source in &program.sources {
            put_u32(&mut blob, u32::try_from(source).map_err(|_| overflow())?)?;
        }
        encode_aux(program, &mut blob)?;
        let total_bytes = u32::try_from(blob.len()).map_err(|_| overflow())?;
        blob[12..16].copy_from_slice(&total_bytes.to_le_bytes());
        let descriptor = NativeProgramDescriptor {
            abi_version: ABI_VERSION,
            header_bytes: HEADER_BYTES as u32,
            total_bytes,
            operation_offset,
            operation_count: u32::try_from(program.operations.len()).map_err(|_| overflow())?,
            link_offset,
            link_count: u32::try_from(program.links.len()).map_err(|_| overflow())?,
            component_offset,
            component_count: u32::try_from(program.positions.len()).map_err(|_| overflow())?,
            source_offset,
            source_count: u32::try_from(program.sources.len()).map_err(|_| overflow())?,
            arrangement_link_count: u32::try_from(program.arrangement_links.len()).map_err(|_| overflow())?,
            watched_wire_count: u32::try_from(program.watched_wires.len()).map_err(|_| overflow())?,
        };
        validate_blob(&descriptor, &blob)?;
        Ok(Self { descriptor, blob })
    }
}

fn encode_aux(program: &Program, blob: &mut Vec<u8>) -> Result<(), ScratchError> {
    let mut tables: [Vec<u32>; AUX_TABLES] = std::array::from_fn(|_| Vec::new());
    let id = |n: usize| u32::try_from(n).map_err(|_| overflow());
    for row in &program.side_outputs {
        let start = id(tables[1].len() / 2)?;
        tables[0].extend([start, id(row.len())?]);
        for &(target, attenuation) in row {
            tables[1].extend([id(target)?, u32::from(attenuation)]);
        }
    }
    for row in &program.source_watchers {
        let start = id(tables[4].len())?;
        tables[2].extend([start, id(row.len())?]);
        for &node in row {
            tables[4].push(id(node)?);
        }
    }
    for record in &program.watched_wires {
        let start = id(tables[4].len())?;
        tables[3].extend([id(record.wire)?, start, id(record.watchers.len())?]);
        for &node in &record.watchers {
            tables[4].push(id(node)?);
        }
    }
    for row in &program.outgoing {
        let start = id(tables[6].len())?;
        tables[5].extend([start, id(row.len())?]);
        for &link in row {
            tables[6].push(id(link)?);
        }
    }
    encode_movement(program, &mut tables)?;
    let directory = blob.len();
    blob.resize(directory.checked_add(AUX_TABLES * 8).ok_or_else(overflow)?, 0);
    for (index, table) in tables.iter().enumerate() {
        let offset = id(blob.len())?;
        let count = id(table.len())?;
        blob[directory + index * 8..directory + index * 8 + 4].copy_from_slice(&offset.to_le_bytes());
        blob[directory + index * 8 + 4..directory + index * 8 + 8].copy_from_slice(&count.to_le_bytes());
        for &value in table {
            put_u32(blob, value)?;
        }
    }
    Ok(())
}

pub(super) fn validate_blob(descriptor: &NativeProgramDescriptor, blob: &[u8]) -> Result<(), ScratchError> {
    let invalid = || ScratchError {
        position: None,
        reason: "invalid native Program descriptor",
    };
    if descriptor.abi_version != ABI_VERSION
        || descriptor.header_bytes as usize != HEADER_BYTES
        || descriptor.total_bytes as usize != blob.len()
        || blob.get(..4) != Some(&MAGIC)
        || read_u32(blob, 4) != Some(ABI_VERSION)
        || read_u32(blob, 8) != Some(descriptor.header_bytes)
        || read_u32(blob, 12) != Some(descriptor.total_bytes)
    {
        return Err(invalid());
    }
    let operations_end = (descriptor.operation_count as usize)
        .checked_mul(OP_BYTES)
        .and_then(|bytes| (descriptor.operation_offset as usize).checked_add(bytes))
        .ok_or_else(invalid)?;
    let links_end = (descriptor.link_count as usize)
        .checked_mul(LINK_BYTES)
        .and_then(|bytes| (descriptor.link_offset as usize).checked_add(bytes))
        .ok_or_else(invalid)?;
    let components_end = (descriptor.component_count as usize)
        .checked_mul(COMPONENT_BYTES)
        .and_then(|bytes| (descriptor.component_offset as usize).checked_add(bytes))
        .ok_or_else(invalid)?;
    let sources_end = (descriptor.source_count as usize)
        .checked_mul(4)
        .and_then(|bytes| (descriptor.source_offset as usize).checked_add(bytes))
        .ok_or_else(invalid)?;
    if descriptor.operation_offset as usize != HEADER_BYTES || operations_end != descriptor.link_offset as usize || links_end > blob.len() {
        return Err(invalid());
    }
    for operation in blob[descriptor.operation_offset as usize..operations_end].chunks_exact(OP_BYTES) {
        let operand = u32::from_le_bytes(operation[4..8].try_into().unwrap());
        let limit = match operation[0] {
            0 => descriptor.link_count,
            1 => descriptor.arrangement_link_count,
            2 | 4..=13 => descriptor.component_count,
            3 => descriptor.watched_wire_count,
            _ => return Err(invalid()),
        };
        if operation[1..4] != [0, 0, 0] || operand >= limit {
            return Err(invalid());
        }
    }
    for link in blob[descriptor.link_offset as usize..links_end].chunks_exact(LINK_BYTES) {
        let source = u32::from_le_bytes(link[0..4].try_into().unwrap());
        let target = u32::from_le_bytes(link[4..8].try_into().unwrap());
        if source >= descriptor.component_count || target >= descriptor.component_count || link[9] > 1 || link[10] != 0 || link[11] != 0 {
            return Err(invalid());
        }
    }
    if descriptor.component_offset as usize != links_end
        || components_end != descriptor.source_offset as usize
        || sources_end.checked_add(AUX_TABLES * 8).is_none_or(|end| end > blob.len())
    {
        return Err(invalid());
    }
    for component in blob[descriptor.component_offset as usize..components_end].chunks_exact(COMPONENT_BYTES) {
        let kind = component[12];
        let on_start = u32::from_le_bytes(component[16..20].try_into().unwrap()) as usize;
        let on_count = u32::from_le_bytes(component[20..24].try_into().unwrap()) as usize;
        let off_start = u32::from_le_bytes(component[24..28].try_into().unwrap()) as usize;
        let off_count = u32::from_le_bytes(component[28..32].try_into().unwrap()) as usize;
        if (kind > 8 && kind != u8::MAX)
            || component[13]
                > (if kind == 5 {
                    15
                } else if kind == 4 {
                    3
                } else {
                    1
                })
            || if kind == 4 {
                !(1..=4).contains(&component[14]) || component[15] > 6 || component[15] & 1 != 0
            } else {
                component[14] != 0 || component[15] != 0
            }
            || on_start.checked_add(on_count).is_none_or(|end| end > descriptor.operation_count as usize)
            || off_start
                .checked_add(off_count)
                .is_none_or(|end| end > descriptor.operation_count as usize)
        {
            return Err(invalid());
        }
    }
    for source in blob[descriptor.source_offset as usize..sources_end].chunks_exact(4) {
        let source = u32::from_le_bytes(source.try_into().unwrap());
        if source >= descriptor.component_count {
            return Err(invalid());
        }
    }
    let mut tables = Vec::with_capacity(AUX_TABLES);
    let mut cursor = sources_end + AUX_TABLES * 8;
    for index in 0..AUX_TABLES {
        let offset = read_u32(blob, sources_end + index * 8).ok_or_else(invalid)? as usize;
        let words = read_u32(blob, sources_end + index * 8 + 4).ok_or_else(invalid)? as usize;
        let end = words.checked_mul(4).and_then(|bytes| offset.checked_add(bytes)).ok_or_else(invalid)?;
        if offset != cursor || end > blob.len() {
            return Err(invalid());
        }
        tables.push(
            blob[offset..end]
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>(),
        );
        cursor = end;
    }
    if cursor != blob.len()
        || tables[0].len() != descriptor.component_count as usize * 2
        || tables[2].len() != descriptor.component_count as usize * 2
        || tables[5].len() != descriptor.component_count as usize * 2
        || tables[1].len() % 2 != 0
        || tables[3].len() != descriptor.watched_wire_count as usize * 3
    {
        return Err(invalid());
    }
    for (rows, limit) in [(0, tables[1].len() / 2), (2, tables[4].len()), (5, tables[6].len())] {
        for row in tables[rows].chunks_exact(2) {
            if u64::from(row[0]) + u64::from(row[1]) > limit as u64 {
                return Err(invalid());
            }
        }
    }
    for row in tables[1].chunks_exact(2) {
        if row[0] >= descriptor.component_count || row[1] > 15 {
            return Err(invalid());
        }
    }
    for row in tables[3].chunks_exact(3) {
        if row[0] >= descriptor.component_count || u64::from(row[1]) + u64::from(row[2]) > tables[4].len() as u64 {
            return Err(invalid());
        }
    }
    if tables[4].iter().any(|&node| node >= descriptor.component_count) || tables[6].iter().any(|&link| link >= descriptor.link_count) {
        return Err(invalid());
    }
    Ok(())
}

fn encode_operation(operation: Operation) -> Result<(u8, u32), ScratchError> {
    let (tag, operand) = match operation {
        Operation::Contribution(i) => (0, i),
        Operation::ArrangementContribution(i) => (1, i),
        Operation::RefreshWire(i) => (2, i),
        Operation::CheckWatchedWire(i) => (3, i),
        Operation::UpdateLamp(i) => (4, i),
        Operation::UpdateHopper(i) => (5, i),
        Operation::UpdateTrapdoor(i) => (6, i),
        Operation::UpdateCommandBlock(i) => (7, i),
        Operation::UpdateNoteBlock(i) => (8, i),
        Operation::UpdateCopperBulb(i) => (9, i),
        Operation::UpdateRepeater(i) => (10, i),
        Operation::UpdatePiston(i) => (11, i),
        Operation::UpdateTorch(i) => (12, i),
        Operation::UpdateComparator(i) => (13, i),
    };
    u32::try_from(operand).map(|operand| (tag, operand)).map_err(|_| overflow())
}

fn component_state(block: Block) -> (u8, u8) {
    match block {
        Block::Lever { lever } => (0, u8::from(lever.powered)),
        Block::StoneButton { button } => (1, u8::from(button.powered)),
        Block::RedstoneLamp { lit } => (2, u8::from(lit)),
        Block::RedstoneBlock => (3, 0),
        Block::RedstoneRepeater { repeater } => (4, u8::from(repeater.powered) | (u8::from(repeater.locked) << 1)),
        Block::RedstoneWire { wire } => (5, wire.power),
        Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => (6, u8::from(lit)),
        Block::Observer { observer } => (7, u8::from(observer.powered)),
        Block::Piston { piston } => (8, u8::from(piston.extended)),
        _ => (u8::MAX, 0),
    }
}

fn put_u32(out: &mut Vec<u8>, value: u32) -> Result<(), ScratchError> {
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn overflow() -> ScratchError {
    ScratchError {
        position: None,
        reason: "native Program exceeds ABI limits",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn compare_execution(mut program: Program, controls: &[(usize, usize)], ticks: usize) {
        if program.projection_positions.is_empty() || program.effective_links.len() != program.links.len() {
            program.prepare_projection();
        }
        let positions = program.positions.clone();
        let mut projection = program.clone().into_runtime_prepared();
        let mut rust = program.clone().into_runtime_prepared();
        rust.set_pending(Vec::new()).unwrap();
        let mut native = NativeRuntime::new(program, &[]).unwrap();
        for tick in 0..ticks {
            for &(_, source) in controls.iter().filter(|&&(at, _)| at == tick) {
                rust.use_control(positions[source]).unwrap();
                native.use_control(positions[source]).unwrap();
            }
            rust.tick().unwrap();
            native.tick().unwrap();
            let mut states = vec![0u8; positions.len()];
            assert_eq!(
                unsafe { (native.library.read_state)(native.state.as_ptr().cast(), states.as_mut_ptr(), states.len() as u32) },
                0
            );
            let expected: Vec<_> = rust.blocks.iter().map(|block| component_state(*block).1).collect();
            assert_eq!(states, expected, "state tick {tick}");
            assert_eq!(native.pending_entries().unwrap(), rust.pending_entries(), "scheduler tick {tick}");
            native.sync_projection(&mut projection).unwrap();
            assert_eq!(projection.projected_blocks(), rust.projected_blocks(), "projection tick {tick}");
            assert_eq!(projection.token_positions, rust.token_positions, "tokens tick {tick}");
            assert_eq!(projection.piston_extended, rust.piston_extended, "pistons tick {tick}");
            assert_eq!(projection.piston_head_present, rust.piston_head_present, "heads tick {tick}");
            assert_eq!(projection.piston_head_moving, rust.piston_head_moving, "moving heads tick {tick}");
            assert_eq!(projection.piston_base_moving, rust.piston_base_moving, "moving bases tick {tick}");
            assert_eq!(projection.piston_events, rust.piston_events, "piston events tick {tick}");
            assert_eq!(projection.contributions, rust.contributions, "ordinary contributions tick {tick}");
            assert_eq!(
                projection.arrangement_contributions, rust.arrangement_contributions,
                "arrangement contributions tick {tick}"
            );
            assert_eq!(projection.group_arrangements, rust.group_arrangements, "arrangements tick {tick}");
            assert_eq!(projection.watched_strengths, rust.watched_strengths, "watched wires tick {tick}");
            for (node, (a, b)) in projection.input_levels.iter().zip(&rust.input_levels).enumerate() {
                assert_eq!(
                    (a[0].active, a[1].active),
                    (b[0].active, b[1].active),
                    "input counts tick {tick} node {node}"
                );
            }
            let motions = |r: &super::super::Runtime| {
                r.active_motions
                    .iter()
                    .map(|&slot| {
                        let m = r.motions[slot];
                        (
                            m.piston,
                            m.token,
                            m.destination,
                            m.start,
                            m.landing,
                            m.order,
                            m.extending,
                            m.piston_done,
                            m.token_done,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(motions(&projection), motions(&rust), "motions tick {tick}");
            if positions.len() > 1000 && tick % 256 == 0 {
                eprintln!("LLVM shadow matched tick {tick}");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "full ZCore native/Rust snapshot comparison; requires the LLVM DLL"]
    fn llvm_zcore_matches_rust_state_and_motion_each_tick() {
        mod cpus {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/test_support/cpus.rs"));
        }
        use crate::{plot::worldedit::load_schematic, world::World};
        let cpu = cpus::CPUS[3];
        let mut world = cpus::load_cpu(cpu);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data")
            .join(cpu.schematic);
        let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
        let bounds = (
            cpu.origin,
            cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
        );
        let program = super::super::compile_program(
            &world,
            bounds,
            super::super::ScratchEntry {
                scheduled: vec![],
                piston: world.piston_state().clone(),
            },
            None,
        )
        .unwrap();
        let start = program.positions.iter().position(|&pos| pos == cpu.origin + cpu.start).unwrap();
        let run = program
            .positions
            .iter()
            .position(|&pos| pos == cpu.origin + BlockPos::new(207, 10, 154))
            .unwrap();
        cpus::click_cpu(&mut world, cpu, cpu.start);
        let mut idle = 0;
        let mut preparation = 0;
        for tick in 1..=4096 {
            world.tick_interpreted();
            let quiet = world.scheduler().iter_entries().next().is_none()
                && world.piston_state().events.is_empty()
                && world.piston_state().motions.is_empty();
            idle = if quiet { idle + 1 } else { 0 };
            if idle == 20 {
                preparation = tick;
                break;
            }
        }
        assert!(preparation > 0);
        let ticks = std::env::var("MCHPRS_LLVM_VERIFY_TICKS").ok().map_or(4096, |s| s.parse().unwrap());
        compare_execution(program, &[(0, start), (preparation, run)], preparation + ticks);
    }

    #[cfg(windows)]
    #[test]
    fn llvm_wire_strengths_torches_and_observer_notifications_match_rust() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        use super::super::program::{ElectricalLink, WatchedWire};
        use mchprs_blocks::blocks::RedstoneWire;
        let initial = vec![
            Block::from_name("lever").unwrap(),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
            Block::from_name("lever").unwrap(),
            Block::RedstoneLamp { lit: false },
            Block::RedstoneTorch { lit: true },
            Block::from_name("observer").unwrap(),
        ];
        let links = vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 2,
                target: 1,
                attenuation: 2,
            },
            ElectricalLink {
                source: 1,
                target: 3,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 4,
                attenuation: 0,
            },
        ];
        let mut edges = vec![[Vec::new(), Vec::new()]; 6];
        for (source, link) in [(0, 0), (2, 1)] {
            let ops = vec![
                Operation::Contribution(link),
                Operation::RefreshWire(1),
                Operation::CheckWatchedWire(0),
                Operation::UpdateLamp(3),
                Operation::UpdateTorch(4),
            ];
            edges[source] = [ops.clone(), ops];
        }
        let mut program = Program::new(
            (0..6).map(|x| BlockPos::new(x, 15, 15)).collect(),
            initial,
            vec![0, 2],
            edges,
            links,
            vec![WatchedWire { wire: 1, watchers: vec![5] }],
            Vec::new(),
        )
        .unwrap();
        program.source_watchers[0] = vec![5];
        program.source_watchers[4] = vec![5];
        compare_execution(program, &[(0, 0), (4, 2), (8, 0), (12, 2), (25, 2), (26, 2), (38, 0), (39, 0)], 96);
    }

    #[cfg(windows)]
    #[test]
    fn llvm_repeater_side_locks_and_lock_observers_match_rust() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        use super::super::program::{ElectricalLink, RepeaterRecord};
        use mchprs_blocks::blocks::RedstoneRepeater;
        let initial = vec![
            Block::from_name("lever").unwrap(),
            Block::from_name("lever").unwrap(),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater::default(),
            },
            Block::from_name("observer").unwrap(),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater::default(),
            },
        ];
        let a = vec![Operation::Contribution(0), Operation::UpdateRepeater(2)];
        let b = vec![Operation::Contribution(1), Operation::UpdateRepeater(4)];
        let c = vec![Operation::UpdateRepeater(2)];
        let mut program = Program::new(
            (0..5).map(|x| BlockPos::new(x, 15, 15)).collect(),
            initial,
            vec![0, 1],
            vec![[a.clone(), a], [b.clone(), b], [vec![], vec![]], [vec![], vec![]], [c.clone(), c]],
            vec![
                ElectricalLink {
                    source: 0,
                    target: 2,
                    attenuation: 0,
                },
                ElectricalLink {
                    source: 1,
                    target: 4,
                    attenuation: 0,
                },
            ],
            Vec::new(),
            vec![
                RepeaterRecord {
                    component: 2,
                    output_is_diode: false,
                    side_sources: Some(vec![4]),
                },
                RepeaterRecord {
                    component: 4,
                    output_is_diode: false,
                    side_sources: None,
                },
            ],
        )
        .unwrap();
        program.source_watchers[2] = vec![3];
        compare_execution(program, &[(0, 1), (4, 0), (8, 1), (13, 1), (17, 0), (21, 1), (32, 0), (34, 0)], 96);
    }

    #[cfg(windows)]
    #[test]
    fn llvm_compressed_concrete_axes_preserve_motion_and_wire_state() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, blocks::{Lever, LeverFace, RedstonePiston}};
        let mut world = super::super::tests::host();
        let base = BlockPos::new(15, 15, 15);
        let lever = base.offset(BlockFace::East);
        let upper = BlockPos::new(15, 14, 15);
        let lower = BlockPos::new(15, 13, 15);
        world.set_block(lever, Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) });
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(base, Block::Piston { piston: RedstonePiston { facing: BlockFacing::Down, sticky: true, extended: false } });
        world.set_block(upper, Block::from_name("black_concrete").unwrap());
        world.set_block(lower.offset(BlockFace::Bottom), Block::from_name("redstone_wire").unwrap());
        world.set_block(BlockPos::new(16, 12, 15), Block::Stone {});
        world.set_block(BlockPos::new(16, 13, 15), Block::from_name("redstone_wire").unwrap());
        world.set_block(BlockPos::new(14, 12, 15), Block::RedstoneBlock);
        for pos in [lower.offset(BlockFace::Bottom), BlockPos::new(16, 13, 15)] {
            let wire = crate::redstone::wire::get_state_for_placement(&world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        let program = super::super::compile_program(&world, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        assert!(program.movement_groups.iter().flat_map(|group| &group.axes).any(|axis| axis.states.len() == 3 && axis.states.iter().max() == Some(&1)));
        let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
        compare_execution(program, &[(0, source), (6, source), (12, source), (13, source), (20, source), (28, source)], 40);
    }

    #[cfg(windows)]
    #[test]
    fn llvm_empty_head_notifies_quasi_powered_piston_immediately() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, blocks::{Lever, LeverFace, RedstonePiston}};
        let mut world = super::super::tests::host();
        let driver = BlockPos::new(10, 10, 10);
        let bud = BlockPos::new(9, 10, 9);
        let lever = BlockPos::new(10, 10, 11);
        world.set_block(lever, Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) });
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        for (pos, facing) in [(driver, BlockFacing::North), (bud, BlockFacing::Down)] {
            world.set_block(pos, Block::Piston { piston: RedstonePiston { facing, sticky: true, extended: false } });
        }
        world.set_block(bud + BlockPos::new(0, 2, 0), Block::RedstoneBlock);
        let program = super::super::compile_program(&world, (BlockPos::new(5, 5, 5), BlockPos::new(15, 15, 15)),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
        compare_execution(program, &[(0, source), (5, source), (10, source), (11, source)], 32);
    }

    #[cfg(windows)]
    #[test]
    fn llvm_moving_redstone_block_and_observers_match_rust() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        use crate::world::World;
        use mchprs_blocks::blocks::RedstonePiston;
        use mchprs_blocks::{BlockFace, BlockFacing};
        for early in [false, true] {
            let mut world = super::super::tests::host();
            let lever = BlockPos::new(15, 15, 15);
            let base = lever.offset(BlockFace::East);
            let home = base.offset(BlockFace::East);
            world.set_block(lever, Block::from_name("lever").unwrap());
            world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                base,
                Block::Piston {
                    piston: RedstonePiston {
                        facing: BlockFacing::East,
                        sticky: true,
                        extended: false,
                    },
                },
            );
            world.set_block(home, Block::RedstoneBlock);
            let lamp = home.offset(BlockFace::East).offset(BlockFace::East);
            world.set_block(lamp, Block::RedstoneLamp { lit: false });
            let Block::Observer { mut observer } = Block::from_name("observer").unwrap() else {
                unreachable!()
            };
            observer.facing = BlockFacing::North;
            world.set_block(lamp.offset(BlockFace::South), Block::Observer { observer });
            let program = super::super::compile_program(
                &world,
                (BlockPos::new(10, 10, 10), BlockPos::new(23, 20, 20)),
                super::super::ScratchEntry {
                    scheduled: vec![],
                    piston: world.piston_state().clone(),
                },
                None,
            )
            .unwrap();
            let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
            assert!(!program.arrangement_links.is_empty());
            let controls = if early {
                vec![(0, source), (1, source), (8, source), (9, source), (20, source), (27, source)]
            } else {
                vec![(0, source), (7, source), (15, source), (23, source), (32, source), (41, source)]
            };
            compare_execution(program, &controls, 64);
        }
    }

    #[cfg(windows)]
    #[test]
    fn llvm_repeaters_match_rust_delays_pulses_and_pending_work() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        for delay in 1..=4 {
            for locked in [false, true] {
                for diode in [false, true] {
                    for asymmetric in [false, true] {
                        let mut program = super::super::tests::repeater_program(delay, false, locked, diode);
                        if asymmetric {
                            program.output_programs[0][0] = 0..0;
                        }
                        program.prepare_projection();
                        let mut rust = program.clone().into_runtime_prepared();
                        rust.set_pending(Vec::new()).unwrap();
                        let control = program.positions[0];
                        let mut native = NativeRuntime::new(program, &[]).unwrap();
                        for tick in 0..96 {
                            if [0, 1, 12, 32, 35, 60].contains(&tick) {
                                rust.use_control(control).unwrap();
                                native.use_control(control).unwrap();
                            }
                            rust.tick().unwrap();
                            native.tick().unwrap();
                            let mut states = [0u8; 3];
                            assert_eq!(
                                unsafe { (native.library.read_state)(native.state.as_ptr().cast(), states.as_mut_ptr(), 3) },
                                0
                            );
                            let expected: Vec<_> = rust.blocks.iter().map(|block| component_state(*block).1).collect();
                            assert_eq!(states.as_slice(), expected, "delay={delay} locked={locked} diode={diode} tick={tick}");
                            assert_eq!(native.pending_entries().unwrap(), rust.pending_entries(), "pending at tick {tick}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn descriptor_layout_and_bounds_are_stable() {
        assert_eq!(std::mem::size_of::<NativeApiDescriptor>(), 16);
        assert_eq!(std::mem::align_of::<NativeApiDescriptor>(), 8);
        assert_eq!(std::mem::offset_of!(NativeApiDescriptor, semantic_version), 8);
        assert_eq!(std::mem::size_of::<NativePendingTick>(), 12);
        assert_eq!(std::mem::size_of::<NativeProgramDescriptor>(), 52);
        assert_eq!(std::mem::align_of::<NativeProgramDescriptor>(), 4);
        assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, operation_offset), 12);
        assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, component_offset), 28);
        assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, source_count), 40);
        assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, watched_wire_count), 48);
        let descriptor = NativeProgramDescriptor {
            abi_version: ABI_VERSION,
            header_bytes: HEADER_BYTES as u32,
            total_bytes: (HEADER_BYTES + AUX_TABLES * 8) as u32,
            operation_offset: HEADER_BYTES as u32,
            operation_count: 0,
            link_offset: HEADER_BYTES as u32,
            link_count: 0,
            component_offset: HEADER_BYTES as u32,
            component_count: 0,
            source_offset: HEADER_BYTES as u32,
            source_count: 0,
            arrangement_link_count: 0,
            watched_wire_count: 0,
        };
        let mut blob = Vec::from(*b"MCHN");
        blob.extend_from_slice(&ABI_VERSION.to_le_bytes());
        blob.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        blob.extend_from_slice(&((HEADER_BYTES + AUX_TABLES * 8) as u32).to_le_bytes());
        for _ in 0..AUX_TABLES {
            blob.extend_from_slice(&((HEADER_BYTES + AUX_TABLES * 8) as u32).to_le_bytes());
            blob.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(validate_blob(&descriptor, &blob).is_ok());
        let mut corrupt = descriptor;
        corrupt.link_offset = u32::MAX;
        assert!(validate_blob(&corrupt, &blob).is_err());
    }
}
