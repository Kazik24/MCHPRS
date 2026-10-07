//! Opt-in characterization of larger author fixtures. Never an execution fallback.
use super::*;
use crate::redstone::piston::trace;
use std::collections::BTreeMap;
use std::time::Instant;

mod anpu;
mod anpu_motion_scope;

#[path = "research/fpu_divider.rs"]
mod fpu_divider;

#[path = "../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

fn origin(manifest: &Value) -> BlockPos {
    manifest.get("origin").map_or(BASE, |p| local_pos(p) - BASE)
}

fn manifest(name: &str) -> Value {
    serde_json::from_slice(
        &std::fs::read(
            root()
                .join("test_data/piston-research/fixtures")
                .join(format!("{name}.json")),
        )
        .unwrap(),
    )
    .unwrap()
}

fn load(manifest: &Value) -> (PlotWorld, (BlockPos, BlockPos)) {
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(
        manifest["dimensions"],
        json!([clipboard.size_x, clipboard.size_y, clipboard.size_z])
    );
    assert_eq!(
        manifest["loader_offset"],
        json!([clipboard.offset_x, clipboard.offset_y, clipboard.offset_z])
    );
    let mut world = empty();
    let first = origin(manifest);
    let last = first
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    let (world_first, world_last) = world.get_corners();
    assert_eq!(first.max(world_first), first, "fixture must fit the plot");
    assert_eq!(last.min(world_last), last, "fixture must fit the plot");
    paste_clipboard(
        &mut world,
        &clipboard,
        first + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    (world, (first, last))
}

fn block_state(world: &PlotWorld, pos: BlockPos, first: BlockPos) -> Value {
    let block = world.get_block(pos);
    json!({"pos": pos - first, "name": block.get_name(), "properties": block.properties(),
    "entity": world.get_block_entity(pos), "power": match block {
        Block::Piston { piston } => Some(crate::redstone::piston::should_piston_extend(world, piston.facing, pos)),
        _ => None,
    }})
}

fn observations(world: &PlotWorld, manifest: &Value) -> Value {
    let ports: BTreeMap<_, _> = manifest["observations"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, positions)| {
            (
                name,
                positions
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        block_state(
                            world,
                            local_pos(p) - BASE + origin(manifest),
                            origin(manifest),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    json!({"ports": ports, "tick": world.piston_state().logical_tick,
        "phase": world.piston_state().phase, "pending_ticks": world.scheduler().iter_entries().collect::<Vec<_>>(),
        "piston_events": world.piston_state().events, "motions": world.piston_state().motions})
}

fn cpu_words(world: &PlotWorld, fixture: &Value) -> Value {
    let words: BTreeMap<_, _> = fixture["observations"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| name.starts_with("ram_") || name.starts_with("register"))
        .map(|(name, positions)| {
            let words: Vec<Option<u16>> = positions
                .as_array()
                .unwrap()
                .chunks(12)
                .map(|bits| {
                    bits.iter().enumerate().try_fold(0, |word, (bit, p)| {
                        match world.get_block(local_pos(p) - BASE + origin(fixture)) {
                            Block::Piston { piston } => {
                                Some(word | (u16::from(!piston.extended) << bit))
                            }
                            _ => None,
                        }
                    })
                })
                .collect();
            (name, words)
        })
        .collect();
    json!(words)
}

#[test]
#[ignore = "bounded CPU protocol capture; explicit new output file required"]
fn capture_bubblesort_execution() {
    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = manifest("cpu_bubblesort");
    let selected = std::env::var("MCHPRS_PISTON_RESEARCH_CASE").unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == selected)
        .unwrap();
    let (mut world, _) = load(&fixture);
    let watched: rustc_hash::FxHashSet<_> = fixture["observations"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|p| p.as_array().unwrap())
        .map(|p| local_pos(p) - BASE + origin(&fixture))
        .collect();
    let mut samples =
        vec![json!({"label":"import", "tick":0, "words":cpu_words(&world, &fixture)})];
    for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        let count = step["advance"]
            .as_u64()
            .or_else(|| step["wait_quiet"].as_u64());
        if let Some(count) = count {
            assert!(count <= 50_000, "bounded CPU research run");
            let mut quiet = 0;
            for elapsed in 1..=count {
                let entries = trace::capture(|| world.tick_interpreted());
                let total = entries.len();
                let sha256 = format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&entries).unwrap())
                );
                let entries: Vec<_> = entries
                    .into_iter()
                    .filter(|entry| {
                        watched.contains(&match entry.operation {
                            trace::Operation::Sample { pos, .. } => pos,
                            trace::Operation::Applied(event) => event.pos,
                        })
                    })
                    .collect();
                let pending = world.scheduler().iter_entries().count();
                let motions = world.piston_state().motions.len();
                let events = world.piston_state().events.len();
                quiet = if pending == 0 && motions == 0 && events == 0 {
                    quiet + 1
                } else {
                    0
                };
                let words = cpu_words(&world, &fixture);
                samples.push(
                    json!({"label":"tick", "step":index, "tick":world.piston_state().logical_tick,
                    "words":words, "pending":pending, "motions":motions, "events":events,
                    "operation_count":total, "operation_sha256":sha256, "operations":entries}),
                );
                if elapsed % 500 == 0 {
                    println!("{selected}: step {index}, {elapsed} ticks, pending {pending}, motions {motions}");
                }
                if step.get("wait_quiet").is_some() && quiet >= 20 {
                    break;
                }
            }
            samples.push(json!({"label":"boundary", "step":index, "tick":world.piston_state().logical_tick,
                "quiet_ticks":quiet, "state":observations(&world, &fixture), "checkpoint":cpus::checkpoint(&world, world.piston_state().logical_tick as u32, &[])}));
            println!(
                "{selected}: boundary {index}, tick {}, quiet {quiet}, RAM {}",
                world.piston_state().logical_tick,
                cpu_words(&world, &fixture)["ram_y37"]
            );
        } else if step.get("diagnose").is_some() {
            samples.push(json!({"label":"diagnostics", "step":index, "tick":world.piston_state().logical_tick,
                "words":cpu_words(&world, &fixture), "diagnostics":diagnose(&world)}));
        } else {
            let entries = trace::capture(|| action(&mut world, &fixture, step));
            samples.push(
                json!({"label":"action", "step":index, "tick":world.piston_state().logical_tick,
                "action":step, "words":cpu_words(&world, &fixture), "operations":entries}),
            );
        }
    }
    if let Some(expected) = case.get("expected_final_ram_y37") {
        assert_eq!(
            &cpu_words(&world, &fixture)["ram_y37"],
            expected,
            "completed CPU sort"
        );
        assert_eq!(world.scheduler().iter_entries().count(), 0);
        assert!(world.piston_state().motions.is_empty());
        assert!(world.piston_state().events.is_empty());
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &json!({"schema_version":1, "engine":"MCHPRS interpreter",
        "fixture":fixture["fixture"], "fixture_sha256":fixture["sha256"], "origin":fixture["origin"], "case":case,
        "coordinates":"words selection-local; operations absolute; null word means moving or missing base",
        "trace_scope":"full ordered operations SHA-256 each tick; explicit ordered entries only at manifest observation positions", "samples":samples})).unwrap();
}

