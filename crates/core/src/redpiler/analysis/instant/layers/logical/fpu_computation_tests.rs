//! Native tick-by-tick diagnostic for the FPU computation cone.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    BoundaryDirection, BoundaryOwner, BoundaryPortKind, CallbackEdgeRef, CellReset, ContactId, InputPort, LayerInputSource, LinkSpec, PowerPort,
    PowerTarget, ResourceId, SettledCircuit, SettledTarget, SiteSource, StateTest,
};
use crate::redpiler::{CompilerOptions, LogicalDeviceAdmission, passes};
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, LeverFace, RedstoneRepeater};
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::BTreeMap;

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const SEED: u64 = 28_192_451_288_813;
const INPUT_TICKS: usize = 64;
const INITIAL_TICKS: usize = 1024;

pub(super) fn fixture() -> (serde_json::Value, PlotWorld, (BlockPos, BlockPos)) {
    fixture_from_descriptor(fixture_descriptor())
}

fn fixture_descriptor() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/fixtures/fpu_legal.json");
    let mut descriptor: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    descriptor["origin"] = serde_json::json!([ORIGIN.x, ORIGIN.y, ORIGIN.z]);
    descriptor
}

fn fixture_from_descriptor(descriptor: serde_json::Value) -> (serde_json::Value, PlotWorld, (BlockPos, BlockPos)) {
    let world = super::cpus::load_manifest(&descriptor).unwrap();
    let dimensions = &descriptor["dimensions"];
    let bounds = (
        ORIGIN,
        ORIGIN
            + BlockPos::new(
                dimensions[0].as_i64().unwrap() as i32 - 1,
                dimensions[1].as_i64().unwrap() as i32 - 1,
                dimensions[2].as_i64().unwrap() as i32 - 1,
            ),
    );
    (descriptor, world, bounds)
}

pub(super) fn pos(value: &serde_json::Value) -> BlockPos {
    ORIGIN
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
}

pub(super) fn set_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("FPU input is not a lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(support));
}

fn assert_fpu_retained_cells(graph: &CompileGraph, circuit: &SettledCircuit) {
    let catalog = graph.plans.instant.boundaries.as_ref().expect("FPU boundary catalog is present");
    let mut output_actors = FxHashSet::default();
    for effect in circuit.retained_effects.iter() {
        let (BoundaryOwner::Cell(cell), BoundaryPortKind::Power { link }) = (effect.owner, effect.kind) else {
            continue;
        };
        if effect.direction != BoundaryDirection::Output {
            continue;
        }
        let Some(link) = graph.power_links.get(link as usize) else {
            continue;
        };
        let PowerTarget::Input { node, port: InputPort::Main } = link.to else {
            continue;
        };
        if !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })) {
            continue;
        }
        if let Some(contract) = catalog.cells.get(cell as usize) {
            output_actors.extend(contract.uses.iter().map(|use_| use_.actor));
        }
    }
    assert_eq!(output_actors.len(), 3, "the FPU must retain its three certified command-output cells");

    let actual_actors = circuit.retained_cells.iter().map(|use_| use_.actor).collect::<FxHashSet<_>>();
    assert!(
        output_actors.is_subset(&actual_actors),
        "all three certified command-output actors must remain Graph-owned"
    );
    assert_eq!(actual_actors.len(), circuit.retained_cells.len(), "an actor may be retained only once");

    let feedback: FxHashSet<_> = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source)),
            _ => None,
        })
        .collect();
    let catalog_owner_cells = |actor| {
        catalog
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.uses.iter().any(|use_| use_.actor == actor))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let monitor = crate::redpiler::TaskMonitor::default();
    for actor in actual_actors.difference(&output_actors).copied() {
        assert!(!feedback.is_empty(), "an extra retained actor requires a ResourceState output");
        let cells = catalog_owner_cells(actor);
        assert_eq!(cells.len(), 1, "extra retained actor must map to one unique catalog cell");
        let cell_index = cells[0];
        let cell = &catalog.cells[cell_index];
        assert_eq!(cell.uses.len(), 1, "feedback retention only admits single-use cells");
        assert_eq!(cell.resources.len(), 1, "feedback retention only admits a private resource pin");
        let use_ = &cell.uses[0];
        let pin = &cell.resources[0];
        let piston = &graph.pistons[actor.index()];
        assert_eq!(use_.actor, actor);
        assert_eq!(pin.owners.as_ref(), [actor]);
        assert!(
            feedback.contains(&(pin.resource, pin.source)),
            "retained resource must have a matching feedback output"
        );
        assert_eq!(
            catalog
                .cells
                .iter()
                .filter(|candidate| candidate.resources.iter().any(|candidate_pin| candidate_pin.resource == pin.resource))
                .count(),
            1,
            "the feedback resource must have one complete catalog owner cell"
        );
        let transfer = use_.retraction.transfer.expect("feedback cell has a certified transfer");
        assert_eq!(
            (transfer.resource, transfer.source, transfer.destination),
            (pin.resource, pin.source, piston.near)
        );
        assert!(use_.reset.observer().is_some(), "the cell retains its reset observer");
        assert!(!use_.activations.is_empty(), "the retained owner has certified activation routes");
        assert!(piston.sticky && matches!(piston.lane, PistonLane::SingleResource { far } if far == pin.source));
        let resource = &graph.resources[pin.resource.index()];
        assert_eq!(resource.contacts.len(), 2, "private feedback uses an exact two-contact resource lane");
        assert!(resource.contacts.contains(&pin.source) && resource.contacts.contains(&piston.near));

        let closure = crate::redpiler::analysis::instant::ownership::close_seeded_ownership(graph, &[], &[actor], &monitor)
            .expect("physical ownership closure for retained feedback actor");
        let group_index = closure.actors[actor.index()].expect("retained actor has a physical ownership group");
        let group = &closure.groups[group_index];
        assert_eq!(group.actors.as_slice(), [actor], "feedback actor cannot share its physical group");
        assert_eq!(
            group.resources.as_slice(),
            [pin.resource],
            "feedback resource cannot share its physical group"
        );

        let output_effects = catalog
            .ports
            .iter()
            .filter(|port| port.owner == BoundaryOwner::Cell(cell_index as u32) && port.direction == BoundaryDirection::Output)
            .collect::<Vec<_>>();
        assert!(!output_effects.is_empty(), "feedback cell must have certified output effects");
        for effect in output_effects {
            assert!(
                circuit.retained_effects.contains(effect),
                "all certified outputs of an extra feedback cell must remain native effects"
            );
        }
    }
}

fn add_foreign_observer_reset_source(graph: &mut CompileGraph, circuit: &crate::redpiler::program_graph::SettledCircuit) {
    let use_ = circuit.retained_cells.first().expect("FPU frame retains an output cell");
    let CellReset::Observer { node: reset, .. } = &use_.reset else {
        panic!("FPU output cell must use an observer reset");
    };
    let reset = *reset;
    let actor_node = graph.pistons[use_.actor.index()].node;
    let reset_link = graph
        .power_links
        .iter()
        .copied()
        .find(|link| {
            matches!(
                link.to,
                PowerTarget::Input {
                    node,
                    port: InputPort::Main | InputPort::Quasi
                } if node == actor_node
            ) && matches!(graph.sites[link.from.index()].source, SiteSource::Component(node) if node == reset)
        })
        .expect("retained observer reset directly powers its actor input");
    let other_reset = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (NodeId::new(index as u32), &node.kind))
        .find_map(|(node, kind)| {
            (node != reset
                && matches!(
                    kind,
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
                ))
            .then_some(node)
        })
        .expect("FPU contains another observer reset");
    let source = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(other_reset))
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .expect("other observer has a power source site");
    graph
        .power_links
        .push(crate::redpiler::program_graph::PowerLink { from: source, ..reset_link });
}

