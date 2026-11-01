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
    /// immutable source world being compiled.
    pub(super) world: &'a W,
    /// inclusive minimum and maximum bounds of the captured compilation region.
    pub(super) bounds: (BlockPos, BlockPos),
    /// scheduled ticks and piston state captured at the compile boundary.
    pub(super) entry: &'a ScratchEntry,
    /// cancellation handle for long-running compilation work.
    pub(super) monitor: Option<&'a TaskMonitor>,
}

pub(super) struct Inventory {
    /// scanned component world positions; each position's vector index is its component ID.
    pub(super) positions: Vec<BlockPos>,
    /// component IDs of external lever/button input sources.
    sources: Vec<usize>,
    /// discovered repeater metadata keyed by each record's component ID.
    repeaters: Vec<RepeaterRecord>,
    /// copper-bulb positions found during the scan.
    copper_bulbs: Vec<BlockPos>,
    /// redstone-wire positions found during the scan.
    wire_positions: Vec<BlockPos>,
    /// redstone-lamp positions found during the scan.
    lamp_positions: Vec<BlockPos>,
    /// hopper and iron-trapdoor positions needing lamp-like input discovery.
    hopper_positions: Vec<BlockPos>,
    /// note-block positions needing input and notification discovery.
    note_positions: Vec<BlockPos>,
    /// piston actors discovered in the captured region.
    pistons: Vec<PistonActor>,
    /// movable blocks and legal cells discovered for piston groups.
    tokens: Vec<PistonToken>,
}

#[cfg(test)]
mod tests;
