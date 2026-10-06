//! Bounded, operation-scoped test capture. No recorder is present in server builds.
use crate::world::World;
use mchprs_blocks::BlockPos;
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
    record(
        world,
        Operation::Sample {
            pos,
            extended,
            powered,
        },
    );
}

pub(crate) fn applied(world: &impl World, event: PistonEvent) {
    record(world, Operation::Applied(event));
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
