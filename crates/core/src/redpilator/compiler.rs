//! Own the active executors and expose the plot-facing compiler API.
mod compilation;
mod diagnostics;
mod execution;
mod lifecycle;
mod publication;

pub use diagnostics::{CompileError, CompileStatistics, ExecutionError};
use publication::ExportedMotion;

#[cfg(feature = "plot-bench")]
use super::TaskMonitor;
use super::{
    CompilerOptions,
    sampled::{ExportKey, LlvmRuntime, MovingPart, Runtime},
};
#[cfg(feature = "plot-bench")]
use crate::world::World;
use mchprs_blocks::BlockPos;
#[cfg(feature = "plot-bench")]
use mchprs_world::TickEntry;
use std::collections::{HashMap, HashSet};
#[cfg(feature = "plot-bench")]
use std::sync::Arc;

#[derive(Default)]
pub struct Compiler {
    /// Rust executor, or a read-only publication projection of the LLVM executor.
    pub(super) runtime: Option<Runtime>,
    /// active experimental LLVM executor; ticks use it when present.
    pub(super) llvm_runtime: Option<LlvmRuntime>,
    /// publication/reset options used by the current compilation.
    options: CompilerOptions,
    /// most recent successful compile statistics.
    statistics: Option<CompileStatistics>,
    /// host moving-piston identities tracked between publications.
    exported_motions: HashMap<ExportKey, ExportedMotion>,
    /// moving piston block parts queued for host-world publication.
    publication_parts: Vec<MovingPart>,
    /// positions already included in the current publication.
    publication_positions: HashSet<BlockPos>,
    /// host motion identities retired during reconciliation.
    retired_motions: HashSet<u64>,
    /// moving-piston positions mapped to host motion indices.
    host_motion_indices: HashMap<BlockPos, usize>,
    /// indicates a publication reconciliation pass is in progress.
    publication_phase: bool,
}

impl Compiler {
    /// Experimental benchmark helper: the unticked seed and pristine world must describe the same fixture.
    #[cfg(feature = "plot-bench")]
    pub fn benchmark_restart<W: World>(&self, pristine: &W, bounds: (BlockPos, BlockPos), ticks: Vec<TickEntry>) -> Result<Self, CompileError> {
        let mut fresh = Self::default();
        fresh.compile_cached(
            pristine,
            bounds,
            self.options.clone(),
            ticks,
            Arc::new(TaskMonitor::default()),
            self.cached_program(),
        )?;
        Ok(fresh)
    }

    #[cfg(feature = "plot-bench")]
    pub fn benchmark_tick_batch(&mut self, count: u32, mode: u32) -> Result<(), ExecutionError> {
        self.llvm_runtime
            .as_mut()
            .expect("batch experiment requires LLVM")
            .benchmark_tick_batch(count, mode)
            .map_err(ExecutionError)
    }

    #[cfg(feature = "plot-bench")]
    pub fn benchmark_snapshot(&self) -> Result<Vec<u32>, ExecutionError> {
        self.llvm_runtime
            .as_ref()
            .expect("batch experiment requires LLVM")
            .benchmark_snapshot()
            .map_err(ExecutionError)
    }

    pub fn stats(&self) -> Option<&CompileStatistics> {
        self.statistics.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.runtime.is_some() || self.llvm_runtime.is_some()
    }

    pub(crate) fn uses_llvm(&self) -> bool {
        self.llvm_runtime.is_some()
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.is_active().then_some(&self.options)
    }
}
