//! Counter/divider throughput and raw changing-input full-FPU workloads.
use anyhow::{bail, ensure, Context, Result};
use mchprs_blocks::{
    blocks::{Block, LeverFace},
    BlockFace, BlockPos,
};
use mchprs_core::{
    plot::{
        worldedit::{load_schematic, paste_clipboard},
        PlotWorld, PLOT_WIDTH,
    },
    redpiler::{Compiler, CompilerOptions},
    redstone,
    world::{storage::Chunk, World},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut component = String::from("counter_basic");
    let mut iterations = 3usize;
    let mut episodes = 32usize;
    let mut ticks = 60_000u32;
    let mut flush_every = 0u32;
    let mut optimize = false;
    let mut interpreted = false;
    let mut changing_inputs = false;
    let mut seed = 0x4d43_4850_5253_2026u64;
    let mut input_every = 1usize;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => continue,
            "--optimize" => {
                optimize = true;
                continue;
            }
            "--interpreted" => {
                interpreted = true;
                continue;
            }
            "--changing-inputs" => {
                changing_inputs = true;
                continue;
            }
            "--help" => {
                println!("instant [--component counter_basic|cpu_bubblesort|fpu_divider|fpu_legal] [--changing-inputs] [--seed integer] [--input-every ticks] [--interpreted | --optimize] [--iterations 3] [--episodes 32] [--ticks 60000] [--flush-every 0|1] [--output path.json]");
                return Ok(());
            }
            _ => {}
        }
        let value = args
            .next()
            .with_context(|| format!("{arg} requires a value"))?;
        match arg.as_str() {
            "--component" => component = value,
            "--iterations" => iterations = value.parse()?,
            "--episodes" => episodes = value.parse()?,
            "--ticks" => ticks = value.parse()?,
            "--flush-every" => flush_every = value.parse()?,
            "--output" => output = Some(value),
            "--seed" => seed = value.parse()?,
            "--input-every" => input_every = value.parse()?,
            _ => bail!("unknown argument {arg}"),
        }
    }
    ensure!(iterations > 0, "--iterations must be positive");
    ensure!(episodes > 0, "--episodes must be positive");
    ensure!(seed != 0, "--seed must be nonzero");
    ensure!(input_every > 0, "--input-every must be positive");
    ensure!(
        ticks >= 50_000 && ticks % 6 == 0,
        "--ticks must be at least 50000 and divisible by six"
    );
    ensure!(flush_every <= 1, "--flush-every must be zero or one");
    ensure!(
        !interpreted || !optimize,
        "--optimize applies only to compiled execution"
    );
    ensure!(
        !interpreted || matches!(component.as_str(), "counter_basic" | "fpu_divider"),
        "--interpreted compares Counter/divider runtime"
    );
    ensure!(
        !changing_inputs || matches!(component.as_str(), "fpu_legal" | "fpu_divider"),
        "--changing-inputs applies to fpu_legal or fpu_divider"
    );
    ensure!(
        !changing_inputs || !interpreted,
        "--changing-inputs compares compiled plans against an unoptimized compiled reference"
    );
    let manifest_path = match component.as_str() {
        "counter_basic" => "test_data/instant-pistons-io/fixtures/counter_basic.json",
        "cpu_bubblesort" => "test_data/piston-research/fixtures/cpu_bubblesort.json",
        "fpu_divider" => "test_data/piston-research/fixtures/fpu_divider.json",
        "fpu_legal" => "test_data/piston-research/fixtures/fpu_legal.json",
        _ => bail!("unknown component {component}"),
    };
    let mut descriptor: Value =
        serde_json::from_slice(&std::fs::read(root().join(manifest_path))?)?;
    if component == "cpu_bubblesort" {
        // Same closed-context origin as the compiled CPU harness.
        descriptor["origin"] = json!([2, 8, 2]);
    } else if component == "fpu_legal" && descriptor.get("origin").is_none() {
        descriptor["origin"] = json!([40, 30, 40]);
    }
    // Arithmetic is unspecified. Validate the selected plan's trace
    // against an independent plain compiled run before measuring whole phases.
    let mut reference_traces: Option<Vec<Vec<u16>>> = None;
    let mut validation = None;
    if changing_inputs {
        for checked_optimize in [false, optimize] {
            let mut world = load(&descriptor)?;
            let probes = fpu_probes(&mut world, &descriptor)?;
            let mut compiler = Some(Compiler::default());
            compiler
                .as_mut()
                .unwrap()
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant: true,
                        optimize: checked_optimize,
                        budget_multiplier: 8,
                        ..Default::default()
                    },
                    Vec::new(),
                    Default::default(),
                )
                .context("changing-input trace validation compilation failed")?;
            let (report, traces) = fpu_changing_inputs(
                &mut compiler,
                &mut world,
                &descriptor,
                &probes,
                episodes,
                seed,
                input_every,
                flush_every,
                true,
                reference_traces.as_deref(),
            )?;
            if reference_traces.is_none() {
                reference_traces = Some(traces);
            } else {
                validation = Some(report);
            }
        }
    }
    let mut samples = Vec::new();
    let backend = if interpreted {
        "interpreted"
    } else {
        "compiled"
    };
    let tick_operation = if interpreted {
        "tick_interpreted"
    } else {
        "tick_with_world"
    };
    for index in 1..=iterations {
        println!("{backend} {component} sample {index}/{iterations}");
        let mut world = load(&descriptor)?;
        let probes = if changing_inputs {
            fpu_probes(&mut world, &descriptor)?
        } else {
            Vec::new()
        };
        let mut compiler = (!interpreted).then(Compiler::default);
        let started = Instant::now();
        let admission = compiler.as_mut().map_or(Ok(()), |compiler| {
            compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant: true,
                    optimize,
                    budget_multiplier: 8,
                    ..Default::default()
                },
                Vec::new(),
                Default::default(),
            )
        });
        let compile_seconds = if interpreted {
            0.0
        } else {
            started.elapsed().as_secs_f64()
        };
        let mut sample = match admission {
            Err(error) => json!({"admitted": false, "error": error.to_string()}),
            Ok(())
                if component == "cpu_bubblesort"
                    || (component == "fpu_legal" && !changing_inputs) =>
            {
                json!({"admitted": true, "measurement_kind": "admission_only"})
            }
            Ok(()) => {
                let result = if changing_inputs {
                    fpu_changing_inputs(
                        &mut compiler,
                        &mut world,
                        &descriptor,
                        &probes,
                        episodes,
                        seed,
                        input_every,
                        flush_every,
                        false,
                        reference_traces.as_deref(),
                    )
                    .map(|(report, _)| report)
                } else if component == "counter_basic" {
                    counter(&mut compiler, &mut world, &descriptor, ticks, flush_every)
                } else {
                    divider(
                        &mut compiler,
                        &mut world,
                        &descriptor,
                        episodes,
                        flush_every,
                    )
                };
                let mut result = result.unwrap_or_else(|error| json!({"error": error.to_string()}));
                if !interpreted {
                    result["admitted"] = json!(true);
                }
                result
            }
        };
        sample["compile_seconds_metadata"] = json!(compile_seconds);
        sample["compile_statistics"] = json!(compiler.as_ref().and_then(Compiler::stats).map(|stats| {
            let regions = &stats.regions;
            json!({
                "backend_nodes": stats.backend_nodes,
                "graph": {
                    "baseline": stats.graph.baseline().map(|counts| json!({"nodes": counts.nodes, "links": counts.links})),
                    "final": stats.graph.final_graph().map(|counts| json!({"nodes": counts.nodes, "links": counts.links})),
                    "wire_nodes_elided_before_baseline": stats.graph.wire_nodes_elided,
                },
                "regions": {
                    "logical": regions.logical_regions, "clocked": regions.clocked_regions,
                    "pistons": regions.pistons, "payload_groups": regions.payload_groups,
                    "memory_cells": regions.memory_cells,
                },
                "program": {
                    "arena_decisions_including_handoff": regions.decisions,
                    "response_roots": regions.response_roots,
                    "output_ports": regions.output_ports, "output_terms": regions.output_terms,
                    "executable_response_decisions": regions.logical_response_decisions,
                    "executable_output_and_sampling_decisions": regions.logical_output_decisions,
                    "input_bindings": regions.logical_input_bindings,
                },
            })
        }));
        sample["warnings"] = json!(compiler.as_ref().map_or(&[][..], Compiler::warnings));
        println!("{}", serde_json::to_string(&sample)?);
        samples.push(sample);
    }
    let mut elapsed: Vec<_> = samples
        .iter()
        .filter(|sample| sample["activity_confirmed"] == true)
        .filter_map(|sample| sample["seconds"].as_f64())
        .collect();
    elapsed.sort_by(f64::total_cmp);
    let median = elapsed.get(elapsed.len() / 2).copied();
    let changing_game_ticks = episodes as u64
        * if component == "fpu_legal" {
            input_every as u64
        } else {
            256
        };
    let report = json!({
        "schema": 1, "backend": backend, "component": component, "manifest": manifest_path, "schematic_sha256": descriptor["sha256"],
        "actual_origin": [origin(&descriptor).x, origin(&descriptor).y, origin(&descriptor).z],
        "flags": if interpreted { None } else { Some(if optimize { "-O --assume-instant" } else { "--assume-instant" }) }, "budget_multiplier": (!interpreted).then_some(8),
        "admitted_means": (!interpreted).then_some("compiler acceptance only; no physical or arithmetic equivalence claim"),
        "scope": if interpreted { "interpreter Counter/divider with identical saved inputs, warmup, stimulus and observation windows" } else { "actual logical workloads; BubbleSort and full FPU without --changing-inputs are admission only; no runtime fallback" },
        "measurement_kind": if changing_inputs { if component == "fpu_legal" { "full_fpu_raw_changing_inputs" } else { "divider_raw_changing_inputs" } } else { match (component.as_str(), interpreted) { ("counter_basic", true) => "interpreted_counter_runtime", ("counter_basic", false) => "logical_counter_runtime", ("fpu_divider", true) => "interpreted_divider_episodes", ("fpu_divider", false) => "logical_divider_episodes", _ => "admission_only" } },
        "timing": if changing_inputs {
            String::from("whole phase timers include lever stimulus, tick_with_world and flush only with --flush-every 1; phase observations, compilation, warmup and independent validation excluded")
        } else if component == "fpu_divider" {
            format!("accumulated stimulus and per-tick timers include lever use, {tick_operation} and optional compiled flush; initialization, one warm episode and output observations excluded")
        } else {
            format!("per-tick {tick_operation}; optional compiled flush included; preparation and observations excluded")
        },
        "interpreter_flush_policy": interpreted.then_some("no-op: interpreter block state is already published"),
        "counter_protocol": "exact IO fixture; 24 inactive ticks, lever OFF->ON, 600 active warmup ticks, then fixed active window",
        "game_ticks": if changing_inputs { Some(changing_game_ticks) } else if component == "counter_basic" { Some(u64::from(ticks)) } else { None }, "flush_every_game_ticks": flush_every,
        "divider_protocol": (component == "fpu_divider" && !changing_inputs).then_some("saved A/B held; 64 ON initialization ticks; one untimed OFF128/ON128 warm episode; repeated held OFF128/ON128 complete episodes preserving ordinary clock timing; observe response170 during OFF and verify final reset0 after ON"),
        "episodes": (component == "fpu_divider" || changing_inputs).then_some(episodes),
        "changing_inputs": changing_inputs,
        "seed": changing_inputs.then_some(seed),
        "input_every_game_ticks": (changing_inputs && component == "fpu_legal").then_some(input_every),
        "changing_input_protocol": changing_inputs.then_some(if component == "fpu_legal" {
            "continuous OFF random raw-port stream: trigger set OFF once before warmup and held OFF; 1024 initial ticks and 32 untimed random vectors; each measured vector delivers ordered changed-lever updates then input-every native game ticks; no arithmetic or readiness oracle"
        } else {
            "fixed-window random raw-port benchmark: 64 initial ON ticks; eight untimed warm episodes; ordered lever updates set seeded random operand port masks while ON, then hold all inputs ON64, OFF128, ON64; no arithmetic or readiness oracle"
        }),
        "trace_validation": validation.as_ref().map(|report| json!({
            "reference": "independent unoptimized --assume-instant compiler with identical output ports/adapters and workload",
            "selected_plan_every_game_tick_matches": true,
            "output_varied": report["activity_confirmed"],
            "output_trace_sha256": report["output_trace_sha256"],
            "first_output_change_game_ticks": report["first_output_change_game_ticks"],
            "latency_scope": if component == "fpu_legal" { "untimed replay; first visible output-word change after vector update relative to previous interval boundary; no valid-result latency claim" } else { "untimed replay; first visible output-word change after OFF relative to end of ON preparation; no valid-result latency claim" },
        })),
        "median_seconds": median,
        "median_tps": median.filter(|_| component == "counter_basic" || changing_inputs).map(|seconds| if changing_inputs { changing_game_ticks as f64 / seconds } else { f64::from(ticks) / seconds }),
        "median_ns_per_tick": median.filter(|_| component == "counter_basic" || changing_inputs).map(|seconds| seconds * 1e9 / if changing_inputs { changing_game_ticks as f64 } else { f64::from(ticks) }),
        "median_episodes_per_second": median.filter(|_| component == "fpu_divider" || changing_inputs).map(|seconds| episodes as f64 / seconds),
        "samples": samples,
    });
    if let Some(path) = output {
        let path = root().join(path);
        std::fs::create_dir_all(path.parent().context("output path has no parent")?)?;
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}

