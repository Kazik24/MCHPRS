//! Bounded, paired execution episodes. Publication and observation are outside execution timers.
use crate::cpu_support::*;
use anyhow::{Context, Result, bail, ensure};
use mchprs_blocks::{
    BlockFace, BlockPos,
    block_entities::BlockEntity,
    blocks::{Block, LeverFace},
};
use mchprs_core::{
    plot::{PlotWorld, worldedit::load_schematic},
    redpiler::{BackendKind, Compiler, CompilerOptions, GraphExecutionCounts, GraphExecutionCoverage},
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
    instant_wire_node_invocations: 0,
    instant_transport_invocations: 0,
    instant_transport_wire_visits: 0,
    instant_transition_hits: 0,
    instant_transition_misses: 0,
    instant_transition_programs: 0,
    instant_transition_operations: 0,
    instant_transition_wire_visits_elided: 0,
    instant_value_region_solves: 0,
    instant_value_region_cache_hits: 0,
    instant_transport_compact_captures: 0,
    instant_value_region_inert_visits: 0,
    signal_channel_invocations: 0,
    signal_channel_fallbacks: 0,
    signal_channel_operations: 0,
    stable_logic_invocations: 0,
    stable_logic_fallbacks: 0,
    stable_logic_wire_values: 0,
    stable_logic_boundary_calls: 0,
    instant_kernel_invocations: 0,
    instant_kernel_wire_walks_elided: 0,
    logical_kernel_evaluations: 0,
    logical_kernel_operations: 0,
    logical_bud_samples: 0,
    logical_bud_accepts: 0,
    logical_bud_publications: 0,
    classic_net_invocations: 0,
    classic_receiver_ops: 0,
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
    stable_inputs: bool,
    cpu: Option<Cpu>,
    manifest: Value,
    bounds: (BlockPos, BlockPos),
    controls: Vec<BlockPos>,
    electrical: Vec<BlockPos>,
}

