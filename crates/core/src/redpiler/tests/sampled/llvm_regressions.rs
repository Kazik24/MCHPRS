
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
        let mut projection = program.clone().into_runtime_prepared();
        let native = LlvmRuntime::new(program, &[]).unwrap();
        let aligned: Vec<u64> = invalid.chunks(8).map(|chunk| {
            let mut bytes = [0; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            u64::from_ne_bytes(bytes)
        }).collect();
        assert_eq!(unsafe { (native.library.state_size)(
            &encoded.descriptor, aligned.as_ptr().cast(), invalid.len() as u32,
        ) }, 0);
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(projection.project(pos), Some(Block::RedstoneWire { wire }));
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

