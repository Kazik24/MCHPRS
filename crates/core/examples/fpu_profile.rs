//! Exact FPU timing: cargo run -p mchprs_core --release --example fpu_profile -- FILE [GAME_TICKS] [FLUSH_EVERY] [on|off]
#[path = "../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_core::plot::worldedit::{load_schematic, paste_clipboard};
use mchprs_core::plot::{PlotWorld, PLOT_WIDTH};
use mchprs_core::redpiler::{Compiler, CompilerOptions};
use mchprs_core::world::storage::Chunk;
use mchprs_core::world::World;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("schematic path required"))?;
    let ticks: u32 = args.get(1).map_or(Ok(32), |value| value.parse())?;
    let flush_every: u32 = args.get(2).map_or(Ok(1), |value| value.parse())?;
    let clipboard = load_schematic(std::fs::File::open(path)?)?;
    println!(
        "fixture {path}; dimensions {}x{}x{}",
        clipboard.size_x, clipboard.size_y, clipboard.size_z
    );
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(
        &mut world,
        &clipboard,
        BlockPos::new(
            40 + clipboard.offset_x,
            30 + clipboard.offset_y,
            40 + clipboard.offset_z,
        ),
        false,
    );
    let mut compiler = Compiler::default();
    let started = Instant::now();
    compiler.compile(
        &world,
        world.get_corners(),
        CompilerOptions {
            optimize: true,
            assume_instant: true,
            ..Default::default()
        },
        vec![],
        Default::default(),
    )?;
    println!("compiled in {:.3}s", started.elapsed().as_secs_f64());
    for line in compiler.stats().unwrap().summary_lines() {
        println!("{line}");
    }
    if let Some(control) = args.get(3) {
        anyhow::ensure!(
            control == "on" || control == "off",
            "control must be on or off"
        );
        let trigger = BlockPos::new(46, 66, 116);
        let Block::Lever { lever } = world.get_block(trigger) else {
            anyhow::bail!("missing FPU control at {trigger:?}");
        };
        let step = Instant::now();
        if lever.powered != (control == "on") {
            compiler.on_use_block(trigger);
        }
        println!(
            "FPU control {control}: {:.6}s",
            step.elapsed().as_secs_f64()
        );
    }
    let started = Instant::now();
    for game_tick in 1..=ticks {
        let step = Instant::now();
        for _ in 0..2 {
            compiler.tick_with_world(&mut world);
        }
        if flush_every != 0 && game_tick % flush_every == 0 {
            compiler.flush(&mut world);
        }
        println!(
            "game tick {game_tick}: {:.6}s",
            step.elapsed().as_secs_f64()
        );
    }
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{ticks} game ticks: {seconds:.6}s, {:.2} TPS; flush_every={flush_every}",
        f64::from(ticks) / seconds
    );
    if args.get(4).is_some_and(|value| value == "checkpoint") {
        let bounds = world.get_corners();
        compiler.reset(&mut world, bounds);
        println!(
            "checkpoint {}",
            serde_json::to_string(&cpus::checkpoint(&world, ticks, &[]))?
        );
    }
    Ok(())
}
