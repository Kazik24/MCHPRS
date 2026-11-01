use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::blocks::{ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstonePiston};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos, block_entities::BlockEntity, blocks::Block};
use std::sync::Arc;

fn piston_world() -> (
    PlotWorld,
    BlockPos,
    BlockPos,
    BlockPos,
    BlockPos,
    BlockPos,
    BlockPos,
    (BlockPos, BlockPos),
) {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let comparator = far.offset(BlockFace::South);
    let lamp = comparator.offset(BlockFace::South);
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(home, Block::RedstoneBlock {});
    world.set_block(
        comparator,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::North, ComparatorMode::Compare, false),
        },
    );
    world.set_block_entity(comparator, BlockEntity::Comparator { output_strength: 0 });
    world.set_block(comparator.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(24, 20, 20));
    (world, lever, base, home, far, comparator, lamp, bounds)
}

#[test]
fn explicit_publication_modes_select_piston_projection() {
    for (options, full) in [
        (CompilerOptions { publication: Some(PublicationMode::All), ..Default::default() }, true),
        (CompilerOptions { publication: Some(PublicationMode::Redstone), ..Default::default() }, false),
        (CompilerOptions { io_only: true, ..Default::default() }, false),
    ] {
        let (mut world, lever, base, home, far, _, _, bounds) = piston_world();
        let original = [base, home, far].map(|pos| world.get_block(pos));
        let mut compiler = Compiler::default();
        compiler.compile(&world, bounds, options, Vec::new(), Arc::new(TaskMonitor::default())).unwrap();
        compiler.try_on_use_block(lever).unwrap();
        for _ in 0..4 { compiler.tick().unwrap(); }
        compiler.try_flush(&mut world).unwrap();
        assert!(matches!(world.get_block(lever), Block::Lever { lever } if lever.powered));
        assert_eq!([base, home, far].map(|pos| world.get_block(pos)) != original, full);
        compiler.try_reset(&mut world, bounds).unwrap();
    }
}

#[test]
fn effects_can_be_drained_without_publishing_blocks() {
    let (mut world, lever, _, _, _, _, _, bounds) = piston_world();
    let note = lever.offset(BlockFace::North);
    world.set_block(note, Block::NoteBlock { instrument: mchprs_blocks::blocks::Instrument::Harp, note: 1, powered: false });
    let mut compiler = Compiler::default();
    compiler.compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default())).unwrap();
    compiler.try_on_use_block(lever).unwrap();
    assert_eq!(compiler.runtime.as_ref().unwrap().notes.len(), 1);
    compiler.flush_effects(&mut world);
    assert!(compiler.runtime.as_ref().unwrap().notes.is_empty());
    assert!(matches!(world.get_block(lever), Block::Lever { lever } if !lever.powered));
    compiler.try_flush(&mut world).unwrap();
    assert!(matches!(world.get_block(lever), Block::Lever { lever } if lever.powered));
}

#[cfg(feature = "redpiler-full-publication")]
fn assert_publication_matches_runtime(compiler: &Compiler, world: &PlotWorld, moving_cells: &[BlockPos], comparator: BlockPos) {
    let runtime = compiler.runtime.as_ref().expect("compiled runtime active");
    for (pos, expected) in runtime.projected_blocks() {
        assert_eq!(world.get_block(pos), expected, "dirty/full projection disagrees at {pos:?}");
    }
    for &pos in moving_cells {
        match runtime.project(pos) {
            Some(expected) => assert_eq!(world.get_block(pos), expected, "projection at {pos:?}"),
            None => assert!(
                matches!(world.get_block(pos), Block::MovingPiston { .. }),
                "runtime marks {pos:?} as in motion but publication is {:?}",
                world.get_block(pos)
            ),
        }
    }
    let expected_comparator = runtime
        .comparator_outputs()
        .into_iter()
        .find_map(|(pos, output)| (pos == comparator).then_some(output))
        .expect("test circuit comparator is compiled");
    assert!(matches!(
        world.get_block_entity(comparator),
        Some(BlockEntity::Comparator { output_strength }) if *output_strength == expected_comparator
    ));
}

