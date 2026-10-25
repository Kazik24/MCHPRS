//! Narrow diagnosis of the historical unchecked CHUNGUS mismatch. This deliberately avoids
//! per-tick World projection and is ignored by default because compiling CHUNGUS is expensive.

use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::program_graph::ComponentState;
use crate::world::storage::Chunk;
use mchprs_save_data::plot_data::PlotData;
use serde_json::json;
use std::sync::Arc;

const FOCUS_WIRE: BlockPos = BlockPos::new(99, 79, 212);
const FOCUS_COMPARATOR: BlockPos = BlockPos::new(100, 80, 212);
const START_BUTTON: BlockPos = BlockPos::new(187, 99, 115);
const FOCUS_TICK: u64 = 512;

fn load_chungus() -> PlotWorld {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/chungus_mandelbrot_plot");
    let data = PlotData::load_from_file(path, false).unwrap();
    let chunks = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| Chunk::load(index as i32 / PLOT_WIDTH, index as i32 % PLOT_WIDTH, chunk))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect())
}

fn output(backend: &GraphBackend, node: NodeId) -> u8 {
    match &backend.state.components[node.index()] {
        ComponentState::Electrical(state) => state.output_strength,
        ComponentState::Wire(state) => state.power,
        ComponentState::Specialized => 0,
    }
}

fn source_value(backend: &mut GraphBackend, source: super::Source) -> Option<u8> {
    match source {
        super::Source::Node(node) => Some(output(backend, node)),
        super::Source::LiveBlockPower(node) => {
            let plan = backend.wire_plan(node).clone();
            Some(backend.seed_power_general(&plan).block_power)
        }
    }
}

fn input_links(backend: &GraphBackend, node: NodeId, port: InputPort) -> Vec<serde_json::Value> {
    backend
        .program
        .power_links_to(PowerTarget::Input { node, port })
        .iter()
        .map(|link| {
            let site = backend.program.sites()[link.from.index()];
            let enabled = backend.guard_matches(link.guard, None);
            let power = backend.site_power(link.from, link.from_port);
            let source_node = match site.source {
                SiteSource::Component(source) => Some(source),
                SiteSource::Contact(_) | SiteSource::Constant(_) => None,
            };
            let source = source_node.map(|source| {
                json!({
                    "node":source.index(),
                    "position":backend.program.nodes()[source.index()].position,
                    "kind":format!("{:?}", backend.program.nodes()[source.index()].kind),
                    "state":format!("{:?}", backend.state.components[source.index()]),
                    "wire_power":matches!(backend.program.nodes()[source.index()].kind, CircuitNodeKind::Wire(_))
                        .then(|| backend.wire_state(source).power),
                })
            });
            json!({
                "site":link.from.index(),
                "position":site.position,
                "source":format!("{:?}", site.source),
                "component":source,
                "port":format!("{:?}", link.from_port),
                "guard":link.guard.index(),
                "guard_enabled":enabled,
                "attenuation":link.attenuation,
                "site_power":power,
                "contribution":if enabled { power.saturating_sub(link.attenuation) } else { 0 },
                "resolved":format!("{:?}", backend.resolve_site(link.from, None)),
            })
        })
        .collect()
}

fn fixed_signal_rows(backend: &GraphBackend, node: NodeId, port: InputPort) -> serde_json::Value {
    let Some((constant, inputs)) = backend.program.fixed_signal_query(PowerTarget::Input { node, port }) else {
        return serde_json::Value::Null;
    };
    let inputs = inputs
        .iter()
        .map(|input| {
            let strength = match &backend.state.components[input.node.index()] {
                ComponentState::Electrical(state) => state.output_strength,
                ComponentState::Wire(_) => backend.wire_state(input.node).power,
                ComponentState::Specialized => 0,
            };
            let shape_ok = !input.require_cross || !crate::redstone::wire::is_dot(backend.wire_state(input.node));
            json!({
                "node":input.node.index(),
                "position":backend.program.nodes()[input.node.index()].position,
                "kind":format!("{:?}", backend.program.nodes()[input.node.index()].kind),
                "strength":strength,
                "attenuation":input.attenuation,
                "require_cross":input.require_cross,
                "shape_ok":shape_ok,
                "contribution":if shape_ok { strength.saturating_sub(input.attenuation) } else { 0 },
            })
        })
        .collect::<Vec<_>>();
    json!({"constant":constant,"inputs":inputs})
}