fn add_command_success_output_sink(graph: &mut CompileGraph, circuit: &crate::redpiler::program_graph::SettledCircuit) {
    let command = circuit
        .retained_effects
        .iter()
        .find_map(|effect| match effect.kind {
            crate::redpiler::program_graph::BoundaryPortKind::Power { link } => match graph.power_links[link as usize].to {
                PowerTarget::Input { node, port: InputPort::Main }
                    if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })) =>
                {
                    Some(node)
                }
                _ => None,
            },
            _ => None,
        })
        .expect("FPU frame retains a command power output");
    let sink = graph
        .nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })).then_some(NodeId::new(index as u32)))
        .expect("FPU contains an observable comparator sink");
    let sink_site = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(sink))
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .expect("comparator has a callback site");
    let mut delivery = graph
        .routes
        .iter()
        .flat_map(|route| route.deliveries.iter())
        .find(|delivery| {
            matches!(delivery.kind, crate::redpiler::program_graph::DeliveryKind::Update { .. })
                && matches!(delivery.receiver, Some(crate::redpiler::program_graph::CallbackReceiver::Component(_)))
        })
        .copied()
        .expect("FPU has a component update delivery");
    delivery.target = sink_site;
    delivery.receiver = Some(crate::redpiler::program_graph::CallbackReceiver::Component(sink));
    delivery.guard = crate::redpiler::program_graph::GuardId::new(0);
    delivery.view = crate::redpiler::program_graph::OperandView::Live;

    let route = if let Some(route) = graph.nodes[command.index()].output_route {
        route
    } else {
        let route = crate::redpiler::program_graph::RouteId::new(graph.routes.len() as u32);
        graph.routes.push(crate::redpiler::program_graph::Route {
            source: command,
            deliveries: Box::new([]),
        });
        graph.nodes[command.index()].output_route = Some(route);
        route
    };
    {
        let deliveries = &mut graph.routes[route.index()].deliveries;
        delivery.sequence = deliveries.last().map_or(0, |last| last.sequence.saturating_add(1));
        let mut extended = deliveries.to_vec();
        extended.push(delivery);
        *deliveries = extended.into_boxed_slice();
    }
    graph.links.push(LinkSpec {
        from: command,
        to: sink,
        kind: delivery.link_kind(),
        callback: CallbackEdgeRef {
            route,
            sequence: delivery.sequence,
        },
    });
}

fn evaluations_after_trigger_tick(
    graph: &mut CompileGraph,
    entry: &crate::redpiler::program_graph::EntryState,
    options: &CompilerOptions,
    world: &mut PlotWorld,
    trigger: BlockPos,
    monitor: &TaskMonitor,
    require_admitted_frame: bool,
) -> u64 {
    use crate::redpiler::backend::graph::GraphBackend;

    graph.clear_execution_plans();
    let mut pipeline = passes::default_graph_pipeline(options);
    pipeline.entries.retain(|entry| matches!(&entry.pass, passes::Pass::Build(_)));
    passes::run_graph_pipeline(graph, options, entry, &pipeline, monitor).unwrap();
    if require_admitted_frame {
        let frame = graph
            .plans
            .settled_circuit
            .as_ref()
            .expect("positive control must rebuild the settled frame");
        assert!(frame.timing.is_some(), "positive control frame must retain timing");
        assert_fpu_retained_cells(graph, frame);
    }
    let (program, state) = graph.clone().lower().unwrap();
    let lowered = crate::redpiler::program_graph::LoweredCircuit {
        positions: program.position_index(),
        program,
        state,
        logical_tick: 0,
        imported_ticks: Vec::new(),
    };
    let trigger_node = lowered
        .program
        .nodes()
        .iter()
        .position(|node| node.position == Some(trigger))
        .expect("trigger lever must be in the lowered graph");
    let trigger_powered = match &lowered.state.components[trigger_node] {
        crate::redpiler::program_graph::ComponentState::Electrical(component) => component.powered,
        _ => panic!("trigger must lower as an electrical component"),
    };
    let mut backend = GraphBackend::from_lowered(lowered, monitor).unwrap();
    if trigger_powered {
        backend.use_block(trigger).unwrap();
    }
    backend.tick_with_world(world).unwrap();
    backend.execution_counts().logical_kernel_evaluations
}

fn assert_hybrid_falls_back(
    graph: &mut CompileGraph,
    entry: &crate::redpiler::program_graph::EntryState,
    options: &CompilerOptions,
    world: &mut PlotWorld,
    trigger: BlockPos,
    monitor: &TaskMonitor,
) {
    assert_eq!(
        evaluations_after_trigger_tick(graph, entry, options, world, trigger, monitor, false),
        0,
        "unsafe hybrid frame must not install"
    );
}

pub(super) fn stimuli(initial: u64, count: usize) -> Vec<u64> {
    let mask = (1u64 << 35) - 1;
    let mut state = SEED;
    let mut previous = initial & mask;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let word = state & mask;
            let word = if word == previous { word ^ 1 } else { word };
            previous = word;
            word
        })
        .collect()
}

#[test]
#[ignore = "production O6 stable-input FPU compile-only regression; no execution replay"]
fn compile_fpu_o6_stable_io_only() {
    let (_, world, bounds) = fixture();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        io_only: true,
        update: false,
    };
    let prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let validated = prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .unwrap();
    let graph = validated.graph();
    graph.validate_for_lowering(&monitor).unwrap();
    let circuit = compile_settled(graph, &monitor).expect("production settled FPU extraction should complete");
    assert!(!circuit.outputs.is_empty(), "production settled extraction must retain FPU outputs");
    assert!(
        circuit.declines.is_empty(),
        "production settled extraction declined: {:#?}",
        circuit.declines
    );
}

#[test]
#[ignore = "production O6 FPU readout-repeater frame regression"]
fn compile_fpu_o6_terminal_repeater_readouts() {
    let (descriptor, world, bounds) = fixture();
    run_fpu_o6_terminal_repeater_readouts(descriptor, world, bounds);
}

#[test]
#[ignore = "production O6 FPU readout-repeater frame regression on the fixed saved schematic"]
fn compile_fpu_o6_terminal_repeater_readouts_fixed_schematic() {
    let mut descriptor = fixture_descriptor();
    descriptor["fixture"] = serde_json::json!("target/fpu-fixed-investigation/Lord225@FPU_FIXED_INSTANT.schem");
    descriptor["sha256"] = serde_json::json!("41d1cb4e5c3e5a2bd00d83ccf859969f9fe2ac7b1c0ab64070e685fa26388f33");
    let (descriptor, world, bounds) = fixture_from_descriptor(descriptor);
    run_fpu_o6_terminal_repeater_readouts(descriptor, world, bounds);
}

#[test]
fn declined_scoped_frame_warning_links_its_source() {
    let (descriptor, mut world, (min, max)) = fixture();
    let readout = pos(&descriptor["observations"]["output"][0]);
    world.set_block(readout.offset(BlockFace::East), Block::RedstoneLamp { lit: false });
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        io_only: true,
        ..Default::default()
    };
    let prepared = passes::rich::prepare(
        &world,
        (min, max + BlockPos::new(1, 0, 0)),
        vec![],
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let validated = prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .unwrap();
    let warnings = monitor.warnings();
    let example = warnings
        .iter()
        .find(|warning| warning.starts_with("Instant circuit candidate at ("))
        .expect("candidate warning has a physical source");
    let message = crate::player::diagnostic_component(example);
    assert!(
        message["extra"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| part["click_event"]["command"].as_str().is_some_and(|command| command.starts_with("/hl ")))
    );
    assert!(!validated.graph().nodes.is_empty());
}

