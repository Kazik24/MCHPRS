//! Opt-in characterization of larger author fixtures. Never an execution fallback.
use super::*;
use crate::redstone::piston::trace;
use std::collections::BTreeMap;
use std::time::Instant;

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
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let last = BASE
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    (world, (BASE, last))
}

fn block_state(world: &PlotWorld, pos: BlockPos) -> Value {
    let block = world.get_block(pos);
    json!({"pos": pos - BASE, "name": block.get_name(), "properties": block.properties(),
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
                    .map(|p| block_state(world, local_pos(p)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    json!({"ports": ports, "tick": world.piston_state().logical_tick,
        "phase": world.piston_state().phase, "pending_ticks": world.scheduler().iter_entries().collect::<Vec<_>>(),
        "piston_events": world.piston_state().events, "motions": world.piston_state().motions})
}

fn diagnose(world: &PlotWorld) -> Value {
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut reports = Vec::new();
    for budget in [1, 8] {
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
    for budget in [1, 8] {
        let mut compiler = Compiler::default();
        let start = Instant::now();
        let result = compiler.compile(
            world,
            world.get_corners(),
            CompilerOptions {
                budget_multiplier: budget,
                optimize: true,
                io_only: true,
                ..Default::default()
            },
            ticks.clone(),
            Default::default(),
        );
        attempts.push(json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "active": compiler.is_active(),
            "error": result.err().map(|error| error.to_string())}));
    }
    json!({"analysis": reports, "compile": attempts})
}

fn action(world: &mut PlotWorld, operation: &Value) {
    let pos = local_pos(&operation["pos"]);
    match operation["op"].as_str().unwrap() {
        "lever" => lever_action(world, pos, operation["powered"].as_bool().unwrap()),
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
            } else {
                let operations = trace::capture(|| action(&mut world, step));
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
        "origin": [BASE.x, BASE.y, BASE.z], "setup": "strict paste, no notifications or implicit settling",
        "coordinates": "observations selection-local; operation entries and queued work absolute",
        "diagnostics": diagnostics, "episodes": episodes})).unwrap();
}

#[test]
#[ignore = "read-only FPU expression probe, not executable admission; new output file required"]
fn capture_fpu_response_extraction() {
    let destination = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = manifest("fpu_legal");
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
        "fixture_sha256": fixture["sha256"], "scope": "expression extraction only; entry, ownership and reset admission are bypassed, no backend is activated",
        "probes": probes})).unwrap();
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