fn diagnose(world: &PlotWorld) -> Value {
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut reports = Vec::new();
    let before = cpus::checkpoint(world, world.piston_state().logical_tick as u32, &[]);
    for budget in [1, 2, 4, 8] {
        let start = Instant::now();
        let result = analyze(
            world,
            world.get_corners(),
            &ticks,
            &Default::default(),
            AnalysisLimits::for_budget(budget),
        );
        reports.push(match result {
            Ok(report) => json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "report": report}),
            Err(error) => json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "error": error.to_string()}),
        });
    }
    let mut attempts = Vec::new();
    for (budget, assume_instant) in [(1, false), (2, false), (4, false), (8, false), (8, true)] {
        let mut compiler = Compiler::default();
        let start = Instant::now();
        let result = compiler.compile(
            world,
            world.get_corners(),
            CompilerOptions {
                budget_multiplier: budget,
                assume_instant,
                optimize: true,
                io_only: true,
                ..Default::default()
            },
            ticks.clone(),
            Default::default(),
        );
        assert_eq!(
            cpus::checkpoint(world, world.piston_state().logical_tick as u32, &[]),
            before,
            "read-only admission must preserve physical state and work"
        );
        attempts.push(json!({"budget": budget, "assume_instant": assume_instant, "elapsed_ms": start.elapsed().as_millis(), "active": compiler.is_active(),
            "error": result.err().map(|error| error.to_string())}));
    }
    json!({"analysis": reports, "compile": attempts})
}

