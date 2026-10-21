use super::*;
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::BTreeMap;

#[test]
#[ignore = "expensive O6 FPU equation extraction diagnostic; reports coverage and exact decline causes"]
fn report_fpu_logical_output_equations() {
    let (descriptor, world, origin, bounds) = load_fpu_fixture();

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();

    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().expect("O6 FPU should have cell boundaries");
    let actors = catalog.cells.iter().map(|cell| cell.uses.len()).sum::<usize>();
    let equations = Equations::new(graph, &monitor).unwrap();
    println!(
        "FPU graph: nodes={} pistons={} resources={} recognized_cells={} recognized_cell_actors={} bud_memory_reads={} raw_exprs={} reduced_exprs={} reduced_inputs={}",
        graph.nodes.len(),
        graph.pistons.len(),
        graph.resources.len(),
        catalog.cells.len(),
        actors,
        graph.plans.bud_memory_reads.len(),
        equations.raw.expressions.len(),
        equations.reduced.expressions.len(),
        equations.reduced.inputs.len(),
    );

    let mut equations = equations;
    let mut all_leaves = FxHashSet::default();
    let mut failures = BTreeMap::<String, Vec<String>>::new();
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .expect("manifest final dust output positions");
    let mut admitted = 0;
    let mut reduced_roots = Vec::new();
    for raw_position in output_positions {
        let position = origin + cpus::manifest_position(raw_position);
        let Some((index, node)) = graph.nodes.iter().enumerate().find(|(_, node)| node.position == Some(position)) else {
            failures
                .entry("manifest final dust position has no prepared node".into())
                .or_default()
                .push(format!("{position:?}"));
            continue;
        };
        let node_id = NodeId::new(index as u32);
        if !matches!(&node.kind, CircuitNodeKind::Wire(_)) {
            failures
                .entry("manifest final dust node is not a wire".into())
                .or_default()
                .push(format!("{position:?} {node_id:?} {:?}", node.kind));
            continue;
        }
        // A dust output's strength is its wire-node Main input. Conducted power
        // at the wire's site is a different query and loses the wire's own drive.
        match equations.input_power(node_id, InputPort::Main) {
            Ok(root) => {
                reduced_roots.push((position, node_id, root));
            }
            Err(error) => {
                failures
                    .entry(format!("{error:?}"))
                    .or_default()
                    .push(format!("{position:?} {node_id:?}"));
            }
        }
    }
    for (position, node_id, root) in reduced_roots {
        let dependencies = equation_inputs(&equations.reduced, root);
        all_leaves.extend(dependencies.iter().copied());
        admitted += 1;
        println!("final dust {position:?} {node_id:?}: root={root:?} unique_leaves={}", dependencies.len());
    }

    let mut leaf_kinds = BTreeMap::<String, usize>::new();
    for leaf in all_leaves {
        *leaf_kinds.entry(leaf_kind(graph, &equations, leaf)).or_default() += 1;
    }
    println!(
        "FPU final dust outputs: requested={} admitted={} failed={} shared_raw_exprs={} shared_reduced_exprs={} shared_reduced_inputs={} unique_input_leaves={}",
        output_positions.len(),
        admitted,
        output_positions.len().saturating_sub(admitted),
        equations.raw.expressions.len(),
        equations.reduced.expressions.len(),
        equations.reduced.inputs.len(),
        leaf_kinds.values().sum::<usize>(),
    );
    println!("FPU final dust unique input leaf kinds/dependencies: {leaf_kinds:#?}");
    println!("FPU unresolved resource/component causes: {failures:#?}");
}

