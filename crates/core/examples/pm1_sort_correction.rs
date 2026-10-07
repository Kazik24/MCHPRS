//! Opt-in correction validation: cargo run -p mchprs_core --release --locked --example pm1_sort_correction
#[path = "../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use cpus::*;
use mchprs_blocks::{blocks::Block, BlockPos};
use mchprs_core::{
    plot::PlotWorld,
    redpiler::{Compiler, CompilerOptions},
    world::World,
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const REMOVED: [BlockPos; 2] = [BlockPos::new(206, 42, 40), BlockPos::new(206, 43, 40)];
const FIXED: Cpu = Cpu {
    schematic: "PM1_SORT_FIXED.schem",
    sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
    ..CPUS[0]
};

fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none()
        && world.piston_state().events.is_empty()
        && world.piston_state().motions.is_empty()
}

// Hash the corrected world both as saved and with ONLY the removed entries
// restored. Mutate storage directly to avoid queuing visual or redstone work.
fn checked_checkpoint(
    world: &mut PlotWorld,
    tick: u32,
    trace: &[(u32, String)],
    expected: &Checkpoint,
    removed_ids: &[u32; 2],
) -> Value {
    let actual = checkpoint(world, tick, trace);
    for (pos, id) in REMOVED.into_iter().zip(removed_ids) {
        assert_eq!(world.get_block(pos), Block::Air);
        assert!(world.get_block_entity(pos).is_none());
        world
            .get_chunk_mut(pos.x >> 4, pos.z >> 4)
            .unwrap()
            .set_block((pos.x & 15) as u32, pos.y as u32, (pos.z & 15) as u32, *id);
    }
    let normalized = checkpoint(world, tick, trace);
    for pos in REMOVED {
        world
            .get_chunk_mut(pos.x >> 4, pos.z >> 4)
            .unwrap()
            .set_block((pos.x & 15) as u32, pos.y as u32, (pos.z & 15) as u32, 0);
    }
    assert_eq!(
        &normalized, expected,
        "only the two removed entries may differ at {tick}"
    );
    json!({"tick":tick,"original_blocks":expected.blocks,"corrected_blocks":actual.blocks,
        "restored_blocks":normalized.blocks,"all_other_fields_equal":true})
}

fn replay_fixed(expected: &Reference, sample: usize) -> Value {
    let mut original = (sample == 1).then(|| load_cpu(CPUS[0]));
    let mut world = load_cpu(FIXED);
    let initial = load_cpu(CPUS[0]);
    let removed_ids = REMOVED.map(|pos| {
        assert!(initial.get_block_entity(pos).is_none());
        assert!(!initial.scheduler().iter_entries().any(|t| t.pos == pos));
        assert!(!initial.piston_state().motions.iter().any(|m| m.pos == pos));
        initial.get_block_raw(pos)
    });
    assert!(
        quiet(&initial),
        "fixture has no initial scheduled work, events or motions"
    );
    drop(initial);
    let mut trace = Vec::new();
    let mut original_trace = Vec::new();
    let mut hashes = vec![checked_checkpoint(
        &mut world,
        0,
        &trace,
        &expected.checkpoints[0],
        &removed_ids,
    )];
    if let Some(original) = &mut original {
        assert_eq!(checkpoint(original, 0, &[]), expected.checkpoints[0]);
        click_cpu(original, CPUS[0], CPUS[0].start);
    }
    click_cpu(&mut world, FIXED, FIXED.start);
    let mut active = Duration::ZERO;
    let mut first_quiet = None;
    let mut index = 1;
    for tick in 1..=50_100 {
        if tick == 50_001 {
            click_cpu(&mut world, FIXED, FIXED.stop.unwrap());
            if let Some(original) = &mut original {
                click_cpu(original, CPUS[0], CPUS[0].stop.unwrap());
            }
        }
        let now = Instant::now();
        world.tick_interpreted();
        let elapsed = now.elapsed();
        if tick <= FIXED.active_ticks {
            active += elapsed;
        }
        collect_chat(&world, tick, &mut trace);
        let idle = quiet(&world);
        if idle && first_quiet.is_none() {
            first_quiet = Some(tick);
        }
        if let Some(original) = &mut original {
            original.tick_interpreted();
            collect_chat(original, tick, &mut original_trace);
            assert_eq!(idle, quiet(original), "halt/work state differs at {tick}");
            assert_eq!(
                trace, original_trace,
                "ordered tick-stamped output differs at {tick}"
            );
            for (pos, id) in REMOVED.into_iter().zip(removed_ids) {
                assert_eq!(
                    original.get_block_raw(pos),
                    id,
                    "removed geometry changed at {tick}"
                );
                assert!(original.get_block_entity(pos).is_none());
            }
        }
        if CHECKPOINTS.contains(&tick) || tick == 50_100 {
            if let Some(original) = &original {
                assert_eq!(
                    checkpoint(original, tick, &original_trace),
                    expected.checkpoints[index]
                );
            }
            hashes.push(checked_checkpoint(
                &mut world,
                tick,
                &trace,
                &expected.checkpoints[index],
                &removed_ids,
            ));
            index += 1;
            println!("interpreter sample {sample}: checkpoint {tick} passed");
        }
    }
    assert_eq!(trace, expected.chat_trace);
    assert_eq!(trace.len(), 1535);
    assert!(trace
        .iter()
        .any(|(t, s)| *t == 12_051 && s.contains("shut")));
    assert!(quiet(&world));
    println!(
        "interpreter sample {sample}: {:.6}s, {:.2} active TPS",
        active.as_secs_f64(),
        f64::from(FIXED.active_ticks) / active.as_secs_f64()
    );
    json!({"sample":sample,"active_seconds":active.as_secs_f64(),
        "active_tps":f64::from(FIXED.active_ticks)/active.as_secs_f64(),
        "first_quiet_tick":first_quiet,"final_quiet":true,"messages":trace.len(),
        "halt_message_tick":12051,"stop_message":trace.last(),"geometry_hashes":hashes,
        "original_lockstep_comparison":sample == 1})
}

