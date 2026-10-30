//! Manifest acceptance boundary for the independent MAIR executor.
use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::world::storage::Chunk;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn position(value: &Value) -> BlockPos {
    BlockPos::new(value[0].as_i64().unwrap() as i32, value[1].as_i64().unwrap() as i32, value[2].as_i64().unwrap() as i32)
}

fn compare_reference_frame(runtime: &Runtime<'_>, manifest: &Value, origin: BlockPos, reference: Option<&Value>, phase: &str, tick: u64, reported: &mut bool) {
    if *reported { return; }
    let Some(frame) = reference.and_then(|case| case["timeline"].as_array()).and_then(|frames| frames.iter().find(|frame| frame["phase"] == phase && frame["tick"] == tick)) else { return; };
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
        let expected = if state.starts_with("minecraft:moving_piston") { "moving" }
            else if state.starts_with("minecraft:air") { "air" }
            else if state.starts_with("minecraft:redstone_block") { "redstone_block" }
            else if state.starts_with("minecraft:repeater") && state.contains("powered=true") { "repeater_on" }
            else if state.starts_with("minecraft:repeater") { "repeater_off" }
            else { "other" };
        if observed != expected {
            eprintln!("first visible reference divergence: {phase} tick {tick} {port} at {at:?}: Rust={observed}, MAIR={expected}");
            *reported = true;
            return;
        }
    }
}

