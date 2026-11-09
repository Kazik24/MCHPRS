//! Native execution for sampled Redpilator programs.
use super::{
    ScratchError,
    program::{Operation, PISTON_HEAD_NOTIFICATION, Program, notification_component},
};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_world::{TickEntry, TickPriority};
mod cache;
mod command;
mod command_notifications;
mod effects;
mod encoding;
mod library;
mod movement;
mod routing;
mod shapes;
use crate::redpilator::memory;
pub(in crate::redpilator) use cache::clear as clear_cache;
pub(in crate::redpilator) use cache::initialize as initialize_cache;
pub(super) use encoding::LlvmProgram;
use encoding::{component_state, encode_notification, overflow};
use library::Library;

pub(super) const ABI_VERSION: u32 = 3;
const AUX_TABLES: usize = 61;
const MAGIC: [u8; 4] = *b"MCHN";
const HEADER_BYTES: usize = 16;
const OP_BYTES: usize = 8;
const LINK_BYTES: usize = 12;
const COMPONENT_BYTES: usize = 32;
const PASSIVE_TICK: u32 = 0x8000_0000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativePendingTick {
    component: u32,
    ticks_left: u32,
    priority: u8,
    reserved: [u8; 3],
}

struct InitialTicks {
    entries: Vec<NativePendingTick>,
    capacity: u32,
}

impl InitialTicks {
    /// Translate host pending ticks into native scheduler records.
    ///
    /// Inputs: The validated program and ordered TickEntry slice.
    ///
    /// Check count/delay/priority limits, resolve live matching component IDs, encode stale or passive
    /// positions as imported-tick references and compute bounded queue capacity.
    fn encode(program: &Program, ticks: &[TickEntry]) -> Result<Self, ScratchError> {
        let count = u32::try_from(ticks.len()).map_err(|_| overflow())?;
        if count >= PASSIVE_TICK || program.component_count() >= PASSIVE_TICK as usize {
            return Err(overflow());
        }
        let capacity = program
            .component_count()
            .checked_mul(2)
            .and_then(|count| count.checked_add(ticks.len()))
            .ok_or_else(overflow)?
            .max(64);
        let capacity = u32::try_from(capacity).map_err(|_| overflow())?;
        let mut entries = Vec::new();
        memory::reserve(&mut entries, ticks.len())?;
        for (index, tick) in ticks.iter().enumerate() {
            if tick.ticks_left >= 32 || (tick.ticks_left == 0 && tick.tick_priority != TickPriority::NanoTick) {
                return Err(ScratchError {
                    position: Some(tick.pos),
                    reason: "LLVM cannot import this scheduled tick",
                });
            }
            let component = program
                .component_indices
                .get(&tick.pos)
                .filter(|&&node| tick.block_type == Some(program.initial[node].registry_id()))
                .map_or(PASSIVE_TICK | index as u32, |&node| node as u32);
            entries.push(NativePendingTick {
                component,
                ticks_left: tick.ticks_left,
                priority: tick.tick_priority as u8,
                reserved: [0; 3],
            });
        }
        Ok(Self { entries, capacity })
    }
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

#[cfg(any(windows, target_os = "linux"))]
pub(in crate::redpilator) struct LlvmRuntime {
    library: Library,
    state: Vec<u64>,
    descriptor: NativeProgramDescriptor,
    blob: Vec<u64>,
    positions: Vec<BlockPos>,
    initial: Vec<Block>,
    sources: Vec<(BlockPos, u32)>,
    commands: Option<command::State>,
    imported_pending: Vec<TickEntry>,
    pending_capacity: u32,
}

#[cfg(any(windows, target_os = "linux"))]
impl LlvmRuntime {
    pub(in crate::redpilator) fn acknowledge_effects(&mut self, host_queued: usize) {
        unsafe { (self.library.ack_effects)(self.state.as_mut_ptr().cast()) }
        if let Some(commands) = &mut self.commands {
            commands.acknowledge(&self.library.commands, host_queued);
        }
    }
    pub(in crate::redpilator) fn initialize_tick(&mut self, tick: u64) {
        if let Some(commands) = &mut self.commands {
            commands.logical_base = tick;
        }
    }
    /// Construct a native executor while retaining all backing storage.
    ///
    /// Inputs: The shared program data and imported host ticks.
    ///
    /// Check backend support, encode and align program tables, validate the loaded ABI, create owned
    /// state, then bind commands and import ticks so failures release the partially built runtime.
    // Borrow the shared graph: cloning a large circuit here can exhaust server memory.
    pub(in crate::redpilator) fn new(program: &Program, ticks: &[TickEntry]) -> Result<Self, ScratchError> {
        log_program_diagnostics(program);
        if let Some(error) = unsupported_reason(program) {
            return Err(error);
        }
        let initial_ticks = InitialTicks::encode(program, ticks)?;
        let encoded = LlvmProgram::encode(program)?;
        log_artifact_diagnostics(encoded.blob.len());
        let library = Library::load()?;
        library.validate_abi()?;
        let descriptor = encoded.descriptor;
        let blob = encoded.into_aligned_blob()?;
        let state = library.create_state(&descriptor, &blob, initial_ticks.entries.len() as u32)?;
        let sources = program
            .sources
            .iter()
            .enumerate()
            .map(|(index, &source)| (program.positions[source], index as u32))
            .collect();
        let mut runtime = Self {
            library,
            state,
            descriptor,
            blob,
            positions: program.positions.clone(),
            initial: program.initial.clone(),
            sources,
            commands: None,
            imported_pending: ticks.to_vec(),
            pending_capacity: initial_ticks.capacity,
        };
        runtime.bind_commands(program)?;
        runtime.import_initial_ticks(&initial_ticks.entries)?;
        Ok(runtime)
    }