#[test]
#[ignore = "bounded native diagnostic: switch saved trigger OFF once and hold OFF for 4096 ticks"]
fn probe_fpu_native_trigger_off_4096() {
    let (descriptor, mut world, origin, _) = load_fpu_fixture();
    world.disable_command_output_limits_for_replay();
    let repeaters = descriptor["observations"]["output_repeaters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + cpus::manifest_position(position))
        .collect::<Vec<_>>();
    let dust = descriptor["observations"]["output"]
        .as_array()
        .expect("manifest final dust positions")
        .iter()
        .map(|position| origin + cpus::manifest_position(position))
        .collect::<Vec<_>>();
    let trigger = origin + cpus::manifest_position(&descriptor["ports"]["trigger"]);
    let Block::Lever { lever } = world.get_block(trigger) else {
        panic!("expected trigger lever at {trigger:?}");
    };
    assert!(lever.powered, "probe must start with the saved trigger ON");
    let initial = output_words(&world, &repeaters, &dust);
    println!(
        "FPU fresh saved entry: trigger=ON post_adder_repeaters={:04x} final_dust={:04x} pending_ticks={}",
        initial.0,
        initial.1,
        world.scheduler().iter_entries().count(),
    );
    assert!(set_lever(&mut world, trigger, false), "single OFF edge must change the trigger");
    let start = output_words(&world, &repeaters, &dust);
    println!(
        "FPU single trigger OFF edge: post_adder_repeaters={:04x} final_dust={:04x} pending_ticks={}",
        start.0,
        start.1,
        world.scheduler().iter_entries().count(),
    );

    let mut first_change = None;
    for phase in 0..16 {
        let mut histogram = BTreeMap::<(u16, u16), usize>::new();
        for offset in 0..256 {
            world.tick_interpreted();
            let output = output_words(&world, &repeaters, &dust);
            let tick = phase * 256 + offset + 1;
            if first_change.is_none() && output != start {
                first_change = Some(tick);
            }
            *histogram.entry(output).or_default() += 1;
        }
        let last = output_words(&world, &repeaters, &dust);
        println!(
            "FPU trigger held OFF ticks={}..{} first_change={first_change:?} last_post_adder={:04x} last_final_dust={:04x} histogram={histogram:#?} pending_ticks={}",
            phase * 256 + 1,
            (phase + 1) * 256,
            last.0,
            last.1,
            world.scheduler().iter_entries().count(),
        );
    }
    let Block::Lever { lever } = world.get_block(trigger) else {
        unreachable!("trigger block kind must remain a lever");
    };
    assert!(!lever.powered, "trigger must remain OFF throughout the probe");
    let last = output_words(&world, &repeaters, &dust);
    println!(
        "FPU trigger-OFF result: first_change={first_change:?} last_word=(post_adder_repeaters={:04x}, final_dust={:04x}) pending_ticks={}",
        last.0,
        last.1,
        world.scheduler().iter_entries().count(),
    );
}

fn load_fpu_fixture() -> (serde_json::Value, crate::plot::PlotWorld, BlockPos, (BlockPos, BlockPos)) {
    let descriptor_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/fixtures/fpu_legal.json");
    let mut descriptor: serde_json::Value = serde_json::from_slice(&std::fs::read(descriptor_path).unwrap()).unwrap();
    let origin = BlockPos::new(40, 30, 40);
    descriptor["origin"] = serde_json::json!([origin.x, origin.y, origin.z]);
    let world = cpus::load_manifest(&descriptor).unwrap();
    let dimensions = &descriptor["dimensions"];
    let bounds = (
        origin,
        origin
            + BlockPos::new(
                dimensions[0].as_i64().unwrap() as i32 - 1,
                dimensions[1].as_i64().unwrap() as i32 - 1,
                dimensions[2].as_i64().unwrap() as i32 - 1,
            ),
    );
    (descriptor, world, origin, bounds)
}

fn output_word(world: &crate::plot::PlotWorld, outputs: &[BlockPos]) -> u16 {
    outputs.iter().fold(0u16, |word, &position| {
        (word << 1) | u16::from(matches!(world.get_block(position), Block::RedstoneRepeater { repeater } if repeater.powered))
    })
}

fn output_words(world: &crate::plot::PlotWorld, repeaters: &[BlockPos], dust: &[BlockPos]) -> (u16, u16) {
    let dust_word = dust.iter().fold(0u16, |word, &position| {
        (word << 1) | u16::from(matches!(world.get_block(position), Block::RedstoneWire { wire } if wire.power > 0))
    });
    (output_word(world, repeaters), dust_word)
}

fn set_lever(world: &mut crate::plot::PlotWorld, position: BlockPos, powered: bool) -> bool {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected trigger lever at {position:?}");
    };
    if lever.powered == powered {
        return false;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let face = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(face));
    true
}

