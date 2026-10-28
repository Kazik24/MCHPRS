//! Native reset-edge timelines for one source-derived observer cell use.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::redpiler::program_graph::{CellReset, ComponentState, PistonAvailability, PistonLane};
use crate::redstone::piston::trace::{self, DetailedEntry, Stage};
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const TIMELINE_TICKS: usize = 96;

#[derive(Debug, Clone, Copy)]
struct ActorProbe {
    actor: PistonId,
    observer: NodeId,
    resource: ResourceId,
    near: ContactId,
    far: ContactId,
    actor_position: BlockPos,
    observer_position: BlockPos,
    near_position: BlockPos,
    far_position: BlockPos,
}

#[derive(Debug)]
struct NativeSnapshot {
    tick: u64,
    phase: mchprs_world::AdvancePhase,
    observer_powered: bool,
    piston_extended: Option<bool>,
    near_payload: bool,
    far_payload: bool,
}

#[derive(Debug)]
struct GraphSnapshot {
    tick: u64,
    phase: GraphPhase,
    observer_powered: Option<bool>,
    piston_extended: bool,
    availability: PistonAvailability,
    resource_enabled: bool,
    resource_at: ContactId,
    near_payload: bool,
    far_payload: bool,
}

fn counter_trigger() -> BlockPos {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let position = &manifest["ports"]["inputs"]["trigger"];
    ORIGIN
        + BlockPos::new(
            position[0].as_i64().unwrap() as i32,
            position[1].as_i64().unwrap() as i32,
            position[2].as_i64().unwrap() as i32,
        )
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("counter trigger is not a lever at {position:?}");
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

fn prepare_counter(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> passes::rich::PreparedGraph {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(
        world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
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
    prepared
}

fn observer_actor_probes(graph: &CompileGraph) -> Vec<ActorProbe> {
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Vec::new();
    };
    let mut seen = rustc_hash::FxHashSet::default();
    let mut probes = Vec::new();
    for use_ in catalog.cells.iter().flat_map(|cell| cell.uses.iter()) {
        let CellReset::Observer { node: observer, .. } = &use_.reset else {
            continue;
        };
        if !seen.insert(use_.actor) {
            continue;
        }
        let piston = graph.pistons[use_.actor.index()];
        let PistonLane::SingleResource { far } = piston.lane else {
            continue;
        };
        let Some((resource_index, _)) = graph
            .resources
            .iter()
            .enumerate()
            .find(|(_, resource)| resource.contacts.contains(&piston.near) && resource.contacts.contains(&far))
        else {
            continue;
        };
        let Some(observer_position) = graph.nodes[observer.index()].position else {
            continue;
        };
        probes.push(ActorProbe {
            actor: use_.actor,
            observer: *observer,
            resource: ResourceId::new(resource_index as u32),
            near: piston.near,
            far,
            actor_position: piston.position,
            observer_position,
            near_position: graph.contacts[piston.near.index()].position,
            far_position: graph.contacts[far.index()].position,
        });
    }
    probes.sort_unstable_by_key(|probe| probe.actor);
    probes
}

fn has_retraction_and_extension(entries: &[DetailedEntry], position: BlockPos) -> bool {
    let mut retracted = false;
    for entry in entries.iter().filter(|entry| entry.pos == position && entry.stage == Stage::Accept) {
        let Some(event) = entry.event else { continue };
        match event.action {
            mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull => retracted = true,
            mchprs_world::PistonAction::Extend if retracted => return true,
            mchprs_world::PistonAction::Extend => {}
        }
    }
    false
}

fn native_timeline(mut world: PlotWorld, trigger: BlockPos, probe: ActorProbe) -> (Vec<DetailedEntry>, Vec<NativeSnapshot>) {
    let mut snapshots = Vec::with_capacity(TIMELINE_TICKS);
    let entries = trace::capture_detailed(&[probe.actor_position], || {
        set_native_lever(&mut world, trigger, true);
        for _ in 0..TIMELINE_TICKS {
            world.tick_interpreted();
            let observer_powered = matches!(world.get_block(probe.observer_position), Block::Observer { observer } if observer.powered);
            let piston_extended = match world.get_block(probe.actor_position) {
                Block::Piston { piston } => Some(piston.extended),
                _ => None,
            };
            snapshots.push(NativeSnapshot {
                tick: world.piston_state().logical_tick,
                phase: world.piston_state().phase,
                observer_powered,
                piston_extended,
                near_payload: matches!(world.get_block(probe.near_position), Block::RedstoneBlock { .. }),
                far_payload: matches!(world.get_block(probe.far_position), Block::RedstoneBlock { .. }),
            });
        }
    });
    (entries, snapshots)
}

fn graph_timeline(
    world: &mut PlotWorld,
    trigger: BlockPos,
    probe: ActorProbe,
    prepared: passes::rich::PreparedGraph,
) -> (Vec<GraphSnapshot>, Vec<(u64, mchprs_world::PistonAction)>) {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let lowered = prepared.lower(&monitor).unwrap();
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    backend.set_test_recording(true);
    backend.use_block(trigger).unwrap();
    backend.flush(world, false);
    let mut snapshots = Vec::with_capacity(TIMELINE_TICKS);
    for _ in 0..TIMELINE_TICKS {
        backend.tick().unwrap();
        backend.flush(world, false);
        let piston = backend.state.pistons[probe.actor.index()];
        let resource = backend.state.resources[probe.resource.index()];
        let observer_powered = match &backend.state.components[probe.observer.index()] {
            ComponentState::Electrical(state) => Some(state.powered),
            _ => None,
        };
        snapshots.push(GraphSnapshot {
            tick: backend.future.clock.logical_tick,
            phase: backend.future.clock.phase,
            observer_powered,
            piston_extended: piston.extended,
            availability: piston.availability,
            resource_enabled: resource.enabled,
            resource_at: resource.at,
            near_payload: matches!(world.get_block(probe.near_position), Block::RedstoneBlock { .. }),
            far_payload: matches!(world.get_block(probe.far_position), Block::RedstoneBlock { .. }),
        });
    }
    let entries = backend
        .native_piston_trace()
        .iter()
        .filter(|entry| match &entry.operation {
            trace::Operation::Sample { pos, .. } => *pos == probe.actor_position,
            trace::Operation::Applied(event) => event.pos == probe.actor_position,
        })
        .filter_map(|entry| match &entry.operation {
            trace::Operation::Applied(event) => Some((entry.tick, event.action)),
            trace::Operation::Sample { .. } => None,
        })
        .collect();
    (snapshots, entries)
}

#[test]
#[ignore = "research: source-derived observer reset and one-shot latch clear edge timeline"]
fn observer_firing_latch_reset_edge_timeline() {
    let (mut native, bounds) = super::adder_tests::counter_world();
    let (mut graph_world, graph_bounds) = super::adder_tests::counter_world();
    assert_eq!(bounds, graph_bounds);
    let trigger = counter_trigger();
    let prepared_for_native = prepare_counter(&native, bounds);
    let candidates = observer_actor_probes(&prepared_for_native.graph);
    assert!(!candidates.is_empty(), "COUNTER must expose at least one cataloged observer-reset actor");
    let positions: Vec<_> = candidates.iter().map(|probe| probe.actor_position).collect();
    let mut initial_entries = Vec::new();
    trace::capture_detailed(&positions, || {
        set_native_lever(&mut native, trigger, true);
        for _ in 0..TIMELINE_TICKS {
            native.tick_interpreted();
        }
    })
    .into_iter()
    .for_each(|entry| initial_entries.push(entry));
    let probe = candidates
        .iter()
        .copied()
        .find(|probe| has_retraction_and_extension(&initial_entries, probe.actor_position))
        .unwrap_or_else(|| candidates[0]);

    let (native, _) = super::adder_tests::counter_world();
    let (native_entries, native_snapshots) = native_timeline(native, trigger, probe);
    let mut native_stages = native_entries
        .iter()
        .filter(|entry| entry.pos == probe.actor_position)
        .collect::<Vec<_>>();
    native_stages.sort_by_key(|entry| (entry.tick, entry.phase as u8, entry.stage as u8));

    let prepared_for_graph = prepare_counter(&graph_world, bounds);
    let (graph_snapshots, graph_entries) = graph_timeline(&mut graph_world, trigger, probe, prepared_for_graph);
    let graph = &prepared_for_native.graph;
    let actor = graph.pistons[probe.actor.index()];
    let observer_kind = &graph.nodes[probe.observer.index()].kind;
    eprintln!(
        "observer firing reset probe actor={:?}@{:?} observer={:?}@{:?} kind={observer_kind:?} resource={:?} near={:?}@{:?} far={:?}@{:?} accepted_cycle={} native_entries={} graph_entries={}",
        probe.actor,
        probe.actor_position,
        probe.observer,
        probe.observer_position,
        probe.resource,
        probe.near,
        probe.near_position,
        probe.far,
        probe.far_position,
        has_retraction_and_extension(&native_entries, probe.actor_position),
        native_entries.len(),
        graph_entries.len(),
    );
    eprintln!("source actor spec: {actor:?}");
    for entry in native_stages {
        let state = native_snapshots.iter().find(|snapshot| snapshot.tick == entry.tick);
        eprintln!(
            "native cut: tick={} phase={:?} stage={:?} extended={:?} powered={} main={} quasi={} near_payload={} far_payload={} observer_powered={} event={:?}",
            entry.tick,
            entry.phase,
            entry.stage,
            entry.extended,
            entry.powered,
            entry.main,
            entry.quasi,
            state.is_some_and(|snapshot| snapshot.near_payload),
            state.is_some_and(|snapshot| snapshot.far_payload),
            state.is_some_and(|snapshot| snapshot.observer_powered),
            entry.event
        );
    }
    for snapshot in &native_snapshots {
        eprintln!(
            "native state: tick={} phase={:?} observer_powered={} piston_extended={:?} near_payload={} far_payload={}",
            snapshot.tick, snapshot.phase, snapshot.observer_powered, snapshot.piston_extended, snapshot.near_payload, snapshot.far_payload
        );
    }
    for snapshot in &graph_snapshots {
        eprintln!(
            "graph state: tick={} phase={:?} observer_powered={:?} piston_extended={} availability={:?} resource_enabled={} resource_at={:?} near_payload={} far_payload={}",
            snapshot.tick,
            snapshot.phase,
            snapshot.observer_powered,
            snapshot.piston_extended,
            snapshot.availability,
            snapshot.resource_enabled,
            snapshot.resource_at,
            snapshot.near_payload,
            snapshot.far_payload
        );
    }
    for entry in &graph_entries {
        eprintln!("graph accepted action: {entry:?}");
    }
    let accepted = |action| {
        native_entries
            .iter()
            .filter(|entry| entry.pos == probe.actor_position && entry.stage == Stage::Accept)
            .filter_map(|entry| entry.event.map(|event| (entry.tick, event.action)))
            .filter(|(_, seen)| *seen == action)
            .map(|(tick, _)| tick)
            .collect::<Vec<_>>()
    };
    let native_publications = native_snapshots
        .windows(2)
        .filter_map(|pair| (!pair[0].far_payload && pair[1].far_payload).then_some(pair[1].tick))
        .collect::<Vec<_>>();
    let observer_on = native_snapshots
        .windows(2)
        .filter_map(|pair| (!pair[0].observer_powered && pair[1].observer_powered).then_some(pair[1].tick))
        .collect::<Vec<_>>();
    let observer_off = native_snapshots
        .windows(2)
        .filter_map(|pair| (pair[0].observer_powered && !pair[1].observer_powered).then_some(pair[1].tick))
        .collect::<Vec<_>>();
    let graph_publications = graph_snapshots
        .windows(2)
        .filter_map(|pair| (!pair[0].far_payload && pair[1].far_payload).then_some(pair[1].tick))
        .collect::<Vec<_>>();

    // Clearing is a publication edge, not the observer's OFF edge. The
    // observer has reset the source latch by tick 12 in this trace, while the
    // owned payload is still unavailable until the matching extension is
    // fully published at the canonical Far contact.
    let retracts = accepted(mchprs_world::PistonAction::Retract);
    let extends = accepted(mchprs_world::PistonAction::Extend);
    let first_retract = *retracts.first().expect("observer cell must retract");
    let first_extend = *extends.first().expect("observer cell must extend");
    let first_observer_on = *observer_on.first().expect("observer must turn on");
    let first_observer_off = *observer_off.first().expect("observer must turn off");
    let first_native_publication = *native_publications.first().expect("resource must publish at Far");
    let next_retract = *retracts.get(1).expect("probe window must include the next cycle");
    assert!(first_retract < first_observer_on);
    assert!(first_observer_on < first_extend);
    assert!(first_extend < first_observer_off);
    assert!(first_observer_off < first_native_publication);
    assert!(first_native_publication < next_retract);

    let native_off = native_snapshots.iter().find(|s| s.tick == first_observer_off).unwrap();
    assert!(
        !native_off.far_payload,
        "observer OFF must not make the payload available before publication"
    );
    assert_eq!(native_off.piston_extended, Some(true), "extension acceptance precedes publication");
    assert!(
        native_snapshots
            .iter()
            .filter(|s| s.tick >= first_observer_off && s.tick < first_native_publication)
            .all(|s| !s.far_payload)
    );
    let native_published = native_snapshots.iter().find(|s| s.tick == first_native_publication).unwrap();
    assert!(native_published.far_payload);
    assert_eq!(native_published.piston_extended, Some(true));

    let graph_observer_off = graph_snapshots
        .windows(2)
        .find(|pair| pair[0].observer_powered == Some(true) && pair[1].observer_powered == Some(false))
        .map(|pair| pair[1].tick)
        .expect("Graph observer must turn off");
    let graph_publication = *graph_publications.first().expect("Graph resource must publish at Far");
    let graph_extend = graph_entries
        .iter()
        .find_map(|(tick, action)| (*action == mchprs_world::PistonAction::Extend).then_some(*tick))
        .expect("Graph must accept the matching extension");
    assert!(graph_observer_off < graph_publication);
    assert!(graph_extend < graph_publication, "resource publication follows accepted extension");
    let graph_off = graph_snapshots.iter().find(|s| s.tick == graph_observer_off).unwrap();
    assert!(
        !graph_off.resource_enabled || graph_off.availability != PistonAvailability::Ready,
        "observer OFF precedes ready/enabled resource publication"
    );
    assert!(
        graph_snapshots
            .iter()
            .filter(|s| s.tick >= graph_observer_off && s.tick < graph_publication)
            .all(|s| !s.resource_enabled || s.availability != PistonAvailability::Ready)
    );
    let graph_published = graph_snapshots.iter().find(|s| s.tick == graph_publication).unwrap();
    assert_eq!(graph_published.availability, PistonAvailability::Ready);
    assert!(graph_published.piston_extended);
    assert!(graph_published.resource_enabled);
    assert_eq!(graph_published.resource_at, probe.far);
    assert!(graph_published.far_payload);
    eprintln!(
        "reset edge order: native accepted_retract={:?} observer_on={observer_on:?} far_publication={native_publications:?} observer_off={observer_off:?} accepted_extend={:?} next_accepted_retract={:?}; Graph far_publication={graph_publications:?}",
        accepted(mchprs_world::PistonAction::Retract),
        accepted(mchprs_world::PistonAction::Extend),
        accepted(mchprs_world::PistonAction::Retract).into_iter().skip(1).collect::<Vec<_>>(),
    );
}
