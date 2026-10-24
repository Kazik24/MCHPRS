//! Frame-boundary external input scheduling.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Block, ButtonFace, StoneButton};
use std::sync::Arc;

const BUTTON: BlockPos = BlockPos::new(5, 20, 5);

fn button_backend() -> GraphBackend {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(
        BUTTON,
        Block::StoneButton {
            button: StoneButton::new(ButtonFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    GraphBackend::compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)), vec![], Arc::default()).unwrap()
}

#[test]
fn frame_button_release_matches_native_scheduler_delay_without_callbacks() {
    let mut frame = button_backend();
    let mut ordinary = button_backend();
    let button = frame.positions[&BUTTON];
    assert_eq!(ordinary.positions[&BUTTON], button);

    // Logical-frame button activation suppresses route delivery and uses the shared timer queue.
    frame.schedule(button, 10, TickPriority::Normal);
    frame.set_node(button, true, 15, None, false).unwrap();
    ordinary.schedule(button, 10, TickPriority::Normal);
    ordinary.set_node(button, true, 15, None, false).unwrap();

    let boundary_buttons = [button];
    for tick in 1..=20 {
        frame.future.clock.logical_tick += 1;
        frame.advance_frame_scheduler(button, &boundary_buttons).unwrap();
        let mut host = None;
        ordinary.tick_phases(&mut host).unwrap();

        let remains_powered = tick < 20;
        assert_eq!(frame.node_state(button).powered, remains_powered, "frame button at tick {tick}");
        assert_eq!(
            frame.node_state(button).powered,
            ordinary.node_state(button).powered,
            "ordinary button at tick {tick}"
        );
        assert_eq!(
            frame.future.scheduler.iter().collect::<Vec<_>>(),
            ordinary.future.scheduler.iter().collect::<Vec<_>>()
        );
        assert_eq!(frame.future.clock.phase, ordinary.future.clock.phase, "scheduler phase at tick {tick}");
        assert_eq!(
            frame.future.clock.scheduled_advanced, ordinary.future.clock.scheduled_advanced,
            "advance flag at tick {tick}"
        );
    }

    assert!(frame.future.scheduler.iter().next().is_none());
    assert!(frame.future.pending.is_empty());
    assert_eq!(frame.counts.scheduled_ticks, 1);
    assert_eq!(frame.counts.callbacks, 0);
    assert_eq!(frame.future.clock.phase, GraphPhase::BetweenTicks);
    assert!(!frame.future.clock.scheduled_advanced);
}
