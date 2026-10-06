pub mod direct;
mod queue;
pub use queue::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    InstantRuntimeUnavailable,
    InvalidInstantProgram,
    MissingInstantBinding {
        pos: mchprs_blocks::BlockPos,
    },
    InvalidStrength {
        pos: Option<mchprs_blocks::BlockPos>,
        strength: u8,
    },
    TooManyInputs {
        pos: Option<mchprs_blocks::BlockPos>,
        default_inputs: usize,
        side_inputs: usize,
    },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InstantRuntimeUnavailable => {
                f.write_str("instant graph boundaries require the region runtime")
            }
            Self::InvalidInstantProgram => f.write_str("instant program contains unresolved actuator variables"),
            Self::MissingInstantBinding { pos } => write!(f, "instant port at {pos:?} was lost during graph preparation"),
            Self::InvalidStrength { pos, strength } => write!(f, "graph node at {pos:?} has invalid strength {strength}; expected 0..15"),
            Self::TooManyInputs { pos, default_inputs, side_inputs } => write!(f, "graph node at {pos:?} has {default_inputs} main and {side_inputs} side inputs; each channel supports at most 255"),
        }
    }
}
impl std::error::Error for BackendError {}

use std::sync::Arc;

use super::compile_graph::CompileGraph;
use super::task_monitor::TaskMonitor;
use super::CompilerOptions;
use crate::world::World;
use enum_dispatch::enum_dispatch;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;

#[enum_dispatch]
pub trait JITBackend {
    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), BackendError>;
    fn tick(&mut self);
    fn on_use_block(&mut self, pos: BlockPos);
    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool);
    fn flush<W: World>(&mut self, world: &mut W, io_only: bool);
    fn reset<W: World>(&mut self, world: &mut W, io_only: bool);
    /// Inspect block for debugging
    fn inspect(&mut self, pos: BlockPos);
}

use direct::DirectBackend;

#[enum_dispatch(JITBackend)]
pub enum BackendDispatcher {
    DirectBackend,
}