fn run_fpu_o6_terminal_repeater_readouts(descriptor: serde_json::Value, mut world: PlotWorld, (min, max): (BlockPos, BlockPos)) {
    let controls = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    let witness_word = (0xffffu64 << 16) | (0b110u64 << 32);
    let readouts = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(readouts.len(), 16);
    for position in &readouts {
        let repeater = position.offset(BlockFace::East);
        world.set_block(
            repeater,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: mchprs_blocks::BlockDirection::West,
                    delay: 1,
                    powered: false,
                    locked: false,
                },
            },
        );
    }
    let bounds = (min, max + BlockPos::new(1, 0, 0));
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        io_only: true,
        update: false,
    };
    let prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let validated = prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .unwrap();
    let graph = validated.graph();
    let targets = super::settled::frame_power_targets(graph);
    let repeater_roots: FxHashSet<_> = targets
        .iter()
        .filter_map(|target| match target {
            PowerTarget::Input { node, port: InputPort::Main }
                if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) =>
            {
                Some(*node)
            }
            _ => None,
        })
        .collect();
    let manifest_repeater_roots: FxHashSet<_> = readouts
        .iter()
        .map(|position| {
            graph
                .nodes
                .iter()
                .position(|node| node.position == Some(position.offset(BlockFace::East)))
                .unwrap()
        })
        .map(|index| NodeId::new(index as u32))
        .collect();
    assert_eq!(manifest_repeater_roots.len(), 16);
    assert!(
        manifest_repeater_roots.is_subset(&repeater_roots),
        "all 16 manifest readout repeaters must become delayed frame boundaries; extracted {} repeaters",
        repeater_roots.len()
    );
    assert_eq!(
        repeater_roots.len(),
        16,
        "internal repeaters with semantic output callbacks are not public frame boundaries"
    );
    let wire_roots: FxHashSet<_> = targets
        .iter()
        .filter_map(|target| match target {
            PowerTarget::Input { node, port: InputPort::Main } if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => Some(*node),
            _ => None,
        })
        .collect();
    for repeater in &manifest_repeater_roots {
        assert!(
            graph.power_links.iter().any(|link| {
                link.to
                    == PowerTarget::Input {
                        node: *repeater,
                        port: InputPort::Main,
                    }
                    && matches!(graph.sites[link.from.index()].source, SiteSource::Component(wire) if wire_roots.contains(&wire))
            }),
            "the computed public boundary must include the upstream readout wire for {repeater:?}"
        );
    }
    let circuit = super::settled::compile_device(graph, &monitor).expect("FPU readout frame extraction should complete");
    assert!(circuit.declines.is_empty(), "terminal repeater frame declined: {:#?}", circuit.declines);
    assert!(circuit.timing.is_some(), "FPU readout frame must retain its certified clock");
    assert_fpu_retained_cells(graph, &circuit);
    assert!(
        circuit
            .outputs
            .iter()
            .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
    );
    assert!(
        repeater_roots.iter().all(|node| circuit.outputs.iter().any(|output| {
            output.target
                == SettledTarget::Power(PowerTarget::Input {
                    node: *node,
                    port: InputPort::Main,
                })
        })),
        "all 16 repeater inputs must be bound into the frame equations"
    );
    let scoped = super::settled::compile_scoped_devices(graph, &monitor).expect("scoped FPU extraction should complete");
    let output_wire = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(readouts[0]))
        .map(|index| NodeId::new(index as u32))
        .expect("first FPU output wire exists in the graph");
    assert!(
        scoped.iter().any(|device| {
            device.scope.owned_nodes.binary_search(&output_wire).is_ok()
                && device
                    .circuit
                    .outputs
                    .iter()
                    .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
        }),
        "the complete FPU readout island must remain admitted"
    );

    // A lamp attached to the same public output makes this island own an
    // ordinary native electrical sink. The frame must decline the whole
    // interaction island rather than suppressing the lamp's update route.
    let mut lamp_world = super::cpus::load_manifest(&descriptor).unwrap();
    lamp_world.set_block(readouts[0].offset(BlockFace::East), Block::RedstoneLamp { lit: false });
    let lamp_bounds = (min, max + BlockPos::new(1, 0, 0));
    let lamp_prepared = passes::rich::prepare(&lamp_world, lamp_bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let lamp_validated = lamp_prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .unwrap();
    let lamp_graph = lamp_validated.graph();
    let lamp = lamp_graph
        .nodes
        .iter()
        .position(|node| node.position == Some(readouts[0].offset(BlockFace::East)))
        .map(|index| NodeId::new(index as u32))
        .expect("attached lamp exists in the graph");
    assert!(
        lamp_graph.power_links.iter().any(|link| {
            link.to == PowerTarget::Input { node: lamp, port: InputPort::Main }
                && matches!(lamp_graph.sites[link.from.index()].source, SiteSource::Component(node) if matches!(lamp_graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)))
        }),
        "fixture mutation must create a real wire-to-lamp electrical sink"
    );
    let lamp_partition = super::islands::partition(lamp_graph, &monitor).expect("lamp island partition should complete");
    let lamp_component = lamp_partition.node_island[lamp.index()];
    let lamp_devices = super::settled::compile_scoped_devices(lamp_graph, &monitor).expect("lamp scoped extraction should complete");
    assert!(
        lamp_devices.iter().all(|device| device.scope.component != lamp_component),
        "an ordinary terminal lamp must keep its connected FPU island on Graph"
    );
    let owned_nodes: FxHashSet<_> = scoped.iter().flat_map(|device| device.scope.owned_nodes.iter().copied()).collect();
    let control_positions = controls
        .iter()
        .copied()
        .chain(std::iter::once(pos(&descriptor["ports"]["trigger"])))
        .collect::<Vec<_>>();
    let output_positions: FxHashSet<_> = readouts
        .iter()
        .copied()
        .chain(readouts.iter().map(|position| position.offset(BlockFace::East)))
        .collect();
    let mut retained_native_positions = FxHashSet::default();
    let mut retained_reset_nodes = FxHashSet::default();
    for use_ in circuit.retained_cells.iter() {
        let piston = &graph.pistons[use_.actor.index()];
        retained_native_positions.insert(piston.position);
        for contact in [piston.base, piston.near, piston.far] {
            retained_native_positions.insert(graph.contacts[contact.index()].position);
        }
        if let Some(transfer) = use_.retraction.transfer {
            for contact in [transfer.source, transfer.destination] {
                retained_native_positions.insert(graph.contacts[contact.index()].position);
            }
            for &contact in graph.resources[transfer.resource.index()].contacts.iter() {
                retained_native_positions.insert(graph.contacts[contact.index()].position);
            }
        }
        if let CellReset::Observer { node, .. } = &use_.reset {
            retained_reset_nodes.insert(*node);
            if let Some(position) = graph.nodes[node.index()].position {
                retained_native_positions.insert(position);
            }
        }
    }
    let native_positions: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| !owned_nodes.contains(&NodeId::new(*index as u32)))
        .filter_map(|(_, node)| node.position)
        .filter(|position| !output_positions.contains(position))
        .chain(control_positions.iter().copied())
        .chain(retained_native_positions.iter().copied())
        .filter(|position| !output_positions.contains(position))
        .collect();
    let observer_nodes = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| !owned_nodes.contains(&NodeId::new(*index as u32)))
        .filter(|(_, node)| {
            matches!(
                node.kind,
                CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
            )
        })
        .map(|(index, _)| NodeId::new(index as u32))
        .chain(retained_reset_nodes.iter().copied())
        .collect::<FxHashSet<_>>();
    let observer_positions = observer_nodes
        .iter()
        .filter_map(|node| graph.nodes[node.index()].position)
        .collect::<FxHashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    assert!(
        !observer_positions.is_empty(),
        "the native FPU input cone must include observers or compiled reset observers"
    );
    let lamp_positions = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| !owned_nodes.contains(&NodeId::new(*index as u32)))
        .filter(|(_, node)| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
        .filter_map(|(_, node)| node.position)
        .collect::<Vec<_>>();
    let initial_lamp_blocks = lamp_positions.iter().map(|&position| world.get_block(position)).collect::<Vec<_>>();
    let lowered = validated.lower(&monitor).unwrap();
    assert!(
        lowered.imported_ticks.is_empty(),
        "the fixture supplies no pending ticks to the native baseline"
    );
    let mut physical = GraphBackend::new_unactivated(lowered.program.clone(), lowered.state.clone(), lowered.logical_tick)
        .expect("ordinary Graph FPU baseline should initialize from the same compiled program and state");
    let backend = GraphBackend::from_lowered(lowered, &monitor).expect("FPU readout backend should lower");
    assert_eq!(backend.logical_device_admission(), &LogicalDeviceAdmission::Active);

    let mut backend = backend;
    let mut physical_world = super::cpus::load_manifest(&descriptor).unwrap();
    for position in &readouts {
        physical_world.set_block(
            position.offset(BlockFace::East),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: mchprs_blocks::BlockDirection::West,
                    delay: 1,
                    powered: false,
                    locked: false,
                },
            },
        );
    }
    let mut saw_lamp_transition = false;
    let mut saw_observer_on = false;
    let mut saw_observer_off_after_on = false;
    let mut observer_was_on = observer_positions
        .iter()
        .map(|&position| matches!(world.get_block(position), Block::Observer { observer } if observer.powered))
        .collect::<Vec<_>>();
    let mut compare_native_state = |tick: usize, world: &PlotWorld, physical_world: &PlotWorld| {
        for &position in &native_positions {
            assert_eq!(
                world.get_block(position),
                physical_world.get_block(position),
                "native FPU projection at {position:?}, tick {tick}"
            );
        }
        for (lamp_index, &position) in lamp_positions.iter().enumerate() {
            let projected = world.get_block(position);
            assert_eq!(
                projected,
                physical_world.get_block(position),
                "FPU indicator at {position:?}, tick {tick}"
            );
            saw_lamp_transition |= projected != initial_lamp_blocks[lamp_index];
        }
        for (observer_index, &position) in observer_positions.iter().enumerate() {
            let on = matches!(world.get_block(position), Block::Observer { observer } if observer.powered);
            assert_eq!(
                on,
                matches!(physical_world.get_block(position), Block::Observer { observer } if observer.powered)
            );
            saw_observer_on |= on;
            saw_observer_off_after_on |= observer_was_on[observer_index] && !on;
            observer_was_on[observer_index] = on;
        }
    };
    let trigger = pos(&descriptor["ports"]["trigger"]);
    let mut saw_output_high = false;
    let mut saw_readout_repeater_on = false;
    for (bit, &position) in controls.iter().enumerate() {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("FPU input is not a lever at {position:?}");
        };
        if lever.powered != (witness_word & (1 << bit) != 0) {
            backend.use_block(position).unwrap();
            physical.use_block(position).unwrap();
        }
    }
    if !matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered) {
        backend.use_block(trigger).unwrap();
        physical.use_block(trigger).unwrap();
    }
    for tick in 0..64 {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        compare_native_state(tick, &world, &physical_world);
        saw_output_high |= readouts
            .iter()
            .any(|position| matches!(world.get_block(*position), Block::RedstoneWire { wire } if wire.power > 0));
        saw_readout_repeater_on |= readouts
            .iter()
            .any(|position| matches!(world.get_block(position.offset(BlockFace::East)), Block::RedstoneRepeater { repeater } if repeater.powered));
    }
    backend.use_block(trigger).unwrap();
    physical.use_block(trigger).unwrap();
    for offset in 0..25 {
        if offset == 24 {
            backend.use_block(trigger).unwrap();
            physical.use_block(trigger).unwrap();
        }
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        compare_native_state(64 + offset, &world, &physical_world);
        saw_output_high |= readouts
            .iter()
            .any(|position| matches!(world.get_block(*position), Block::RedstoneWire { wire } if wire.power > 0));
        saw_readout_repeater_on |= readouts
            .iter()
            .any(|position| matches!(world.get_block(position.offset(BlockFace::East)), Block::RedstoneRepeater { repeater } if repeater.powered));
    }
    for offset in 0..8 {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        compare_native_state(89 + offset, &world, &physical_world);
        saw_output_high |= readouts
            .iter()
            .any(|position| matches!(world.get_block(*position), Block::RedstoneWire { wire } if wire.power > 0));
        saw_readout_repeater_on |= readouts
            .iter()
            .any(|position| matches!(world.get_block(position.offset(BlockFace::East)), Block::RedstoneRepeater { repeater } if repeater.powered));
    }
    assert!(
        backend.execution_counts().logical_kernel_evaluations > 0,
        "the admitted readout frame must execute during the trigger protocol"
    );
    assert!(
        backend.execution_counts().wire_walks < physical.execution_counts().wire_walks,
        "the readout frame should reduce wire walks against ordinary Graph for the same trigger and input protocol: candidate={} physical={}",
        backend.execution_counts().wire_walks,
        physical.execution_counts().wire_walks
    );
    assert!(saw_output_high, "the chosen FPU input vector must exercise a nonzero output wire");
    assert!(
        saw_readout_repeater_on,
        "the native terminal repeaters must publish a computed high output"
    );
    for output in &readouts {
        let input_powered = matches!(world.get_block(*output), Block::RedstoneWire { wire } if wire.power > 0);
        let repeater = output.offset(BlockFace::East);
        assert!(
            matches!(world.get_block(repeater), Block::RedstoneRepeater { repeater } if repeater.powered == input_powered),
            "terminal readout repeater at {repeater:?} must follow its published input wire after the native delay"
        );
    }

    // Compare a deterministic sequence of complete stable frames against ordinary Graph.
    // Inputs change only at the six-GT frame boundary, after each prior frame has published.
    backend.use_block(trigger).unwrap();
    physical.use_block(trigger).unwrap();
    for tick in 0..24 {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        compare_native_state(200 + tick, &world, &physical_world);
    }

    let input_mask = if controls.len() == u64::BITS as usize {
        u64::MAX
    } else {
        (1u64 << controls.len()) - 1
    };
    let mut random_state = 0x9e37_79b9_7f4a_7c15u64;
    let mut frame_inputs = Vec::with_capacity(16);
    for _ in 0..16 {
        random_state ^= random_state << 13;
        random_state ^= random_state >> 7;
        random_state ^= random_state << 17;
        frame_inputs.push(random_state & input_mask);
    }
    assert_eq!(frame_inputs.len(), 16);

    let apply_inputs = |backend: &mut GraphBackend, physical: &mut GraphBackend, world: &PlotWorld, physical_world: &PlotWorld, word: u64| {
        for (bit, &position) in controls.iter().enumerate() {
            let powered = matches!(world.get_block(position), Block::Lever { lever } if lever.powered);
            let physical_powered = matches!(physical_world.get_block(position), Block::Lever { lever } if lever.powered);
            assert_eq!(powered, physical_powered, "input lever differs before frame preparation at {position:?}");
            if powered != (word & (1u64 << bit) != 0) {
                backend.use_block(position).unwrap();
                physical.use_block(position).unwrap();
            }
        }
    };
    let repeater_word = |world: &PlotWorld| {
        readouts.iter().fold(0u16, |word, position| {
            let powered = matches!(
                world.get_block(position.offset(BlockFace::East)),
                Block::RedstoneRepeater { repeater } if repeater.powered
            );
            (word << 1) | u16::from(powered)
        })
    };

    // Prepare the first vector while stopped, then activate the trigger.
    apply_inputs(&mut backend, &mut physical, &world, &physical_world, frame_inputs[0]);
    backend.use_block(trigger).unwrap();
    physical.use_block(trigger).unwrap();
    for (frame, &word) in frame_inputs.iter().enumerate() {
        if frame > 0 {
            apply_inputs(&mut backend, &mut physical, &world, &physical_world, word);
        }
        for gt in 0..6 {
            backend.tick_with_world(&mut world).unwrap();
            backend.flush(&mut world, false);
            physical.tick_with_world(&mut physical_world).unwrap();
            physical.flush(&mut physical_world, false);
            compare_native_state(300 + frame * 6 + gt, &world, &physical_world);
        }
        assert_eq!(
            native_output_word(&world, &readouts),
            native_output_word(&physical_world, &readouts),
            "FPU output wires differ at the published end of randomized frame {frame}"
        );
        assert_eq!(
            repeater_word(&world),
            repeater_word(&physical_world),
            "FPU terminal repeater outputs differ at the end of randomized frame {frame}"
        );
    }

    backend.use_block(trigger).unwrap();
    physical.use_block(trigger).unwrap();
    for tick in 0..24 {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        compare_native_state(400 + tick, &world, &physical_world);
    }
    assert_eq!(
        native_output_word(&world, &readouts),
        native_output_word(&physical_world, &readouts),
        "FPU outputs differ after the randomized protocol drains"
    );
    assert_eq!(
        repeater_word(&world),
        repeater_word(&physical_world),
        "FPU terminal repeaters differ after the randomized protocol drains"
    );
    assert!(
        saw_observer_on && saw_observer_off_after_on,
        "native FPU observers must pulse and return off during the input protocol"
    );
    assert!(
        lamp_positions.is_empty() || saw_lamp_transition,
        "native FPU indicator lamps must show an exercised transition"
    );
}

