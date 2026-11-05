#![recursion_limit = "256"]
#[path = "../test_support/cpus.rs"]
mod cpu_support;

use cpu_support::*;
use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::worldedit::load_schematic,
    redpilator::{Compiler, CompilerOptions, TaskMonitor},
    world::World,
};
use std::{
    fs::File,
    sync::Arc,
    time::{Duration, Instant},
};

fn main() {
    let cpu = CPUS[1];
    let expected = reference(cpu);
    let args: Vec<_> = std::env::args().skip(1).collect();
    let runs = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--runs=").map(str::to_owned))
        .map_or(3, |value| value.parse::<usize>().expect("--runs=N must be a positive integer"));
    let game_ticks = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--ticks=").map(str::to_owned))
        .map_or(512, |value| value.parse::<u32>().expect("--ticks=N must be a positive integer"));
    assert!(runs > 0);
    assert!(game_ticks > 0);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let mut reports = Vec::new();

    for run in 1..=runs {
        let mut native = load_cpu(cpu);
        let compiler_world = load_cpu(cpu);
        let ticks = compiler_world.scheduler().iter_entries().collect();
        let timeout = Duration::from_secs(
            std::env::var("MCHPRS_REDPILATOR_COMPILE_TIMEOUT_SECS")
                .ok()
                .and_then(|seconds| seconds.parse().ok())
                .unwrap_or(300),
        );
        let monitor = Arc::new(TaskMonitor::default());
        let worker_monitor = monitor.clone();
        let compile_started = Instant::now();
        let (mut compiler, result, mut compiler_world, timed_out) = std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let mut compiler = Compiler::default();
                let result = compiler.compile(&compiler_world, bounds, CompilerOptions::default(), ticks, worker_monitor);
                (compiler, result, compiler_world)
            });
            let mut timed_out = false;
            while !worker.is_finished() {
                if compile_started.elapsed() >= timeout && !monitor.cancelled() {
                    timed_out = true;
                    monitor.cancel();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let (compiler, result, compiler_world) = worker.join().unwrap();
            (compiler, result, compiler_world, timed_out)
        });
        let compile = compile_started.elapsed();
        if result.is_ok() {
            eprintln!("run {run}: compile {:.3}s", compile.as_secs_f64());
            let mut native_chat = Vec::new();
            let mut native_screens = Vec::new();
            let mut compiled_screens = Vec::new();
            let mut native_checkpoints = Vec::new();
            collect_screen(&native, cpu, 0, &mut native_screens);
            native_checkpoints.push(checkpoint(&native, 0, &native_chat));
            click_cpu(&mut native, cpu, cpu.start);
            compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();

            let mut native_prefix = Duration::ZERO;
            let mut compiled_tick_prefix = Duration::ZERO;
            let mut compiled_publication_prefix = Duration::ZERO;
            let mut first_divergence = None;
            for tick in 1..=game_ticks {
                let started = Instant::now();
                native.tick_interpreted();
                let elapsed = started.elapsed();
                native_prefix += elapsed;
                collect_chat(&native, tick, &mut native_chat);
                if CHECKPOINTS.contains(&tick) {
                    let sample = checkpoint(&native, tick, &native_chat);
                    let index = native_checkpoints.len();
                    assert_eq!(sample, expected.checkpoints[index], "native reference mismatch at tick {tick}");
                    native_checkpoints.push(sample);
                }
                collect_screen(&native, cpu, tick, &mut native_screens);

                let started = Instant::now();
                compiler.tick().unwrap();
                let elapsed = started.elapsed();
                compiled_tick_prefix += elapsed;
                let started = Instant::now();
                compiler.try_flush(&mut compiler_world).unwrap();
                let elapsed = started.elapsed();
                compiled_publication_prefix += elapsed;
                collect_screen(&compiler_world, cpu, tick, &mut compiled_screens);
                let expected_screen = screen(&native, cpu);
                let actual_screen = screen(&compiler_world, cpu);
                if expected_screen != actual_screen && first_divergence.is_none() {
                    let pixel = expected_screen.bytes().zip(actual_screen.bytes()).position(|(a, b)| a != b);
                    first_divergence = Some(serde_json::json!({"tick":tick,"first_pixel":pixel,"native":expected_screen,"compiled":actual_screen}));
                }
            }
            let reference_screen: Vec<ScreenFrame> = serde_json::from_slice(
                &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/cpu-references/anpu_screen.json")).unwrap(),
            )
            .unwrap();
            let expected_screen_trace = reference_screen
                .into_iter()
                .take_while(|frame| frame.tick <= game_ticks)
                .collect::<Vec<_>>();
            assert_eq!(native_screens, expected_screen_trace, "native screen reference mismatch");
            assert!(
                native_screens.len() >= 2 && compiled_screens.len() >= 2,
                "prefix must contain at least two distinct screen frames"
            );
            assert_eq!(
                native_checkpoints.len(),
                expected.checkpoints.iter().filter(|sample| sample.tick <= game_ticks).count()
            );
            let screen_output_parity = first_divergence.is_none();
            reports.push(serde_json::json!({
                "run":run,"compile_seconds":compile.as_secs_f64(),"native_prefix_ticks":game_ticks,"native_tick_seconds":native_prefix.as_secs_f64(),
                "compiled_tick_only_seconds":compiled_tick_prefix.as_secs_f64(),"compiled_publication_seconds":compiled_publication_prefix.as_secs_f64(),
                "compiled_tick_plus_full_flush_seconds":(compiled_tick_prefix+compiled_publication_prefix).as_secs_f64(),
                "native_prefix_tps":f64::from(game_ticks)/native_prefix.as_secs_f64(),
                "compiled_tick_only_tps":f64::from(game_ticks)/compiled_tick_prefix.as_secs_f64(),
                "compiled_tick_plus_full_flush_tps":f64::from(game_ticks)/(compiled_tick_prefix+compiled_publication_prefix).as_secs_f64(),
                "distinct_native_screen_frames":native_screens.len(),"distinct_compiled_screen_frames":compiled_screens.len(),
                "screen_output_parity":screen_output_parity,"first_screen_divergence":first_divergence,
                "native_reference":"passed: frozen checkpoints and screen transition trace through prefix"
            }));
        } else if let Err(error) = result {
            let (native_seconds, screen_frames) = native_prefix(cpu, game_ticks, &expected);
            let rejected_at = match &error {
                mchprs_core::redpilator::CompileError::Unsupported(error) => error.position.unwrap_or_else(BlockPos::zero),
                _ => BlockPos::zero(),
            };
            reports.push(serde_json::json!({
                "run":run,"compile_seconds":compile.as_secs_f64(),"compile_timeout_seconds":timeout.as_secs(),
                "compile_timed_out":timed_out,"compile_rejected":error.to_string(),
                "rejection_block":format!("{:?}", compiler_world.get_block(rejected_at)),
                "rejection_block_entity":format!("{:?}", compiler_world.get_block_entity(rejected_at)),
                "native_prefix_ticks":game_ticks,"native_tick_seconds":native_seconds.as_secs_f64(),
                "native_prefix_tps":f64::from(game_ticks)/native_seconds.as_secs_f64(),
                "distinct_native_screen_frames":screen_frames,"compiled_tps":"N/A: compile rejected"
            }));
        }
    }
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/redpilator-bench-results/anpu.json");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(
        &output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"schema":1,"cpu":cpu.name,"fixture_sha256":cpu.sha256,"ticks_per_run":game_ticks,"runs":reports}),
        )
        .unwrap(),
    )
    .unwrap();
    println!("wrote {}", output.display());
}

