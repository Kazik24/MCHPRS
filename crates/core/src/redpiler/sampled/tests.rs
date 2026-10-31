use super::program::RepeaterRecord;
use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::scratch::discover_lamp_sources;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::{
    BlockDirection, BlockFace,
    blocks::{Lever, RedstonePiston, RedstonePistonHead, RedstoneRepeater, RedstoneWire},
};
use mchprs_world::TickPriority;
fn test_program(
    positions: Vec<BlockPos>,
    initial: Vec<Block>,
    sources: Vec<usize>,
    source_programs: Vec<[Vec<Operation>; 2]>,
    links: Vec<ElectricalLink>,
    watched_wires: Vec<WatchedWire>,
) -> Result<Program, ScratchError> {
    if source_programs.len() != sources.len() {
        return Err(ScratchError {
            position: None,
            reason: "sampled program source transition count mismatch",
        });
    }
    let mut output_programs = vec![[vec![], vec![]]; initial.len()];
    for (&source, programs) in sources.iter().zip(source_programs) {
        if let Some(output) = output_programs.get_mut(source) {
            *output = programs;
        }
    }
    Program::new(positions, initial, sources, output_programs, links, watched_wires, vec![])
}
fn repeater_program(delay: u8, powered: bool, locked: bool, output_is_diode: bool) -> Program {
    let positions = (0..3).map(|x| BlockPos::new(x, 15, 15)).collect();
    let initial = vec![
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                delay,
                facing: BlockDirection::East,
                locked,
                powered,
            },
        },
        Block::RedstoneLamp { lit: powered },
    ];
    let mut output_programs = vec![[vec![], vec![]]; 3];
    output_programs[0] = [
        vec![Operation::Contribution(0), Operation::UpdateRepeater(1)],
        vec![Operation::Contribution(0), Operation::UpdateRepeater(1)],
    ];
    output_programs[1] = [
        vec![Operation::Contribution(1), Operation::UpdateLamp(2)],
        vec![Operation::Contribution(1), Operation::UpdateLamp(2)],
    ];
    Program::new(
        positions,
        initial,
        vec![0],
        output_programs,
        vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 2,
                attenuation: 0,
            },
        ],
        vec![],
        vec![RepeaterRecord {
            component: 1,
            output_is_diode,
            side_sources: None,
        }],
    )
    .unwrap()
}
fn host() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}
#[test]
fn comparator_compare_and_subtract_levels() {
    use mchprs_blocks::blocks::ComparatorMode::{Compare, Subtract};
    for (rear, side, compare, subtract) in [
        (0, 0, (0, false), (0, false)),
        (10, 4, (10, true), (6, true)),
        (10, 10, (10, true), (0, false)),
        (10, 12, (0, false), (0, false)),
    ] {
        assert_eq!(super::executor::compare(rear, side, Compare), compare);
        assert_eq!(super::executor::compare(rear, side, Subtract), subtract);
    }
}
#[test]
fn unchanged_comparator_output_notifies_reactive_neighbor() {
    use super::program::ComparatorRecord;
    let comparator = Block::from_name("comparator").unwrap();
    let positions = vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10), BlockPos::new(12, 10, 10)];
    let mut program = test_program(
        positions.clone(),
        vec![comparator, Block::RedstoneLamp { lit: false }, Block::RedstoneBlock],
        vec![],
        vec![],
        vec![ElectricalLink {
            source: 2,
            target: 1,
            attenuation: 0,
        }],
        vec![],
    )
    .unwrap();
    program.comparators.push(ComparatorRecord {
        component: 0,
        stored: 0,
        rear_override: None,
        far_override: None,
        output_is_diode: false,
    });
    program.reactive[1] = true;
    program.reactive_neighbours[0].push(1);
    let mut runtime = program.start();
    runtime.pending.schedule_tick(
        super::ScheduledBlockTick {
            pos: positions[0],
            block_type: Some(comparator.registry_id()),
        },
        1,
        TickPriority::Normal,
    );
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    assert_eq!(runtime.project(positions[1]), Some(Block::RedstoneLamp { lit: true }));
}
#[test]
fn token_move_notifies_wire_watchers_only_when_strength_changes() {
    use super::program::MoveStep;
    let cell = BlockPos::new(10, 10, 10);
    for masked in [false, true] {
        let mut program = test_program(
            vec![BlockPos::new(12, 10, 10), BlockPos::new(13, 10, 10), BlockPos::new(14, 10, 10)],
            vec![
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, masked),
                },
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15,
                        ..Default::default()
                    },
                },
                Block::RedstoneLamp { lit: true },
            ],
            vec![0],
            vec![[vec![], vec![]]],
            vec![ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            }],
            vec![WatchedWire { wire: 1, watchers: vec![2] }],
        )
        .unwrap();
        program.tokens.push(super::compiler::PistonToken {
            block: Block::RedstoneBlock,
            position: Some(cell),
            cells: vec![cell],
        });
        program.token_sources.push(vec![vec![(1, 0)]]);
        program.move_steps.push(vec![[
            vec![
                MoveStep {
                    node: 1,
                    watcher: false,
                    wire: true,
                    wires: vec![],
                },
                MoveStep {
                    node: 2,
                    watcher: false,
                    wire: false,
                    wires: vec![1],
                },
            ],
            vec![],
        ]]);
        program.compile_movement_flips().unwrap();
        let mut runtime = program.start();
        runtime.set_token_pos(0, None);
        assert_eq!(runtime.watched_strengths[0], if masked { 15 } else { 0 });
        assert_eq!(runtime.updated_in_phase[2].is_some(), !masked);
    }
}
#[test]
fn movement_flips_preserve_discovery_order_for_remaining_recipients() {
    use super::program::{ArrangementSource, InputArrangement};
    let cell = BlockPos::new(10, 10, 10);
    let positions = vec![cell, cell.offset(BlockFace::East), cell.offset(BlockFace::West)];
    let mut program = test_program(
        positions.clone(),
        vec![Block::RedstoneBlock, Block::RedstoneLamp { lit: true }, Block::RedstoneLamp { lit: true }],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    program.tokens.push(super::compiler::PistonToken {
        block: Block::RedstoneBlock,
        position: Some(cell),
        cells: vec![cell],
    });
    for target in [2, 1] {
        program.input_arrangements.push(InputArrangement {
            target,
            side: false,
            silent: false,
            tokens: vec![0],
            conditional_sources: vec![ArrangementSource::Token(0)],
            inputs: vec![vec![], vec![(ArrangementSource::Token(0), 0)]],
        });
    }
    program.compile_movement_flips().unwrap();
    let mut runtime = program.start();
    runtime.set_token_pos(0, None);
    assert_eq!(
        runtime.pending.iter_entries().map(|entry| entry.pos).collect::<Vec<_>>(),
        vec![positions[2], positions[1]]
    );
    runtime.set_token_pos(0, Some(cell));
    assert_eq!(runtime.input_strength(1), 15);
    runtime.set_token_pos(0, None);
    assert_eq!(runtime.group_arrangements, vec![0]);
}
#[test]
fn conditional_source_changes_wait_for_their_ordered_link_instruction() {
    use super::program::{ArrangementSource, InputArrangement};
    let cell = BlockPos::new(10, 10, 10);
    let mut program = test_program(
        vec![cell, cell.offset(BlockFace::East)],
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
            Block::RedstoneLamp { lit: false },
        ],
        vec![0],
        vec![[vec![], vec![Operation::UpdateLamp(1)]]],
        vec![],
        vec![],
    )
    .unwrap();
    program.tokens.push(super::compiler::PistonToken {
        block: Block::Stone {},
        position: Some(cell.offset(BlockFace::Top)),
        cells: vec![cell.offset(BlockFace::Top)],
    });
    program.input_arrangements.push(InputArrangement {
        target: 1,
        side: false,
        silent: false,
        tokens: vec![0],
        conditional_sources: vec![ArrangementSource::Component(0)],
        inputs: vec![vec![], vec![(ArrangementSource::Component(0), 1)]],
    });
    program.compile_movement_flips().unwrap();
    let mut runtime = program.start();
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: false });
    drop(runtime);
    program.output_programs[0][1] = vec![Operation::ArrangementContribution(0), Operation::UpdateLamp(1)];
    let mut runtime = program.start();
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.input_strength(1), 14);
    assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: true });
    runtime.set_token_pos(0, None);
    assert_eq!(runtime.input_strength(1), 0);
}
#[test]
fn separate_arrangement_groups_mask_each_of_their_fixed_sources() {
    use super::program::{ArrangementSource, InputArrangement};
    let cell = BlockPos::new(10, 10, 10);
    let lever = |powered| Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, powered),
    };
    let mut program = test_program(
        vec![cell, cell.offset(BlockFace::East), cell.offset(BlockFace::West)],
        vec![lever(false), lever(true), Block::RedstoneLamp { lit: false }],
        vec![0, 1],
        vec![[vec![], vec![]], [vec![], vec![]]],
        vec![
            ElectricalLink {
                source: 0,
                target: 2,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 2,
                attenuation: 0,
            },
        ],
        vec![],
    )
    .unwrap();
    program.tokens.push(super::compiler::PistonToken {
        block: Block::Stone {},
        position: None,
        cells: vec![cell.offset(BlockFace::Top)],
    });
    for source in [0, 1] {
        program.input_arrangements.push(InputArrangement {
            target: 2,
            side: false,
            silent: false,
            tokens: vec![0],
            conditional_sources: vec![ArrangementSource::Component(source)],
            inputs: vec![vec![], vec![(ArrangementSource::Component(source), 0)]],
        });
    }
    program.compile_movement_flips().unwrap();
    let runtime = program.start();
    assert_eq!(runtime.input_strength(2), 0);
}
#[test]
fn changed_links_include_masked_sources_additions_removals_and_attenuation() {
    use super::program::ArrangementSource::Component;
    let before = [(Component(0), 0), (Component(1), 4), (Component(2), 2)].into_iter().collect();
    let after = [(Component(0), 0), (Component(1), 5), (Component(3), 3)].into_iter().collect();
    assert_eq!(
        super::compiler::find_changed_links(&before, &after),
        vec![Component(1), Component(2), Component(3)]
    );
    let before = [((Component(0), false), 1), ((Component(0), true), 3)].into_iter().collect();
    let after = [((Component(0), false), 1), ((Component(0), true), 4)].into_iter().collect();
    assert_eq!(super::compiler::find_changed_links(&before, &after), vec![(Component(0), true)]);
}
#[test]
fn group_probe_finds_masked_conductor_dependency() {
    use super::program::ArrangementSource;
    let cell = BlockPos::new(15, 15, 15);
    let lever_pos = cell.offset(BlockFace::West);
    let lamp_pos = cell.offset(BlockFace::East);
    let constant_pos = lamp_pos.offset(BlockFace::Top);
    let mut world = host();
    let lever = Block::Lever {
        lever: Lever::new(LeverFace::Wall, BlockDirection::West, true),
    };
    world.set_block(cell, Block::Stone {});
    world.set_block(lever_pos, lever);
    world.set_block(lamp_pos, Block::RedstoneLamp { lit: true });
    world.set_block(constant_pos, Block::RedstoneBlock);
    let mut program = test_program(
        vec![lever_pos, constant_pos, lamp_pos],
        vec![lever, Block::RedstoneBlock, Block::RedstoneLamp { lit: true }],
        vec![0],
        vec![[vec![], vec![]]],
        vec![],
        vec![],
    )
    .unwrap();
    program.tokens.push(super::compiler::PistonToken {
        block: Block::Stone {},
        position: Some(cell),
        cells: vec![cell],
    });
    let groups = super::group_probe::run(&world, (BlockPos::new(10, 10, 10), BlockPos::new(22, 21, 22)), &entry(&world), &program).unwrap();
    let group = groups
        .iter()
        .find(|group| group.target == 2 && !group.side && group.conditional_sources.contains(&ArrangementSource::Component(0)))
        .unwrap();
    assert_eq!(group.tokens, vec![0]);
    assert_eq!(group.inputs[0], vec![]);
    assert_eq!(group.inputs[1], vec![(ArrangementSource::Component(0), 0)]);
}
#[test]
fn conditional_side_input_locks_repeater_without_powering_its_rear() {
    use super::program::{ArrangementSource, InputArrangement};
    let cell = BlockPos::new(10, 10, 10);
    let repeater = |powered| Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            delay: 1,
            facing: BlockDirection::East,
            locked: false,
            powered,
        },
    };
    let mut program = Program::new(
        vec![cell, cell.offset(BlockFace::North)],
        vec![repeater(false), repeater(true)],
        vec![],
        vec![[vec![], vec![]]; 2],
        vec![],
        vec![],
        vec![
            RepeaterRecord {
                component: 0,
                output_is_diode: false,
                side_sources: Some(vec![1]),
            },
            RepeaterRecord {
                component: 1,
                output_is_diode: false,
                side_sources: Some(vec![]),
            },
        ],
    )
    .unwrap();
    let token_cell = cell.offset(BlockFace::Top);
    program.tokens.push(super::compiler::PistonToken {
        block: Block::Stone {},
        position: None,
        cells: vec![token_cell],
    });
    program.input_arrangements.push(InputArrangement {
        target: 0,
        side: true,
        silent: false,
        tokens: vec![0],
        conditional_sources: vec![ArrangementSource::Component(1)],
        inputs: vec![vec![], vec![(ArrangementSource::Component(1), 0)]],
    });
    program.compile_movement_flips().unwrap();
    let mut runtime = program.start();
    runtime.update_repeater(0);
    assert!(matches!(runtime.blocks[0], Block::RedstoneRepeater { repeater } if !repeater.locked));
    runtime.set_token_pos(0, Some(token_cell));
    assert_eq!(runtime.input_strength(0), 0);
    assert!(matches!(runtime.blocks[0], Block::RedstoneRepeater { repeater } if repeater.locked));
}
#[test]
fn compiled_floor_and_wall_torches_delay_recheck_and_rearm() {
    for wall in [false, true] {
        let support = BlockPos::new(16, 15, 15);
        let torch = support.offset(if wall { BlockFace::South } else { BlockFace::Top });
        let lever = support.offset(BlockFace::East);
        let mut world = host();
        world.set_block(support, Block::Stone {});
        world.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Wall, BlockDirection::East, false),
            },
        );
        world.set_block(
            torch,
            if wall {
                Block::RedstoneWallTorch {
                    lit: true,
                    facing: BlockDirection::South,
                }
            } else {
                Block::RedstoneTorch { lit: true }
            },
        );
        let program = compile(&world, (BlockPos::new(10, 10, 10), BlockPos::new(22, 21, 22)), entry(&world)).unwrap();
        let node = program.positions.iter().position(|&pos| pos == torch).unwrap();
        let mut runtime = program.start();
        assert_eq!(runtime.project(support), Some(Block::Stone {}));
        for powered in [true, false, true, false] {
            runtime.set_input(0, powered).unwrap();
            assert_eq!(runtime.output(node), if powered { 15 } else { 0 });
            runtime.tick().unwrap();
            assert_eq!(runtime.output(node), if powered { 15 } else { 0 });
            runtime.tick().unwrap();
            assert_eq!(runtime.output(node), if powered { 0 } else { 15 });
        }
        runtime.set_input(0, true).unwrap();
        runtime.tick().unwrap();
        runtime.set_input(0, false).unwrap();
        runtime.tick().unwrap();
        assert_eq!(runtime.output(node), 15, "scheduled torch tick must recheck a withdrawn input");
    }
}
#[test]
fn piston_request_moves_token_after_two_motion_ticks() {
    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let mut world = host();
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
    world.set_block(home, Block::Stone {});
    let actors = discover_piston_actors(&world, bounds, entry(&world)).unwrap();
    let tokens = super::compiler::discover_piston_tokens(&world, &actors).unwrap();
    let lever = BlockPos::new(14, 15, 15);
    let mut program = test_program(
        vec![lever, base],
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            world.get_block(base),
        ],
        vec![0],
        vec![[vec![], vec![]]],
        vec![ElectricalLink {
            source: 0,
            target: 1,
            attenuation: 0,
        }],
        vec![],
    )
    .unwrap();
    program.pistons = actors;
    program.tokens = tokens;
    let mut runtime = program.start();
    runtime.queue_piston_extension(0);
    runtime.queue_piston_extension(0);
    runtime.tick().unwrap();
    assert_eq!(runtime.accepted_piston_actions, 1);
    assert_eq!(runtime.token_movements, 1);
    assert_eq!(runtime.token_positions[0], None);
    assert_eq!(runtime.motion_completions, 0);
    assert_eq!(runtime.project(far), None);
    runtime.tick().unwrap();
    assert_eq!(runtime.token_positions[0], None);
    assert_eq!(runtime.motion_completions, 0);
    runtime.tick().unwrap();
    assert_eq!(runtime.token_positions[0], Some(far));
    assert_eq!(runtime.project(far), Some(Block::Stone {}));
    assert!(runtime.piston_head_present[0]);
    assert!(matches!(runtime.project(home), Some(Block::PistonHead { head }) if head.facing == BlockFacing::East && head.sticky));
    assert_eq!(runtime.motion_completions, 1);
}
#[test]
fn normal_and_early_retract_requests_keep_distinct_queue_identities() {
    use super::executor::PistonEvent;
    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let mut world = host();
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        home,
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::East,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(far, Block::Stone {});
    let actors = discover_piston_actors(&world, (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20)), entry(&world)).unwrap();
    let tokens = super::compiler::discover_piston_tokens(&world, &actors).unwrap();
    let mut program = test_program(vec![base], vec![world.get_block(base)], vec![], vec![], vec![], vec![]).unwrap();
    program.pistons = actors;
    program.tokens = tokens;
    let mut runtime = program.start();
    runtime.update_piston(0);
    runtime.update_piston(0);
    runtime.motions.push(super::executor::Motion {
        piston: 0,
        token: Some(0),
        destination: far,
        start: 0,
        landing: 2,
        order: 0,
        extending: true,
        piston_done: false,
        token_done: false,
    });
    runtime.update_piston(0);
    runtime.update_piston(0);
    assert_eq!(
        runtime.piston_events.into_iter().collect::<Vec<_>>(),
        vec![(0, PistonEvent::Retract), (0, PistonEvent::EarlyRetract)]
    );
}