pub(super) fn source_strength(graph: &CompileGraph, world: &PlotWorld, source: LayerInputSource) -> Option<bool> {
    let LayerInputSource::SourceAbove { site, port, threshold } = source else {
        return None;
    };
    let site = &graph.sites[site.index()];
    if matches!(site.source, SiteSource::Constant(_)) {
        return Some(site.source_strength > threshold);
    }
    let SiteSource::Component(node) = site.source else {
        return None;
    };
    if port != PowerPort::Output
        || !matches!(
            graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
        )
    {
        return None;
    }
    let position = graph.nodes[node.index()].position?;
    let block = world.get_block(position);
    let strength = crate::redstone::source_strength(block, world, position);
    Some(strength > threshold)
}

fn native_output_word(world: &PlotWorld, outputs: &[BlockPos]) -> u16 {
    outputs.iter().fold(0u16, |word, &position| {
        let powered = matches!(world.get_block(position), Block::RedstoneWire { wire } if wire.power > 0);
        (word << 1) | u16::from(powered)
    })
}

fn source_class(graph: &CompileGraph, node: NodeId) -> &'static str {
    match &graph.nodes[node.index()].kind {
        CircuitNodeKind::Wire(_) => "wire",
        CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Observer { .. } => "reset",
        CircuitNodeKind::Electrical(NodeType::Observer { .. }) => "reset",
        CircuitNodeKind::Electrical(NodeType::Torch) => "torch",
        CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => "repeater",
        CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => "comparator",
        CircuitNodeKind::Electrical(NodeType::Lever) => "lever",
        CircuitNodeKind::Electrical(NodeType::Button) => "button",
        CircuitNodeKind::Electrical(NodeType::PressurePlate) => "pressure_plate",
        _ => "other",
    }
}