fn counter(
    compiler: &mut Option<Compiler>,
    world: &mut PlotWorld,
    descriptor: &Value,
    ticks: u32,
    flush_every: u32,
) -> Result<Value> {
    let origin = origin(descriptor);
    let trigger = origin + position(&descriptor["ports"]["inputs"]["trigger"]);
    let outputs: Vec<_> = descriptor["ports"]["observations"]["repeater"]
        .as_array()
        .context("missing Counter repeater bank")?
        .iter()
        .map(|pos| origin + position(pos))
        .collect();
    ensure!(
        outputs.len() == 16,
        "Counter must have sixteen output repeaters"
    );
    ensure!(
        matches!(world.get_block(trigger), Block::Lever { lever } if !lever.powered),
        "saved Counter release lever must be OFF"
    );
    let read = |world: &PlotWorld| -> Result<u16> {
        outputs
            .iter()
            .enumerate()
            .try_fold(0u16, |word, (bit, &pos)| {
                let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                    bail!("missing output repeater at {pos:?}");
                };
                Ok(word | (u16::from(!repeater.powered) << bit))
            })
    };
    read(world)?;
    for _ in 0..24 {
        tick(compiler, world);
    }
    use_lever(compiler, world, trigger);
    for _ in 0..600 {
        tick(compiler, world);
    }
    let mut elapsed = Duration::ZERO;
    let mut progression = Vec::new();
    for game_tick in 1..=ticks {
        let started = Instant::now();
        tick(compiler, world);
        if flush_every == 1 {
            flush(compiler, world);
        }
        elapsed += started.elapsed();
        if game_tick % 600 == 5 || game_tick == ticks - 1 {
            if flush_every == 0 {
                flush(compiler, world);
            }
            progression.push((600 + game_tick, read(world)?));
        }
    }
    let changing = progression.windows(2).all(|pair| pair[0].1 != pair[1].1);
    Ok(
        json!({"measurement_kind": if compiler.is_some() { "logical_counter_runtime" } else { "interpreted_counter_runtime" }, "activity_confirmed": changing,
        "seconds": elapsed.as_secs_f64(), "word_progression": progression}),
    )
}