#[test]
fn landing_order_that_already_passed_requests_normal_retraction() {
    use super::executor::PistonEvent;

    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let mut program = test_program(
        vec![base],
        vec![Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: true,
            },
        }],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    program.pistons.push(super::compiler::PistonActor {
        id: 0,
        base,
        facing: BlockFacing::East,
        sticky: true,
        home,
        far,
        extended: true,
        head_present: false,
        group: 0,
    });
    program.tokens.push(super::compiler::PistonToken {
        block: Block::Stone {},
        position: None,
        cells: vec![far],
    });

    let request = |motion_time, order, cursor| {
        let mut runtime = program.start();
        runtime.motion_time = motion_time;
        runtime.motion_phase = true;
        runtime.landing_cursor = Some(cursor);
        runtime.motions.push(super::executor::Motion {
            piston: 0,
            token: Some(0),
            destination: far,
            start: 0,
            landing: 2,
            order,
            extending: true,
            piston_done: false,
            token_done: false,
        });
        runtime.update_piston(0);
        runtime.piston_events.into_iter().collect::<Vec<_>>()
    };

    assert_eq!(request(2, 0, 1), vec![(0, PistonEvent::EarlyRetract)]);
    assert_eq!(request(2, 1, 0), vec![(0, PistonEvent::Retract)]);
    assert_eq!(request(2, 1, 1), vec![(0, PistonEvent::Retract)]);
    assert_eq!(request(1, 0, 1), vec![(0, PistonEvent::EarlyRetract)]);
}

