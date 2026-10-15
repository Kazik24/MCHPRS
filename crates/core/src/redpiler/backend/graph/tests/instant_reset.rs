//! The private instant-output transfer preserves the general walk's observable work.
use super::fuzz_tests::{graph_operations_since, native_operations};
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use mchprs_blocks::blocks::LeverFace;
use mchprs_world::AdvancePhase;

fn fixture_graph() -> (PlotWorld, CompileGraph, (BlockPos, BlockPos)) {
    let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
    assert!(world.scheduler().iter_entries().next().is_none(), "strict import must stay quiet");
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
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
    assert_eq!(
        prepared.graph.wires.iter().filter(|wire| wire.execution.instant_reset.is_some()).count(),
        1
    );
    (world, prepared.graph, bounds)
}

fn output_plan(graph: &CompileGraph) -> (NodeId, InstantResetPlan) {
    let wire = graph.wires.iter().find(|wire| wire.execution.instant_reset.is_some()).unwrap();
    (wire.node, wire.execution.instant_reset.unwrap())
}

fn backend(graph: &CompileGraph, certified: bool) -> GraphBackend {
    let mut graph = graph.clone();
    if !certified {
        for wire in &mut graph.wires {
            wire.execution.instant_reset = None;
        }
    }
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    backend.set_test_recording(true);
    backend
}

fn owned_wire_deliveries(backend: &GraphBackend, owned: [NodeId; 2]) -> u64 {
    backend
        .trace()
        .iter()
        .filter(|line| line.starts_with("wire deliver ") && owned.iter().any(|node| line.ends_with(&format!("receiver=Component({node:?})"))))
        .count() as u64
}

fn effects(backend: &GraphBackend) -> Vec<String> {
    backend
        .effects
        .iter()
        .map(|effect| match effect {
            PresentationEffect::Sound {
                position,
                id,
                category,
                volume,
                pitch,
            } => {
                format!("sound {position:?} {id} {category} {volume:?} {pitch:?}")
            }
            PresentationEffect::BlockAction { position, action } => match action {
                BlockAction::Piston { action, piston } => format!("piston {position:?} {action:?} {piston:?}"),
                BlockAction::BlockChange { pos, block_id } => format!("change {position:?} {pos:?} {block_id}"),
            },
        })
        .collect()
}

