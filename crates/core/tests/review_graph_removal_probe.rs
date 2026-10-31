use mchprs_blocks::{
    BlockDirection, BlockFace, BlockFacing, BlockPos,
    blocks::{Block, Lever, LeverFace, RedstonePiston},
};
use mchprs_core::{
    plot::{PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld},
    redpiler::{Compiler, CompilerOptions, TaskMonitor, TickScheduler},
    world::{World, storage::Chunk},
};
use mchprs_world::{TickEntry, TickPriority};
use std::sync::Arc;

fn world(x: i32, z: i32) -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|cx| (0..PLOT_WIDTH).map(move |cz| Chunk::empty(x * PLOT_WIDTH + cx, z * PLOT_WIDTH + cz)))
        .collect();
    PlotWorld::from_chunks(x, z, chunks, TickScheduler::default())
}
fn compile(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Compiler {
    let mut c = Compiler::default();
    c.compile(
        world,
        bounds,
        CompilerOptions::default(),
        world.scheduler().iter_entries().collect(),
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    c
}

#[test]
fn review_non_origin_plot_accepts_supported_circuit() {
    let mut w = world(1, -1);
    let pos = BlockPos::new(PLOT_BLOCK_WIDTH + 20, 20, -PLOT_BLOCK_WIDTH + 20);
    w.set_block(pos, Block::RedstoneLamp { lit: false });
    compile(&w, (pos - BlockPos::new(3, 3, 3), pos + BlockPos::new(3, 3, 3)));
}

#[test]
fn review_imported_stale_tick_is_ignored_like_native() {
    let pos = BlockPos::new(20, 20, 20);
    let bounds = (BlockPos::new(15, 15, 15), BlockPos::new(25, 25, 25));
    let mut native = world(0, 0);
    let mut sampled = world(0, 0);
    for w in [&mut native, &mut sampled] {
        w.set_block(pos, Block::RedstoneLamp { lit: true });
        w.restore_scheduled_tick(TickEntry {
            pos,
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
            block_type: Some(Block::Stone {}.registry_id()),
        });
    }
    let mut c = compile(&sampled, bounds);
    native.tick_interpreted();
    c.tick().unwrap();
    c.try_flush(&mut sampled).unwrap();
    assert_eq!(sampled.get_block(pos), native.get_block(pos));
}

#[test]
fn review_reset_motion_at_each_age_matches_continued_sampled() {
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let head = base.offset(BlockFace::East);
    let payload = head.offset(BlockFace::East);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    for age in [1, 2] {
        let mut a = world(0, 0);
        let mut b = world(0, 0);
        for w in [&mut a, &mut b] {
            w.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
            w.set_block(
                lever,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            w.set_block(
                base,
                Block::Piston {
                    piston: RedstonePiston {
                        facing: BlockFacing::East,
                        sticky: true,
                        extended: false,
                    },
                },
            );
            w.set_block(head, Block::RedstoneBlock {});
        }
        let mut ca = compile(&a, bounds);
        let mut cb = compile(&b, bounds);
        ca.try_on_use_block(lever).unwrap();
        cb.try_on_use_block(lever).unwrap();
        for _ in 0..age {
            ca.tick().unwrap();
            cb.tick().unwrap();
        }
        ca.try_reset(&mut a, bounds).unwrap();
        for step in 1..=3 {
            a.tick_interpreted();
            cb.tick().unwrap();
            cb.try_flush(&mut b).unwrap();
            for pos in [base, head, payload] {
                assert_eq!(a.get_block(pos), b.get_block(pos), "age={age}, step={step}, pos={pos:?}");
            }
        }
    }
}

#[test]
fn review_exported_extension_keeps_payload_before_head_order() {
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let head = base.offset(BlockFace::East);
    let payload = head.offset(BlockFace::East);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let mut native = world(0, 0);
    let mut sampled = world(0, 0);
    for w in [&mut native, &mut sampled] {
        w.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        w.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        w.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: false,
                },
            },
        );
        w.set_block(head, Block::RedstoneBlock {});
    }
    let mut c = compile(&sampled, bounds);
    c.try_on_use_block(lever).unwrap();
    c.tick().unwrap();
    c.try_reset(&mut sampled, bounds).unwrap();
    native.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    mchprs_core::redstone::update_surrounding_blocks(&mut native, lever);
    mchprs_core::redstone::update_surrounding_blocks(&mut native, lever.offset(BlockFace::Bottom));
    native.tick_interpreted();
    let order = |w: &PlotWorld| w.piston_state().motions.iter().map(|m| m.pos).collect::<Vec<_>>();
    assert_eq!(order(&native), vec![payload, head]);
    assert_eq!(order(&sampled), order(&native));
}

#[test]
fn review_projection_cost() {
    for n in [1000, 10000] {
        let mut w = world(0, 0);
        for i in 0..n {
            w.set_block(BlockPos::new(10 + i % 100, 20, 10 + i / 100), Block::Stone {});
        }
        let mut c = compile(&w, (BlockPos::new(5, 15, 5), BlockPos::new(115, 25, 115)));
        let start = std::time::Instant::now();
        for _ in 0..10 {
            c.try_flush(&mut w).unwrap();
        }
        eprintln!("projection n={n}: {:?} per flush", start.elapsed() / 10);
    }
}

#[test]
fn review_headless_untyped_tick_matches_native() {
    use mchprs_core::headless::{Session, Snapshot};
    let pos=BlockPos::new(20,20,20);
    let snapshot=|| Snapshot {
        min:BlockPos::new(15,15,15),max:BlockPos::new(25,25,25),
        blocks:vec![(pos,Block::RedstoneLamp {lit:true})],entities:vec![],
        ticks:vec![TickEntry {pos,ticks_left:1,tick_priority:TickPriority::Normal,block_type:None}],
    };
    let mut native=Session::new(snapshot()).unwrap();
    let mut sampled=Session::new(snapshot()).unwrap();
    sampled.compile(CompilerOptions::default()).unwrap();
    native.advance(1).unwrap();sampled.advance(1).unwrap();
    assert_eq!(sampled.get(pos).unwrap(),native.get(pos).unwrap());
}
