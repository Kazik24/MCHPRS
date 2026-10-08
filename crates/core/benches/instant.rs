//! Counter/divider throughput and raw changing-input full-FPU workloads.
#![recursion_limit = "256"]

#[path = "support/cpus.rs"]
#[allow(dead_code)]
mod cpu_support;

use anyhow::{bail, ensure, Context, Result};
use mchprs_blocks::{
    blocks::{Block, LeverFace, RedstoneRepeater},
    BlockDirection, BlockFace, BlockPos,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workload {
    Random,
    RandomWalk,
    Sequence,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::RandomWalk => "random-walk",
            Self::Sequence => "sequence",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Random => "independent xorshift64 raw port masks; resample identical consecutive masks by flipping bit zero",
            Self::RandomWalk => "xorshift64 chooses one input bit to flip per vector, including opcode bits",
            Self::Sequence => "cycle 128 fixed raw port pairs (i, 127-i), i=0..127; opcode mask zero; seed independent",
        }
    }
}

fn input_stimuli(
    workload: Workload,
    initial: u64,
    operand_bits: usize,
    opcode_bits: usize,
    seed: u64,
    count: usize,
) -> Vec<u64> {
    let width = operand_bits * 2 + opcode_bits;
    assert!((7..=31).contains(&operand_bits) && width < 64 && seed != 0);
    let mask = (1u64 << width) - 1;
    let mut state = seed;
    let mut previous = initial & mask;
    (0..count)
        .map(|index| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let word = match workload {
                Workload::Random => {
                    let word = state & mask;
                    if word == previous {
                        word ^ 1
                    } else {
                        word
                    }
                }
                Workload::RandomWalk => previous ^ (1 << (state % width as u64)),
                Workload::Sequence => {
                    let value = (index % 128) as u64;
                    value | ((127 - value) << operand_bits)
                }
            };
            previous = word;
            word
        })
        .collect()
}

fn check_workloads() {
    for (bits, opcode_bits) in [(10, 0), (16, 3)] {
        for workload in [Workload::Random, Workload::RandomWalk, Workload::Sequence] {
            let initial = 3;
            let words = input_stimuli(workload, initial, bits, opcode_bits, 7, 384);
            assert_eq!(
                words,
                input_stimuli(workload, initial, bits, opcode_bits, 7, 384)
            );
            assert!(words
                .iter()
                .all(|word| word >> (bits * 2 + opcode_bits) == 0));
            if workload == Workload::Sequence {
                assert_eq!(&words[..128], &words[128..256]);
                assert_eq!(
                    words,
                    input_stimuli(workload, 999, bits, opcode_bits, 11, 384)
                );
                let mut unique = words[..128].to_vec();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), 128);
                for (index, word) in words.iter().enumerate() {
                    assert_eq!(word & ((1 << bits) - 1), (index % 128) as u64);
                    assert_eq!(word >> bits, (127 - index % 128) as u64);
                }
            } else {
                assert_ne!(
                    words,
                    input_stimuli(workload, initial, bits, opcode_bits, 11, 384)
                );
                let mut previous = initial;
                for word in words {
                    let flips = (previous ^ word).count_ones();
                    assert!(flips > 0);
                    if workload == Workload::RandomWalk {
                        assert_eq!(flips, 1);
                    }
                    previous = word;
                }
            }
        }
    }
    println!(
        "workload checks passed: deterministic random, one-bit random walk, 128-vector sequence"
    );
}

