//! Shared inputs, progress reporting and notification order for compiler passes.
use super::{
    ScratchEntry, ScratchError,
    pistons::{PistonActor, PistonToken},
    program::RepeaterRecord,
};
use crate::{redpilator::TaskMonitor, world::World};
use mchprs_blocks::{
    BlockPos,
    blocks::{Block, ButtonFace, LeverFace},
};
use std::time::Instant;

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
    /// component IDs of external lever/button/pressure-plate input sources.
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

pub(super) struct CompilePhases<'a> {
    monitor: Option<&'a TaskMonitor>,
    /// enables phase timing and progress diagnostics.
    pub(super) enabled: bool,
    /// start time for the entire compilation.
    total: Instant,
    /// start time for the current phase.
    started: Instant,
    /// label of the current compilation phase.
    phase: &'static str,
    /// last detailed stage and its source/component index.
    detail: Option<(&'static str, usize)>,
}

impl<'a> CompilePhases<'a> {
    pub(super) fn new(monitor: Option<&'a TaskMonitor>) -> Self {
        if let Some(monitor) = monitor {
            monitor.set_phase("scratch snapshot");
        }
        let now = Instant::now();
        Self {
            monitor,
            enabled: std::env::var_os("MCHPRS_REDPILATOR_PHASE_TIMINGS").is_some(),
            total: now,
            started: now,
            phase: "scratch snapshot",
            detail: None,
        }
    }

    pub(super) fn next(&mut self, phase: &'static str) {
        if let Some(monitor) = self.monitor {
            monitor.set_phase(phase);
        }
        if self.enabled {
            eprintln!("Redpilator compile phase {}: {} ms", self.phase, self.started.elapsed().as_millis());
        }
        self.phase = phase;
        self.started = Instant::now();
        self.detail = None;
    }

    fn detail(&mut self, detail: &'static str, item: usize) {
        if let Some(monitor) = self.monitor {
            monitor.set_detail(detail, item);
        }
        self.detail = Some((detail, item));
    }
}

impl Drop for CompilePhases<'_> {
    fn drop(&mut self) {
        if self.enabled {
            let detail = self
                .detail
                .map_or_else(String::new, |(step, item)| format!("; last step {step}, source/component {item}"));
            eprintln!(
                "Redpilator compile phase {}{}: {} ms; total {} ms",
                self.phase,
                detail,
                self.started.elapsed().as_millis(),
                self.total.elapsed().as_millis()
            );
        }
    }
}

pub(super) fn check_cancelled(monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
    if monitor.is_some_and(TaskMonitor::cancelled) {
        Err(ScratchError {
            position: None,
            reason: "compilation cancelled",
        })
    } else {
        Ok(())
    }
}

fn update_order() -> [mchprs_blocks::BlockFace; 6] {
    use mchprs_blocks::BlockFace::*;
    [West, East, Bottom, Top, North, South]
}

/// Return notification targets in the order used by the native redstone updates.
fn update_footprint(pos: BlockPos, block: Block) -> Vec<BlockPos> {
    use mchprs_blocks::BlockFace;
    let neighbours = |pos: BlockPos| update_order().map(|face| pos.offset(face));
    let directional_targets = |rear: BlockFace| {
        let front = pos.offset(rear.opposite());
        std::iter::once(front)
            .chain(update_order().into_iter().filter(|&face| face != rear).map(|face| front.offset(face)))
            .collect()
    };
    match block {
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. } => {
            let rear = match block {
                Block::RedstoneRepeater { repeater } => repeater.facing.block_face(),
                Block::RedstoneComparator { comparator } => comparator.facing.block_face(),
                _ => unreachable!(),
            };
            directional_targets(rear)
        }
        Block::Observer { observer } => directional_targets(observer.facing.into()),
        Block::Lever { lever } => {
            let support = match lever.face {
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            neighbours(pos).into_iter().chain(neighbours(pos.offset(support))).collect()
        }
        Block::StoneButton { button } => {
            let support = match button.face {
                ButtonFace::Floor => BlockFace::Bottom,
                ButtonFace::Ceiling => BlockFace::Top,
                ButtonFace::Wall => button.facing.opposite().block_face(),
            };
            neighbours(pos).into_iter().chain(neighbours(pos.offset(support))).collect()
        }
        Block::Piston { .. } => neighbours(pos).to_vec(),
        block if block.pressure_plate_powered().is_some() => neighbours(pos).into_iter().chain(neighbours(pos.offset(BlockFace::Bottom))).collect(),
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => {
            let mut positions = Vec::new();
            crate::redstone::surrounding_notifications(pos, |target, _, _| positions.push(target));
            positions
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "../../tests/sampled/passes.rs"]
mod tests;