    fn bind_commands(&mut self, program: &Program) -> Result<(), ScratchError> {
        self.commands = command::State::new(
            program,
            &self.library.commands,
            self.state.as_mut_ptr().cast(),
            self.blob
                .as_ptr()
                .cast::<u8>()
                .wrapping_add(self.descriptor.component_offset as usize)
                .cast(),
        )?;
        Ok(())
    }

    fn import_initial_ticks(&mut self, ticks: &[NativePendingTick]) -> Result<(), ScratchError> {
        for tick in ticks {
            let status = unsafe { (self.library.import_tick)(self.state.as_mut_ptr().cast(), tick.component, tick.ticks_left, tick.priority) };
            if status != 0 {
                return Err(ScratchError {
                    position: None,
                    reason: "LLVM rejected an initial scheduled tick",
                });
            }
        }
        Ok(())
    }

    pub(in crate::redpilator) fn tick(&mut self) -> Result<(), ScratchError> {
        if let Some(commands) = &mut self.commands {
            commands.prepare_tick(&self.library.commands, self.state.as_mut_ptr().cast())?;
        }
        let status = unsafe { (self.library.tick)(self.state.as_mut_ptr().cast()) };
        self.call_status(status)
    }

    pub(in crate::redpilator) fn use_control(&mut self, pos: BlockPos) -> Result<(), ScratchError> {
        let Some((_, source)) = self.sources.iter().find(|(source_pos, _)| *source_pos == pos) else {
            return Ok(());
        };
        let status = unsafe { (self.library.use_control)(self.state.as_mut_ptr().cast(), *source) };
        self.call_status(status)
    }

    pub(in crate::redpilator) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.sources.iter().any(|(source, _)| *source == pos)
    }

    pub(in crate::redpilator) fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), ScratchError> {
        let Some((_, source)) = self.sources.iter().find(|(source_pos, _)| *source_pos == pos).filter(|_| {
            self.positions
                .iter()
                .zip(&self.initial)
                .any(|(&at, block)| at == pos && block.pressure_plate_powered().is_some())
        }) else {
            return Err(ScratchError {
                position: Some(pos),
                reason: "pressure plate is not a compiled input",
            });
        };
        let status = unsafe { (self.library.set_input)(self.state.as_mut_ptr().cast(), *source, u8::from(powered)) };
        self.call_status(status)
    }

    pub(super) fn sync_commands(&self, runtime: &mut super::Runtime) -> Result<(), ScratchError> {
        if let Some(commands) = &self.commands {
            commands.sync(&self.library.commands, runtime)?;
        }
        Ok(())
    }

    /// Export native pending work as host scheduler entries.
    ///
    /// Inputs: This runtime's native state, queue capacity and original passive tick records.
    ///
    /// Allocate a bounded read buffer, request pending records from the native API and decode each record
    /// back into a checked world position, delay and priority.
    pub(in crate::redpilator) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
        let count = unsafe { (self.library.pending_count)(self.state.as_ptr().cast()) };
        if count > self.pending_capacity {
            return Err(ScratchError {
                position: None,
                reason: "LLVM pending tick count is invalid",
            });
        }
        let mut pending = vec![NativePendingTick::default(); count as usize];
        let status = unsafe { (self.library.read_pending)(self.state.as_ptr().cast(), pending.as_mut_ptr(), count) };
        self.call_status(status)?;
        pending.into_iter().map(|entry| self.decode_pending_tick(entry)).collect()
    }

    /// Validate and decode one pending native callback.
    ///
    /// Inputs: The encoded pending record and this runtime's component/passive-tick maps.
    ///
    /// Reject invalid delay/priority combinations, distinguish passive imported references from component
    /// IDs and reconstruct the corresponding host TickEntry.
    fn decode_pending_tick(&self, entry: NativePendingTick) -> Result<TickEntry, ScratchError> {
        let invalid = || ScratchError {
            position: None,
            reason: "LLVM pending tick component is invalid",
        };
        let (pos, block_type) = if entry.component & PASSIVE_TICK != 0 {
            let original = self
                .imported_pending
                .get((entry.component & !PASSIVE_TICK) as usize)
                .ok_or_else(invalid)?;
            (original.pos, original.block_type)
        } else {
            let pos = *self.positions.get(entry.component as usize).ok_or_else(invalid)?;
            (pos, Some(self.initial[entry.component as usize].registry_id()))
        };
        if entry.ticks_left >= 32 {
            return Err(invalid());
        }
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
            block_type,
        })
    }

    fn call_status(&self, status: u32) -> Result<(), ScratchError> {
        if status == 0 {
            Ok(())
        } else {
            tracing::warn!(status, "LLVM operation failed");
            Err(ScratchError {
                position: None,
                reason: native_status_reason(status),
            })
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl Drop for LlvmRuntime {
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
pub(in crate::redpilator) struct LlvmRuntime;

#[cfg(not(any(windows, target_os = "linux")))]
impl LlvmRuntime {
    pub(in crate::redpilator) fn acknowledge_effects(&mut self, _: usize) {
        unreachable!()
    }
    pub(in crate::redpilator) fn initialize_tick(&mut self, _: u64) {
        unreachable!()
    }
    pub(in crate::redpilator) fn new(_: &Program, _: &[TickEntry]) -> Result<Self, ScratchError> {
        Library::load().map(|_| Self)
    }
    pub(in crate::redpilator) fn sync_projection(&self, _: &mut super::Runtime) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpilator) fn sync_publication(&self, _: &mut super::Runtime) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpilator) fn tick(&mut self) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpilator) fn use_control(&mut self, _: BlockPos) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpilator) fn accepts_control(&self, _: BlockPos) -> bool {
        false
    }
    pub(in crate::redpilator) fn set_pressure_plate(&mut self, _: BlockPos, _: bool) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpilator) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
        unreachable!()
    }
}