fn action(world: &mut PlotWorld, manifest: &Value, operation: &Value) {
    let pos = local_pos(&operation["pos"]) - BASE + origin(manifest);
    match operation["op"].as_str().unwrap() {
        "lever" => lever_action(world, pos, operation["powered"].as_bool().unwrap()),
        "button" => cpus::click(world, pos - BlockPos::new(8, 8, 8)),
        "notify" => crate::redstone::update_surrounding_blocks(world, pos),
        "sample" => {
            let block = world.get_block(pos);
            crate::redstone::update(block, world, pos, None);
        }
        _ => panic!("unsupported research action {operation}"),
    }
}

#[test]
#[ignore = "explicit new-file research capture; MCHPRS_PISTON_RESEARCH_OUTPUT required"]
fn capture_author_research_fixtures() {
    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = std::env::var("MCHPRS_PISTON_RESEARCH_FIXTURE").unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(
            root()
                .join("test_data/piston-research/fixtures")
                .join(format!("{fixture}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        !Path::new(&output).exists(),
        "capture never replaces a frozen artifact"
    );
    let selected = std::env::var("MCHPRS_PISTON_RESEARCH_CASE").ok();
    let (initial, _) = load(&manifest);
    let diagnostics = if selected.is_none() || selected.as_deref() == Some("diagnostics") {
        diagnose(&initial)
    } else {
        Value::Null
    };
    println!("{fixture}: diagnostics {}", diagnostics["compile"]);
    let mut episodes = Vec::new();
    for case in manifest["cases"].as_array().unwrap() {
        if selected.as_deref().is_some_and(|id| case["id"] != id) {
            continue;
        }
        let (mut world, _) = load(&manifest);
        let mut samples = vec![
            json!({"label": "import", "state": observations(&world, &manifest), "operations": []}),
        ];
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            if let Some(ticks) = step["advance"].as_u64() {
                assert!(ticks <= 512, "bounded research episode");
                for _ in 0..ticks {
                    let operations = trace::capture(|| world.tick_interpreted());
                    samples.push(json!({"label": "tick", "step": index, "state": observations(&world, &manifest), "operations": operations}));
                }
            } else if step.get("diagnose").is_some() {
                samples.push(json!({"label": "diagnostics", "step": index, "state": observations(&world, &manifest), "operations": [], "diagnostics": diagnose(&world)}));
            } else {
                let operations = trace::capture(|| action(&mut world, &manifest, step));
                samples.push(json!({"label": "action", "step": index, "action": step, "state": observations(&world, &manifest), "operations": operations}));
            }
        }
        println!(
            "{fixture}: {} captured {} samples",
            case["id"],
            samples.len()
        );
        episodes.push(json!({"case": case, "samples": samples}));
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &json!({"schema_version": 1, "engine": "MCHPRS interpreter",
        "fixture_sha256": manifest["sha256"], "fixture": manifest["fixture"],
        "origin": [origin(&manifest).x, origin(&manifest).y, origin(&manifest).z], "setup": "strict paste, no notifications or implicit settling",
        "coordinates": "observations selection-local; operation entries and queued work absolute",
        "diagnostics": diagnostics, "episodes": episodes})).unwrap();
}

#[test]
fn bubblesort_import_preserves_full_memory_and_button_protocol() {
    let fixture = manifest("cpu_bubblesort");
    let (mut world, bounds) = load(&fixture);
    assert_eq!(
        bounds,
        (BlockPos::new(1, 8, 1), BlockPos::new(225, 49, 254))
    );
    for row in 0..16 {
        let mut word = 0u16;
        for bit in 0..12 {
            let pos = bounds.0 + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row);
            let Block::Piston { piston } = world.get_block(pos) else {
                panic!("missing saved RAM cell at {pos}");
            };
            word |= u16::from(!piston.extended) << bit;
        }
        assert_eq!(word, 15 - row as u16);
    }
    action(
        &mut world,
        &fixture,
        &json!({"op": "button", "pos": [166,7,158]}),
    );
    let pos = bounds.0 + BlockPos::new(166, 7, 158);
    assert!(matches!(world.get_block(pos), Block::StoneButton { button } if button.powered));
    assert!(world.pending_tick_at(pos));
    advance(&mut world, 20);
    assert!(matches!(world.get_block(pos), Block::StoneButton { button } if !button.powered));
    // A second cell of margin contains all potential notification positions.
    let mut relocated = fixture;
    relocated["origin"] = json!([2, 8, 2]);
    let (world, _) = load(&relocated);
    let report = analyze_world(&world);
    assert!(report
        .ports
        .pistons
        .iter()
        .all(|p| p.outside_bounds.is_empty()));
}

