//! Compilation stages, called directly from `compile_monitored`.
use super::{
    CompilePhases, PistonActor, PistonToken, check_cancelled, discover_piston_actors_monitored, discover_piston_tokens, update_footprint,
    update_order,
};
use crate::redpiler::TaskMonitor;
use crate::redpiler::sampled::group_probe::ProbeIndexes;
use crate::redpiler::sampled::program::{ArrangementSource, ComparatorInput, RepeaterRecord, WatchedWire};
use crate::redpiler::sampled::{ElectricalLink, Operation, Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::HashMap;

pub(super) mod arrangements;
pub(super) mod electrical;
pub(super) mod finalize;
pub(super) mod movement;
pub(super) mod parse;
pub(super) mod reactive;
pub(super) mod share_output_programs;
pub(super) mod source_order;

pub(super) struct Context<'a, W: World> {
    pub(super) world: &'a W,
    pub(super) bounds: (BlockPos, BlockPos),
    pub(super) entry: &'a ScratchEntry,
    pub(super) monitor: Option<&'a TaskMonitor>,
}

pub(super) struct Inventory {
    pub(super) positions: Vec<BlockPos>,
    sources: Vec<usize>,
    repeaters: Vec<RepeaterRecord>,
    copper_bulbs: Vec<BlockPos>,
    wire_positions: Vec<BlockPos>,
    lamp_positions: Vec<BlockPos>,
    hopper_positions: Vec<BlockPos>,
    note_positions: Vec<BlockPos>,
    pistons: Vec<PistonActor>,
    tokens: Vec<PistonToken>,
}

#[cfg(test)]
mod tests;