#[test]
fn landing_callbacks_preserve_both_motion_orders() {
    use super::{executor::PistonEvent, program::MoveStep};

    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let callback_cell = BlockPos::new(20, 15, 15);
    let mut program = test_program(
        vec![base],
        vec![Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: true,
            },
        }],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    program.pistons.push(super::compiler::PistonActor {
        id: 0,
        base,
        facing: BlockFacing::East,
        sticky: false,
        home,
        far,
        extended: true,
        head_present: false,
        group: 0,
    });
    program.tokens = vec![
        super::compiler::PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![far],
        },
        super::compiler::PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![callback_cell],
        },
    ];
    program.move_steps = vec![
        vec![[Vec::new(), Vec::new()]],
        vec![[
            Vec::new(),
            vec![MoveStep {
                node: 0,
                watcher: true,
                wire: false,
                wires: vec![],
            }],
        ]],
    ];

    let events_for_order = |callback_first: bool| {
        let pushing_order = usize::from(callback_first);
        let callback_order = usize::from(!callback_first);
        let mut runtime = program.start();
        runtime.motion_time = 1;
        runtime.piston_head_moving[0] = true;
        runtime.motions = vec![
            super::executor::Motion {
                piston: 0,
                token: Some(0),
                destination: far,
                start: 0,
                landing: 2,
                order: pushing_order,
                extending: true,
                piston_done: false,
                token_done: false,
            },
            super::executor::Motion {
                piston: 0,
                token: Some(1),
                destination: callback_cell,
                start: 0,
                landing: 2,
                order: callback_order,
                extending: true,
                piston_done: true,
                token_done: false,
            },
        ];
        runtime.tick().unwrap();
        runtime.piston_events.into_iter().collect::<Vec<_>>()
    };

    assert_eq!(events_for_order(false), vec![(0, PistonEvent::Retract)]);
    assert_eq!(events_for_order(true), vec![(0, PistonEvent::Retract)]);
}

#[test]
fn interrupted_extension_exports_only_its_unfinished_parts() {
    use super::executor::PistonEvent;

    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    for sticky in [false, true] {
        let mut program = test_program(
            vec![base],
            vec![Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky,
                    extended: true,
                },
            }],
            vec![],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        program.pistons.push(super::compiler::PistonActor {
            id: 0,
            base,
            facing: BlockFacing::East,
            sticky,
            home,
            far,
            extended: true,
            head_present: false,
            group: 0,
        });
        program.tokens.push(super::compiler::PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![far],
        });

        let mut runtime = program.start();
        runtime.piston_head_moving[0] = true;
        runtime.motions.push(super::executor::Motion {
            piston: 0,
            token: Some(0),
            destination: far,
            start: 0,
            landing: 2,
            order: 0,
            extending: true,
            piston_done: false,
            token_done: false,
        });
        runtime.update_piston(0);
        assert_eq!(runtime.piston_events.front(), Some(&(0, PistonEvent::EarlyRetract)));

        runtime.tick().unwrap();

        assert!(runtime.motions[0].piston_done);
        assert_eq!(runtime.motions[0].token_done, sticky);
        let parts = runtime.moving_parts().unwrap();
        assert_eq!(parts.len(), if sticky { 1 } else { 2 });
        assert!(parts.iter().any(|part| part.pos == base && part.entity.source));
        assert!(!parts.iter().any(|part| part.pos == home));
        assert_eq!(runtime.project(far), if sticky { Some(Block::Stone {}) } else { None });
        assert_eq!(parts.iter().any(|part| part.pos == far), !sticky);
    }
}

fn entry(w: &PlotWorld) -> ScratchEntry {
    ScratchEntry {
        scheduled: w.scheduler().iter_entries().collect(),
        piston: w.piston_state().clone(),
    }
}
fn assert_repeater_powered(runtime: &Runtime, powered: bool) {
    assert!(matches!(runtime.blocks[1], Block::RedstoneRepeater { repeater } if repeater.powered == powered));
}
#[test]
fn repeater_delays_priorities_short_pulses_and_rearm() {
    for delay in 1..=4 {
        let program = repeater_program(delay, false, false, false);
        let mut runtime = program.start();
        runtime.set_input(0, true).unwrap();
        let queued = runtime.pending.iter_entries().next().unwrap();
        assert_eq!(queued.ticks_left, u32::from(delay) * 2);
        assert_eq!(queued.tick_priority, TickPriority::High);
        let due_in = queued.ticks_left;
        runtime.update_repeater(1);
        assert_eq!(runtime.pending.iter_entries().count(), 1);
        assert_eq!(runtime.pending.iter_entries().next().unwrap().ticks_left, due_in);
        for _ in 1..u32::from(delay) * 2 {
            runtime.tick().unwrap();
            assert_repeater_powered(&runtime, false);
            assert_eq!(runtime.scheduled_repeater_ticks, 0);
        }
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, true);
        assert_eq!(runtime.blocks[2], Block::RedstoneLamp { lit: true });
        assert_eq!(runtime.scheduled_repeater_ticks, 1);

        runtime.set_input(0, false).unwrap();
        let queued = runtime.pending.iter_entries().next().unwrap();
        assert_eq!(queued.ticks_left, u32::from(delay) * 2);
        assert_eq!(queued.tick_priority, TickPriority::Higher);
        for _ in 1..u32::from(delay) * 2 {
            runtime.tick().unwrap();
            assert_repeater_powered(&runtime, true);
        }
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, false);
        assert_eq!(runtime.scheduled_repeater_ticks, 2);
        assert_eq!(runtime.pending.iter_entries().count(), 1);
        for tick in 1..=4 {
            runtime.tick().unwrap();
            assert_eq!(runtime.blocks[2], Block::RedstoneLamp { lit: tick < 4 });
        }
        assert!(runtime.pending.iter_entries().next().is_none());
    }

    let diode_program = repeater_program(2, false, false, true);
    let mut diode = diode_program.start();
    diode.set_input(0, true).unwrap();
    assert_eq!(diode.pending.iter_entries().next().unwrap().tick_priority, TickPriority::Highest);
    let loaded_fall_program = repeater_program(2, true, false, false);
    let mut falling = loaded_fall_program.start();
    falling.set_input(0, true).unwrap();
    falling.tick().unwrap();
    falling.tick().unwrap();
    falling.set_input(0, false).unwrap();
    assert_eq!(falling.pending.iter_entries().next().unwrap().tick_priority, TickPriority::Higher);

    let short_program = repeater_program(2, false, false, false);
    let mut short = short_program.start();
    short.set_input(0, true).unwrap();
    let first = short.pending.iter_entries().next().unwrap();
    short.set_input(0, false).unwrap();
    assert_eq!(short.pending.iter_entries().count(), 1);
    assert_eq!(short.pending.iter_entries().next().unwrap().ticks_left, first.ticks_left);
    for _ in 0..3 {
        short.tick().unwrap();
        assert_repeater_powered(&short, false);
    }
    short.tick().unwrap();
    assert_repeater_powered(&short, true);
    assert_eq!(short.pending.iter_entries().next().unwrap().tick_priority, TickPriority::Higher);
    for _ in 0..3 {
        short.tick().unwrap();
        assert_repeater_powered(&short, true);
    }
    short.tick().unwrap();
    assert_repeater_powered(&short, false);
    assert_eq!(short.scheduled_repeater_ticks, 2);
}
#[test]
fn repeater_loaded_output_locking_and_repeated_compiled_propagation() {
    let loaded_program = repeater_program(1, true, false, false);
    let loaded = loaded_program.start();
    assert_repeater_powered(&loaded, true);
    assert_eq!(loaded.contributions[1], 15);
    assert_eq!(loaded.blocks[2], Block::RedstoneLamp { lit: true });
    assert!(loaded.pending.iter_entries().next().is_none());
    assert_eq!(loaded.deliveries, 0);

    let locked_program = repeater_program(1, false, true, false);
    let mut locked = locked_program.start();
    locked.set_input(0, true).unwrap();
    assert!(locked.pending.iter_entries().next().is_none());
    let tick = ScheduledBlockTick {
        pos: locked.program.positions[1],
        block_type: Some(locked.blocks[1].registry_id()),
    };
    locked.pending.schedule_tick(tick, 1, TickPriority::Normal);
    locked.tick().unwrap();
    locked.tick().unwrap();
    assert_repeater_powered(&locked, false);
    assert_eq!(locked.scheduled_repeater_ticks, 1);
    assert!(locked.pending.iter_entries().next().is_none());

    let program = repeater_program(1, false, false, false);
    let mut runtime = program.start();
    for cycle in 1..=3 {
        runtime.set_input(0, true).unwrap();
        assert_eq!(runtime.pending.iter_entries().count(), 1);
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, false);
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, true);
        assert_eq!(runtime.contributions[1], 15);
        assert_eq!(runtime.blocks[2], Block::RedstoneLamp { lit: true });
        runtime.set_input(0, false).unwrap();
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, true);
        runtime.tick().unwrap();
        assert_repeater_powered(&runtime, false);
        assert_eq!(runtime.contributions[1], 0);
        assert_eq!(runtime.pending.iter_entries().count(), 1);
        for tick in 1..=4 {
            runtime.tick().unwrap();
            assert_eq!(runtime.blocks[2], Block::RedstoneLamp { lit: tick < 4 });
        }
        assert!(runtime.pending.iter_entries().next().is_none());
        assert_eq!(runtime.scheduled_repeater_ticks, cycle * 2);
    }
    assert!(runtime.operations > 0);
}
#[test]
fn malformed_repeater_program_records_and_instructions_reject() {
    let program = repeater_program(1, false, false, false);
    let mut outputs = program.output_programs.clone();
    outputs[0][1] = vec![Operation::UpdateRepeater(2)];
    assert_eq!(
        Program::new(
            program.positions.clone(),
            program.initial.clone(),
            program.sources.clone(),
            outputs,
            program.links.clone(),
            vec![],
            vec![RepeaterRecord {
                component: 1,
                output_is_diode: false,
                side_sources: None
            }]
        )
        .err()
        .unwrap()
        .reason,
        "sampled program UpdateRepeater target is not a repeater"
    );
    let outputs = program.output_programs.clone();
    assert_eq!(
        Program::new(
            program.positions.clone(),
            program.initial.clone(),
            program.sources.clone(),
            outputs.clone(),
            program.links.clone(),
            vec![],
            vec![]
        )
        .err()
        .unwrap()
        .reason,
        "sampled repeater has no repeater record"
    );
    assert_eq!(
        Program::new(
            program.positions.clone(),
            program.initial.clone(),
            program.sources.clone(),
            outputs.clone(),
            program.links.clone(),
            vec![],
            vec![
                RepeaterRecord {
                    component: 1,
                    output_is_diode: false,
                    side_sources: None
                },
                RepeaterRecord {
                    component: 1,
                    output_is_diode: true,
                    side_sources: None
                }
            ]
        )
        .err()
        .unwrap()
        .reason,
        "sampled repeater record is duplicated"
    );
    assert_eq!(
        Program::new(
            program.positions.clone(),
            program.initial.clone(),
            program.sources.clone(),
            outputs.clone(),
            program.links.clone(),
            vec![],
            vec![RepeaterRecord {
                component: 3,
                output_is_diode: false,
                side_sources: None
            }]
        )
        .err()
        .unwrap()
        .reason,
        "sampled repeater record index is not a repeater"
    );
    assert_eq!(
        Program::new(
            program.positions.clone(),
            program.initial.clone(),
            program.sources.clone(),
            vec![],
            program.links.clone(),
            vec![],
            vec![RepeaterRecord {
                component: 1,
                output_is_diode: false,
                side_sources: None
            }]
        )
        .err()
        .unwrap()
        .reason,
        "sampled output program count mismatch"
    );
}
#[test]
fn explicit_deliveries_repeat_while_watchers_update_once() {
    let program = test_program(
        vec![BlockPos::new(0, 15, 15), BlockPos::new(1, 15, 15)],
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
            Block::RedstoneLamp { lit: false },
        ],
        vec![0],
        vec![[vec![], vec![Operation::UpdateLamp(1), Operation::UpdateLamp(1)]]],
        vec![],
        vec![],
    )
    .unwrap();
    let mut runtime = program.start();
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.operations, 2);
    assert_eq!(runtime.deliveries, 2);
    runtime.update_once(1);
    runtime.update_once(1);
    assert_eq!(runtime.deliveries, 3);
    runtime.dispatch_receiver(1);
    assert_eq!(runtime.deliveries, 4);
    assert!(runtime.pending.iter_entries().next().is_none());
}
#[test]
fn empty() {
    let w = host();
    let p = compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)).unwrap();
    p.start().tick().unwrap();
}

