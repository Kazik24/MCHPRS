use super::*;
use crate::redpiler::scratch::discover_lamp_sources;
    use mchprs_blocks::block_entities::BlockEntity;
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    use crate::world::storage::Chunk;
    use mchprs_blocks::{
        BlockDirection, BlockFace,
        blocks::{Lever, RedstonePiston, RedstonePistonHead, RedstoneWire},
    };
    fn host() -> PlotWorld {
        PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        )
    }
    fn entry(w: &PlotWorld) -> ScratchEntry {
        ScratchEntry {
            scheduled: w.scheduler().iter_entries().collect(),
            piston: w.piston_state().clone(),
        }
    }
    #[test]
    fn explicit_deliveries_repeat_while_watchers_update_once() {
        let program = Program::new(
            vec![BlockPos::new(0, 15, 15), BlockPos::new(1, 15, 15)],
            vec![Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) }, Block::RedstoneLamp { lit: false }],
            vec![0], vec![[vec![], vec![Operation::UpdateLamp(1), Operation::UpdateLamp(1)]]],
            vec![], vec![],
        ).unwrap();
        let mut runtime = program.start();
        runtime.set_input(0, true);
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
        p.start().tick();
    }
    #[test]
    fn competing_sources_preserve_attenuation_masking_and_lamp_delay() {
        let lever = Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) };
        // Source indexes intentionally differ from component indexes.
        let operations = vec![
            Operation::Contribution(0),
            Operation::Contribution(1),
            Operation::RefreshWire(1),
            Operation::Contribution(2),
            Operation::UpdateLamp(3),
        ];
        let program = Program::new(
            (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
            vec![lever, Block::RedstoneWire { wire: RedstoneWire::default() }, lever, Block::RedstoneLamp { lit: false }],
            vec![2, 0],
            vec![[operations.clone(), operations.clone()], [operations.clone(), operations]],
            vec![
                ElectricalLink { source: 2, target: 1, attenuation: 2 },
                ElectricalLink { source: 0, target: 1, attenuation: 0 },
                ElectricalLink { source: 1, target: 3, attenuation: 0 },
            ],
            vec![],
        ).unwrap();
        let mut runtime = program.start();
        for (source, powered, expected) in [(0, true, 13), (1, true, 15), (0, false, 15), (0, true, 15), (1, false, 13), (0, false, 0)] {
            runtime.set_input(source, powered);
            let Block::RedstoneWire { wire } = runtime.blocks[1] else { panic!("expected wire") };
            assert_eq!(wire.power, expected);
            assert_eq!(runtime.input_strength(1), expected);
            assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
            if expected > 0 {
                assert!(runtime.pending.iter_entries().next().is_none());
            } else {
                assert_eq!(runtime.pending.iter_entries().count(), 1);
                for tick in 1..=4 {
                    runtime.tick();
                    assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: tick < 4 });
                    assert_eq!(runtime.pending.iter_entries().count(), usize::from(tick < 4));
                }
            }
        }
        assert_eq!(runtime.operations, 31);
        // The external input is legal again after scheduled work settles.
        runtime.set_input(0, true);
        assert_eq!(runtime.input_strength(1), 13);
        assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
    }
    #[test]
    fn electrical_change_without_notification_preserves_lamp_and_queue() {
        let program = Program::new(
            vec![BlockPos::new(0, 15, 15), BlockPos::new(1, 15, 15)],
            vec![Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) }, Block::RedstoneLamp { lit: true }],
            vec![0], vec![[vec![Operation::Contribution(0)], vec![Operation::Contribution(0)]]],
            vec![ElectricalLink { source: 0, target: 1, attenuation: 2 }],
            vec![],
        ).unwrap();
        let mut runtime = program.start();
        assert_eq!(runtime.input_strength(1), 13);
        for (powered, strength) in [(false, 0), (true, 13)] {
            runtime.set_input(0, powered);
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
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
            Block::RedstoneLamp { lit: false },
        ];
        let link = ElectricalLink { source: 0, target: 1, attenuation: 0 };
        let ordered = Program::new(
            positions.clone(), initial.clone(), vec![0],
            vec![[vec![], vec![Operation::UpdateLamp(1), Operation::Contribution(0)]]],
            vec![ElectricalLink { source: 0, target: 1, attenuation: 0 }],
            vec![],
        ).unwrap();
        let mut runtime = ordered.start();
        runtime.set_input(0, true);
        assert_eq!(runtime.input_strength(1), 15);
        assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: false });

        let reversed = Program::new(
            positions, initial, vec![0],
            vec![[vec![], vec![Operation::Contribution(0), Operation::UpdateLamp(1)]]],
            vec![link],
            vec![],
        ).unwrap();
        let mut runtime = reversed.start();
        runtime.set_input(0, true);
        assert_eq!(runtime.input_strength(1), 15);
        assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: true });
    }
    #[test]
    fn watched_wire_observes_raw_strength_changes_only() {
        let lever = Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) };
        let wire = Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } };
        let lamp = Block::RedstoneLamp { lit: true };
        let a_ops = vec![Operation::Contribution(0), Operation::CheckWatchedWire(0), Operation::CheckWatchedWire(0)];
        let b_ops = vec![Operation::Contribution(1), Operation::CheckWatchedWire(0), Operation::CheckWatchedWire(0)];
        let program = Program::new(
            (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
            vec![lever, wire, lever, lamp],
            vec![0, 2],
            vec![[a_ops.clone(), a_ops], [b_ops.clone(), b_ops]],
            vec![
                ElectricalLink { source: 0, target: 1, attenuation: 0 },
                ElectricalLink { source: 2, target: 1, attenuation: 2 },
            ],
            vec![WatchedWire { wire: 1, watchers: vec![3, 3] }],
        ).unwrap();
        let mut runtime = program.start();
        assert_eq!(runtime.program.watched_wires[0].watchers, vec![3]);
        runtime.set_input(1, false);
        assert_eq!(runtime.input_strength(1), 15, "source change is masked by the stronger source");
        assert_eq!(runtime.watched_strengths[0], 15);
        assert_eq!(runtime.deliveries, 0);

        runtime.set_input(1, true);
        assert_eq!(runtime.input_strength(1), 15);
        assert_eq!(runtime.deliveries, 0);
        runtime.set_input(0, false);
        assert_eq!(runtime.input_strength(1), 13);
        assert_eq!(runtime.watched_strengths[0], 13);
        assert_eq!(runtime.blocks[1], wire, "watch checks do not write physical wire state");
        assert_eq!(runtime.deliveries, 1, "15 to 13 is still a raw strength change");
    }
    #[test]
    fn watched_wire_notification_is_separate_from_lamp_power() {
        let program = Program::new(
            (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
            vec![
                Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) },
                Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } },
                Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) },
                Block::RedstoneLamp { lit: true },
            ],
            vec![0],
            vec![[vec![Operation::Contribution(0), Operation::CheckWatchedWire(0)], vec![Operation::Contribution(0), Operation::CheckWatchedWire(0)]]],
            vec![
                ElectricalLink { source: 0, target: 1, attenuation: 0 },
                ElectricalLink { source: 2, target: 3, attenuation: 0 },
            ],
            vec![WatchedWire { wire: 1, watchers: vec![3] }],
        ).unwrap();
        let mut runtime = program.start();
        runtime.set_input(0, false);
        assert_eq!(runtime.input_strength(3), 15);
        assert_eq!(runtime.watched_strengths[0], 0);
        assert_eq!(runtime.blocks[1], Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } });
        assert_eq!(runtime.blocks[3], Block::RedstoneLamp { lit: true });
        assert_eq!(runtime.deliveries, 1);
    }
    #[test]
    fn watcher_updates_are_suppressed_per_phase() {
        let program = Program::new(
            (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
            vec![
                Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) },
                Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } },
                Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } },
                Block::RedstoneLamp { lit: false },
            ],
            vec![0],
            vec![[
                vec![Operation::Contribution(0), Operation::CheckWatchedWire(0), Operation::Contribution(1), Operation::CheckWatchedWire(1)],
                vec![Operation::Contribution(0), Operation::CheckWatchedWire(0), Operation::Contribution(1), Operation::CheckWatchedWire(1)],
            ]],
            vec![
                ElectricalLink { source: 0, target: 1, attenuation: 0 },
                ElectricalLink { source: 0, target: 2, attenuation: 0 },
            ],
            vec![WatchedWire { wire: 1, watchers: vec![3] }, WatchedWire { wire: 2, watchers: vec![3] }],
        ).unwrap();
        let mut runtime = program.start();
        runtime.set_input(0, false);
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
        let program = Program::new(
            (0..3).map(|x| BlockPos::new(x, 15, 15)).collect(),
            vec![
                Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) },
                Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } },
                Block::RedstoneLamp { lit: false },
            ],
            vec![0],
            vec![[operations_off, operations_on]],
            vec![
                ElectricalLink { source: 0, target: 1, attenuation: 0 },
                ElectricalLink { source: 0, target: 1, attenuation: 2 },
                ElectricalLink { source: 0, target: 2, attenuation: 0 },
            ],
            vec![WatchedWire { wire: 1, watchers: vec![2, 2] }],
        ).unwrap();
        let mut runtime = program.start();
        runtime.set_input(0, false);
        assert_eq!(runtime.watched_strengths[0], 0);
        assert_eq!(runtime.deliveries, 3, "two watcher updates plus the independent explicit delivery");
        let phase = runtime.phase;
        for _ in 0..4 {
            runtime.tick();
        }
        assert_eq!(runtime.phase, phase, "scheduler ticks do not define update phases");
        assert!(runtime.pending.iter_entries().next().is_none());
        runtime.set_input(0, true);
        assert_eq!(runtime.watched_strengths[0], 15);
        assert_eq!(runtime.deliveries, 4, "the next external transition starts a fresh update phase");
    }
    #[test]
    fn program_construction_rejects_malformed_records() {
        let positions = vec![BlockPos::new(0, 0, 0), BlockPos::new(1, 0, 0), BlockPos::new(2, 0, 0)];
        let initial = vec![
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
            Block::RedstoneWire { wire: RedstoneWire::default() },
            Block::RedstoneLamp { lit: false },
        ];
        let links = vec![
            ElectricalLink { source: 0, target: 1, attenuation: 0 },
            ElectricalLink { source: 1, target: 2, attenuation: 0 },
        ];
        let ops = vec![Operation::Contribution(0), Operation::RefreshWire(1), Operation::UpdateLamp(2)];
        let transitions = vec![[ops.clone(), ops.clone()]];
        let make = |positions, initial, sources, transitions, links| Program::new(positions, initial, sources, transitions, links, vec![]);
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
            make(positions.clone(), vec![initial[0], Block::Stone {}, initial[2]], vec![0], transitions.clone(), links.clone()),
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
            make(positions.clone(), initial.clone(), vec![0], transitions.clone(), vec![ElectricalLink { source: 3, target: 1, attenuation: 0 }]),
            "sampled program link source is out of bounds",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], transitions.clone(), vec![ElectricalLink { source: 0, target: 3, attenuation: 0 }]),
            "sampled program link target is out of bounds",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], transitions.clone(), vec![ElectricalLink { source: 2, target: 1, attenuation: 0 }]),
            "sampled program link source must be lever or wire",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], transitions.clone(), vec![ElectricalLink { source: 0, target: 0, attenuation: 0 }]),
            "sampled program link target must be wire or lamp",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], transitions.clone(), vec![ElectricalLink { source: 0, target: 1, attenuation: 16 }]),
            "sampled program attenuation exceeds 15",
        );
        let mut overpowered_wire = initial.clone();
        overpowered_wire[1] = Block::RedstoneWire { wire: RedstoneWire { power: 16, ..Default::default() } };
        assert_reason(
            make(positions.clone(), overpowered_wire, vec![0], transitions.clone(), links.clone()),
            "sampled program wire power exceeds 15",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], vec![[vec![Operation::Contribution(2)], vec![]]], links.clone()),
            "sampled program contribution link is out of bounds",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], vec![[vec![Operation::RefreshWire(2)], vec![]]], links.clone()),
            "sampled program RefreshWire target is not a wire",
        );
        assert_reason(
            make(positions.clone(), initial.clone(), vec![0], vec![[vec![Operation::UpdateLamp(1)], vec![]]], links.clone()),
            "sampled program UpdateLamp target is not a lamp",
        );
        assert_reason(
            Program::new(
                vec![BlockPos::new(0, 0, 0)],
                vec![Block::RedstoneWire { wire: RedstoneWire::default() }],
                vec![], vec![], vec![], vec![WatchedWire { wire: 1, watchers: vec![] }],
            ),
            "sampled watched-wire index is not a wire",
        );
        assert_reason(
            Program::new(
                vec![BlockPos::new(0, 0, 0)],
                vec![Block::RedstoneLamp { lit: false }],
                vec![], vec![], vec![], vec![WatchedWire { wire: 0, watchers: vec![] }],
            ),
            "sampled watched-wire index is not a wire",
        );
        assert_reason(
            Program::new(
                positions.clone(), initial.clone(), vec![0], vec![[vec![Operation::CheckWatchedWire(0)], vec![]]], links.clone(), vec![],
            ),
            "sampled program watched-wire index is out of bounds",
        );
        assert_reason(
            Program::new(
                positions.clone(), initial.clone(), vec![0], vec![[vec![], vec![]]], links.clone(),
                vec![WatchedWire { wire: 3, watchers: vec![] }],
            ),
            "sampled watched-wire index is not a wire",
        );
        assert_reason(
            Program::new(
                positions.clone(), initial.clone(), vec![0], vec![[vec![], vec![]]], links.clone(),
                vec![WatchedWire { wire: 1, watchers: vec![3] }],
            ),
            "sampled watched-wire receiver is not a lamp",
        );
        assert_reason(
            Program::new(
                positions, initial, vec![0], vec![[vec![], vec![]]], links,
                vec![WatchedWire { wire: 1, watchers: vec![1] }],
            ),
            "sampled watched-wire receiver is not a lamp",
        );
    }
    #[test]
    fn rejects_unrepresented_blocks() {
        let mut w = host();
        let pos = BlockPos::new(15, 15, 15);
        w.set_block(pos, Block::RedstoneBlock);
        let err = match compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)) {
            Err(err) => err,
            Ok(_) => panic!("unsupported block admitted"),
        };
        assert_eq!(err.position, Some(pos));
    }

    #[test]
    fn lamp_input_discovery_keeps_off_sources_and_expands_only_eligible_contacts() {
        let lamp = BlockPos::new(20, 20, 20);
        let lever_pos = lamp.offset(BlockFace::West);
        let mut world = host();
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        world.set_block(
            lever_pos,
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
        );
        assert_eq!(
            discover_lamp_sources(&world, lamp).unwrap().iter().map(|source| (source.source, source.attenuation)).collect::<Vec<_>>(),
            [(lever_pos, 0)],
        );

        let mut dust_world = host();
        let dust = lamp.offset(BlockFace::West);
        let dust_source = dust.offset(BlockFace::West);
        dust_world.set_block(lamp, Block::RedstoneLamp { lit: false });
        dust_world.set_block(dust, Block::RedstoneWire { wire: RedstoneWire::default() });
        dust_world.set_block(
            dust_source,
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
        );
        let connected = discover_lamp_sources(&dust_world, lamp).unwrap();
        assert!(connected.iter().any(|source| source.source == dust_source));
        let lamp_above_dust = dust.offset(BlockFace::Top);
        dust_world.set_block(lamp, Block::Air);
        dust_world.set_block(lamp_above_dust, Block::RedstoneLamp { lit: false });
        assert!(!discover_lamp_sources(&dust_world, lamp_above_dust).unwrap().iter().any(|source| source.source == dust_source));
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
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
        );
        assert_eq!(
            discover_lamp_sources(&world, lamp).unwrap().iter().map(|source| (source.source, source.attenuation)).collect::<Vec<_>>(),
            [(lever_pos, 0)],
        );
        world.set_block(conductor.offset(BlockFace::West), Block::Stone {});
        world.set_block(lever_pos, Block::Air);
        let beyond_second_conductor = conductor.offset(BlockFace::West).offset(BlockFace::Top);
        world.set_block(
            beyond_second_conductor,
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
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
            Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) },
        );
        for pos in [root, bridge] {
            world.set_block(pos, Block::RedstoneWire { wire: RedstoneWire::default() });
        }
        let dust_path = crate::redpiler::scratch::discover_wire_sources(&world, &[root]).unwrap();
        assert_eq!(dust_path.iter().find(|source| source.source == direct).unwrap().attenuation, 1);
        let merged = discover_lamp_sources(&world, lamp).unwrap();
        assert_eq!(merged.iter().filter(|source| source.source == direct).count(), 1);
        assert_eq!(merged.iter().find(|source| source.source == direct).unwrap().attenuation, 0);
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

        for initially_powered in [false, true] {
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
            for x in 15..=17 {
                w.set_block(BlockPos::new(x, 14, 15), Block::Stone {});
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
                assert_eq!(w.get_block(source), Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) });
                assert_eq!(w.get_block(BlockPos::new(16, 15, 15)), Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } });
                assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
                assert!(w.scheduler().iter_entries().next().is_none());
            }

            let before = snapshot(&w);
            let p = compile(&w, bounds, entry(&w)).unwrap();
            let discovered_link = p.links.iter().position(|link| p.positions[link.source] == source
                && matches!(p.initial[link.target], Block::RedstoneWire { .. }) && link.attenuation == 0).unwrap();
            let lamp_link = p.links.iter().position(|link| p.positions[link.source] == source
                && p.positions[link.target] == BlockPos::new(17, 15, 15) && link.attenuation == 0).unwrap();
            assert!(!p.links.iter().any(|link| p.positions[link.source] == BlockPos::new(16, 15, 15)
                && p.positions[link.target] == BlockPos::new(17, 15, 15)), "lamp dependencies must use extracted source identities");
            for operations in &p.transitions[0] {
                assert!(operations.iter().any(|op| matches!(op, Operation::Contribution(id) if *id == discovered_link)));
                assert!(operations.iter().any(|op| matches!(op, Operation::Contribution(id) if *id == lamp_link)));
            }
            assert_eq!(before, snapshot(&w));
            assert_eq!(p.initial, p.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
            let mut r = p.start();
            let compare = |runtime: &Runtime<'_>, world: &PlotWorld| {
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
                r.set_input(0, powered);
                compare(&r, &w);
                if !powered {
                    assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
                    assert_eq!(r.blocks[p.initial.iter().position(|b| matches!(b, Block::RedstoneLamp { .. })).unwrap()], Block::RedstoneLamp { lit: true });
                }
                for tick in 1..=4 {
                    w.tick_interpreted();
                    r.tick();
                    compare(&r, &w);
                    if !powered {
                        assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: tick < 4 });
                        assert_eq!(r.blocks[p.initial.iter().position(|b| matches!(b, Block::RedstoneLamp { .. })).unwrap()], Block::RedstoneLamp { lit: tick < 4 });
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
                .flat_map(|y| (full_snapshot_bounds.0.z..=full_snapshot_bounds.1.z).flat_map(move |z| (full_snapshot_bounds.0.x..=full_snapshot_bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
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
            world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
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
                position.x < bounds.0.x || position.x > bounds.1.x
                    || position.y < bounds.0.y || position.y > bounds.1.y
                    || position.z < bounds.0.z || position.z > bounds.1.z,
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
                piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false },
            },
        );
        world.set_block(
            bases[1],
            Block::Piston {
                piston: RedstonePiston { facing: BlockFacing::South, sticky: false, extended: true },
            },
        );
        world.set_block(
            bases[1].offset(mchprs_blocks::BlockFace::South),
            Block::PistonHead {
                head: RedstonePistonHead { facing: BlockFacing::South, sticky: false, short: false },
            },
        );
        world.set_block(
            bases[2],
            Block::Piston {
                piston: RedstonePiston { facing: BlockFacing::North, sticky: true, extended: false },
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
                PistonActor { id: 0, base: bases[0], facing: BlockFacing::East, sticky: false, home: bases[0].offset(mchprs_blocks::BlockFace::East), far: bases[0].offset(mchprs_blocks::BlockFace::East).offset(mchprs_blocks::BlockFace::East), extended: false, head_present: false, group: 0 },
                PistonActor { id: 1, base: bases[1], facing: BlockFacing::South, sticky: false, home: bases[1].offset(mchprs_blocks::BlockFace::South), far: bases[1].offset(mchprs_blocks::BlockFace::South).offset(mchprs_blocks::BlockFace::South), extended: true, head_present: true, group: 1 },
                PistonActor { id: 2, base: bases[2], facing: BlockFacing::North, sticky: true, home: bases[2].offset(mchprs_blocks::BlockFace::North), far: bases[2].offset(mchprs_blocks::BlockFace::North).offset(mchprs_blocks::BlockFace::North), extended: false, head_present: false, group: 2 },
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
        assert!(compile(&world, bounds, entry(&world)).is_err(), "lamp-only runtime admission must reject pistons");
    }

    #[test]
    fn rejects_invalid_or_unreadable_piston_contacts() {
        let base = BlockPos::new(15, 15, 15);
        let facing = BlockFacing::East;
        let home = base.offset(mchprs_blocks::BlockFace::East);
        let far = home.offset(mchprs_blocks::BlockFace::East);
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
        let mut mismatched = host();
        mismatched.set_block(base, Block::Piston { piston: RedstonePiston { facing, sticky: false, extended: true } });
        mismatched.set_block(home, Block::PistonHead { head: RedstonePistonHead { facing: BlockFacing::West, sticky: false, short: false } });
        let error = discover_piston_actors(&mismatched, bounds, entry(&mismatched)).unwrap_err();
        assert_eq!(error.position, Some(home));
        assert_eq!(error.reason, "piston head does not match base state");

        let mut occupied = host();
        occupied.set_block(base, Block::Piston { piston: RedstonePiston { facing, sticky: false, extended: false } });
        occupied.set_block(far, Block::Stone {});
        let error = discover_piston_actors(&occupied, bounds, entry(&occupied)).unwrap_err();
        assert_eq!(error.position, Some(far));
        assert_eq!(error.reason, "piston contact cell is occupied");

        let mut moving = host();
        moving.set_block(base, Block::MovingPiston { moving: Default::default() });
        let error = discover_piston_actors(&moving, bounds, entry(&moving)).unwrap_err();
        assert_eq!(error.position, Some(base));
        assert_eq!(error.reason, "moving piston state is unsupported");

        let mut entity = host();
        entity.set_block_entity(BlockPos::new(16, 15, 16), BlockEntity::Comparator { output_strength: 0 });
        let error = discover_piston_actors(&entity, bounds, entry(&entity)).unwrap_err();
        assert_eq!(error.position, Some(BlockPos::new(16, 15, 16)));
        assert_eq!(error.reason, "unsupported block entity in piston capture");

        let mut escaped = host();
        let edge = BlockPos::new(22, 15, 15);
        escaped.set_block(edge, Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false } });
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
    fn rejects_pending_and_boundary() {
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
        assert!(compile(&w, (pos, pos), entry).is_err());
        let entry = ScratchEntry {
            scheduled: vec![],
            piston: w.piston_state().clone(),
        };
        assert!(compile(&w, (BlockPos::new(-1, 0, 0), pos), entry).is_err());
    }

