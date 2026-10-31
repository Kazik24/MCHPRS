#[path = "../../../test_support/cpus.rs"]
mod cpus;

use super::{ScratchEntry, compiler, program::{ArrangementSource, ElectricalLink, InputArrangement, MoveStep, Program, RepeaterRecord}};
use crate::{plot::{PlotWorld, worldedit::load_schematic}, world::World};
use mchprs_blocks::{BlockDirection, BlockPos, blocks::{Block, RedstoneRepeater, RedstoneWire}};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use mchprs_world::TickPriority;

fn canonical(block: Block) -> String {
    let properties: BTreeMap<_, _> = block.properties().into_iter().collect();
    format!(
        "minecraft:{}[{}]",
        block.get_name(),
        properties.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(",")
    )
}

fn scheduled(entries: impl IntoIterator<Item = mchprs_world::TickEntry>, world: &impl World) -> Vec<Value> {
    entries
        .into_iter()
        .map(|entry| {
            json!({
                "pos": entry.pos,
                "block": canonical(world.get_block(entry.pos)),
                "ticks_left": entry.ticks_left,
                "priority": format!("{:?}", entry.tick_priority),
            })
        })
        .collect()
}

fn cone(program: &super::Program, roots: &[usize]) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut queue = VecDeque::from(roots.to_vec());
    let mut tokens = BTreeSet::new();
    while let Some(target) = queue.pop_front() {
        if !found.insert(target) {
            continue;
        }
        for &link in &program.incoming[target] {
            queue.push_back(program.links[link].source);
        }
        for link in program.side_links.iter().filter(|link| link.target == target) {
            queue.push_back(link.source);
        }
        for link in program.arrangement_links.iter().filter(|link| link.target == target) {
            if let ArrangementSource::Component(source) = link.source {
                queue.push_back(source);
            }
        }
        for arrangement in program.input_arrangements.iter().filter(|group| group.target == target) {
            for source in &arrangement.conditional_sources {
                match source {
                    ArrangementSource::Component(source) => queue.push_back(*source),
                    ArrangementSource::Token(token) => { tokens.insert(*token); }
                }
            }
        }
        if let Some(repeater) = program.repeaters.iter().find(|record| record.component == target) {
            queue.extend(repeater.side_sources.iter().flatten().copied());
        }
    }
    for token in tokens {
        let cells = &program.tokens[token].cells;
        for piston in &program.pistons {
            if cells.contains(&piston.home) || cells.contains(&piston.far) {
                if let Some(&component) = program.component_indices.get(&piston.base) {
                    queue.push_back(component);
                }
            }
        }
    }
    while let Some(target) = queue.pop_front() {
        if !found.insert(target) { continue; }
        for &link in &program.incoming[target] { queue.push_back(program.links[link].source); }
        for link in program.side_links.iter().filter(|link| link.target == target) { queue.push_back(link.source); }
    }
    found
}

