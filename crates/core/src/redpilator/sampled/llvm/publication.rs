//! Small native exports for visual IO and effects; reset retains the complete snapshot.
use super::{LlvmRuntime, ScratchError};
use crate::redpilator::sampled::Runtime;
use mchprs_blocks::blocks::Block;

#[cfg(any(windows, target_os = "linux"))]
impl LlvmRuntime {
    pub(in crate::redpilator) fn sync_io(&mut self, runtime: &mut Runtime) -> Result<(), ScratchError> {
        if runtime.blocks.len() != self.initial.len() {
            return Err(ScratchError {
                position: None,
                reason: "invalid LLVM IO projection",
            });
        }
        if self.io_dirty && !self.io_components.is_empty() {
            // SAFETY: new builds equal-length ID/output buffers from valid component IDs; C checks IDs and
            // writes count bytes to separate output.
            let status = unsafe {
                (self.library.read_components)(
                    self.state.as_ptr().cast(),
                    self.io_components.as_ptr(),
                    self.io_states.as_mut_ptr(),
                    self.io_components.len() as u32,
                )
            };
            self.call_status(status)?;
        }
        for (&id, &state) in self.io_components.iter().zip(&self.io_states) {
            if state > if self.initial[id as usize].is_copper_bulb() { 3 } else { 1 } {
                return Err(ScratchError {
                    position: Some(self.positions[id as usize]),
                    reason: "invalid LLVM IO state",
                });
            }
        }
        self.sync_effects(runtime)?;
        for (&id, &state) in self.io_components.iter().zip(&self.io_states) {
            let node = id as usize;
            let invalid = || ScratchError {
                position: Some(self.positions[node]),
                reason: "invalid LLVM IO state",
            };
            let block = match self.initial[node] {
                Block::Lever { mut lever } if state <= 1 => {
                    lever.powered = state != 0;
                    Block::Lever { lever }
                }
                Block::StoneButton { mut button } if state <= 1 => {
                    button.powered = state != 0;
                    Block::StoneButton { button }
                }
                Block::RedstoneLamp { .. } if state <= 1 => Block::RedstoneLamp { lit: state != 0 },
                block if block.is_copper_bulb() && state <= 3 => block.with_copper_bulb_state(state & 1 != 0, state & 2 != 0).ok_or_else(invalid)?,
                block if block.pressure_plate_powered().is_some() && state <= 1 => block.with_pressure_plate_power(state != 0).ok_or_else(invalid)?,
                _ => return Err(invalid()),
            };
            if runtime.blocks[node] != block {
                runtime.blocks[node] = block;
                runtime.mark_dirty(runtime.program.component_cells[node]);
            }
        }
        self.io_dirty = false;
        Ok(())
    }

    pub(in crate::redpilator) fn sync_redstone(&mut self, runtime: &mut Runtime) -> Result<(), ScratchError> {
        if runtime.blocks.len() != self.initial.len() || runtime.comparator_outputs.len() != self.initial.len() {
            return Err(ScratchError {
                position: None,
                reason: "invalid LLVM redstone projection",
            });
        }
        if self.redstone_dirty && !self.redstone_components.is_empty() {
            // SAFETY: The retained ID and word buffers have equal lengths and valid component IDs; C validates
            // every ID before writing the separate packed output.
            let status = unsafe {
                (self.library.read_visual_components)(
                    self.state.as_ptr().cast(),
                    self.redstone_components.as_ptr(),
                    self.redstone_states.as_mut_ptr(),
                    self.redstone_components.len() as u32,
                )
            };
            self.call_status(status)?;
        }
        // Validate the complete export before changing the projection or synchronizing effects.
        for (&id, &state) in self.redstone_components.iter().zip(&self.redstone_states) {
            self.visual_block(id as usize, state)?;
        }
        self.sync_effects_scope(runtime, true)?;
        for (&id, &state) in self.redstone_components.iter().zip(&self.redstone_states) {
            let node = id as usize;
            let (block, comparator) = self.visual_block(node, state)?;
            if runtime.blocks[node] != block || comparator.is_some_and(|value| runtime.comparator_outputs[node] != value) {
                runtime.blocks[node] = block;
                if let Some(value) = comparator {
                    runtime.comparator_outputs[node] = value;
                }
                runtime.mark_dirty(runtime.program.component_cells[node]);
            }
        }
        self.redstone_dirty = false;
        Ok(())
    }

