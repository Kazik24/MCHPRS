//! Export execution state before resetting or suspending a compiler.
use super::{Compiler, ExecutionError};
use crate::{
    redstone,
    world::{World, for_each_block_mut_optimized},
};
use mchprs_blocks::BlockPos;

impl Compiler {
    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Err(error) = self.try_reset(world, bounds) {
            tracing::error!(%error, "compiled reset failed; compiler state retained");
        }
    }

    pub fn try_reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), ExecutionError> {
        self.export_for_reset(world)?;
        self.runtime = None;
        self.llvm_runtime = None;
        self.exported_motions.clear();
        if self.options.update {
            let (first_pos, second_pos) = bounds;
            for_each_block_mut_optimized(world, first_pos, second_pos, |world, pos| {
                let block = world.get_block(pos);
                redstone::update(block, world, pos, None);
            });
        }
        self.options = Default::default();
        Ok(())
    }

    /// Export reset state while retaining all topology, execution and publication bookkeeping.
    pub(crate) fn try_suspend<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<Option<Self>, ExecutionError> {
        if !self.is_active() || self.options.update {
            self.try_reset(world, bounds)?;
            return Ok(None);
        }
        self.export_for_reset(world)?;
        Ok(Some(std::mem::take(self)))
    }

    fn export_for_reset<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        if self.runtime.is_some() {
            self.publish(world, false, true)?;
            let runtime = self.runtime.as_ref().unwrap();
            #[cfg(feature = "redpilator-trace")]
            runtime.log_routing_metrics();
            let pending = runtime.pending_entries();
            for entry in pending {
                world.restore_scheduled_tick(entry);
            }
        }
        Ok(())
    }
}