#[test]
fn adder1bit_mair_manifest_arithmetic() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
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
    let positions: Vec<_> = (bounds.0.y..=bounds.1.y).flat_map(|y| (bounds.0.z..=bounds.1.z)
        .flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z)))).collect();
    let snapshot = |world: &PlotWorld| {
        (positions.iter().map(|&pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap())).collect::<Vec<_>>(),
         world.scheduler().iter_entries().collect::<Vec<_>>(), serde_json::to_value(world.piston_state()).unwrap())
    };
    let mut cases = 0;
    let mut regression = false;
    let mut mismatches = 0usize;
    let reference: Option<Value> = std::fs::read("E:/codex-builds/sampled-supervisor-mair-adder-20261030.json").ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    for case in manifest["cases"].as_array().unwrap().iter().filter(|case| case["inputs"].is_array()) {
        let id = case["id"].as_str().unwrap();
        let mut world = PlotWorld::from_chunks(0, 0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(), Default::default());
        paste_clipboard(&mut world, &clipboard, anchor, false);
        let before = snapshot(&world);
        let compiled = compile(&world, bounds, ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(), piston: world.piston_state().clone(),
        });
        assert_eq!(before, snapshot(&world), "{id}: compilation changed host");
        let program = compiled.unwrap_or_else(|error| panic!("{id}: MAIR compilation declined: {error:?}; block={:?}", error.position.map(|pos| world.get_block(pos))));
        if cases == 0 {
            eprintln!("compiled {} watched wires", program.watched_wires.len());
            for group in &program.input_arrangements {
                eprintln!("input arrangement {:?}: tokens={:?}, sources={:?}, count={}", program.positions[group.target], group.tokens.iter().map(|&token| program.tokens[token].position).collect::<Vec<_>>(), group.conditional_sources, group.inputs.len());
            }
            for piston in &program.pistons {
                let component = program.positions.iter().position(|&pos| pos == piston.base).unwrap();
                let links: Vec<_> = program.incoming[component].iter().map(|&id| {
                    let link = &program.links[id];
                    (program.positions[link.source], link.attenuation)
                }).collect();
                eprintln!("piston {:?} loaded electrical links={links:?}", piston.base);
            }
            let piston = BlockPos::new(56, 32, 41);
            let source = program.positions.iter().position(|&pos| pos == BlockPos::new(54, 34, 42)).unwrap();
            let group = program.input_arrangements.iter().find(|group| program.positions[group.target] == piston).unwrap();
            let token = program.tokens.iter().position(|token| token.position == Some(BlockPos::new(54, 33, 41))).unwrap();
            for (state, at) in [None, Some(BlockPos::new(53, 33, 41)), Some(BlockPos::new(54, 33, 41))].into_iter().enumerate() {
                let mut index = 0;
                let mut stride = 1;
                for &id in &group.tokens {
                    let pos = if id == token { at } else { program.tokens[id].position };
                    let digit = pos.and_then(|pos| program.tokens[id].cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1);
                    index += digit * stride;
                    stride *= program.tokens[id].cells.len() + 1;
                }
                let observed = group.inputs[index].iter().find_map(|&(input, attenuation)| (input == super::program::ArrangementSource::Component(source)).then_some(attenuation));
                let expected = (state != 2).then_some(1);
                if observed != expected {
                    mismatches += 1;
                    eprintln!("piston {piston:?} stationary source arrangement {state}: observed {observed:?}, expected {expected:?}");
                }
            }
            for (index, token) in program.tokens.iter().enumerate() {
                eprintln!("token {index}: {:?} at {:?}, cells {:?}", token.block, token.position, token.cells);
                for (cell, links) in program.token_sources[index].iter().enumerate() {
                    let links: Vec<_> = links.iter().map(|&(target, attenuation)| (program.positions[target], attenuation)).collect();
                    eprintln!("  {:?}: {:?}", token.cells[cell], links);
                    let steps = &program.token_updates[index][cell];
                    for (arrival, recipients) in steps.iter().enumerate() {
                        eprintln!("  {:?} {} callbacks: {:?}", token.cells[cell], if arrival == 0 { "departure" } else { "arrival" }, recipients.iter().map(|&component| (program.positions[component], program.initial[component])).collect::<Vec<_>>());
                    }
                    eprintln!("  {:?} cell watchers: {:?}", token.cells[cell], program.token_watchers[index][cell].iter().map(|&component| (program.positions[component], program.initial[component])).collect::<Vec<_>>());
                    for (arrival, steps) in program.move_steps[index][cell].iter().enumerate() {
                        eprintln!("  {:?} {} move steps: {:?}", token.cells[cell], if arrival == 0 { "departure" } else { "arrival" }, steps.iter().map(|step| (program.positions[step.node], step.watcher, step.wire, step.wires.iter().map(|&node| program.positions[node]).collect::<Vec<_>>())).collect::<Vec<_>>());
                    }
                }
            }
            for (source_initial, other_initial, source_cells, other_cells, wire) in [
                (BlockPos::new(47, 32, 41), BlockPos::new(48, 33, 41), [BlockPos::new(46, 32, 41), BlockPos::new(47, 32, 42), BlockPos::new(47, 32, 41)], [BlockPos::new(47, 33, 41), BlockPos::new(48, 33, 41)], BlockPos::new(48, 33, 40)),
                (BlockPos::new(53, 32, 41), BlockPos::new(54, 33, 41), [BlockPos::new(52, 32, 41), BlockPos::new(53, 32, 42), BlockPos::new(53, 32, 41)], [BlockPos::new(53, 33, 41), BlockPos::new(54, 33, 41)], BlockPos::new(54, 33, 42)),
            ] {
                let other_block = world.get_block(other_initial);
                let nearby: Vec<_> = program.tokens.iter().filter(|token| token.cells.iter().any(|cell| cell.x.abs_diff(wire.x) <= 2 && cell.y.abs_diff(wire.y) <= 2 && cell.z.abs_diff(wire.z) <= 2)).map(|token| token.position).collect();
                eprintln!("joint wire {wire:?}: nearby token candidates={nearby:?}");
                let mut table = Vec::new();
                let static_source = wire.offset(mchprs_blocks::BlockFace::Top);
                for other_state in 0..=other_cells.len() {
                    for source_state in 0..=source_cells.len() {
                        let mut probe = ScratchWorld::from_world(&world, bounds, ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() }).unwrap();
                        probe.world_mut().set_block(source_initial, Block::Air);
                        probe.world_mut().set_block(other_initial, Block::Air);
                        if other_state > 0 { probe.world_mut().set_block(other_cells[other_state - 1], other_block); }
                        let token_source = (source_state > 0).then(|| source_cells[source_state - 1]);
                        if let Some(source) = token_source { probe.world_mut().set_block(source, Block::RedstoneBlock); }
                        let links = crate::redpiler::scratch::discover_wire_sources(probe.world(), &[wire]).unwrap();
                        let mut sources: Vec<_> = links.iter().map(|link| {
                            let identity = if Some(link.source) == token_source { "token A".to_owned() }
                                else if other_state > 0 && link.source == other_cells[other_state - 1] { "token B".to_owned() }
                                else { format!("{:?}", link.source) };
                            (identity, link.attenuation)
                        }).collect();
                        sources.sort();
                        let source_a = links.iter().find(|link| Some(link.source) == token_source).map(|link| link.attenuation);
                        let constant = links.iter().find(|link| link.source == static_source).map(|link| link.attenuation);
                        let arrangement = source_state + 4 * other_state;
                        let mut expected_sources = vec![(format!("{static_source:?}"), 0)];
                        if [3, 7].contains(&arrangement) { expected_sources.push(("token A".to_owned(), 1)); }
                        expected_sources.sort();
                        if sources != expected_sources || source_a != ([3, 7].contains(&arrangement)).then_some(1) || constant != Some(0) {
                            mismatches += 1;
                            eprintln!("joint wire {wire:?} arrangement {arrangement}: complete sources={sources:?}; expected token A={:?}, static={static_source:?}:0", ([3, 7].contains(&arrangement)).then_some(1));
                            crate::redpiler::scratch::trace_wire_sources(probe.world(), &[wire]).unwrap();
                        }
                        probe.finish_probe().unwrap();
                        table.push(sources);
                    }
                }
                eprintln!("joint wire {wire:?}: complete Rust source maps={table:?}");
                for (index, sources) in table.iter().enumerate().skip(1) {
                    let removed: Vec<_> = table[0].iter().filter(|(source, _)| !sources.iter().any(|(key, _)| key == source)).collect();
                    let added: Vec<_> = sources.iter().filter(|(source, _)| !table[0].iter().any(|(key, _)| key == source)).collect();
                    let changed: Vec<_> = sources.iter().filter_map(|(source, attenuation)| table[0].iter().find(|(key, old)| key == source && old != attenuation).map(|(_, old)| (source, old, attenuation))).collect();
                    eprintln!("joint wire {wire:?} arrangement {index}: added={added:?}, removed={removed:?}, attenuation_changes={changed:?}");
                }
                let source_id = program.tokens.iter().position(|token| token.position == Some(source_initial)).unwrap();
                let other_id = program.tokens.iter().position(|token| token.position == Some(other_initial)).unwrap();
                let group = program.input_arrangements.iter().find(|group| program.positions[group.target] == wire);
                if let Some(group) = group {
                    for other_state in 0..=other_cells.len() {
                        for source_state in 0..=source_cells.len() {
                            let mut index = 0;
                            let mut stride = 1;
                            for &token in &group.tokens {
                                let at = if token == source_id { source_state.checked_sub(1).map(|cell| source_cells[cell]) }
                                    else if token == other_id { other_state.checked_sub(1).map(|cell| other_cells[cell]) }
                                    else { program.tokens[token].position };
                                let digit = at.and_then(|at| program.tokens[token].cells.iter().position(|&cell| cell == at)).map_or(0, |cell| cell + 1);
                                index += digit * stride;
                                stride *= program.tokens[token].cells.len() + 1;
                            }
                            let observed = group.inputs[index].iter().find_map(|&(source, attenuation)| (source == super::program::ArrangementSource::Token(source_id)).then_some(attenuation));
                            let expected = ([3, 7].contains(&(source_state + 4 * other_state))).then_some(1);
                            if observed != expected {
                                mismatches += 1;
                                eprintln!("joint wire {wire:?} compiled arrangement {}: observed {observed:?}, expected {expected:?}", source_state + 4 * other_state);
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
        let reference_case = reference.as_ref().and_then(|value| value["cases"].as_array()).and_then(|cases| cases.iter().find(|item| item["id"] == id));
        let mut reported = !matches!(id, "prepared-0-1-0" | "prepared-0-1-1" | "prepared-1-1-0" | "prepared-1-1-1");
        compare_reference_frame(&runtime, &manifest, origin, reference_case, "entry", 0, &mut reported);
        let mut actions_executed = 0;
        for (action_index, action) in case["actions"].as_array().unwrap().iter().enumerate() {
            let phase = format!("action-{action_index}");
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = origin + position(&action["pos"]);
                    let source = program.sources.iter().position(|&index| program.positions[index] == pos).expect("manifest control represented");
                    runtime.set_input(source, action["powered"].as_bool().unwrap()).unwrap();
                }
                "wait" | "wait_ready" => for tick in 1..=action["ticks"].as_u64().unwrap() { runtime.tick().unwrap(); compare_reference_frame(&runtime, &manifest, origin, reference_case, &phase, tick, &mut reported); },
                op => panic!("unsupported manifest action {op}"),
            }
            if !matches!(action["op"].as_str().unwrap(), "wait" | "wait_ready") { compare_reference_frame(&runtime, &manifest, origin, reference_case, &phase, 0, &mut reported); }
            actions_executed += 1;
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.tick().unwrap();
            compare_reference_frame(&runtime, &manifest, origin, reference_case, "response", tick, &mut reported);
            for (port, low, high, expected) in [("sum_payload", 1, 3, "sum"), ("sum_repeater", 3, 7, "sum"), ("carry_repeater", 5, 9, "carry")] {
                if !(low..=high).contains(&tick) { continue; }
                let pos = origin + position(&manifest["ports"]["observations"][port]);
                let block = runtime.project(pos);
                if block.is_none() {
                    mismatches += 1;
                    if mismatches <= 32 { eprintln!("{id} {port} tick {tick}: payload is moving"); }
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
                    if mismatches <= 32 { eprintln!("{id} {port} tick {tick}: observed {value}, expected {expected}"); }
                }
            }
        }
        assert!(runtime.operations > 0, "{id}: no compiled instructions executed");
        eprintln!("{id}: actions={}, operations={}, repeater_ticks={}, accepted_pistons={}, token_moves={}, completions={}", actions_executed, runtime.operations, runtime.scheduled_repeater_ticks, runtime.accepted_piston_actions, runtime.token_movements, runtime.motion_completions);
        for event in &runtime.trace_events { eprintln!("{id} trace {event}"); }
        regression |= id == "prepared-0-1-0";
        cases += 1;
    }
    assert_eq!(cases, 8);
    assert!(regression, "prepared-0-1-0 regression must remain in the manifest");
    assert_eq!(mismatches, 0, "ADDER1BIT output mismatches (first 32 printed)");
}

#[test]
fn counter_basic_mair_manifest_execution() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/counter_basic.json")).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest["sha256"].as_str().unwrap());
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let origin = position(&manifest["coordinates"]["origin"]);
    let bounds = (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(27, 20, 38));
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut world = PlotWorld::from_chunks(0, 0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(), Default::default());
        paste_clipboard(&mut world, &clipboard, position(&manifest["coordinates"]["paste_anchor"]), false);
        let program = compile(&world, bounds, ScratchEntry { scheduled: world.scheduler().iter_entries().collect(), piston: world.piston_state().clone() })
            .unwrap_or_else(|error| panic!("{id}: compilation declined: {error:?}; block={:?}", error.position.map(|pos| world.get_block(pos))));
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
        for _ in 0..case["ticks"].as_u64().unwrap() { runtime.tick().unwrap(); }
        eprintln!("{id}: operations={}, repeater_ticks={}, accepted_pistons={}, token_moves={}, completions={}", runtime.operations, runtime.scheduled_repeater_ticks, runtime.accepted_piston_actions, runtime.token_movements, runtime.motion_completions);
    }
}
