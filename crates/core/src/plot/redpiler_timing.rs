//! Cold native probes for timing diagnostics; this code never touches the live plot.
use super::PlotWorld;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::diagnostics::timing::{
    TimingHazard, TimingProbeReport, TimingProbeUnknown, TimingSourceKind, TimingUnknownKind, TimingWireChange,
};
use crate::redpiler::program_graph::{CircuitNodeKind, CompileGraph, ProfileElectrical, SiteSource};
use crate::redstone::{self, piston};
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{AdvancePhase, PistonAction, PistonEvent};
use rustc_hash::{FxHashMap, FxHashSet};

const MAX_SOURCE_PROBES: usize = 16;
const MAX_COHORTS_PER_SOURCE: u32 = 64;
const MAX_OPERATIONS_PER_SOURCE: usize = 512;
const MAX_POWER_CHECKS_PER_SOURCE: usize = 8192;
const MAX_CONSUMER_COMPARISONS_PER_SOURCE: usize = 8192;
const WIRE_WITNESS_RADIUS: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FallingSource {
    /// This labels the physical source removal applied to the replay clone.
    kind: TimingSourceKind,
    /// This is the absolute block position of the source.
    position: BlockPos,
}

#[derive(Debug, Clone)]
struct AcceptedConsumer {
    /// This is the accepted base whose power will be tracked for late inhibition.
    position: BlockPos,
    /// This facing is retained for the native live-power query.
    facing: mchprs_blocks::BlockFacing,
    /// This is the public nano cohort in which its retraction was accepted.
    cohort: u32,
    /// This is an optional local wire drop observed during its retraction.
    wire_change: Option<TimingWireChange>,
}

#[derive(Clone, Copy)]
struct AcceptedRetraction {
    /// This is the queued native retraction whose live predicate passed.
    event: PistonEvent,
    /// This is the accepted actor's facing used for later live power reads.
    facing: mchprs_blocks::BlockFacing,
}

enum ReplayResult {
    Complete(Vec<TimingHazard>),
    Unknown(TimingUnknownKind),
    WorkCap,
}

/// Probe each eligible fixed source independently while preserving native queue semantics.
pub(crate) fn probe(world: &PlotWorld, graph: &CompileGraph, monitor: &TaskMonitor) -> TimingProbeReport {
    let mut result = TimingProbeReport::default();
    let moving_contacts = moving_contact_positions(graph);
    let mut sources = source_candidates(world, graph, &moving_contacts);
    sources.sort_by_key(|source| (source.position.x, source.position.y, source.position.z, source.kind as u8));
    sources.dedup();
    result.source_candidates = sources.len();
    result.source_cap_reached = sources.len() > MAX_SOURCE_PROBES;

    let wires: FxHashSet<_> = graph
        .nodes
        .iter()
        .filter_map(|node| matches!(&node.kind, CircuitNodeKind::Wire(_)).then_some(node.position).flatten())
        .collect();

    for source in sources.into_iter().take(MAX_SOURCE_PROBES) {
        if monitor.cancelled() {
            result.unknown.push(TimingProbeUnknown {
                source: Some((source.kind, source.position)),
                kind: TimingUnknownKind::Cancelled,
            });
            continue;
        }
        let mut replay = world.activation_snapshot();
        if !entry_is_quiescent(&replay) {
            result.unknown.push(TimingProbeUnknown {
                source: Some((source.kind, source.position)),
                kind: TimingUnknownKind::PendingSnapshot,
            });
            continue;
        }
        match apply_falling_source(&mut replay, source) {
            Ok(()) => {}
            Err(_) => {
                result.unknown.push(TimingProbeUnknown {
                    source: Some((source.kind, source.position)),
                    kind: TimingUnknownKind::Unsupported,
                });
                continue;
            }
        }
        match replay_first_piston_phase(&mut replay, source, &wires, monitor) {
            ReplayResult::Complete(hazards) => {
                result.sources_tested += 1;
                result.hazards.extend(hazards);
            }
            ReplayResult::Unknown(kind) => result.unknown.push(TimingProbeUnknown {
                source: Some((source.kind, source.position)),
                kind,
            }),
            ReplayResult::WorkCap => {
                result.work_cap_reached = true;
                result.unknown.push(TimingProbeUnknown {
                    source: Some((source.kind, source.position)),
                    kind: TimingUnknownKind::WorkBound,
                });
            }
        }
    }
    result
}