#[test]
fn runtime_stops_at_semantic_operation_limit() {
    let program = repeater_program(1, false, false, false);
    let mut runtime = program.start();
    runtime.tick_operations = 999_999;
    let error = runtime.set_input(0, true).unwrap_err();
    assert_eq!(error.position, Some(program.positions[0]));
    assert_eq!(error.reason, "runtime semantic operation limit exceeded");
    assert_eq!(runtime.tick().unwrap_err(), error);
}
#[test]
fn competing_sources_preserve_attenuation_masking_and_lamp_delay() {
    let lever = Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
    };
    // Source indexes intentionally differ from component indexes.
    let operations = vec![
        Operation::Contribution(0),
        Operation::Contribution(1),
        Operation::RefreshWire(1),
        Operation::Contribution(2),
        Operation::UpdateLamp(3),
    ];
    let program = test_program(
        (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
        vec![
            lever,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
            lever,
            Block::RedstoneLamp { lit: false },
        ],
        vec![2, 0],
        vec![[operations.clone(), operations.clone()], [operations.clone(), operations]],
        vec![
            ElectricalLink {
                source: 2,
                target: 1,
                attenuation: 2,
            },
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 3,
                attenuation: 0,
            },
        ],
        vec![],
    )
    .unwrap();
    let mut runtime = program.start();
    for (source, powered, expected) in [(0, true, 13), (1, true, 15), (0, false, 15), (0, true, 15), (1, false, 13), (0, false, 0)] {
        runtime.set_input(source, powered).unwrap();
        let Block::RedstoneWire { wire } = runtime.blocks[1] else {
            panic!("expected wire")
        };
        assert_eq!(wire.power, expected);
        assert_eq!(runtime.input_strength(1), expected);
        assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
        if expected > 0 {
            assert!(runtime.pending.iter_entries().next().is_none());
        } else {
            assert_eq!(runtime.pending.iter_entries().count(), 1);
            for tick in 1..=4 {
                runtime.tick().unwrap();
                assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: tick < 4 });
                assert_eq!(runtime.pending.iter_entries().count(), usize::from(tick < 4));
            }
        }
    }
    assert_eq!(runtime.operations, 31);
    // The external input is legal again after scheduled work settles.
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.input_strength(1), 13);
    assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
}
#[test]
fn electrical_change_without_notification_preserves_lamp_and_queue() {
    let program = test_program(
        vec![BlockPos::new(0, 15, 15), BlockPos::new(1, 15, 15)],
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            Block::RedstoneLamp { lit: true },
        ],
        vec![0],
        vec![[vec![Operation::Contribution(0)], vec![Operation::Contribution(0)]]],
        vec![ElectricalLink {
            source: 0,
            target: 1,
            attenuation: 2,
        }],
        vec![],
    )
    .unwrap();
    let mut runtime = program.start();
    assert_eq!(runtime.input_strength(1), 13);
    for (powered, strength) in [(false, 0), (true, 13)] {
        runtime.set_input(0, powered).unwrap();
        assert_eq!(runtime.input_strength(1), strength);
        assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: true });
        assert!(runtime.pending.iter_entries().next().is_none());
    }
    assert_eq!(runtime.operations, 2);
}
#[test]
fn contribution_order_does_not_implicitly_notify_receiver() {
    let positions = vec![BlockPos::new(0, 15, 15), BlockPos::new(1, 15, 15)];
    let initial = vec![
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
        Block::RedstoneLamp { lit: false },
    ];
    let link = ElectricalLink {
        source: 0,
        target: 1,
        attenuation: 0,
    };
    let ordered = test_program(
        positions.clone(),
        initial.clone(),
        vec![0],
        vec![[vec![], vec![Operation::UpdateLamp(1), Operation::Contribution(0)]]],
        vec![ElectricalLink {
            source: 0,
            target: 1,
            attenuation: 0,
        }],
        vec![],
    )
    .unwrap();
    let mut runtime = ordered.start();
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.input_strength(1), 15);
    assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: false });

    let reversed = test_program(
        positions,
        initial,
        vec![0],
        vec![[vec![], vec![Operation::Contribution(0), Operation::UpdateLamp(1)]]],
        vec![link],
        vec![],
    )
    .unwrap();
    let mut runtime = reversed.start();
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.input_strength(1), 15);
    assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: true });
}
#[test]
fn watched_wire_observes_raw_strength_changes_only() {
    let lever = Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
    };
    let wire = Block::RedstoneWire {
        wire: RedstoneWire {
            power: 15,
            ..Default::default()
        },
    };
    let lamp = Block::RedstoneLamp { lit: true };
    let a_ops = vec![Operation::Contribution(0), Operation::CheckWatchedWire(0), Operation::CheckWatchedWire(0)];
    let b_ops = vec![Operation::Contribution(1), Operation::CheckWatchedWire(0), Operation::CheckWatchedWire(0)];
    let program = test_program(
        (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
        vec![lever, wire, lever, lamp],
        vec![0, 2],
        vec![[a_ops.clone(), a_ops], [b_ops.clone(), b_ops]],
        vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 2,
                target: 1,
                attenuation: 2,
            },
        ],
        vec![WatchedWire {
            wire: 1,
            watchers: vec![3, 3],
        }],
    )
    .unwrap();
    let mut runtime = program.start();
    assert_eq!(runtime.program.watched_wires[0].watchers, vec![3]);
    runtime.set_input(1, false).unwrap();
    assert_eq!(runtime.input_strength(1), 15, "source change is masked by the stronger source");
    assert_eq!(runtime.watched_strengths[0], 15);
    assert_eq!(runtime.deliveries, 0);

    runtime.set_input(1, true).unwrap();
    assert_eq!(runtime.input_strength(1), 15);
    assert_eq!(runtime.deliveries, 0);
    runtime.set_input(0, false).unwrap();
    assert_eq!(runtime.input_strength(1), 13);
    assert_eq!(runtime.watched_strengths[0], 13);
    assert_eq!(runtime.blocks[1], wire, "watch checks do not write physical wire state");
    assert_eq!(runtime.deliveries, 1, "15 to 13 is still a raw strength change");
}
#[test]
fn watched_wire_notification_is_separate_from_lamp_power() {
    let program = test_program(
        (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                },
            },
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            Block::RedstoneLamp { lit: true },
        ],
        vec![0],
        vec![[
            vec![Operation::Contribution(0), Operation::CheckWatchedWire(0)],
            vec![Operation::Contribution(0), Operation::CheckWatchedWire(0)],
        ]],
        vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 2,
                target: 3,
                attenuation: 0,
            },
        ],
        vec![WatchedWire { wire: 1, watchers: vec![3] }],
    )
    .unwrap();
    let mut runtime = program.start();
    runtime.set_input(0, false).unwrap();
    assert_eq!(runtime.input_strength(3), 15);
    assert_eq!(runtime.watched_strengths[0], 0);
    assert_eq!(
        runtime.blocks[1],
        Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..Default::default()
            }
        }
    );
    assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
    assert_eq!(runtime.deliveries, 1);
}
#[test]
fn watcher_updates_are_suppressed_per_phase() {
    let program = test_program(
        (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                },
            },
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                },
            },
            Block::RedstoneLamp { lit: false },
        ],
        vec![0],
        vec![[
            vec![
                Operation::Contribution(0),
                Operation::CheckWatchedWire(0),
                Operation::Contribution(1),
                Operation::CheckWatchedWire(1),
            ],
            vec![
                Operation::Contribution(0),
                Operation::CheckWatchedWire(0),
                Operation::Contribution(1),
                Operation::CheckWatchedWire(1),
            ],
        ]],
        vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 0,
                target: 2,
                attenuation: 0,
            },
        ],
        vec![WatchedWire { wire: 1, watchers: vec![3] }, WatchedWire { wire: 2, watchers: vec![3] }],
    )
    .unwrap();
    let mut runtime = program.start();
    runtime.set_input(0, false).unwrap();
    assert_eq!(runtime.watched_strengths, vec![0, 0]);
    assert_eq!(runtime.deliveries, 1, "two changed watched wires notify the same lamp once in one phase");
}
#[test]
fn watcher_update_once_invalidates_on_effective_input_change_and_resets_per_phase() {
    let operations_off = vec![
        Operation::Contribution(0),
        Operation::CheckWatchedWire(0),
        Operation::UpdateLamp(2),
        Operation::Contribution(2),
        Operation::Contribution(1),
        Operation::CheckWatchedWire(0),
        Operation::CheckWatchedWire(0),
    ];
    let operations_on = vec![
        Operation::Contribution(0),
        Operation::CheckWatchedWire(0),
        Operation::Contribution(2),
        Operation::Contribution(1),
        Operation::CheckWatchedWire(0),
    ];
    let program = test_program(
        (0..3).map(|x| BlockPos::new(x, 15, 15)).collect(),
        vec![
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                },
            },
            Block::RedstoneLamp { lit: false },
        ],
        vec![0],
        vec![[operations_off, operations_on]],
        vec![
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            },
            ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 2,
            },
            ElectricalLink {
                source: 0,
                target: 2,
                attenuation: 0,
            },
        ],
        vec![WatchedWire {
            wire: 1,
            watchers: vec![2, 2],
        }],
    )
    .unwrap();
    let mut runtime = program.start();
    runtime.set_input(0, false).unwrap();
    assert_eq!(runtime.watched_strengths[0], 0);
    assert_eq!(runtime.deliveries, 3, "two watcher updates plus the independent explicit delivery");
    let phase = runtime.phase;
    for _ in 0..4 {
        runtime.tick().unwrap();
    }
    assert_eq!(runtime.phase, phase + 4, "each scheduler tick starts an update phase");
    assert!(runtime.pending.iter_entries().next().is_none());
    runtime.set_input(0, true).unwrap();
    assert_eq!(runtime.watched_strengths[0], 15);
    assert_eq!(runtime.deliveries, 4, "the next external transition starts a fresh update phase");
}
#[test]
fn watched_wire_level_changes_invalidate_even_when_masked() {
    let positions = (0..3).map(|x| BlockPos::new(x, 15, 15)).collect();
    let wire = |power| Block::RedstoneWire {
        wire: RedstoneWire { power, ..Default::default() },
    };
    let mut output_programs = vec![[vec![], vec![]]; 3];
    output_programs[0][0].push(Operation::Contribution(0));
    output_programs[1][0].push(Operation::Contribution(1));
    let program = Program::new(
        positions,
        vec![wire(7), wire(10), wire(10)],
        vec![],
        output_programs,
        vec![
            ElectricalLink {
                source: 0,
                target: 2,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 2,
                attenuation: 0,
            },
        ],
        vec![WatchedWire { wire: 2, watchers: vec![] }],
        vec![],
    )
    .unwrap();
    let mut runtime = program.start();
    runtime.updated_in_phase[2] = Some(runtime.phase);
    runtime.blocks[0] = wire(6);
    runtime.execute_output_program(0, false);
    assert_eq!(runtime.updated_in_phase[2], None);
    assert_eq!(runtime.input_strength(2), 10);
    runtime.blocks[1] = wire(0);
    runtime.execute_output_program(1, false);
    assert_eq!(runtime.input_strength(2), 6);
}
#[test]
fn program_construction_rejects_malformed_records() {
    let positions = vec![BlockPos::new(0, 0, 0), BlockPos::new(1, 0, 0), BlockPos::new(2, 0, 0)];
    let initial = vec![
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
        Block::RedstoneLamp { lit: false },
    ];
    let links = vec![
        ElectricalLink {
            source: 0,
            target: 1,
            attenuation: 0,
        },
        ElectricalLink {
            source: 1,
            target: 2,
            attenuation: 0,
        },
    ];
    let ops = vec![Operation::Contribution(0), Operation::RefreshWire(1), Operation::UpdateLamp(2)];
    let transitions = vec![[ops.clone(), ops.clone()]];
    let make = |positions, initial, sources, transitions, links| test_program(positions, initial, sources, transitions, links, vec![]);
    let assert_reason = |result: Result<Program, ScratchError>, reason| {
        assert_eq!(result.err().expect("malformed program was accepted").reason, reason);
    };
    assert!(make(positions.clone(), initial.clone(), vec![0], vec![[vec![], vec![]]], links.clone()).is_ok());

    assert_reason(
        make(positions[..2].to_vec(), initial.clone(), vec![0], transitions.clone(), links.clone()),
        "sampled program position/initial length mismatch",
    );
    assert_reason(
        make(vec![positions[0]; 3], initial.clone(), vec![0], transitions.clone(), links.clone()),
        "sampled program has duplicate component positions",
    );
    assert_reason(
        make(
            positions.clone(),
            vec![initial[0], Block::Stone {}, initial[2]],
            vec![0],
            transitions.clone(),
            links.clone(),
        ),
        "sampled program contains unsupported component",
    );
    assert_reason(
        make(positions.clone(), initial.clone(), vec![3], transitions.clone(), links.clone()),
        "sampled program source index is out of bounds",
    );
    assert_reason(
        make(positions.clone(), initial.clone(), vec![0, 0], vec![[vec![], vec![]]; 2], links.clone()),
        "sampled program source is duplicated",
    );
    assert_reason(
        make(positions.clone(), initial.clone(), vec![1], vec![[vec![], vec![]]], links.clone()),
        "sampled program source is not a lever",
    );
    assert_reason(
        make(positions.clone(), initial.clone(), vec![0], vec![], links.clone()),
        "sampled program source transition count mismatch",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            transitions.clone(),
            vec![ElectricalLink {
                source: 3,
                target: 1,
                attenuation: 0,
            }],
        ),
        "sampled program link source is out of bounds",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            transitions.clone(),
            vec![ElectricalLink {
                source: 0,
                target: 3,
                attenuation: 0,
            }],
        ),
        "sampled program link target is out of bounds",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            transitions.clone(),
            vec![ElectricalLink {
                source: 2,
                target: 1,
                attenuation: 0,
            }],
        ),
        "sampled program link source must be lever, wire or repeater",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            transitions.clone(),
            vec![ElectricalLink {
                source: 0,
                target: 0,
                attenuation: 0,
            }],
        ),
        "sampled program link target must be wire, lamp or repeater",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            transitions.clone(),
            vec![ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 16,
            }],
        ),
        "sampled program attenuation exceeds 15",
    );
    let mut overpowered_wire = initial.clone();
    overpowered_wire[1] = Block::RedstoneWire {
        wire: RedstoneWire {
            power: 16,
            ..Default::default()
        },
    };
    assert_reason(
        make(positions.clone(), overpowered_wire, vec![0], transitions.clone(), links.clone()),
        "sampled program wire power exceeds 15",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![Operation::Contribution(2)], vec![]]],
            links.clone(),
        ),
        "sampled program contribution link is out of bounds",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![Operation::RefreshWire(2)], vec![]]],
            links.clone(),
        ),
        "sampled program RefreshWire target is not a wire",
    );
    assert_reason(
        make(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![Operation::UpdateLamp(1)], vec![]]],
            links.clone(),
        ),
        "sampled program UpdateLamp target is not a lamp",
    );
    assert_reason(
        test_program(
            vec![BlockPos::new(0, 0, 0)],
            vec![Block::RedstoneWire {
                wire: RedstoneWire::default(),
            }],
            vec![],
            vec![],
            vec![],
            vec![WatchedWire { wire: 1, watchers: vec![] }],
        ),
        "sampled watched-wire index is not a wire",
    );
    assert_reason(
        test_program(
            vec![BlockPos::new(0, 0, 0)],
            vec![Block::RedstoneLamp { lit: false }],
            vec![],
            vec![],
            vec![],
            vec![WatchedWire { wire: 0, watchers: vec![] }],
        ),
        "sampled watched-wire index is not a wire",
    );
    assert_reason(
        test_program(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![Operation::CheckWatchedWire(0)], vec![]]],
            links.clone(),
            vec![],
        ),
        "sampled program watched-wire index is out of bounds",
    );
    assert_reason(
        test_program(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![], vec![]]],
            links.clone(),
            vec![WatchedWire { wire: 3, watchers: vec![] }],
        ),
        "sampled watched-wire index is not a wire",
    );
    assert_reason(
        test_program(
            positions.clone(),
            initial.clone(),
            vec![0],
            vec![[vec![], vec![]]],
            links.clone(),
            vec![WatchedWire { wire: 1, watchers: vec![3] }],
        ),
        "sampled watched-wire receiver is not a lamp, piston or observer",
    );
    assert_reason(
        test_program(
            positions,
            initial,
            vec![0],
            vec![[vec![], vec![]]],
            links,
            vec![WatchedWire { wire: 1, watchers: vec![1] }],
        ),
        "sampled watched-wire receiver is not a lamp, piston or observer",
    );
}
#[test]
fn rejects_unrepresented_blocks() {
    let mut w = host();
    let pos = BlockPos::new(15, 15, 15);
    w.set_block(pos, Block::from_name("note_block").unwrap());
    let err = match compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)) {
        Err(err) => err,
        Ok(_) => panic!("unsupported block admitted"),
    };
    assert_eq!(err.position, Some(pos));
}
#[test]
fn passive_geometry_preserves_material_properties_and_can_move() {
    let mut world = host();
    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let sandstone = base.offset(BlockFace::North);
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
    world.set_block(home, Block::Glass {});
    world.set_block(sandstone, Block::Sandstone {});
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let program = compile(&world, bounds, entry(&world)).unwrap();
    assert_eq!(program.tokens[0].block, Block::Glass {});
    assert_eq!(program.start().project(sandstone), Some(Block::Sandstone {}));
    assert!(!world.get_block(home).is_solid());
    assert!(world.get_block(sandstone).is_solid());
}

