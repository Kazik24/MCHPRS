//! Manifest acceptance boundary for the independent reference executor.
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::storage::Chunk;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn position(value: &Value) -> BlockPos {
    BlockPos::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

fn compare_reference_frame(
    runtime: &Runtime,
    manifest: &Value,
    origin: BlockPos,
    reference: Option<&Value>,
    phase: &str,
    tick: u64,
    reported: &mut bool,
) {
    if *reported {
        return;
    }
    let Some(frame) = reference
        .and_then(|case| case["timeline"].as_array())
        .and_then(|frames| frames.iter().find(|frame| frame["phase"] == phase && frame["tick"] == tick))
    else {
        return;
    };
    for port in ["sum_payload", "sum_repeater", "carry_repeater"] {
        let at = origin + position(&manifest["ports"]["observations"][port]);
        let observed = match runtime.project(at) {
            None => "moving",
            Some(Block::Air) => "air",
            Some(Block::RedstoneBlock) => "redstone_block",
            Some(Block::RedstoneRepeater { repeater }) if repeater.powered => "repeater_on",
            Some(Block::RedstoneRepeater { .. }) => "repeater_off",
            _ => "other",
        };
        let state = frame["states"][port].as_str().unwrap();
        let expected = if state.starts_with("minecraft:moving_piston") {
            "moving"
        } else if state.starts_with("minecraft:air") {
            "air"
        } else if state.starts_with("minecraft:redstone_block") {
            "redstone_block"
        } else if state.starts_with("minecraft:repeater") && state.contains("powered=true") {
            "repeater_on"
        } else if state.starts_with("minecraft:repeater") {
            "repeater_off"
        } else {
            "other"
        };
        if observed != expected {
            eprintln!("first visible reference divergence: {phase} tick {tick} {port} at {at:?}: Rust={observed}, reference={expected}");
            *reported = true;
            return;
        }
    }
}

#[test]
fn adder1bit_reference_manifest_arithmetic() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let inspection: Value = serde_json::from_slice(&std::fs::read(root.join(manifest["inspection"].as_str().unwrap())).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(hash, "e80092c97fc4637c313a735d38d0241b69b876f41dd2db7de571a6458369e526");
    assert_eq!(manifest["sha256"], hash);
    assert_eq!(inspection["sha256"], hash);
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!((clipboard.size_x, clipboard.size_y, clipboard.size_z), (21, 7, 7));
    let origin = position(&manifest["coordinates"]["origin"]);
    let anchor = position(&manifest["coordinates"]["paste_anchor"]);
    assert_eq!(anchor, origin + position(&manifest["coordinates"]["loader_offset"]));
    assert_eq!(anchor, origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z));
    let bounds = (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(24, 10, 10));
    let positions: Vec<_> = (bounds.0.y..=bounds.1.y)
        .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
        .collect();
    let snapshot = |world: &PlotWorld| {
        (
            positions
                .iter()
                .map(|&pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect::<Vec<_>>(),
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        )
    };
    let mut cases = 0;
    let mut regression = false;
    let mut mismatches = 0usize;
    let reference: Option<Value> = std::fs::read("E:/codex-builds/sampled-supervisor-reference-adder-20261030.json")
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    for case in manifest["cases"].as_array().unwrap().iter().filter(|case| case["inputs"].is_array()) {
        let id = case["id"].as_str().unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(&mut world, &clipboard, anchor, false);
        let before = snapshot(&world);
        let compiled = compile(
            &world,
            bounds,
            ScratchEntry {
                scheduled: world.scheduler().iter_entries().collect(),
                piston: world.piston_state().clone(),
            },
        );
        assert_eq!(before, snapshot(&world), "{id}: compilation changed host");
        let program = compiled.unwrap_or_else(|error| {
            panic!(
                "{id}: reference compilation declined: {error:?}; block={:?}",
                error.position.map(|pos| world.get_block(pos))
            )
        });
        if cases == 0 {
            eprintln!("compiled {} watched wires", program.watched_wires.len());
            for group in &program.input_arrangements {
                eprintln!(
                    "input arrangement {:?}: tokens={:?}, sources={:?}, count={}",
                    program.positions[group.target],
                    group.tokens.iter().map(|&token| program.tokens[token].position).collect::<Vec<_>>(),
                    group.conditional_sources,
                    group.inputs.len()
                );
            }
            for piston in &program.pistons {
                let component = program.positions.iter().position(|&pos| pos == piston.base).unwrap();
                let links: Vec<_> = program.incoming[component]
                    .iter()
                    .map(|&id| {
                        let link = &program.links[id];
                        (program.positions[link.source], link.attenuation)
                    })
                    .collect();
                eprintln!("piston {:?} loaded electrical links={links:?}", piston.base);
            }
            let piston = BlockPos::new(56, 32, 41);
            let source = program.positions.iter().position(|&pos| pos == BlockPos::new(54, 34, 42)).unwrap();
            let group = program
                .input_arrangements
                .iter()
                .find(|group| {
                    program.positions[group.target] == piston
                        && !group.side
                        && group.conditional_sources.contains(&super::program::ArrangementSource::Component(source))
                })
                .unwrap();
            let token = program
                .tokens
                .iter()
                .position(|token| token.position == Some(BlockPos::new(54, 33, 41)))
                .unwrap();
            for (state, at) in [None, Some(BlockPos::new(53, 33, 41)), Some(BlockPos::new(54, 33, 41))]
                .into_iter()
                .enumerate()
            {
                let mut index = 0;
                let mut stride = 1;
                for &id in &group.tokens {
                    let pos = if id == token { at } else { program.tokens[id].position };
                    let digit = pos
                        .and_then(|pos| program.tokens[id].cells.iter().position(|&cell| cell == pos))
                        .map_or(0, |cell| cell + 1);
                    index += digit * stride;
                    stride *= program.tokens[id].cells.len() + 1;
                }
                let observed = group.inputs[index]
                    .iter()
                    .find_map(|&(input, attenuation)| (input == super::program::ArrangementSource::Component(source)).then_some(attenuation));
                let expected = (state != 2).then_some(1);
                if observed != expected {
                    mismatches += 1;
                    eprintln!("piston {piston:?} stationary source arrangement {state}: observed {observed:?}, expected {expected:?}");
                }
            }
            for (index, token) in program.tokens.iter().enumerate() {
                eprintln!("token {index}: {:?} at {:?}, cells {:?}", token.block, token.position, token.cells);
                for (cell, links) in program.token_sources[index].iter().enumerate() {
                    let links: Vec<_> = links
                        .iter()
                        .map(|&(target, attenuation)| (program.positions[target], attenuation))
                        .collect();
                    eprintln!("  {:?}: {:?}", token.cells[cell], links);
                    let steps = &program.token_updates[index][cell];
                    for (arrival, recipients) in steps.iter().enumerate() {
                        eprintln!(
                            "  {:?} {} callbacks: {:?}",
                            token.cells[cell],
                            if arrival == 0 { "departure" } else { "arrival" },
                            recipients
                                .iter()
                                .map(|&component| (program.positions[component], program.initial[component]))
                                .collect::<Vec<_>>()
                        );
                    }
                    eprintln!(
                        "  {:?} cell watchers: {:?}",
                        token.cells[cell],
                        program.token_watchers[index][cell]
                            .iter()
                            .map(|&component| (program.positions[component], program.initial[component]))
                            .collect::<Vec<_>>()
                    );
                    for (arrival, steps) in program.move_steps[index][cell].iter().enumerate() {
                        eprintln!(
                            "  {:?} {} move steps: {:?}",
                            token.cells[cell],
                            if arrival == 0 { "departure" } else { "arrival" },
                            steps
                                .iter()
                                .map(|step| (
                                    program.positions[step.node],
                                    step.watcher,
                                    step.wire,
                                    step.wires.iter().map(|&node| program.positions[node]).collect::<Vec<_>>()
                                ))
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
            for (source_initial, other_initial, source_cells, other_cells, wire) in [
                (
                    BlockPos::new(47, 32, 41),
                    BlockPos::new(48, 33, 41),
                    [BlockPos::new(46, 32, 41), BlockPos::new(47, 32, 42), BlockPos::new(47, 32, 41)],
                    [BlockPos::new(47, 33, 41), BlockPos::new(48, 33, 41)],
                    BlockPos::new(48, 33, 40),
                ),
                (
                    BlockPos::new(53, 32, 41),
                    BlockPos::new(54, 33, 41),
                    [BlockPos::new(52, 32, 41), BlockPos::new(53, 32, 42), BlockPos::new(53, 32, 41)],
                    [BlockPos::new(53, 33, 41), BlockPos::new(54, 33, 41)],
                    BlockPos::new(54, 33, 42),
                ),
            ] {
                let other_block = world.get_block(other_initial);
                let nearby: Vec<_> = program
                    .tokens
                    .iter()
                    .filter(|token| {
                        token
                            .cells
                            .iter()
                            .any(|cell| cell.x.abs_diff(wire.x) <= 2 && cell.y.abs_diff(wire.y) <= 2 && cell.z.abs_diff(wire.z) <= 2)
                    })
                    .map(|token| token.position)
                    .collect();
                eprintln!("joint wire {wire:?}: nearby token candidates={nearby:?}");
                let mut table = Vec::new();
                let static_source = wire.offset(mchprs_blocks::BlockFace::Top);
                for other_state in 0..=other_cells.len() {
                    for source_state in 0..=source_cells.len() {
                        let mut probe = ScratchWorld::from_world(
                            &world,
                            bounds,
                            ScratchEntry {
                                scheduled: vec![],
                                piston: world.piston_state().clone(),
                            },
                        )
                        .unwrap();
                        probe.world_mut().set_block(source_initial, Block::Air);
                        probe.world_mut().set_block(other_initial, Block::Air);
                        if other_state > 0 {
                            probe.world_mut().set_block(other_cells[other_state - 1], other_block);
                        }
                        let token_source = (source_state > 0).then(|| source_cells[source_state - 1]);
                        if let Some(source) = token_source {
                            probe.world_mut().set_block(source, Block::RedstoneBlock);
                        }
                        let links = crate::redpiler::scratch::discover_wire_sources(probe.world(), &[wire]).unwrap();
                        let mut sources: Vec<_> = links
                            .iter()
                            .map(|link| {
                                let identity = if Some(link.source) == token_source {
                                    "token A".to_owned()
                                } else if other_state > 0 && link.source == other_cells[other_state - 1] {
                                    "token B".to_owned()
                                } else {
                                    format!("{:?}", link.source)
                                };
                                (identity, link.attenuation)
                            })
                            .collect();
                        sources.sort();
                        let source_a = links.iter().find(|link| Some(link.source) == token_source).map(|link| link.attenuation);
                        let constant = links.iter().find(|link| link.source == static_source).map(|link| link.attenuation);
                        let arrangement = source_state + 4 * other_state;
                        let mut expected_sources = vec![(format!("{static_source:?}"), 0)];
                        if [3, 7].contains(&arrangement) {
                            expected_sources.push(("token A".to_owned(), 1));
                        }
                        expected_sources.sort();
                        if sources != expected_sources || source_a != ([3, 7].contains(&arrangement)).then_some(1) || constant != Some(0) {
                            mismatches += 1;
                            eprintln!(
                                "joint wire {wire:?} arrangement {arrangement}: complete sources={sources:?}; expected token A={:?}, static={static_source:?}:0",
                                ([3, 7].contains(&arrangement)).then_some(1)
                            );
                            crate::redpiler::scratch::trace_wire_sources(probe.world(), &[wire]).unwrap();
                        }
                        probe.finish_probe().unwrap();
                        table.push(sources);
                    }
                }
                eprintln!("joint wire {wire:?}: complete Rust source maps={table:?}");
                for (index, sources) in table.iter().enumerate().skip(1) {
                    let removed: Vec<_> = table[0]
                        .iter()
                        .filter(|(source, _)| !sources.iter().any(|(key, _)| key == source))
                        .collect();
                    let added: Vec<_> = sources
                        .iter()
                        .filter(|(source, _)| !table[0].iter().any(|(key, _)| key == source))
                        .collect();
                    let changed: Vec<_> = sources
                        .iter()
                        .filter_map(|(source, attenuation)| {
                            table[0]
                                .iter()
                                .find(|(key, old)| key == source && old != attenuation)
                                .map(|(_, old)| (source, old, attenuation))
                        })
                        .collect();
                    eprintln!("joint wire {wire:?} arrangement {index}: added={added:?}, removed={removed:?}, attenuation_changes={changed:?}");
                }
                let source_id = program.tokens.iter().position(|token| token.position == Some(source_initial)).unwrap();
                let other_id = program.tokens.iter().position(|token| token.position == Some(other_initial)).unwrap();
                let group = program.input_arrangements.iter().find(|group| {
                    program.positions[group.target] == wire
                        && !group.side
                        && group.conditional_sources.contains(&super::program::ArrangementSource::Token(source_id))
                });
                if let Some(group) = group {
                    for other_state in 0..=other_cells.len() {
                        for source_state in 0..=source_cells.len() {
                            let mut index = 0;
                            let mut stride = 1;
                            for &token in &group.tokens {
                                let at = if token == source_id {
                                    source_state.checked_sub(1).map(|cell| source_cells[cell])
                                } else if token == other_id {
                                    other_state.checked_sub(1).map(|cell| other_cells[cell])
                                } else {
                                    program.tokens[token].position
                                };
                                let digit = at
                                    .and_then(|at| program.tokens[token].cells.iter().position(|&cell| cell == at))
                                    .map_or(0, |cell| cell + 1);
                                index += digit * stride;
                                stride *= program.tokens[token].cells.len() + 1;
                            }
                            let observed = group.inputs[index].iter().find_map(|&(source, attenuation)| {
                                (source == super::program::ArrangementSource::Token(source_id)).then_some(attenuation)
                            });
                            let expected = ([3, 7].contains(&(source_state + 4 * other_state))).then_some(1);
                            if observed != expected {
                                mismatches += 1;
                                eprintln!(
                                    "joint wire {wire:?} compiled arrangement {}: observed {observed:?}, expected {expected:?}",
                                    source_state + 4 * other_state
                                );
                            }
                        }
                    }
                } else {
                    mismatches += 1;
                    eprintln!("joint wire {wire:?}: no compiled arrangement table");
                }
            }
        }
        let mut runtime = program.start();
        runtime.trace_enabled = matches!(id, "prepared-0-1-0" | "prepared-0-1-1" | "prepared-1-1-0" | "prepared-1-1-1");
        let reference_case = reference
            .as_ref()
            .and_then(|value| value["cases"].as_array())
            .and_then(|cases| cases.iter().find(|item| item["id"] == id));
        let mut reported = !matches!(id, "prepared-0-1-0" | "prepared-0-1-1" | "prepared-1-1-0" | "prepared-1-1-1");
        compare_reference_frame(&runtime, &manifest, origin, reference_case, "entry", 0, &mut reported);
        let mut actions_executed = 0;
        for (action_index, action) in case["actions"].as_array().unwrap().iter().enumerate() {
            let phase = format!("action-{action_index}");
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = origin + position(&action["pos"]);
                    let source = program
                        .sources
                        .iter()
                        .position(|&index| program.positions[index] == pos)
                        .expect("manifest control represented");
                    runtime.set_input(source, action["powered"].as_bool().unwrap()).unwrap();
                }
                "wait" | "wait_ready" => {
                    for tick in 1..=action["ticks"].as_u64().unwrap() {
                        runtime.tick().unwrap();
                        compare_reference_frame(&runtime, &manifest, origin, reference_case, &phase, tick, &mut reported);
                    }
                }
                op => panic!("unsupported manifest action {op}"),
            }
            if !matches!(action["op"].as_str().unwrap(), "wait" | "wait_ready") {
                compare_reference_frame(&runtime, &manifest, origin, reference_case, &phase, 0, &mut reported);
            }
            actions_executed += 1;
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.tick().unwrap();
            compare_reference_frame(&runtime, &manifest, origin, reference_case, "response", tick, &mut reported);
            for (port, low, high, expected) in [
                ("sum_payload", 1, 3, "sum"),
                ("sum_repeater", 3, 7, "sum"),
                ("carry_repeater", 5, 9, "carry"),
            ] {
                if !(low..=high).contains(&tick) {
                    continue;
                }
                let pos = origin + position(&manifest["ports"]["observations"][port]);
                let block = runtime.project(pos);
                if block.is_none() {
                    mismatches += 1;
                    if mismatches <= 32 {
                        eprintln!("{id} {port} tick {tick}: payload is moving");
                    }
                    continue;
                }
                let block = block.unwrap();
                let value = match (port, block) {
                    ("sum_payload", Block::Air) => 1,
                    ("sum_payload", Block::RedstoneBlock) => 0,
                    (_, Block::RedstoneRepeater { repeater }) if port != "sum_payload" => u64::from(!repeater.powered),
                    _ => panic!("{id} {port} response tick {tick}: undecodable output {block:?}"),
                };
                let expected = case["expectation"][expected].as_u64().unwrap();
                if value != expected {
                    mismatches += 1;
                    if mismatches <= 32 {
                        eprintln!("{id} {port} tick {tick}: observed {value}, expected {expected}");
                    }
                }
            }
        }
        assert!(runtime.operations > 0, "{id}: no compiled instructions executed");
        eprintln!(
            "{id}: actions={}, operations={}, repeater_ticks={}, accepted_pistons={}, token_moves={}, completions={}",
            actions_executed,
            runtime.operations,
            runtime.scheduled_repeater_ticks,
            runtime.accepted_piston_actions,
            runtime.token_movements,
            runtime.motion_completions
        );
        for event in &runtime.trace_events {
            eprintln!("{id} trace {event}");
        }
        regression |= id == "prepared-0-1-0";
        cases += 1;
    }
    assert_eq!(cases, 8);
    assert!(regression, "prepared-0-1-0 regression must remain in the manifest");
    assert_eq!(mismatches, 0, "ADDER1BIT output mismatches (first 32 printed)");
}

fn run_repeated_adder(manifest_name: &str, cases: &[(u64, u64, u64)]) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("test_data/instant-pistons-io/fixtures/{manifest_name}.json"))).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let origin = position(&manifest["coordinates"]["origin"]);
    let anchor = position(&manifest["coordinates"]["paste_anchor"]);
    let width = if manifest["ports"]["inputs"]["A"][0].is_array() {
        manifest["ports"]["inputs"]["A"].as_array().unwrap().len()
    } else {
        1
    };
    let mask = (1u64 << width) - 1;
    let bounds = (
        origin - BlockPos::new(4, 4, 4),
        origin
            + if width == 1 {
                BlockPos::new(24, 10, 10)
            } else {
                BlockPos::new(27, 11, 50)
            },
    );
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(&mut world, &clipboard, anchor, false);
    let program = compile(
        &world,
        bounds,
        ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(),
            piston: world.piston_state().clone(),
        },
    )
    .unwrap_or_else(|error| panic!("{manifest_name} repeated compile declined: {error:?}"));
    let source = |relative: BlockPos| {
        let pos = origin + relative;
        program.sources.iter().position(|&node| program.positions[node] == pos).unwrap()
    };
    let trigger = source(position(&manifest["ports"]["inputs"]["trigger"]));
    let mut runtime = program.start();
    let mut elapsed_ticks = 0;
    for (operation, &(a, b, cin)) in cases.iter().cycle().take(cases.len() * 2).enumerate() {
        if operation != 0 {
            runtime.set_input(trigger, true).unwrap();
            for _ in 0..32 {
                runtime.tick().unwrap();
            }
            elapsed_ticks += 32;
            assert!(
                runtime.pending.iter_entries().next().is_none(),
                "{manifest_name} operation {operation}: rearm left pending work"
            );
        }
        for (name, value) in [("A", a), ("B", b), ("Cin", cin)] {
            if !manifest["ports"]["inputs"].get(name).is_some_and(Value::is_array) {
                continue;
            }
            for bit in 0..width {
                let port = if manifest["ports"]["inputs"][name][0].is_array() {
                    &manifest["ports"]["inputs"][name][bit]
                } else {
                    &manifest["ports"]["inputs"][name]
                };
                let input = source(position(port));
                runtime.set_input(input, value & (1 << bit) == 0).unwrap();
            }
        }
        for _ in 0..32 {
            runtime.tick().unwrap();
        }
        elapsed_ticks += 32;
        assert!(
            runtime.pending.iter_entries().next().is_none(),
            "{manifest_name} operation {operation}: preparation left pending work"
        );
        let before = runtime.operations;
        runtime.set_input(trigger, false).unwrap();
        let sum = (a + b + cin) & mask;
        let carry = (a + b + cin) >> width;
        for tick in 1..=24 {
            runtime.tick().unwrap();
            elapsed_ticks += 1;
            for (port, low, high, expected) in [("sum_payload", 1, 3, sum), ("sum_repeater", 3, 7, sum), ("carry_repeater", 5, 9, carry)] {
                let Some(outputs) = manifest["ports"]["observations"][port].as_array() else {
                    continue;
                };
                if !(low..=high).contains(&tick) {
                    continue;
                }
                let output_count = if outputs[0].is_array() { outputs.len() } else { 1 };
                let actual = (0..output_count).fold(0, |word, bit| {
                    let output = if outputs[0].is_array() {
                        &outputs[bit]
                    } else {
                        &manifest["ports"]["observations"][port]
                    };
                    let pos = origin + position(output);
                    let value = match (port, runtime.project(pos)) {
                        ("sum_payload", Some(Block::Air)) => 1,
                        ("sum_payload", Some(Block::RedstoneBlock)) => 0,
                        (_, Some(Block::RedstoneRepeater { repeater })) => u64::from(!repeater.powered),
                        (_, block) => panic!("{manifest_name} operation {operation} tick {tick} {port}: invalid output {block:?}"),
                    };
                    word | (value << bit)
                });
                assert_eq!(actual, expected, "{manifest_name} operation {operation} tick {tick} {port}");
            }
        }
        assert!(runtime.operations > before, "{manifest_name} operation {operation}: no compiled work");
    }
    assert!(elapsed_ticks > 2 * super::super::queue::NUM_QUEUES as u64);
}

