//! Bounded, operation-scoped test capture. No recorder is present in server builds.
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{AdvancePhase, PistonEvent};
use serde::Serialize;
use std::cell::RefCell;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) enum Operation {
    /// Includes unchanged samples: power changes alone are not storage writes.
    Sample {
        pos: BlockPos,
        extended: bool,
        powered: bool,
    },
    Applied(PistonEvent),
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Entry {
    pub tick: u64,
    pub phase: AdvancePhase,
    pub operation: Operation,
}

thread_local! {
    static TRACE: RefCell<Option<Vec<Entry>>> = const { RefCell::new(None) };
    static DETAIL: RefCell<Option<(Vec<BlockPos>, Vec<DetailedEntry>)>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum Stage {
    Sample,
    /// A request attempt, before the world queue deduplicates it.
    Request,
    /// Live revalidation before execution; only Applied establishes success.
    Accept,
    Applied,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DetailedEntry {
    pub tick: u64,
    pub phase: AdvancePhase,
    pub stage: Stage,
    pub pos: BlockPos,
    pub base: u32,
    pub extended: Option<bool>,
    pub powered: bool,
    pub main: bool,
    pub quasi: bool,
    pub main_sources: Vec<BlockPos>,
    pub quasi_sources: Vec<BlockPos>,
    pub front: u32,
    pub far: u32,
    pub event: Option<PistonEvent>,
}

pub(crate) fn detail(world: &impl World, pos: BlockPos, stage: Stage, event: Option<PistonEvent>, powered: bool) {
    DETAIL.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some((positions, entries)) = trace.as_mut() else {
            return;
        };
        if !positions.contains(&pos) {
            return;
        }
        let base = world.get_block(pos);
        let (facing, extended) = match (base, event) {
            (Block::Piston { piston }, _) => (piston.facing, Some(piston.extended)),
            // Retraction replaces the base with a moving source before Applied.
            (_, Some(event)) if stage == Stage::Applied => (event.facing.into(), None),
            _ => return,
        };
        let powered = if stage == Stage::Applied {
            super::should_piston_extend(world, facing, pos)
        } else {
            powered
        };
        let front = facing.into();
        let main_sources: Vec<_> = super::NEIGHBORS
            .into_iter()
            .filter(|&face| face != front && super::powered(world, pos, face))
            .map(|face| pos.offset(face))
            .collect();
        let above = pos.offset(BlockFace::Top);
        let quasi_sources: Vec<_> = super::NEIGHBORS
            .into_iter()
            .filter(|&face| super::powered(world, above, face))
            .map(|face| above.offset(face))
            .collect();
        entries.push(DetailedEntry {
            tick: world.piston_state().logical_tick,
            phase: world.piston_state().phase,
            stage,
            pos,
            base: base.get_id(),
            extended,
            powered,
            main: !main_sources.is_empty(),
            quasi: !quasi_sources.is_empty(),
            main_sources,
            quasi_sources,
            front: world.get_block(pos.offset(front)).get_id(),
            far: world.get_block(pos.offset(front).offset(front)).get_id(),
            event,
        });
    });
}

fn record(world: &impl World, operation: Operation) {
    TRACE.with(|trace| {
        if let Some(entries) = trace.borrow_mut().as_mut() {
            entries.push(Entry {
                tick: world.piston_state().logical_tick,
                phase: world.piston_state().phase,
                operation,
            });
        }
    });
}

pub(crate) fn sample(world: &impl World, pos: BlockPos, extended: bool, powered: bool) {
    crate::redstone::instant_piston_tests::record_operation(
        world,
        "piston_sample",
        serde_json::json!({"pos":pos,"extended":extended,"powered":powered}),
    );
    record(world, Operation::Sample { pos, extended, powered });
    detail(world, pos, Stage::Sample, None, powered);
}

pub(crate) fn applied(world: &impl World, event: PistonEvent) {
    record(world, Operation::Applied(event));
    detail(world, event.pos, Stage::Applied, Some(event), false);
}

/// Separate from the frozen trace format; records live reads only at selected bases.
pub(crate) fn capture_detailed(positions: &[BlockPos], f: impl FnOnce()) -> Vec<DetailedEntry> {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            DETAIL.with(|trace| *trace.borrow_mut() = None);
        }
    }
    DETAIL.with(|trace| {
        let mut trace = trace.borrow_mut();
        assert!(trace.is_none(), "nested detailed piston trace capture");
        *trace = Some((positions.to_vec(), Vec::new()));
    });
    let _guard = Guard;
    f();
    DETAIL.with(|trace| trace.borrow_mut().take().unwrap().1)
}

pub(crate) fn capture(f: impl FnOnce()) -> Vec<Entry> {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            TRACE.with(|trace| *trace.borrow_mut() = None);
        }
    }
    TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        assert!(trace.is_none(), "nested piston trace capture");
        *trace = Some(Vec::new());
    });
    let _guard = Guard;
    f();
    TRACE.with(|trace| trace.borrow_mut().take().unwrap())
}
