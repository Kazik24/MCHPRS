//! Frame-boundary external input scheduling.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::CommandBlockEntity;
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
        let mut host: Option<&mut dyn GraphCommandHost> = None;
        frame.advance_frame_scheduler(button, &boundary_buttons, &mut host).unwrap();
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

#[test]
fn frame_command_outputs_use_shared_edges_scheduler_and_entity_updates() {
    let first = BlockPos::new(8, 20, 5);
    let chain = first.offset(mchprs_blocks::BlockFace::East);
    let make_world = || {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        for (position, name, command, automatic) in [
            (first, "command_block", "say frame", false),
            (chain, "chain_command_block", r#"tellraw @a {"text":"chain"}"#, true),
        ] {
            let mut block = Block::from_name(name).unwrap();
            block.set_properties(std::collections::HashMap::from([("facing", "east")]));
            world.set_block(position, block);
            world.set_block_entity(
                position,
                BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                    command: command.into(),
                    automatic,
                    ..Default::default()
                })),
            );
        }
        world
    };
    let mut world = make_world();
    let mut backend = GraphBackend::compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)), vec![], Arc::default()).unwrap();
    let first_node = backend.positions[&first];
    let chain_node = backend.positions[&chain];

    // The settled output adapter delegates to the same edge logic as ordinary Main input.
    backend.publish_frame_output(first_node, 15).unwrap();
    backend.publish_frame_output(first_node, 15).unwrap();
    assert_eq!(backend.scheduled_ticks().len(), 1);
    let pending_before = backend.scheduled_ticks();
    let clock_before = backend.future.clock.logical_tick;
    assert_eq!(backend.tick(), Err(GraphExecutionError::CommandHostUnavailable { position: first }));
    assert_eq!(backend.scheduled_ticks(), pending_before);
    assert_eq!(backend.future.clock.logical_tick, clock_before);
    assert!(matches!(
        backend.node_state(first_node).command_entity.as_ref(),
        Some(entity) if entity.powered
    ));
    let run_frame = |backend: &mut GraphBackend, world: &mut PlotWorld| {
        backend.future.clock.logical_tick += 1;
        backend.active_frame_command_outputs.push(first_node);
        backend.active_frame_command_outputs.push(chain_node);
        let mut host: Option<&mut dyn GraphCommandHost> = Some(world);
        host.as_mut().unwrap().publish_logical_tick(backend.future.clock.logical_tick);
        backend.advance_frame_scheduler(first_node, &[], &mut host).unwrap();
        backend.active_frame_command_outputs.clear();
    };
    run_frame(&mut backend, &mut world);
    assert_eq!(
        world.command_output().collect::<Vec<_>>(),
        [r#"{"text":"[@] frame"}"#, r#"{"text":"chain"}"#]
    );
    assert!(matches!(
        world.get_block_entity(first),
        Some(BlockEntity::CommandBlock(entity)) if entity.success_count == 1
    ));
    assert!(matches!(
        world.get_block_entity(chain),
        Some(BlockEntity::CommandBlock(entity)) if entity.success_count == 1
    ));
    assert!(matches!(
        backend.node_state(first_node).command_entity.as_ref(),
        Some(entity) if entity.powered
    ));
    assert!(backend.scheduled_ticks().is_empty(), "one-shot command ticks are fully drained");
    assert_eq!(backend.node_state(first_node).output_strength, 1);

    // A held positive output does not retrigger an impulse block; a fall and rise does.
    backend.publish_frame_output(first_node, 15).unwrap();
    run_frame(&mut backend, &mut world);
    assert_eq!(world.command_output().count(), 2, "held impulse output must not retrigger");
    backend.publish_frame_output(first_node, 0).unwrap();
    backend.publish_frame_output(first_node, 15).unwrap();
    run_frame(&mut backend, &mut world);
    assert_eq!(world.command_output().count(), 4);
    assert!(matches!(
        world.get_block_entity(first),
        Some(BlockEntity::CommandBlock(entity)) if entity.success_count == 1 && entity.last_execution == 3
    ));
    assert!(matches!(
        world.get_block_entity(chain),
        Some(BlockEntity::CommandBlock(entity)) if entity.success_count == 1 && entity.last_execution == 3
    ));
    assert_eq!(backend.node_state(chain_node).output_strength, 1);
}