/// Collect representative components unsupported by the native backend.
///
/// Inputs: The compiled program's initial component blocks and metadata.
///
/// Check each represented block against backend restrictions, count all rejections and retain only the
/// diagnostic examples needed by the caller.
pub(in crate::redpilator) fn unsupported_components(program: &Program) -> (Vec<ScratchError>, usize) {
    let mut first = Vec::new();
    let mut count = 0;
    for (node, block) in program.initial.iter().enumerate() {
        if !matches!(
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
                | Block::RedstoneComparator { .. }
                | Block::Hopper { .. }
                | Block::NoteBlock { .. }
                | Block::StonePressurePlate { .. }
                | Block::IronTrapdoor { .. }
        ) && !block.is_copper_bulb()
            && !block.pressure_plate_powered().is_some()
            && !block.is_command_block()
        {
            count += 1;
            if first.len() < 3 {
                let reason = block.get_name();
                first.push(ScratchError {
                    position: Some(program.positions[node]),
                    reason,
                });
            }
        }
    }
    (first, count)
}

/// Find the first program-level native-backend admission failure.
///
/// Inputs: The compiled program and its supported-component/topology metadata.
fn unsupported_reason(program: &Program) -> Option<ScratchError> {
    let error = |position, reason| Some(ScratchError { position, reason });
    if let Some(issue) = unsupported_components(program).0.into_iter().next() {
        return Some(issue);
    }
    for link in &program.links {
        if matches!(
            program.initial[link.target],
            Block::Lever { .. } | Block::StoneButton { .. } | Block::RedstoneBlock
        ) {
            return error(Some(program.positions[link.target]), "LLVM does not support this electrical link target");
        }
    }
    None
}

fn log_program_diagnostics(program: &Program) {
    if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_none() {
        return;
    }
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for block in &program.initial {
        let name = format!("{block:?}").split([' ', '{']).next().unwrap().to_owned();
        *kinds.entry(name).or_default() += 1;
    }
    eprintln!("LLVM Program component kinds: {kinds:?}");
}

fn log_artifact_diagnostics(blob_bytes: usize) {
    if std::env::var_os("MCHPRS_LLVM_DIAGNOSTICS").is_none() {
        return;
    }
    eprintln!("LLVM native: per-plot generated C bytes=0, Program blob bytes={blob_bytes}");
}

fn native_status_reason(status: u32) -> &'static str {
    match status {
        1 => "LLVM rejected invalid runtime state or input",
        2 => "LLVM encountered an unsupported runtime operation",
        3 => "LLVM runtime capacity exceeded",
        4 => "LLVM exceeded the per-tick operation limit; check for feedback loops or reduce the circuit",
        _ => "LLVM returned an unknown error; see server log",
    }
}

#[cfg(test)]
mod tests;