#[test]
fn projection_skips_stationary_geometry_but_keeps_components() {
    let mut world = host();
    let lamp = BlockPos::new(12, 15, 12);
    let stone = BlockPos::new(13, 15, 12);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(stone, Block::Stone {});
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(15, 20, 15));
    let program = compile(&world, bounds, entry(&world)).unwrap();

    assert_eq!(program.start().projected_blocks(), vec![(lamp, Block::RedstoneLamp { lit: false })]);
    assert_eq!(program.start().project(stone), Some(Block::Stone {}));
}
#[test]
fn rejects_unrelated_block_entities() {
    let mut w = host();
    let pos = BlockPos::new(15, 15, 15);
    w.set_block_entity(pos, BlockEntity::Comparator { output_strength: 0 });
    let error = match compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)) {
        Err(error) => error,
        Ok(_) => panic!("unsupported entity admitted"),
    };
    assert_eq!(error.position, Some(pos));
    assert_eq!(error.reason, "unsupported block entity");
}

#[test]
fn rejects_bottom_slab_as_dust_support() {
    use mchprs_blocks::blocks::SlabType;
    let mut w = host();
    let source = BlockPos::new(15, 15, 15);
    let wire = BlockPos::new(16, 15, 15);
    w.set_block(
        source,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    w.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    w.set_block(BlockPos::new(17, 15, 15), Block::RedstoneLamp { lit: false });
    w.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    w.set_block(
        wire.offset(BlockFace::Bottom),
        Block::SmoothStoneSlab {}.with_slab_type(SlabType::Bottom).unwrap(),
    );
    let error = match compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20)), entry(&w)) {
        Err(error) => error,
        Ok(_) => panic!("bottom slab admitted as dust support"),
    };
    assert_eq!(error.position, Some(wire));
    assert_eq!(error.reason, "invalid attachment support");
}

