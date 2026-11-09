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
        let status = unsafe {
            (self.library.read_components)(
                self.state.as_ptr().cast(),
                self.io_components.as_ptr(),
                self.io_states.as_mut_ptr(),
                self.io_components.len() as u32,
            )
        };
        self.call_status(status)?;
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
        Ok(())
    }

    pub(in crate::redpilator) fn sync_effects(&mut self, runtime: &mut Runtime) -> Result<(), ScratchError> {
        let invalid = || ScratchError {
            position: None,
            reason: "invalid LLVM effect state",
        };
        let count = unsafe { (self.library.note_count)(self.state.as_ptr().cast()) } as usize;
        if count > self.initial.len() || runtime.note_queued.len() != self.initial.len() || runtime.program.initial.len() != self.initial.len() {
            return Err(invalid());
        }
        let words = count.checked_mul(3).ok_or_else(invalid)?;
        if words > self.note_events.len() {
            let additional = words - self.note_events.len();
            super::memory::reserve(&mut self.note_events, additional)?;
            self.note_events.resize(words, 0);
        }
        let status = unsafe { (self.library.read_notes)(self.state.as_ptr().cast(), self.note_events.as_mut_ptr(), count as u32) };
        self.call_status(status)?;
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
        self.sync_commands(runtime)?;
        runtime.motion_time = unsafe { (self.library.tick_count)(self.state.as_ptr().cast()) };
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