#[test]
#[ignore = "large CPU ideal admission diagnostic; does not run the CPU"]
fn bubblesort_ideal_requires_explicit_sampling_contract() {
    let mut fixture = manifest("cpu_bubblesort");
    fixture["origin"] = json!([2, 8, 2]);
    let (world, _) = load(&fixture);
    let before = cpus::checkpoint(&world, 0, &[]);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                assume_instant: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("logical piston admission failed"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
}

#[test]
#[ignore = "read-only FPU expression probe, not executable admission; new output file required"]
fn capture_fpu_response_extraction() {
    let destination = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture_name =
        std::env::var("MCHPRS_PISTON_RESEARCH_FIXTURE").unwrap_or_else(|_| "fpu_legal".into());
    let fixture = manifest(&fixture_name);
    let (world, _) = load(&fixture);
    let report = analyze_world(&world);
    let mut probes = Vec::new();
    for budget in [1, 8] {
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(budget);
        let start = Instant::now();
        let result = crate::redpiler::instant::logic::extract(&world, &report, &monitor);
        let probe = match result {
            Ok(logic) => json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(),
                "sources": logic.sources.len(), "responses": logic.responses.len(), "outputs": logic.outputs.len()}),
            Err(error) => {
                json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "error": error})
            }
        };
        println!("FPU expression probe: {probe}");
        probes.push(probe);
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &json!({"schema_version": 1,
        "fixture": fixture_name, "fixture_sha256": fixture["sha256"], "scope": "expression extraction only; entry, ownership and reset admission are bypassed, no backend is activated",
        "probes": probes})).unwrap();
}

