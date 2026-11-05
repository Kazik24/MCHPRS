//! Versioned, pointer-free input format for the experimental native runner.
//!
//! The payload is encoded field by field in little endian. Rust allocation layouts never cross
//! the DLL boundary, and future descriptor versions must not reinterpret existing blobs.
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
mod library;
mod movement;
use library::Library;
mod routing;
mod shapes;
use crate::redpiler::memory;
pub(in crate::redpiler) use cache::clear as clear_cache;
pub(in crate::redpiler) use cache::initialize as initialize_cache;
use movement::encode_movement;

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

pub(super) struct LlvmProgram {
    pub(super) descriptor: NativeProgramDescriptor,
    pub(super) blob: Vec<u8>,
}

#[cfg(any(windows, target_os = "linux"))]
pub(in crate::redpiler) struct LlvmRuntime {
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
    pub(in crate::redpiler) fn acknowledge_effects(&mut self, host_queued: usize) {
        unsafe { (self.library.ack_effects)(self.state.as_mut_ptr().cast()) }
        if let Some(commands) = &mut self.commands {
            commands.acknowledge(&self.library.commands, host_queued);
        }
    }
    pub(in crate::redpiler) fn initialize_tick(&mut self, tick: u64) {
        if let Some(commands) = &mut self.commands {
            commands.logical_base = tick;
        }
    }
    // Borrow the shared graph: cloning a large circuit here can exhaust server memory.
    pub(in crate::redpiler) fn new(program: &Program, ticks: &[TickEntry]) -> Result<Self, ScratchError> {
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

    pub(in crate::redpiler) fn tick(&mut self) -> Result<(), ScratchError> {
        if let Some(commands) = &mut self.commands {
            commands.prepare_tick(&self.library.commands, self.state.as_mut_ptr().cast())?;
        }
        let status = unsafe { (self.library.tick)(self.state.as_mut_ptr().cast()) };
        self.call_status(status)
    }

    pub(in crate::redpiler) fn use_control(&mut self, pos: BlockPos) -> Result<(), ScratchError> {
        let Some((_, source)) = self.sources.iter().find(|(source_pos, _)| *source_pos == pos) else {
            return Ok(());
        };
        let status = unsafe { (self.library.use_control)(self.state.as_mut_ptr().cast(), *source) };
        self.call_status(status)
    }

    pub(in crate::redpiler) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.sources.iter().any(|(source, _)| *source == pos)
    }

