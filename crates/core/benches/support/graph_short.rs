//! Bounded, paired execution episodes. Publication and observation are outside tick timers.
use crate::cpu_support::*;
use anyhow::{Context, Result, bail, ensure};
use mchprs_blocks::{
    BlockFace, BlockPos,
    blocks::{Block, LeverFace},
};
use mchprs_core::{
    plot::{PlotWorld, worldedit::load_schematic},
    redpiler::{BackendKind, Compiler, CompilerOptions, GraphExecutionCounts},
    redstone,
    world::World,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

const NAMES: [&str; 4] = ["cpu_bubblesort", "anpu_pong", "fpu_divider", "counter_basic"];
const CPU_COUNTS: GraphExecutionCounts = GraphExecutionCounts {
    ticks: 100,
    scheduled_ticks: 20_005,
    callbacks: 2_059_909,
    wire_walks: 32_505,
    dust_path_power_samples_elided: 0,
    piston_samples: 302_784,
    piston_actions: 27_884,
    deferred_publications: 27_884,
};

struct Episode {
    name: String,
    cpu: Option<Cpu>,
    manifest: Value,
    bounds: (BlockPos, BlockPos),
    controls: Vec<BlockPos>,
    electrical: Vec<BlockPos>,
}

impl Episode {
    fn new(name: &str) -> Result<Self> {
        let cpu = CPUS.iter().copied().find(|cpu| cpu.name == name);
        let manifest = if let Some(cpu) = cpu {
            let bytes = std::fs::read(root().join("test_data").join(cpu.schematic))?;
            ensure!(hash(&bytes) == cpu.sha256, "frozen CPU schematic changed");
            let clip = load_schematic(std::io::Cursor::new(bytes))?;
            json!({"fixture":format!("test_data/{}",cpu.schematic), "sha256":cpu.sha256,
                "origin":[cpu.origin.x,cpu.origin.y,cpu.origin.z], "dimensions":[clip.size_x,clip.size_y,clip.size_z]})
        } else {
            let path = match name {
                "fpu_divider" => "test_data/piston-research/fixtures/fpu_divider.json",
                "counter_basic" => "test_data/instant-pistons-io/fixtures/counter_basic.json",
                _ => "",
            };
            if path.is_empty() {
                device_manifest(name)?
            } else {
                serde_json::from_slice(&std::fs::read(root().join(path))?)?
            }
        };
        let origin = manifest_origin(&manifest);
        let bounds = (origin, origin + manifest_position(&manifest["dimensions"]) - BlockPos::new(1, 1, 1));
        let mut episode = Self {
            name: name.to_owned(),
            cpu,
            manifest,
            bounds,
            controls: Vec::new(),
            electrical: Vec::new(),
        };
        if episode.generic() {
            let world = episode.load()?;
            for y in bounds.0.y..=bounds.1.y {
                for z in bounds.0.z..=bounds.1.z {
                    for x in bounds.0.x..=bounds.1.x {
                        let position = BlockPos::new(x, y, z);
                        let block = world.get_block(position);
                        if matches!(block, Block::Lever { .. }) {
                            episode.controls.push(position);
                        }
                        if observable_electrical(block) {
                            episode.electrical.push(position);
                        }
                    }
                }
            }
        }
        Ok(episode)
    }

    fn generic(&self) -> bool {
        self.cpu.is_none() && !matches!(self.name.as_str(), "fpu_divider" | "counter_basic")
    }

    fn load(&self) -> Result<PlotWorld> {
        let mut world = if let Some(cpu) = self.cpu {
            load_cpu(cpu)
        } else {
            load_manifest(&self.manifest)?
        };
        if self.name == "cpu_bubblesort" {
            let pos = self.bounds.0 + BlockPos::new(186, 2, 184);
            ensure!(
                matches!(world.get_block(pos), Block::WallSign { .. }),
                "approved CPU normalization sign missing"
            );
            ensure!(world.set_block(pos, Block::Air), "CPU normalization did not remove sign");
        }
        world.set_screen_only(false);
        world.flush_block_changes();
        Ok(world)
    }

    fn compile(&self, world: &PlotWorld, tier: usize) -> Result<Compiler> {
        let mut compiler = Compiler::default();
        compiler
            .compile(
                world,
                self.bounds,
                CompilerOptions {
                    optimization_level: Some(tier),
                    budget_multiplier: 8,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .context("Graph admission failed")?;
        ensure!(compiler.backend_kind() == Some(BackendKind::Graph), "expected Graph backend");
        Ok(compiler)
    }

    fn observe(&self, world: &PlotWorld) -> Result<Value> {
        if self.generic() {
            let mut digest = Sha256::new();
            for &position in &self.electrical {
                digest.update(world.get_block(position).get_id().to_le_bytes());
            }
            return Ok(json!(format!("{:x}", digest.finalize())));
        }
        if self.name == "cpu_bubblesort" {
            let mut ram = Vec::new();
            collect_ram(world, self.cpu.unwrap(), 0, &mut ram);
            return Ok(json!(ram.pop().context("missing CPU RAM")?.1));
        }
        if self.name == "anpu_pong" {
            return Ok(json!(screen(world)));
        }
        let bank = if self.name == "fpu_divider" {
            &self.manifest["observations"]["output_msb_first"]
        } else {
            &self.manifest["ports"]["observations"]["repeater"]
        };
        let positions = bank.as_array().context("missing output bank")?;
        ensure!(positions.len() == if self.name == "fpu_divider" { 10 } else { 16 }, "wrong output width");
        let mut word = 0u16;
        for (bit, local) in positions.iter().enumerate() {
            let pos = self.bounds.0 + manifest_position(local);
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                bail!("missing output repeater at {pos:?}");
            };
            let active = u16::from(!repeater.powered);
            word = if self.name == "fpu_divider" {
                (word << 1) | active
            } else {
                word | (active << bit)
            };
        }
        Ok(json!(word))
    }
}

fn observable_electrical(block: Block) -> bool {
    matches!(
        block,
        Block::Lever { .. }
            | Block::StoneButton { .. }
            | Block::StonePressurePlate { .. }
            | Block::RedstoneWire { .. }
            | Block::RedstoneRepeater { .. }
            | Block::RedstoneComparator { .. }
            | Block::RedstoneTorch { .. }
            | Block::RedstoneWallTorch { .. }
            | Block::Observer { .. }
            | Block::RedstoneLamp { .. }
            | Block::Hopper { .. }
    ) || block.is_copper_bulb()
}

const DEVICE_DIRECTORIES: [(&str, &str, &str); 5] = [
    ("io", "test_data/instant-pistons-io/fixtures", "json"),
    ("instant", "test_data/instant-pistons/fixtures", "json"),
    ("research", "test_data/piston-research/fixtures", "json"),
    ("test", "test_data/piston-research/test-prefix-20261008", "schem"),
    ("graph", "test_data/graph-fixtures", "schem"),
];

fn device_names(all: bool) -> Result<Vec<String>> {
    let mut names: Vec<_> = NAMES.iter().map(|name| (*name).to_owned()).collect();
    if all {
        names.extend(CPUS.iter().map(|cpu| cpu.name.to_owned()));
        for (prefix, directory, extension) in DEVICE_DIRECTORIES {
            for entry in std::fs::read_dir(root().join(directory))? {
                let path = entry?.path();
                if path.extension().is_some_and(|value| value == extension) {
                    names.push(format!("{prefix}/{}", path.file_stem().unwrap().to_string_lossy()));
                }
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn device_manifest(name: &str) -> Result<Value> {
    let (prefix, stem) = name.split_once('/').context("unknown device")?;
    let (_, directory, extension) = DEVICE_DIRECTORIES
        .iter()
        .find(|entry| entry.0 == prefix)
        .context("unknown device family")?;
    let path = root().join(directory).join(format!("{stem}.{extension}"));
    if *extension == "json" {
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        if manifest.get("origin").is_none() && manifest["coordinates"].get("origin").is_none() {
            manifest["origin"] = json!([40, 30, 40]);
        }
        return Ok(manifest);
    }
    let bytes = std::fs::read(&path)?;
    let clipboard = load_schematic(std::io::Cursor::new(&bytes))?;
    Ok(json!({"fixture":path.strip_prefix(root())?.to_string_lossy(),"sha256":hash(&bytes),
        "origin":[40,30,40],"dimensions":[clipboard.size_x,clipboard.size_y,clipboard.size_z]}))
}

fn tick(compiler: &mut Option<Compiler>, world: &mut PlotWorld) -> Result<()> {
    if let Some(compiler) = compiler {
        compiler.tick_with_world(world)?;
    } else {
        world.tick_interpreted();
    }
    Ok(())
}

fn publish(compiler: &mut Option<Compiler>, world: &mut PlotWorld) {
    if let Some(compiler) = compiler {
        compiler.flush(world);
    }
}

fn toggle(compiler: &mut Option<Compiler>, world: &mut PlotWorld, pos: BlockPos) -> Result<()> {
    if let Some(compiler) = compiler {
        compiler.on_use_block(pos);
        return Ok(());
    }
    let Block::Lever { mut lever } = world.get_block(pos) else {
        bail!("missing trigger lever at {pos:?}");
    };
    lever.powered = !lever.powered;
    world.set_block(pos, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, pos);
    let face = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, pos.offset(face));
    Ok(())
}

fn counts(value: GraphExecutionCounts) -> Value {
    json!({"ticks":value.ticks,"scheduled_ticks":value.scheduled_ticks,"callbacks":value.callbacks,
        "wire_walks":value.wire_walks,"dust_path_power_samples_elided":value.dust_path_power_samples_elided,
        "piston_samples":value.piston_samples,"piston_actions":value.piston_actions,"deferred_publications":value.deferred_publications})
}

fn window_counts(before: GraphExecutionCounts, after: GraphExecutionCounts) -> Value {
    counts(GraphExecutionCounts {
        ticks: after.ticks - before.ticks,
        scheduled_ticks: after.scheduled_ticks - before.scheduled_ticks,
        callbacks: after.callbacks - before.callbacks,
        wire_walks: after.wire_walks - before.wire_walks,
        dust_path_power_samples_elided: after.dust_path_power_samples_elided - before.dust_path_power_samples_elided,
        piston_samples: after.piston_samples - before.piston_samples,
        piston_actions: after.piston_actions - before.piston_actions,
        deferred_publications: after.deferred_publications - before.deferred_publications,
    })
}

fn future(world: &PlotWorld) -> Value {
    let state = world.piston_state();
    json!({"logical_tick":state.logical_tick,"phase":state.phase,
        "scheduled_ticks":world.scheduler().iter_entries().collect::<Vec<_>>(),
        "piston_events":state.events,
        "motions":state.motions.iter().map(|motion|
            json!([motion.pos,motion.progress,motion.previous_progress,motion.last_tick])).collect::<Vec<_>>()})
}

fn warm_up(episode: &Episode, compiler: &mut Option<Compiler>, world: &mut PlotWorld) -> Result<Option<BlockPos>> {
    let trigger = match episode.name.as_str() {
        "fpu_divider" => Some(episode.bounds.0 + manifest_position(&episode.manifest["protocol"]["trigger"])),
        "counter_basic" => Some(episode.bounds.0 + manifest_position(&episode.manifest["ports"]["inputs"]["trigger"])),
        _ => None,
    };
    if let Some(cpu) = episode.cpu {
        if let Some(compiler) = compiler {
            compiler.on_use_block(cpu.origin + cpu.start);
        } else {
            click_cpu(world, cpu, cpu.start);
        }
    } else if episode.name == "counter_basic" {
        ensure!(
            matches!(world.get_block(trigger.unwrap()), Block::Lever { lever } if !lever.powered),
            "Counter must start OFF"
        );
        for _ in 0..24 {
            tick(compiler, world)?;
        }
        toggle(compiler, world, trigger.unwrap())?;
        for _ in 0..600 {
            tick(compiler, world)?;
        }
    } else if episode.name == "fpu_divider" {
        ensure!(
            matches!(world.get_block(trigger.unwrap()), Block::Lever { lever } if lever.powered),
            "Divider must start ON"
        );
        for _ in 0..64 {
            tick(compiler, world)?;
        }
        for _ in 0..2 {
            toggle(compiler, world, trigger.unwrap())?;
            for _ in 0..128 {
                tick(compiler, world)?;
            }
        }
    } else {
        ensure!(
            !episode.controls.is_empty(),
            "no existing lever protocol; active runtime was not measured"
        );
    }
    publish(compiler, world);
    Ok(trigger)
}

fn episode_tick_phases(episode: &Episode) -> &'static [u32] {
    match episode.name.as_str() {
        "cpu_bubblesort" => &[100],
        "anpu_pong" => &[400],
        "fpu_divider" => &[128, 128, 128, 128],
        "counter_basic" => &[1800],
        "pm1_sort" => &[100],
        _ => &[8; 64],
    }
}

fn verify_phase_response(episode: &Episode, phase: usize, trace: &[Value]) -> Result<()> {
    if episode.name == "fpu_divider" {
        if phase % 2 == 0 {
            ensure!(trace.contains(&json!(170)), "Divider OFF phase {phase} never responded 170");
        } else {
            ensure!(trace.last() == Some(&json!(0)), "Divider ON phase {phase} did not reset to zero");
        }
    }
    Ok(())
}

fn verify_workload_response(episode: &Episode, trace: &[Value]) -> Result<()> {
    if episode.name == "anpu_pong" {
        let frames: Vec<ScreenFrame> = serde_json::from_slice(&std::fs::read(root().join("test_data/cpu-references/anpu_screen.json"))?)?;
        ensure!(trace.iter().any(|value| value != &trace[0]), "ANPU screen never changed");
        for frame in frames.iter().filter(|frame| frame.tick <= 400) {
            ensure!(
                trace[frame.tick as usize] == json!(frame.pixels),
                "ANPU frozen screen mismatch at {}",
                frame.tick
            );
        }
    }
    if episode.name == "counter_basic" {
        let mut unique = trace.to_vec();
        unique.sort_by_key(Value::to_string);
        unique.dedup();
        ensure!(unique.len() > 3, "Counter did not produce multiple BUD counts");
    }
    Ok(())
}

fn run(episode: &Episode, tier: Option<usize>, reset_pending: bool) -> Result<Value> {
    let mut world = episode.load()?;
    let initial = checkpoint(&world, 0, &[]);
    let compile_started = Instant::now();
    let mut compiler = tier.map(|tier| episode.compile(&world, tier)).transpose()?;
    let compile_seconds = tier.map(|_| compile_started.elapsed().as_secs_f64());
    let tables = compiler.as_ref().map(|compiler| {
        let stats = compiler.stats().unwrap();
        json!({"backend_nodes":stats.backend_nodes,
            "baseline":stats.graph.baseline().map(|counts|json!({"nodes":counts.nodes,"links":counts.links})),
            "final":stats.graph.final_graph().map(|counts|json!({"nodes":counts.nodes,"links":counts.links})),
            "transformations":stats.graph.transformations.iter().map(|pass|json!({"name":pass.name,"enabled":pass.enabled,
                "before":{"nodes":pass.before.nodes,"links":pass.before.links},"after":{"nodes":pass.after.nodes,"links":pass.after.links},
                "seconds_metadata":pass.duration.as_secs_f64()})).collect::<Vec<_>>(),
            "preparation_tables":stats.graph.preparation_tables.iter().map(|table|
            json!({"stage":table.stage,"table":table.table,"entries":table.entries,"capacity":table.capacity,"estimated_bytes":table.estimated_bytes})).collect::<Vec<_>>()})
    });
    let trigger = warm_up(episode, &mut compiler, &mut world)?;
    let before = compiler.as_ref().and_then(Compiler::graph_execution_counts);
    let phases = episode_tick_phases(episode);
    let active_ticks = phases.iter().sum::<u32>();
    let mut elapsed = Duration::ZERO;
    let mut trace = vec![episode.observe(&world)?];
    let mut chat = Vec::new();
    collect_chat(&world, 0, &mut chat);
    let mut reset_work = None;
    for (phase, &ticks) in phases.iter().enumerate() {
        if episode.name == "fpu_divider" {
            toggle(&mut compiler, &mut world, trigger.unwrap())?;
        } else if episode.generic() {
            let started = Instant::now();
            toggle(&mut compiler, &mut world, episode.controls[phase % episode.controls.len()])?;
            elapsed += started.elapsed();
        }
        let phase_start = trace.len();
        for _ in 0..ticks {
            let started = Instant::now();
            tick(&mut compiler, &mut world)?;
            elapsed += started.elapsed();
            publish(&mut compiler, &mut world);
            trace.push(episode.observe(&world)?);
            collect_chat(&world, trace.len() as u32 - 1, &mut chat);
            if trace.len() == 2 && (reset_pending || tier.is_none()) {
                reset_work = Some(
                    json!({"projected_before_reset":trace.last(),"host_scheduler_entries_before_reset":world.scheduler().iter_entries().count()}),
                );
                if let Some(mut active) = compiler.take() {
                    active.reset(&mut world, episode.bounds);
                }
                reset_work.as_mut().unwrap()["exported_scheduler_entries"] = json!(world.scheduler().iter_entries().count());
                reset_work.as_mut().unwrap()["exported_piston_events"] = json!(world.piston_state().events.len());
                reset_work.as_mut().unwrap()["exported_piston_motions"] = json!(world.piston_state().motions.len());
                reset_work.as_mut().unwrap()["future"] = future(&world);
                reset_work.as_mut().unwrap()["checkpoint"] = json!(checkpoint(&world, 1, &chat));
            }
        }
        verify_phase_response(episode, phase, &trace[phase_start..])?;
    }
    let work = compiler.as_ref().and_then(Compiler::graph_execution_counts);
    if episode.name == "cpu_bubblesort" && tier == Some(0) && !reset_pending {
        ensure!(work == Some(CPU_COUNTS), "O0 CPU frozen work changed: {work:?}");
    }
    verify_workload_response(episode, &trace)?;
    let active_final = checkpoint(&world, active_ticks, &chat);
    // Reset exports pending typed work; a native continuation checks its future effects.
    if let Some(mut active) = compiler.take() {
        active.reset(&mut world, episode.bounds);
    }
    let pending_after_reset = world.scheduler().iter_entries().count();
    let reset_checkpoint = checkpoint(&world, active_ticks, &chat);
    let reset_future = future(&world);
    let mut continuation = Vec::new();
    for _ in 0..32 {
        world.tick_interpreted();
        continuation.push(episode.observe(&world)?);
    }
    let continuation_checkpoint = checkpoint(&world, active_ticks + 32, &chat);
    let continuation_future = future(&world);
    Ok(
        json!({"executor":tier.map_or("interpreter".to_owned(),|tier|format!("O{tier}")),"seconds":elapsed.as_secs_f64(),
        "timed_operations":if episode.generic(){"64 round-robin lever operations and 512 tick calls; observations and projection excluded"}else{"tick calls only; setup and start stimulus excluded"},
        "compile_seconds_metadata":compile_seconds,"active_ticks":active_ticks,"initial":initial,
        "trace":trace,"chat":chat,"continuation":continuation,"active_final_checkpoint":active_final,
        "reset_checkpoint":reset_checkpoint,"reset_future":reset_future,
        "continuation_checkpoint":continuation_checkpoint,"continuation_future":continuation_future,
        "pending_scheduler_entries_after_reset":pending_after_reset,"pending_reset":reset_work,
        "work_before_window":before.map(counts),"work_after_window":work.map(counts),
        "work_in_window":before.zip(work).map(|(before,after)|window_counts(before,after)),"tables":tables}),
    )
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn benchmark_episode(episode: &Episode, iterations: usize, smoke: bool, selected_tier: Option<usize>, interpreted: bool) -> Result<(Value, usize)> {
    let name = episode.name.as_str();
    let mut blockers = 0;
    let mut workload = json!({"name":name,"manifest":episode.manifest,"normalization":if name=="cpu_bubblesort"{json!({"removed_wall_sign":[188,10,186]})}else{Value::Null},"samples":[],"pending_reset_validation":[]});
    if smoke {
        for tier in [0, 2, 3] {
            let admitted = (|| -> Result<()> {
                let world = episode.load()?;
                episode.compile(&world, tier)?;
                Ok(())
            })();
            let admission = match admitted {
                Ok(()) => json!({"executor":format!("O{tier}"),"admitted":true}),
                Err(error) => {
                    blockers += 1;
                    json!({"executor":format!("O{tier}"),"admitted":false,"error":format!("{error:#}")})
                }
            };
            println!("{name}: {admission}");
            workload["samples"].as_array_mut().unwrap().push(admission);
        }
    } else {
        for sample in 0..iterations {
            let mut order = if interpreted {
                vec![None]
            } else if let Some(tier) = selected_tier {
                vec![Some(tier), None]
            } else {
                if episode.generic() {
                    vec![Some(2), Some(3), None]
                } else {
                    vec![Some(0), Some(2), Some(3), None]
                }
            };
            if sample % 2 != 0 {
                order.reverse();
            }
            let mut results = Vec::new();
            for (position, &tier) in order.iter().enumerate() {
                let mut result = run(&episode, tier, false).with_context(|| format!("{name} sample {} executor {tier:?}", sample + 1))?;
                result["sample"] = json!(sample + 1);
                result["order"] = json!(position + 1);
                println!(
                    "{name} sample {} {}: {:.6}s",
                    sample + 1,
                    result["executor"],
                    result["seconds"].as_f64().unwrap()
                );
                results.push(result);
            }
            let native = results.iter().find(|result| result["executor"] == "interpreter").unwrap();
            for result in &results {
                for field in ["initial", "trace", "chat", "continuation", "reset_future", "continuation_future"] {
                    ensure!(
                        result[field] == native[field],
                        "{name} sample {} {} mismatch: {field}",
                        sample + 1,
                        result["executor"]
                    );
                }
                // Active Graph display hides moving contacts; reset materializes the native future.
                for checkpoint in ["reset_checkpoint", "continuation_checkpoint"] {
                    for field in ["blocks", "entities", "chat"] {
                        ensure!(
                            result[checkpoint][field] == native[checkpoint][field],
                            "{name} sample {} {} {checkpoint} {field} mismatch",
                            sample + 1,
                            result["executor"]
                        );
                    }
                }
            }
            if sample == 0 {
                for tier in order.iter().filter_map(|tier| *tier) {
                    let reset = run(&episode, Some(tier), true).with_context(|| format!("{name} O{tier} pending reset continuation"))?;
                    ensure!(
                        ["trace", "chat", "continuation", "reset_future", "continuation_future"]
                            .iter()
                            .all(|field| reset[field] == native[field]),
                        "{name} O{tier} pending reset changed future outputs"
                    );
                    ensure!(
                        reset["pending_reset"]["future"] == native["pending_reset"]["future"],
                        "{name} O{tier} first-tick reset future differs from native"
                    );
                    for field in ["blocks", "entities", "chat"] {
                        ensure!(
                            reset["pending_reset"]["checkpoint"][field] == native["pending_reset"]["checkpoint"][field],
                            "{name} O{tier} first-tick reset {field} differs from native"
                        );
                    }
                    workload["pending_reset_validation"].as_array_mut().unwrap().push(reset);
                }
            }
            workload["samples"].as_array_mut().unwrap().extend(results);
        }
        let mut summaries = Vec::new();
        for executor in ["O0", "O1", "O2", "O3", "interpreter"] {
            let mut times: Vec<_> = workload["samples"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|sample| sample["executor"] == executor)
                .map(|sample| sample["seconds"].as_f64().unwrap())
                .collect();
            if times.is_empty() {
                continue;
            }
            times.sort_by(f64::total_cmp);
            summaries.push(json!({"executor":executor,"median_seconds":times[times.len()/2],"range_seconds":[times[0],times[times.len()-1]]}));
        }
        workload["summaries"] = json!(summaries);
    }
    Ok((workload, blockers))
}

pub fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let argument = |name: &str| args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str());
    let smoke = args.iter().any(|arg| arg == "--admission-smoke");
    let selected = argument("--cpu").or(argument("--component"));
    let names = device_names(args.iter().any(|arg| arg == "--all-devices"))?;
    ensure!(
        selected.is_none_or(|name| names.iter().any(|candidate| candidate == name)),
        "unknown bounded workload; use --all-devices for fixture families"
    );
    let iterations = argument("--iterations").unwrap_or("5").parse::<usize>()?;
    ensure!(iterations > 0, "--iterations must be positive");
    let selected_tier = argument("--graph-tier").map(str::parse::<usize>).transpose()?;
    ensure!(selected_tier.is_none_or(|tier| matches!(tier, 1 | 2 | 3)), "--graph-tier needs 1, 2 or 3");
    let interpreted = args.iter().any(|arg| arg == "--interpreted");
    let label = argument("--label").unwrap_or("current");
    let git = |args: &[&str]| -> Result<String> {
        let output = Command::new("git").args(args).current_dir(root()).output()?;
        ensure!(output.status.success(), "git metadata failed");
        Ok(String::from_utf8(output.stdout)?)
    };
    let mut report = json!({"schema":5,"label":label,"profile":"bench/release","admission_smoke":smoke,
        "binary_sha256":hash(&std::fs::read(std::env::current_exe()?)?),"git_head":git(&["rev-parse","HEAD"])?.trim(),
        "working_tree_diff":git(&["diff","HEAD","--"] )?,"working_tree_status":git(&["status","--short"] )?,
        "harness_source_sha256":hash(include_bytes!("graph_short.rs")),"cpu_loader_source_sha256":hash(include_bytes!("cpus.rs")),
        "visual_mode":"all; no timed visual flush; same projected block and light state", "samples":[],"workloads":[],
        "timed_operations":"tick_with_world or tick_interpreted calls only; fresh world per executor/sample; setup, compilation, stimulus, publication, reads, reset and continuation excluded"});
    let mut blockers = 0;
    for name in names.iter().filter(|name| selected.is_none_or(|selected| selected == name.as_str())) {
        let measured = Episode::new(name).and_then(|episode| benchmark_episode(&episode, iterations, smoke, selected_tier, interpreted));
        let workload = match measured {
            Ok((workload, rejected)) => {
                blockers += rejected;
                workload
            }
            Err(error) => {
                blockers += 1;
                println!("{name}: NOT MEASURED: {error:#}");
                json!({"name":name,"status":"not_measured","error":format!("{error:#}"),"samples":[]})
            }
        };
        report["workloads"].as_array_mut().unwrap().push(workload);
    }
    report["admission_blockers"] = json!(blockers);
    let output = root().join(argument("--output").unwrap_or(if smoke {
        "target/graph-workloads-admission.json"
    } else {
        "target/graph-workloads.json"
    }));
    std::fs::create_dir_all(output.parent().context("output needs parent")?)?;
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("report: {}", output.display());
    ensure!(blockers == 0, "{blockers} Graph admissions failed; see structured report");
    Ok(())
}
