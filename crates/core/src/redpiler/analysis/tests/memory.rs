use super::*;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneWire, RedstoneWireSide};
use mchprs_blocks::BlockDirection;

// The second cell's QC data reads the first cell's far payload through a
// separate dust line three blocks above its base. Neither data line delivers
// a qualifying notification. One lower dust staircase samples both cells.
fn wire_bank() -> (PlotWorld, [BlockPos; 2], BlockPos, BlockPos) {
    let mut world = empty();
    let cells = [BASE, BASE + BlockPos::new(8, -5, 0)];
    for base in cells {
        world.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::Down,
                    sticky: true,
                    extended: true,
                },
            },
        );
        world.set_block(
            base.offset(BlockFace::Bottom),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::Down,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(base + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
    }
    let wire = |west| Block::RedstoneWire {
        wire: RedstoneWire {
            north: RedstoneWireSide::None,
            south: RedstoneWireSide::None,
            east: RedstoneWireSide::Side,
            west,
            power: 0,
        },
    };
    let data_wire = BASE + BlockPos::new(0, 3, 0);
    world.set_block(data_wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(data_wire, wire(RedstoneWireSide::Side));
    let data = data_wire.offset(BlockFace::East);
    world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        data,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for x in 1..=8 {
        let pos = BASE + BlockPos::new(x, -2, 0);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, wire(RedstoneWireSide::Side));
    }
    for x in 0..=8 {
        let pos = BASE + BlockPos::new(x, -(x - 3).max(0), 2);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            wire(if x > 3 {
                RedstoneWireSide::Up
            } else {
                RedstoneWireSide::Side
            }),
        );
    }
    let sample = BASE + BlockPos::new(-1, 0, 2);
    world.set_block(sample.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        sample,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    (world, cells, data, sample)
}

fn bank(compiler: &Compiler, cells: [BlockPos; 2]) -> (u64, [bool; 2]) {
    let stats = compiler.backend.as_ref().unwrap().logical_stats();
    let (_, samples, memory) = stats
        .iter()
        .find(|(_, _, memory)| memory.iter().any(|(pos, _)| *pos == cells[0]))
        .expect("explicit generic memory bank");
    assert_eq!(memory.len(), 2);
    (
        *samples,
        cells.map(|cell| memory.iter().find(|(pos, _)| *pos == cell).unwrap().1),
    )
}

#[test]
fn independent_wire_delivery_samples_old_bank_atomically_without_collapsing_source_actions() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, cells, data, sample) = wire_bank();
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                bank(&compiler, cells),
                (0, [false, false]),
                "activation must not sample"
            );
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(bank(&compiler, cells), (0, [false, false]));
            // Changing and restoring only data leaves both stored cells untouched.
            compiler.on_use_block(data);
            for _ in 0..8 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(bank(&compiler, cells), (0, [false, false]));
            compiler.on_use_block(data);
            for _ in 0..8 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(bank(&compiler, cells), (0, [false, false]));
            // Cell A now wants one; B still sees A's old far payload (zero).
            compiler.on_use_block(sample);
            assert_eq!(
                bank(&compiler, cells),
                (1, [true, false]),
                "read all affected cells before committing any cell"
            );
            // A second delivered edge in the same half tick is a new transaction.
            compiler.on_use_block(sample);
            assert_eq!(
                bank(&compiler, cells),
                (2, [true, true]),
                "distinct source actions must not be coalesced"
            );
            compiler.on_use_block(sample);
            assert_eq!(
                bank(&compiler, cells),
                (3, [true, true]),
                "unchanged stored values still count the sampling delivery"
            );
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(bank(&compiler, cells), (3, [true, true]));
            compiler.reset(&mut world, bounds);
            for base in cells {
                assert!(
                    matches!(world.get_block(base), Block::Piston { piston } if !piston.extended)
                );
                assert_eq!(
                    world.get_block(base.offset(BlockFace::Bottom)),
                    Block::RedstoneBlock
                );
                assert_eq!(world.get_block(base + BlockPos::new(0, -2, 0)), Block::Air);
            }
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
            assert!(!world
                .scheduler()
                .iter_entries()
                .any(|entry| cells.contains(&entry.pos)));
        }
    }
}