#[test]
fn adder1bit_repeated_operations_match_declared_ports() {
    let cases: Vec<_> = (0u64..8).map(|input| ((input >> 2) & 1, (input >> 1) & 1, input & 1)).collect();
    run_repeated_adder("adder_1bit", &cases);
}

#[test]
fn adder11bit_repeated_carry_overflow_and_zero() {
    run_repeated_adder("adder_11bits", &[(2047, 1, 0), (1023, 1, 0), (0, 0, 0)]);
}

#[test]
fn counter_basic_reference_manifest_execution() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/counter_basic.json")).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest["sha256"].as_str().unwrap());
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let timeline = std::fs::read(root.join("test_data/instant-pistons-io/reference/counter-reference-timeline.csv")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&timeline)),
        "929a9c6a49aaf381780a3177bca97264a135019588cfe5061d08c85c8bec5f37"
    );
    let timeline = std::str::from_utf8(&timeline).unwrap();
    let origin = position(&manifest["coordinates"]["origin"]);
    let bounds = (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(27, 20, 38));
    let mut mismatches = Vec::new();
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(&mut world, &clipboard, position(&manifest["coordinates"]["paste_anchor"]), false);
        let program = compile(
            &world,
            bounds,
            ScratchEntry {
                scheduled: world.scheduler().iter_entries().collect(),
                piston: world.piston_state().clone(),
            },
        )
        .unwrap_or_else(|error| {
            panic!(
                "{id}: compilation declined: {error:?}; block={:?}",
                error.position.map(|pos| world.get_block(pos))
            )
        });
        let mut runtime = program.start();
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = origin + position(&action["pos"]);
                    let source = program.sources.iter().position(|&index| program.positions[index] == pos).unwrap();
                    runtime.set_input(source, action["powered"].as_bool().unwrap()).unwrap();
                }
                op => panic!("unsupported counter action {op}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.tick().unwrap();
            let row = timeline.lines().skip(1).find(|line| line.starts_with(&format!("{id},{tick},"))).unwrap();
            let parts: Vec<_> = row.split('"').collect();
            let fields: Vec<_> = parts[0].split(',').collect();
            let expected_dust: Vec<u8> = serde_json::from_str(parts[1]).unwrap();
            let expected_repeaters = parts[2].split(',').nth(1).unwrap();
            let observations = &manifest["ports"]["observations"];
            let memory: String = observations["memory"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| match runtime.project(origin + position(p)) {
                    None => 'M',
                    Some(Block::Piston { piston }) => {
                        if piston.extended {
                            '0'
                        } else {
                            '1'
                        }
                    }
                    _ => '?',
                })
                .collect();
            let dust: Vec<_> = observations["output_input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| match runtime.project(origin + position(p)) {
                    Some(Block::RedstoneWire { wire }) => wire.power,
                    _ => 255,
                })
                .collect();
            let repeaters: String = observations["repeater"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| match runtime.project(origin + position(p)) {
                    Some(Block::RedstoneRepeater { repeater }) => {
                        if repeater.powered {
                            '0'
                        } else {
                            '1'
                        }
                    }
                    None => 'M',
                    _ => '?',
                })
                .collect();
            if memory != fields[2] || dust != expected_dust || repeaters != expected_repeaters {
                mismatches.push(format!("{id} tick {tick}: memory {memory} expected {}, dust {dust:?} expected {expected_dust:?}, repeaters {repeaters} expected {expected_repeaters}", fields[2]));
            }
        }
        eprintln!(
            "{id}: operations={}, repeater_ticks={}, accepted_pistons={}, token_moves={}, completions={}",
            runtime.operations,
            runtime.scheduled_repeater_ticks,
            runtime.accepted_piston_actions,
            runtime.token_movements,
            runtime.motion_completions
        );
    }
    for mismatch in mismatches.iter().take(16) {
        eprintln!("{mismatch}");
    }
    assert!(
        mismatches.is_empty(),
        "COUNTER_BASIC has {} reference timeline mismatches",
        mismatches.len()
    );
}

