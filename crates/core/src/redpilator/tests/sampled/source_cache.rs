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
    assert_eq!(warm.positions, cold.positions, "fresh graph assigns the same component IDs");
    assert_eq!(
        canonical(&warm),
        canonical(&cold),
        "cached ordering or operand remapping differs from cold sampling"
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
                "warm ordered pending work control={control:?}, step={step}"
            );
            assert_eq!(
                cold.pending_entries(),
                expected,
                "cold ordered pending work control={control:?}, step={step}"
            );
            for &pos in &warm.program.positions {
                assert_eq!(
                    warm.project(pos),
                    Some(native.get_block(pos)),
                    "warm block {pos:?}, control={control:?}, step={step}"
                );
                assert_eq!(
                    cold.project(pos),
                    Some(native.get_block(pos)),
                    "cold block {pos:?}, control={control:?}, step={step}"
                );
            }
        }
    }
    assert_eq!(warm.operations, cold.operations, "reuse retains semantic work accounting");
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