fn source_candidates(world: &PlotWorld, graph: &CompileGraph, moving_contacts: &FxHashSet<BlockPos>) -> Vec<FallingSource> {
    let mut sources = FxHashSet::default();
    for node in &graph.nodes {
        let Some(position) = node.position else { continue };
        if moving_contacts.contains(&position) {
            continue;
        }
        match (&node.kind, world.get_block(position)) {
            (CircuitNodeKind::Electrical(_), Block::RedstoneBlock {}) => {
                sources.insert(FallingSource {
                    kind: TimingSourceKind::RedstoneBlock,
                    position,
                });
            }
            (CircuitNodeKind::Electrical(_), Block::Lever { lever }) if lever.powered => {
                sources.insert(FallingSource {
                    kind: TimingSourceKind::Lever,
                    position,
                });
            }
            _ => {}
        }
    }
    for site in &graph.sites {
        let SiteSource::Constant(profile) = site.source else { continue };
        if graph.profiles[profile.index()].electrical != ProfileElectrical::RedstoneSource
            || moving_contacts.contains(&site.position)
            || !matches!(world.get_block(site.position), Block::RedstoneBlock {})
        {
            continue;
        }
        sources.insert(FallingSource {
            kind: TimingSourceKind::RedstoneBlock,
            position: site.position,
        });
    }
    sources.into_iter().collect()
}

fn moving_contact_positions(graph: &CompileGraph) -> FxHashSet<BlockPos> {
    let mut positions: FxHashSet<_> = graph.contacts.iter().map(|contact| contact.position).collect();
    positions.extend(
        graph
            .resources
            .iter()
            .flat_map(|resource| resource.contacts.iter().map(|contact| graph.contacts[contact.index()].position)),
    );
    positions
}

fn entry_is_quiescent(world: &PlotWorld) -> bool {
    let state = world.piston_state();
    state.phase == AdvancePhase::BetweenTicks
        && state.events.is_empty()
        && state.motions.is_empty()
        && state.movement_work.is_empty()
        && state.movement_cursor == 0
        && world.scheduler().iter_entries().next().is_none()
}