#[test]
fn lamp_input_discovery_keeps_off_sources_and_expands_only_eligible_contacts() {
    let lamp = BlockPos::new(20, 20, 20);
    let lever_pos = lamp.offset(BlockFace::West);
    let mut world = host();
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        lever_pos,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    assert_eq!(
        discover_lamp_sources(&world, lamp)
            .unwrap()
            .iter()
            .map(|source| (source.source, source.attenuation))
            .collect::<Vec<_>>(),
        [(lever_pos, 0)],
    );

    let mut dust_world = host();
    let dust = lamp.offset(BlockFace::West);
    let dust_source = dust.offset(BlockFace::West);
    dust_world.set_block(lamp, Block::RedstoneLamp { lit: false });
    dust_world.set_block(
        dust,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    dust_world.set_block(
        dust_source,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    let connected = discover_lamp_sources(&dust_world, lamp).unwrap();
    assert!(connected.iter().any(|source| source.source == dust_source));
    let lamp_above_dust = dust.offset(BlockFace::Top);
    dust_world.set_block(lamp, Block::Air);
    dust_world.set_block(lamp_above_dust, Block::RedstoneLamp { lit: false });
    assert!(
        !discover_lamp_sources(&dust_world, lamp_above_dust)
            .unwrap()
            .iter()
            .any(|source| source.source == dust_source)
    );
}

#[test]
fn lamp_input_conduction_is_one_hop_and_merges_minimum_attenuation() {
    let lamp = BlockPos::new(20, 20, 20);
    let conductor = lamp.offset(BlockFace::West);
    let lever_pos = conductor.offset(BlockFace::Top);
    let mut world = host();
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(conductor, Block::Stone {});
    world.set_block(
        lever_pos,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    assert_eq!(
        discover_lamp_sources(&world, lamp)
            .unwrap()
            .iter()
            .map(|source| (source.source, source.attenuation))
            .collect::<Vec<_>>(),
        [(lever_pos, 0)],
    );
    world.set_block(conductor.offset(BlockFace::West), Block::Stone {});
    world.set_block(lever_pos, Block::Air);
    let beyond_second_conductor = conductor.offset(BlockFace::West).offset(BlockFace::Top);
    world.set_block(
        beyond_second_conductor,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    assert!(discover_lamp_sources(&world, lamp).unwrap().is_empty());

    let lamp = BlockPos::new(30, 20, 30);
    let direct = lamp.offset(BlockFace::West);
    let root = lamp.offset(BlockFace::Top);
    let bridge = root.offset(BlockFace::West);
    let mut world = host();
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        direct,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for pos in [root, bridge] {
        world.set_block(
            pos,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
    }
    let dust_path = crate::redpiler::scratch::discover_wire_sources(&world, &[root]).unwrap();
    assert_eq!(dust_path.iter().find(|source| source.source == direct).unwrap().attenuation, 1);
    let merged = discover_lamp_sources(&world, lamp).unwrap();
    assert_eq!(merged.iter().filter(|source| source.source == direct).count(), 1);
    assert_eq!(merged.iter().find(|source| source.source == direct).unwrap().attenuation, 0);
}
#[test]
fn wall_lever_bent_dust_and_decorative_sign_match_native() {
    let mut w = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(23, 20, 23));
    let source = BlockPos::new(15, 15, 15);
    let wires = [BlockPos::new(16, 15, 15), BlockPos::new(17, 15, 15), BlockPos::new(17, 15, 16)];
    let lamp = BlockPos::new(17, 15, 17);
    let sign = BlockPos::new(20, 15, 20);
    w.set_block(
        source,
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::East, false),
        },
    );
    w.set_block(source.offset(BlockFace::West), Block::Stone {});
    for pos in wires {
        w.set_block(
            pos,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        w.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    w.set_block(lamp, Block::RedstoneLamp { lit: false });
    w.set_block(sign, Block::from_name("oak_sign").unwrap());
    w.set_block(sign.offset(BlockFace::Bottom), Block::Stone {});
    let mut sign_entity = mchprs_blocks::block_entities::SignBlockEntity::default();
    sign_entity.rows[0] = "{\"text\":\"keep this\"}".into();
    w.set_block_entity(sign, BlockEntity::Sign(Box::new(sign_entity)));
    let sign_before = serde_json::to_value(w.get_block_entity(sign)).unwrap();
    let scratch = ScratchWorld::from_world(&w, bounds, entry(&w)).unwrap();
    assert_eq!(w.get_block(sign), scratch.world().get_block(sign));
    assert_eq!(sign_before, serde_json::to_value(scratch.world().get_block_entity(sign)).unwrap());
    scratch.finish_probe().unwrap();
    let blocks_before: Vec<_> = (bounds.0.y..=bounds.1.y)
        .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
        .map(|pos| w.get_block_raw(pos))
        .collect();
    let program = compile(&w, bounds, entry(&w)).unwrap();
    let blocks_after: Vec<_> = (bounds.0.y..=bounds.1.y)
        .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
        .map(|pos| w.get_block_raw(pos))
        .collect();
    assert_eq!(blocks_before, blocks_after);
    assert_eq!(sign_before, serde_json::to_value(w.get_block_entity(sign)).unwrap());
    let mut runtime = program.start();
    for powered in [true, false, true, false, true, false] {
        crate::interaction::use_redstone(w.get_block(source), &mut w, source, None);
        runtime.set_input(0, powered).unwrap();
        for (index, pos) in wires.iter().enumerate() {
            let Block::RedstoneWire { wire } = w.get_block(*pos) else {
                panic!("missing wire")
            };
            assert_eq!(wire.power, if powered { 15 - index as u8 } else { 0 });
        }
        for tick in 0..=4 {
            if tick > 0 {
                w.tick_interpreted();
                runtime.tick().unwrap();
            }
            assert_eq!(runtime.blocks, program.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
            assert_eq!(
                runtime.pending.iter_entries().collect::<Vec<_>>(),
                w.scheduler().iter_entries().collect::<Vec<_>>()
            );
            assert_eq!(w.get_block(lamp), Block::RedstoneLamp { lit: powered || tick < 4 });
        }
        assert_eq!(sign_before, serde_json::to_value(w.get_block_entity(sign)).unwrap());
    }
    assert!(runtime.operations > 0);
}

#[test]
fn world_compiler_drives_empty_piston() {
    let mut world = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let lever = BlockPos::new(15, 15, 15);
    let base = BlockPos::new(16, 15, 15);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
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
    let program = compile(&world, bounds, entry(&world)).unwrap();
    assert_eq!(program.pistons.len(), 1);
    let mut runtime = program.start();
    runtime.set_input(0, true).unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    assert_eq!(runtime.accepted_piston_actions, 1);
    assert!(runtime.piston_head_present[0]);
}

#[test]
fn world_compiler_runs_repeater_and_lamp() {
    let mut w = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let lever = BlockPos::new(15, 15, 15);
    let repeater = BlockPos::new(16, 15, 15);
    let lamp = BlockPos::new(17, 15, 15);
    w.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    w.set_block(
        repeater,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                delay: 1,
                facing: BlockDirection::West,
                locked: false,
                powered: false,
            },
        },
    );
    w.set_block(lamp, Block::RedstoneLamp { lit: false });
    w.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    w.set_block(repeater.offset(BlockFace::Bottom), Block::Stone {});
    let program = compile(&w, bounds, entry(&w)).unwrap();
    let repeater_index = program.positions.iter().position(|&pos| pos == repeater).unwrap();
    let lamp_index = program.positions.iter().position(|&pos| pos == lamp).unwrap();
    assert!(
        program.output_programs[repeater_index][0]
            .iter()
            .any(|op| matches!(op, Operation::UpdateLamp(id) if *id == lamp_index))
    );
    let mut runtime = program.start();
    runtime.set_input(0, true).unwrap();
    assert!(!matches!(runtime.blocks[repeater_index], Block::RedstoneRepeater { repeater } if repeater.powered));
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    assert!(matches!(runtime.blocks[repeater_index], Block::RedstoneRepeater { repeater } if repeater.powered));
    assert_eq!(runtime.blocks[lamp_index], Block::RedstoneLamp { lit: true });
    assert!(runtime.scheduled_repeater_ticks > 0);
    runtime.set_input(0, false).unwrap();
    for _ in 0..6 {
        runtime.tick().unwrap();
    }
    assert_eq!(runtime.blocks[lamp_index], Block::RedstoneLamp { lit: false });
}

#[test]
fn world_compiler_repeater_side_lock() {
    let mut w = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let main_lever = BlockPos::new(15, 15, 15);
    let main = BlockPos::new(16, 15, 15);
    let side = BlockPos::new(16, 15, 14);
    let side_lever = BlockPos::new(16, 15, 13);
    for pos in [main_lever, side_lever] {
        w.set_block(
            pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        w.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    for (pos, facing) in [(main, BlockDirection::West), (side, BlockDirection::North)] {
        w.set_block(
            pos,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 1,
                    facing,
                    locked: false,
                    powered: false,
                },
            },
        );
        w.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    let program = compile(&w, bounds, entry(&w)).unwrap();
    let source = |pos| program.sources.iter().position(|&id| program.positions[id] == pos).unwrap();
    let main_index = program.positions.iter().position(|&pos| pos == main).unwrap();
    let mut runtime = program.start();
    runtime.set_input(source(side_lever), true).unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    assert!(matches!(runtime.blocks[main_index], Block::RedstoneRepeater { repeater } if repeater.locked));
    runtime.set_input(source(main_lever), true).unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    assert!(matches!(runtime.blocks[main_index], Block::RedstoneRepeater { repeater } if repeater.locked && !repeater.powered));
}

#[test]
fn world_compiler_runs_multiple_sources_and_lamps() {
    let mut w = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(25, 20, 25));
    let mut sources = Vec::new();
    let mut lamps = Vec::new();
    for z in [14, 20] {
        let source = BlockPos::new(15, 15, z);
        let wire = BlockPos::new(16, 15, z);
        let lamp = BlockPos::new(17, 15, z);
        w.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        w.set_block(
            wire,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        w.set_block(lamp, Block::RedstoneLamp { lit: false });
        w.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
        w.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
        sources.push(source);
        lamps.push(lamp);
    }
    let program = compile(&w, bounds, entry(&w)).unwrap();
    let mut runtime = program.start();
    for (source, lamp) in sources.iter().zip(&lamps) {
        let source_index = program.sources.iter().position(|&id| program.positions[id] == *source).unwrap();
        let lamp_index = program.positions.iter().position(|pos| pos == lamp).unwrap();
        runtime.set_input(source_index, true).unwrap();
        assert_eq!(runtime.blocks[lamp_index], Block::RedstoneLamp { lit: true });
    }
    assert!(runtime.operations > 0);
}

#[test]
fn native_edges_and_host_preservation() {
    let source = BlockPos::new(15, 15, 15);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let snapshot = |world: &PlotWorld| {
        let blocks: Vec<_> = (bounds.0.y..=bounds.1.y)
            .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
            .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
            .collect();
        (
            blocks,
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        )
    };

    let wool = Block::from_name("white_wool").unwrap();
    let slab = Block::SmoothStoneSlab {};
    for (initially_powered, supports) in [false, true].into_iter().flat_map(|on| {
        [
            (on, [Block::Stone {}, Block::Stone {}, Block::Stone {}]),
            (on, [wool, wool, wool]),
            (on, [slab, Block::Glowstone, wool]),
        ]
    }) {
        let mut w = host();
        w.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        w.set_block(
            BlockPos::new(16, 15, 15),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        w.set_block(BlockPos::new(17, 15, 15), Block::RedstoneLamp { lit: false });
        for (x, block) in (15..=17).zip(supports) {
            w.set_block(BlockPos::new(x, 14, 15), block);
        }

        if initially_powered {
            if let Block::Lever { mut lever } = w.get_block(source) {
                lever.powered = true;
                w.set_block(source, Block::Lever { lever });
            }
            crate::redstone::update_surrounding_blocks(&mut w, source);
            for _ in 0..4 {
                w.tick_interpreted();
            }
            assert_eq!(
                w.get_block(source),
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, true)
                }
            );
            assert_eq!(
                w.get_block(BlockPos::new(16, 15, 15)),
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15,
                        ..Default::default()
                    }
                }
            );
            assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
            assert!(w.scheduler().iter_entries().next().is_none());
        }

        let before = snapshot(&w);
        let p = compile(&w, bounds, entry(&w)).unwrap();
        let discovered_link = p
            .links
            .iter()
            .position(|link| {
                p.positions[link.source] == source && matches!(p.initial[link.target], Block::RedstoneWire { .. }) && link.attenuation == 0
            })
            .unwrap();
        let lamp_link = p
            .links
            .iter()
            .position(|link| p.positions[link.source] == source && p.positions[link.target] == BlockPos::new(17, 15, 15) && link.attenuation == 0)
            .unwrap();
        assert!(
            !p.links
                .iter()
                .any(|link| p.positions[link.source] == BlockPos::new(16, 15, 15) && p.positions[link.target] == BlockPos::new(17, 15, 15)),
            "lamp dependencies must use extracted source identities"
        );
        for operations in &p.output_programs[0] {
            assert!(
                operations
                    .iter()
                    .any(|op| matches!(op, Operation::Contribution(id) if *id == discovered_link))
            );
            assert!(operations.iter().any(|op| matches!(op, Operation::Contribution(id) if *id == lamp_link)));
        }
        assert_eq!(before, snapshot(&w));
        assert_eq!(p.initial, p.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
        let mut r = p.start();
        let compare = |runtime: &Runtime, world: &PlotWorld| {
            assert_eq!(runtime.blocks, p.positions.iter().map(|&pos| world.get_block(pos)).collect::<Vec<_>>());
            assert_eq!(
                runtime.pending.iter_entries().collect::<Vec<_>>(),
                world.scheduler().iter_entries().collect::<Vec<_>>()
            );
        };
        compare(&r, &w);

        let mut powered = initially_powered;
        for _ in 0..12 {
            powered = !powered;
            if let Block::Lever { mut lever } = w.get_block(source) {
                lever.powered = powered;
                w.set_block(source, Block::Lever { lever });
            }
            crate::redstone::update_surrounding_blocks(&mut w, source);
            r.set_input(0, powered).unwrap();
            compare(&r, &w);
            if !powered {
                assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
                assert_eq!(
                    r.blocks[p.initial.iter().position(|b| matches!(b, Block::RedstoneLamp { .. })).unwrap()],
                    Block::RedstoneLamp { lit: true }
                );
            }
            for tick in 1..=4 {
                w.tick_interpreted();
                r.tick().unwrap();
                compare(&r, &w);
                if !powered {
                    assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: tick < 4 });
                    assert_eq!(
                        r.blocks[p.initial.iter().position(|b| matches!(b, Block::RedstoneLamp { .. })).unwrap()],
                        Block::RedstoneLamp { lit: tick < 4 }
                    );
                }
            }
            assert!(w.scheduler().iter_entries().next().is_none());
            assert!(r.pending.iter_entries().next().is_none());
        }
        assert!(r.operations > 0);
    }
}

