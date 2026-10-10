//! Steady-state interpreted extension/retraction; world construction is excluded.
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_core::plot::{PLOT_WIDTH, PlotWorld};
use mchprs_core::redstone;
use mchprs_core::world::{World, storage::Chunk};
use std::time::Duration;

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

fn install(world: &mut PlotWorld, pos: BlockPos, length: i32) {
    world.set_block(
        pos,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    for x in 1..=length {
        world.set_block(BlockPos::new(pos.x + x, pos.y, pos.z), Block::Stone {});
    }
}

fn cycle(world: &mut PlotWorld, positions: &[BlockPos]) {
    for power in [Block::RedstoneBlock {}, Block::Air] {
        for &pos in positions {
            world.set_block(pos.offset(BlockFace::Bottom), power);
            redstone::update(world.get_block(pos), world, pos, None);
        }
        for _ in 0..4 {
            world.tick_interpreted();
        }
    }
    black_box(world.get_block(positions[0]));
}

fn pistons(c: &mut Criterion) {
    let mut group = c.benchmark_group("piston-cycle");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(20);
    for count in [1, 64, 256, 1024] {
        let mut world = world();
        let positions: Vec<_> = (0..count)
            .map(|i| BlockPos::new(8 + (i % 32) * 6, 30, 8 + (i / 32) * 6))
            .collect();
        for &pos in &positions {
            install(&mut world, pos, 1);
        }
        group.bench_with_input(BenchmarkId::new("independent", count), &count, |b, _| {
            b.iter(|| cycle(&mut world, &positions));
        });
    }
    // Long straight chains exercise this fork's deliberately unrestricted movement.
    for length in [8, 32, 128] {
        let mut world = world();
        let positions = [BlockPos::new(8, 30, 8)];
        install(&mut world, positions[0], length);
        group.bench_with_input(BenchmarkId::new("chain", length), &length, |b, _| {
            b.iter(|| {
                cycle(&mut world, &positions);
                // Retraction pulls only the nearest payload. Restore the contiguous
                // homogeneous chain for the next extension (two storage writes).
                world.set_block(BlockPos::new(10, 30, 8), Block::Stone {});
                world.set_block(BlockPos::new(9 + length, 30, 8), Block::Air);
            });
        });
    }
    group.finish();
}
criterion_group!(benches, pistons);
criterion_main!(benches);