#[test]
#[ignore = "logical source delta diagnostic; explicit new output file required"]
fn capture_divider_logical_sources() {
    use crate::redpiler::instant::boolean::Variable;
    use rustc_hash::{FxHashMap, FxHashSet};

    let destination = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = manifest("fpu_divider");
    let (mut world, _) = load(&fixture);
    let first = origin(&fixture);
    let report = analyze_world(&world);
    let options = CompilerOptions {
        assume_instant: true,
        ..Default::default()
    };
    let (_, programs) = crate::redpiler::instant::program::prepare(
        &world,
        &report,
        &[],
        &options,
        Default::default(),
    )
    .unwrap();
    let mut live = FxHashSet::default();
    let mut regions = Vec::new();
    for program in &programs {
        let mut pending = program.logic.responses.clone();
        let mut visited = FxHashSet::default();
        for output in &program.logic.outputs {
            pending.extend(output.terms.iter().map(|term| term.guard));
            live.extend(output.terms.iter().filter_map(|term| term.source));
        }
        while let Some(root) = pending.pop() {
            if !visited.insert(root) {
                continue;
            }
            if let Some(decision) = program.logic.arena.decision(root) {
                if let Variable::Signal { pos, .. } = decision.variable {
                    live.insert(pos);
                }
                pending.extend([decision.low, decision.high]);
            }
        }
        regions.push(json!({"responses": program.logic.responses.len(),
            "response_sources": program.logic.response_sources, "all_sources_including_handoff": program.logic.sources}));
    }
    let torch = first + BlockPos::new(3, 22, 44);
    let owner = first + BlockPos::new(3, 21, 43);
    let support = torch.offset(BlockFace::Bottom);
    let monitor = crate::redpiler::TaskMonitor::default();
    let mobile = report
        .payload_groups
        .iter()
        .enumerate()
        .flat_map(|(id, group)| group.positions.iter().map(move |&pos| (pos, id)))
        .collect();
    let mut topology = crate::redpiler::analysis::topology::Topology::new(
        &world,
        report.bounds,
        &monitor,
        1_000_000,
        mobile,
    );
    let support_inputs = topology.signal_inputs(support, BlockFace::Top).unwrap();
    let owner_id = report
        .pistons
        .iter()
        .position(|piston| piston.pos == owner)
        .unwrap();
    let structure = json!({"torch": block_state(&world, torch, first),
        "support": block_state(&world, support, first), "owner": block_state(&world, owner, first),
        "owner_recognition": report.recognition[owner_id], "support_inputs": support_inputs});
    drop(programs);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            options,
            vec![],
            Default::default(),
        )
        .unwrap();
    let trigger = first + local_pos(&fixture["protocol"]["trigger"]) - BASE;
    let sources = |compiler: &Compiler| -> FxHashMap<BlockPos, u8> {
        compiler
            .backend
            .as_ref()
            .unwrap()
            .ordinary_sources()
            .into_iter()
            .filter(|(pos, _)| live.contains(pos))
            .collect()
    };
    let mut previous = sources(&compiler);
    let mut samples = Vec::new();
    let mut tick = 0;
    for (phase, count, toggle) in [
        ("initialize_on", 24, false),
        ("active_off", 128, true),
        ("held_off", 64, false),
        ("reset_on", 64, true),
    ] {
        if toggle {
            compiler.on_use_block(trigger);
        }
        for _ in 0..count {
            compiler.tick_with_world(&mut world);
            tick += 1;
            let current = sources(&compiler);
            let mut changes: Vec<_> = current
                .iter()
                .filter_map(|(&pos, &power)| {
                    let old = previous.get(&pos).copied();
                    (old != Some(power)).then_some((pos, old, power))
                })
                .collect();
            changes.sort_by_key(|(pos, _, _)| (pos.y, pos.z, pos.x));
            if !changes.is_empty() {
                samples.push(json!({"phase": phase, "tick": tick, "changes": changes,
                    "logical_stats": compiler.backend.as_ref().unwrap().logical_stats()}));
            }
            previous = current;
        }
        compiler.flush(&mut world);
        samples.push(json!({"phase": phase, "tick": tick, "boundary": true,
            "observations": observations(&world, &fixture),
            "logical_stats": compiler.backend.as_ref().unwrap().logical_stats()}));
    }
    let mut live: Vec<_> = live.into_iter().collect();
    live.sort_by_key(|pos| (pos.y, pos.z, pos.x));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &json!({"schema_version": 1,
        "fixture": "fpu_divider", "fixture_sha256": fixture["sha256"],
        "scope": "compiled logical response/output source deltas; handoff-only and unrelated ordinary sources excluded",
        "structure": structure, "regions": regions, "live_sources": live, "samples": samples})).unwrap();
}

fn memory_pos(address: u8, bit: u8) -> BlockPos {
    BASE + BlockPos::new(14 - 2 * i32::from(address), 9, 24 - 2 * i32::from(bit))
}

fn memory_words(world: &PlotWorld) -> [u8; 8] {
    std::array::from_fn(|address| {
        (0..8).fold(0, |word, bit| {
            let pos = memory_pos(address as u8, bit);
            let Block::Piston { piston } = world.get_block(pos) else {
                panic!("memory is moving at {pos:?}; sample a settled boundary")
            };
            word | (u8::from(!piston.extended) << bit)
        })
    })
}

fn output_word(world: &PlotWorld) -> u8 {
    (0..8).fold(0, |word, bit| {
        let pos = BASE + BlockPos::new(15, 1, 24 - 2 * bit);
        let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
            panic!("missing output at {pos:?}")
        };
        word | (u8::from(repeater.powered) << bit)
    })
}

fn advance(world: &mut PlotWorld, ticks: usize) {
    for _ in 0..ticks {
        world.tick_interpreted();
    }
}