fn feedback_geometry(graph: &CompileGraph, feedback: &[(ResourceId, ContactId, usize)]) -> Vec<String> {
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return vec!["missing cell catalog".into()];
    };
    feedback
        .iter()
        .map(|&(resource, source, _)| {
            let owners_and_resets = catalog
                .cells
                .iter()
                .find_map(|cell| {
                    let token = cell.resources.iter().find(|token| token.resource == resource)?;
                    let owners = token
                        .owners
                        .iter()
                        .map(|owner| {
                            let position = graph.pistons[owner.index()].position;
                            let reset = cell.uses.iter().find(|use_| use_.actor == *owner).map_or_else(
                                || "missing-use".to_owned(),
                                |use_| match &use_.reset {
                                    CellReset::Observer { node, .. } => {
                                        format!("observer:{:?}@{:?}", graph.nodes[node.index()].kind, graph.nodes[node.index()].position)
                                    }
                                    CellReset::Payload { resource, .. } => format!("payload:{resource:?}"),
                                    CellReset::InputRestoration { resource, contact, .. } => {
                                        format!("input-restoration:{resource:?}@{contact:?}")
                                    }
                                },
                            );
                            format!("{owner:?}@{position:?}/{reset}")
                        })
                        .collect::<Vec<_>>();
                    Some(owners.join(","))
                })
                .unwrap_or_else(|| "missing-contract".into());
            format!(
                "resource={resource:?} source={source:?}@{:?} owners=[{owners_and_resets}]",
                graph.contacts[source.index()].position
            )
        })
        .collect()
}