#[test]
fn different_writers_delivered_in_one_half_tick_both_commit_their_banks() {
    let shift = BlockPos::new(64, 0, 48);
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, first_cells, _, first_sample) = wire_bank();
            let (second, second_cells, _, second_sample) = wire_bank();
            let second_cells = second_cells.map(|pos| pos + shift);
            let second_sample = second_sample + shift;
            let bounds = second.get_corners();
            crate::world::for_each_block_optimized(&second, bounds.0, bounds.1, |pos| {
                let block = second.get_block(pos);
                if block != Block::Air {
                    world.set_block(pos + shift, block);
                }
            });
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            assert_eq!(bank(&compiler, first_cells), (0, [false, false]));
            assert_eq!(bank(&compiler, second_cells), (0, [false, false]));
            compiler.on_use_block(first_sample);
            assert_eq!(bank(&compiler, first_cells), (1, [true, false]));
            assert_eq!(bank(&compiler, second_cells), (0, [false, false]));
            compiler.on_use_block(second_sample);
            assert_eq!(bank(&compiler, first_cells), (1, [true, false]));
            assert_eq!(bank(&compiler, second_cells), (1, [true, false]));
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}

fn generator_cell(head_only: bool) -> (PlotWorld, BlockPos, BlockPos, BlockPos, BlockPos) {
    let mut world = empty();
    let cell = BASE;
    world.set_block(
        cell,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::Down,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        cell.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::Down,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(cell + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
    let data_wire = cell + BlockPos::new(0, 3, 0);
    world.set_block(data_wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        data_wire,
        Block::RedstoneWire {
            wire: RedstoneWire {
                north: RedstoneWireSide::None,
                south: RedstoneWireSide::None,
                east: RedstoneWireSide::Side,
                west: RedstoneWireSide::Side,
                power: 0,
            },
        },
    );
    let data = data_wire.offset(BlockFace::East);
    world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        data,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    // Direct-base delivery always reaches the cell. The lower layout only
    // reaches its old head. Controls remain separate from the QC data route.
    let generator = cell
        + if head_only {
            BlockPos::new(2, -1, 0)
        } else {
            BlockPos::new(2, 0, 0)
        };
    world.set_block(
        generator,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::West,
                sticky: false,
                extended: false,
            },
        },
    );
    let sample = generator.offset(BlockFace::South);
    world.set_block(sample.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        sample,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    (world, cell, data, generator, sample)
}

#[test]
fn empty_generator_pose_edges_sample_independent_data_without_periodic_writes() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, cell, data, generator, sample) = generator_cell(false);
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            let stored = |compiler: &Compiler| {
                let stats = compiler.backend.as_ref().unwrap().logical_stats();
                let (_, count, memory) = stats
                    .iter()
                    .find(|(_, _, memory)| memory.iter().any(|(pos, _)| *pos == cell))
                    .unwrap();
                assert_eq!(memory.len(), 1);
                (*count, memory[0].1)
            };
            assert_eq!(stored(&compiler), (0, false));
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(stored(&compiler), (0, false));
            compiler.on_use_block(sample);
            assert_eq!(stored(&compiler), (1, true));
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(
                stored(&compiler),
                (1, true),
                "held generator pose is not another event"
            );
            compiler.on_use_block(data);
            for _ in 0..16 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(stored(&compiler), (1, true), "data changes never sample");
            compiler.on_use_block(sample);
            assert_eq!(stored(&compiler), (2, false));
            compiler.reset(&mut world, bounds);
            assert!(matches!(world.get_block(cell), Block::Piston { piston } if piston.extended));
            assert_eq!(
                world.get_block(cell + BlockPos::new(0, -2, 0)),
                Block::RedstoneBlock
            );
            assert!(
                matches!(world.get_block(generator), Block::Piston { piston } if !piston.extended)
            );
            assert_eq!(
                world.get_block(generator.offset(BlockFace::West)),
                Block::Air
            );
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}

#[test]
fn generator_delivery_to_storage_head_uses_old_extended_eligibility() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, cell, data, _, sample) = generator_cell(true);
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            let stored = |compiler: &Compiler| {
                let stats = compiler.backend.as_ref().unwrap().logical_stats();
                let (_, count, memory) = stats
                    .iter()
                    .find(|(_, _, memory)| memory.iter().any(|(pos, _)| *pos == cell))
                    .unwrap();
                (
                    *count,
                    memory.iter().find(|(pos, _)| *pos == cell).unwrap().1,
                )
            };
            assert_eq!(stored(&compiler), (0, false));
            compiler.on_use_block(sample);
            assert_eq!(
                stored(&compiler),
                (1, true),
                "the old extended head forwards its delivered recheck"
            );
            compiler.on_use_block(data);
            for _ in 0..16 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(stored(&compiler), (1, true));
            compiler.on_use_block(sample);
            assert_eq!(
                stored(&compiler),
                (1, true),
                "a retained near block does not forward a head-only notification"
            );
            compiler.on_use_block(sample);
            assert_eq!(stored(&compiler), (1, true));
            compiler.reset(&mut world, bounds);
            assert_eq!(
                world.get_block(cell.offset(BlockFace::Bottom)),
                Block::RedstoneBlock
            );
            assert_eq!(world.get_block(cell + BlockPos::new(0, -2, 0)), Block::Air);
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}
