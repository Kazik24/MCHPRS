//! Private sampled reference compiler and runtime.
#![allow(dead_code)]
mod compiler;
mod executor;
mod group_probe;
mod program;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "sampled/adder_tests.rs"]
mod adder_tests;

#[cfg(test)]
use compiler::{assign_piston_groups, compile, discover_piston_actors, PistonActor};
use executor::Runtime;
use program::{ElectricalLink, Operation, Program};
#[cfg(test)]
use program::WatchedWire;
use super::{backend::ScheduledBlockTick, scratch::{ScratchEntry, ScratchError, ScratchWorld}};
#[cfg(test)]
use crate::world::World;
#[cfg(test)]
use mchprs_blocks::{BlockFacing, BlockPos, blocks::{Block, LeverFace}};