fn divider(
    compiler: &mut Option<Compiler>,
    world: &mut PlotWorld,
    descriptor: &Value,
    episodes: usize,
    flush_every: u32,
) -> Result<Value> {
    let origin = origin(descriptor);
    let trigger = origin + position(&descriptor["protocol"]["trigger"]);
    let outputs: Vec<_> = descriptor["observations"]["output_msb_first"]
        .as_array()
        .context("missing divider output bank")?
        .iter()
        .map(|pos| origin + position(pos))
        .collect();
    ensure!(
        outputs.len() == 10,
        "divider must have ten output repeaters"
    );
    ensure!(
        matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered),
        "saved divider enable lever must be ON"
    );
    let read = |world: &PlotWorld| -> Result<u16> {
        outputs.iter().try_fold(0u16, |word, &pos| {
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                bail!("missing divider output repeater at {pos:?}");
            };
            Ok((word << 1) | u16::from(!repeater.powered))
        })
    };
    read(world)?;
    for _ in 0..64 {
        tick(compiler, world);
    }
    flush(compiler, world);
    let (_, warm_response, _) = divider_phase(compiler, world, trigger, flush_every, &read)?;
    let (_, _, warm_reset) = divider_phase(compiler, world, trigger, flush_every, &read)?;
    ensure!(
        warm_response && warm_reset == 0,
        "divider warm episode expected response 170 during OFF and final reset 0 after ON; observed response 170: {warm_response}, final reset: {warm_reset}"
    );
    let mut elapsed = Duration::ZERO;
    let mut words = Vec::with_capacity(episodes);
    for episode in 1..=episodes {
        let (duration, response, _) = divider_phase(compiler, world, trigger, flush_every, &read)?;
        elapsed += duration;
        let (duration, _, reset) = divider_phase(compiler, world, trigger, flush_every, &read)?;
        elapsed += duration;
        ensure!(
            response && reset == 0,
            "divider episode {episode} expected response 170 during OFF and final reset 0 after ON; observed response 170: {response}, final reset: {reset}"
        );
        words.push((episode, response, reset));
    }
    let seconds = elapsed.as_secs_f64();
    Ok(json!({
        "measurement_kind": if compiler.is_some() { "logical_divider_episodes" } else { "interpreted_divider_episodes" }, "activity_confirmed": true,
        "seconds": seconds, "episodes": episodes, "episodes_per_second": episodes as f64 / seconds,
        "game_ticks_per_episode": 256, "warmup_game_ticks": 320,
        "response_170_observed_each_off_window": true, "final_reset_0_verified": true,
        "verified_episodes": episodes, "episode_observations": words,
        "flush_policy": if compiler.is_none() { "no-op: interpreter block state is already published" } else if flush_every == 0 { "every game tick for observation, excluded" } else { "every game tick, included" },
        "timed_operations": if compiler.is_some() { "on_use_block for both enable edges, 256 tick_with_world calls per episode, optional per-tick flush; timer overhead included" } else { "lever toggle and surrounding/support callbacks for both enable edges, 256 tick_interpreted calls per episode; timer overhead included" },
        "arithmetic_validity": "saved-input response/reset protocol only; no general arithmetic proof"
    }))
}