    fn visual_block(&self, node: usize, packed: u32) -> Result<(Block, Option<u8>), ScratchError> {
        let invalid = || ScratchError {
            position: Some(self.positions[node]),
            reason: "invalid LLVM redstone state",
        };
        let state = packed as u8;
        let sides = (packed >> 8) as u8;
        let comparator = (packed >> 16) as u8;
        let instrument = (packed >> 24) as u8;
        let initial = self.initial[node];
        if (sides != 0 && !matches!(initial, Block::RedstoneWire { .. }))
            || (comparator != 0 && !matches!(initial, Block::RedstoneComparator { .. }))
            || (instrument != 0 && !matches!(initial, Block::NoteBlock { .. }))
            || comparator > 15
            || instrument > 15
        {
            return Err(invalid());
        }
        let block = match initial {
            Block::Lever { mut lever } if state <= 1 => {
                lever.powered = state != 0;
                Block::Lever { lever }
            }
            Block::StoneButton { mut button } if state <= 1 => {
                button.powered = state != 0;
                Block::StoneButton { button }
            }
            Block::RedstoneLamp { .. } if state <= 1 => Block::RedstoneLamp { lit: state != 0 },
            Block::IronTrapdoor { facing, half, .. } if state <= 1 => Block::IronTrapdoor {
                facing,
                half,
                powered: state != 0,
            },
            block if block.is_copper_bulb() && state <= 3 => block.with_copper_bulb_state(state & 1 != 0, state & 2 != 0).ok_or_else(invalid)?,
            Block::RedstoneComparator { mut comparator } if state <= 1 => {
                comparator.powered = state != 0;
                Block::RedstoneComparator { comparator }
            }
            Block::Hopper { facing, .. } if state <= 1 => Block::Hopper { facing, enabled: state != 0 },
            block if block.pressure_plate_powered().is_some() && state <= 1 => block.with_pressure_plate_power(state != 0).ok_or_else(invalid)?,
            Block::NoteBlock { note, .. } if state <= 1 => Block::NoteBlock {
                instrument: mchprs_blocks::blocks::Instrument::from_id(u32::from(instrument)),
                note,
                powered: state != 0,
            },
            Block::RedstoneRepeater { mut repeater } if state <= 3 => {
                repeater.powered = state & 1 != 0;
                repeater.locked = state & 2 != 0;
                Block::RedstoneRepeater { repeater }
            }
            Block::RedstoneWire { mut wire } if state <= 15 => {
                if (0..4).any(|side| (sides >> (side * 2)) & 3 == 3) {
                    return Err(invalid());
                }
                use mchprs_blocks::blocks::RedstoneWireSide;
                wire.north = RedstoneWireSide::from_id(u32::from(sides & 3));
                wire.south = RedstoneWireSide::from_id(u32::from((sides >> 2) & 3));
                wire.east = RedstoneWireSide::from_id(u32::from((sides >> 4) & 3));
                wire.west = RedstoneWireSide::from_id(u32::from((sides >> 6) & 3));
                wire.power = state;
                Block::RedstoneWire { wire }
            }
            Block::Observer { mut observer } if state <= 1 => {
                observer.powered = state != 0;
                Block::Observer { observer }
            }
            Block::RedstoneTorch { .. } if state <= 1 => Block::RedstoneTorch { lit: state != 0 },
            Block::RedstoneWallTorch { facing, .. } if state <= 1 => Block::RedstoneWallTorch { facing, lit: state != 0 },
            block if block.is_command_block() && state <= 1 => block,
            _ => return Err(invalid()),
        };
        Ok((block, matches!(initial, Block::RedstoneComparator { .. }).then_some(comparator)))
    }

    fn sync_clock(&mut self, runtime: &mut Runtime) {
        if self.clock_dirty {
            // SAFETY: This scalar query reads retained native state without concurrent mutation.
            self.native_clock = unsafe { (self.library.tick_count)(self.state.as_ptr().cast()) };
            self.clock_dirty = false;
        }
        runtime.motion_time = self.native_clock;
    }

    pub(in crate::redpilator) fn sync_effects(&mut self, runtime: &mut Runtime) -> Result<(), ScratchError> {
        self.sync_effects_scope(runtime, false)
    }

