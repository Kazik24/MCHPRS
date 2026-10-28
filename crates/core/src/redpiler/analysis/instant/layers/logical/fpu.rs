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

    // These are state leaves rejected by the first output pass. Dump their physical
    // ownership and transfer contracts so we can decide whether each is a real latch
    // or merely a reset/callback artifact before adding it to a logical state model.
    for resource in [0u32, 2, 6, 9, 13, 16, 20, 23, 27, 31, 4970] {
        let resource = ResourceId::new(resource);
        let spec = &graph.resources[resource.index()];
        println!("FPU feedback resource {resource:?}: material={:?}", spec.material);
        let touching_actors = graph
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(index, piston)| {
                spec.contacts
                    .iter()
                    .any(|contact| [piston.base, piston.near, piston.far].contains(contact))
                    .then_some(PistonId::new(index as u32))
            })
            .collect::<FxHashSet<_>>();
        let associated_declines = catalog
            .declines
            .iter()
            .filter(|decline| decline.actors.iter().any(|actor| touching_actors.contains(actor)))
            .collect::<Vec<_>>();
        let mut group_actors = touching_actors.clone();
        for decline in &associated_declines {
            group_actors.extend(decline.actors.iter().copied());
            println!("  associated cell decline: {decline:#?}");
        }
        if resource == ResourceId::new(4970) {
            for actor in group_actors {
                let piston = &graph.pistons[actor.index()];
                let missing_reset = associated_declines.iter().any(|decline| {
                    matches!(decline.reason, crate::redpiler::program_graph::CellDeclineReason::UnrecognizedActor(missing) if missing == actor)
                });
                println!(
                    "  group actor {actor:?}@{:?}: facing={:?} sticky={} base={:?} near={:?} far={:?} lane={:?} recognized_reset_witness={}",
                    piston.position,
                    piston.facing,
                    piston.sticky,
                    piston.base,
                    piston.near,
                    piston.far,
                    piston.lane,
                    if missing_reset { "missing" } else { "not identified by group decline" },
                );
                for link in graph
                    .power_links
                    .iter()
                    .filter(|link| matches!(link.to, PowerTarget::Input { node, .. } if node == piston.node))
                {
                    let source = &graph.sites[link.from.index()];
                    let port = match link.to {
                        PowerTarget::Input { port, .. } => port,
                        _ => unreachable!(),
                    };
                    println!(
                        "    input {port:?} <- {:?}@{:?} source_port={:?} attenuation={}",
                        source.source, source.position, link.from_port, link.attenuation,
                    );
                }
                let contacts = [piston.base, piston.near, piston.far];
                let observers = graph
                    .nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, node)| {
                        let watched = match node.kind {
                            CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => watched,
                            _ => return None,
                        };
                        let Some(SiteSource::Contact(contact)) = graph.sites.get(watched.index()).map(|site| site.source) else {
                            return None;
                        };
                        contacts
                            .contains(&contact)
                            .then_some((NodeId::new(index as u32), node.position, contact, &node.kind))
                    })
                    .collect::<Vec<_>>();
                println!("    observers watching actor contacts (reset candidates): {observers:#?}");
            }
        }
        for (cell_index, cell) in catalog.cells.iter().enumerate() {
            for pin in cell.resources.iter().filter(|pin| pin.resource == resource) {
                println!(
                    "  cell {cell_index} resource contract: source={:?} owners={:?} present_pin={:?} absent_pin={:?} pins={:#?}",
                    pin.source, pin.owners, pin.present, pin.absent, cell.pins,
                );
                for use_ in cell.uses.iter().filter(|use_| pin.owners.contains(&use_.actor)) {
                    let actor = graph.pistons[use_.actor.index()];
                    println!(
                        "    use actor={:?}@{:?} {:?} powered={:?} requested={:?} valid={:?} reset={:#?} retraction={:#?}",
                        use_.actor, actor.position, actor.facing, use_.powered, use_.requested, use_.valid, use_.reset, use_.retraction,
                    );
                    if let CellReset::Observer { node, wake } = &use_.reset {
                        let observer = &graph.nodes[node.index()];
                        println!(
                            "      reset observer {node:?}@{:?} kind={:?} state={:?} wake={wake:#?}",
                            observer.position, observer.kind, observer.state,
                        );
                    }
                }
            }
        }
        for &contact in &spec.contacts {
            let contact_spec = &graph.contacts[contact.index()];
            let node = &graph.nodes[contact_spec.node.index()];
            println!(
                "  contact {contact:?} at {:?}, node {:?} {:?}, variants={:?}",
                contact_spec.position, contact_spec.node, node.kind, contact_spec.variants,
            );
            for (actor_index, piston) in graph
                .pistons
                .iter()
                .enumerate()
                .filter(|(_, piston)| piston.base == contact || piston.near == contact || piston.far == contact)
            {
                let actor = PistonId::new(actor_index as u32);
                let owner = catalog
                    .cells
                    .iter()
                    .enumerate()
                    .find_map(|(cell_index, cell)| cell.resources.iter().find(|pin| pin.resource == resource).map(|pin| (cell_index, pin)));
                println!(
                    "    piston {actor:?} at {:?} {:?} lane={:?} resource_cell={:?}",
                    piston.position,
                    piston.facing,
                    piston.lane,
                    owner.map(|(cell, pin)| (cell, pin.source, pin.owners.clone())),
                );
            }
        }
    }

    let mut equations = equations;
    let mut all_leaves = FxHashSet::default();
    let mut failures = BTreeMap::<String, Vec<String>>::new();
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .expect("manifest final dust output positions");
    let mut admitted = 0;
    let mut public_targets = Vec::new();
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
        public_targets.push(PowerTarget::Input {
            node: node_id,
            port: InputPort::Main,
        });
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
    match timing::infer(graph, &monitor) {
        Ok(timing) => println!(
            "FPU inferred timing island: clock={:?} trigger={:?} actors={:#?} nodes={:#?}",
            timing.clock, timing.trigger, timing.actor_specs, timing.node_specs,
        ),
        Err(error) => println!("FPU timing-island inference declined: {error}"),
    }
    match diagnostic_computation_failures(graph, &public_targets, &monitor) {
        Err(error) => println!("FPU compact diagnostic failed: {error}"),
        Ok(diagnostic) => {
            println!(
                "FPU diagnostic builders: raw=(expr {}, inputs {}, work {}, bytes {}), reduced=(expr {}, inputs {}, work {}, bytes {})",
                diagnostic.raw_expressions,
                diagnostic.raw_inputs,
                diagnostic.raw_work,
                diagnostic.raw_bytes,
                diagnostic.reduced_expressions,
                diagnostic.reduced_inputs,
                diagnostic.reduced_work,
                diagnostic.reduced_bytes,
            );
            println!("FPU compact computation blockers: {:#?}", diagnostic.failures);
            println!("FPU unresolved logical resource leaves: {:#?}", diagnostic.unresolved_resources);
            for resource in diagnostic.unresolved_resources {
                dump_unresolved_resource(graph, catalog, resource);
            }
            match compile_computation(graph, &public_targets, &monitor) {
                Ok(circuit) => {
                    let state = circuit
                        .outputs
                        .iter()
                        .filter(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
                        .count();
                    println!(
                        "FPU compact logical computation: inputs={} expressions={} outputs={} public={} resource_states={} retained_effects={} declines={}",
                        circuit.program.inputs.len(),
                        circuit.program.expressions.len(),
                        circuit.outputs.len(),
                        public_targets.len(),
                        state,
                        circuit.retained_effects.len(),
                        circuit.declines.len(),
                    );
                    println!(
                        "FPU compact inputs: {:#?}",
                        circuit.program.inputs.iter().map(|input| input.source).collect::<Vec<_>>()
                    );
                    println!(
                        "FPU compact state targets: {:#?}",
                        circuit
                            .outputs
                            .iter()
                            .filter_map(|output| match output.target {
                                SettledTarget::ResourceState { resource, source } => Some((resource, source)),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    );
                }
                Err(error) => println!("FPU compact logical computation declined: {error}"),
            }
        }
    }
}

fn dump_unresolved_resource(graph: &CompileGraph, catalog: &BoundaryCatalog, resource: ResourceId) {
    let Some(spec) = graph.resources.get(resource.index()) else {
        println!("FPU unresolved resource {resource:?} is missing from the graph");
        return;
    };
    println!("FPU newly discovered unresolved resource {resource:?}: material={:?}", spec.material);
    for &contact in &spec.contacts {
        let contact_spec = &graph.contacts[contact.index()];
        println!("  contact {contact:?}@{:?}: {:?}", contact_spec.position, contact_spec.variants);
    }
    let touching = graph
        .pistons
        .iter()
        .enumerate()
        .filter_map(|(index, piston)| {
            spec.contacts
                .iter()
                .any(|contact| [piston.base, piston.near, piston.far].contains(contact))
                .then_some((PistonId::new(index as u32), piston))
        })
        .collect::<Vec<_>>();
    for (actor, piston) in &touching {
        println!("  touching owner {actor:?}: {piston:#?}");
    }
    for cell in catalog
        .cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.resources.iter().any(|pin| pin.resource == resource))
    {
        println!("  recognized cell: {cell:#?}");
    }
    let actors = touching.iter().map(|(actor, _)| *actor).collect::<FxHashSet<_>>();
    for decline in catalog
        .declines
        .iter()
        .filter(|decline| decline.actors.iter().any(|actor| actors.contains(actor)))
    {
        println!("  associated cell decline: {decline:#?}");
        if matches!(decline.reason, crate::redpiler::program_graph::CellDeclineReason::UnrecognizedActor(_)) {
            println!(
                "  reset witness: none admitted; cell discovery requires a recognized reset provenance or delayed payload-restoration witness for every actor"
            );
        }
    }
    for (node_index, node) in graph.nodes.iter().enumerate() {
        let watched_contact = match node.kind {
            CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                graph.sites.get(watched.index()).and_then(|site| match site.source {
                    SiteSource::Contact(contact) if spec.contacts.contains(&contact) => Some(contact),
                    _ => None,
                })
            }
            _ => None,
        };
        if let Some(contact) = watched_contact {
            println!(
                "  possible reset observer {:?}@{:?} watches resource contact {contact:?}; kind={:?} state={:?}",
                NodeId::new(node_index as u32),
                node.position,
                node.kind,
                node.state,
            );
        }
    }
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
        LayerInputSource::WireDot { .. } => "WireDot".into(),
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