fn runtime_state(runtime: &super::Runtime, nodes: &BTreeSet<usize>) -> Value {
    let program = &runtime.program;
    let components: Vec<_> = nodes
        .iter()
        .map(|&index| {
            json!({
                "pos": program.positions[index],
                "block": canonical(runtime.blocks[index]),
                "rear_power": runtime.input_strength(index),
                "side_power": runtime.side_strength(index),
                "links": program.incoming[index].iter().map(|&link_index| {
                    let link = &program.links[link_index];
                    json!({
                        "source": program.positions[link.source],
                        "source_block": canonical(runtime.blocks[link.source]),
                        "attenuation": link.attenuation,
                        "contribution": runtime.contributions[link_index],
                    })
                }).collect::<Vec<_>>(),
                "arrangements": program.arrangement_links.iter().enumerate()
                    .filter(|(_, link)| link.target == index)
                    .map(|(id, link)| json!({
                        "source": format!("{:?}", link.source),
                        "source_power": runtime.arrangement_output(link.source),
                        "attenuation": link.attenuation,
                        "active": link.active[runtime.group_arrangements[link.group]],
                        "contribution": runtime.arrangement_contributions[id],
                        "side": link.side,
                    })).collect::<Vec<_>>(),
                "input_arrangements": program.input_arrangements.iter().filter(|group| group.target == index).map(|group| {
                    let mut state = 0;
                    let mut stride = 1;
                    for &token in &group.tokens {
                        let digit = runtime.token_positions[token]
                            .and_then(|position| program.tokens[token].cells.iter().position(|&cell| cell == position))
                            .map_or(0, |cell| cell + 1);
                        state += digit * stride;
                        stride *= program.tokens[token].cells.len() + 1;
                    }
                    json!({
                        "side": group.side,
                        "state": state,
                        "tokens": group.tokens.iter().map(|&token| json!({"block": canonical(program.tokens[token].block), "pos": runtime.token_positions[token], "cells": program.tokens[token].cells})).collect::<Vec<_>>(),
                        "active_sources": group.inputs[state].iter().map(|(source, attenuation)| json!([format!("{:?}", source), attenuation])).collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "components": components,
        "token_arrangements": program.input_arrangements.iter()
            .filter(|group| nodes.contains(&group.target))
            .map(|group| {
                let mut state = 0;
                let mut stride = 1;
                for &token in &group.tokens {
                    let digit = runtime.token_positions[token]
                        .and_then(|position| program.tokens[token].cells.iter().position(|&cell| cell == position))
                        .map_or(0, |cell| cell + 1);
                    state += digit * stride;
                    stride *= program.tokens[token].cells.len() + 1;
                }
                json!({ "target": program.positions[group.target], "state": state })
            }).collect::<Vec<_>>(),
        "pending": runtime.pending.iter_entries().map(|entry| json!({
            "pos": entry.pos,
            "block": runtime.program.component_indices.get(&entry.pos).map(|&i| canonical(runtime.blocks[i])).unwrap_or_else(|| "outside-components".into()),
            "ticks_left": entry.ticks_left,
            "priority": format!("{:?}", entry.tick_priority),
        })).collect::<Vec<_>>(),
        "piston_events": runtime.pending_piston_events(),
        "motions": runtime.motions.iter().map(|motion| json!({
            "piston": program.pistons[motion.piston].base,
            "destination": motion.destination,
            "start": motion.start,
            "landing": motion.landing,
            "extending": motion.extending,
            "token_done": motion.token_done,
            "piston_done": motion.piston_done,
        })).collect::<Vec<_>>(),
    })
}

fn native_callbacks(callbacks: Vec<crate::world::RedstoneUpdateObservation>, positions: &[BlockPos]) -> Vec<Value> {
    callbacks.into_iter().filter(|callback| positions.contains(&callback.pos)).map(|callback| json!({
        "pos": callback.pos,
        "block": canonical(callback.block),
        "direction": format!("{:?}", callback.direction),
        "input_on": callback.input_on,
    })).collect()
}

fn relevant_events(events: &[String], positions: &[BlockPos]) -> Vec<String> {
    events.iter().filter(|event| positions.iter().any(|position| event.contains(&format!("{position:?}")))).cloned().collect()
}

fn native_state(world: &PlotWorld, positions: &[BlockPos]) -> Value {
    json!({
        "blocks": positions.iter().map(|&pos| json!({"pos": pos, "block": canonical(world.get_block(pos))})).collect::<Vec<_>>(),
        "pending": scheduled(world.scheduler().iter_entries(), world),
        "piston_state": format!("{:?}", world.piston_state()),
    })
}

#[test]
fn moving_redstone_block_refreshes_wire_contribution_before_repeater_callback() {
    let wire_pos = BlockPos::new(0, 20, 20);
    let repeater_pos = BlockPos::new(1, 20, 20);
    let token_cell = BlockPos::new(0, 20, 21);
    let mut program = Program::new(
        vec![wire_pos, repeater_pos],
        vec![
            Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } },
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater { delay: 1, facing: BlockDirection::West, locked: false, powered: true },
            },
        ],
        vec![],
        vec![[vec![], vec![]]; 2],
        vec![ElectricalLink { source: 0, target: 1, attenuation: 0 }],
        vec![],
        vec![RepeaterRecord { component: 1, output_is_diode: false, side_sources: None }],
    ).unwrap();
    program.tokens.push(compiler::PistonToken {
        block: Block::RedstoneBlock,
        position: Some(token_cell),
        cells: vec![token_cell],
    });
    program.input_arrangements.push(InputArrangement {
        target: 0,
        side: false,
        silent: false,
        tokens: vec![0],
        conditional_sources: vec![ArrangementSource::Token(0)],
        inputs: vec![vec![], vec![(ArrangementSource::Token(0), 0)]],
    });
    program.compile_movement_flips().unwrap();
    program.move_steps = vec![vec![[
        vec![MoveStep { node: 1, watcher: true, wire: false, wires: vec![] }],
        vec![],
    ]]];
    let mut runtime = program.start();
    runtime.trace_enabled = true;
    for (index, link) in runtime.program.arrangement_links.iter().enumerate() {
        if link.active[runtime.group_arrangements[link.group]] {
            runtime.arrangement_contributions[index] = runtime.arrangement_output(link.source).saturating_sub(link.attenuation);
        }
    }
    assert_eq!(runtime.input_strength(0), 15);
    assert_eq!(runtime.input_strength(1), 15);

    runtime.set_token_pos(0, None);
    assert_eq!(runtime.blocks[0], Block::RedstoneWire { wire: RedstoneWire { power: 0, ..Default::default() } });
    assert_eq!(runtime.contributions[0], 0);
    let pending = runtime.pending_entries();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].pos, repeater_pos);
    assert_eq!(pending[0].ticks_left, 2);
    assert_eq!(pending[0].tick_priority, TickPriority::Higher);
    for _ in 0..2 { runtime.tick().unwrap(); }
    assert!(matches!(runtime.blocks[1], Block::RedstoneRepeater { repeater } if !repeater.powered));

    let events = &runtime.trace_events;
    let link = events.iter().position(|event| event.contains("link BlockPos { x: 0, y: 20, z: 20 } -> BlockPos { x: 1, y: 20, z: 20 } strength=15->0")).unwrap();
    let callback = events.iter().position(|event| event.contains("callback BlockPos { x: 1, y: 20, z: 20 }")).unwrap();
    let repeater_callback = events.iter().position(|event| event.contains("repeater callback BlockPos { x: 1, y: 20, z: 20 } powered=true")).unwrap();
    let scheduled = events.iter().position(|event| event.contains("schedule repeater BlockPos { x: 1, y: 20, z: 20 } remaining=Some(2) priority=Higher")).unwrap();
    let due = events.iter().position(|event| event.contains("due tick BlockPos { x: 1, y: 20, z: 20 }")).unwrap();
    let executed = events.iter().position(|event| event.contains("repeater tick BlockPos { x: 1, y: 20, z: 20 }")).unwrap();
    assert!(link < callback && callback < repeater_callback && repeater_callback < scheduled && scheduled < due && due < executed);
}

#[test]
#[ignore = "run the new 50,000-tick ZCore fixture once and retain its screen trace"]
fn zcore12b_fix_interpreter_screen_trace() {
    let cpu = cpus::CPUS[3];
    let mut world = cpus::load_cpu(cpu);
    let start = cpu.start + cpu.origin;
    let button = world.get_block(start);
    assert!(matches!(button, Block::StoneButton { .. }), "ZCore Start at {start}: {button:?}");
    let mut frames = Vec::new();
    cpus::collect_screen(&world, cpu, 0, &mut frames);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    for tick in 1..=50_000 {
        world.tick_interpreted();
        cpus::collect_screen(&world, cpu, tick, &mut frames);
    }
    assert_eq!(frames.first().map(|frame| frame.tick), Some(0));
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zcore12b-fix-screen-trace.json");
    std::fs::write(&artifact, serde_json::to_vec_pretty(&json!({
        "fixture": cpu.schematic,
        "fixture_sha256": cpu.sha256,
        "engine": "current Rust native interpreter",
        "start": start,
        "start_block": canonical(button),
        "ticks": 50_000,
        "screen_frames": frames,
        "final_screen": cpus::screen(&world, cpu),
    })).unwrap()).unwrap();
    eprintln!("ZCore12B_FIX native screen trace: {}", artifact.display());
}

#[test]
#[ignore = "rebuild a tick 0..10 native/compiled trace; run explicitly"]
fn bubblesort_early_native_compiled_trace() {
    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let program = compiler::compile(&world, bounds, ScratchEntry { scheduled: ticks.clone(), piston: world.piston_state().clone() }).unwrap();
    let start = cpu.start + cpu.origin;
    let wire = BlockPos::new(172, 14, 161);
    let repeater = BlockPos::new(173, 14, 162);
    let wire_index = program.component_indices[&wire];
    let repeater_index = program.component_indices[&repeater];
    let dependency_cone = cone(&program, &[wire_index, repeater_index]);
    let nodes = BTreeSet::from([wire_index, repeater_index]);
    let positions: Vec<_> = nodes.iter().map(|&index| program.positions[index]).collect();
    let moving_source = BlockPos::new(172, 14, 160);
    let focus = [moving_source, wire, repeater];
    let mut runtime = program.start();
    runtime.set_pending(ticks.clone());
    runtime.piston_state = world.piston_state().clone();
    runtime.trace_enabled = true;

    for (index, &position) in program.positions.iter().enumerate() {
        assert_eq!(program.initial[index], world.get_block(position), "compiled entry state at {position}");
    }
    assert_eq!(runtime.pending_entries(), ticks, "runtime entry scheduler");
    let mut trace = vec![json!({
        "step": "tick-zero",
        "native": native_state(&world, &focus),
        "compiled": runtime_state(&runtime, &nodes),
    })];

    world.observe_redstone_updates(true);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    let native_start_callbacks = native_callbacks(world.take_redstone_update_observations(), &focus);
    world.observe_redstone_updates(true);
    runtime.use_control(start).unwrap();
    trace.push(json!({
        "step": "immediately-after-start",
        "native": native_state(&world, &focus),
        "native_callbacks": native_start_callbacks,
        "compiled": runtime_state(&runtime, &nodes),
        "compiled_events": relevant_events(&runtime.trace_events, &focus),
    }));

    let mut prior = runtime.trace_events.len();
    let mut tick_2_matched = false;
    let mut tick_4_matched = false;
    for tick in 1..=10 {
        world.tick_interpreted();
        let callbacks = native_callbacks(world.take_redstone_update_observations(), &focus);
        world.observe_redstone_updates(true);
        runtime.tick().unwrap();
        let events = relevant_events(&runtime.trace_events[prior..], &focus);
        prior = runtime.trace_events.len();
        trace.push(json!({
            "step": format!("tick-{tick}"),
            "native": native_state(&world, &focus),
            "native_callbacks": callbacks,
            "compiled": runtime_state(&runtime, &nodes),
            "compiled_events": events,
        }));
        if tick == 2 {
            let native_due = world.scheduler().iter_entries().any(|entry| entry.pos == repeater && entry.ticks_left == 2 && entry.tick_priority == TickPriority::Higher);
            let compiled_due = runtime.pending_entries().iter().any(|entry| entry.pos == repeater && entry.ticks_left == 2 && entry.tick_priority == TickPriority::Higher);
            let contribution = runtime.program.incoming[repeater_index].iter().map(|&link| runtime.contributions[link]).max().unwrap_or(0);
            tick_2_matched = native_due && compiled_due && contribution == 0
                && matches!(world.get_block(repeater), Block::RedstoneRepeater { repeater } if repeater.powered)
                && matches!(runtime.project(repeater), Some(Block::RedstoneRepeater { repeater }) if repeater.powered);
        }
        if tick == 4 {
            tick_4_matched = matches!(world.get_block(repeater), Block::RedstoneRepeater { repeater } if !repeater.powered)
                && matches!(runtime.project(repeater), Some(Block::RedstoneRepeater { repeater }) if !repeater.powered);
        }
    }

    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cpu-bubblesort-early-trace.json");
    std::fs::write(&artifact, serde_json::to_vec_pretty(&json!({
        "baseline": "ba76b232f97f90070d5bcbc2263bdfb9c7e983ac",
        "fixture_sha256": cpu.sha256,
        "start": start,
        "moving_source": moving_source,
        "wire": wire,
        "repeater": repeater,
        "component_count": program.component_count(),
        "dependency_cone_component_count": dependency_cone.len(),
        "dependency_cone": positions,
        "trace": trace,
    })).unwrap()).unwrap();
    eprintln!("BubbleSort tick 0..10 trace: {}", artifact.display());
    assert!(tick_2_matched, "native and compiled executions retain the same tick-4 repeater pulse");
    assert!(tick_4_matched, "native and compiled repeaters both release at game tick 4");
}