#[test]
#[ignore = "native FPU computation/state diagnostic; reports every-tick divergences without an arithmetic oracle"]
fn report_fpu_computation_native_tick_trace() {
    let (descriptor, mut world, bounds) = fixture();
    let input_positions = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(input_positions.len(), 35);
    let trigger = pos(&descriptor["ports"]["trigger"]);
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(output_positions.len(), 16);

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
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
    let graph = prepared.graph;
    let targets = output_positions
        .iter()
        .map(|&position| {
            let (index, node) = graph
                .nodes
                .iter()
                .enumerate()
                .find(|(_, node)| node.position == Some(position))
                .unwrap_or_else(|| panic!("manifest FPU dust output has no node at {position:?}"));
            assert!(matches!(node.kind, CircuitNodeKind::Wire(_)));
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let circuit = match compile_computation(&graph, &targets, &monitor) {
        Ok(circuit) => circuit,
        Err(error) => {
            println!("FPU computation extraction declined before producing a closed circuit: {error}");
            panic!("FPU computation extraction is not closed: {error}");
        }
    };
    assert!(
        circuit.declines.is_empty(),
        "whole FPU query declined ({} entries, first 12: {:#?})",
        circuit.declines.len(),
        circuit.declines.iter().take(12).collect::<Vec<_>>()
    );
    assert_eq!(
        circuit
            .outputs
            .iter()
            .filter(|output| matches!(output.target, SettledTarget::Power(_)))
            .count(),
        16
    );

    let feedback_outputs = circuit
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source, index)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let feedback_by_resource: rustc_hash::FxHashMap<_, _> = feedback_outputs
        .iter()
        .map(|&(resource, source, index)| (resource, (source, index)))
        .collect();
    for row in feedback_geometry(&graph, &feedback_outputs).iter().take(32) {
        println!("FPU feedback geometry: {row}");
    }
    let mut class_counts = BTreeMap::<&'static str, (usize, rustc_hash::FxHashSet<NodeId>, BTreeMap<String, usize>)>::new();
    let mut unclassified_count = 0usize;
    let mut unclassified_samples = Vec::new();
    let mut samples_by_class = BTreeMap::<&'static str, usize>::new();
    for input in circuit.program.inputs.iter() {
        match input.source {
            LayerInputSource::SourceAbove { site, port, threshold } => match graph.sites[site.index()].source {
                SiteSource::Component(node) => {
                    let class = source_class(&graph, node);
                    let (count, nodes, predicates) = class_counts.entry(class).or_default();
                    *count += 1;
                    nodes.insert(node);
                    *predicates.entry(format!("{port:?}/>{threshold}")).or_default() += 1;

                    let declared = matches!(class, "lever" | "button" | "pressure_plate") && port == PowerPort::Output && threshold == 0;
                    if !declared {
                        unclassified_count += 1;
                        let sampled = samples_by_class.entry(class).or_default();
                        if *sampled < 4 {
                            let component = &graph.nodes[node.index()];
                            unclassified_samples.push(format!(
                                "site={site:?} port={port:?} threshold={threshold} node={node:?} kind={:?} position={:?}",
                                component.kind, component.position
                            ));
                            *sampled += 1;
                        }
                    }
                }
                SiteSource::Constant(_) => {}
                source => {
                    unclassified_count += 1;
                    let sampled = samples_by_class.entry("source").or_default();
                    if *sampled < 4 {
                        unclassified_samples.push(format!("site={site:?} port={port:?} threshold={threshold} source={source:?}"));
                        *sampled += 1;
                    }
                }
            },
            LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                if feedback_by_resource
                    .get(&resource)
                    .is_some_and(|(source, _)| *source == contact || graph.resources[resource.index()].contacts.contains(&contact)) => {}
            source => {
                unclassified_count += 1;
                let sampled = samples_by_class.entry("state").or_default();
                if *sampled < 4 {
                    unclassified_samples.push(format!("unsupported/unbound input {source:?}"));
                    *sampled += 1;
                }
            }
        }
    }
    let classes = class_counts
        .iter()
        .map(|(class, (predicates, nodes, kinds))| {
            format!("{class}: predicates={predicates} unique_nodes={} input_predicates={kinds:?}", nodes.len())
        })
        .collect::<Vec<_>>();
    let class_summary = classes.iter().take(12).cloned().collect::<Vec<_>>();
    println!(
        "FPU computation circuit: graph_nodes={} recognized_cells={} BUD_registers={} logical_inputs={} expressions={} power_outputs={} feedback_resources={} source_classes={class_summary:?} undeclared_input_count={unclassified_count} first_undeclared_inputs={unclassified_samples:#?}",
        graph.nodes.len(),
        graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
        graph.plans.bud_memory_reads.len(),
        circuit.program.inputs.len(),
        circuit.program.expressions.len(),
        targets.len(),
        feedback_outputs.len(),
    );
    assert!(
        unclassified_count == 0,
        "FPU computation has {unclassified_count} undeclared external/state inputs; first samples: {unclassified_samples:#?}"
    );

    let mut external_sources = rustc_hash::FxHashSet::default();
    let mut initial_inputs = Vec::with_capacity(circuit.program.inputs.len());
    let mut feedback_slots = Vec::<(usize, ResourceId, ContactId)>::new();
    for (slot, input) in circuit.program.inputs.iter().enumerate() {
        match input.source {
            LayerInputSource::SourceAbove { .. } => {
                let sampled = source_strength(&graph, &world, input.source).expect("declared FPU boundary can be sampled");
                external_sources.insert(input.source);
                initial_inputs.push(sampled);
            }
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                let (source, _) = feedback_by_resource[&resource];
                let at = graph.initial_resources[resource.index()].at;
                let present = at == source;
                initial_inputs.push(if contact == source { present } else { !present });
                feedback_slots.push((slot, resource, contact));
            }
            other => panic!("closure assertion should have rejected {other:?}"),
        }
    }
    let mut machine = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial_inputs).unwrap();
    let output_indices = targets
        .iter()
        .map(|&target| {
            circuit
                .outputs
                .iter()
                .position(|output| output.target == SettledTarget::Power(target))
                .unwrap()
        })
        .collect::<Vec<_>>();

    let Block::Lever { lever } = world.get_block(trigger) else {
        panic!("green control is not a lever");
    };
    assert!(lever.powered, "fixture saved trigger is expected ON before the replay protocol");
    world.disable_command_output_limits_for_replay();
    set_lever(&mut world, trigger, false);
    let mut current_mask = input_positions.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("manifest input is not a lever at {position:?}");
        };
        word | (u64::from(lever.powered) << bit)
    });
    let vectors = stimuli(current_mask, 16);
    let mut mismatches = Vec::new();
    let mut mismatches_by_phase = BTreeMap::<(usize, usize), usize>::new();
    let mut vector_boundaries = Vec::new();
    let mut tick_number = 0usize;
    let mut tick_rows = Vec::new();
    let mut fresh_frames = Vec::new();

    for _ in 0..INITIAL_TICKS {
        tick_number += 1;
        world.tick_interpreted();
        let external = machine
            .external_inputs()
            .iter()
            .map(|input| source_strength(&graph, &world, input.source).expect("classified source remains sampleable"))
            .collect::<Vec<_>>();
        machine.step(&external).unwrap();
        compare_tick(
            &mut tick_rows,
            &mut mismatches,
            &mut mismatches_by_phase,
            tick_number,
            usize::MAX,
            tick_number,
            &machine,
            &output_indices,
            &world,
            &output_positions,
        );
    }

    for (vector_index, &vector) in vectors.iter().enumerate() {
        for (bit, &position) in input_positions.iter().enumerate() {
            let powered = vector & (1 << bit) != 0;
            set_lever(&mut world, position, powered);
        }
        current_mask = vector;
        for offset in 0..INPUT_TICKS {
            tick_number += 1;
            world.tick_interpreted();
            let external = machine
                .external_inputs()
                .iter()
                .map(|input| source_strength(&graph, &world, input.source).expect("classified source remains sampleable"))
                .collect::<Vec<_>>();
            machine.step(&external).unwrap();
            compare_tick(
                &mut tick_rows,
                &mut mismatches,
                &mut mismatches_by_phase,
                tick_number,
                vector_index,
                offset,
                &machine,
                &output_indices,
                &world,
                &output_positions,
            );
            if offset + 1 == INPUT_TICKS {
                let native = native_output_word(&world, &output_positions);
                let logical = output_indices
                    .iter()
                    .fold(0u16, |word, &index| (word << 1) | u16::from(machine.outputs()[index] != 0));
                vector_boundaries.push((vector_index, native, logical));
            }
        }
        // Diagnostic: an instant computation starts with reset Far sources,
        // rather than inheriting the preceding computation's feedback values.
        let frame_inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| match input.source {
                LayerInputSource::SourceAbove { .. } => source_strength(&graph, &world, input.source).unwrap(),
                LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => contact == feedback_by_resource[&resource].0,
                other => panic!("unclassified frame input {other:?}"),
            })
            .collect::<Vec<_>>();
        let mut frame = crate::redpiler::program_graph::SettledMachine::new(&circuit, &frame_inputs).unwrap();
        let frame_external = frame
            .external_inputs()
            .iter()
            .map(|input| source_strength(&graph, &world, input.source).unwrap())
            .collect::<Vec<_>>();
        let mut converged = false;
        let mut steps = 0;
        for _ in 0..128 {
            let before = frame.input_values().to_vec();
            frame.step(&frame_external).unwrap();
            steps += 1;
            if frame.input_values() == before {
                converged = true;
                break;
            }
        }
        let word = output_indices
            .iter()
            .fold(0u16, |word, &index| (word << 1) | u16::from(frame.outputs()[index] != 0));
        fresh_frames.push(serde_json::json!({"vector":vector_index,"word":word,"steps":steps,"converged":converged}));
    }

    let mut bad_state_feedback = Vec::new();
    for &(slot, resource, contact) in &feedback_slots {
        let (source, output) = feedback_by_resource[&resource];
        let current = machine.input_values()[slot];
        let expected_next = machine.outputs()[output] != 0;
        if contact == source && current != expected_next {
            bad_state_feedback.push((resource, slot, current, expected_next));
        }
    }
    println!(
        "FPU native-vs-logical trace: ticks={} vectors={} ticks_per_vector={} feedback_resources={} feedback_input_slots={} external_sources={} mismatch_count={} first_mismatches={:?} first_vector_boundaries={:?} first_phase_counts={:?} first_feedback_snapshot_differences={:?}",
        tick_number,
        vectors.len(),
        INPUT_TICKS,
        feedback_outputs.len(),
        feedback_slots.len(),
        external_sources.len(),
        mismatches_by_phase.values().sum::<usize>(),
        mismatches.iter().take(12).collect::<Vec<_>>(),
        vector_boundaries.iter().take(12).collect::<Vec<_>>(),
        mismatches_by_phase.iter().take(12).collect::<Vec<_>>(),
        bad_state_feedback.iter().take(12).collect::<Vec<_>>(),
    );
    let report = serde_json::json!({
        "contract":"Candidate settled-frame values; raw tick differences include removable reset glitches. This trace alone does not certify publication timing.",
        "expressions":circuit.program.expressions.len(),
        "feedback_resources":feedback_outputs.len(),
        "external_sources":external_sources.len(),
        "input_vectors":vectors,
        "fresh_frames":fresh_frames,
        "ticks":tick_rows,
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-computation-frame-trace.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("FPU computation frame trace: {}", path.display());
    assert_eq!(current_mask, *vectors.last().unwrap());
}

#[test]
#[ignore = "focused O6 FPU production-admission diagnostic; reports graph-derived targets and compiler declines"]
fn report_fpu_production_admission() {
    let (_descriptor, _world, bounds) = fixture();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&_world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
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
    let command_blocks = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })).then_some(NodeId::new(index as u32))
        })
        .collect::<rustc_hash::FxHashSet<_>>();
    let command_block_support = command_blocks
        .iter()
        .map(|&command_block| {
            let node = &graph.nodes[command_block.index()];
            let incoming = graph
                .power_links
                .iter()
                .filter(|link| matches!(link.to, PowerTarget::Input { node, .. } if node == command_block))
                .map(|link| (graph.sites[link.from.index()].source, link.from_port, link.to))
                .collect::<Vec<_>>();
            let outgoing = graph
                .power_links
                .iter()
                .filter(|link| graph.sites[link.from.index()].source == SiteSource::Component(command_block))
                .map(|link| (link.to, link.from_port))
                .collect::<Vec<_>>();
            (command_block, node.position, &node.kind, &node.state, incoming, outgoing)
        })
        .collect::<Vec<_>>();
    println!(
        "FPU command-block census: blocks={} input_links={} output_links={} detail={command_block_support:#?}",
        command_blocks.len(),
        command_block_support
            .iter()
            .map(|(_, _, _, _, incoming, _)| incoming.len())
            .sum::<usize>(),
        command_block_support
            .iter()
            .map(|(_, _, _, _, _, outgoing)| outgoing.len())
            .sum::<usize>(),
    );
    let known_clock = graph
        .pistons
        .iter()
        .enumerate()
        .find_map(|(index, piston)| (piston.position == BlockPos::new(46, 66, 112)).then_some(PistonId::new(index as u32)));
    if let Some(clock) = known_clock {
        println!(
            "FPU known direct-clock support: {:#?}",
            super::timing::diagnostic_candidate_support(graph, clock, &monitor)
        );
    } else {
        println!("FPU known direct-clock support: no piston at diagnostic position");
    }
    let targets = super::settled::frame_power_targets(graph);
    let selected_commands = targets
        .iter()
        .filter_map(|target| match target {
            PowerTarget::Input { node, port: InputPort::Main }
                if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })) =>
            {
                Some(*node)
            }
            _ => None,
        })
        .collect::<rustc_hash::FxHashSet<_>>();
    assert_eq!(
        selected_commands, command_blocks,
        "every command block Main output must be included in the frame output cone"
    );
    let timing = super::timing::infer(graph, &monitor);
    let computation = compile_computation(graph, &targets, &monitor);
    println!(
        "FPU production admission: graph_nodes={} pistons={} resources={} cell_contracts={} BUD_reads={} selected_frame_outputs={} selected_clock={:?}",
        graph.nodes.len(),
        graph.pistons.len(),
        graph.resources.len(),
        graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
        graph.plans.bud_memory_reads.len(),
        targets.len(),
        timing
            .as_ref()
            .map(|island| (island.clock, island.actors.len(), island.nodes.len(), island.trigger)),
    );
    println!("FPU production timing inference: {timing:#?}");
    match computation {
        Ok(circuit) => {
            let compiled_commands = circuit
                .outputs
                .iter()
                .filter_map(|output| match output.target {
                    SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) if command_blocks.contains(&node) => Some(node),
                    _ => None,
                })
                .collect::<rustc_hash::FxHashSet<_>>();
            assert_eq!(
                compiled_commands, command_blocks,
                "every command block Main output must be evaluated by the frame"
            );
            println!(
                "FPU production unsupported frame inputs: {:#?}",
                super::settled::unsupported_computation_frame_inputs(graph, &circuit)
            );
            let mut sources = BTreeMap::<String, usize>::new();
            for input in &circuit.program.inputs {
                let kind = match input.source {
                    LayerInputSource::SourceAbove { site, .. } => match graph.sites[site.index()].source {
                        SiteSource::Component(node) => source_class(graph, node).to_owned(),
                        other => format!("{other:?}"),
                    },
                    LayerInputSource::State(StateTest::ResourceAt { .. }) => "feedback".to_owned(),
                    other => format!("{other:?}"),
                };
                *sources.entry(kind).or_default() += 1;
            }
            println!("FPU production input classes: {sources:?}");
            println!(
                "FPU production computation: inputs={} expressions={} outputs={} power_outputs={} resource_states={} declines={:#?}",
                circuit.program.inputs.len(),
                circuit.program.expressions.len(),
                circuit.outputs.len(),
                circuit
                    .outputs
                    .iter()
                    .filter(|output| matches!(output.target, SettledTarget::Power(_)))
                    .count(),
                circuit
                    .outputs
                    .iter()
                    .filter(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
                    .count(),
                circuit.declines,
            );
        }
        Err(error) => println!("FPU production computation declined: {error}"),
    }
    match super::settled::compile_device(graph, &monitor) {
        Ok(circuit) => println!(
            "FPU selected device artifact: inputs={} expressions={} outputs={} timing={} declines={:#?}",
            circuit.program.inputs.len(),
            circuit.program.expressions.len(),
            circuit.outputs.len(),
            circuit.timing.is_some(),
            circuit.declines,
        ),
        Err(error) => println!("FPU selected device compiler error: {error}"),
    }
}