fn data(world: &mut PlotWorld, value: u8) {
    for bit in 0..8 {
        lever_action(
            world,
            BASE + BlockPos::new(14, 14, 25 - 2 * bit),
            value & (1 << bit) != 0,
        );
    }
}

fn address(world: &mut PlotWorld, value: u8, write: bool) {
    for bit in 0..3 {
        lever_action(
            world,
            BASE + BlockPos::new(
                if write { 19 } else { 20 },
                if write { 12 } else { 7 },
                3 + 2 * bit,
            ),
            value & (1 << bit) != 0,
        );
    }
}

const WRITE_ENABLE: BlockPos = BlockPos::new(19, 12, 1);
const READ_ENABLE: BlockPos = BlockPos::new(18, 7, 1);

#[test]
fn fpu_strict_import_preserves_the_947_analog_reference_values() {
    let (world, bounds) = load(&manifest("fpu_legal"));
    let mut strengths = BTreeMap::<u8, usize>::new();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
        let block = world.get_block(pos);
        if matches!(block, Block::Furnace { .. }) {
            let strength = crate::redstone::comparator::get_override(block, &world, pos);
            *strengths.entry(strength).or_default() += 1;
        }
    });
    assert_eq!(
        strengths,
        BTreeMap::from([
            (1, 636),
            (2, 1),
            (3, 1),
            (4, 128),
            (6, 64),
            (11, 1),
            (12, 16),
            (15, 100)
        ])
    );
}

#[test]
fn fixed_fpu_compiles_with_and_without_optimization() {
    let fixture = manifest("fpu_legal");
    let (world, _) = load(&fixture);
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        optimize,
                        assume_instant,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            assert!(compiler.is_active());
            assert!(compiler.warnings().is_empty());
            if assume_instant {
                assert!(!compiler
                    .backend
                    .as_ref()
                    .unwrap()
                    .logical_stats()
                    .is_empty());
            }
        }
    }
}

#[test]
fn fpu_material_legalization_preserves_the_author_confirmed_invalid_entries() {
    use families::RecognitionFailure;
    let (world, _) = load(&manifest("fpu_legal_legacy"));
    let report = analyze_world(&world);
    let base = BASE + BlockPos::new(123, 8, 41);
    let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
    println!(
        "FPU fixed furnace actor: {:?}",
        report.recognition[actor].failures
    );
    assert!(!report.recognition[actor]
        .failures
        .iter()
        .any(|f| matches!(f, RecognitionFailure::BlockEntity { .. })));
    assert!(report.recognition.iter().all(|r| !r
        .failures
        .iter()
        .any(|f| matches!(f, RecognitionFailure::BlockEntity { .. }))));
    let failures = |predicate: fn(&RecognitionFailure) -> bool| {
        report
            .recognition
            .iter()
            .flat_map(|r| &r.failures)
            .filter(|f| predicate(f))
            .count()
    };
    assert_eq!(
        failures(|f| matches!(f, RecognitionFailure::UnsupportedPayload { .. })),
        1,
        "only the moving target still needs material behavior"
    );
    assert_eq!(
        failures(|f| matches!(f, RecognitionFailure::MismatchedHead)),
        16
    );
    assert_eq!(
        failures(|f| matches!(f, RecognitionFailure::RetractedEntry)),
        2
    );
    for p in &report.pistons {
        if matches!(
            world.get_block(p.payload),
            Block::Quartz | Block::SmoothQuartz
        ) {
            assert!(!p
                .diagnostics
                .contains(&PistonDiagnostic::UnsupportedPayload));
        }
    }
    for budget_multiplier in [1, 8] {
        let mut compiler = Compiler::default();
        let result = compiler.compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                budget_multiplier,
                optimize: true,
                io_only: true,
                ..Default::default()
            },
            vec![],
            Default::default(),
        );
        let error = result.unwrap_err().to_string();
        assert!(!error.contains("BlockEntity"), "{error}");
        assert!(!compiler.is_active());
        println!("FPU after material legalization, budget {budget_multiplier}: {error}");
    }
}

