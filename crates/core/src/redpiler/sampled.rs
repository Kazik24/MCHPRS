//! Private sampled MAIR compiler and runtime.
mod compiler;
mod executor;
mod program;
#[cfg(test)]
mod tests;

use compiler::{assign_piston_groups, compile, discover_piston_actors, PistonActor};
use executor::Runtime;
use program::{ElectricalLink, Operation, Program, WatchedWire};
use super::{backend::ScheduledBlockTick, scratch::{ScratchEntry, ScratchError, ScratchWorld}, TickScheduler};
use crate::world::{RedstoneUpdateObservation, World};
use mchprs_blocks::{BlockFacing, BlockPos, blocks::{Block, LeverFace}};
use mchprs_world::TickEntry;