fn assert_same_execution(general: &GraphBackend, certified: &GraphBackend, owned: [NodeId; 2]) {
    assert_eq!(certified.state, general.state, "all mutable component, actor and resource state");
    assert_eq!(
        certified.observed_operations, general.observed_operations,
        "ordered semantic observations"
    );
    let external_callbacks = |backend: &GraphBackend| {
        backend
            .callback_order
            .iter()
            .filter(|(position, _)| !owned.iter().any(|node| general.position(*node) == *position))
            .copied()
            .collect::<Vec<_>>()
    };
    assert_eq!(external_callbacks(certified), external_callbacks(general));
    let writes = |backend: &GraphBackend| {
        backend
            .trace()
            .iter()
            .filter(|line| line.starts_with("wire state "))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(writes(certified), writes(general), "every intermediate wire strength and shape write");
    assert_eq!(certified.scheduled_ticks(), general.scheduled_ticks(), "ordered scheduler");
    assert_eq!(certified.pending, general.pending, "pending membership and multiplicity");
    let requests = |backend: &GraphBackend| {
        backend
            .piston_requests
            .iter()
            .map(|request| (request.piston, request.action))
            .collect::<Vec<_>>()
    };
    assert_eq!(requests(certified), requests(general), "piston request FIFO");
    let deferred = |backend: &GraphBackend| {
        backend
            .deferred_pistons
            .iter()
            .map(|event| {
                (
                    event.piston,
                    event.revision,
                    event.due_tick,
                    event.extending,
                    event.resource.map(|resource| (resource.resource, resource.revision, resource.contact)),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(deferred(certified), deferred(general), "deferred publication FIFO");
    assert_eq!(certified.dirty, general.dirty);
    assert_eq!(effects(certified), effects(general));
    assert_eq!(certified.clock.logical_tick, general.clock.logical_tick);
    assert_eq!(certified.clock.phase, general.clock.phase);
    assert_eq!(certified.clock.scheduled_advanced, general.clock.scheduled_advanced);
    assert_eq!(certified.failure, general.failure);
    let mut expected_counts = general.execution_counts();
    let removed = owned_wire_deliveries(general, owned) - owned_wire_deliveries(certified, owned);
    expected_counts.callbacks -= removed;
    assert_eq!(
        certified.execution_counts(),
        expected_counts,
        "only identified private wire callbacks may disappear"
    );
    for node in owned {
        assert!(
            !certified.trace().iter().any(|line| line == &format!("wire turbo start {node:?}")),
            "owned output wire must avoid the general Turbo walk"
        );
    }
}

fn set_wire_power(backend: &mut GraphBackend, node: NodeId, power: u8) {
    let ComponentState::Wire(wire) = &mut backend.state.components[node.index()] else {
        unreachable!()
    };
    wire.power = power;
}

#[test]
fn instant_output_matches_every_saved_strength_source_and_captured_receiver_state() {
    let (_, graph, _) = fixture_graph();
    let (a, plan) = output_plan(&graph);
    let repeaters: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })).then_some(NodeId::new(index as u32)))
        .collect();
    assert!(!repeaters.is_empty(), "the original output line must have a captured receiver");
    let actor = graph.pistons[plan.piston.index()];
    let mut replayed = 0;
    let mut removed_callbacks = 0;
    for old_a in 0..=15 {
        for old_b in 0..=15 {
            for source in [0, 15] {
                for receiver_case in 0..8 {
                    let mut general = backend(&graph, false);
                    let mut certified = backend(&graph, true);
                    for backend in [&mut general, &mut certified] {
                        set_wire_power(backend, a, old_a);
                        set_wire_power(backend, plan.partner, old_b);
                        let resource = &mut backend.state.resources[plan.resource.index()];
                        resource.at = if source == 0 { actor.near } else { actor.far };
                        resource.enabled = true;
                        let piston = &mut backend.state.pistons[plan.piston.index()];
                        piston.extended = source != 0;
                        piston.head_present = source != 0;
                        piston.availability = PistonAvailability::Ready;
                        let ComponentState::Electrical(observer) = &mut backend.state.components[plan.observer.index()] else {
                            unreachable!()
                        };
                        observer.powered = receiver_case & 1 != 0;
                        observer.output_strength = if receiver_case & 2 != 0 { 15 } else { 0 };
                        for (index, &node) in repeaters.iter().enumerate() {
                            let ComponentState::Electrical(repeater) = &mut backend.state.components[node.index()] else {
                                unreachable!()
                            };
                            repeater.powered = (receiver_case + index) & 1 != 0;
                            repeater.output_strength = if repeater.powered { 15 } else { 0 };
                            repeater.repeater_locked = receiver_case & 2 != 0;
                            if receiver_case & 4 != 0 {
                                backend.schedule(node, 1 + index % 2, TickPriority::High);
                            }
                        }
                        if receiver_case & 4 != 0 {
                            backend.schedule(plan.observer, 1, TickPriority::Normal);
                        }
                        let source_site = backend.wire_plan(a).canonical_neighbors[4];
                        assert_eq!(backend.power_no_dust(source_site, BlockFace::North, None), source);
                        backend.evaluate_seed(a, None).unwrap();
                    }
                    assert_same_execution(&general, &certified, [a, plan.partner]);
                    replayed += certified.trace().iter().filter(|line| line.starts_with("wire instant output ")).count();
                    removed_callbacks += general.execution_counts().callbacks - certified.execution_counts().callbacks;
                }
            }
        }
    }
    assert!(replayed > 0, "the exhaustive matrix must execute the certified transfer");
    assert!(removed_callbacks > 0, "the matrix must exercise private callbacks removed by the kernel");
}

#[derive(Debug, PartialEq)]
struct Future {
    tick: u64,
    phase: AdvancePhase,
    scheduled_advanced: bool,
    cells: Vec<(BlockPos, Block)>,
    entities: Vec<(BlockPos, String)>,
    ticks: Vec<TickEntry>,
    events: Vec<PistonEvent>,
    motions: Vec<(BlockPos, f32, f32, u64)>,
}

fn future(world: &PlotWorld, positions: &[BlockPos]) -> Future {
    let piston = world.piston_state();
    Future {
        tick: piston.logical_tick,
        phase: piston.phase,
        // BetweenTicks overwrites this latch before its next read in prepare_operation.
        scheduled_advanced: piston.phase != AdvancePhase::BetweenTicks && piston.scheduled_advanced,
        cells: positions
            .iter()
            .map(|&position| (position, world.get_block(position)))
            .filter(|(_, block)| *block != Block::Air)
            .collect(),
        entities: positions
            .iter()
            .filter_map(|&position| world.get_block_entity(position).map(|entity| (position, format!("{entity:?}"))))
            .collect(),
        ticks: world.scheduler().iter_entries().collect(),
        events: piston.events.iter().copied().collect(),
        motions: piston
            .motions
            .iter()
            .map(|motion| (motion.pos, motion.progress, motion.previous_progress, motion.last_tick))
            .collect(),
    }
}

fn clear_projection(world: &mut PlotWorld) {
    world.clear_scheduled_ticks();
    world.piston_state_mut().motions.clear();
}

fn project(backend: &GraphBackend, world: &mut PlotWorld) {
    clear_projection(world);
    backend.project_future(world);
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Toggle,
    Tick,
}

fn native_step(world: &mut PlotWorld, control: BlockPos, step: Step) {
    match step {
        Step::Tick => world.tick_interpreted(),
        Step::Toggle => {
            let Block::Lever { mut lever } = world.get_block(control) else {
                unreachable!()
            };
            lever.powered = !lever.powered;
            world.set_block(control, Block::Lever { lever });
            redstone::update_surrounding_blocks(world, control);
            let support = match lever.face {
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            redstone::update_surrounding_blocks(world, control.offset(support));
        }
    }
}

fn histories() -> Vec<Vec<Step>> {
    let mut histories = Vec::new();
    // Every short control history includes no-change holds and same-tick reversals.
    for mask in 0..32 {
        let mut steps = Vec::new();
        for bit in 0..5 {
            if mask & (1 << bit) != 0 {
                steps.push(Step::Toggle);
            }
            steps.push(Step::Tick);
        }
        histories.push(steps);
    }
    for hold in 0..=6 {
        for retrigger in 0..=4 {
            let mut steps = vec![Step::Toggle];
            steps.extend(std::iter::repeat_n(Step::Tick, hold));
            steps.push(Step::Toggle);
            steps.extend(std::iter::repeat_n(Step::Tick, retrigger));
            steps.push(Step::Toggle);
            steps.extend(std::iter::repeat_n(Step::Tick, 8));
            steps.push(Step::Toggle);
            histories.push(steps);
        }
    }
    histories.push(std::iter::once(Step::Toggle).chain(std::iter::repeat_n(Step::Tick, 32)).collect());
    histories
}

#[test]
fn instant_output_matches_native_histories_and_every_reset_continuation_cut() {
    let (_, graph, bounds) = fixture_graph();
    let (a, plan) = output_plan(&graph);
    let control = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
        .unwrap()
        .position
        .unwrap();
    let mut positions = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                positions.push(BlockPos::new(x, y, z));
            }
        }
    }
    let mut replayed = 0;
    let mut pending_cuts = 0;
    let mut request_cuts = 0;
    for (history, steps) in histories().iter().enumerate() {
        for cut in 0..=steps.len() {
            let (mut native, _) = passes::simplify_instant_resets::tests::fixture();
            let (mut general_world, _) = passes::simplify_instant_resets::tests::fixture();
            let (mut certified_world, _) = passes::simplify_instant_resets::tests::fixture();
            let mut general = backend(&graph, false);
            let mut certified = backend(&graph, true);
            for (boundary, &step) in steps[..cut].iter().enumerate() {
                let start = certified.observed_operations.len();
                let entries = capture_at(&positions, || native_step(&mut native, control, step));
                for backend in [&mut general, &mut certified] {
                    match step {
                        Step::Toggle => backend.use_block(control),
                        Step::Tick => backend.tick(),
                    }
                    .unwrap();
                }
                assert_same_execution(&general, &certified, [a, plan.partner]);
                assert_eq!(
                    graph_operations_since(&certified, start, &positions),
                    native_operations(&entries, &positions),
                    "history {history}, cut {cut}, boundary {boundary}: native semantic order"
                );
                project(&general, &mut general_world);
                project(&certified, &mut certified_world);
                assert_eq!(
                    future(&general_world, &positions),
                    future(&native, &positions),
                    "general history {history}, boundary {boundary}"
                );
                assert_eq!(
                    future(&certified_world, &positions),
                    future(&native, &positions),
                    "certified history {history}, boundary {boundary}"
                );
            }
            replayed += certified.trace().iter().filter(|line| line.starts_with("wire instant output ")).count();
            pending_cuts += usize::from(!certified.scheduled_ticks().is_empty() || !certified.deferred_pistons.is_empty());
            request_cuts += usize::from(!certified.piston_requests.is_empty());
            // Snapshot projections must not become a second owner of pending work.
            clear_projection(&mut general_world);
            clear_projection(&mut certified_world);
            general.reset(&mut general_world);
            certified.reset(&mut certified_world);
            for boundary in cut..=steps.len() + 32 {
                assert_eq!(
                    future(&general_world, &positions),
                    future(&native, &positions),
                    "general reset history {history}, cut {cut}, boundary {boundary}"
                );
                assert_eq!(
                    future(&certified_world, &positions),
                    future(&native, &positions),
                    "certified reset history {history}, cut {cut}, boundary {boundary}"
                );
                if boundary == steps.len() + 32 {
                    break;
                }
                let step = steps.get(boundary).copied().unwrap_or(Step::Tick);
                for world in [&mut native, &mut general_world, &mut certified_world] {
                    native_step(world, control, step);
                }
            }
        }
    }
    assert!(replayed > 0, "host histories must execute the new path");
    assert!(pending_cuts > 0, "reset must preserve pending ticks or publications");
    assert!(request_cuts > 0, "reset must preserve an unaccepted request FIFO");
}
