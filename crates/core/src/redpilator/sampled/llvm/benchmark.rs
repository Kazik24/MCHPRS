//! Temporary, opt-in entry points for the fixed tick-batch experiment.
use super::{LlvmRuntime, NativePendingTick, ScratchError};

fn invalid() -> ScratchError {
    ScratchError {
        position: None,
        reason: "invalid LLVM batch experiment request or missing experimental DLL",
    }
}

impl LlvmRuntime {
    /// Count zero performs allocation and clock setup outside the measured interval.
    pub(in crate::redpilator) fn benchmark_tick_batch(&mut self, count: u32, mode: u32) -> Result<(), ScratchError> {
        if mode > 3 || (mode == 3 && count > 32) {
            return Err(invalid());
        }
        let batch = self.library.benchmark_tick_batch;
        if mode >= 2 && batch.is_none() {
            return Err(invalid());
        }
        if let Some(commands) = &mut self.commands {
            commands.benchmark_prepare(&self.library.commands, self.state.as_mut_ptr().cast())?;
        }
        if mode == 0 {
            for _ in 0..count {
                self.tick()?;
            }
            return Ok(());
        }
        if mode == 1 {
            for _ in 0..count {
                // SAFETY: Same owned, initialized buffers and ABI as tick; benchmark_prepare preallocates effects.
                let status = unsafe { (self.library.tick)(self.state.as_mut_ptr().cast()) };
                self.call_status(status)?;
            }
            return Ok(());
        }
        // SAFETY: Optional export belongs to this retained experimental module; buffers remain live.
        let status = unsafe { (batch.ok_or_else(invalid)?)(self.state.as_mut_ptr().cast(), count, mode) };
        self.call_status(status)
    }

    /// Canonical pointer-free graph, pending work, notes, command entities and accumulated effects.
    pub(in crate::redpilator) fn benchmark_snapshot(&self) -> Result<Vec<u32>, ScratchError> {
        let engine = self.state.as_ptr().cast();
        // SAFETY: Read-only calls into retained native state, followed by exactly sized allocations.
        let words = unsafe { (self.library.snapshot_words)(engine) };
        if words > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut result = vec![0; words as usize];
        self.call_status(unsafe { (self.library.read_snapshot)(engine, result.as_mut_ptr(), words) })?;
        let pending_count = unsafe { (self.library.pending_count)(engine) };
        if pending_count > self.pending_capacity {
            return Err(invalid());
        }
        let mut pending = vec![NativePendingTick::default(); pending_count as usize];
        self.call_status(unsafe { (self.library.read_pending)(engine, pending.as_mut_ptr(), pending_count) })?;
        result.insert(0, words);
        result.push(pending_count);
        for tick in pending {
            result.extend([tick.component, tick.ticks_left, u32::from(tick.priority)]);
        }
        let notes = unsafe { (self.library.note_count)(engine) };
        if notes as usize > self.positions.len() {
            return Err(invalid());
        }
        let mut note_ids = vec![0; (notes as usize).checked_mul(3).ok_or_else(invalid)?];
        self.call_status(unsafe { (self.library.read_notes)(engine, note_ids.as_mut_ptr(), notes) })?;
        result.push(notes);
        result.extend(note_ids);
        for count in [unsafe { (self.library.tick_count)(engine) }, unsafe { (self.library.work_count)(engine) }] {
            result.extend([count as u32, (count >> 32) as u32]);
        }
        let commands = self
            .commands
            .as_ref()
            .map(|state| state.benchmark_snapshot(&self.library.commands))
            .transpose()?
            .unwrap_or_default();
        result.push(u32::try_from(commands.len()).map_err(|_| invalid())?);
        result.extend(commands);
        Ok(result)
    }
}