#[test]
#[cfg(not(feature = "redpiler-full-publication"))]
fn redstone_flush_hides_piston_changes_and_reset_restores_them() {
    let (mut world, lever, base, home, far, _comparator, _lamp, bounds) = piston_world();
    let original = [base, home, far].map(|pos| world.get_block(pos));
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    for _ in 0..4 {
        compiler.tick().unwrap();
        compiler.try_flush(&mut world).unwrap();
        assert_eq!([base, home, far].map(|pos| world.get_block(pos)), original);
        assert!(world.piston_state().motions.is_empty());
    }
    assert!(matches!(world.get_block(lever), Block::Lever { lever } if lever.powered));
    let projection = compiler.runtime.as_ref().unwrap().projected_blocks();
    compiler.try_reset(&mut world, bounds).unwrap();
    for (pos, expected) in projection {
        assert_eq!(world.get_block(pos), expected, "reset projection at {pos:?}");
    }
}

#[test]
#[cfg(not(feature = "redpiler-full-publication"))]
fn redstone_flush_then_reset_exports_active_motion() {
    let (mut world, lever, _base, _home, _far, _comparator, _lamp, bounds) = piston_world();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    assert!(compiler.runtime.as_ref().unwrap().has_motions());
    compiler.try_flush(&mut world).unwrap();
    assert!(world.piston_state().motions.is_empty());
    compiler.try_reset(&mut world, bounds).unwrap();
    assert!(!world.piston_state().motions.is_empty());
}

#[test]
fn flush_consumes_queued_note_once() {
    let (mut world, lever, _base, _home, _far, _comparator, _lamp, bounds) = piston_world();
    let note = lever.offset(BlockFace::North);
    world.set_block(note, Block::NoteBlock { instrument: mchprs_blocks::blocks::Instrument::Harp, note: 1, powered: false });
    let mut compiler = Compiler::default();
    compiler.compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default())).unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.try_on_use_block(lever).unwrap();
    assert_eq!(compiler.runtime.as_ref().unwrap().notes.len(), 1);
    compiler.try_flush(&mut world).unwrap();
    assert!(compiler.runtime.as_ref().unwrap().notes.is_empty());
    compiler.try_flush(&mut world).unwrap();
    assert!(compiler.runtime.as_ref().unwrap().notes.is_empty());
}

#[test]
fn failed_publication_retains_pending_note_for_reset() {
    let (mut world, lever, _base, _home, _far, _comparator, _lamp, bounds) = piston_world();
    let note = lever.offset(BlockFace::North);
    world.set_block(note, Block::NoteBlock { instrument: mchprs_blocks::blocks::Instrument::Harp, note: 1, powered: false });
    let mut compiler = Compiler::default();
    compiler.compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default())).unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    assert!(compiler.runtime.as_ref().unwrap().has_motions());
    world.piston_state_mut().next_identity = u64::MAX;
    assert!(compiler.try_flush_complete(&mut world).is_err());
    assert_eq!(compiler.runtime.as_ref().unwrap().notes.len(), 1);
    world.piston_state_mut().next_identity = 0;
    compiler.try_reset(&mut world, bounds).unwrap();
    assert!(!compiler.is_active());
}

#[test]
#[cfg(feature = "redpiler-full-publication")]
fn dirty_publication_matches_full_projection_through_sticky_piston_lifecycle() {
    let (mut world, lever, base, home, far, comparator, lamp, bounds) = piston_world();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    let tracked = [lever, base, home, far, comparator, lamp];

    compiler.try_flush(&mut world).unwrap();
    assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);
    compiler.try_flush(&mut world).unwrap();
    assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);

    // Pulse during the first extension to exercise interruption before landing.
    for (tick, input) in [(1, Some(true)), (2, Some(false)), (3, None), (4, None), (5, None)] {
        if let Some(powered) = input {
            let current = matches!(world.get_block(lever), Block::Lever { lever } if lever.powered);
            assert_ne!(current, powered, "requested lever edge at tick {tick}");
            compiler.try_on_use_block(lever).unwrap();
        }
        compiler.tick().unwrap();
        compiler.try_flush(&mut world).unwrap();
        assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);
        compiler.try_flush(&mut world).unwrap();
        assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);
    }

    // Rearm after the interrupted motion has drained, extend, then sticky retract.
    for (tick, input) in [
        (6, Some(true)),
        (7, None),
        (8, None),
        (9, None),
        (10, Some(false)),
        (11, None),
        (12, None),
        (13, None),
    ] {
        if let Some(powered) = input {
            let current = matches!(world.get_block(lever), Block::Lever { lever } if lever.powered);
            assert_ne!(current, powered, "requested lever edge at tick {tick}");
            compiler.try_on_use_block(lever).unwrap();
        }
        compiler.tick().unwrap();
        compiler.try_flush(&mut world).unwrap();
        assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);
        compiler.try_flush(&mut world).unwrap();
        assert_publication_matches_runtime(&compiler, &world, &tracked, comparator);
    }
}