fn leaf_kind(graph: &CompileGraph, equations: &Equations<'_>, source: LayerInputSource) -> String {
    match source {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let kind = match graph.sites[site.index()].source {
                SiteSource::Component(node) => match &graph.nodes[node.index()].kind {
                    CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => "repeater",
                    CircuitNodeKind::Electrical(NodeType::Torch) => "torch",
                    CircuitNodeKind::Electrical(NodeType::Lamp) => "lamp",
                    CircuitNodeKind::Electrical(NodeType::Lever) => "lever",
                    CircuitNodeKind::Electrical(NodeType::Button) => "button",
                    CircuitNodeKind::Electrical(NodeType::PressurePlate) => "pressure_plate",
                    CircuitNodeKind::Electrical(NodeType::Constant) => "constant",
                    CircuitNodeKind::Electrical(_) => "other_electrical",
                    CircuitNodeKind::Observer { .. } => "observer",
                    CircuitNodeKind::BuiltinReset { .. } => "builtin_reset",
                    CircuitNodeKind::Wire(_) => "wire",
                    CircuitNodeKind::Piston(_) => "piston",
                    CircuitNodeKind::Contact(_) => "contact",
                },
                SiteSource::Constant(_) => "constant_site",
                SiteSource::Contact(_) => "contact_site",
            };
            format!("SourceAbove::{kind}:{port:?}:threshold={threshold}")
        }
        LayerInputSource::State(StateTest::ResourceAt { resource, .. }) => {
            if equations.registers.contains_key(&resource) {
                "State::ResourceAt(register)".into()
            } else if equations.cells.contains_key(&resource) {
                "State::ResourceAt(cell)".into()
            } else {
                "State::ResourceAt(unclassified)".into()
            }
        }
        LayerInputSource::State(StateTest::ResourceEnabled(resource)) => {
            if equations.registers.contains_key(&resource) {
                "State::ResourceEnabled(register)".into()
            } else if equations.cells.contains_key(&resource) {
                "State::ResourceEnabled(cell)".into()
            } else {
                "State::ResourceEnabled(unclassified)".into()
            }
        }
        LayerInputSource::State(StateTest::PistonExtended(actor)) => {
            let register = equations.registers.values().any(|read| read.storage == actor);
            let cell = equations.cells.values().any(|(_, actors)| actors.contains(&actor));
            format!("State::PistonExtended({})", owner_kind(register, cell))
        }
        LayerInputSource::State(StateTest::PistonHeadPresent(actor)) => {
            let register = equations.registers.values().any(|read| read.storage == actor);
            let cell = equations.cells.values().any(|(_, actors)| actors.contains(&actor));
            format!("State::PistonHeadPresent({})", owner_kind(register, cell))
        }
        LayerInputSource::State(StateTest::PistonAvailability { .. }) => "State::PistonAvailability".into(),
        LayerInputSource::BoundaryEvent { .. } => "BoundaryEvent".into(),
        LayerInputSource::BoundaryRequest { .. } => "BoundaryRequest".into(),
        LayerInputSource::BoundaryState { .. } => "BoundaryState".into(),
        LayerInputSource::PowerContribution { .. } => "PowerContribution".into(),
        LayerInputSource::LocalResetOutput { .. } => "LocalResetOutput".into(),
    }
}

fn owner_kind(register: bool, cell: bool) -> &'static str {
    match (register, cell) {
        (true, true) => "register+cell",
        (true, false) => "register",
        (false, true) => "cell",
        (false, false) => "unclassified",
    }
}