#[test]
fn rejects_probe_reads_outside_capture_without_mutating_host() {
    let source = BlockPos::new(15, 15, 15);
    let wire = BlockPos::new(16, 15, 15);
    let lamp = BlockPos::new(17, 15, 15);
    let full_snapshot_bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 16, 20));
    let snapshot = |world: &PlotWorld| {
        let blocks = (full_snapshot_bounds.0.y..=full_snapshot_bounds.1.y)
            .flat_map(|y| {
                (full_snapshot_bounds.0.z..=full_snapshot_bounds.1.z)
                    .flat_map(move |z| (full_snapshot_bounds.0.x..=full_snapshot_bounds.1.x).map(move |x| BlockPos::new(x, y, z)))
            })
            .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
            .collect::<Vec<_>>();
        (
            blocks,
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        )
    };
    let chain = || {
        let mut world = host();
        world.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(
            wire,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        for x in 15..=17 {
            world.set_block(BlockPos::new(x, 14, 15), Block::Stone {});
        }
        world
    };

    for (bounds, add_outside_source) in [
        ((BlockPos::new(15, 14, 15), BlockPos::new(17, 15, 15)), false),
        ((BlockPos::new(10, 10, 10), BlockPos::new(22, 15, 20)), true),
    ] {
        let mut world = chain();
        if add_outside_source {
            world.set_block(BlockPos::new(16, 16, 15), Block::RedstoneBlock {});
        }
        let before = snapshot(&world);
        let error = match compile(&world, bounds, entry(&world)) {
            Err(error) => error,
            Ok(_) => panic!("probe reading outside capture was admitted"),
        };
        assert_eq!(error.reason, "scratch probe read outside captured boundary");
        let position = error.position.expect("boundary error includes escaped read position");
        assert!(
            position.x < bounds.0.x
                || position.x > bounds.1.x
                || position.y < bounds.0.y
                || position.y > bounds.1.y
                || position.z < bounds.0.z
                || position.z > bounds.1.z,
            "reported boundary position {position:?} is inside {bounds:?}"
        );
        assert_eq!(before, snapshot(&world));
    }
}

