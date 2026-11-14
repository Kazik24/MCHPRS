//! Dispatch ticks and external controls to the active backend.
use super::{Compiler, ExecutionError};
use crate::world::World;
use mchprs_blocks::BlockPos;

impl Compiler {
    /// Run one normal native iteration, retaining the completed prefix if a tick fails.
    pub(crate) fn tick_batch(&mut self, count: u32) -> (u32, Result<(), ExecutionError>) {
        let mut completed = 0;
        let result = if let Some(native) = self.llvm_runtime.as_mut() {
            native.tick_batch(count, &mut completed).map_err(ExecutionError)
        } else {
            (|| {
                for _ in 0..count {
                    self.tick()?;
                    completed += 1;
                }
                Ok(())
            })()
        };
        (completed, result)
    }

    pub fn tick(&mut self) -> Result<(), ExecutionError> {
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.tick().map_err(ExecutionError);
        }
        self.runtime.as_mut().expect("compiler inactive").tick().map_err(ExecutionError)
    }

    pub fn tick_with_world<W: World>(&mut self, _world: &mut W) -> Result<(), ExecutionError> {
        self.tick()
    }

    pub(crate) fn refresh_command_queue<W: World>(&mut self, world: &W) -> Result<(), ExecutionError> {
        let Some(runtime) = &mut self.runtime else {
            return Ok(());
        };
        let queued = world.command_block_output_state().map_or(0, |state| state.queued);
        if let Some(native) = &mut self.llvm_runtime {
            native.refresh_command_queue(queued).map_err(ExecutionError)?;
        } else {
            runtime.refresh_command_queue(queued);
        }
        Ok(())
    }

    pub fn on_use_block(&mut self, pos: BlockPos) {
        if let Err(error) = self.try_on_use_block(pos) {
            tracing::error!(%error, "sampled runtime control failed");
        }
    }

    pub fn try_on_use_block(&mut self, pos: BlockPos) -> Result<(), ExecutionError> {
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.use_control(pos).map_err(ExecutionError);
        }
        self.runtime.as_mut().expect("compiler inactive").use_control(pos).map_err(ExecutionError)
    }

    pub fn try_set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), ExecutionError> {
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.set_pressure_plate(pos, powered).map_err(ExecutionError);
        }
        self.runtime
            .as_mut()
            .expect("compiler inactive")
            .set_pressure_plate(pos, powered)
            .map_err(ExecutionError)
    }

    pub(crate) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.runtime.as_ref().is_some_and(|runtime| runtime.accepts_control(pos))
            || self.llvm_runtime.as_ref().is_some_and(|runtime| runtime.accepts_control(pos))
    }
}
