use super::*;
use crate::{plot::PlotWorld, world::World};
use mchprs_blocks::{
    BlockDirection, BlockFace,
    block_entities::BlockEntity,
    blocks::{Lever, LeverFace, RedstoneWire},
};
use mchprs_world::TickPriority;
use std::collections::BTreeMap;

const CONTROLS: [BlockPos; 2] = [BlockPos::new(5, 20, 8), BlockPos::new(40, 20, 8)];
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(3, 18, 4), BlockPos::new(48, 22, 12));

fn wire(world: &mut PlotWorld, pos: BlockPos) {
    world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        pos,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
}

fn fixture() -> PlotWorld {
    let mut world = super::super::tests::host();
    for control in CONTROLS {
        world.set_block(control.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            control,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        for x in 1..=4 {
            wire(&mut world, control + BlockPos::new(x, 0, 0));
        }
        world.set_block(control + BlockPos::new(5, 0, 0), Block::RedstoneLamp { lit: false });
    }
    wire(&mut world, BlockPos::new(7, 20, 9));
    // Disconnected branch reached only after inserting the currently absent bridge at (8,20,7).
    wire(&mut world, BlockPos::new(8, 20, 6));
    world.set_block(BlockPos::new(9, 20, 6), Block::RedstoneLamp { lit: false });
    let sign = BlockPos::new(6, 20, 7);
    world.set_block(sign.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(sign, Block::from_name("oak_sign").unwrap());
    let wires: Vec<_> = (BOUNDS.0.z..=BOUNDS.1.z)
        .flat_map(|z| (BOUNDS.0.x..=BOUNDS.1.x).map(move |x| BlockPos::new(x, 20, z)))
        .filter(|&p| matches!(world.get_block(p), Block::RedstoneWire { .. }))
        .collect();
    for pos in wires {
        let state = crate::redstone::wire::get_state_for_placement(&world, pos);
        world.set_block(pos, Block::RedstoneWire { wire: state });
    }
    // A nonquiet captured entry deliberately retains the existing frame-validation eligibility.
    for control in CONTROLS {
        crate::interaction::use_redstone(world.get_block(control), &mut world, control, None);
    }
    assert!(world.scheduler().iter_entries().next().is_none());
    world
}

fn compile(world: &PlotWorld, previous: Option<&super::super::SourceOrderCache>) -> Program {
    super::super::compiler::compile_monitored_with_source_cache(
        world,
        BOUNDS,
        super::super::ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(),
            piston: world.piston_state().clone(),
        },
        None,
        previous,
    )
    .unwrap()
}

fn reuse(program: &Program) -> (usize, usize) {
    let (hits, misses) = program.source_order_cache.as_ref().expect("source cache captured").reuse_counts();
    (hits as usize, misses as usize)
}

fn canonical(program: &Program) -> BTreeMap<(i32, i32, i32), [Vec<String>; 2]> {
    program
        .positions
        .iter()
        .enumerate()
        .map(|(node, pos)| {
            let edges = [false, true].map(|powered| {
                program
                    .output_program(node, powered)
                    .iter()
                    .map(|&op| match op {
                        Operation::Contribution(index) => {
                            let link = &program.links[index];
                            format!(
                                "link {:?}->{:?} -{}",
                                program.positions[link.source], program.positions[link.target], link.attenuation
                            )
                        }
                        Operation::ArrangementContribution(_) => panic!("fixed fixture must not contain arrangement operations"),
                        Operation::CheckWatchedWire(index) => {
                            let record = &program.watched_wires[index];
                            let watchers: Vec<_> = record
                                .watchers
                                .iter()
                                .map(|&watcher| {
                                    (
                                        program.positions[notification_component(watcher)],
                                        watcher & PISTON_HEAD_NOTIFICATION != 0,
                                    )
                                })
                                .collect();
                            format!("watch {:?}: {watchers:?}", program.positions[record.wire])
                        }
                        Operation::RefreshWire(target)
                        | Operation::UpdateLamp(target)
                        | Operation::UpdateHopper(target)
                        | Operation::UpdateTrapdoor(target)
                        | Operation::UpdateCommandBlock(target)
                        | Operation::UpdateNoteBlock(target)
                        | Operation::UpdateCopperBulb(target)
                        | Operation::UpdateRepeater(target)
                        | Operation::UpdatePiston(target)
                        | Operation::UpdateTorch(target)
                        | Operation::UpdateComparator(target) => format!("{:?} {:?}", std::mem::discriminant(&op), program.positions[target]),
                    })
                    .collect()
            });
            ((pos.x, pos.y, pos.z), edges)
        })
        .collect()
}

fn assert_execution_matches_native(world: &PlotWorld, warm: Program, cold: Program) {
    assert_execution_matches_native_for(world, warm, cold, "");
}

fn assert_execution_matches_native_for(world: &PlotWorld, warm: Program, cold: Program, context: &str) {
    assert_eq!(warm.positions, cold.positions, "{context} fresh graph assigns the same component IDs");
    assert_eq!(
        canonical(&warm),
        canonical(&cold),
        "{context} cached ordering or operand remapping differs from cold sampling"
    );
    let mut native = world.activation_snapshot();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut warm = super::super::start_runtime(warm, ticks.clone(), world.piston_state().clone()).unwrap();
    let mut cold = super::super::start_runtime(cold, ticks, world.piston_state().clone()).unwrap();
    for control in [CONTROLS[0], CONTROLS[1], CONTROLS[0], CONTROLS[1]] {
        crate::interaction::use_redstone(native.get_block(control), &mut native, control, None);
        warm.use_control(control).unwrap();
        cold.use_control(control).unwrap();
        for step in 0..=5 {
            if step != 0 {
                native.tick_interpreted();
                warm.tick().unwrap();
                cold.tick().unwrap();
            }
            let expected: Vec<_> = native.scheduler().iter_entries().collect();
            assert_eq!(
                warm.pending_entries(),
                expected,
                "{context} warm ordered pending work control={control:?}, step={step}"
            );
            assert_eq!(
                cold.pending_entries(),
                expected,
                "{context} cold ordered pending work control={control:?}, step={step}"
            );
            for &pos in &warm.program.positions {
                assert_eq!(
                    warm.project(pos),
                    Some(native.get_block(pos)),
                    "{context} warm block {pos:?}, control={control:?}, step={step}"
                );
                assert_eq!(
                    cold.project(pos),
                    Some(native.get_block(pos)),
                    "{context} cold block {pos:?}, control={control:?}, step={step}"
                );
            }
        }
    }
    assert_eq!(warm.operations, cold.operations, "{context} reuse retains semantic work accounting");
}

#[test]
fn deleting_wire_resamples_affected_source_and_reuses_independent_chunk() {
    let mut world = fixture();
    let before = compile(&world, None);
    let cache = before.source_order_cache.as_ref().unwrap();
    assert_eq!(reuse(&before), (0, 2), "both controls must be racing and cache eligible");
    let leaf = BlockPos::new(7, 20, 9);
    world.set_block(leaf, Block::Air);
    crate::redstone::update_surrounding_blocks(&mut world, leaf);
    assert!(
        world.scheduler().iter_entries().next().is_none(),
        "edit must preserve the global queue context"
    );
    let warm = compile(&world, Some(cache));
    assert_eq!(reuse(&warm), (1, 1), "only the edited source's read/write chunks changed");
    let cold = compile(&world, None);
    assert_execution_matches_native(&world, warm, cold);
}

#[test]
fn unrelated_chunk_edit_reuses_every_source() {
    let mut world = fixture();
    let before = compile(&world, None);
    let cache = before.source_order_cache.as_ref().unwrap();
    world.set_block(BlockPos::new(400, 20, 400), Block::RedstoneLamp { lit: true });
    let warm = compile(&world, Some(cache));
    assert_eq!(reuse(&warm), (2, 0), "a distant chunk is outside both source dependencies");
    let cold = compile(&world, None);
    assert_execution_matches_native(&world, warm, cold);
}

#[test]
fn air_gap_reconnection_and_component_id_shift_never_reuse_stale_operations() {
    let mut world = fixture();
    let before = compile(&world, None);
    let old_peer = before.component_indices[&CONTROLS[1]];
    let cache = before.source_order_cache.as_ref().unwrap();
    let bridge = BlockPos::new(8, 20, 7);
    assert_eq!(world.get_block(bridge), Block::Air);
    wire(&mut world, bridge);
    let state = crate::redstone::wire::get_state_for_placement(&world, bridge);
    world.set_block(bridge, Block::RedstoneWire { wire: state });
    crate::redstone::update_surrounding_blocks(&mut world, bridge);
    // Earlier scan position forces all later component references to receive fresh IDs.
    world.set_block(BlockPos::new(3, 20, 5), Block::RedstoneLamp { lit: false });
    assert!(world.scheduler().iter_entries().next().is_none());
    let warm = compile(&world, Some(cache));
    assert_ne!(warm.component_indices[&CONTROLS[1]], old_peer);
    assert_eq!(reuse(&warm), (1, 1), "negative air reads must invalidate the newly merged source only");
    let cold = compile(&world, None);
    assert_execution_matches_native(&world, warm, cold);
}

#[test]
fn entity_payload_changes_invalidate_read_chunks_and_pending_context_rejects_all_records() {
    let mut world = fixture();
    let before = compile(&world, None);
    let cache = before.source_order_cache.as_ref().unwrap();
    let Some(BlockEntity::Sign(sign)) = world.get_block_entity_mut(BlockPos::new(6, 20, 7)) else {
        panic!("missing sign")
    };
    sign.rows[0] = "{\"text\":\"changed opaque entity payload\"}".into();
    let edited = compile(&world, Some(cache));
    assert_eq!(reuse(&edited), (1, 1), "entity revisions in a source dependency chunk must resample it");
    assert_eq!(canonical(&edited), canonical(&compile(&world, None)));
    world.schedule_tick(CONTROLS[0] + BlockPos::new(5, 0, 0), 4, TickPriority::High);
    let warm = compile(&world, Some(cache));
    assert_eq!(reuse(&warm), (0, 2), "ordered pending work changes the complete probe context");
    let cold = compile(&world, None);
    assert_execution_matches_native(&world, warm, cold);
}

#[test]
fn independent_circuits_in_the_same_chunk_are_conservatively_invalidated_together() {
    let mut world = fixture();
    let third = BlockPos::new(5, 20, 12);
    world.set_block(third.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        third,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for x in 1..=4 {
        wire(&mut world, third + BlockPos::new(x, 0, 0));
    }
    let third_lamp = third + BlockPos::new(5, 0, 0);
    world.set_block(third_lamp, Block::RedstoneLamp { lit: false });
    for x in 1..=4 {
        let pos = third + BlockPos::new(x, 0, 0);
        let state = crate::redstone::wire::get_state_for_placement(&world, pos);
        world.set_block(pos, Block::RedstoneWire { wire: state });
    }
    crate::interaction::use_redstone(world.get_block(third), &mut world, third, None);
    assert_eq!(world.get_block(third_lamp), Block::RedstoneLamp { lit: true });
    assert!(world.scheduler().iter_entries().next().is_none());
    // Include the third circuit's callback context without moving it into a different chunk.
    let bounds = (BOUNDS.0, BlockPos::new(BOUNDS.1.x, BOUNDS.1.y, 15));
    let compile = |world: &PlotWorld, previous: Option<&super::super::SourceOrderCache>| {
        super::super::compiler::compile_monitored_with_source_cache(
            world,
            bounds,
            super::super::ScratchEntry {
                scheduled: world.scheduler().iter_entries().collect(),
                piston: world.piston_state().clone(),
            },
            None,
            previous,
        )
        .unwrap()
    };
    let before = compile(&world, None);
    assert_eq!(reuse(&before), (0, 3));
    let cache = before.source_order_cache.as_ref().unwrap();
    let leaf = BlockPos::new(7, 20, 9);
    world.set_block(leaf, Block::Air);
    crate::redstone::update_surrounding_blocks(&mut world, leaf);
    assert_eq!(world.get_block(third_lamp), Block::RedstoneLamp { lit: true });
    assert!(world.scheduler().iter_entries().next().is_none());
    let warm = compile(&world, Some(cache));
    assert_eq!(
        reuse(&warm),
        (1, 2),
        "dependencies cover chunks: both first-chunk sources miss, the other chunk hits"
    );
    let cold = compile(&world, None);
    assert_execution_matches_native(&world, warm, cold);
}

#[test]
fn solid_support_swaps_invalidate_only_their_dependency_chunks() {
    let baseline = fixture();
    let before = compile(&baseline, None);
    let cache = before.source_order_cache.as_ref().unwrap().clone();
    let support = |control| control + BlockPos::new(1, -1, 0);
    let scenarios = [
        (vec![support(CONTROLS[0])], (1, 1)),
        (vec![support(CONTROLS[0]), support(CONTROLS[0]) + BlockPos::new(1, 0, 0)], (1, 1)),
        (vec![support(CONTROLS[0]), support(CONTROLS[1])], (0, 2)),
    ];

    for (supports, expected) in scenarios {
        let mut world = fixture();
        for pos in supports {
            assert_eq!(world.get_block(pos), Block::Stone {});
            world.set_block(pos, Block::from_name("white_concrete").unwrap());
            crate::redstone::update_surrounding_blocks(&mut world, pos);
        }
        assert!(world.scheduler().iter_entries().next().is_none());
        let warm = compile(&world, Some(&cache));
        assert_eq!(reuse(&warm), expected, "support edit at dependency chunk(s)");
        let cold = compile(&world, None);
        assert_execution_matches_native(&world, warm, cold);
    }
}

#[test]
fn deterministic_wire_removal_seeds_match_warm_cold_and_native_execution() {
    let baseline = fixture();
    let before = compile(&baseline, None);
    let cache = before.source_order_cache.as_ref().unwrap().clone();
    let candidates = [
        BlockPos::new(6, 20, 8),
        BlockPos::new(7, 20, 8),
        BlockPos::new(8, 20, 8),
        BlockPos::new(9, 20, 8),
        BlockPos::new(41, 20, 8),
        BlockPos::new(42, 20, 8),
        BlockPos::new(43, 20, 8),
        BlockPos::new(44, 20, 8),
        BlockPos::new(7, 20, 9),
        BlockPos::new(8, 20, 6),
    ];

    for seed in 0u32..20 {
        let mut state = seed.wrapping_add(1);
        let mut next_index = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state as usize % candidates.len()
        };
        let count = 1 + next_index() % 2;
        let mut removed = Vec::with_capacity(count);
        while removed.len() < count {
            let pos = candidates[next_index()];
            if !removed.contains(&pos) {
                removed.push(pos);
            }
        }

        let mut world = fixture();
        for &pos in &removed {
            assert!(
                matches!(world.get_block(pos), Block::RedstoneWire { .. }),
                "seed={seed} action=remove {pos:?}"
            );
            world.set_block(pos, Block::Air);
            crate::redstone::update_surrounding_blocks(&mut world, pos);
        }
        let context = format!("seed={seed} removed={removed:?}");
        let warm = compile(&world, Some(&cache));
        let cold = compile(&world, None);
        assert_execution_matches_native_for(&world, warm, cold, &context);
    }
}

#[test]
fn source_cache_preserves_runtime_and_publication_across_backends() {
    let before = compile(&fixture(), None);
    let cache = before.source_order_cache.as_ref().unwrap().clone();

    let leaf = BlockPos::new(7, 20, 9);
    for llvm in [false, true] {
        if llvm && !cfg!(any(windows, target_os = "linux")) {
            continue;
        }
        for publication in [
            crate::redpilator::PublicationMode::All,
            crate::redpilator::PublicationMode::Redstone,
            crate::redpilator::PublicationMode::Io,
        ] {
            let mut warm_world = fixture();
            warm_world.set_block(leaf, Block::Air);
            crate::redstone::update_surrounding_blocks(&mut warm_world, leaf);
            let mut cold_world = fixture();
            cold_world.set_block(leaf, Block::Air);
            crate::redstone::update_surrounding_blocks(&mut cold_world, leaf);
            let options = crate::redpilator::CompilerOptions {
                llvm,
                publication: Some(publication),
                ..Default::default()
            };
            let mut warm = crate::redpilator::Compiler::default();
            warm.compile_cached_with_sampling(
                &warm_world,
                BOUNDS,
                options.clone(),
                vec![],
                std::sync::Arc::new(crate::redpilator::TaskMonitor::default()),
                None,
                Some(cache.clone()),
            )
            .unwrap();
            let stats = warm.source_sampling_stats().unwrap();
            assert_eq!((stats.0, stats.1), (1, 1));

            let mut cold = crate::redpilator::Compiler::default();
            cold.compile_cached_with_sampling(
                &cold_world,
                BOUNDS,
                options,
                vec![],
                std::sync::Arc::new(crate::redpilator::TaskMonitor::default()),
                None,
                None,
            )
            .unwrap();
            let stats = cold.source_sampling_stats().unwrap();
            assert_eq!((stats.0, stats.1), (0, 2));

            let positions = warm.cached_program().unwrap().positions.clone();
            assert_eq!(warm.cached_program().unwrap().positions, cold.cached_program().unwrap().positions);
            for control in [CONTROLS[0], CONTROLS[1], CONTROLS[0], CONTROLS[1]] {
                warm.try_on_use_block(control).unwrap();
                cold.try_on_use_block(control).unwrap();
                for step in 0..=5 {
                    if step != 0 {
                        warm.tick().unwrap();
                        cold.tick().unwrap();
                    }
                    warm.try_flush(&mut warm_world).unwrap();
                    cold.try_flush(&mut cold_world).unwrap();
                    let pending = |compiler: &crate::redpilator::Compiler| {
                        compiler.llvm_runtime.as_ref().map_or_else(
                            || compiler.runtime.as_ref().unwrap().pending_entries(),
                            |native| native.pending_entries().unwrap(),
                        )
                    };
                    assert_eq!(pending(&warm), pending(&cold), "llvm={llvm} {publication:?} {control:?} step={step}");
                    assert_eq!(
                        warm.runtime.as_ref().unwrap().projected_blocks(),
                        cold.runtime.as_ref().unwrap().projected_blocks(),
                        "runtime state llvm={llvm} {publication:?} {control:?} step={step}"
                    );
                    for &pos in &positions {
                        assert_eq!(
                            warm_world.get_block(pos),
                            cold_world.get_block(pos),
                            "published block llvm={llvm} {publication:?} at {pos:?}, {control:?} step={step}"
                        );
                    }
                }
            }
        }
    }
}