#[test]
fn rilax_material_diagnostics_reject_uncertified_logical_sampling_transactionally() {
    let (world, bounds) = load(&manifest("rilax_memory_bank_bud"));
    let report = analyze_world(&world);
    let unsupported: Vec<_> = report
        .pistons
        .iter()
        .filter(|p| {
            p.diagnostics
                .contains(&PistonDiagnostic::UnsupportedPayload)
        })
        .collect();
    assert_eq!(unsupported.len(), 56);
    assert!(unsupported
        .iter()
        .all(|p| !p.piston.sticky && world.get_block(p.payload) == Block::Air));
    let before = snapshot(&world, bounds);
    for assume_instant in [false, true] {
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    optimize: true,
                    io_only: true,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("sampling") || error.contains("generator"),
            "{error}"
        );
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

#[test]
fn rilax_memory_preserves_prepared_data_until_neighbor_movement_samples_it() {
    let fixture = manifest("rilax_memory_bank_bud");
    for selected in [0, 2, 3, 4, 5, 6, 7] {
        let (mut world, _) = load(&fixture);
        address(&mut world, selected, true);
        advance(&mut world, 24);
        let before = memory_words(&world);
        data(&mut world, 0xa5);
        advance(&mut world, 24);
        assert_eq!(
            memory_words(&world),
            before,
            "preparation at address {selected}"
        );
        assert!(!crate::redstone::piston::should_piston_extend(
            &world,
            BlockFacing::Down,
            memory_pos(selected, 0)
        ));
        let operations = trace::capture(|| {
            lever_action(&mut world, BASE + WRITE_ENABLE, true);
            advance(&mut world, 24);
        });
        let mut expected = before;
        expected[usize::from(selected)] = 0xa5;
        assert_eq!(memory_words(&world), expected);
        assert_eq!(
            output_word(&world),
            0,
            "writing alone does not expose the output"
        );
        assert!(operations.iter().any(|entry| matches!(entry.operation, trace::Operation::Applied(event) if event.pos == memory_pos(selected, 0))));
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        address(&mut world, selected, false);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + READ_ENABLE, true);
        advance(&mut world, 9);
        assert_eq!(output_word(&world), 0);
        advance(&mut world, 1);
        assert_eq!(
            output_word(&world),
            0xa5,
            "ten-tick read at address {selected}"
        );
        advance(&mut world, 14);
        lever_action(&mut world, BASE + READ_ENABLE, false);
        advance(&mut world, 11);
        assert_eq!(output_word(&world), 0xa5);
        advance(&mut world, 1);
        assert_eq!(output_word(&world), 0, "read release after twelve ticks");
        assert_eq!(
            memory_words(&world),
            expected,
            "read must preserve stored data"
        );
    }
}

#[test]
fn rilax_seven_update_pistons_sample_all_eight_bits_on_both_movement_edges() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    data(&mut world, 255);
    advance(&mut world, 24);
    assert_eq!(
        memory_words(&world)[0],
        0,
        "holding the update level does not continuously sample"
    );
    let operations = trace::capture(|| {
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
    });
    let mut samplers = 0;
    let mut cells = FxHashSet::default();
    for entry in operations {
        if let trace::Operation::Applied(event) = entry.operation {
            if !event.sticky && event.action == PistonAction::Retract {
                samplers += 1;
            }
            if event.sticky && event.pos.y == BASE.y + 9 {
                cells.insert(event.pos);
            }
        }
    }
    assert_eq!(samplers, 7);
    assert_eq!(cells.len(), 8);
    assert_eq!(
        memory_words(&world)[0],
        255,
        "the falling enable causes another sampling movement"
    );
}

#[test]
fn rilax_decoder_omission_is_a_reproducible_destructive_address_change() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    assert_eq!(world.get_block(BASE + BlockPos::new(13, 11, 2)), Block::Air);
    assert_eq!(memory_words(&world)[1], 3);
    address(&mut world, 1, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world)[1], 0);
    let Block::Lever { lever } = world.get_block(BASE + WRITE_ENABLE) else {
        unreachable!()
    };
    assert!(
        !lever.powered,
        "the missing torch admits updates while enable is off"
    );
}