    fn sync_effects_scope(&mut self, runtime: &mut Runtime, include_command_entities: bool) -> Result<(), ScratchError> {
        if !self.has_effects() {
            self.sync_clock(runtime);
            return Ok(());
        }
        let invalid = || ScratchError {
            position: None,
            reason: "invalid LLVM effect state",
        };
        // SAFETY: The runtime retains initialized state and its blob; note_count only reads the pending note
        // count.
        let count = if self.has_notes {
            (unsafe { (self.library.note_count)(self.state.as_ptr().cast()) }) as usize
        } else {
            0
        };
        if count > self.initial.len() || runtime.note_queued.len() != self.initial.len() || runtime.program.initial.len() != self.initial.len() {
            return Err(invalid());
        }
        let words = count.checked_mul(3).ok_or_else(invalid)?;
        if words > self.note_events.len() {
            let additional = words - self.note_events.len();
            super::memory::reserve(&mut self.note_events, additional)?;
            self.note_events.resize(words, 0);
        }
        if count != 0 {
            // SAFETY: note_events holds at least count * 3 u32s; C checks the unchanged count and writes three
            // words per note.
            let status = unsafe { (self.library.read_notes)(self.state.as_ptr().cast(), self.note_events.as_mut_ptr(), count as u32) };
            self.call_status(status)?;
        }
        // Validate before changing the projection or acknowledging native effects.
        if count != 0 {
            if self.note_seen.is_empty() {
                self.note_seen = super::memory::zeroed(self.initial.len())?;
            }
            self.note_seen.fill(false);
        }
        for row in self.note_events[..words].chunks_exact(3) {
            if row[0] as usize >= self.initial.len()
                || !matches!(self.initial[row[0] as usize], Block::NoteBlock { .. })
                || row[1] > 15
                || row[2] > 24
            {
                return Err(invalid());
            }
            if self.note_seen[row[0] as usize] {
                return Err(invalid());
            }
            self.note_seen[row[0] as usize] = true;
        }
        if include_command_entities {
            self.sync_commands(runtime)?;
        } else if let Some(commands) = &self.commands {
            commands.sync_effects(&self.library.commands, runtime)?;
        }
        self.sync_clock(runtime);
        for (component, ..) in runtime.notes.drain(..) {
            runtime.note_queued[component] = false;
        }
        for row in self.note_events[..words].chunks_exact(3) {
            let node = row[0] as usize;
            runtime.note_queued[node] = true;
            runtime
                .notes
                .push((node, self.positions[node], mchprs_blocks::blocks::Instrument::from_id(row[1]), row[2]));
        }
        Ok(())
    }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use crate::redpilator::sampled::program::{ComparatorRecord, ElectricalLink, Operation, Program};
    use mchprs_blocks::BlockPos;
    use std::ffi::c_void;

    fn program(with_note: bool) -> Program {
        let mut blocks = vec![
            Block::from_name("lever").unwrap(),
            Block::from_name("redstone_wire").unwrap(),
            Block::RedstoneLamp { lit: false },
            Block::from_name("comparator").unwrap(),
        ];
        if let Block::RedstoneWire { ref mut wire } = blocks[1] {
            wire.north = mchprs_blocks::blocks::RedstoneWireSide::Up;
        }
        if with_note {
            blocks.push(Block::NoteBlock {
                instrument: mchprs_blocks::blocks::Instrument::Bell,
                note: 3,
                powered: false,
            });
        }
        let positions: Vec<_> = (0..blocks.len()).map(|index| BlockPos::new(index as i32, 10, 10)).collect();
        let mut outputs = vec![[vec![], vec![]]; blocks.len()];
        let edge = vec![
            Operation::Contribution(0),
            Operation::Contribution(1),
            Operation::RefreshWire(1),
            Operation::UpdateLamp(2),
        ];
        outputs[0] = [edge.clone(), edge];
        let mut program = Program::new(
            positions,
            blocks,
            vec![0],
            outputs,
            vec![
                ElectricalLink {
                    source: 0,
                    target: 1,
                    attenuation: 0,
                },
                ElectricalLink {
                    source: 0,
                    target: 2,
                    attenuation: 0,
                },
            ],
            vec![],
            vec![],
        )
        .unwrap();
        program.comparators.push(ComparatorRecord {
            component: 3,
            stored: 7,
            rear_override: None,
            far_override: None,
            output_is_diode: false,
        });
        program.prepare_projection();
        program.validate().unwrap();
        program
    }

    unsafe extern "C" fn reject_count(_: *const c_void) -> u32 {
        u32::MAX
    }
    unsafe extern "C" fn reject_words(_: *const c_void, _: *mut u32, _: u32) -> u32 {
        1
    }
    unsafe extern "C" fn reject_pending(_: *const c_void, _: *mut super::super::NativePendingTick, _: u32) -> u32 {
        1
    }
    unsafe extern "C" fn reject_components(_: *const c_void, _: *const u32, _: *mut u8, _: u32) -> u32 {
        1
    }
    unsafe extern "C" fn reject_visual(_: *const c_void, _: *const u32, _: *mut u32, _: u32) -> u32 {
        1
    }
    unsafe extern "C" fn reject_clock(_: *const c_void) -> u64 {
        u64::MAX
    }

    fn snapshot(native: &LlvmRuntime) -> Vec<u32> {
        // SAFETY: The runtime owns initialized state; size and data are queried without intervening execution.
        let count = unsafe { (native.library.snapshot_words)(native.state.as_ptr().cast()) };
        let mut words = vec![0; count as usize];
        assert_eq!(
            unsafe { (native.library.read_snapshot)(native.state.as_ptr().cast(), words.as_mut_ptr(), count) },
            0
        );
        words
    }