fn report_boundary(label: &str, backend: &mut GraphBackend, wire: NodeId, comparator: NodeId, sources: &[(super::Source, u8)]) {
    let source_rows = sources
        .iter()
        .map(|&(source, attenuation)| {
            json!({
                "source":format!("{source:?}"),
                "position":match source {
                    super::Source::Node(node) | super::Source::LiveBlockPower(node) => backend.program.nodes()[node.index()].position,
                },
                "value":source_value(backend, source),
                "attenuation":attenuation,
            })
        })
        .collect::<Vec<_>>();
    let comparator_state = match &backend.state.components[comparator.index()] {
        ComponentState::Electrical(state) => state,
        _ => unreachable!(),
    };
    let comparator_ticks = backend
        .scheduled_ticks()
        .into_iter()
        .filter(|tick| tick.pos == FOCUS_COMPARATOR)
        .collect::<Vec<_>>();
    let ordered_future = backend
        .scheduled_ticks()
        .into_iter()
        .enumerate()
        .map(|(order, tick)| {
            json!({
                "order":order,
                "position":tick.pos,
                "block_type":tick.block_type,
                "ticks_left":tick.ticks_left,
                "priority":format!("{:?}",tick.tick_priority),
            })
        })
        .collect::<Vec<_>>();
    let comparator_main = backend.comparator_input(comparator);
    let comparator_side = backend.comparator_side(comparator);
    println!(
        "CHUNGUS TICK512 {label} {}",
        json!({
            "logical_tick":backend.future.clock.logical_tick,
            "wire":backend.wire_state(wire).power,
            "comparator_powered":comparator_state.powered,
            "comparator_output":comparator_state.output_strength,
            "comparator_main_input":comparator_main,
            "comparator_side_input":comparator_side,
            "comparator_calculated_output":backend.comparator_output(comparator),
            "comparator_main_links":input_links(backend, comparator, InputPort::Main),
            "comparator_side_links":input_links(backend, comparator, InputPort::Side),
            "comparator_main_fixed_signals":fixed_signal_rows(backend, comparator, InputPort::Main),
            "comparator_side_fixed_signals":fixed_signal_rows(backend, comparator, InputPort::Side),
            "comparator_pending":backend.is_pending(comparator),
            "comparator_future":comparator_ticks,
            "ordered_future":ordered_future,
            "source_terms":source_rows,
        })
    );
}

#[test]
#[ignore = "research: bounded no-projection trace for unchecked CHUNGUS tick-512 analog mismatch"]
fn unchecked_chungus_tick512_traces_live_sources_and_receiver_timing() {
    let world = load_chungus();
    let bounds = world.get_corners();
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(2),
        budget_multiplier: 8,
        ..Default::default()
    };
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let template = GraphBackend::compile_with_options(&world, bounds, world.scheduler().iter_entries().collect(), &options, monitor).unwrap();
    let program = template.program.clone();
    let state = template.state.clone();
    let wire = NodeId::new(
        program
            .nodes()
            .iter()
            .position(|node| node.position == Some(FOCUS_WIRE))
            .expect("focus wire in CHUNGUS graph") as u32,
    );
    let comparator = NodeId::new(
        program
            .nodes()
            .iter()
            .position(|node| node.position == Some(FOCUS_COMPARATOR))
            .expect("focus comparator in CHUNGUS graph") as u32,
    );
    assert!(matches!(program.nodes()[wire.index()].kind, CircuitNodeKind::Wire(_)));
    assert!(matches!(
        program.nodes()[comparator.index()].kind,
        CircuitNodeKind::Electrical(NodeType::Comparator { .. })
    ));

    let mut reference = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let mut candidate = GraphBackend::new_unactivated(program.clone(), state, 0).unwrap();
    reference.test_recorder.comparator_tick_focus = Some((comparator, FOCUS_TICK));
    candidate.test_recorder.comparator_tick_focus = Some((comparator, FOCUS_TICK));
    let unchecked = UncheckedProgram::attach_all(&mut candidate);
    unchecked.set_diagnostic_focus(wire, comparator, FOCUS_TICK);
    let net_index = unchecked.by_node[wire.index()];
    let net = &unchecked.nets[net_index];
    let local = net.wires.iter().position(|&node| node == wire).unwrap();
    let source_terms = net.terms[local]
        .iter()
        .map(|&(source, attenuation)| (net.sources[source], attenuation))
        .collect::<Vec<_>>();
    println!(
        "CHUNGUS TICK512 MODEL {}",
        json!({
            "wire":FOCUS_WIRE,
            "wire_node":wire.index(),
            "comparator":FOCUS_COMPARATOR,
            "comparator_node":comparator.index(),
            "weighted_component_wires":net.wires.len(),
            "target_source_terms":source_terms.iter().map(|(source,attenuation)|json!({"source":format!("{source:?}"),"attenuation":attenuation})).collect::<Vec<_>>(),
            "start_button":START_BUTTON,
        })
    );

    reference.use_block(START_BUTTON).unwrap();
    candidate.use_block(START_BUTTON).unwrap();
    for tick in 1..=FOCUS_TICK {
        reference.tick().unwrap();
        candidate.tick().unwrap();
        if tick == FOCUS_TICK - 1 {
            report_boundary("after511-reference", &mut reference, wire, comparator, &source_terms);
            report_boundary("after511-weighted", &mut candidate, wire, comparator, &source_terms);
        }
    }
    report_boundary("after512-reference", &mut reference, wire, comparator, &source_terms);
    report_boundary("after512-weighted", &mut candidate, wire, comparator, &source_terms);
    assert_eq!(reference.wire_state(wire).power, 13);
    assert_eq!(candidate.wire_state(wire).power, 9);
    assert_eq!(output(&reference, comparator), 13);
    assert_eq!(output(&candidate, comparator), 9);
}