fn save(report: &Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/cpu-references/pm1-sort-correction-results.json");
    std::fs::write(path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
}

fn main() {
    if std::env::args().any(|arg| arg == "--probe-fixed") {
        let world = load_cpu(FIXED);
        let before = checkpoint(&world, 0, &[]);
        for budget in [1, 8] {
            for flags in ["", "-O", "--assume-instant", "-O --assume-instant"] {
                let mut compiler = Compiler::default();
                let mut options = CompilerOptions::parse(flags).unwrap();
                options.budget_multiplier = budget;
                let now = Instant::now();
                let result = compiler.compile(
                    &world,
                    world.get_corners(),
                    options,
                    world.scheduler().iter_entries().collect(),
                    Default::default(),
                );
                println!(
                    "corrected, budget {budget}, flags {flags:?}, {:.6}s: {}",
                    now.elapsed().as_secs_f64(),
                    result
                        .as_ref()
                        .map_or_else(|error| error.to_string(), |_| "admitted".into())
                );
                assert_eq!(checkpoint(&world, 0, &[]), before);
                assert_eq!(compiler.is_active(), result.is_ok());
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--probe-original") {
        let mut world = load_cpu(CPUS[0]);
        let before = checkpoint(&world, 0, &[]);
        let mut compiler = Compiler::default();
        let now = Instant::now();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    budget_multiplier: 8,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap_err();
        println!(
            "original, maximum budget, {:.6}s: {error}",
            now.elapsed().as_secs_f64()
        );
        assert_eq!(
            error.to_string(),
            "moving piston at BlockPos { x: 206, y: 42, z: 40 } requires the interpreter"
        );
        assert!(!compiler.is_active());
        assert_eq!(checkpoint(&world, 0, &[]), before);
        // Demonstrate why deleting just the first placeholder is insufficient.
        world.set_block(REMOVED[0], Block::Air);
        let now = Instant::now();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    budget_multiplier: 8,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap_err();
        println!(
            "only first entry removed, maximum budget, {:.6}s: {error}",
            now.elapsed().as_secs_f64()
        );
        assert_eq!(
            error.to_string(),
            "moving piston at BlockPos { x: 206, y: 43, z: 40 } requires the interpreter"
        );
        return;
    }
    let expected = reference(CPUS[0]);
    let mut report = json!({"schema":1,"profile":"release","os":std::env::consts::OS,
        "arch":std::env::consts::ARCH,"original_sha256":CPUS[0].sha256,"corrected_sha256":FIXED.sha256,
        "active_window_ticks":FIXED.active_ticks,"protocol_ticks":50100,
        "interpreter_flush_timed":false,"interpreter_flush_interval":0,
        "timing_excludes":"loading, preparation, compilation, checks, manual stop tail",
        "admission":[],"interpreter_samples":[]});
    let world = load_cpu(FIXED);
    let before = checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    for budget in [1, 8] {
        for flags in ["", "-O", "--assume-instant", "-O --assume-instant"] {
            let mut compiler = Compiler::default();
            let mut options = CompilerOptions::parse(flags).unwrap();
            options.budget_multiplier = budget;
            let now = Instant::now();
            let result = compiler.compile(
                &world,
                world.get_corners(),
                options,
                ticks.clone(),
                Default::default(),
            );
            let seconds = now.elapsed().as_secs_f64();
            assert_eq!(
                checkpoint(&world, 0, &[]),
                before,
                "compilation mutated input"
            );
            let error = result.expect_err(
                "PM1 admission changed; validate compiled behavior before benchmarking",
            );
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            println!("budget {budget}, flags {flags:?}, {seconds:.6}s: {error}");
            let row = json!({"budget_multiplier":budget,"flags":flags,"compile_seconds":seconds,
                "input_unchanged":true,"admitted":false,"error":error.to_string()});
            report["admission"].as_array_mut().unwrap().push(row);
            save(&report);
        }
    }
    drop(world);
    for sample in 1..=3 {
        let row = replay_fixed(&expected, sample);
        report["interpreter_samples"]
            .as_array_mut()
            .unwrap()
            .push(row);
        save(&report);
    }
}