#[test]
fn focused_test_samples_match_reference_observations() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture_dir = root.join("test_data/piston-research/test-prefix-20261008");
    let manifest: Value = serde_json::from_slice(&std::fs::read(fixture_dir.join("download-manifest.json")).unwrap()).unwrap();
    let reference = std::fs::read(root.join("test_data/instant-pistons-io/reference/test-reference-focused-replays.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&reference)),
        "bd32d2372253788f53031939d4f8de9fa34803eb2869150b576627a3c98eabfa"
    );
    let reference: Value = serde_json::from_slice(&reference).unwrap();
    let origin = BlockPos::new(40, 30, 40);
    let mut errors = Vec::new();
    for sample in reference["samples"].as_array().unwrap().iter() {
        let name = sample["name"].as_str().unwrap();
        let bytes = std::fs::read(fixture_dir.join(name)).unwrap();
        let manifest_entry = manifest["files"].as_array().unwrap().iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest_entry["sha256"].as_str().unwrap());
        let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(
            &mut world,
            &clipboard,
            origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
            false,
        );
        for control in sample["added_controls"].as_array().unwrap() {
            let pos = origin
                + BlockPos::new(
                    control["x"].as_i64().unwrap() as i32,
                    control["y"].as_i64().unwrap() as i32,
                    control["z"].as_i64().unwrap() as i32,
                );
            world.set_block(
                pos,
                Block::Lever {
                    lever: mchprs_blocks::blocks::Lever::new(LeverFace::Wall, mchprs_blocks::BlockDirection::North, false),
                },
            );
        }
        for z in 0..clipboard.size_z as i32 {
            for x in 0..clipboard.size_x as i32 {
                let pos = origin + BlockPos::new(x, 0, z);
                let below = pos.offset(mchprs_blocks::BlockFace::Bottom);
                if crate::interaction::attachment_support(world.get_block(pos), pos).is_some_and(|(support, _)| support == below)
                    && world.get_block(below) == Block::Air
                {
                    world.set_block(below, Block::Stone {});
                }
            }
        }
        let bounds = (
            origin - BlockPos::new(4, 4, 4),
            origin + BlockPos::new(clipboard.size_x as i32 + 4, clipboard.size_y as i32 + 4, clipboard.size_z as i32 + 4),
        );
        let program = match compile(
            &world,
            bounds,
            ScratchEntry {
                scheduled: world.scheduler().iter_entries().collect(),
                piston: world.piston_state().clone(),
            },
        ) {
            Ok(program) => program,
            // This full schematic also contains a connected copper bulb, which the engine declines.
            Err(error) if name == "TEST_PISTION.schem" && error.reason == "unsupported copper bulb input" => continue,
            Err(error) => {
                errors.push(format!("{name}: compilation declined: {error:?}"));
                continue;
            }
        };
        for (scenario, record) in sample["scenarios"].as_object().unwrap() {
            let mut runtime = program.start();
            for frame in record["frames"].as_array().unwrap() {
                let tick = frame["tick"].as_u64().unwrap();
                for action in record["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|action| action["tick"].as_u64() == Some(tick))
                {
                    let pos = origin + position(&action["control"]);
                    let source = program.sources.iter().position(|&node| program.positions[node] == pos).unwrap();
                    runtime.set_input(source, action["event"] == "rise").unwrap();
                }
                runtime.tick().unwrap();
                for (output, expected) in sample["outputs"].as_array().unwrap().iter().zip(frame["outputs"].as_array().unwrap()) {
                    let pos = origin
                        + BlockPos::new(
                            output["x"].as_i64().unwrap() as i32,
                            output["y"].as_i64().unwrap() as i32,
                            output["z"].as_i64().unwrap() as i32,
                        );
                    let expected = expected.as_str().unwrap();
                    let actual = runtime.project(pos);
                    let matches = match actual {
                        Some(block @ Block::RedstoneRepeater { repeater }) => {
                            expected.starts_with("minecraft:repeater[")
                                && expected.contains(&format!("powered={}", repeater.powered))
                                && expected.contains(&format!("locked={}", repeater.locked))
                                && expected.contains(&format!("delay={}", repeater.delay))
                                && expected.contains(&format!("facing={}", block.properties()["facing"]))
                        }
                        Some(block @ Block::RedstoneComparator { comparator }) => {
                            expected.starts_with("minecraft:comparator[")
                                && expected.contains(&format!("powered={}", comparator.powered))
                                && expected.contains(&format!("mode={}", block.properties()["mode"]))
                                && expected.contains(&format!("facing={}", block.properties()["facing"]))
                        }
                        Some(block) if block.is_copper_bulb() => {
                            expected.starts_with(&format!("minecraft:{}[", block.get_name()))
                                && block.properties().iter().all(|(key, value)| expected.contains(&format!("{key}={value}")))
                        }
                        _ => false,
                    };
                    if !matches {
                        errors.push(format!("{name} {scenario} tick {tick} at {pos:?}: {actual:?}, expected {expected}"));
                    }
                }
            }
        }
    }
    for error in errors.iter().take(16) {
        eprintln!("{error}");
    }
    assert!(errors.is_empty(), "{} focused TEST observations or admissions differ", errors.len());
}