#[test]
#[cfg(feature = "redpiler-full-publication")]
fn repeated_unchanged_motion_flush_keeps_identity_and_carried_entity() {
    let (mut world, lever, _base, _home, _far, _comparator, _lamp, bounds) = piston_world();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    compiler.try_flush(&mut world).unwrap();
    assert!(!world.piston_state().motions.is_empty(), "initial flush must export active motion");

    let injected_entity = Box::new(BlockEntity::Comparator { output_strength: 11 });
    let state = world.piston_state_mut();
    let carried = state.motions.front_mut().unwrap();
    carried.carried_entity = Some(injected_entity.clone());
    let identities: Vec<_> = state.motions.iter().map(|motion| (motion.pos, motion.identity)).collect();
    let next_identity = state.next_identity;

    compiler.try_flush(&mut world).unwrap();

    let state = world.piston_state();
    assert_eq!(state.next_identity, next_identity, "unchanged parts must not allocate new identities");
    assert_eq!(
        state.motions.iter().map(|motion| (motion.pos, motion.identity)).collect::<Vec<_>>(),
        identities,
        "unchanged parts keep their identities and order"
    );
    assert_eq!(
        state
            .motions
            .front()
            .unwrap()
            .carried_entity
            .as_deref()
            .map(|entity| format!("{entity:?}")),
        Some(format!("{injected_entity:?}"))
    );
}

#[test]
fn io_only_flush_then_reset_restores_full_extension_and_retraction_projection() {
    let (mut world, lever, _base, _home, _far, comparator, _lamp, bounds) = piston_world();
    let mut compiler = Compiler::default();

    for (powered, expected_phase) in [(true, "extension"), (false, "retraction")] {
        compiler
            .compile(
                &world,
                bounds,
                CompilerOptions {
                    io_only: true,
                    ..Default::default()
                },
                Vec::new(),
                Arc::new(TaskMonitor::default()),
            )
            .unwrap();
        compiler.try_flush(&mut world).unwrap();
        compiler.try_flush(&mut world).unwrap();
        let current = matches!(world.get_block(lever), Block::Lever { lever } if lever.powered);
        assert_ne!(current, powered, "the {expected_phase} control edge toggles the lever");
        compiler.try_on_use_block(lever).unwrap();
        for _ in 0..4 {
            compiler.tick().unwrap();
            compiler.try_flush(&mut world).unwrap();
            compiler.try_flush(&mut world).unwrap();
        }

        let runtime = compiler.runtime.as_ref().unwrap();
        assert!(!runtime.has_motions(), "{expected_phase} reaches a settled snapshot before reset");
        let projected = runtime.projected_blocks();
        let comparator_outputs = runtime.comparator_outputs();
        compiler.try_reset(&mut world, bounds).unwrap();
        assert!(!compiler.is_active());
        for (pos, expected) in projected {
            assert_eq!(world.get_block(pos), expected, "full reset projection after {expected_phase} at {pos:?}");
        }
        let expected_comparator = comparator_outputs
            .into_iter()
            .find_map(|(pos, output)| (pos == comparator).then_some(output))
            .expect("comparison circuit has its comparator");
        assert!(matches!(
            world.get_block_entity(comparator),
            Some(BlockEntity::Comparator { output_strength }) if *output_strength == expected_comparator
        ));
    }
}
