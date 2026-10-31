use super::cpu_support::*;
use mchprs_core::world::World;
use std::time::{Duration, Instant};

pub struct RunTiming {
    pub total: Duration,
    pub active: Duration,
    pub active_ticks: u32,
    pub compile: Option<Duration>,
    pub visual_counts: (u64, u64, u64),
    pub instant_piston_cache_stats: Option<(usize, usize, u64)>,
}

pub fn replay_with_visuals(cpu: Cpu, expected: &Reference, screen_only: bool, flush_every: u32, redpiler: bool) -> RunTiming {
    let mut world = load_cpu(cpu);
    if flush_every != 0 {
        world.flush_block_changes();
    }
    let (world, compiler, compile) = if redpiler {
        let (world, compiler, elapsed) = compile_cpu(world, cpu);
        (world, Some(compiler), Some(elapsed))
    } else {
        (world, None, None)
    };
    let mut world = world;
    let mut compiler = compiler;
    let mut elapsed = Duration::ZERO;
    let active_ticks = cpu.active_ticks;
    let mut active = Duration::ZERO;
    let visual_counts = replay(&mut world, cpu, expected, screen_only, flush_every, redpiler, |world, step| match step {
        ReplayStep::Start => {
            if let Some(compiler) = &mut compiler {
                compiler.try_on_use_block(cpu.start + cpu.origin).unwrap();
            } else {
                click_cpu(world, cpu, cpu.start);
            }
        }
        ReplayStep::Tick(tick) => {
            let now = Instant::now();
            if let Some(compiler) = &mut compiler {
                compiler.tick().unwrap();
                compiler.try_flush(world).unwrap();
            } else {
                world.tick_interpreted();
            }
            let tick_elapsed = now.elapsed();
            elapsed += tick_elapsed;
            if tick <= active_ticks {
                active += tick_elapsed;
            }
        }
        ReplayStep::StopTick => {
            if let Some(compiler) = &mut compiler {
                compiler.tick().unwrap();
                compiler.try_flush(world).unwrap();
            } else {
                world.tick_interpreted();
            }
        }
        ReplayStep::Stop => {
            let stop = cpu.stop.unwrap();
            if let Some(compiler) = &mut compiler {
                compiler.try_on_use_block(stop + cpu.origin).unwrap();
            } else {
                click_cpu(world, cpu, stop);
            }
        }
    });
    RunTiming {
        total: elapsed,
        active,
        active_ticks,
        compile,
        instant_piston_cache_stats: world.instant_piston_cache().map(|cache| cache.stats()),
        visual_counts,
    }
}
