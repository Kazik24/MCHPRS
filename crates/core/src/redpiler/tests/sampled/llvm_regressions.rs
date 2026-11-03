    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_projection_preserves_admitted_pending_capacity_after_imported_ticks_expire() {
        let pos = BlockPos::new(15, 15, 15);
        let block = Block::StonePressurePlate { powered: false };
        let mut program = Program::new(vec![pos], vec![block], vec![], vec![[vec![], vec![]]], vec![], vec![], vec![]).unwrap();
        program.prepare_projection();
        let imported = vec![TickEntry { pos, block_type: None, ticks_left: 1, tick_priority: TickPriority::Normal }; 128];
        let mut native = LlvmRuntime::new(program.clone(), &imported).unwrap();
        native.tick().unwrap();
        let mut projection = program.into_runtime();
        native.sync_projection(&mut projection).unwrap();
        assert!(projection.pending_entries().is_empty());
        let capacity = projection.blocks.len() * 2 + imported.len();
        let tick = crate::redpiler::sampled::executor::ScheduledComponentTick { component: 0, block_type: Some(block.registry_id()) };
        for _ in 0..capacity {
            projection.schedule_half_tick(tick, 31, TickPriority::Normal);
            assert!(projection.failure.is_none());
        }
        projection.schedule_half_tick(tick, 31, TickPriority::Normal);
        assert_eq!(projection.pending_entries().len(), capacity);
        assert_eq!(projection.failure.as_ref().unwrap().reason, "runtime scheduled tick capacity exceeded");
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_matches_rust_rescheduling_with_imported_observer_and_repeater_duplicates() {
        use crate::world::World;
        let pos = BlockPos::new(15, 15, 15);
        for block in [Block::from_name("observer").unwrap(), Block::from_name("repeater").unwrap()] {
            let mut world = super::super::tests::host();
            world.set_block(pos, block);
            world.set_block(pos.offset(mchprs_blocks::BlockFace::Bottom), Block::Stone {});
            let imported = vec![TickEntry { pos, block_type: Some(block.registry_id()), ticks_left: 1, tick_priority: TickPriority::Normal }; 2];
            let mut program = super::super::compile_program(&world,
                (pos - BlockPos::new(3, 3, 3), pos + BlockPos::new(3, 3, 3)),
                super::super::ScratchEntry { scheduled: imported.clone(), piston: world.piston_state().clone() }, None).unwrap();
            program.prepare_projection();
            let mut native = LlvmRuntime::new(program.clone(), &imported).unwrap();
            let mut rust = program.clone().into_runtime_prepared();
            rust.set_pending(imported).unwrap();
            let mut projection = program.into_runtime_prepared();
            for tick in 1..=4 {
                rust.tick().unwrap();
                native.tick().unwrap();
                native.sync_projection(&mut projection).unwrap();
                assert_eq!(projection.projected_blocks(), rust.projected_blocks(), "blocks at tick {tick}: {block:?}");
                let pending = rust.pending_entries();
                assert_eq!(projection.pending_entries(), pending, "LLVM pending at tick {tick}: {block:?}");
                assert_eq!(pending.len(), 1, "Rust retains the unconditional reset at tick {tick}: {block:?}");
                assert_eq!(pending[0].ticks_left, if tick % 2 == 1 { 2 } else { 1 });
                if tick == 1 { assert_eq!(rust.project(pos), Some(block)); }
            }
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_wire_sides_round_trip_and_reject_invalid_encoding() {
        assert!(AUX_TABLES > 52, "checkpoint lacks the LLVM wire-side state table");
        use crate::world::World;
        use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
        let mut world = super::super::tests::host();
        let pos = BlockPos::new(15, 15, 15);
        let wire = RedstoneWire::new(
            RedstoneWireSide::Up, RedstoneWireSide::Side, RedstoneWireSide::None, RedstoneWireSide::Up, 7,
        );
        world.set_block(pos, Block::RedstoneWire { wire });
        let program = super::super::compile_program(&world, (pos, pos),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        let encoded = LlvmProgram::encode(&program).unwrap();
        let mut invalid = encoded.blob.clone();
        let directory = encoded.descriptor.source_offset as usize + encoded.descriptor.source_count as usize * 4;
        let sides_offset = read_u32(&invalid, directory + 52 * 8).unwrap() as usize;
        invalid[sides_offset] |= 3;
        assert!(validate_blob(&encoded.descriptor, &invalid).is_err());
        let mut projection_program = program.clone();
        let node = projection_program.component_indices[&pos];
        projection_program.initial[node] = Block::RedstoneWire { wire: RedstoneWire::default() };
        let mut projection = projection_program.into_runtime_prepared();
        let native = LlvmRuntime::new(program, &[]).unwrap();
        let aligned: Vec<u64> = invalid.chunks(8).map(|chunk| {
            let mut bytes = [0; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            u64::from_ne_bytes(bytes)
        }).collect();
        assert_eq!(unsafe { (native.library.state_size)(
            &encoded.descriptor, aligned.as_ptr().cast(), invalid.len() as u32, 0,
        ) }, 0);
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(projection.project(pos), Some(Block::RedstoneWire { wire }));
        for mask in [3u32, 3 << 2, 3 << 4, 3 << 6, 1 << 8] {
            let mut invalid = encoded.blob.clone();
            let sides = read_u32(&invalid, sides_offset).unwrap() | mask;
            invalid[sides_offset..sides_offset + 4].copy_from_slice(&sides.to_le_bytes());
            assert!(validate_blob(&encoded.descriptor, &invalid).is_err());
            let aligned: Vec<u64> = invalid.chunks(8).map(|chunk| {
                let mut bytes = [0; 8];
                bytes[..chunk.len()].copy_from_slice(chunk);
                u64::from_ne_bytes(bytes)
            }).collect();
            assert_eq!(unsafe { (native.library.state_size)(
                &encoded.descriptor, aligned.as_ptr().cast(), invalid.len() as u32, 0,
            ) }, 0);
        }
    }


    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_empty_piston_observers_match_interpreter_fifo() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing,
            blocks::{Lever, LeverFace, RedstoneObserver, RedstonePiston}};
        let mut world = super::super::tests::host();
        let base = BlockPos::new(15, 15, 15);
        let home = base.offset(BlockFace::East);
        let lever = base.offset(BlockFace::West);
        world.set_block(base, Block::Piston { piston: RedstonePiston {
            facing: BlockFacing::East, sticky: true, extended: false,
        } });
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever, Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) });
        let observers = [base.offset(BlockFace::North), home.offset(BlockFace::North)];
        for pos in observers {
            world.set_block(pos, Block::Observer { observer: RedstoneObserver {
                facing: BlockFacing::South, powered: false,
            } });
        }
        let program = super::super::compile_program(&world,
            (BlockPos::new(10, 10, 10), BlockPos::new(22, 22, 22)),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        let mut projection = program.clone().into_runtime_prepared();
        let mut native = LlvmRuntime::new(program, &[]).unwrap();
        for phase in 0..4 {
            crate::interaction::use_redstone(world.get_block(lever), &mut world, lever, None);
            native.use_control(lever).unwrap();
            for tick in 0..8 {
                world.tick_interpreted();
                native.tick().unwrap();
                native.sync_projection(&mut projection).unwrap();
                for pos in observers {
                    assert_eq!(projection.project(pos), Some(world.get_block(pos)), "phase {phase}, tick {tick}");
                }
                assert_eq!(projection.pending_entries(), world.scheduler().iter_entries().collect::<Vec<_>>(),
                    "phase {phase}, tick {tick}");
            }
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_redstonedb_ram_matches_rust_and_interpreter() {
        use crate::world::World;
        use crate::plot::worldedit::{load_schematic, paste_clipboard};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/redstonedb/schematics/7abf58b22e8e91c72ca27f70c1fd10b1-8_bit_Ram.schem");
        let mut clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
        clip.offset_x = 0;
        clip.offset_y = 0;
        clip.offset_z = 0;
        let origin = BlockPos::new(8, 8, 8);
        let end = origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1);
        let mut world = super::super::tests::host();
        paste_clipboard(&mut world, &clip, origin, false);
        let program = super::super::compile_program(&world, (origin, end),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        let levers: Vec<_> = (origin.y..=end.y).flat_map(|y| (origin.z..=end.z)
            .flat_map(move |z| (origin.x..=end.x).map(move |x| BlockPos::new(x, y, z))))
            .filter(|&pos| matches!(world.get_block(pos), Block::Lever { .. })).collect();
        assert!(levers.len() > 3);
        let positions = program.projection_positions.clone();
        let mut rust = program.start();
        let mut projection = program.clone().into_runtime_prepared();
        let mut native = LlvmRuntime::new(program, &[]).unwrap();
        for index in (0..levers.len()).chain((0..levers.len()).rev()) {
            let lever = levers[index];
            crate::interaction::use_redstone(world.get_block(lever), &mut world, lever, None);
            rust.use_control(lever).unwrap();
            native.use_control(lever).unwrap();
            for tick in 1..=20 {
                world.tick_interpreted();
                rust.tick().unwrap();
                native.tick().unwrap();
                native.sync_projection(&mut projection).unwrap();
                for &pos in &positions {
                    assert_eq!(rust.project(pos), Some(world.get_block(pos)), "RAM interpreter at {pos:?}, lever {lever:?}, tick {tick}");
                    assert_eq!(projection.project(pos), rust.project(pos), "RAM LLVM at {pos:?}, lever {lever:?}, tick {tick}");
                }
                assert_eq!(projection.pending_entries(), rust.pending_entries(), "RAM LLVM pending, lever {lever:?}, tick {tick}");
            }
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_redstonedb_short_pulse_traces_match_rust() {
        use crate::plot::worldedit::{load_schematic, paste_clipboard};
        use crate::world::World;
        use rand::{RngExt, SeedableRng, rngs::StdRng};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/redstonedb");
        let definitions: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("definitions.json")).unwrap()).unwrap();
        for (id, expected_ticks) in [
            ("8111b000090d8f0b3e1370448bcb64a3", 831), ("2cdc24e68c78d7f0cdce7a141648385a", 813),
            ("2735cc14b1051cab4197df60265878b2", 834), ("b6a6e870229ace7eb2afd8cb387a514e", 3496),
        ] {
            let definition = definitions["circuits"].as_array().unwrap().iter().find(|d| d["id"] == id).unwrap();
            let mut clip = load_schematic(std::fs::File::open(root.join(definition["schematic"].as_str().unwrap())).unwrap()).unwrap();
            clip.offset_x = 0;
            clip.offset_y = 0;
            clip.offset_z = 0;
            let origin = BlockPos::new(8, 8, 8);
            let end = origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1);
            let mut world = super::super::tests::host();
            paste_clipboard(&mut world, &clip, origin, false);
            let program = super::super::compile_program(&world, (origin, end),
                super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
            let levers: Vec<_> = (origin.y..=end.y).flat_map(|y| (origin.z..=end.z)
                .flat_map(move |z| (origin.x..=end.x).map(move |x| BlockPos::new(x, y, z))))
                .filter(|&pos| matches!(world.get_block(pos), Block::Lever { .. })).collect();
            assert!(levers.len() > 3);
            let seed = id.as_bytes().iter().fold(0u64, |seed, byte| seed.wrapping_mul(31).wrapping_add(u64::from(*byte)));
            let mut rng = StdRng::seed_from_u64(seed);
            let mut time = 0usize;
            let mut controls = Vec::new();
            for (index, wait) in (0..levers.len()).chain((0..levers.len()).rev()).map(|index| (index, 20i32))
                .chain((0..32).map(|_| (rng.random_range(0..levers.len()), rng.random_range(1..=8))))
            {
                controls.push((time, program.component_indices[&levers[index]]));
                time += wait as usize;
            }
            eprintln!("LLVM/Rust full short-pulse trace: {id}, {} actions, {time} ticks", controls.len());
            assert_eq!(time, expected_ticks, "seeded action trace changed for {id}");
            compare_execution(program, &controls, time);
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    #[ignore = "full ANPU Rust/LLVM reset and interpreted continuation comparison"]
    fn llvm_anpu_reset_and_continuation_match_rust() {
        use super::super::cpu_trace::cpus;
        use crate::world::World;
        use crate::redpiler::CompilerOptions;
        let cpu = cpus::CPUS[1];
        let (mut rust_world, mut rust, _) = cpus::compile_cpu(cpus::load_cpu(cpu), cpu);
        let (mut llvm_world, mut llvm, _) = cpus::compile_cpu_with_options(cpus::load_cpu(cpu), cpu,
            CompilerOptions { llvm: true, ..Default::default() });
        let positions = rust.runtime.as_ref().unwrap().program.projection_positions.clone();
        let start = cpu.origin + cpu.start;
        rust.try_on_use_block(start).unwrap();
        llvm.try_on_use_block(start).unwrap();
        for _ in 0..2000 {
            rust.tick().unwrap();
            llvm.tick().unwrap();
        }
        rust.try_reset(&mut rust_world, (cpu.origin, cpu.origin)).unwrap();
        llvm.try_reset(&mut llvm_world, (cpu.origin, cpu.origin)).unwrap();
        let mut rust_chat = Vec::new();
        let mut llvm_chat = Vec::new();
        for tick in 0..=128 {
            for &pos in &positions {
                assert_eq!(llvm_world.get_block(pos), rust_world.get_block(pos), "block at {pos:?} continuation tick {tick}");
                if matches!(rust_world.get_block(pos), Block::RedstoneComparator { .. }) {
                    let a = llvm_world.get_block_entity(pos).map(|entity| bincode::serialize(entity).unwrap());
                    let b = rust_world.get_block_entity(pos).map(|entity| bincode::serialize(entity).unwrap());
                    assert_eq!(a, b, "comparator entity at {pos:?} continuation tick {tick}");
                }
            }
            assert_eq!(llvm_world.scheduler().iter_entries().collect::<Vec<_>>(), rust_world.scheduler().iter_entries().collect::<Vec<_>>(), "pending tick {tick}");
            assert_eq!(serde_json::to_value(llvm_world.piston_state()).unwrap(), serde_json::to_value(rust_world.piston_state()).unwrap(), "piston state tick {tick}");
            cpus::collect_chat(&llvm_world, tick, &mut llvm_chat);
            cpus::collect_chat(&rust_world, tick, &mut rust_chat);
            assert_eq!(llvm_chat, rust_chat, "chat tick {tick}");
            if tick < 128 {
                rust_world.tick_interpreted();
                llvm_world.tick_interpreted();
            }
        }
    }


    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_landing_and_retraction_preserve_wire_and_observer_order() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, blocks::{Lever, LeverFace, RedstoneObserver, RedstonePiston, RedstoneWire, RedstoneWireSide}};
        let mut world = super::super::tests::host();
        let wire_pos = BlockPos::new(15, 15, 15);
        let north = wire_pos.offset(BlockFace::North);
        let south = wire_pos.offset(BlockFace::South);
        let base = wire_pos + BlockPos::new(-2, 1, 0);
        let home = base.offset(BlockFace::East);
        let lever = base.offset(BlockFace::West);
        let observer = wire_pos.offset(BlockFace::East);
        let cap = observer.offset(BlockFace::Top);
        for pos in [wire_pos.offset(BlockFace::Bottom), south.offset(BlockFace::Bottom), north, home, lever.offset(BlockFace::Bottom)] {
            world.set_block(pos, Block::Stone {});
        }
        world.set_block(base, Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: true, extended: false } });
        world.set_block(lever, Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) });
        for pos in [observer, cap] {
            world.set_block(pos, Block::Observer { observer: RedstoneObserver { facing: BlockFacing::West, powered: false } });
        }
        for pos in [wire_pos, south, north.offset(BlockFace::Top)] {
            world.set_block(pos, Block::RedstoneWire { wire: RedstoneWire::default() });
        }
        for pos in [wire_pos, south, north.offset(BlockFace::Top)] {
            let wire = crate::redstone::wire::get_state_for_placement(&world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        let Block::RedstoneWire { wire: initial } = world.get_block(wire_pos) else { unreachable!() };
        for wire in [initial,
            RedstoneWire::new(RedstoneWireSide::Side, RedstoneWireSide::Side, RedstoneWireSide::Side, RedstoneWireSide::Side, initial.power),
            RedstoneWire::new(RedstoneWireSide::None, RedstoneWireSide::None, RedstoneWireSide::None, RedstoneWireSide::None, initial.power),
            RedstoneWire::new(RedstoneWireSide::Up, RedstoneWireSide::Side, RedstoneWireSide::None, RedstoneWireSide::None, initial.power),
        ] {
            world.set_block(wire_pos, Block::RedstoneWire { wire });
            let program = super::super::compile_program(&world, (BlockPos::new(9, 9, 9), BlockPos::new(22, 22, 22)),
                super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
            let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
            compare_execution(program, &[(0, source), (8, source), (16, source), (17, source)], 28);
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_compiled_empty_shape_plan_suppresses_observer_fallback() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing,
            blocks::{Lever, LeverFace, RedstoneObserver, RedstonePiston}};
        let mut world = super::super::tests::host();
        let base = BlockPos::new(15, 15, 15);
        let home = base.offset(BlockFace::East);
        let lever = base.offset(BlockFace::West);
        world.set_block(base, Block::Piston { piston: RedstonePiston {
            facing: BlockFacing::East, sticky: true, extended: false,
        } });
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever, Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, false) });
        world.set_block(home.offset(BlockFace::North), Block::Observer { observer: RedstoneObserver {
            facing: BlockFacing::South, powered: false,
        } });
        let mut program = super::super::compile_program(&world,
            (base - BlockPos::new(4, 4, 4), base + BlockPos::new(4, 4, 4)),
            super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
        program.shape_programs.insert(home, 0..0);
        program.prepare_projection();
        let source = program.component_indices[&lever];
        compare_execution(program, &[(0, source), (4, source)], 10);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_all_pressure_plates_and_trapdoors_match_rust() {
        use crate::world::World;
        use mchprs_blocks::{BlockDirection, BlockFace,
            blocks::{Lever, LeverFace, TrapdoorHalf}};
        for id in 0..mchprs_blocks::generated::STATE_TO_BLOCK_INDEX.len() as u32 {
            let plate = Block::from_id(id);
            if plate.pressure_plate_powered().is_none() { continue; }
            let mut world = super::super::tests::host();
            let base = BlockPos::new(15, 15, 15);
            let lever = base.offset(BlockFace::East);
            world.set_block(base.offset(BlockFace::West), plate);
            world.set_block(base, Block::IronTrapdoor {
                facing: BlockDirection::North, half: TrapdoorHalf::Bottom, powered: false,
            });
            let mut observer = Block::from_name("observer").unwrap();
            observer.set_properties(std::collections::HashMap::from([("facing", "south")]));
            world.set_block(base.offset(BlockFace::North), observer);
            world.set_block(lever, Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            });
            for pos in [base.offset(BlockFace::West), lever] {
                world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            }
            let program = super::super::compile_program(&world,
                (base - BlockPos::new(3, 3, 3), base + BlockPos::new(3, 3, 3)),
                super::super::ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }, None).unwrap();
            let source = program.component_indices[&lever];
            compare_execution(program, &[(0, source), (3, source), (5, source)], 9);
        }
    }

