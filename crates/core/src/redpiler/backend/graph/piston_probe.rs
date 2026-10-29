//! Source-qualified piston requests sampled from fresh physical Graph executions.
//! Observations cover only the supplied entry state and stimulus, not other states.
use super::*;
use crate::plot::PlotWorld;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourcePosition {
    pub(crate) id: usize,
    pub(crate) position: BlockPos,
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PistonAttempt {
    pub(crate) tick: usize,
    pub(crate) source: BlockPos,
    pub(crate) route: usize,
    pub(crate) actor: BlockPos,
    pub(crate) action: mchprs_world::PistonAction,
    pub(crate) powered: bool,
    pub(crate) accepted: bool,
    pub(crate) before: Option<ResourcePosition>,
    pub(crate) after: Option<ResourcePosition>,
    pub(crate) outputs: Vec<OutputObservation>,
    pub(crate) pending_after: PendingWork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputObservation {
    pub(crate) position: BlockPos,
    pub(crate) input_strength: u8,
    pub(crate) powered: bool,
    pub(crate) locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PistonTrial {
    pub(crate) stimulus: BlockPos,
    pub(crate) input_before: bool,
    pub(crate) input_events: Vec<BlockPos>,
    pub(crate) initial_resources: Vec<ResourcePosition>,
    pub(crate) attempts: Vec<PistonAttempt>,
    pub(crate) final_resources: Vec<ResourcePosition>,
    pub(crate) settled: bool,
    pub(crate) pending: PendingWork,
    pub(crate) clock: Option<BlockPos>,
    pub(crate) clock_retractions: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingWork {
    pub(crate) scheduled: Vec<(BlockPos, usize, TickPriority)>,
    pub(crate) requests: Vec<(BlockPos, mchprs_world::PistonAction, Option<usize>)>,
    pub(crate) deferred: Vec<(BlockPos, u64, bool)>,
}

fn resource_position(graph: &GraphBackend, id: ResourceId) -> ResourcePosition {
    let state = graph.state.resources[id.index()];
    ResourcePosition {
        id: id.index(),
        position: graph.program.contacts()[state.at.index()].position,
        enabled: state.enabled,
    }
}

/// Probe each lever transition from the same entry state using the physical FIFO.
pub(crate) fn probe(world: &PlotWorld, bounds: (BlockPos, BlockPos), sources: &[BlockPos], ticks: usize) -> Result<Vec<PistonTrial>, String> {
    let baseline = prepare(world, bounds, ticks)?;
    let mut unique = std::collections::HashSet::new();
    for &source in sources {
        if !unique.insert(source) {
            return Err("piston probe sources must be distinct levers".into());
        }
    }
    sources.iter().map(|&source| run_trial(&baseline, &[source], &[], ticks, true)).collect()
}

/// Observe a caller-declared clock without assuming any pending work is owned by it.
pub(crate) fn probe_clock_window(
    world: &PlotWorld,
    bounds: (BlockPos, BlockPos),
    sources: &[BlockPos],
    clock: BlockPos,
    ticks: usize,
) -> Result<Vec<PistonTrial>, String> {
    probe_clock_outputs(world, bounds, sources, clock, &[], ticks)
}

/// Capture declared output inputs and states immediately after each physical request.
pub(crate) fn probe_clock_outputs(
    world: &PlotWorld,
    bounds: (BlockPos, BlockPos),
    sources: &[BlockPos],
    clock: BlockPos,
    outputs: &[BlockPos],
    ticks: usize,
) -> Result<Vec<PistonTrial>, String> {
    let baseline = prepare(world, bounds, ticks)?;
    if !baseline.program.pistons().iter().any(|piston| piston.position == clock) {
        return Err("declared clock is not a piston in the graph".into());
    }
    let mut unique = std::collections::HashSet::new();
    let mut trials = Vec::with_capacity(sources.len());
    for &source in sources {
        if !unique.insert(source) {
            return Err("piston probe sources must be distinct levers".into());
        }
        let mut trial = run_trial(&baseline, &[source], outputs, ticks, false)?;
        trial.clock = Some(clock);
        trial.clock_retractions = trial
            .attempts
            .iter()
            .enumerate()
            .filter_map(|(index, attempt)| {
                (attempt.actor == clock
                    && attempt.accepted
                    && matches!(
                        attempt.action,
                        mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull
                    ))
                .then_some(index)
            })
            .collect();
        if trial.clock_retractions.is_empty() {
            return Err("declared clock has no accepted retraction in the probe window".into());
        }
        trials.push(trial);
    }
    Ok(trials)
}

/// Probe ordered lever events before the next tick, including cancellation at acceptance.
pub(crate) fn probe_sequence(world: &PlotWorld, bounds: (BlockPos, BlockPos), events: &[BlockPos], ticks: usize) -> Result<PistonTrial, String> {
    let baseline = prepare(world, bounds, ticks)?;
    run_trial(&baseline, events, &[], ticks, true)
}

fn prepare(world: &PlotWorld, bounds: (BlockPos, BlockPos), ticks: usize) -> Result<GraphBackend, String> {
    if ticks == 0 || ticks > 32 {
        return Err("piston probe requires one to 32 ticks".into());
    }
    let graph = super::wire_probe::physical_graph(world, bounds)?;
    if !graph.program.command_blocks().is_empty() {
        return Err("piston probe does not execute command blocks".into());
    }
    Ok(graph)
}

fn run_trial(baseline: &GraphBackend, events: &[BlockPos], outputs: &[BlockPos], ticks: usize, require_settled: bool) -> Result<PistonTrial, String> {
    let &source = events.first().ok_or("piston probe requires an input event")?;
    for &event in events {
        let node = *baseline.positions.get(&event).ok_or("piston probe source missing from graph")?;
        if !matches!(baseline.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) {
            return Err("piston probe sources must be levers".into());
        }
    }
    let mut output_nodes = Vec::with_capacity(outputs.len());
    let mut unique = std::collections::HashSet::new();
    for &position in outputs {
        let node = *baseline.positions.get(&position).ok_or("piston probe output missing from graph")?;
        if !unique.insert(position)
            || !matches!(
                baseline.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp)
            )
        {
            return Err("piston probe outputs must be distinct repeaters or lamps".into());
        }
        output_nodes.push(node);
    }
    let mut graph = GraphBackend::new_unactivated(baseline.program.clone(), baseline.state.clone(), baseline.future.clock.logical_tick)
        .map_err(|error| error.to_string())?;
    let input_before = graph.node_state(graph.positions[&source]).powered;
    let initial_resources = (0..graph.state.resources.len())
        .map(|index| resource_position(&graph, ResourceId::new(index as u32)))
        .collect();
    let mut attempts = Vec::new();
    for &event in events {
        graph.use_block(event).map_err(|error| error.to_string())?;
    }
    for tick in 1..=ticks {
        let mut host: Option<&mut dyn GraphCommandHost> = None;
        graph
            .tick_phases_with_hook(&mut host, |graph, _| {
                graph.future.clock.phase = GraphPhase::PistonActions;
                while let Some(request) = graph.future.piston_requests.pop_front() {
                    if attempts.len() >= 4096 {
                        return Err(graph.unsupported(graph.program.pistons()[request.piston.index()].node, "piston probe action limit"));
                    }
                    let spec = graph.program.pistons()[request.piston.index()];
                    let Some(route) = request.source else {
                        return Err(graph.unsupported(spec.node, "piston probe request has no source route"));
                    };
                    let Some(route_source) = graph.program.nodes()[graph.program.routes()[route.index()].source.index()].position else {
                        return Err(graph.unsupported(spec.node, "piston probe source has no position"));
                    };
                    let before_states = graph.state.resources.to_vec();
                    let powered = graph.piston_powered(request.piston);
                    let accepted = graph.native_action_accepted(request);
                    graph.accept_action_using(request, None)?;
                    let changed: Vec<_> = graph
                        .state
                        .resources
                        .iter()
                        .zip(&before_states)
                        .enumerate()
                        .filter_map(|(index, (after, before))| (after != before).then_some(index))
                        .collect();
                    let owned = graph.program.resource_for_piston(request.piston);
                    if changed.len() > 1 || changed.first().is_some_and(|index| owned.is_none_or(|id| id.index() != *index)) {
                        return Err(graph.unsupported(spec.node, "piston probe found an unrepresented resource write"));
                    }
                    let before = owned.map(|id| {
                        let state = before_states[id.index()];
                        ResourcePosition {
                            id: id.index(),
                            position: graph.program.contacts()[state.at.index()].position,
                            enabled: state.enabled,
                        }
                    });
                    let after = owned.map(|id| resource_position(graph, id));
                    let observations = outputs
                        .iter()
                        .zip(&output_nodes)
                        .map(|(&position, &node)| {
                            let state = graph.node_state(node);
                            OutputObservation {
                                position,
                                input_strength: graph.input_strength(node, InputPort::Main),
                                powered: state.powered,
                                locked: state.repeater_locked,
                            }
                        })
                        .collect();
                    let pending_after = pending_work(graph);
                    attempts.push(PistonAttempt {
                        tick,
                        source: route_source,
                        route: route.index(),
                        actor: spec.position,
                        action: request.action,
                        powered,
                        accepted,
                        before,
                        after,
                        outputs: observations,
                        pending_after,
                    });
                }
                Ok(())
            })
            .map_err(|error| error.to_string())?;
    }
    let pending = pending_work(&graph);
    let settled = pending.scheduled.is_empty() && pending.requests.is_empty() && pending.deferred.is_empty();
    if require_settled && !settled {
        return Err(format!(
            "piston probe did not settle after {ticks} ticks; pending={pending:?}; attempts={attempts:?}"
        ));
    }
    let final_resources = (0..graph.state.resources.len())
        .map(|index| resource_position(&graph, ResourceId::new(index as u32)))
        .collect();
    Ok(PistonTrial {
        stimulus: source,
        input_before,
        input_events: events.to_vec(),
        initial_resources,
        attempts,
        final_resources,
        settled,
        pending,
        clock: None,
        clock_retractions: Vec::new(),
    })
}

pub(crate) fn pending_work(graph: &GraphBackend) -> PendingWork {
    PendingWork {
        scheduled: graph
            .future
            .scheduler
            .iter()
            .map(|(tick, delay, priority)| (tick.position, delay, priority))
            .collect(),
        requests: graph
            .future
            .piston_requests
            .iter()
            .map(|request| {
                (
                    graph.program.pistons()[request.piston.index()].position,
                    request.action,
                    request.source.map(|route| route.index()),
                )
            })
            .collect(),
        deferred: graph
            .future
            .deferred_pistons
            .iter()
            .map(|event| (graph.program.pistons()[event.piston.index()].position, event.due_tick, event.extending))
            .collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::redstone;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{Lever, LeverFace};
    use mchprs_blocks::{BlockDirection, BlockFacing};

    pub(crate) const ACTOR: BlockPos = BlockPos::new(5, 20, 5);
    pub(crate) const LEVER: BlockPos = BlockPos::new(4, 20, 5);
    pub(crate) const NEAR: BlockPos = BlockPos::new(6, 20, 5);
    pub(crate) const FAR: BlockPos = BlockPos::new(7, 20, 5);
    pub(crate) const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));

    pub(crate) fn fixture() -> PlotWorld {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(
            ACTOR,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: false,
                },
            },
        );
        world.set_block(NEAR, Block::RedstoneBlock);
        world.set_block(LEVER.offset(BlockFace::West), Block::Stone {});
        world.set_block(
            LEVER,
            Block::Lever {
                lever: Lever::new(LeverFace::Wall, BlockDirection::East, false),
            },
        );
        world
    }

    #[test]
    fn physical_probe_records_source_and_resource_move_against_native() {
        let world = fixture();
        let before = [ACTOR, LEVER, NEAR, FAR].map(|pos| world.get_block_raw(pos));
        let trials = probe(&world, BOUNDS, &[LEVER], 4).unwrap();
        let trial = &trials[0];
        assert!(!trial.input_before);
        let attempt = trial
            .attempts
            .iter()
            .find(|attempt| attempt.actor == ACTOR && attempt.accepted)
            .expect("piston accepted its request");
        assert_eq!(attempt.action, mchprs_world::PistonAction::Extend);
        assert_eq!(attempt.before.as_ref().unwrap().position, NEAR);
        assert_eq!(attempt.after.as_ref().unwrap().position, FAR);
        assert_eq!(attempt.before.as_ref().unwrap().id, attempt.after.as_ref().unwrap().id);

        let mut native = world.activation_snapshot();
        let Block::Lever { mut lever } = native.get_block(LEVER) else {
            panic!("missing lever")
        };
        lever.powered = true;
        native.set_block(LEVER, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut native, LEVER);
        redstone::update_surrounding_blocks(&mut native, LEVER.offset(BlockFace::West));
        for _ in 0..4 {
            native.tick_interpreted();
        }
        assert_eq!(native.get_block(FAR), Block::RedstoneBlock);
        assert!(
            trial
                .final_resources
                .iter()
                .any(|resource| resource.id == attempt.before.as_ref().unwrap().id && resource.position == FAR && resource.enabled)
        );
        assert_eq!([ACTOR, LEVER, NEAR, FAR].map(|pos| world.get_block_raw(pos)), before);
        assert!(world.scheduler().iter_entries().next().is_none());
    }

    #[test]
    fn physical_probe_records_canceled_request_without_moving_resource() {
        let world = fixture();
        let trial = probe_sequence(&world, BOUNDS, &[LEVER, LEVER], 4).unwrap();
        let attempt = trial.attempts.iter().find(|attempt| attempt.actor == ACTOR).expect("piston request");
        assert!(!attempt.powered);
        assert!(!attempt.accepted);
        assert_eq!(attempt.before, attempt.after);
        assert_eq!(trial.initial_resources, trial.final_resources);
        assert_eq!(trial.input_events, [LEVER, LEVER]);
    }

    pub(crate) fn test_pistion_fixture() -> (PlotWorld, (BlockPos, BlockPos), Vec<BlockPos>) {
        use crate::plot::worldedit::{load_schematic, paste_clipboard};
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/test-prefix-20261008");
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("download-manifest.json")).unwrap()).unwrap();
        let expected = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == "TEST_PISTION.schem")
            .unwrap();
        let bytes = std::fs::read(root.join("TEST_PISTION.schem")).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected["sha256"].as_str().unwrap());
        let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
        let origin = BlockPos::new(8, 12, 8);
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..crate::plot::PLOT_WIDTH)
                .flat_map(|x| (0..crate::plot::PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
                .collect(),
            Default::default(),
        );
        paste_clipboard(
            &mut world,
            &clipboard,
            origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
            false,
        );
        for z in 0..clipboard.size_z as i32 {
            for x in 0..clipboard.size_x as i32 {
                let position = origin + BlockPos::new(x, 0, z);
                let below = position.offset(BlockFace::Bottom);
                if crate::interaction::attachment_support(world.get_block(position), position).is_some_and(|(support, _)| support == below)
                    && world.get_block(below) == Block::Air
                {
                    world.set_block(below, Block::Stone {});
                }
            }
        }
        let bounds = (
            origin - BlockPos::new(2, 2, 2),
            origin + BlockPos::new(clipboard.size_x as i32 + 1, clipboard.size_y as i32 + 1, clipboard.size_z as i32 + 1),
        );
        let mut sources = Vec::new();
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let pos = BlockPos::new(x, y, z);
                    if matches!(world.get_block(pos), Block::Lever { .. }) {
                        sources.push(pos);
                    }
                }
            }
        }
        assert!(!sources.is_empty());
        (world, bounds, sources)
    }

    #[test]
    fn test_pistion_fixture_records_source_qualified_trial() {
        let (world, bounds, sources) = test_pistion_fixture();
        let mut pistons = Vec::new();
        let mut outputs = Vec::new();
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let pos = BlockPos::new(x, y, z);
                    if matches!(world.get_block(pos), Block::Piston { .. }) {
                        pistons.push(pos);
                    }
                    if matches!(world.get_block(pos), Block::RedstoneRepeater { .. }) {
                        outputs.push(pos);
                    }
                }
            }
        }
        assert_eq!(pistons.len(), 1);
        assert_eq!(sources.len(), 1);
        assert!(!outputs.is_empty());
        let clock = pistons[0];
        let trials = probe_clock_outputs(&world, bounds, &sources, clock, &outputs, 12).unwrap();
        let trial = &trials[0];
        assert!(!trial.settled, "clock work must remain visible at the observation boundary");
        assert!(!trial.pending.deferred.is_empty());
        assert!(trial.clock_retractions.len() >= 2, "observe repeated retractions, not a fixed cadence");
        let actual: Vec<_> = trial
            .attempts
            .iter()
            .filter(|attempt| attempt.accepted)
            .map(|attempt| (attempt.tick as u64, attempt.actor, attempt.action, attempt.outputs.clone()))
            .collect();

        let mut native = world.activation_snapshot();
        let source = sources[0];
        let Block::Lever { mut lever } = native.get_block(source) else {
            panic!("missing lever")
        };
        lever.powered = !lever.powered;
        native.set_block(source, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut native, source);
        let support = match lever.face {
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        redstone::update_surrounding_blocks(&mut native, source.offset(support));
        let mut expected = Vec::new();
        let mut operations = 0;
        while native.piston_state().logical_tick < 12 || native.piston_state().phase != mchprs_world::AdvancePhase::BetweenTicks {
            let trace = crate::redstone::piston::trace::capture(|| native.picotick_advance(1));
            operations += 1;
            assert!(operations < 10_000, "native probe exceeded operation bound");
            for entry in trace {
                if let crate::redstone::piston::trace::Operation::Applied(event) = entry.operation {
                    let observations = outputs
                        .iter()
                        .map(|&position| {
                            let Block::RedstoneRepeater { repeater } = native.get_block(position) else {
                                panic!("missing output repeater")
                            };
                            OutputObservation {
                                position,
                                input_strength: redstone::diode_get_input_strength(&native, position, repeater.facing),
                                powered: repeater.powered,
                                locked: repeater.locked,
                            }
                        })
                        .collect();
                    expected.push((entry.tick, event.pos, event.action, observations));
                }
            }
        }
        assert_eq!(actual, expected, "physical Graph and native output at each accepted piston action");
        for &index in &trial.clock_retractions {
            let edge = &trial.attempts[index];
            println!(
                "falling action tick {} route {} outputs {:?} pending {:?}",
                edge.tick, edge.route, edge.outputs, edge.pending_after
            );
        }
        assert!(probe(&world, bounds, &sources, 12).unwrap_err().contains("did not settle"));
    }
}