fn apply_falling_source(world: &mut PlotWorld, source: FallingSource) -> Result<(), String> {
    match source.kind {
        TimingSourceKind::RedstoneBlock => {
            let block = world.get_block(source.position);
            if !matches!(block, Block::RedstoneBlock {}) {
                return Err("candidate is no longer a fixed redstone block".into());
            }
            crate::interaction::destroy(block, world, source.position);
        }
        TimingSourceKind::Lever => {
            let Block::Lever { mut lever } = world.get_block(source.position) else {
                return Err("candidate is no longer a lever".into());
            };
            if !lever.powered {
                return Err("candidate lever is not powered in the snapshot".into());
            }
            lever.powered = false;
            world.set_block(source.position, Block::Lever { lever });
            redstone::update_surrounding_blocks(world, source.position);
            let attachment = match lever.face {
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            redstone::update_surrounding_blocks(world, source.position.offset(attachment));
        }
    }
    Ok(())
}

fn replay_first_piston_phase(world: &mut PlotWorld, source: FallingSource, wires: &FxHashSet<BlockPos>, monitor: &TaskMonitor) -> ReplayResult {
    let mut consumers = Vec::<AcceptedConsumer>::new();
    let mut hazards = Vec::new();
    let mut operations = 0;
    let mut cohorts = 0u32;
    let mut power_checks = 0usize;
    let mut consumer_comparisons = 0usize;
    let mut saw_piston_phase = false;

    loop {
        if monitor.cancelled() {
            return ReplayResult::Unknown(TimingUnknownKind::Cancelled);
        }
        if operations >= MAX_OPERATIONS_PER_SOURCE || cohorts >= MAX_COHORTS_PER_SOURCE {
            return ReplayResult::WorkCap;
        }
        if !world.prepare_operation() {
            // prepare_operation has advanced past an empty PistonEvents queue into
            // movement (or completed it); with a quiescent entry there is no actor
            // acceptance to compare, so this source probe is complete and silent.
            return ReplayResult::Complete(hazards);
        }
        let phase = world.piston_state.phase;
        if phase == AdvancePhase::MovingEntities {
            return if saw_piston_phase {
                ReplayResult::Complete(hazards)
            } else {
                ReplayResult::Unknown(TimingUnknownKind::Unsupported)
            };
        }
        if phase == AdvancePhase::PistonEvents {
            saw_piston_phase = true;
            if world.piston_state.events.is_empty() {
                return ReplayResult::Complete(hazards);
            }
        }
        let count = world.current_phase_operation_count();
        if count == 0 {
            return ReplayResult::Unknown(TimingUnknownKind::Unsupported);
        }
        cohorts += 1;
        for _ in 0..count {
            if operations >= MAX_OPERATIONS_PER_SOURCE {
                return ReplayResult::WorkCap;
            }
            operations += 1;
            let candidate_event = if phase == AdvancePhase::PistonEvents {
                world.piston_state.events.front().copied()
            } else {
                None
            };
            let accepting = candidate_event.and_then(|event| accepted_retraction_before(world, event));
            if candidate_event.is_some_and(|event| matches!(event.action, PistonAction::Retract | PistonAction::RetractWithoutPull)) {
                power_checks += 1;
            }
            if power_checks > MAX_POWER_CHECKS_PER_SOURCE {
                return ReplayResult::WorkCap;
            }
            let before_power = if accepting.is_some() {
                consumer_comparisons = consumer_comparisons.saturating_add(consumers.len());
                power_checks = power_checks.saturating_add(consumers.len());
                if consumer_comparisons > MAX_CONSUMER_COMPARISONS_PER_SOURCE || power_checks > MAX_POWER_CHECKS_PER_SOURCE {
                    return ReplayResult::WorkCap;
                }
                let mut values = Vec::with_capacity(consumers.len());
                for consumer in &consumers {
                    if monitor.cancelled() {
                        return ReplayResult::Unknown(TimingUnknownKind::Cancelled);
                    }
                    values.push(piston::should_piston_extend(world, consumer.facing, consumer.position));
                }
                values
            } else {
                Vec::new()
            };
            let before_wires = accepting
                .as_ref()
                .map(|accepted| sample_local_wires(world, wires, accepted.event.pos))
                .unwrap_or_default();

            world.advance_operation();

            let mut accepted_actor = None;
            if let Some(accepted) = accepting {
                let event = accepted.event;
                if accepted_retraction_after(world, event) {
                    accepted_actor = Some(event.pos);
                    let wire_change = first_wire_drop(&before_wires, &sample_local_wires(world, wires, event.pos));
                    consumers.push(AcceptedConsumer {
                        position: event.pos,
                        facing: accepted.facing,
                        cohort: cohorts,
                        wire_change,
                    });
                }
            }
            let Some(inhibitor) = accepted_actor else { continue };
            for (index, consumer) in consumers.iter().enumerate() {
                if monitor.cancelled() {
                    return ReplayResult::Unknown(TimingUnknownKind::Cancelled);
                }
                consumer_comparisons += 1;
                if consumer_comparisons > MAX_CONSUMER_COMPARISONS_PER_SOURCE {
                    return ReplayResult::WorkCap;
                }
                if consumer.position == inhibitor || before_power.get(index).copied() != Some(false) {
                    continue;
                }
                power_checks += 1;
                if power_checks > MAX_POWER_CHECKS_PER_SOURCE {
                    return ReplayResult::WorkCap;
                }
                if piston::should_piston_extend(world, consumer.facing, consumer.position) {
                    hazards.push(TimingHazard {
                        source_kind: source.kind,
                        source: source.position,
                        consumer: consumer.position,
                        inhibitor,
                        consumer_cohort: consumer.cohort,
                        inhibit_cohort: cohorts,
                        wire_change: consumer.wire_change,
                    });
                }
            }
        }
        // Once this public count-snapshot queue is empty, stop before native movement/reset phases.
        if phase == AdvancePhase::PistonEvents && world.piston_state.events.is_empty() {
            return ReplayResult::Complete(hazards);
        }
    }
}

fn accepted_retraction_before(world: &PlotWorld, event: PistonEvent) -> Option<AcceptedRetraction> {
    if !matches!(event.action, PistonAction::Retract | PistonAction::RetractWithoutPull) {
        return None;
    }
    let Block::Piston { piston: base } = world.get_block(event.pos) else {
        return None;
    };
    (base.extended && base.sticky == event.sticky && !piston::should_piston_extend(world, base.facing, event.pos))
        .then_some(AcceptedRetraction { event, facing: base.facing })
}

fn accepted_retraction_after(world: &PlotWorld, event: PistonEvent) -> bool {
    matches!(
        (
            world.get_block(event.pos),
            world.get_block_entity(event.pos),
            world.piston_motion_index(event.pos, None).is_some(),
        ),
        (
            Block::MovingPiston { moving },
            Some(BlockEntity::MovingPiston(entity)),
            true,
        ) if !entity.extending
            && entity.source
            && entity.facing == event.facing
            && moving.facing == event.facing.into()
            && moving.sticky == event.sticky
            && matches!(Block::from_id(entity.block_state), Block::Piston { piston } if !piston.extended && piston.sticky == event.sticky)
    )
}

fn sample_local_wires(world: &PlotWorld, wires: &FxHashSet<BlockPos>, origin: BlockPos) -> FxHashMap<BlockPos, u8> {
    let mut samples = FxHashMap::default();
    for dx in -WIRE_WITNESS_RADIUS..=WIRE_WITNESS_RADIUS {
        for dy in -WIRE_WITNESS_RADIUS..=WIRE_WITNESS_RADIUS {
            for dz in -WIRE_WITNESS_RADIUS..=WIRE_WITNESS_RADIUS {
                if dx.abs() + dy.abs() + dz.abs() > WIRE_WITNESS_RADIUS {
                    continue;
                }
                let position = BlockPos::new(origin.x + dx, origin.y + dy, origin.z + dz);
                if !wires.contains(&position) {
                    continue;
                }
                if let Block::RedstoneWire { wire } = world.get_block(position) {
                    samples.insert(position, wire.power);
                }
            }
        }
    }
    samples
}

fn first_wire_drop(before: &FxHashMap<BlockPos, u8>, after: &FxHashMap<BlockPos, u8>) -> Option<TimingWireChange> {
    before
        .iter()
        .filter_map(|(&position, &old)| {
            let new = *after.get(&position)?;
            (new < old).then_some(TimingWireChange {
                position,
                before: old,
                after: new,
            })
        })
        .min_by_key(|change| (change.after != 0, change.position.x, change.position.y, change.position.z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::CompilerOptions;
    use crate::redpiler::analysis::graph::prepare_candidate_graph;
    use crate::redstone::instant_piston_tests::{ORIGIN, balanced_nanotick_variant, fixture, load};
    use crate::world::World;
    use mchprs_blocks::blocks::Block;
    use std::sync::Arc;

    fn graph(world: &PlotWorld, dimensions: (u32, u32, u32), optimization_level: usize) -> CompileGraph {
        let max = ORIGIN + BlockPos::new(dimensions.0 as i32 - 1, dimensions.1 as i32 - 1, dimensions.2 as i32 - 1);
        let options = CompilerOptions {
            optimization_level: Some(optimization_level),
            ..Default::default()
        };
        prepare_candidate_graph(world, (ORIGIN, max), &[], &options, Arc::new(TaskMonitor::default()))
            .expect("fixture candidate graph")
            .graph
    }

    #[test]
    fn native_falling_probe_finds_fixture_hazard_and_leaves_snapshot_untouched() {
        let manifest = fixture("nanotick_example");
        let (world, _, dimensions) = load(&manifest, 0);
        let trigger = ORIGIN + BlockPos::new(0, 2, 0);
        let output = ORIGIN + BlockPos::new(1, 2, 12);
        let output_block = world.get_block(output);
        let piston_state = bincode::serialize(world.piston_state()).unwrap();
        let queued_ticks: Vec<_> = world.scheduler().iter_entries().collect();
        for optimization_level in [0, 4] {
            let graph = graph(&world, dimensions, optimization_level);
            let report = probe(&world, &graph, &TaskMonitor::default());

            let hazard = report
                .hazards
                .iter()
                .find(|hazard| hazard.source == trigger)
                .expect("the source-falling replay should expose late inhibition");
            assert_eq!(hazard.consumer, ORIGIN + BlockPos::new(1, 2, 9));
            assert_eq!(hazard.inhibitor, ORIGIN + BlockPos::new(2, 2, 5));
            assert_eq!((hazard.consumer_cohort, hazard.inhibit_cohort), (2, 3));
            assert_eq!(
                hazard.wire_change,
                Some(TimingWireChange {
                    position: output,
                    before: 15,
                    after: 0,
                })
            );
        }
        assert_eq!(world.get_block(trigger), Block::RedstoneBlock {});
        assert_eq!(world.get_block(output), output_block);
        assert_eq!(bincode::serialize(world.piston_state()).unwrap(), piston_state);
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), queued_ticks);
    }

    #[test]
    fn balanced_physical_variant_has_no_late_inhibition_finding() {
        let (world, _, _, dimensions) = balanced_nanotick_variant();
        let graph = graph(&world, dimensions, 4);
        let report = probe(&world, &graph, &TaskMonitor::default());
        assert!(report.unknown.is_empty(), "{:?}", report.unknown);
        assert!(report.hazards.is_empty(), "{:?}", report.hazards);
    }

    #[test]
    fn pending_entry_queue_is_reported_unknown_without_a_hazard() {
        let manifest = fixture("nanotick_example");
        let (world, _, dimensions) = load(&manifest, 0);
        let graph = graph(&world, dimensions, 4);
        let mut pending = world.activation_snapshot();
        pending.piston_state.events.push_back(PistonEvent {
            pos: ORIGIN,
            sticky: false,
            facing: BlockFace::North,
            action: PistonAction::Retract,
        });

        let report = probe(&pending, &graph, &TaskMonitor::default());
        assert_eq!(report.sources_tested, 0);
        assert!(report.hazards.is_empty());
        assert_eq!(report.unknown.len(), report.source_candidates);
    }

    #[test]
    fn cancelled_retraction_request_is_not_an_accepted_consumer() {
        let manifest = fixture("nanotick_example");
        let (mut world, _, _) = load(&manifest, 0);
        let position = ORIGIN + BlockPos::new(0, 2, 5);
        let Block::Piston { mut piston } = world.get_block(position) else {
            panic!("fixture ordinary actor should be a piston");
        };
        piston.extended = false;
        world.set_block(position, Block::Piston { piston });
        let request = PistonEvent {
            pos: position,
            sticky: piston.sticky,
            facing: piston.facing.into(),
            action: PistonAction::Retract,
        };

        assert!(accepted_retraction_before(&world, request).is_none());
    }

    #[test]
    fn cancelled_probe_does_not_report_successful_coverage() {
        let manifest = fixture("nanotick_example");
        let (world, _, dimensions) = load(&manifest, 0);
        let graph = graph(&world, dimensions, 4);
        let monitor = TaskMonitor::default();
        monitor.cancel();

        let report = probe(&world, &graph, &monitor);
        assert_eq!(report.sources_tested, 0);
        assert_eq!(report.unknown.len(), report.source_candidates);
        assert!(report.hazards.is_empty());
    }

    #[test]
    fn first_piston_phase_work_limit_returns_unknown_instead_of_silent_success() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![crate::world::storage::Chunk::empty(0, 0)], Default::default());
        let total_events = MAX_OPERATIONS_PER_SOURCE + 1;
        for index in 0..total_events {
            let position = BlockPos::new((index % 16) as i32, 1 + (index / 256) as i32, ((index / 16) % 16) as i32);
            world.enqueue_piston_event(PistonEvent {
                pos: position,
                sticky: false,
                facing: BlockFace::North,
                action: PistonAction::Retract,
            });
        }
        assert_eq!(world.piston_state.events.len(), total_events);

        let result = replay_first_piston_phase(
            &mut world,
            FallingSource {
                kind: TimingSourceKind::RedstoneBlock,
                position: BlockPos::new(1, 1, 1),
            },
            &FxHashSet::default(),
            &TaskMonitor::default(),
        );
        assert!(matches!(result, ReplayResult::WorkCap));
    }
}