impl Episode {
    fn new(name: &str, stable_inputs: bool) -> Result<Self> {
        ensure!(
            !stable_inputs
                || matches!(
                    name,
                    "io/adder_1bit" | "io/adder_11bits" | "io/instant_observer" | "fpu_divider" | "counter_basic"
                ),
            "stable-input measurements require a prepared adder, observer, Divider, or Counter protocol"
        );
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
            stable_inputs,
            cpu,
            manifest,
            bounds,
            controls: Vec::new(),
            electrical: Vec::new(),
        };
        if episode.generic() || episode.name == "pm1_sort" {
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
        self.cpu.is_none() && !matches!(self.name.as_str(), "fpu_divider" | "counter_basic" | "research/rilax_memory_bank_bud")
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

    fn compile(&self, world: &PlotWorld, tier: usize, compose_regions: bool) -> Result<Compiler> {
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
        if self.name == "research/rilax_memory_bank_bud" {
            let word = |positions: &[Value]| -> Result<Option<u8>> {
                let mut word = 0u8;
                for (bit, local) in positions.iter().enumerate() {
                    let position = self.bounds.0 + manifest_position(local);
                    let powered = match world.get_block(position) {
                        Block::Piston { piston } => !piston.extended,
                        Block::RedstoneRepeater { repeater } => repeater.powered,
                        Block::Air | Block::PistonHead { .. } | Block::MovingPiston { .. } => return Ok(None),
                        block => bail!("unexpected Rilax observation block {} at {position:?}", block.get_name()),
                    };
                    word |= u8::from(powered) << bit;
                }
                Ok(Some(word))
            };
            let memory = self.manifest["observations"]["memory"]
                .as_array()
                .context("missing Rilax memory probes")?;
            let mut memory: Vec<Option<u8>> = memory.chunks(8).map(word).collect::<Result<_>>()?;
            // Manifest storage probes are emitted x=0..14, while protocol addresses map 7..0.
            memory.reverse();
            let output = word(
                self.manifest["observations"]["output"]
                    .as_array()
                    .context("missing Rilax output probes")?,
            )?;
            return Ok(json!({"memory":memory,"output":output}));
        }
        if self.generic() || self.name == "pm1_sort" {
            let mut digest = Sha256::new();
            for &position in &self.electrical {
                let block = world.get_block(position);
                digest.update(block.get_id().to_le_bytes());
                if matches!(block, Block::RedstoneComparator { .. }) {
                    let strength = match world.get_block_entity(position) {
                        Some(BlockEntity::Comparator { output_strength }) => *output_strength,
                        _ => 0,
                    };
                    digest.update([strength]);
                }
            }
            let electrical = format!("{:x}", digest.finalize());
            if self.name == "io/adder_11bits" {
                let bank = self.manifest["ports"]["observations"]["sum_repeater"]
                    .as_array()
                    .context("missing adder output bank")?;
                let sum = bank.iter().enumerate().try_fold(0u16, |sum, (bit, local)| {
                    let position = self.bounds.0 + manifest_position(local);
                    let Block::RedstoneRepeater { repeater } = world.get_block(position) else {
                        bail!("missing adder output repeater at {position:?}");
                    };
                    Ok(sum | (u16::from(!repeater.powered) << bit))
                })?;
                return Ok(json!({"electrical": electrical, "sum": sum}));
            }
            return Ok(json!(electrical));
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
    json!({"instant_wire_node_invocations":value.instant_wire_node_invocations,"instant_transport_invocations":value.instant_transport_invocations,"instant_transport_wire_visits":value.instant_transport_wire_visits,"instant_value_region_solves":value.instant_value_region_solves,"instant_value_region_cache_hits":value.instant_value_region_cache_hits,"instant_transport_compact_captures":value.instant_transport_compact_captures,"instant_value_region_inert_visits":value.instant_value_region_inert_visits,"signal_channel_invocations":value.signal_channel_invocations,"signal_channel_fallbacks":value.signal_channel_fallbacks,"signal_channel_operations":value.signal_channel_operations,"instant_kernel_invocations":value.instant_kernel_invocations,"instant_kernel_wire_walks_elided":value.instant_kernel_wire_walks_elided,"logical_kernel_evaluations":value.logical_kernel_evaluations,"logical_kernel_operations":value.logical_kernel_operations,"logical_bud_samples":value.logical_bud_samples,"logical_bud_accepts":value.logical_bud_accepts,"logical_bud_publications":value.logical_bud_publications,"classic_net_invocations":value.classic_net_invocations,"classic_receiver_ops":value.classic_receiver_ops,
        "stable_logic_invocations":value.stable_logic_invocations,"stable_logic_fallbacks":value.stable_logic_fallbacks,
        "instant_transition_hits":value.instant_transition_hits,"instant_transition_misses":value.instant_transition_misses,
        "instant_transition_programs":value.instant_transition_programs,"instant_transition_operations":value.instant_transition_operations,
        "instant_transition_wire_visits_elided":value.instant_transition_wire_visits_elided,
        "stable_logic_wire_values":value.stable_logic_wire_values,"stable_logic_boundary_calls":value.stable_logic_boundary_calls,
        "ticks":value.ticks,"scheduled_ticks":value.scheduled_ticks,"callbacks":value.callbacks,
        "wire_walks":value.wire_walks,"dust_path_power_samples_elided":value.dust_path_power_samples_elided,
        "piston_samples":value.piston_samples,"piston_actions":value.piston_actions,"deferred_publications":value.deferred_publications})
}

fn window_counts(before: GraphExecutionCounts, after: GraphExecutionCounts) -> Value {
    counts(GraphExecutionCounts {
        instant_wire_node_invocations: after.instant_wire_node_invocations - before.instant_wire_node_invocations,
        instant_transport_invocations: after.instant_transport_invocations - before.instant_transport_invocations,
        instant_transport_wire_visits: after.instant_transport_wire_visits - before.instant_transport_wire_visits,
        instant_transition_hits: after.instant_transition_hits - before.instant_transition_hits,
        instant_transition_misses: after.instant_transition_misses - before.instant_transition_misses,
        instant_transition_programs: after.instant_transition_programs - before.instant_transition_programs,
        instant_transition_operations: after.instant_transition_operations - before.instant_transition_operations,
        instant_transition_wire_visits_elided: after.instant_transition_wire_visits_elided - before.instant_transition_wire_visits_elided,
        instant_value_region_solves: after.instant_value_region_solves - before.instant_value_region_solves,
        instant_value_region_cache_hits: after.instant_value_region_cache_hits - before.instant_value_region_cache_hits,
        instant_transport_compact_captures: after.instant_transport_compact_captures - before.instant_transport_compact_captures,
        instant_value_region_inert_visits: after.instant_value_region_inert_visits - before.instant_value_region_inert_visits,
        signal_channel_invocations: after.signal_channel_invocations - before.signal_channel_invocations,
        signal_channel_fallbacks: after.signal_channel_fallbacks - before.signal_channel_fallbacks,
        signal_channel_operations: after.signal_channel_operations - before.signal_channel_operations,
        stable_logic_invocations: after.stable_logic_invocations - before.stable_logic_invocations,
        stable_logic_fallbacks: after.stable_logic_fallbacks - before.stable_logic_fallbacks,
        stable_logic_wire_values: after.stable_logic_wire_values - before.stable_logic_wire_values,
        stable_logic_boundary_calls: after.stable_logic_boundary_calls - before.stable_logic_boundary_calls,
        instant_kernel_invocations: after.instant_kernel_invocations - before.instant_kernel_invocations,
        instant_kernel_wire_walks_elided: after.instant_kernel_wire_walks_elided - before.instant_kernel_wire_walks_elided,
        logical_kernel_evaluations: after.logical_kernel_evaluations - before.logical_kernel_evaluations,
        logical_kernel_operations: after.logical_kernel_operations - before.logical_kernel_operations,
        logical_bud_samples: after.logical_bud_samples - before.logical_bud_samples,
        logical_bud_accepts: after.logical_bud_accepts - before.logical_bud_accepts,
        logical_bud_publications: after.logical_bud_publications - before.logical_bud_publications,
        classic_net_invocations: after.classic_net_invocations - before.classic_net_invocations,
        classic_receiver_ops: after.classic_receiver_ops - before.classic_receiver_ops,
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

/// Include synchronous activation work without timing publication or correctness observations.
fn measure<T>(elapsed: &mut Duration, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let started = Instant::now();
    let result = operation();
    *elapsed += started.elapsed();
    result
}

fn warm_up(episode: &Episode, compiler: &mut Option<Compiler>, world: &mut PlotWorld) -> Result<(Option<BlockPos>, Duration)> {
    let mut elapsed = Duration::ZERO;
    if episode.stable_inputs && episode.generic() {
        if episode.name == "io/instant_observer" {
            let trigger = prepare_stable_observer(episode, compiler, world, &mut elapsed)?;
            return Ok((Some(trigger), elapsed));
        }
        let trigger = prepare_stable_adder(episode, compiler, world, &mut elapsed)?;
        return Ok((Some(trigger), elapsed));
    }
    let trigger = match episode.name.as_str() {
        "fpu_divider" => Some(episode.bounds.0 + manifest_position(&episode.manifest["protocol"]["trigger"])),
        "counter_basic" => Some(episode.bounds.0 + manifest_position(&episode.manifest["ports"]["inputs"]["trigger"])),
        _ => None,
    };
    if let Some(cpu) = episode.cpu {
        measure(&mut elapsed, || {
            if let Some(compiler) = compiler {
                compiler.on_use_block(cpu.origin + cpu.start);
            } else {
                click_cpu(world, cpu, cpu.start);
            }
            Ok(())
        })?;
    } else if episode.name == "counter_basic" {
        ensure!(
            matches!(world.get_block(trigger.unwrap()), Block::Lever { lever } if !lever.powered),
            "Counter must start OFF"
        );
        for _ in 0..24 {
            measure(&mut elapsed, || tick(compiler, world))?;
        }
        measure(&mut elapsed, || toggle(compiler, world, trigger.unwrap()))?;
        for _ in 0..600 {
            measure(&mut elapsed, || tick(compiler, world))?;
        }
    } else if episode.name == "fpu_divider" {
        ensure!(
            matches!(world.get_block(trigger.unwrap()), Block::Lever { lever } if lever.powered),
            "Divider must start ON"
        );
        for _ in 0..64 {
            measure(&mut elapsed, || tick(compiler, world))?;
        }
        for _ in 0..2 {
            measure(&mut elapsed, || toggle(compiler, world, trigger.unwrap()))?;
            for _ in 0..128 {
                measure(&mut elapsed, || tick(compiler, world))?;
            }
        }
    } else {
        ensure!(
            !episode.controls.is_empty(),
            "no existing lever protocol; active runtime was not measured"
        );
    }
    publish(compiler, world);
    Ok((trigger, elapsed))
}

/// Run the frozen Rilax address/write/read cases independently, with setup and observation outside the timer.
fn run_rilax(episode: &Episode, tier: Option<usize>, compose_regions: bool) -> Result<Value> {
    let cases = episode.manifest["cases"].as_array().context("missing Rilax protocol cases")?;
    let selected: Vec<_> = cases
        .iter()
        .filter(|case| {
            let id = case["id"].as_str().unwrap_or_default();
            id.starts_with("address_") && id.ends_with("_write_a5_read") && id != "address_1_write_a5_read"
        })
        .collect();
    ensure!(selected.len() == 7, "expected seven valid Rilax address write/read cases");
    let mut measured_cases = Vec::new();
    let mut seconds = 0.0;
    let mut active_ticks = 0u32;
    for case in selected {
        let id = case["id"].as_str().context("Rilax case without id")?;
        let mut world = episode.load()?;
        let mut compiler = tier.map(|tier| episode.compile(&world, tier, compose_regions)).transpose()?;
        let mut trace = vec![episode.observe(&world)?];
        let mut elapsed = Duration::ZERO;
        for step in case["steps"].as_array().context("Rilax case without steps")? {
            if let Some(ticks) = step["advance"].as_u64() {
                for _ in 0..ticks {
                    measure(&mut elapsed, || tick(&mut compiler, &mut world))?;
                    active_ticks += 1;
                    publish(&mut compiler, &mut world);
                    trace.push(episode.observe(&world)?);
                }
            } else if step["op"] == "lever" {
                let position = episode.bounds.0 + manifest_position(&step["pos"]);
                let desired = step["powered"].as_bool().context("Rilax lever action missing state")?;
                let Block::Lever { lever } = world.get_block(position) else {
                    bail!("missing Rilax protocol lever at {position:?}");
                };
                if lever.powered != desired {
                    toggle(&mut compiler, &mut world, position)?;
                }
                publish(&mut compiler, &mut world);
            } else {
                bail!("unsupported Rilax protocol action {step}");
            }
        }
        let address = case["address"].as_u64().context("missing address")? as usize;
        let data = case["data"].clone();
        let expected_memory = trace
            .last()
            .and_then(|state| state["memory"][address].as_u64())
            .context("Rilax addressed storage remained in motion at transaction end")?;
        ensure!(
            expected_memory == data.as_u64().context("Rilax data is not a byte")?,
            "Rilax {id} did not store expected byte: {expected_memory:#04x}"
        );
        ensure!(
            trace.iter().any(|state| state["memory"][address] == data && state["output"] == data),
            "Rilax {id} never exposed the written byte on the output while read-enable was exercised"
        );
        let active_final = checkpoint(&world, trace.len() as u32 - 1, &[]);
        if let Some(mut active) = compiler.take() {
            active.reset(&mut world, episode.bounds);
        }
        let reset_future = future(&world);
        let reset_checkpoint = checkpoint(&world, trace.len() as u32 - 1, &[]);
        let mut continuation = Vec::new();
        for _ in 0..32 {
            world.tick_interpreted();
            continuation.push(episode.observe(&world)?);
        }
        let continuation_future = future(&world);
        let continuation_checkpoint = checkpoint(&world, trace.len() as u32 + 31, &[]);
        seconds += elapsed.as_secs_f64();
        measured_cases.push(json!({"id":id,"address":case["address"],"data":case["data"],"active_ticks":trace.len()-1,
            "seconds":elapsed.as_secs_f64(),"trace":trace,"active_final_checkpoint":active_final,
            "reset_checkpoint":reset_checkpoint,"reset_future":reset_future,"continuation":continuation,
            "continuation_checkpoint":continuation_checkpoint,"continuation_future":continuation_future}));
    }
    Ok(
        json!({"executor":tier.map_or("interpreter".to_owned(),|tier| if compose_regions {format!("O{tier}-stable")} else {format!("O{tier}")}),
        "seconds":seconds,"active_ticks":active_ticks,"ticks_per_second":f64::from(active_ticks)/seconds,
        "timed_operations":"tick_with_world/tick_interpreted calls only; compilation, lever input, publication, observations, reset and continuation excluded",
        "cases":measured_cases}),
    )
}

/// Execute four complete arithmetic transactions from their frozen manifest protocols.
fn run_adder(episode: &Episode, tier: Option<usize>, compose_regions: bool) -> Result<Value> {
    let cases = episode.manifest["cases"].as_array().context("missing adder cases")?;
    let ids = ["prepared-0-0-0", "prepared-1-0-0", "prepared-1023-1-0", "prepared-2047-1-0"];
    let selected: Vec<_> = ids
        .iter()
        .map(|id| {
            cases
                .iter()
                .find(|case| case["id"].as_str() == Some(*id))
                .with_context(|| format!("missing manifest case {id}"))
        })
        .collect::<Result<_>>()?;
    let mut measured_cases = Vec::new();
    let mut seconds = 0.0;
    let mut active_ticks = 0u32;
    let mut sums = Vec::new();
    for case in selected {
        let id = case["id"].as_str().context("adder case without id")?;
        let mut world = episode.load()?;
        let mut compiler = tier.map(|tier| episode.compile(&world, tier, compose_regions)).transpose()?;
        let trigger = episode.bounds.0 + manifest_position(&episode.manifest["ports"]["inputs"]["trigger"]);
        let mut prepared_before_trigger = None;
        let mut elapsed = Duration::ZERO;
        let mut work_before = None;
        for action in case["actions"].as_array().context("adder case without actions")? {
            ensure!(
                prepared_before_trigger.is_none(),
                "adder manifest actions continue after the falling trigger"
            );
            match action["op"].as_str() {
                Some("lever") => {
                    let position = episode.bounds.0 + manifest_position(&action["pos"]);
                    let desired = action["powered"].as_bool().context("adder lever action missing state")?;
                    if position == trigger {
                        ensure!(!desired, "adder transaction must launch on the fixture's falling trigger");
                        ensure!(prepared_before_trigger.is_none(), "adder case contains multiple trigger actions");
                        prepared_before_trigger = Some(episode.observe(&world)?);
                        work_before = Some(compiler.as_ref().and_then(Compiler::graph_execution_counts));
                    }
                    let Block::Lever { lever } = world.get_block(position) else {
                        bail!("missing adder protocol lever at {position:?}");
                    };
                    if lever.powered != desired {
                        if position == trigger {
                            ensure!(lever.powered, "adder transaction trigger was not armed before falling edge");
                            measure(&mut elapsed, || toggle(&mut compiler, &mut world, position))?;
                        } else {
                            toggle(&mut compiler, &mut world, position)?;
                        }
                    }
                    publish(&mut compiler, &mut world);
                }
                Some("wait" | "wait_ready") => {
                    let ticks = action["ticks"].as_u64().context("adder wait without tick count")?;
                    for _ in 0..ticks {
                        tick(&mut compiler, &mut world)?;
                        publish(&mut compiler, &mut world);
                    }
                }
                operation => bail!("unsupported adder protocol operation {operation:?}"),
            }
        }
        ensure!(prepared_before_trigger.is_some(), "adder case did not contain a falling trigger action");
        let work_before = work_before.context("missing adder pre-trigger work snapshot")?;
        let mut trace = Vec::new();
        let ticks = case["ticks"].as_u64().context("adder case without response ticks")? as u32;
        ensure!(ticks > 0, "adder case must include active response ticks");
        for _ in 0..ticks {
            measure(&mut elapsed, || tick(&mut compiler, &mut world))?;
            active_ticks += 1;
            publish(&mut compiler, &mut world);
            trace.push(episode.observe(&world)?);
        }
        let work_after = compiler.as_ref().and_then(Compiler::graph_execution_counts);
        let inputs = case["inputs"].as_array().context("adder case without arithmetic inputs")?;
        let a = inputs[0].as_u64().context("adder left input missing")?;
        let b = inputs[1].as_u64().context("adder right input missing")?;
        let carry_in = inputs[2].as_u64().context("adder carry input missing")?;
        let total = a + b + carry_in;
        let expected_sum = total & 0x7ff;
        let expected_carry = total >> 11;
        ensure!(
            case["expectation"]["sum"].as_u64() == Some(expected_sum),
            "{id} manifest sum is not arithmetic"
        );
        ensure!(
            case["expectation"]["carry"].as_u64() == Some(expected_carry),
            "{id} manifest carry is not arithmetic"
        );
        ensure!(
            trace.last().is_some_and(|value| value["sum"].as_u64() == Some(expected_sum)),
            "{id} did not reach expected sum {expected_sum}"
        );
        sums.push(expected_sum);

        let active_final = checkpoint(&world, ticks, &[]);
        if let Some(mut active) = compiler.take() {
            active.reset(&mut world, episode.bounds);
        }
        let reset_future = future(&world);
        let reset_checkpoint = checkpoint(&world, ticks, &[]);
        let mut continuation = Vec::new();
        for _ in 0..32 {
            world.tick_interpreted();
            continuation.push(episode.observe(&world)?);
        }
        let continuation_future = future(&world);
        let continuation_checkpoint = checkpoint(&world, ticks + 32, &[]);
        seconds += elapsed.as_secs_f64();
        measured_cases.push(json!({"id":id,"inputs":inputs,"expectation":case["expectation"],
            "prepared_before_trigger":prepared_before_trigger,"active_ticks":ticks,"seconds":elapsed.as_secs_f64(),"trace":trace,
            "work_in_window":work_before.zip(work_after).map(|(before,after)|window_counts(before,after)),
            "active_final_checkpoint":active_final,"reset_checkpoint":reset_checkpoint,"reset_future":reset_future,
            "continuation":continuation,"continuation_checkpoint":continuation_checkpoint,"continuation_future":continuation_future}));
    }
    sums.sort_unstable();
    sums.dedup();
    ensure!(sums.len() > 1, "adder transactions did not produce changing sum outputs");
    let transaction_count = measured_cases.len();
    Ok(
        json!({"executor":tier.map_or("interpreter".to_owned(),|tier| if compose_regions {format!("O{tier}-stable")} else {format!("O{tier}")}),
        "seconds":seconds,"active_ticks":active_ticks,"ticks_per_second":f64::from(active_ticks)/seconds,
            "game_ticks_per_second":f64::from(active_ticks)/seconds,"transactions_per_second":transaction_count as f64/seconds,
            "timed_operations":"final falling-trigger action plus response tick_with_world/tick_interpreted calls; compilation, operand preparation, wait-ready ticks, publication, observations, reset and continuation excluded",
        "cases":measured_cases}),
    )
}

/// Use a pinned arithmetic case, then hold every external lever throughout the timed episode.
fn prepare_stable_adder(episode: &Episode, compiler: &mut Option<Compiler>, world: &mut PlotWorld, elapsed: &mut Duration) -> Result<BlockPos> {
    let case = episode.manifest["cases"]
        .as_array()
        .context("missing adder cases")?
        .iter()
        .find(|case| case["expectation"]["carry"] == 1)
        .context("missing carry case")?;
    for action in case["actions"].as_array().context("missing prepared actions")? {
        match action["op"].as_str() {
            Some("lever") => {
                let position = episode.bounds.0 + manifest_position(&action["pos"]);
                let Block::Lever { lever } = world.get_block(position) else {
                    bail!("missing adder control {position:?}");
                };
                if lever.powered != action["powered"].as_bool().context("missing lever value")? {
                    measure(elapsed, || toggle(compiler, world, position))?;
                }
            }
            Some("wait" | "wait_ready") => {
                for _ in 0..action["ticks"].as_u64().context("missing preparation wait")? {
                    measure(elapsed, || tick(compiler, world))?;
                }
            }
            _ => bail!("unsupported stable adder preparation {action}"),
        }
        publish(compiler, world);
    }
    Ok(episode.bounds.0 + manifest_position(&episode.manifest["ports"]["inputs"]["trigger"]))
}

/// Turn on the observer fixture's initially-off lever before timing, then keep it held.
fn prepare_stable_observer(episode: &Episode, compiler: &mut Option<Compiler>, world: &mut PlotWorld, elapsed: &mut Duration) -> Result<BlockPos> {
    let trigger = episode.bounds.0 + manifest_position(&episode.manifest["ports"]["inputs"]["trigger"]);
    ensure!(
        matches!(world.get_block(trigger), Block::Lever { lever } if !lever.powered),
        "Observer fixture must start with its trigger OFF"
    );
    measure(elapsed, || toggle(compiler, world, trigger))?;
    publish(compiler, world);
    ensure!(
        matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered),
        "Observer trigger did not remain ON after warm-up"
    );
    Ok(trigger)
}

fn episode_tick_phases(episode: &Episode) -> &'static [u32] {
    if episode.stable_inputs && episode.generic() {
        return &[512];
    }
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
    if episode.name == "io/adder_11bits" {
        let mut sums: Vec<_> = trace.iter().filter_map(|value| value["sum"].as_u64()).collect();
        sums.sort_unstable();
        sums.dedup();
        ensure!(sums.len() > 1, "adder did not produce changing sum outputs");
    }
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

fn run(episode: &Episode, tier: Option<usize>, reset_pending: bool, compose_regions: bool) -> Result<Value> {
    if episode.name == "io/adder_11bits" {
        ensure!(!reset_pending, "adder reset-pending microepisodes are not part of this protocol");
        return run_adder(episode, tier, compose_regions);
    }
    if episode.name == "research/rilax_memory_bank_bud" {
        ensure!(!reset_pending, "Rilax reset-pending microepisodes are not part of this protocol");
        return run_rilax(episode, tier, compose_regions);
    }
    let mut world = episode.load()?;
    let initial = checkpoint(&world, 0, &[]);
    let compile_started = Instant::now();
    let mut compiler = tier.map(|tier| episode.compile(&world, tier, compose_regions)).transpose()?;
    let compile_seconds = tier.map(|_| compile_started.elapsed().as_secs_f64());
    let coverage_at_compile = compiler.as_ref().and_then(Compiler::graph_execution_coverage);
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
    let preparation_work_before = compiler.as_ref().and_then(Compiler::graph_execution_counts);
    let (trigger, preparation_elapsed) = warm_up(episode, &mut compiler, &mut world)?;
    let before = compiler.as_ref().and_then(Compiler::graph_execution_counts);
    let phases = episode_tick_phases(episode);
    let active_ticks = phases.iter().sum::<u32>();
    let mut elapsed = Duration::ZERO;
    let mut phase_stimulus_elapsed = Duration::ZERO;
    let mut graph_tick_calls = 0u32;
    let mut native_tick_calls = 0u32;
    let mut trace = vec![episode.observe(&world)?];
    let mut chat = Vec::new();
    collect_chat(&world, 0, &mut chat);
    let mut reset_work = None;
    for (phase, &ticks) in phases.iter().enumerate() {
        if episode.name == "fpu_divider" {
            measure(&mut phase_stimulus_elapsed, || toggle(&mut compiler, &mut world, trigger.unwrap()))?;
        } else if episode.generic() && !episode.stable_inputs {
            let started = Instant::now();
            toggle(&mut compiler, &mut world, episode.controls[phase % episode.controls.len()])?;
            elapsed += started.elapsed();
        }
        let phase_start = trace.len();
        for _ in 0..ticks {
            let started = Instant::now();
            tick(&mut compiler, &mut world)?;
            elapsed += started.elapsed();
            // Count outside the timer; reset-pending episodes deliberately change executor.
            if compiler.is_some() {
                graph_tick_calls += 1;
            } else {
                native_tick_calls += 1;
            }
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
    let coverage_after_window = compiler.as_ref().and_then(Compiler::graph_execution_coverage);
    let execution_context_summary = execution_context_summary(coverage_after_window, graph_tick_calls, native_tick_calls);
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
        json!({"executor":tier.map_or("interpreter".to_owned(),|tier| if compose_regions { format!("O{tier}-stable") } else { format!("O{tier}") }),"seconds":elapsed.as_secs_f64(),
        "preparation_execution_seconds":preparation_elapsed.as_secs_f64(),
        "phase_stimulus_seconds":phase_stimulus_elapsed.as_secs_f64(),
        "episode_execution_seconds":(preparation_elapsed + phase_stimulus_elapsed + elapsed).as_secs_f64(),
        "episode_timed_operations":"all preparation/start operations, preparation tick calls, phase stimuli and active tick calls; loading, compilation, publication, observations, reset and continuation excluded",
        "stable_input_protocol":episode.stable_inputs,"composed_instant_regions":compose_regions,
        "timed_operations":if episode.generic() && !episode.stable_inputs {"64 round-robin lever operations and 512 tick calls; observations and projection excluded"}else{"tick calls only; setup and start stimulus excluded"},
        "compile_seconds_metadata":compile_seconds,"active_ticks":active_ticks,"initial":initial,
        "trace":trace,"chat":chat,"continuation":continuation,"active_final_checkpoint":active_final,
        "reset_checkpoint":reset_checkpoint,"reset_future":reset_future,
        "continuation_checkpoint":continuation_checkpoint,"continuation_future":continuation_future,
        "pending_scheduler_entries_after_reset":pending_after_reset,"pending_reset":reset_work,
        "work_in_preparation":preparation_work_before.zip(before).map(|(before, after)|window_counts(before, after)),
        "work_before_window":before.map(counts),"work_after_window":work.map(counts),
        "coverage_at_compile":coverage_at_compile,"coverage_after_window":coverage_after_window,
        "execution_context_summary":execution_context_summary,
        "timed_tick_contexts":{"graph":graph_tick_calls,"native":native_tick_calls},
        "work_in_window":before.zip(work).map(|(before,after)|window_counts(before,after)),"tables":tables}),
    )
}

/// Structural ownership and tick-call counts have different denominators from CPU time.
fn execution_context_summary(coverage: Option<GraphExecutionCoverage>, graph_ticks: u32, native_ticks: u32) -> Vec<String> {
    let total = graph_ticks + native_ticks;
    let percentage = |count| if total == 0 { 0.0 } else { 100.0 * f64::from(count) / f64::from(total) };
    let mut lines = vec![format!(
        "Timed tick calls: Graph {graph_ticks}/{total} ({:.1}%); native {native_ticks}/{total} ({:.1}%). This is not CPU-time share.",
        percentage(graph_ticks),
        percentage(native_ticks),
    )];
    if let Some(coverage) = coverage {
        lines.extend(coverage.summary_lines());
    } else if graph_ticks == 0 {
        lines.push("Interpreter reference execution; not a compiled fallback.".into());
    } else {
        lines.push("Explicit reset handed execution back to the interpreter during this window; no compiled graph remains at its end.".into());
    }
    lines
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
        for tier in [0, 2, 3, 4] {
            let admitted = (|| -> Result<()> {
                let world = episode.load()?;
                episode.compile(&world, tier, episode.stable_inputs && tier >= 3)?;
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
            let order = if interpreted {
                vec![None]
            } else if let Some(tier) = selected_tier {
                vec![Some(tier), None]
            } else {
                if episode.generic() {
                    vec![Some(2), Some(3), Some(4), Some(5), None]
                } else {
                    vec![Some(0), Some(2), Some(3), Some(4), Some(5), None]
                }
            };
            let mut order: Vec<_> = if episode.stable_inputs && !interpreted {
                if selected_tier == Some(4) {
                    // Compare regional replacement with the same stable-input O3 composition.
                    vec![(Some(4), false), (Some(3), true), (Some(4), true), (None, false)]
                } else {
                    vec![(selected_tier, false), (selected_tier, true), (None, false)]
                }
            } else {
                order.into_iter().map(|tier| (tier, false)).collect()
            };
            if sample % 2 != 0 {
                order.reverse();
            }
            let mut results = Vec::new();
            for (position, &(tier, compose_regions)) in order.iter().enumerate() {
                let mut result = run(&episode, tier, false, compose_regions)
                    .with_context(|| format!("{name} sample {} executor {tier:?} composed={compose_regions}", sample + 1))?;
                result["sample"] = json!(sample + 1);
                result["order"] = json!(position + 1);
                if let Some(episode_seconds) = result["episode_execution_seconds"].as_f64() {
                    println!(
                        "{name} sample {} {}: {:.6}s active; {:.6}s including preparation and activation",
                        sample + 1,
                        result["executor"],
                        result["seconds"].as_f64().unwrap(),
                        episode_seconds
                    );
                } else {
                    println!(
                        "{name} sample {} {}: {:.6}s; {} game ticks, {:.1} ticks/s",
                        sample + 1,
                        result["executor"],
                        result["seconds"].as_f64().unwrap(),
                        result["active_ticks"].as_u64().unwrap_or(0),
                        result["game_ticks_per_second"]
                            .as_f64()
                            .or(result["ticks_per_second"].as_f64())
                            .unwrap_or(0.0)
                    );
                }
                results.push(result);
            }
            let native = results.iter().find(|result| result["executor"] == "interpreter").unwrap().clone();
            for result in &mut results {
                if name == "research/rilax_memory_bank_bud" || name == "io/adder_11bits" {
                    ensure!(
                        result["cases"].as_array().unwrap().len() == native["cases"].as_array().unwrap().len(),
                        "{} case count mismatch",
                        name
                    );
                    let executor = result["executor"].clone();
                    for (actual, expected) in result["cases"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .zip(native["cases"].as_array().unwrap())
                    {
                        ensure!(actual["id"] == expected["id"], "{} manifest case order mismatch", name);
                        for field in ["trace", "continuation", "prepared_before_trigger"] {
                            ensure!(
                                actual[field] == expected[field],
                                "{} {} compiled/native mismatch: {field}",
                                name,
                                actual["id"]
                            );
                        }
                        for field in ["reset_future", "continuation_future"] {
                            if actual[field] != expected[field] {
                                let future_fields = ["logical_tick", "phase", "scheduled_ticks", "piston_events", "motions"];
                                let differences: Vec<_> = future_fields
                                    .into_iter()
                                    .filter(|key| actual[field][*key] != expected[field][*key])
                                    .map(|key| json!({"field":key,"actual":actual[field][key],"expected":expected[field][key]}))
                                    .collect();
                                let diagnostic = json!({"workload":name,"case_id":actual["id"],"executor":executor,
                                    "future_field":field,"differences":differences,"actual":actual[field],"expected":expected[field]});
                                let filename = if name == "io/adder_11bits" {
                                    "adder-future-diff.json"
                                } else {
                                    "rilax-future-diff.json"
                                };
                                let path = root().join("target/build-audit-tps").join(filename);
                                std::fs::create_dir_all(path.parent().unwrap())?;
                                std::fs::write(&path, serde_json::to_vec_pretty(&diagnostic)?)?;
                                bail!(
                                    "{} {} {} differs from interpreter; structured actual/expected future saved to {}",
                                    name,
                                    actual["id"],
                                    field,
                                    path.display()
                                );
                            }
                        }
                        let checkpoint_fields = ["tick", "blocks", "entities", "chat", "messages", "lit_lamps"];
                        for checkpoint in ["reset_checkpoint", "continuation_checkpoint"] {
                            for field in checkpoint_fields {
                                ensure!(
                                    actual[checkpoint][field] == expected[checkpoint][field],
                                    "{} {} {} checkpoint field {field} differs after handoff",
                                    name,
                                    actual["id"],
                                    checkpoint
                                );
                            }
                        }
                        let diagnostic_fields = ["tick", "blocks", "entities", "scheduler", "pistons", "chat", "messages", "lit_lamps"];
                        let active_differences: Vec<_> = diagnostic_fields
                            .into_iter()
                            .filter(|field| actual["active_final_checkpoint"][*field] != expected["active_final_checkpoint"][*field])
                            .collect();
                        let has_active_differences = !active_differences.is_empty();
                        actual["active_final_checkpoint_difference_fields"] = json!(active_differences);
                        if has_active_differences {
                            println!(
                                "{name} {} active checkpoint differences before handoff: {:?}",
                                actual["id"], active_differences
                            );
                        }
                    }
                    continue;
                }
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
            if sample == 0 && name != "research/rilax_memory_bank_bud" && name != "io/adder_11bits" {
                for &(tier, compose_regions) in &order {
                    let Some(tier) = tier else {
                        continue;
                    };
                    let reset = run(&episode, Some(tier), true, compose_regions)
                        .with_context(|| format!("{name} O{tier} composed={compose_regions} pending reset continuation"))?;
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
        for executor in [
            "O0",
            "O1",
            "O2",
            "O3",
            "O3-stable",
            "O4",
            "O4-stable",
            "O5",
            "O5-stable",
            "O6",
            "O6-stable",
            "interpreter",
        ] {
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
            let mut episode_times: Vec<_> = workload["samples"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|sample| sample["executor"] == executor)
                .map(|sample| {
                    sample["episode_execution_seconds"]
                        .as_f64()
                        .unwrap_or(sample["seconds"].as_f64().unwrap())
                })
                .collect();
            episode_times.sort_by(f64::total_cmp);
            let active_ticks = workload["samples"]
                .as_array()
                .unwrap()
                .iter()
                .find(|sample| sample["executor"] == executor)
                .and_then(|sample| sample["active_ticks"].as_u64());
            summaries.push(
                json!({"executor":executor,"median_seconds":times[times.len()/2],"range_seconds":[times[0],times[times.len()-1]],
                "episode_median_seconds":episode_times[episode_times.len()/2],
                "episode_range_seconds":[episode_times[0],episode_times[episode_times.len()-1]],
                "active_ticks":active_ticks,
                "median_ticks_per_second":active_ticks.map(|ticks| ticks as f64 / times[times.len()/2])}),
            );
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
    ensure!(
        selected_tier.is_none_or(|tier| (1..=6).contains(&tier)),
        "--graph-tier needs 1, 2, 3, 4, 5 or 6"
    );
    let interpreted = args.iter().any(|arg| arg == "--interpreted");
    let label = argument("--label").unwrap_or("current");
    let git = |args: &[&str]| -> Result<String> {
        let output = Command::new("git").args(args).current_dir(root()).output()?;
        ensure!(output.status.success(), "git metadata failed");
        Ok(String::from_utf8(output.stdout)?)
    };
    let mut report = json!({"schema":7,"label":label,"profile":if cfg!(debug_assertions) { "dev/debug" } else { "bench/release" },"admission_smoke":smoke,
        "binary_sha256":hash(&std::fs::read(std::env::current_exe()?)?),"git_head":git(&["rev-parse","HEAD"])?.trim(),
        "working_tree_diff":git(&["diff","HEAD","--"] )?,"working_tree_status":git(&["status","--short"] )?,
        "harness_source_sha256":hash(include_bytes!("graph_short.rs")),"cpu_loader_source_sha256":hash(include_bytes!("cpus.rs")),
        "visual_mode":"all; no timed visual flush; same projected block and light state", "samples":[],"workloads":[],
        "timed_operations":"tick_with_world or tick_interpreted calls only; fresh world per executor/sample; setup, compilation, stimulus, publication, reads, reset and continuation excluded"});
    let mut blockers = 0;
    for name in names.iter().filter(|name| selected.is_none_or(|selected| selected == name.as_str())) {
        let measured = Episode::new(name, false).and_then(|episode| benchmark_episode(&episode, iterations, smoke, selected_tier, interpreted));
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
    ensure!(blockers == 0, "{blockers} workloads could not complete validation; see structured report");
    Ok(())
}