fn main() -> Result<()> {
    let mut component = String::from("counter_basic");
    let mut iterations = 3usize;
    let mut episodes = 32usize;
    let mut ticks = 60_000u32;
    let mut flush_every = 0u32;
    let mut optimize = false;
    let mut interpreted = false;
    let mut changing_inputs = false;
    let mut workload = Workload::Random;
    let mut seed = 0x4d43_4850_5253_2026u64;
    let mut input_every = 1usize;
    let mut output = None;
    let mut reference_path = None;
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
            "--check-workloads" => {
                check_workloads();
                return Ok(());
            }
            "--help" => {
                println!("instant [--component counter_basic|pc_counter|cpu_bubblesort|fpu_divider|fpu_legal] [--changing-inputs | --workload random|random-walk|sequence] [--seed integer] [--input-every ticks] [--interpreted | --optimize] [--iterations 3] [--episodes 32] [--ticks 60000] [--flush-every 0|1] [--output path.json] [--reference frozen-native-report.json]\n--workload implies --changing-inputs. Sequence repeats 128 fixed raw port pairs with opcode zero.\n--check-workloads verifies generators without loading a schematic.");
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
            "--reference" => reference_path = Some(value),
            "--seed" => seed = value.parse()?,
            "--input-every" => input_every = value.parse()?,
            "--workload" => {
                workload = match value.as_str() {
                    "random" => Workload::Random,
                    "random-walk" => Workload::RandomWalk,
                    "sequence" => Workload::Sequence,
                    _ => bail!("unknown workload {value}; use random, random-walk or sequence"),
                };
                changing_inputs = true;
            }
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
        !interpreted
            || matches!(
                component.as_str(),
                "counter_basic" | "pc_counter" | "fpu_divider" | "fpu_legal"
            ),
        "unsupported interpreted component"
    );
    ensure!(
        !changing_inputs || matches!(component.as_str(), "fpu_legal" | "fpu_divider"),
        "--changing-inputs applies to fpu_legal or fpu_divider"
    );
    ensure!(
        component != "pc_counter" || interpreted,
        "pc_counter benchmark requires --interpreted"
    );
    ensure!(
        component != "fpu_legal" || !interpreted || changing_inputs,
        "interpreted full FPU requires --changing-inputs or --workload"
    );
    ensure!(
        reference_path.is_none() || (interpreted && (changing_inputs || component == "pc_counter")),
        "--reference requires an interpreted FPU input workload or pc_counter"
    );
    let frozen: Option<Value> = reference_path
        .as_ref()
        .map(|path| -> Result<Value> {
            Ok(serde_json::from_slice(&std::fs::read(root().join(path))?)?)
        })
        .transpose()?;
    let manifest_path = match component.as_str() {
        "counter_basic" => "test_data/instant-pistons-io/fixtures/counter_basic.json",
        "pc_counter" => "inline revised Potados PC counter protocol",
        "cpu_bubblesort" => "test_data/piston-research/fixtures/cpu_bubblesort.json",
        "fpu_divider" => "test_data/piston-research/fixtures/fpu_divider.json",
        "fpu_legal" => "test_data/piston-research/fixtures/fpu_legal.json",
        _ => bail!("unknown component {component}"),
    };
    let mut descriptor: Value = if component == "pc_counter" {
        json!({"fixture":"test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem",
            "sha256":"641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7",
            "dimensions":[24,19,75], "origin":[40,30,40]})
    } else {
        serde_json::from_slice(&std::fs::read(root().join(manifest_path))?)?
    };
    if let Some(frozen) = &frozen {
        ensure!(
            frozen["schema"] == 1
                && frozen["backend"] == "interpreted"
                && frozen["component"] == component
                && frozen["schematic_sha256"] == descriptor["sha256"],
            "frozen report belongs to a different workload or fixture"
        );
        if changing_inputs {
            ensure!(
                component != "fpu_legal"
                    || frozen["input_protocol_id"] == "native-fpu-cycled-off-inputs-on-v1",
                "frozen FPU report uses a different trigger protocol"
            );
            ensure!(
                frozen["episodes"] == episodes
                    && frozen["seed"] == seed
                    && frozen["workload"] == workload.name()
                    && frozen["input_every_game_ticks"]
                        == if component == "fpu_legal" {
                            json!(input_every)
                        } else {
                            Value::Null
                        },
                "frozen FPU workload parameters differ"
            );
        }
    }
    if component == "cpu_bubblesort" {
        // Same closed-context origin as the compiled CPU harness.
        descriptor["origin"] = json!([2, 8, 2]);
    } else if component == "fpu_legal" && descriptor.get("origin").is_none() {
        descriptor["origin"] = json!([40, 30, 40]);
    }
    // Freeze native outputs or validate the compiled plan before timing stimulus and ticks.
    let mut reference_traces: Option<Vec<Vec<u16>>> = frozen
        .as_ref()
        .map(|report| -> Result<_> {
            Ok(serde_json::from_value(
                report["per_game_tick_output_words"].clone(),
            )?)
        })
        .transpose()?;
    let mut captured_traces = None;
    let mut validation = None;
    if changing_inputs {
        for checked_optimize in [false, optimize] {
            let mut world = load(&descriptor)?;
            let probes = fpu_probes(&mut world, &descriptor)?;
            if interpreted {
                world.set_random_tick_speed(0);
            }
            let mut compiler = (!interpreted).then(Compiler::default);
            if let Some(compiler) = &mut compiler {
                compiler
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
            }
            let (report, traces) = fpu_changing_inputs(
                &mut compiler,
                &mut world,
                &descriptor,
                &probes,
                episodes,
                seed,
                workload,
                input_every,
                flush_every,
                true,
                reference_traces.as_deref(),
            )?;
            if interpreted {
                captured_traces = Some(traces.clone());
            }
            if reference_traces.is_none() {
                reference_traces = Some(traces);
            }
            validation = Some(report);
            if interpreted {
                break;
            }
        }
    }
    let mut samples = Vec::new();
    let mut native_final_checkpoint = None;
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
        if interpreted {
            world.set_random_tick_speed(0);
        }
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
                        workload,
                        input_every,
                        flush_every,
                        false,
                        reference_traces.as_deref(),
                    )
                    .map(|(report, _)| report)
                } else if component == "pc_counter" {
                    pc_counter(&mut world, &descriptor, reference_traces.as_deref())
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
                let mut result = if interpreted {
                    result?
                } else {
                    result.unwrap_or_else(|error| json!({"error": error.to_string()}))
                };
                if !interpreted {
                    result["admitted"] = json!(true);
                }
                result
            }
        };
        if interpreted {
            sample["instant_piston_cache_stats"] =
                json!(world.instant_piston_cache().map(|cache| cache.stats()));
            sample["final_checkpoint"] = json!(cpu_support::checkpoint(
                &world,
                if component == "pc_counter" {
                    4096
                } else if changing_inputs {
                    (episodes
                        * if component == "fpu_legal" {
                            input_every * 2
                        } else {
                            256
                        }) as u32
                } else {
                    ticks
                },
                &[]
            ));
            if let Some(frozen) = &frozen {
                ensure!(
                    sample["final_checkpoint"] == frozen["samples"][0]["final_checkpoint"],
                    "native final whole-world checkpoint differs from frozen baseline"
                );
            }
            if let Some(checkpoint) = &native_final_checkpoint {
                ensure!(
                    sample["final_checkpoint"] == *checkpoint,
                    "native final whole-world checkpoint differs across iterations"
                );
            } else {
                native_final_checkpoint = Some(sample["final_checkpoint"].clone());
            }
            if component == "pc_counter" {
                let traces = serde_json::from_value(sample["per_game_tick_output_words"].take())?;
                if reference_traces.is_none() {
                    reference_traces = Some(traces);
                }
            }
        }
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
    let mut input_elapsed: Vec<_> = samples
        .iter()
        .filter_map(|sample| sample["raw_seconds"].as_f64())
        .collect();
    input_elapsed.sort_by(f64::total_cmp);
    let input_median = input_elapsed.get(input_elapsed.len() / 2).copied();
    let changing_game_ticks = episodes as u64
        * if component == "fpu_legal" {
            input_every as u64 * if interpreted { 2 } else { 1 }
        } else {
            256
        };
    let report = json!({
        "schema": 1, "backend": backend, "component": component, "manifest": manifest_path, "schematic_sha256": descriptor["sha256"],
        "actual_origin": [origin(&descriptor).x, origin(&descriptor).y, origin(&descriptor).z],
        "flags": if interpreted { None } else { Some(if optimize { "-O --assume-instant" } else { "--assume-instant" }) }, "budget_multiplier": (!interpreted).then_some(8),
        "admitted_means": (!interpreted).then_some("compiler acceptance only; no physical or arithmetic equivalence claim"),
        "scope": if interpreted { "native interpreter with random ticks disabled; per-tick outputs and final whole-world checkpoint compared with --reference" } else { "actual logical workloads; BubbleSort and full FPU without --changing-inputs are admission only; no runtime fallback" },
        "measurement_kind": if changing_inputs { if component == "fpu_legal" { "full_fpu_raw_changing_inputs" } else { "divider_raw_changing_inputs" } } else { match (component.as_str(), interpreted) { ("pc_counter", true) => "interpreted_pc_counter_runtime", ("counter_basic", true) => "interpreted_counter_runtime", ("counter_basic", false) => "logical_counter_runtime", ("fpu_divider", true) => "interpreted_divider_episodes", ("fpu_divider", false) => "logical_divider_episodes", _ => "admission_only" } },
        "timing": if changing_inputs {
            format!("timers include lever stimulus, {tick_operation} and optional compiled flush; observations, preparation and independent validation excluded")
        } else if component == "pc_counter" {
            String::from("4096 per-tick tick_interpreted timers after 23 ordered lever activations and one untimed activation tick; observation and checkpoint excluded")
        } else if component == "fpu_divider" {
            format!("accumulated stimulus and per-tick timers include lever use, {tick_operation} and optional compiled flush; initialization, one warm episode and output observations excluded")
        } else {
            format!("per-tick {tick_operation}; optional compiled flush included; preparation and observations excluded")
        },
        "interpreter_flush_policy": interpreted.then_some("no-op: interpreter block state is already published"),
        "counter_protocol": "exact IO fixture; 24 inactive ticks, lever OFF->ON, 600 active warmup ticks, then fixed active window",
        "pc_counter_protocol": (component == "pc_counter").then_some("revised saved fixture; all 23 OFF levers enabled in Y/Z/X order; one untimed activation tick; 4096 measured ticks; exact decoded pulse peaks 1..682 and final count 682"),
        "random_tick_speed": interpreted.then_some(0),
        "game_ticks": if component == "pc_counter" { Some(4096) } else if changing_inputs { Some(changing_game_ticks) } else if component == "counter_basic" { Some(u64::from(ticks)) } else { None }, "flush_every_game_ticks": flush_every,
        "divider_protocol": (component == "fpu_divider" && !changing_inputs).then_some("saved A/B held; 64 ON initialization ticks; one untimed OFF128/ON128 warm episode; repeated held OFF128/ON128 complete episodes preserving ordinary clock timing; observe response170 during OFF and verify final reset0 after ON"),
        "episodes": (component == "fpu_divider" || changing_inputs).then_some(episodes),
        "changing_inputs": changing_inputs,
        "workload": changing_inputs.then_some(workload.name()),
        "workload_description": changing_inputs.then_some(workload.description()),
        "sequence_period": (changing_inputs && workload == Workload::Sequence).then_some(128),
        "seed": changing_inputs.then_some(seed),
        "input_every_game_ticks": (changing_inputs && component == "fpu_legal").then_some(input_every),
        "input_protocol_id": (changing_inputs && component == "fpu_legal" && interpreted).then_some("native-fpu-cycled-off-inputs-on-v1"),
        "changing_input_protocol": changing_inputs.then_some(if component == "fpu_legal" && interpreted {
            "cycled native raw-port stream: trigger ensured ON for 64 initial ticks; eight untimed warm vectors; each vector sets trigger OFF, delivers ordered changed-lever updates, holds OFF for input-every ticks, then ON for input-every reset ticks; no arithmetic or readiness oracle"
        } else if component == "fpu_legal" {
            "continuous OFF raw-port stream: trigger ensured OFF before warmup and held OFF; 1024 initial ticks and untimed workload warmup (128 vectors for sequence, 32 otherwise); each measured vector delivers ordered changed-lever updates then input-every native game ticks; no arithmetic or readiness oracle"
        } else {
            "fixed-window raw-port benchmark: 64 initial ON ticks; untimed workload warmup (128 vectors for sequence, eight otherwise); ordered lever updates set operand port masks while ON, then hold all inputs ON64, OFF128, ON64; no arithmetic or readiness oracle"
        }),
        "trace_validation": validation.as_ref().map(|report| json!({
            "reference": if interpreted { if frozen.is_some() { "supplied frozen native baseline" } else { "untimed native capture; reuse this report with --reference" } } else { "independent unoptimized --assume-instant compiler with identical output ports/adapters and workload" },
            "selected_plan_every_game_tick_matches": !interpreted || frozen.is_some(),
            "output_varied": report["activity_confirmed"],
            "output_trace_sha256": report["output_trace_sha256"],
            "first_output_change_game_ticks": report["first_output_change_game_ticks"],
            "latency_scope": if component == "fpu_legal" && interpreted { "untimed replay; first visible output-word change after trigger OFF and vector update relative to previous ON interval boundary; no valid-result latency claim" } else if component == "fpu_legal" { "untimed replay; first visible output-word change after vector update relative to previous interval boundary; no valid-result latency claim" } else { "untimed replay; first visible output-word change after OFF relative to end of ON preparation; no valid-result latency claim" },
        })),
        "reference_path": reference_path,
        "per_game_tick_output_words": if interpreted { captured_traces.as_ref().or(reference_traces.as_ref()) } else { None },
        "median_seconds": median,
        "median_tps": median.filter(|_| component == "pc_counter" || component == "counter_basic" || changing_inputs).map(|seconds| if changing_inputs { changing_game_ticks as f64 / seconds } else if component == "pc_counter" { 4096.0 / seconds } else { f64::from(ticks) / seconds }),
        "median_ns_per_tick": median.filter(|_| component == "pc_counter" || component == "counter_basic" || changing_inputs).map(|seconds| seconds * 1e9 / if changing_inputs { changing_game_ticks as f64 } else if component == "pc_counter" { 4096.0 } else { f64::from(ticks) }),
        "median_episodes_per_second": median.filter(|_| component == "fpu_divider" || changing_inputs).map(|seconds| episodes as f64 / seconds),
        "median_input_processing_seconds": input_median,
        "median_input_processing_tps": input_median.map(|seconds| changing_game_ticks as f64 / seconds),
        "median_input_sets_per_second": input_median.map(|seconds| episodes as f64 / seconds),
        "input_processing_scope": changing_inputs.then_some("stimulus and selected backend tick cost; reported even when outputs stay constant; not completed arithmetic throughput"),
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

fn pc_counter(
    world: &mut PlotWorld,
    descriptor: &Value,
    expected: Option<&[Vec<u16>]>,
) -> Result<Value> {
    let origin = origin(descriptor);
    let mut levers = Vec::new();
    for y in 0..19 {
        for z in 0..75 {
            for x in 0..24 {
                let pos = origin + BlockPos::new(x, y, z);
                if let Block::Lever { lever } = world.get_block(pos) {
                    ensure!(
                        !lever.powered,
                        "PC counter lever must initially be OFF at {pos:?}"
                    );
                    levers.push(pos);
                }
            }
        }
    }
    ensure!(levers.len() == 23, "PC counter must have 23 controls");
    let mut native = None;
    for pos in levers {
        use_lever(&mut native, world, pos);
    }
    world.tick_interpreted();
    let read = |world: &PlotWorld| -> Result<u16> {
        (0..16).try_fold(0u16, |word, bit| {
            let pos = origin + BlockPos::new(13, 2, 3 + bit * 4);
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                bail!("missing PC counter output at {pos:?}");
            };
            Ok((word << 1) | u16::from(!repeater.powered))
        })
    };
    if let Some(expected) = expected {
        ensure!(
            expected.len() == 1 && expected[0].len() == 4096,
            "PC counter reference must have exactly 4096 output words"
        );
    }
    let mut elapsed = Duration::ZERO;
    let mut words = Vec::with_capacity(4096);
    let mut peaks = Vec::new();
    let mut peak = 0;
    for tick in 0..4096 {
        let started = Instant::now();
        world.tick_interpreted();
        elapsed += started.elapsed();
        let word = read(world)?;
        if let Some(expected) = expected {
            ensure!(
                word == expected[0][tick],
                "PC counter output differs from baseline at tick {}",
                tick + 1
            );
        }
        if word == 0 && peak != 0 {
            peaks.push(peak);
            peak = 0;
        }
        peak = peak.max(word);
        words.push(word);
    }
    if peak != 0 {
        peaks.push(peak);
    }
    ensure!(
        peaks == (1..=682).collect::<Vec<u16>>() && words.last() == Some(&682),
        "PC counter must produce consecutive pulse peaks 1 through 682"
    );
    Ok(
        json!({"measurement_kind":"interpreted_pc_counter_runtime", "activity_confirmed":true,
        "seconds":elapsed.as_secs_f64(), "game_ticks":4096,
        "peak_count":peaks.len(), "final_count":words.last(),
        "every_game_tick_compared":expected.is_some(), "per_game_tick_output_words":[words]}),
    )
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
    if let Some(outputs) = descriptor["observations"]["output_repeaters"].as_array() {
        ensure!(
            outputs.len() == 16,
            "full FPU must have sixteen native output repeaters"
        );
        return outputs
            .iter()
            .map(|output| {
                let pos = origin + position(output);
                ensure!(
                    matches!(world.get_block(pos), Block::RedstoneRepeater { .. }),
                    "missing full-FPU output repeater at {pos:?}"
                );
                Ok(pos)
            })
            .collect();
    }
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
            "full-FPU repeater probe position {probe:?} must be air"
        );
        let support = probe.offset(BlockFace::Bottom);
        ensure!(
            world.get_block(support) == Block::Air,
            "full-FPU repeater support {support:?} must be air"
        );
        world.set_block(support, Block::Stone {});
        world.set_block(
            probe,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 1,
                    facing: BlockDirection::East,
                    locked: false,
                    powered: wire.power > 0,
                },
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
    workload: Workload,
    input_every: usize,
    flush_every: u32,
    capture_trace: bool,
    expected: Option<&[Vec<u16>]>,
) -> Result<(Value, Vec<Vec<u16>>)> {
    let origin = origin(descriptor);
    let full_fpu = descriptor["ports"]["light_blue_operand_inputs"].is_array();
    let cycled_full_fpu = full_fpu && compiler.is_none();
    let warm_episodes = if cycled_full_fpu {
        8
    } else if workload == Workload::Sequence {
        128
    } else if full_fpu {
        32
    } else {
        8
    };
    let initial_ticks = if full_fpu && !cycled_full_fpu {
        1024
    } else {
        64
    };
    let episode_ticks = if cycled_full_fpu {
        input_every * 2
    } else if full_fpu {
        input_every
    } else {
        256
    };
    if let Some(expected) = expected {
        ensure!(
            expected.len() == episodes,
            "full-FPU reference episode count changed"
        );
        ensure!(
            expected.iter().all(|trace| trace.len() == episode_ticks),
            "full-FPU reference trace length changed"
        );
    }
    let operand_bits = if full_fpu { 16 } else { 10 };
    let opcode_bits = if full_fpu { 3 } else { 0 };
    let trigger = origin + position(&descriptor["observations"]["trigger"][0]);
    let Block::Lever {
        lever: trigger_lever,
    } = world.get_block(trigger)
    else {
        bail!("missing FPU trigger lever at {trigger:?}");
    };
    ensure!(
        full_fpu || trigger_lever.powered,
        "saved divider trigger must be ON at {trigger:?}"
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
                (Block::RedstoneRepeater { repeater }, true) => repeater.powered,
                (Block::RedstoneRepeater { repeater }, false) => !repeater.powered,
                _ => bail!("missing FPU output at {pos:?}"),
            };
            Ok((word << 1) | u16::from(active))
        })
    };
    let mut current_inputs = read_inputs(world)?;
    // Generate all raw stimuli before the phase timers. Bit indices follow
    // manifest array order; this does not assert an IEEE operand representation.
    let stimuli = input_stimuli(
        workload,
        current_inputs,
        operand_bits,
        opcode_bits,
        seed,
        warm_episodes + episodes,
    );
    if full_fpu && trigger_lever.powered != cycled_full_fpu {
        use_lever(compiler, world, trigger);
    }
    for _ in 0..initial_ticks {
        tick(compiler, world);
    }
    flush(compiler, world);
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
        let mut trace = Vec::new();
        let mut phase_words = Vec::with_capacity(3);
        let mut boundary_tick = 0;
        let before_output = if full_fpu && recording {
            read_output(world)?
        } else {
            0
        };
        let phases: &[usize] = if cycled_full_fpu {
            &[input_every, input_every]
        } else if full_fpu {
            std::slice::from_ref(&input_every)
        } else {
            &[64, 128, 64]
        };
        for (phase, phase_ticks) in phases.iter().copied().enumerate() {
            let started = Instant::now();
            if phase == 0 {
                // The public API delivers these source changes separately.
                // Native FPU rearms between vectors; compiled FPU stays OFF.
                if cycled_full_fpu {
                    use_lever(compiler, world, trigger);
                }
                for (bit, &pos) in inputs.iter().enumerate() {
                    if ((current_inputs ^ stimulus) >> bit) & 1 != 0 {
                        use_lever(compiler, world, pos);
                    }
                }
            } else {
                use_lever(compiler, world, trigger);
            }
            let mut native_elapsed = started.elapsed();
            for game_tick in 0..phase_ticks {
                let native_started = compiler.is_none().then(Instant::now);
                tick(compiler, world);
                if flush_every == 1 {
                    flush(compiler, world);
                }
                if let Some(native_started) = native_started {
                    native_elapsed += native_started.elapsed();
                }
                if compiler.is_none() && measured && !capture_trace {
                    let expected =
                        expected.context("native workload needs a captured reference")?;
                    ensure!(
                        read_output(world)? == expected[episode][boundary_tick + game_tick],
                        "native FPU episode {} game tick {} differs from baseline",
                        episode + 1,
                        boundary_tick + game_tick + 1
                    );
                }
                if recording {
                    if flush_every == 0 {
                        flush(compiler, world);
                    }
                    trace.push(read_output(world)?);
                }
            }
            let duration = if compiler.is_none() {
                native_elapsed
            } else {
                started.elapsed()
            };
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
                    matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered == (cycled_full_fpu && phase == 1)),
                    "full-FPU trigger differs from the declared phase"
                );
            }
            boundary_tick += phase_ticks;
            if measured {
                if let Some(expected) = expected {
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
    let measured_stimuli = &stimuli[warm_episodes..];
    let mut unique_inputs = measured_stimuli.to_vec();
    unique_inputs.sort_unstable();
    unique_inputs.dedup();
    let mut input_hash = Sha256::new();
    for word in measured_stimuli {
        input_hash.update(word.to_le_bytes());
    }
    let report = json!({
        "measurement_kind": if !varied { "fpu_unverified_stimulus_diagnostic" } else if full_fpu { "full_fpu_raw_changing_inputs" } else { "divider_raw_changing_inputs" }, "activity_confirmed": varied,
        "seconds": varied.then_some(seconds), "raw_seconds": seconds, "episodes": episodes, "game_ticks": episodes * episode_ticks,
        "input_processing_tps": episodes as f64 * episode_ticks as f64 / seconds,
        "input_sets_per_second": episodes as f64 / seconds,
        "changed_input_bits_per_second": input_flips as f64 / seconds,
        "tps": varied.then_some(episodes as f64 * episode_ticks as f64 / seconds), "input_episodes_per_second": varied.then_some(episodes as f64 / seconds),
        "no_throughput_reason": (!varied).then_some("no visible output transitions in validation; this input/trigger window is not a confirmed active FPU workload"),
        "unique_output_words": unique_words,
        "unique_output_words_scope": if compiler.is_none() { "separate untimed per-game-tick validation; timed native runs compare every game tick outside timers" } else { "separate untimed per-game-tick validation; actual timed outputs are the phase-boundary words" },
        "phase_seconds": if cycled_full_fpu { json!({"off_compute":phase_times[0].as_secs_f64(), "on_reset":phase_times[1].as_secs_f64()}) } else if full_fpu { json!({"continuous_off": phase_times[0].as_secs_f64()}) } else { json!({"held_on_prepare64": phase_times[0].as_secs_f64(), "off_compute128": phase_times[1].as_secs_f64(), "on_reset64": phase_times[2].as_secs_f64()}) },
        "warmup_game_ticks": initial_ticks + warm_episodes * episode_ticks,
        "warmup_input_sets": warm_episodes,
        "input_lever_bit_flips": input_flips, "trigger_edges": if full_fpu && !cycled_full_fpu { 0 } else { episodes * 2 },
        "mean_changed_bits_per_input_set": input_flips as f64 / episodes as f64,
        "unique_input_sets": unique_inputs.len(), "repeated_input_sets": episodes - unique_inputs.len(),
        "input_stream_sha256": format!("{:x}", input_hash.finalize()),
        "workload": workload.name(), "workload_description": workload.description(),
        "sequence_period": (workload == Workload::Sequence).then_some(128),
        "explicit_stimulus_calls": input_flips + if full_fpu && !cycled_full_fpu { 0 } else { episodes as u64 * 2 },
        "input_updates": if cycled_full_fpu { "trigger OFF before ordered changed-lever callbacks; hold OFF for input-every ticks; trigger ON and hold for input-every reset ticks; no atomic multi-input API" } else if full_fpu { "ordered changed-lever callbacks while trigger remains OFF; inputs held during each native tick interval; no atomic multi-input API" } else { "ordered changed-lever callbacks while ON; all input/opcode levers held for ON64/OFF128/ON64; no atomic multi-input API" },
        "input_every_game_ticks": full_fpu.then_some(input_every),
        "operand_bank_bits": operand_bits, "opcode_bits": opcode_bits, "input_lever_count": inputs.len(),
        "random_generator": (workload != Workload::Sequence).then_some("xorshift64(13,7,17)"), "seed": seed,
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
        "output_adapters": if full_fpu && descriptor["observations"]["output_repeaters"].is_array() { "none; sixteen existing post-adder output repeaters" } else if full_fpu { "sixteen derived east-facing delay-one repeater consumers with stone supports east of existing output dust; saved dust connection shape unchanged" } else { "none; ten existing published output repeaters" },
        "output_scope": descriptor["ports"]["output_repeaters_scope"],
        "output_positions": probes.iter().map(|pos| [pos.x, pos.y, pos.z]).collect::<Vec<_>>(),
        "output_local_positions": probes.iter().map(|pos| { let local = *pos - origin; [local.x, local.y, local.z] }).collect::<Vec<_>>(),
        "output_encoding": if full_fpu { "manifest output order bit15..0; powered Repeater=1; raw presentation mask" } else { "manifest MSB-first output order; unpowered Repeater=1; raw presentation mask" },
        "phase_boundary_words": boundaries, "phase_boundary_sha256": format!("{:x}", boundary_hash.finalize()),
        "output_trace_sha256": format!("{:x}", trace_hash.finalize()),
        "output_trace_sha256_scope": if capture_trace { "actual untimed per-game-tick validation trace" } else if compiler.is_none() { "validated reference trace; timed native runs compare every game tick outside timers" } else { "validated reference trace; actual timed observations are the separate phase-boundary checksum" },
        "every_game_tick_compared": (capture_trace || compiler.is_none()) && expected.is_some(),
        "timed_phase_boundaries_compared": !capture_trace && expected.is_some(),
        "first_output_change_game_ticks": latencies,
        "flush_policy": if flush_every == 1 { "inside every timed tick" } else { "phase boundaries only outside timers; untimed validation publishes every tick" },
        "arithmetic_validity": "raw-input plan equivalence only; no floating-point arithmetic or ready-result oracle",
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