fn divider_phase(
    compiler: &mut Option<Compiler>,
    world: &mut PlotWorld,
    trigger: BlockPos,
    flush_every: u32,
    read: &impl Fn(&PlotWorld) -> Result<u16>,
) -> Result<(Duration, bool, u16)> {
    let started = Instant::now();
    use_lever(compiler, world, trigger);
    let mut elapsed = started.elapsed();
    let mut observed_response = false;
    let mut word = 0;
    for _ in 0..128 {
        let started = Instant::now();
        tick(compiler, world);
        if flush_every == 1 {
            flush(compiler, world);
        }
        elapsed += started.elapsed();
        if flush_every == 0 {
            flush(compiler, world);
        }
        word = read(world)?;
        observed_response |= word == 170;
    }
    Ok((elapsed, observed_response, word))
}

fn fpu_probes(world: &mut PlotWorld, descriptor: &Value) -> Result<Vec<BlockPos>> {
    let origin = origin(descriptor);
    if let Some(outputs) = descriptor["observations"]["output_msb_first"].as_array() {
        ensure!(
            outputs.len() == 10,
            "divider must have ten output repeaters"
        );
        return outputs
            .iter()
            .map(|output| {
                let pos = origin + position(output);
                ensure!(
                    matches!(world.get_block(pos), Block::RedstoneRepeater { .. }),
                    "missing divider output repeater at {pos:?}"
                );
                Ok(pos)
            })
            .collect();
    }
    let outputs = descriptor["observations"]["output"]
        .as_array()
        .context("missing full-FPU output dust bank")?;
    ensure!(
        outputs.len() == 16,
        "full FPU must have sixteen output ports"
    );
    let mut probes = Vec::with_capacity(outputs.len());
    for output in outputs {
        let pos = origin + position(output);
        let Block::RedstoneWire { wire } = world.get_block(pos) else {
            bail!("full-FPU output at {pos:?} is not dust");
        };
        let probe = pos.offset(BlockFace::East);
        ensure!(
            world.get_block(probe) == Block::Air,
            "full-FPU Lamp probe position {probe:?} must be air"
        );
        world.set_block(
            probe,
            Block::RedstoneLamp {
                lit: wire.power > 0,
            },
        );
        probes.push(probe);
    }
    Ok(probes)
}