#[test]
fn adder11bit_reference_manifest_acceptance() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/adder_11bits.json")).unwrap()).unwrap();
    let inspection: Value = serde_json::from_slice(&std::fs::read(root.join(manifest["inspection"].as_str().unwrap())).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(hash, "335ab94a3ba7f89cd0a497a2ac606372ff2743cfcd549f4863ead310af4138b9");
    assert_eq!(manifest["sha256"], hash);
    assert_eq!(inspection["sha256"], hash);
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!((clipboard.size_x, clipboard.size_y, clipboard.size_z), (23, 7, 46));
    let origin = position(&manifest["coordinates"]["origin"]);
    let anchor = position(&manifest["coordinates"]["paste_anchor"]);
    assert_eq!(anchor, origin + position(&manifest["coordinates"]["loader_offset"]));
    assert_eq!(anchor, origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z));
    let bounds = (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(27, 11, 50));
    let positions: Vec<_> = (bounds.0.y..=bounds.1.y)
        .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
        .collect();
    let snapshot = |world: &PlotWorld| {
        (
            positions
                .iter()
                .map(|&pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect::<Vec<_>>(),
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        )
    };
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(&mut world, &clipboard, anchor, false);
    let before = snapshot(&world);
    let compiled = compile(
        &world,
        bounds,
        ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(),
            piston: world.piston_state().clone(),
        },
    );
    assert_eq!(before, snapshot(&world), "compilation changed host");
    let mut first_program = Some(compiled.unwrap_or_else(|error| {
        panic!(
            "ADDER11BIT compilation declined: {error:?}; block={:?}",
            error.position.map(|pos| world.get_block(pos))
        )
    }));
    let mut mismatches = Vec::new();
    let mut failures = Vec::new();
    for (case_index, case) in manifest["cases"].as_array().unwrap().iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let program = if let Some(program) = first_program.take() {
            program
        } else {
            let mut fresh = PlotWorld::from_chunks(
                0,
                0,
                (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
                Default::default(),
            );
            paste_clipboard(&mut fresh, &clipboard, anchor, false);
            let before = snapshot(&fresh);
            let result = compile(
                &fresh,
                bounds,
                ScratchEntry {
                    scheduled: fresh.scheduler().iter_entries().collect(),
                    piston: fresh.piston_state().clone(),
                },
            );
            assert_eq!(before, snapshot(&fresh), "{id}: compilation changed host");
            match result {
                Ok(program) => program,
                Err(error) => {
                    failures.push(format!("{id}: compile {error:?}"));
                    continue;
                }
            }
        };
        let mut runtime = program.start();
        let mut actions = 0;
        let mut prep_ticks = 0;
        let mut error = None;
        for action in case["actions"].as_array().unwrap() {
            let result = match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = origin + position(&action["pos"]);
                    let source = program.sources.iter().position(|&node| program.positions[node] == pos).unwrap();
                    runtime.set_input(source, action["powered"].as_bool().unwrap())
                }
                "wait" | "wait_ready" => {
                    let mut result = Ok(());
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        result = runtime.tick();
                        if result.is_err() {
                            break;
                        }
                        prep_ticks += 1;
                    }
                    result
                }
                op => panic!("unsupported action {op}"),
            };
            if let Err(e) = result {
                error = Some(format!("action {actions}: {e:?}"));
                break;
            }
            actions += 1;
        }
        let expected = case["expectation"]["sum"].as_u64().map(|n| n & 2047);
        let mut frames = Vec::new();
        if error.is_none() {
            for tick in 1..=case["ticks"].as_u64().unwrap() {
                if let Err(e) = runtime.tick() {
                    error = Some(format!("response tick {tick}: {e:?}"));
                    break;
                }
                let mut frame = serde_json::Map::new();
                for port in ["sum_payload", "sum_repeater"] {
                    let mut word = Some(0u64);
                    let mut raw = Vec::new();
                    for (i, p) in manifest["ports"]["observations"][port].as_array().unwrap().iter().enumerate() {
                        let pos = origin + position(p);
                        let block = runtime.project(pos);
                        let bit = match (port, block) {
                            ("sum_payload", Some(Block::Air)) => Some(1),
                            ("sum_payload", Some(Block::RedstoneBlock)) => Some(0),
                            ("sum_repeater", Some(Block::RedstoneRepeater { repeater })) => Some(u64::from(!repeater.powered)),
                            _ => None,
                        };
                        word = word.zip(bit).map(|(w, b)| w | (b << i));
                        raw.push(format!("{block:?}"));
                    }
                    if let Some(expected) = expected
                        && ((port == "sum_payload" && (1..=3).contains(&tick)) || (port == "sum_repeater" && (3..=7).contains(&tick)))
                        && word != Some(expected)
                    {
                        let details = manifest["ports"]["observations"][port]
                            .as_array()
                            .unwrap()
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| {
                                let pos = origin + position(p);
                                let block = runtime.project(pos);
                                let bit = match (port, block) {
                                    ("sum_payload", Some(Block::Air)) => Some(1),
                                    ("sum_payload", Some(Block::RedstoneBlock)) => Some(0),
                                    ("sum_repeater", Some(Block::RedstoneRepeater { repeater })) => Some(u64::from(!repeater.powered)),
                                    _ => None,
                                };
                                (bit != Some((expected >> i) & 1)).then(|| format!("{pos:?} raw={block:?}"))
                            })
                            .collect::<Vec<_>>();
                        mismatches.push(format!(
                            "{id} response tick {tick} {port}: expected={expected} actual={word:?} cells={details:?}"
                        ));
                    }
                    frame.insert(port.to_string(), serde_json::json!({"word":word,"raw":raw}));
                }
                frames.push(serde_json::json!({"tick":tick,"ports":frame}));
            }
        }
        if let Some(e) = &error {
            failures.push(format!("{id}: {e}"));
        }
        eprintln!(
            "CASE_RESULT {}",
            serde_json::json!({"id":id,"index":case_index,"actions":actions,"preparation_ticks":prep_ticks,"response_ticks":frames.len(),"error":error,"frames":frames,"operations":runtime.operations,"repeater_ticks":runtime.scheduled_repeater_ticks,"piston_actions":runtime.accepted_piston_actions,"token_moves":runtime.token_movements,"completions":runtime.motion_completions})
        );
    }
    for mismatch in mismatches.iter().take(32) {
        eprintln!("MISMATCH {mismatch}");
    }
    for failure in &failures {
        eprintln!("FAILURE {failure}");
    }
    eprintln!("TOTAL mismatches={} runtime_failures={}", mismatches.len(), failures.len());
    assert!(mismatches.is_empty() && failures.is_empty(), "ADDER11BIT acceptance failed");
}
