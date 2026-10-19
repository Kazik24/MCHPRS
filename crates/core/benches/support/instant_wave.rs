//! A production-build A/B measurement of one complete certified computation wave.
use super::cpu_support;
use anyhow::{Result, ensure};
use mchprs_blocks::{
    BlockDirection, BlockFace, BlockFacing, BlockPos,
    blocks::{Block, Lever, LeverFace, RedstoneObserver, RedstonePiston, RedstonePistonHead, RedstoneWire, RedstoneWireSide},
};
use mchprs_core::{
    plot::PlotWorld,
    redpiler::{Compiler, CompilerOptions},
    world::{World, storage::Chunk},
};
use serde_json::json;
use std::time::Instant;

const CONTROL: BlockPos = BlockPos::new(7, 20, 8);
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(2, 17, 2), BlockPos::new(15, 24, 15));

fn world() -> PlotWorld {
    let chunks = (0..2).flat_map(|x| (0..2).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    for (position, facing) in [
        (BlockPos::new(8, 20, 8), BlockFacing::East),
        (BlockPos::new(13, 20, 8), BlockFacing::South),
    ] {
        let face: BlockFace = facing.into();
        world.set_block(
            position,
            Block::Piston {
                piston: RedstonePiston {
                    facing,
                    sticky: true,
                    extended: true,
                },
            },
        );
        world.set_block(
            position.offset(face),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(position.offset(face).offset(face), Block::RedstoneBlock);
        world.set_block(
            position.offset(BlockFace::Top),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        );
        world.set_block(position.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    }
    for (x, power) in [(11, 15), (12, 14)] {
        let position = BlockPos::new(x, 20, 8);
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    power,
                ),
            },
        );
    }
    world.set_block(CONTROL.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        CONTROL,
        Block::Lever {
            lever: Lever {
                face: LeverFace::Floor,
                facing: BlockDirection::North,
                powered: true,
            },
        },
    );
    world
}

pub(super) fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let iterations = args
        .windows(2)
        .find(|pair| pair[0] == "--iterations")
        .map_or(Ok(64), |pair| pair[1].parse::<usize>())?;
    ensure!(iterations > 0, "iterations must be positive");
    let mut expected = None;
    let mut samples = Vec::new();
    let mut times = [Vec::new(), Vec::new()];
    for pair in 0..iterations + 4 {
        for tier in if pair % 2 == 0 { [3, 4] } else { [4, 3] } {
            let mut world = world();
            let mut compiler = Compiler::default();
            compiler.compile(
                &world,
                BOUNDS,
                CompilerOptions {
                    optimization_level: Some(tier),
                    stable_instant_inputs: true,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )?;
            let coverage = compiler.graph_execution_coverage().unwrap();
            compiler.on_use_block(CONTROL);
            let started = Instant::now();
            compiler.tick()?;
            let seconds = started.elapsed().as_secs_f64();
            let work = compiler.graph_execution_counts().unwrap();
            ensure!(
                work.instant_kernel_invocations == u64::from(tier == 4),
                "unexpected execution owner at O{tier}"
            );
            ensure!(tier != 4 || work.wire_walks == 0, "covered computation still ran Turbo");
            compiler.reset(&mut world, BOUNDS);
            let observed = cpu_support::checkpoint(&world, 1, &[]);
            if let Some(expected) = &expected {
                ensure!(&observed == expected, "O{tier} state/future differs from O3 control");
            } else {
                ensure!(tier == 3, "O3 must establish the first control checkpoint");
                expected = Some(observed);
            }
            if pair >= 4 {
                times[tier - 3].push(seconds);
                samples.push(
                    json!({"pair":pair-4,"tier":tier,"tick_seconds":seconds,"kernel_invocations":work.instant_kernel_invocations,
                    "callbacks":work.callbacks,"wire_walks":work.wire_walks,"piston_samples":work.piston_samples,"piston_actions":work.piston_actions,
                    "kernel_nodes":coverage.instant_kernel_nodes}),
                );
            }
        }
    }
    for times in &mut times {
        times.sort_by(f64::total_cmp);
    }
    let median = |times: &[f64]| {
        let middle = times.len() / 2;
        if times.len() % 2 == 0 {
            (times[middle - 1] + times[middle]) / 2.0
        } else {
            times[middle]
        }
    };
    let baseline = median(&times[0]);
    let kernel = median(&times[1]);
    let report = json!({"workload":"private-two-cell-wave","profile":if cfg!(debug_assertions) {"debug"} else {"release"},
        "timing":"one tick only; four warmup pairs; alternating O3/O4; fresh world/compiler; compilation, stimulus, reset and O3 checkpoint validation excluded",
        "scope":"computation wave only; recurring reset is not timed","baseline_median_seconds":baseline,"kernel_median_seconds":kernel,
        "speedup":baseline/kernel,"samples":samples});
    let path = super::root().join("target/instant-kernel-wave.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "O3 {baseline:.9}s -> O4 kernel {kernel:.9}s; {:.3}x; O3 state/future matched all pairs; report {}",
        baseline / kernel,
        path.display()
    );
    Ok(())
}