#[test]
#[ignore = "production FPU settled wires and native command pulse/refire witness"]
fn fpu_production_device_matches_benchmark_frames_at_clock_edges() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::program_graph::SettledEvaluation;
    use std::sync::Arc;

    #[derive(Debug)]
    struct NativeCut {
        command_power: Vec<bool>,
        execution_changed: Vec<bool>,
        execution_ticks: Vec<i64>,
        command_output_delta: usize,
    }

    let (descriptor, mut world, bounds) = fixture();
    let controls = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    let outputs = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    let trigger = pos(&descriptor["ports"]["trigger"]);
    let initial_word = controls.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("input is not a lever")
        };
        word | (u64::from(lever.powered) << bit)
    });
    // Selector 011 maps to input bits 2 and 1 because the fixture lists lines by weights 4, 2, 1.
    let witness_word = (0xffffu64 << 16) | (0b110u64 << 32);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let admitted_frame = prepared
        .graph
        .plans
        .settled_circuit
        .as_ref()
        .expect("FPU O6 pipeline must admit the settled frame");
    assert!(admitted_frame.timing.is_some(), "FPU frame must retain its inferred clock timing");
    assert_fpu_retained_cells(&prepared.graph, admitted_frame);
    let mut shared_reset_graph = prepared.graph.clone();
    add_foreign_observer_reset_source(&mut shared_reset_graph, admitted_frame);
    assert_hybrid_falls_back(&mut shared_reset_graph, &prepared.entry, &options, &mut world, trigger, &monitor);

    let mut command_sink_graph = prepared.graph.clone();
    add_command_success_output_sink(&mut command_sink_graph, admitted_frame);
    assert_hybrid_falls_back(&mut command_sink_graph, &prepared.entry, &options, &mut world, trigger, &monitor);
    assert_eq!(
        evaluations_after_trigger_tick(
            &mut prepared.graph.clone(),
            &prepared.entry,
            &options,
            &mut world,
            trigger,
            &monitor,
            true
        ),
        1,
        "the same one-tick trigger path must evaluate the accepted FPU frame"
    );

    let graph = &prepared.graph;
    let targets = outputs
        .iter()
        .map(|&position| {
            let index = graph.nodes.iter().position(|node| node.position == Some(position)).unwrap();
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let mut command_positions = graph
        .nodes
        .iter()
        .filter_map(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })).then_some(node.position.unwrap()))
        .collect::<Vec<_>>();
    command_positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    assert_eq!(
        command_positions,
        [BlockPos::new(66, 44, 51), BlockPos::new(68, 44, 51), BlockPos::new(70, 44, 51)],
        "retain all three native tellraw outputs"
    );
    // Command feedback is intentionally outside this candidate compiler's supported input cone.
    // Compile only the 16 benchmark wire outputs and check command events through native replay.
    let reference = compile_computation(graph, &targets, &monitor).unwrap();
    let slots = targets
        .iter()
        .map(|target| {
            reference
                .outputs
                .iter()
                .position(|output| output.target == SettledTarget::Power(*target))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let state_outputs = reference
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, (source, index))),
            _ => None,
        })
        .collect::<rustc_hash::FxHashMap<_, _>>();
    let feedback = reference
        .program
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| {
            let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = input.source else {
                return None;
            };
            let &(source, output) = state_outputs.get(&resource).unwrap();
            Some((slot, output, contact == source))
        })
        .collect::<Vec<_>>();
    let mut evaluation = SettledEvaluation::new(&reference).unwrap();
    let mut reference_inputs = vec![false; reference.program.inputs.len()];
    let frame = reference
        .program
        .inputs
        .iter()
        .map(|input| match input.source {
            LayerInputSource::SourceAbove { site, .. } => {
                if let SiteSource::Component(node) = graph.sites[site.index()].source {
                    let position = graph.nodes[node.index()].position;
                    if position == Some(trigger) {
                        return false;
                    }
                    if let Some(bit) = controls.iter().position(|&p| position == Some(p)) {
                        return witness_word & (1 << bit) != 0;
                    }
                }
                source_strength(graph, &world, input.source).expect("benchmark external seed")
            }
            LayerInputSource::State(StateTest::ResourceAt { .. }) => false,
            other => panic!("unsupported benchmark seed {other:?}"),
        })
        .collect::<Vec<_>>();
    let settled_word = super::fpu_kernel_bench::run_frame(&mut evaluation, &mut reference_inputs, &frame, &feedback, &slots).0;
    let settled_strengths = slots.iter().map(|&slot| evaluation.outputs()[slot]).collect::<Vec<_>>();

    // Native reference: idle for the same 64-tick preparation window, install
    // A=0/B=ffff/opcode=011, hold the trigger off for 24 ticks, then turn it on
    // for six stop/reset ticks.
    let (_, mut native, _) = fixture();
    native.disable_command_output_limits_for_replay();
    set_lever(&mut native, trigger, true);
    for _ in 0..64 {
        native.tick_interpreted();
    }
    for (bit, &position) in controls.iter().enumerate() {
        set_lever(&mut native, position, witness_word & (1 << bit) != 0);
    }
    set_lever(&mut native, trigger, false);
    let mut previous_native_execution = command_positions
        .iter()
        .map(|&position| match native.get_block_entity(position) {
            Some(BlockEntity::CommandBlock(entity)) => entity.last_execution,
            _ => panic!("native FPU command entity missing at {position:?}"),
        })
        .collect::<Vec<_>>();
    let mut previous_native_output_count = native.command_output().count();
    let mut native_cuts = Vec::with_capacity(30);
    for offset in 0..30 {
        if offset == 24 {
            set_lever(&mut native, trigger, true);
        }
        native.tick_interpreted();
        let mut command_power = Vec::with_capacity(command_positions.len());
        let mut execution_changed = Vec::with_capacity(command_positions.len());
        let mut execution_ticks = Vec::with_capacity(command_positions.len());
        for (index, &position) in command_positions.iter().enumerate() {
            let Some(BlockEntity::CommandBlock(entity)) = native.get_block_entity(position) else {
                panic!("native FPU command entity missing at {position:?}");
            };
            command_power.push(entity.powered);
            execution_changed.push(entity.last_execution != previous_native_execution[index]);
            execution_ticks.push(entity.last_execution);
            previous_native_execution[index] = entity.last_execution;
        }
        let command_output_count = native.command_output().count();
        let command_output_delta = command_output_count - previous_native_output_count;
        previous_native_output_count = command_output_count;
        native_cuts.push(NativeCut {
            command_power,
            execution_changed,
            execution_ticks,
            command_output_delta,
        });
    }
    for cycle in 0..4 {
        let start = cycle * 6;
        for (command_index, active) in [(0, true), (1, false), (2, true)] {
            assert_eq!(
                native_cuts[start + 2].command_power[command_index],
                active,
                "native cycle {cycle} phase 2"
            );
            assert!(!native_cuts[start + 3].command_power[command_index], "command drops by phase 3");
            assert_eq!(
                native_cuts[start + 3].execution_changed[command_index],
                active,
                "native fire at cycle {cycle} phase 3"
            );
            for phase in [0, 1, 4, 5] {
                assert!(
                    !native_cuts[start + phase].command_power[command_index],
                    "native cycle {cycle} phase {phase}"
                );
                assert!(
                    !native_cuts[start + phase].execution_changed[command_index],
                    "native execution cycle {cycle} phase {phase}"
                );
            }
        }
        for phase in 0..6 {
            assert_eq!(
                native_cuts[start + phase].command_output_delta,
                if phase == 3 { 2 } else { 0 },
                "two tellraw host outputs at phase 3 of native cycle {cycle}"
            );
        }
    }
    for offset in 24..30 {
        for command_index in 0..command_positions.len() {
            assert!(
                !native_cuts[offset].command_power[command_index],
                "commands remain stopped at offset {offset}"
            );
            assert!(
                !native_cuts[offset].execution_changed[command_index],
                "no command refire after trigger-on at offset {offset}"
            );
        }
        assert_eq!(
            native_cuts[offset].command_output_delta, 0,
            "no tellraw output after trigger-on at offset {offset}"
        );
    }

    let monitor = Arc::new(TaskMonitor::default());
    let (_, mut graph_world, _) = fixture();
    graph_world.disable_command_output_limits_for_replay();
    let mut backend = GraphBackend::compile_with_options(&graph_world, bounds, vec![], &options, monitor).unwrap();
    let initial_trigger = matches!(graph_world.get_block(trigger), Block::Lever { lever } if lever.powered);
    if !initial_trigger {
        backend.use_block(trigger).unwrap();
    }
    for _ in 0..64 {
        backend.tick_with_world(&mut graph_world).unwrap();
        backend.flush(&mut graph_world, false);
    }
    let current = initial_word;
    for (bit, &position) in controls.iter().enumerate() {
        if current & (1 << bit) != witness_word & (1 << bit) {
            backend.use_block(position).unwrap();
        }
    }
    backend.use_block(trigger).unwrap();
    let baseline = backend.execution_counts();
    let mut previous_graph_execution = command_positions
        .iter()
        .map(|&position| match graph_world.get_block_entity(position) {
            Some(BlockEntity::CommandBlock(entity)) => entity.last_execution,
            _ => panic!("FPU command block lacks its imported entity at {position:?}"),
        })
        .collect::<Vec<_>>();
    let mut previous_graph_output_count = graph_world.command_output().count();
    for offset in 0..30 {
        if offset == 24 {
            backend.use_block(trigger).unwrap();
        }
        backend.tick_with_world(&mut graph_world).unwrap();
        backend.flush(&mut graph_world, false);
        for (&position, &strength) in outputs.iter().zip(&settled_strengths) {
            let Block::RedstoneWire { wire } = graph_world.get_block(position) else {
                panic!("FPU output is not a wire at {position:?}");
            };
            assert_eq!(wire.power, strength, "candidate frame output at offset {offset}, wire {position:?}");
        }
        for (command_index, &position) in command_positions.iter().enumerate() {
            let Some(BlockEntity::CommandBlock(entity)) = graph_world.get_block_entity(position) else {
                panic!("FPU command entity was not published at {position:?}");
            };
            assert_eq!(
                entity.powered, native_cuts[offset].command_power[command_index],
                "command power at offset {offset}"
            );
            let execution_changed = entity.last_execution != previous_graph_execution[command_index];
            assert_eq!(
                execution_changed, native_cuts[offset].execution_changed[command_index],
                "command execution event at offset {offset}"
            );
            if execution_changed {
                assert_eq!(
                    entity.last_execution,
                    backend.execution_counts().ticks as i64,
                    "command at {position:?} must use the current host tick even when last_execution decreases"
                );
                assert_eq!(
                    entity.last_execution, native_cuts[offset].execution_ticks[command_index],
                    "command at {position:?} must execute on the native witness tick"
                );
            }
            previous_graph_execution[command_index] = entity.last_execution;
        }
        let output_count = graph_world.command_output().count();
        assert_eq!(
            output_count - previous_graph_output_count,
            native_cuts[offset].command_output_delta,
            "real tellraw output count at offset {offset}"
        );
        previous_graph_output_count = output_count;
        let work = backend.execution_counts();
        assert_eq!(
            work.wire_walks, baseline.wire_walks,
            "frame execution must not add physical wire walks at offset {offset} (baseline {}, current {})",
            baseline.wire_walks, work.wire_walks
        );
        let retained_actor_work_bound = command_positions.len() as u64 * (offset as u64 + 1);
        let retained_actor_sample_bound = retained_actor_work_bound * 32;
        assert!(work.piston_actions >= baseline.piston_actions);
        assert!(work.piston_samples >= baseline.piston_samples);
        assert!(work.piston_actions - baseline.piston_actions <= retained_actor_work_bound);
        assert!(work.piston_samples - baseline.piston_samples <= retained_actor_sample_bound);
        assert!(work.logical_kernel_evaluations >= baseline.logical_kernel_evaluations);
        if offset == 0 {
            assert!(
                work.logical_kernel_evaluations > baseline.logical_kernel_evaluations,
                "the first active frame tick must execute the admitted logical kernel"
            );
        }
    }

    // Retain the original 16-vector wire-only coverage independently of the
    // native command witness, whose Near feedback is outside this compiler's
    // supported input cone.
    let words = stimuli(initial_word, 16);
    let (_, mut vector_world, _) = fixture();
    vector_world.disable_command_output_limits_for_replay();
    let mut vector_backend = GraphBackend::compile_with_options(&vector_world, bounds, vec![], &options, Arc::new(TaskMonitor::default())).unwrap();
    let initial_vector_trigger = matches!(vector_world.get_block(trigger), Block::Lever { lever } if lever.powered);
    if !initial_vector_trigger {
        vector_backend.use_block(trigger).unwrap();
    }
    vector_backend.use_block(trigger).unwrap();
    let mut vector_current = initial_word;
    for (vector_index, &word) in words.iter().enumerate() {
        for (bit, &position) in controls.iter().enumerate() {
            if (word ^ vector_current) & (1 << bit) != 0 {
                vector_backend.use_block(position).unwrap();
            }
        }
        vector_current = word;
        let vector_frame = reference
            .program
            .inputs
            .iter()
            .map(|input| match input.source {
                LayerInputSource::SourceAbove { site, .. } => {
                    if let SiteSource::Component(node) = graph.sites[site.index()].source {
                        let position = graph.nodes[node.index()].position;
                        if position == Some(trigger) {
                            return false;
                        }
                        if let Some(bit) = controls.iter().position(|&p| position == Some(p)) {
                            return word & (1 << bit) != 0;
                        }
                    }
                    source_strength(graph, &world, input.source).expect("benchmark external seed")
                }
                LayerInputSource::State(StateTest::ResourceAt { .. }) => false,
                other => panic!("unsupported benchmark seed {other:?}"),
            })
            .collect::<Vec<_>>();
        super::fpu_kernel_bench::run_frame(&mut evaluation, &mut reference_inputs, &vector_frame, &feedback, &slots);
        let expected_strengths = slots.iter().map(|&slot| evaluation.outputs()[slot]).collect::<Vec<_>>();
        for tick in 0..6 {
            vector_backend.tick_with_world(&mut vector_world).unwrap();
            vector_backend.flush(&mut vector_world, false);
            for (&position, &strength) in outputs.iter().zip(&expected_strengths) {
                let Block::RedstoneWire { wire } = vector_world.get_block(position) else {
                    panic!("FPU output is not a wire at {position:?}");
                };
                assert_eq!(wire.power, strength, "16-vector frame {vector_index}, tick {tick}, wire {position:?}");
            }
        }
    }
    assert_eq!(
        native_cuts.iter().map(|cut| cut.command_output_delta).sum::<usize>(),
        8,
        "four six-tick command cycles each emit two retained tellraw outputs"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-production-frame-equivalence.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "input_word":witness_word,
            "settled_output_word":settled_word,
            "native_command_power":native_cuts.iter().map(|cut| &cut.command_power).collect::<Vec<_>>(),
            "native_command_execution_ticks":native_cuts.iter().map(|cut| &cut.execution_ticks).collect::<Vec<_>>(),
            "native_command_output_delta":native_cuts.iter().map(|cut| cut.command_output_delta).collect::<Vec<_>>(),
            "game_ticks_per_cycle":6
        }))
        .unwrap(),
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn compare_tick(
    rows: &mut Vec<serde_json::Value>,
    mismatches: &mut Vec<(usize, usize, usize, u16, u16)>,
    by_phase: &mut BTreeMap<(usize, usize), usize>,
    tick: usize,
    vector: usize,
    offset: usize,
    machine: &crate::redpiler::program_graph::SettledMachine,
    output_indices: &[usize],
    world: &PlotWorld,
    outputs: &[BlockPos],
) {
    let native = native_output_word(world, outputs);
    let logical = output_indices
        .iter()
        .fold(0u16, |word, &index| (word << 1) | u16::from(machine.outputs()[index] != 0));
    rows.push(serde_json::json!({
        "tick":tick,"vector":(vector != usize::MAX).then_some(vector),
        "offset":offset,"native":native,"logical":logical,
    }));
    if native != logical {
        *by_phase.entry((vector, offset)).or_default() += 1;
        if mismatches.len() < 12 {
            mismatches.push((tick, vector, offset, native, logical));
        }
    }
}
