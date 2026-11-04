//! Sampled compiled redstone execution.

#[cfg(test)]
#[path = "tests/classic_fuzz.rs"]
mod classic_fuzz_tests;
mod compiler;
mod options;
#[cfg(test)]
#[path = "tests/publication.rs"]
mod publication_tests;
mod sampled;
mod scratch;
mod task_monitor;
#[cfg(test)]
#[path = "tests/compiler.rs"]
mod tests;
#[cfg(test)]
#[path = "tests/cache.rs"]
mod cache_tests;

pub use crate::world::scheduler::{Queues, ScheduledBlockTick, TickScheduler};
pub use compiler::{CompileError, CompileStatistics, Compiler, ExecutionError};
pub use options::{CompilerOptions, PublicationMode};
pub(crate) use sampled::Program as CachedProgram;
pub use task_monitor::TaskMonitor;

pub(crate) fn initialize_llvm_cache() -> Result<(), String> {
    sampled::initialize_cache()
}
