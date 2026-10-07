//! Logical Counter throughput and warmed divider episodes; BubbleSort admission only.
use anyhow::{bail, ensure, Context, Result};
use mchprs_blocks::{blocks::Block, BlockPos};
use mchprs_core::{
    plot::{
        worldedit::{load_schematic, paste_clipboard},
        PlotWorld, PLOT_WIDTH,
    },
    redpiler::{Compiler, CompilerOptions},
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
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => continue,
            "--optimize" => {
                optimize = true;
                continue;
            }
            "--help" => {
                println!("instant [--component counter_basic|cpu_bubblesort|fpu_divider] [--optimize] [--iterations 3] [--episodes 32] [--ticks 60000] [--flush-every 0|1] [--output path.json]");
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
            _ => bail!("unknown argument {arg}"),
        }
    }
    ensure!(iterations > 0, "--iterations must be positive");
    ensure!(episodes > 0, "--episodes must be positive");
    ensure!(
        ticks >= 50_000 && ticks % 6 == 0,
        "--ticks must be at least 50000 and divisible by six"
    );
    ensure!(flush_every <= 1, "--flush-every must be zero or one");
    let manifest_path = match component.as_str() {
        "counter_basic" => "test_data/instant-pistons-io/fixtures/counter_basic.json",
        "cpu_bubblesort" => "test_data/piston-research/fixtures/cpu_bubblesort.json",
        "fpu_divider" => "test_data/piston-research/fixtures/fpu_divider.json",
        _ => bail!("unknown component {component}"),
    };
    let mut descriptor: Value =
        serde_json::from_slice(&std::fs::read(root().join(manifest_path))?)?;
    if component == "cpu_bubblesort" {
        // Same closed-context origin as the compiled CPU harness.
        descriptor["origin"] = json!([2, 8, 2]);
    }
    let mut samples = Vec::new();
    for index in 1..=iterations {
        println!("{component} sample {index}/{iterations}");
        let mut world = load(&descriptor)?;
        let mut compiler = Compiler::default();
        let started = Instant::now();
        let admission = compiler.compile(
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
        );
        let compile_seconds = started.elapsed().as_secs_f64();
        let mut sample = match admission {
            Err(error) => json!({"admitted": false, "error": error.to_string()}),
            Ok(()) if component == "cpu_bubblesort" => {
                json!({"admitted": true, "measurement_kind": "admission_only"})
            }
            Ok(()) => {
                let result = if component == "counter_basic" {
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
                result["admitted"] = json!(true);
                result
            }
        };
        sample["compile_seconds_metadata"] = json!(compile_seconds);
        sample["warnings"] = json!(compiler.warnings());
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
    let report = json!({
        "schema": 1, "component": component, "manifest": manifest_path, "schematic_sha256": descriptor["sha256"],
        "actual_origin": [origin(&descriptor).x, origin(&descriptor).y, origin(&descriptor).z],
        "flags": if optimize { "-O --assume-instant" } else { "--assume-instant" }, "budget_multiplier": 8,
        "admitted_means": "compiler acceptance only; no physical or arithmetic equivalence claim",
        "scope": "actual logical Counter/divider paths; BubbleSort admission only; no runtime fallback",
        "measurement_kind": match component.as_str() { "counter_basic" => "logical_counter_runtime", "fpu_divider" => "logical_divider_episodes", _ => "admission_only" },
        "timing": if component == "fpu_divider" {
            "accumulated stimulus and per-tick timers include on_use_block, tick_with_world and optional flush; initialization, one warm episode and output observations excluded"
        } else {
            "per-tick tick_with_world; optional Compiler::flush included; preparation and observations excluded"
        },
        "counter_protocol": "exact IO fixture; 24 inactive ticks, lever OFF->ON, 600 active warmup ticks, then fixed active window",
        "game_ticks": if component == "counter_basic" { Some(ticks) } else { None }, "flush_every_game_ticks": flush_every,
        "divider_protocol": (component == "fpu_divider").then_some("saved A/B held; 64 ON initialization ticks; one untimed OFF128/ON128 warm episode; repeated held OFF128/ON128 complete episodes preserving ordinary clock timing; observe response170 during OFF and verify final reset0 after ON"),
        "episodes": (component == "fpu_divider").then_some(episodes),
        "median_seconds": median,
        "median_tps": median.filter(|_| component == "counter_basic").map(|seconds| f64::from(ticks) / seconds),
        "median_ns_per_tick": median.filter(|_| component == "counter_basic").map(|seconds| seconds * 1e9 / f64::from(ticks)),
        "median_episodes_per_second": median.filter(|_| component == "fpu_divider").map(|seconds| episodes as f64 / seconds),
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
    compiler: &mut Compiler,
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
        compiler.tick_with_world(world);
    }
    compiler.on_use_block(trigger);
    for _ in 0..600 {
        compiler.tick_with_world(world);
    }
    let mut elapsed = Duration::ZERO;
    let mut progression = Vec::new();
    for tick in 1..=ticks {
        let started = Instant::now();
        compiler.tick_with_world(world);
        if flush_every == 1 {
            compiler.flush(world);
        }
        elapsed += started.elapsed();
        if tick % 600 == 5 || tick == ticks - 1 {
            if flush_every == 0 {
                compiler.flush(world);
            }
            progression.push((600 + tick, read(world)?));
        }
    }
    let changing = progression.windows(2).all(|pair| pair[0].1 != pair[1].1);
    Ok(
        json!({"measurement_kind": "logical_counter_runtime", "activity_confirmed": changing,
        "seconds": elapsed.as_secs_f64(), "word_progression": progression}),
    )
}

fn divider(
    compiler: &mut Compiler,
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
        compiler.tick_with_world(world);
    }
    compiler.flush(world);
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
        "measurement_kind": "logical_divider_episodes", "activity_confirmed": true,
        "seconds": seconds, "episodes": episodes, "episodes_per_second": episodes as f64 / seconds,
        "game_ticks_per_episode": 256, "warmup_game_ticks": 320,
        "response_170_observed_each_off_window": true, "final_reset_0_verified": true,
        "verified_episodes": episodes, "episode_observations": words,
        "flush_policy": if flush_every == 0 { "every game tick for observation, excluded" } else { "every game tick, included" },
        "timed_operations": "on_use_block for both enable edges, 256 tick_with_world calls per episode, optional per-tick flush; timer overhead included",
        "arithmetic_validity": "saved-input response/reset protocol only; no general arithmetic proof"
    }))
}

fn divider_phase(
    compiler: &mut Compiler,
    world: &mut PlotWorld,
    trigger: BlockPos,
    flush_every: u32,
    read: &impl Fn(&PlotWorld) -> Result<u16>,
) -> Result<(Duration, bool, u16)> {
    let started = Instant::now();
    compiler.on_use_block(trigger);
    let mut elapsed = started.elapsed();
    let mut observed_response = false;
    let mut word = 0;
    for _ in 0..128 {
        let started = Instant::now();
        compiler.tick_with_world(world);
        if flush_every == 1 {
            compiler.flush(world);
        }
        elapsed += started.elapsed();
        if flush_every == 0 {
            compiler.flush(world);
        }
        word = read(world)?;
        observed_response |= word == 170;
    }
    Ok((elapsed, observed_response, word))
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