    pub(in crate::redpiler) fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), ScratchError> {
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

    pub(in crate::redpiler) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
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
pub(in crate::redpiler) struct LlvmRuntime;

#[cfg(not(any(windows, target_os = "linux")))]
impl LlvmRuntime {
    pub(in crate::redpiler) fn acknowledge_effects(&mut self, _: usize) {
        unreachable!()
    }
    pub(in crate::redpiler) fn initialize_tick(&mut self, _: u64) {
        unreachable!()
    }
    pub(in crate::redpiler) fn new(_: &Program, _: &[TickEntry]) -> Result<Self, ScratchError> {
        Library::load().map(|_| Self)
    }
    pub(in crate::redpiler) fn sync_projection(&self, _: &mut super::Runtime) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn sync_publication(&self, _: &mut super::Runtime) -> Result<(), ScratchError> {
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
    pub(in crate::redpiler) fn set_pressure_plate(&mut self, _: BlockPos, _: bool) -> Result<(), ScratchError> {
        unreachable!()
    }
    pub(in crate::redpiler) fn pending_entries(&self) -> Result<Vec<TickEntry>, ScratchError> {
        unreachable!()
    }
}

pub(in crate::redpiler) fn unsupported_components(program: &Program) -> (Vec<ScratchError>, usize) {
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

impl LlvmProgram {
    fn into_aligned_blob(self) -> Result<Vec<u64>, ScratchError> {
        // The C executor reads u32 tables directly, so the backing allocation must be aligned.
        let mut aligned = memory::zeroed::<u64>(self.blob.len().div_ceil(8))?;
        for (word, chunk) in aligned.iter_mut().zip(self.blob.chunks(8)) {
            let mut bytes = [0; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            *word = u64::from_ne_bytes(bytes);
        }
        Ok(aligned)
    }

    pub(super) fn encode(program: &Program) -> Result<Self, ScratchError> {
        memory::check(0)?;
        if program.positions.len() >= PISTON_HEAD_NOTIFICATION {
            return Err(overflow());
        }
        let initial_bytes = HEADER_BYTES
            .checked_add(program.operations.len().checked_mul(OP_BYTES).ok_or_else(overflow)?)
            .and_then(|size| size.checked_add(program.links.len().checked_mul(LINK_BYTES)?))
            .and_then(|size| size.checked_add(program.positions.len().checked_mul(COMPONENT_BYTES)?))
            .and_then(|size| size.checked_add(program.sources.len().checked_mul(4)?))
            .ok_or_else(overflow)?;
        let mut blob = Vec::new();
        memory::reserve(&mut blob, initial_bytes)?;
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

fn encode_comparators(program: &Program, table: &mut Vec<u32>) -> Result<(), ScratchError> {
    use super::program::ComparatorInput;
    let level = |input| match input {
        None => Ok(0),
        Some(ComparatorInput::Static(level)) => Ok(u32::from(level) + 1),
        Some(ComparatorInput::CopperBulb(source)) => {
            // Static levels use 1..=16; the high bit tags a live bulb component.
            let source = u32::try_from(source).map_err(|_| overflow())?;
            if source >= 0x8000_0000 {
                return Err(overflow());
            }
            Ok(0x8000_0000 | source)
        }
        Some(ComparatorInput::CommandBlock(source)) => {
            let source = u32::try_from(source).map_err(|_| overflow())?;
            if source >= 0x4000_0000 {
                return Err(overflow());
            }
            Ok(0x4000_0000 | source)
        }
    };
    table.resize(program.initial.len().checked_mul(4).ok_or_else(overflow)?, 0);
    for record in &program.comparators {
        let Block::RedstoneComparator { comparator } = program.initial[record.component] else {
            return Err(overflow());
        };
        table[record.component * 4..record.component * 4 + 4].copy_from_slice(&[
            1 | (u32::from(comparator.mode == mchprs_blocks::blocks::ComparatorMode::Subtract) << 1) | (u32::from(record.output_is_diode) << 2),
            level(record.rear_override)?,
            level(record.far_override)?,
            u32::from(record.stored),
        ]);
    }
    Ok(())
}

fn encode_aux(program: &Program, blob: &mut Vec<u8>) -> Result<(), ScratchError> {
    let mut tables: [Vec<u32>; AUX_TABLES] = std::array::from_fn(|_| Vec::new());
    let id = |n: usize| u32::try_from(n).map_err(|_| overflow());
    for row in &program.side_outputs {
        memory::check_periodic()?;
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
            tables[4].push(encode_notification(program, node)?);
        }
    }
    for record in &program.watched_wires {
        let start = id(tables[4].len())?;
        tables[3].extend([id(record.wire)?, start, id(record.watchers.len())?]);
        for &node in &record.watchers {
            tables[4].push(encode_notification(program, node)?);
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
    memory::check(0)?;
    routing::encode(program, &mut tables)?;
    memory::check(0)?;
    encode_comparators(program, &mut tables[42])?;
    effects::encode(program, &mut tables)?;
    shapes::encode(program, &mut tables)?;
    for row in &program.comparators_by_override {
        let start = id(tables[60].len())?;
        tables[59].extend([start, id(row.len())?]);
        for &node in row {
            tables[60].push(id(node)?);
        }
    }
    tables[52] = program
        .initial
        .iter()
        .map(|block| match block {
            Block::RedstoneWire { wire } => wire.north.get_id() | (wire.south.get_id() << 2) | (wire.east.get_id() << 4) | (wire.west.get_id() << 6),
            _ => 0,
        })
        .collect();
    let directory = blob.len();
    let additional = tables
        .iter()
        .try_fold(AUX_TABLES * 8, |bytes, table| {
            table.len().checked_mul(4).and_then(|size| bytes.checked_add(size))
        })
        .ok_or_else(overflow)?;
    memory::reserve(blob, additional)?;
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

fn encode_notification(program: &Program, notification: usize) -> Result<u32, ScratchError> {
    let encoded = u32::try_from(notification).map_err(|_| overflow())?;
    let component = notification_component(notification);
    if component >= program.initial.len()
        || (notification & PISTON_HEAD_NOTIFICATION != 0
            && (!matches!(program.initial[component], Block::Piston { .. })
                || !program.piston_by_component.get(component).copied().flatten().is_some_and(|piston| {
                    program
                        .pistons
                        .get(piston.get() - 1)
                        .is_some_and(|piston| piston.base == program.positions[component])
                })))
    {
        return Err(overflow());
    }
    Ok(encoded)
}

pub(super) fn validate_blob(descriptor: &NativeProgramDescriptor, blob: &[u8]) -> Result<(), ScratchError> {
    let invalid = || ScratchError {
        position: None,
        reason: "invalid native Program descriptor",
    };
    if descriptor.abi_version != ABI_VERSION
        || descriptor.component_count >= PISTON_HEAD_NOTIFICATION as u32
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
        if (kind > 15 && kind != u8::MAX)
            || component[13]
                > (if kind == 5 {
                    15
                } else if kind == 4 || kind == 13 {
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
    memory::check(blob.len())?;
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
        || tables[52].len() != descriptor.component_count as usize
        || tables[59].len() != descriptor.component_count as usize * 2
    {
        return Err(invalid());
    }
    for (node, &sides) in tables[52].iter().enumerate() {
        if sides > 255
            || (0..4).any(|side| (sides >> (side * 2)) & 3 == 3)
            || (sides != 0 && blob[descriptor.component_offset as usize + node * COMPONENT_BYTES + 12] != 5)
        {
            return Err(invalid());
        }
    }
    for (rows, limit) in [
        (0, tables[1].len() / 2),
        (2, tables[4].len()),
        (5, tables[6].len()),
        (59, tables[60].len()),
    ] {
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
    let piston_components: std::collections::HashSet<_> = tables[7].chunks_exact(10).map(|piston| piston[0] as usize).collect();
    let valid_notification = |notification: u32| {
        let component = notification_component(notification as usize);
        component < descriptor.component_count as usize
            && (notification & PISTON_HEAD_NOTIFICATION as u32 == 0
                || (blob[descriptor.component_offset as usize + component * COMPONENT_BYTES + 12] == 8 && piston_components.contains(&component)))
    };
    if tables[4].iter().any(|&node| !valid_notification(node)) || tables[6].iter().any(|&link| link >= descriptor.link_count) {
        return Err(invalid());
    }
    if tables[7].len() % 10 != 0
        || tables[23].len() % 5 != 0
        || tables[23]
            .chunks_exact(5)
            .any(|row| !valid_notification(row[0]) || (row[0] & PISTON_HEAD_NOTIFICATION as u32 != 0 && row[2] != 0))
        || tables[24].iter().any(|&wire| wire >= descriptor.component_count)
    {
        return Err(invalid());
    }
    if tables[60]
        .iter()
        .any(|&node| node >= descriptor.component_count || blob[descriptor.component_offset as usize + node as usize * COMPONENT_BYTES + 12] != 9)
    {
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
        Block::RedstoneComparator { comparator } => (9, u8::from(comparator.powered)),
        Block::Hopper { enabled, .. } => (10, u8::from(enabled)),
        block if block.pressure_plate_powered().is_some() => (11, u8::from(block.pressure_plate_powered() == Some(true))),
        Block::NoteBlock { powered, .. } => (12, u8::from(powered)),
        Block::IronTrapdoor { powered, .. } => (14, u8::from(powered)),
        block if block.is_command_block() => (15, 0),
        block if block.is_copper_bulb() => {
            let (lit, powered) = block.copper_bulb_state().unwrap();
            (13, u8::from(lit) | (u8::from(powered) << 1))
        }
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