fn native_prefix(cpu: Cpu, game_ticks: u32, expected: &Reference) -> (Duration, usize) {
    let mut world = load_cpu(cpu);
    let mut chat = Vec::new();
    let mut screens = Vec::new();
    let mut checkpoint_count = 1;
    collect_screen(&world, cpu, 0, &mut screens);
    assert_eq!(checkpoint(&world, 0, &chat), expected.checkpoints[0]);
    click_cpu(&mut world, cpu, cpu.start);
    let mut elapsed = Duration::ZERO;
    for tick in 1..=game_ticks {
        let started = Instant::now();
        world.tick_interpreted();
        elapsed += started.elapsed();
        collect_chat(&world, tick, &mut chat);
        if CHECKPOINTS.contains(&tick) {
            assert_eq!(
                checkpoint(&world, tick, &chat),
                expected.checkpoints[checkpoint_count],
                "native reference mismatch at tick {tick}"
            );
            checkpoint_count += 1;
        }
        collect_screen(&world, cpu, tick, &mut screens);
    }
    let reference_screen: Vec<ScreenFrame> = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/cpu-references/anpu_screen.json")).unwrap(),
    )
    .unwrap();
    let expected_screen_trace = reference_screen
        .into_iter()
        .take_while(|frame| frame.tick <= game_ticks)
        .collect::<Vec<_>>();
    assert_eq!(screens, expected_screen_trace, "native screen reference mismatch");
    assert!(screens.len() >= 2, "prefix must contain at least two distinct native screen frames");
    (elapsed, screens.len())
}
