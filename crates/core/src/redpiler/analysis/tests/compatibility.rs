use super::*;
use crate::redpiler::instant::{
    boolean::{BooleanArena, Variable},
    clocked::ClockedProgram,
    logic::WaveLogic,
    sampling::data_notifies,
};
use crate::redstone::piston::trace::{self, Operation};
use mchprs_blocks::blocks::{RedstoneComparator, RedstoneRepeater};
use mchprs_blocks::BlockDirection;
use rustc_hash::FxHashSet;

#[test]
fn multiple_clock_candidates_defer_to_sampling_and_require_independent_control() {
    use crate::redpiler::instant::{clocked, sampling};
    let mut world = empty();
    for dx in [0, 8] {
        let base = BASE + BlockPos::new(dx, 0, 0);
        world.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::Down,
                    sticky: false,
                    extended: false,
                },
            },
        );
        world.set_block(
            base.offset(BlockFace::Top),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        );
        world.set_block(base + BlockPos::new(0, 2, 0), Block::Stone {});
    }
    let report = analyze_world(&world);
    let monitor = TaskMonitor::default();
    for assume_instant in [false, true] {
        assert!(
            clocked::recognize(&world, &report, &monitor, assume_instant)
                .unwrap()
                .is_none()
        );
    }
    let error = sampling::recognize(&world, &report, &monitor, None, &FxHashSet::default())
        .err()
        .expect("observing a generator does not supply its independent control");
    assert!(
        error.contains("no independently coupled control update"),
        "{error}"
    );
    assert!(error.contains(&format!("{:?}", BASE)), "{error}");
}

#[test]
fn notification_geometry_matches_native_piston_rechecks_in_every_orientation() {
    let mut sources = vec![Block::RedstoneTorch { lit: true }];
    for facing in [
        BlockDirection::North,
        BlockDirection::South,
        BlockDirection::East,
        BlockDirection::West,
    ] {
        sources.extend([
            Block::RedstoneWallTorch { facing, lit: true },
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    ..Default::default()
                },
            },
            Block::RedstoneComparator {
                comparator: RedstoneComparator {
                    facing,
                    ..Default::default()
                },
            },
        ]);
    }
    sources.extend(BlockFace::values().map(|face| Block::Observer {
        observer: RedstoneObserver {
            facing: face.into(),
            powered: true,
        },
    }));
    for source in sources {
        let mut world = empty();
        let mut targets = Vec::new();
        for x in -2..=2 {
            for y in -2..=2 {
                for z in -2..=2 {
                    let pos = BASE + BlockPos::new(x, y, z);
                    if pos == BASE {
                        continue;
                    }
                    targets.push(pos);
                    world.set_block(
                        pos,
                        Block::Piston {
                            piston: RedstonePiston {
                                facing: BlockFacing::Down,
                                sticky: true,
                                extended: false,
                            },
                        },
                    );
                }
            }
        }
        world.set_block(BASE, source);
        let samples: FxHashSet<_> = trace::capture(|| match source {
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => {
                crate::redstone::on_torch_state_change(&mut world, BASE);
            }
            Block::Observer { observer } => {
                crate::redstone::tick(source, &mut world, BASE);
                assert!(observer.powered);
            }
            Block::RedstoneRepeater { repeater } => {
                crate::redstone::on_state_change(
                    repeater.facing.block_face().into(),
                    &mut world,
                    BASE,
                );
            }
            Block::RedstoneComparator { comparator } => {
                crate::redstone::on_state_change(
                    comparator.facing.block_face().into(),
                    &mut world,
                    BASE,
                );
            }
            _ => unreachable!(),
        })
        .into_iter()
        .filter_map(|entry| match entry.operation {
            Operation::Sample { pos, .. } => Some(pos),
            _ => None,
        })
        .collect();
        assert!(!samples.is_empty(), "native premise: {source:?}");
        for pos in targets {
            assert_eq!(
                data_notifies(&world, BASE, pos),
                samples.contains(&pos),
                "{source:?} -> {:?}",
                pos - BASE
            );
        }
    }
}

#[test]
fn notification_inventory_includes_absent_writer_heads_and_receiver_eligibility() {
    use crate::redpiler::analysis::ports::UpdateKind;
    let mut world = empty();
    let piston = RedstonePiston {
        facing: BlockFacing::Down,
        sticky: true,
        extended: true,
    };
    world.set_block(BASE, Block::Piston { piston });
    world.set_block(
        BASE.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(BASE + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
    for y in [0, -1] {
        world.set_block(
            BASE + BlockPos::new(-2, y, 0),
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: false,
                    extended: false,
                },
            },
        );
    }
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == BASE).unwrap();
    let updates = &report.ports.pistons[actor].updates;
    for (y, requires_extended) in [(0, false), (-1, true)] {
        let source = BASE + BlockPos::new(-1, y, 0);
        assert_eq!(world.get_block(source), Block::Air);
        assert!(updates.iter().any(|u| u.source == source
            && u.kind == UpdateKind::AdjacentHeadChange
            && u.requires_extended == requires_extended));
    }
    let unique: FxHashSet<_> = updates
        .iter()
        .map(|u| (u.source, u.kind as u8, u.requires_extended))
        .collect();
    assert_eq!(unique.len(), updates.len());
}

#[test]
fn clock_validation_uses_only_its_control_root_and_keeps_existing_guards() {
    let mut world = empty();
    let torch = BASE;
    let data = BASE.offset(BlockFace::East);
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    world.set_block(
        data,
        Block::Lever {
            lever: Default::default(),
        },
    );
    let mut arena = BooleanArena::default();
    let signal = |pos, order| Variable::Signal {
        pos,
        threshold: 0,
        order,
    };
    let control = arena.variable(signal(torch, 0));
    let release = arena.not(control);
    let input = arena.variable(signal(data, 1));
    let upstream = arena.variable(Variable::Actuator(1));
    let mut logic = WaveLogic {
        arena,
        responses: vec![upstream, release, input],
        response_order: vec![1, 0, 2],
        sources: vec![torch, data],
        response_sources: vec![torch, data],
        wires: Default::default(),
        wire_links: Default::default(),
        unprojected_consumer_wires: Default::default(),
        outputs: vec![],
        handoff_wires: vec![],
        follows_payload: vec![false; 3],
        context: Default::default(),
    };
    let clock = ClockedProgram {
        clock: 0,
        memory: vec![],
        observers: Default::default(),
    };
    clock.validate(&world, &logic).unwrap();
    logic.responses[0] = logic.arena.and(release, input);
    assert!(clock
        .validate(&world, &logic)
        .unwrap_err()
        .contains("one ordinary torch"));
    logic.responses[0] = input;
    assert!(clock
        .validate(&world, &logic)
        .unwrap_err()
        .contains("one ordinary torch"));
    logic.responses[0] = control;
    assert!(clock
        .validate(&world, &logic)
        .unwrap_err()
        .contains("release on loss"));
    logic.responses[0] = logic.arena.variable(Variable::Memory(2));
    assert!(clock
        .validate(&world, &logic)
        .unwrap_err()
        .contains("stored data"));
}