    #[test]
    fn llvm_limited_publication_avoids_full_exports_and_reuses_unchanged_reads() {
        let program = program(false);
        let mut rust = program.start();
        let mut projection = program.start();
        let mut native = LlvmRuntime::new(&program, &[]).unwrap();
        let full = (
            native.library.snapshot_words,
            native.library.read_snapshot,
            native.library.pending_count,
            native.library.read_pending,
        );
        native.library.snapshot_words = reject_count;
        native.library.read_snapshot = reject_words;
        native.library.pending_count = reject_count;
        native.library.read_pending = reject_pending;
        native.library.note_count = reject_count;
        native.library.read_notes = reject_words;
        assert!(!native.has_effects());
        for _ in 0..2 {
            rust.use_control(program.positions[0]).unwrap();
            native.use_control(program.positions[0]).unwrap();
            native.sync_io(&mut projection).unwrap();
            assert_eq!(projection.blocks[0], rust.blocks[0]);
            assert_eq!(projection.blocks[2], rust.blocks[2]);
            native.sync_redstone(&mut projection).unwrap();
            assert_eq!(projection.blocks, rust.blocks);
            assert_eq!(projection.comparator_outputs, rust.comparator_outputs);

            let getters = (
                native.library.read_components,
                native.library.read_visual_components,
                native.library.tick_count,
            );
            native.library.read_components = reject_components;
            native.library.read_visual_components = reject_visual;
            native.library.tick_count = reject_clock;
            for _ in 0..2 {
                native.sync_io(&mut projection).unwrap();
                native.sync_redstone(&mut projection).unwrap();
                assert_eq!(projection.logical_tick(), rust.logical_tick());
            }
            (
                native.library.read_components,
                native.library.read_visual_components,
                native.library.tick_count,
            ) = getters;
            for _ in 0..16 {
                rust.tick().unwrap();
            }
            let mut completed = 0;
            native.tick_batch(16, &mut completed).unwrap();
            assert_eq!(completed, 16);
            native.sync_redstone(&mut projection).unwrap();
            native.sync_io(&mut projection).unwrap();
            assert_eq!(projection.logical_tick(), rust.logical_tick());
            assert_eq!(projection.blocks, rust.blocks);
        }
        (
            native.library.snapshot_words,
            native.library.read_snapshot,
            native.library.pending_count,
            native.library.read_pending,
        ) = full;
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(projection.blocks, rust.blocks);
        assert_eq!(projection.contributions, rust.contributions);
        assert_eq!(projection.pending_entries(), rust.pending_entries());
    }

    #[test]
    fn llvm_visual_words_preserve_shapes_metadata_and_reject_invalid_ids_without_writes() {
        let program = program(true);
        let mut native = LlvmRuntime::new(&program, &[]).unwrap();
        let before = snapshot(&native);
        let ids = [0, 1, 2, 3, 4];
        let mut words = [0; 5];
        // SAFETY: Both ID/output arrays have the supplied count, and the runtime owns initialized state.
        assert_eq!(
            unsafe { (native.library.read_visual_components)(native.state.as_ptr().cast(), ids.as_ptr(), words.as_mut_ptr(), 5) },
            0
        );
        assert_eq!(words[0], 0);
        let Block::RedstoneWire { wire } = program.initial[1] else {
            unreachable!()
        };
        let sides = wire.north.get_id() | (wire.south.get_id() << 2) | (wire.east.get_id() << 4) | (wire.west.get_id() << 6);
        assert_eq!(words[1], sides << 8);
        assert_eq!(words[2], 0);
        assert_eq!(words[3], 7 << 16);
        let Block::NoteBlock { instrument, .. } = program.initial[4] else {
            unreachable!()
        };
        assert_eq!(words[4], instrument.get_id() << 24);
        assert_eq!(snapshot(&native), before);
        let invalid_ids = [0, program.initial.len() as u32];
        let mut untouched = [0xdead_beef; 2];
        assert_eq!(
            unsafe { (native.library.read_visual_components)(native.state.as_ptr().cast(), invalid_ids.as_ptr(), untouched.as_mut_ptr(), 2) },
            1
        );
        assert_eq!(untouched, [0xdead_beef; 2]);
        assert_eq!(
            unsafe { (native.library.read_visual_components)(native.state.as_ptr().cast(), std::ptr::null(), std::ptr::null_mut(), 0) },
            0
        );
        assert!(native.visual_block(0, 1 << 8).is_err());
        assert!(native.visual_block(1, 3 << 8).is_err());
        assert!(native.visual_block(3, 16 << 16).is_err());
        assert!(native.visual_block(4, 16 << 24).is_err());
        native.library.read_notes = reject_words;
        let mut projection = program.start();
        native.sync_redstone(&mut projection).unwrap();
        assert_eq!(projection.blocks, program.initial);
        assert_eq!(projection.comparator_outputs[3], 7);
    }
}