fn fpu_changing_inputs(
    compiler: &mut Option<Compiler>,
    world: &mut PlotWorld,
    descriptor: &Value,
    probes: &[BlockPos],
    episodes: usize,
    seed: u64,
    input_every: usize,
    flush_every: u32,
    capture_trace: bool,
    expected: Option<&[Vec<u16>]>,
) -> Result<(Value, Vec<Vec<u16>>)> {
    let origin = origin(descriptor);
    let full_fpu = descriptor["ports"]["light_blue_operand_inputs"].is_array();
    let warm_episodes = if full_fpu { 32 } else { 8 };
    let initial_ticks = if full_fpu { 1024 } else { 64 };
    let episode_ticks = if full_fpu { input_every } else { 256 };
    let operand_bits = if full_fpu { 16 } else { 10 };
    let opcode_bits = if full_fpu { 3 } else { 0 };
    let trigger = origin + position(&descriptor["observations"]["trigger"][0]);
    ensure!(
        matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered),
        "saved full-FPU trigger must be an ON lever at {trigger:?}"
    );
    let mut inputs = Vec::with_capacity(operand_bits * 2 + opcode_bits);
    let banks: &[(&str, usize)] = if full_fpu {
        &[
            ("light_blue_operand_inputs", 16),
            ("red_operand_inputs", 16),
            ("opcode_levers_by_sign_weight_4_2_1", 3),
        ]
    } else {
        &[("input_a_msb_first", 10), ("input_b_msb_first", 10)]
    };
    let port_section = if full_fpu {
        &descriptor["ports"]
    } else {
        &descriptor["observations"]
    };
    for &(key, count) in banks {
        let bank = port_section[key]
            .as_array()
            .with_context(|| format!("missing full-FPU {key}"))?;
        ensure!(
            bank.len() == count,
            "full-FPU {key} requires {count} levers"
        );
        inputs.extend(bank.iter().map(|pos| origin + position(pos)));
    }
    let read_inputs = |world: &PlotWorld| -> Result<u64> {
        inputs.iter().enumerate().try_fold(0, |word, (bit, &pos)| {
            let Block::Lever { lever } = world.get_block(pos) else {
                bail!("missing full-FPU input lever at {pos:?}");
            };
            Ok(word | (u64::from(lever.powered) << bit))
        })
    };
    let read_output = |world: &PlotWorld| -> Result<u16> {
        probes.iter().try_fold(0, |word, &pos| {
            let active = match (world.get_block(pos), full_fpu) {
                (Block::RedstoneLamp { lit }, true) => lit,
                (Block::RedstoneRepeater { repeater }, false) => !repeater.powered,
                _ => bail!("missing FPU output at {pos:?}"),
            };
            Ok((word << 1) | u16::from(active))
        })
    };
    let mut current_inputs = read_inputs(world)?;
    // Generate all raw stimuli before the phase timers. Bit indices follow
    // manifest array order; this does not assert an IEEE operand representation.
    let mut state = seed;
    let mut previous = current_inputs;
    let stimuli: Vec<_> = (0..warm_episodes + episodes)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mut word = state & ((1u64 << inputs.len()) - 1);
            if word == previous {
                word ^= 1;
            }
            previous = word;
            word
        })
        .collect();
    if full_fpu {
        use_lever(compiler, world, trigger);
    }
    for _ in 0..initial_ticks {
        tick(compiler, world);
    }
    flush(compiler, world);
    if let Some(expected) = expected {
        ensure!(
            expected.len() == episodes,
            "full-FPU reference episode count changed"
        );
    }
    let mut phase_times = [Duration::ZERO; 3];
    let mut traces = Vec::with_capacity(episodes);
    let mut boundaries = Vec::with_capacity(episodes);
    let mut latencies = Vec::with_capacity(episodes);
    let mut input_flips = 0u64;
    let mut boundary_hash = Sha256::new();
    for (index, &stimulus) in stimuli.iter().enumerate() {
        let measured = index >= warm_episodes;
        let episode = index.saturating_sub(warm_episodes);
        let recording = measured && capture_trace;
        let flips = (current_inputs ^ stimulus).count_ones();
        ensure!(
            flips != 0,
            "full-FPU stimulus must change at least one input"
        );
        let mut trace = Vec::new();
        let mut phase_words = Vec::with_capacity(3);
        let mut boundary_tick = 0;
        let before_output = if full_fpu && recording {
            read_output(world)?
        } else {
            0
        };
        let phases: &[usize] = if full_fpu {
            std::slice::from_ref(&input_every)
        } else {
            &[64, 128, 64]
        };
        for (phase, phase_ticks) in phases.iter().copied().enumerate() {
            let started = Instant::now();
            if phase == 0 {
                // The public API delivers these source changes separately.
                // Full FPU stays OFF; divider inputs change while ON.
                for (bit, &pos) in inputs.iter().enumerate() {
                    if ((current_inputs ^ stimulus) >> bit) & 1 != 0 {
                        use_lever(compiler, world, pos);
                    }
                }
            } else {
                use_lever(compiler, world, trigger);
            }
            for _ in 0..phase_ticks {
                tick(compiler, world);
                if flush_every == 1 {
                    flush(compiler, world);
                }
                if recording {
                    if flush_every == 0 {
                        flush(compiler, world);
                    }
                    trace.push(read_output(world)?);
                }
            }
            let duration = started.elapsed();
            if measured {
                phase_times[phase] += duration;
            }
            // With flush0, publication and observation are outside whole-phase
            // timers. The independent validation replay observes every tick.
            if flush_every == 0 {
                flush(compiler, world);
            }
            let word = read_output(world)?;
            ensure!(
                read_inputs(world)? == stimulus,
                "full-FPU input vector changed during a held phase"
            );
            if full_fpu {
                ensure!(
                    matches!(world.get_block(trigger), Block::Lever { lever } if !lever.powered),
                    "full-FPU trigger must remain OFF throughout the input stream"
                );
            }
            boundary_tick += phase_ticks;
            if measured {
                if let Some(expected) = expected {
                    ensure!(
                        expected[episode].len() == episode_ticks,
                        "full-FPU reference trace length changed"
                    );
                    ensure!(word == expected[episode][boundary_tick - 1], "full-FPU episode {} phase {phase} output {word} differs from plain reference {}", episode + 1, expected[episode][boundary_tick - 1]);
                }
                boundary_hash.update(word.to_le_bytes());
                phase_words.push(word);
            }
        }
        current_inputs = stimulus;
        if measured {
            input_flips += u64::from(flips);
            if recording {
                if let Some(expected) = expected {
                    for (game_tick, (&actual, &reference)) in
                        trace.iter().zip(&expected[episode]).enumerate()
                    {
                        ensure!(actual == reference, "full-FPU episode {} game tick {} output {actual} differs from plain reference {reference}", episode + 1, game_tick + 1);
                    }
                }
                latencies.push(if full_fpu {
                    trace
                        .iter()
                        .position(|&word| word != before_output)
                        .map(|tick| tick + 1)
                } else {
                    trace[64..192]
                        .iter()
                        .position(|&word| word != trace[63])
                        .map(|tick| tick + 1)
                });
                traces.push(trace);
            }
            boundaries.push(phase_words);
        }
    }
    let activity_trace = if capture_trace {
        traces.as_slice()
    } else {
        expected.context("timed full-FPU workload needs a validated reference")?
    };
    let first = activity_trace
        .first()
        .and_then(|trace| trace.first())
        .context("empty full-FPU validation trace")?;
    let varied = activity_trace.iter().flatten().any(|word| word != first);
    let mut unique_words: Vec<_> = activity_trace.iter().flatten().copied().collect();
    unique_words.sort_unstable();
    unique_words.dedup();
    let mut trace_hash = Sha256::new();
    for word in activity_trace.iter().flatten() {
        trace_hash.update(word.to_le_bytes());
    }
    let seconds = phase_times.iter().sum::<Duration>().as_secs_f64();
    let report = json!({
        "measurement_kind": if !varied { "fpu_unverified_stimulus_diagnostic" } else if full_fpu { "full_fpu_raw_changing_inputs" } else { "divider_raw_changing_inputs" }, "activity_confirmed": varied,
        "seconds": varied.then_some(seconds), "raw_seconds": seconds, "episodes": episodes, "game_ticks": episodes * episode_ticks,
        "tps": varied.then_some(episodes as f64 * episode_ticks as f64 / seconds), "input_episodes_per_second": varied.then_some(episodes as f64 / seconds),
        "no_throughput_reason": (!varied).then_some("no visible output transitions in validation; this input/trigger window is not a confirmed active FPU workload"),
        "unique_output_words": unique_words,
        "unique_output_words_scope": "separate untimed per-game-tick validation; actual timed outputs are the phase-boundary words",
        "phase_seconds": if full_fpu { json!({"continuous_off": phase_times[0].as_secs_f64()}) } else { json!({"held_on_prepare64": phase_times[0].as_secs_f64(), "off_compute128": phase_times[1].as_secs_f64(), "on_reset64": phase_times[2].as_secs_f64()}) },
        "warmup_game_ticks": initial_ticks + warm_episodes * episode_ticks,
        "input_lever_bit_flips": input_flips, "trigger_edges": if full_fpu { 0 } else { episodes * 2 },
        "explicit_stimulus_calls": input_flips + if full_fpu { 0 } else { episodes as u64 * 2 },
        "input_updates": if full_fpu { "ordered changed-lever callbacks while trigger remains OFF; inputs held during each native tick interval; no atomic multi-input API" } else { "ordered changed-lever callbacks while ON; all input/opcode levers held for ON64/OFF128/ON64; no atomic multi-input API" },
        "input_every_game_ticks": full_fpu.then_some(input_every),
        "operand_bank_bits": operand_bits, "opcode_bits": opcode_bits, "input_lever_count": inputs.len(),
        "random_generator": "stdlib xorshift64(13,7,17)", "seed": seed,
        "raw_mask_order": "LSB first in each manifest port array; raw lever states, not certified numerical operands",
        "raw_vector_columns": if full_fpu { json!(["blue_port_mask", "red_port_mask", "opcode_port_mask"]) } else { json!(["input_a_port_mask", "input_b_port_mask"]) },
        "raw_vectors": stimuli[warm_episodes..].iter().map(|&word| {
            let mask = (1u64 << operand_bits) - 1;
            let mut vector = vec![word & mask, (word >> operand_bits) & mask];
            if full_fpu { vector.push(word >> (operand_bits * 2)); }
            vector
        }).collect::<Vec<_>>(),
        "input_lever_positions": inputs.iter().map(|pos| [pos.x, pos.y, pos.z]).collect::<Vec<_>>(),
        "trigger_position": [trigger.x, trigger.y, trigger.z],
        "output_adapters": if full_fpu { "sixteen derived fixed RedstoneLamp probes east of existing output dust; saved dust connection shape unchanged" } else { "none; ten existing published output repeaters" },
        "output_positions": probes.iter().map(|pos| [pos.x, pos.y, pos.z]).collect::<Vec<_>>(),
        "output_local_positions": probes.iter().map(|pos| { let local = *pos - origin; [local.x, local.y, local.z] }).collect::<Vec<_>>(),
        "output_encoding": if full_fpu { "manifest output order bit15..0; lit Lamp=1; raw presentation mask" } else { "manifest MSB-first output order; unpowered Repeater=1; raw presentation mask" },
        "phase_boundary_words": boundaries, "phase_boundary_sha256": format!("{:x}", boundary_hash.finalize()),
        "output_trace_sha256": format!("{:x}", trace_hash.finalize()),
        "output_trace_sha256_scope": if capture_trace { "actual untimed per-game-tick validation trace" } else { "validated reference trace; actual timed observations are the separate phase-boundary checksum" },
        "every_game_tick_compared": capture_trace && expected.is_some(),
        "timed_phase_boundaries_compared": !capture_trace && expected.is_some(),
        "first_output_change_game_ticks": latencies,
        "flush_policy": if flush_every == 1 { "inside every timed tick" } else { "phase boundaries only outside timers; untimed validation publishes every tick" },
        "arithmetic_validity": "random raw-input plan equivalence only; no floating-point arithmetic or ready-result oracle",
    });
    Ok((report, traces))
}