#[test]
fn rilax_derived_missing_torch_probe_restores_address_one_enable_gating() {
    use mchprs_blocks::BlockDirection;
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    // A one-block diagnostic in a fresh test world, never a replacement schematic.
    let pos = BASE + BlockPos::new(13, 11, 2);
    world.set_block(
        pos,
        Block::RedstoneWallTorch {
            lit: true,
            facing: BlockDirection::South,
        },
    );
    crate::redstone::update_surrounding_blocks(&mut world, pos);
    let before = memory_words(&world);
    address(&mut world, 1, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world), before);
    data(&mut world, 0xa5);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world), before);
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    let mut expected = before;
    expected[1] = 0xa5;
    assert_eq!(memory_words(&world), expected);
    lever_action(&mut world, BASE + WRITE_ENABLE, false);
    advance(&mut world, 24);
    address(&mut world, 1, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(output_word(&world), 0xa5);
}

#[test]
fn rilax_held_read_does_not_resample_a_later_write() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    data(&mut world, 255);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world)[0], 255);
    assert_eq!(
        output_word(&world),
        0,
        "held read retains its sampled output"
    );
    lever_action(&mut world, BASE + WRITE_ENABLE, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(
        output_word(&world),
        255,
        "a new read movement exposes the new word"
    );
}

#[test]
fn rilax_short_enable_pulses_are_filtered_by_the_decoder() {
    for width in [1, 2, 4, 6, 8, 12] {
        let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
        data(&mut world, 255);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, true);
        advance(&mut world, width);
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        assert_eq!(
            memory_words(&world)[0],
            if width <= 2 { 0 } else { 255 },
            "enable width {width}"
        );
    }
}

#[test]
fn rilax_unchanged_data_keeps_memory_but_still_samples_on_each_update() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    advance(&mut world, 24);
    let before = memory_words(&world);
    let operations = trace::capture(|| {
        for _ in 0..2 {
            lever_action(&mut world, BASE + WRITE_ENABLE, true);
            advance(&mut world, 24);
            lever_action(&mut world, BASE + WRITE_ENABLE, false);
            advance(&mut world, 24);
        }
    });
    let samples = operations.iter().filter(|entry| matches!(entry.operation, trace::Operation::Sample {pos, ..} if (0..8).any(|bit| pos == memory_pos(0, bit)))).count();
    let writes = operations.iter().filter(|entry| matches!(entry.operation, trace::Operation::Applied(event) if event.sticky && event.pos.y == BASE.y + 9)).count();
    assert_eq!(memory_words(&world), before);
    assert!(
        samples >= 32,
        "both movements of both pulses sample all eight cells"
    );
    assert_eq!(writes, 0);
}

#[test]
fn rilax_memory_writes_are_reusable_with_independent_read_address() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    let mut expected = memory_words(&world);
    for (selected, value) in [(0, 0xa5), (2, 0x5a), (7, 0xff), (0, 0), (2, 0x81)] {
        address(&mut world, selected, true);
        advance(&mut world, 24);
        data(&mut world, value);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, true);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        expected[usize::from(selected)] = value;
        assert_eq!(memory_words(&world), expected);
    }
    for selected in [7, 2, 0] {
        address(&mut world, selected, false);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + READ_ENABLE, true);
        advance(&mut world, 24);
        assert_eq!(output_word(&world), expected[usize::from(selected)]);
        lever_action(&mut world, BASE + READ_ENABLE, false);
        advance(&mut world, 24);
    }
    assert_eq!(memory_words(&world), expected);
}

#[test]
fn rilax_pico_and_game_steps_agree_at_completed_tick_boundaries() {
    let fixture = manifest("rilax_memory_bank_bud");
    let (mut game, _) = load(&fixture);
    let (mut pico, _) = load(&fixture);
    for world in [&mut game, &mut pico] {
        data(world, 255);
        advance(world, 24);
        lever_action(world, BASE + WRITE_ENABLE, true);
    }
    for tick in 25..=40 {
        game.tick_interpreted();
        let mut completed = false;
        for _ in 0..20_000 {
            pico.picotick_advance(1);
            if pico.piston_state().logical_tick == tick
                && pico.piston_state().phase == AdvancePhase::BetweenTicks
            {
                completed = true;
                break;
            }
        }
        assert!(completed, "bounded pico stepping at tick {tick}");
        assert_eq!(
            observations(&pico, &fixture),
            observations(&game, &fixture),
            "aligned tick {tick}"
        );
    }
}
