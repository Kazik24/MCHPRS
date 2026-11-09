//! Private sampled reference compiler and runtime.
#![allow(dead_code)]
#[cfg(test)]
#[path = "../tests/sampled/adder.rs"]
mod adder_tests;
#[cfg(test)]
#[path = "../tests/sampled/admission.rs"]
mod admission_tests;
mod commands;
mod compiler;
#[cfg(test)]
#[path = "../tests/sampled/cpu_trace.rs"]
mod cpu_trace;
mod diagnostics;
mod executor;
mod group_probe;
#[cfg(test)]
#[path = "../tests/sampled/incremental_input.rs"]
mod incremental_input_tests;
mod input;
mod llvm;
mod movement;
mod passes;
#[cfg(test)]
#[path = "../tests/sampled/piston_bud_repro.rs"]
mod piston_bud_repro;
#[cfg(test)]
#[path = "../tests/sampled/piston_head_notifications.rs"]
mod piston_head_notifications;
mod pistons;
#[cfg(test)]
#[path = "../tests/sampled/pm1_causal.rs"]
mod pm1_causal;
mod publication;
#[cfg(test)]
#[path = "../tests/sampled/redstonedb.rs"]
mod redstonedb_tests;
mod routing;
mod shapes;
mod source_cache;
pub(super) use llvm::clear_cache;
pub(super) use llvm::initialize_cache;
pub(in crate::redpilator) use llvm::{LlvmRuntime, unsupported_components};
pub(crate) use source_cache::SourceOrderCache;
mod program;
#[cfg(test)]
#[path = "../tests/sampled/reviewed_fixes.rs"]
mod reviewed_fixes_tests;
#[cfg(test)]
#[path = "../tests/sampled/runtime_index.rs"]
mod runtime_index_tests;
mod scheduler;
#[cfg(test)]
#[path = "../tests/sampled/core.rs"]
mod tests;

use super::scratch::ScratchWorld;
pub(super) use super::scratch::{ScratchEntry, ScratchError};
#[cfg(test)]
use crate::world::World;
#[cfg(test)]
use compiler::compile;
#[cfg(test)]
pub(super) use executor::start_runtime;
pub(super) use executor::{Runtime, start_shared_runtime};
#[cfg(test)]
use mchprs_blocks::blocks::Block;
#[cfg(test)]
use pistons::{PistonActor, assign_piston_groups, discover_piston_actors};
pub(crate) use program::Program;
#[cfg(test)]
use program::WatchedWire;
use program::{ElectricalLink, Operation};
pub(super) use publication::{ExportKey, MovingPart};

#[cfg(test)]
pub(super) use compiler::compile_monitored as compile_program;
pub(super) use compiler::compile_monitored_with_source_cache as compile_program_with_source_cache;

#[cfg(test)]
use mchprs_blocks::{BlockFacing, BlockPos, blocks::LeverFace};