#[test]
fn discovers_static_empty_piston_actors_and_preserves_source() {
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 22));
    let mut world = host();
    let bases = [BlockPos::new(12, 15, 12), BlockPos::new(18, 15, 12), BlockPos::new(12, 15, 17)];
    world.set_block(
        bases[0],
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    world.set_block(
        bases[1],
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: false,
                extended: true,
            },
        },
    );
    world.set_block(
        bases[1].offset(mchprs_blocks::BlockFace::South),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::South,
                sticky: false,
                short: false,
            },
        },
    );
    world.set_block(
        bases[2],
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::North,
                sticky: true,
                extended: false,
            },
        },
    );
    let before = (
        (bounds.0.y..=bounds.1.y)
            .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
            .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
            .collect::<Vec<_>>(),
        world.scheduler().iter_entries().collect::<Vec<_>>(),
        serde_json::to_value(world.piston_state()).unwrap(),
    );
    let actors = discover_piston_actors(&world, bounds, entry(&world)).unwrap();
    assert_eq!(
        actors,
        vec![
            PistonActor {
                id: 0,
                base: bases[0],
                facing: BlockFacing::East,
                sticky: false,
                home: bases[0].offset(mchprs_blocks::BlockFace::East),
                far: bases[0].offset(mchprs_blocks::BlockFace::East).offset(mchprs_blocks::BlockFace::East),
                extended: false,
                head_present: false,
                group: 0
            },
            PistonActor {
                id: 1,
                base: bases[1],
                facing: BlockFacing::South,
                sticky: false,
                home: bases[1].offset(mchprs_blocks::BlockFace::South),
                far: bases[1].offset(mchprs_blocks::BlockFace::South).offset(mchprs_blocks::BlockFace::South),
                extended: true,
                head_present: true,
                group: 1
            },
            PistonActor {
                id: 2,
                base: bases[2],
                facing: BlockFacing::North,
                sticky: true,
                home: bases[2].offset(mchprs_blocks::BlockFace::North),
                far: bases[2].offset(mchprs_blocks::BlockFace::North).offset(mchprs_blocks::BlockFace::North),
                extended: false,
                head_present: false,
                group: 2
            },
        ]
    );
    assert_eq!(
        before,
        (
            (bounds.0.y..=bounds.1.y)
                .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
                .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect::<Vec<_>>(),
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        )
    );
    assert_eq!(compile(&world, bounds, entry(&world)).unwrap().pistons.len(), 3);
}

#[test]
fn rejects_invalid_or_unreadable_piston_contacts() {
    let base = BlockPos::new(15, 15, 15);
    let facing = BlockFacing::East;
    let home = base.offset(mchprs_blocks::BlockFace::East);
    let far = home.offset(mchprs_blocks::BlockFace::East);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let mut mismatched = host();
    mismatched.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing,
                sticky: false,
                extended: true,
            },
        },
    );
    mismatched.set_block(
        home,
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::West,
                sticky: false,
                short: false,
            },
        },
    );
    let error = discover_piston_actors(&mismatched, bounds, entry(&mismatched)).unwrap_err();
    assert_eq!(error.position, Some(home));
    assert_eq!(error.reason, "piston head does not match base state");

    let mut occupied = host();
    occupied.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing,
                sticky: false,
                extended: false,
            },
        },
    );
    occupied.set_block(far, Block::Stone {});
    let actors = discover_piston_actors(&occupied, bounds, entry(&occupied)).unwrap();
    assert!(super::compiler::discover_piston_tokens(&occupied, &actors).unwrap().is_empty());

    occupied.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing,
                sticky: true,
                extended: false,
            },
        },
    );
    let actors = discover_piston_actors(&occupied, bounds, entry(&occupied)).unwrap();
    let tokens = super::compiler::discover_piston_tokens(&occupied, &actors).unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].position, Some(far));
    assert_eq!(tokens[0].block, Block::Stone {});
    assert_eq!(tokens[0].cells, vec![home, far]);

    let mut moving = host();
    moving.set_block(base, Block::MovingPiston { moving: Default::default() });
    let error = discover_piston_actors(&moving, bounds, entry(&moving)).unwrap_err();
    assert_eq!(error.position, Some(base));
    assert_eq!(error.reason, "moving piston state is unsupported");

    let mut entity = host();
    entity.set_block_entity(BlockPos::new(16, 15, 16), BlockEntity::Comparator { output_strength: 0 });
    assert!(discover_piston_actors(&entity, bounds, entry(&entity)).unwrap().is_empty());

    let mut escaped = host();
    let edge = BlockPos::new(22, 15, 15);
    escaped.set_block(
        edge,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let capture = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let error = discover_piston_actors(&escaped, capture, entry(&escaped)).unwrap_err();
    assert_eq!(error.reason, "scratch probe read outside captured boundary");
    assert!(error.position.is_some_and(|pos| pos.x > capture.1.x));
}

#[test]
fn groups_pistons_by_transitive_contact_overlap() {
    let make = |id, home, far| PistonActor {
        id,
        base: BlockPos::zero(),
        facing: BlockFacing::East,
        sticky: false,
        home,
        far,
        extended: false,
        head_present: false,
        group: usize::MAX,
    };
    let mut actors = vec![
        make(0, BlockPos::new(0, 0, 0), BlockPos::new(1, 0, 0)),
        make(1, BlockPos::new(10, 0, 0), BlockPos::new(11, 0, 0)),
        make(2, BlockPos::new(1, 0, 0), BlockPos::new(2, 0, 0)),
        make(3, BlockPos::new(2, 0, 0), BlockPos::new(3, 0, 0)),
    ];
    assign_piston_groups(&mut actors);
    assert_eq!(actors.iter().map(|actor| actor.group).collect::<Vec<_>>(), [0, 1, 0, 0]);
    assert_eq!(actors.iter().map(|actor| actor.id).collect::<Vec<_>>(), [0, 1, 2, 3]);
}
#[test]
fn preserves_pending_ticks_and_rejects_boundary() {
    let mut w = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    let pos = BlockPos::new(10, 10, 10);
    w.schedule_tick(pos, 2, mchprs_world::TickPriority::Normal);
    let entry = ScratchEntry {
        scheduled: w.scheduler().iter_entries().collect(),
        piston: w.piston_state().clone(),
    };
    let scratch = ScratchWorld::from_world(&w, (pos, pos), entry).unwrap();
    assert_eq!(
        scratch.world().scheduler().iter_entries().collect::<Vec<_>>(),
        w.scheduler().iter_entries().collect::<Vec<_>>()
    );
    let entry = ScratchEntry {
        scheduled: vec![],
        piston: w.piston_state().clone(),
    };
    assert!(compile(&w, (BlockPos::new(-1, 0, 0), pos), entry).is_err());
}