fn tick(compiler: &mut Option<Compiler>, world: &mut PlotWorld) {
    if let Some(compiler) = compiler {
        compiler.tick_with_world(world);
    } else {
        world.tick_interpreted();
    }
}

fn flush(compiler: &mut Option<Compiler>, world: &mut PlotWorld) {
    if let Some(compiler) = compiler {
        compiler.flush(world);
    }
}

fn use_lever(compiler: &mut Option<Compiler>, world: &mut PlotWorld, pos: BlockPos) {
    if let Some(compiler) = compiler {
        compiler.on_use_block(pos);
        return;
    }
    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("missing benchmark lever at {pos:?}");
    };
    lever.powered = !lever.powered;
    let block = Block::Lever { lever };
    world.set_block(pos, block);
    redstone::update_surrounding_blocks(world, pos);
    let face = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, pos.offset(face));
}

fn load(descriptor: &Value) -> Result<PlotWorld> {
    let bytes = std::fs::read(
        root().join(
            descriptor["fixture"]
                .as_str()
                .context("missing schematic path")?,
        ),
    )?;
    ensure!(
        descriptor["sha256"] == format!("{:x}", Sha256::digest(&bytes)),
        "schematic hash changed"
    );
    let clipboard = load_schematic(std::io::Cursor::new(bytes))?;
    ensure!(
        descriptor["dimensions"] == json!([clipboard.size_x, clipboard.size_y, clipboard.size_z]),
        "schematic dimensions changed"
    );
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(
        &mut world,
        &clipboard,
        origin(descriptor)
            + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    Ok(world)
}

fn position(value: &Value) -> BlockPos {
    BlockPos::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}
fn origin(descriptor: &Value) -> BlockPos {
    position(
        descriptor
            .get("origin")
            .unwrap_or(&descriptor["coordinates"]["origin"]),
    )
}
fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}
