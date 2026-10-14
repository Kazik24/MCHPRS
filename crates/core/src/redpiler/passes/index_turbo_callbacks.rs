//! Builds per-wire ranges for the ordered Turbo callback rows.
//!
//! The index skips rows for other canonical neighbors while retaining each
//! target's original alternatives in route order. It applies only when the 24
//! canonical target IDs are distinct and the route is grouped in that order;
//! all other wires keep the general linear search. It does not alter routes or
//! callback semantics. The builder runs after opt-01, and later route-changing
//! passes invalidate its table. Building costs one route scan per wire and
//! retains 25 `u32` offsets for each eligible wire.

use super::{GraphPass, PassContext};
use crate::redpiler::PreparationTableStatistics;
use crate::redpiler::program_graph::{CompileGraph, Route, WirePlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Delimits each canonical target's ordered slice in one wire's Turbo route.
pub(crate) struct TurboCallbackIndex {
    pub(crate) offsets: [u32; 25],
}

/// Adds the runtime lookup table for wires with canonical route grouping.
pub(crate) struct IndexTurboCallbacks;

impl GraphPass for IndexTurboCallbacks {
    fn name(&self) -> &'static str {
        "index-turbo-callbacks"
    }

    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String> {
        graph
            .validate()
            .map_err(|error| format!("Turbo callback index input graph is invalid: {error}"))
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let mut indices = Vec::with_capacity(graph.wires.len());
        for wire in &graph.wires {
            if context.monitor.cancelled() {
                return Err("cancelled while indexing Turbo callbacks".into());
            }
            indices.push(index_wire(wire, &graph.routes));
        }
        let indexed = indices.iter().filter(|index| index.is_some()).count();
        let wire_count = indices.len();
        graph.turbo_callback_indices = indices;
        context.monitor.record_preparation_tables(vec![
            PreparationTableStatistics {
                stage: "Turbo callback index",
                table: "wire bindings".into(),
                entries: wire_count,
                capacity: wire_count,
                estimated_bytes: wire_count.saturating_mul(std::mem::size_of::<Option<TurboCallbackIndex>>()),
            },
            PreparationTableStatistics {
                stage: "Turbo callback index",
                table: "indexed wires".into(),
                entries: indexed,
                capacity: indexed,
                estimated_bytes: 0,
            },
            PreparationTableStatistics {
                stage: "Turbo callback index",
                table: "linear fallback wires".into(),
                entries: wire_count - indexed,
                capacity: wire_count - indexed,
                estimated_bytes: 0,
            },
        ]);
        Ok(())
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }
}

fn build_turbo_callback_index(targets: &[crate::redpiler::program_graph::SiteId; 24], route: &Route) -> Option<TurboCallbackIndex> {
    if targets.iter().enumerate().any(|(slot, target)| targets[..slot].contains(target)) {
        return None;
    }
    u32::try_from(route.deliveries.len()).ok()?;
    let mut offsets = [0; 25];
    let mut row = 0;
    for (slot, target) in targets.iter().enumerate() {
        offsets[slot] = u32::try_from(row).ok()?;
        while route.deliveries.get(row).is_some_and(|delivery| delivery.target == *target) {
            row += 1;
        }
        offsets[slot + 1] = u32::try_from(row).ok()?;
    }
    (row == route.deliveries.len()).then_some(TurboCallbackIndex { offsets })
}

fn index_wire(wire: &WirePlan, routes: &[Route]) -> Option<TurboCallbackIndex> {
    wire.turbo_route
        .and_then(|route| routes.get(route.index()))
        .and_then(|route| build_turbo_callback_index(&wire.canonical_neighbors, route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::{
        CircuitDraft, CircuitNodeDraft, CircuitNodeKind, ComponentState, DeliveryKind, Guard, GuardId, NodeId, OperandView, Profile,
        ProfileElectrical, RouteDelivery, RouteId, SiteId, SiteSource, SiteSpec, WirePlan,
    };
    use mchprs_blocks::BlockPos;
    use mchprs_blocks::blocks::{Block, RedstoneWire};

    fn targets() -> [SiteId; 24] {
        std::array::from_fn(|slot| SiteId::new((slot * 7 + 3) as u32))
    }

    fn delivery(target: SiteId, sequence: u32) -> RouteDelivery {
        RouteDelivery {
            target,
            kind: DeliveryKind::Update { direction: None },
            guard: GuardId::new(0),
            view: OperandView::Captured,
            sequence,
            receiver: None,
        }
    }

    fn route(deliveries: Vec<RouteDelivery>) -> Route {
        Route {
            source: NodeId::new(0),
            deliveries: deliveries.into_boxed_slice(),
            #[cfg(feature = "graph-profile")]
            native_event_count: 0,
        }
    }

    fn graph(index: Option<TurboCallbackIndex>, deliveries: Vec<RouteDelivery>) -> CompileGraph {
        let targets = targets();
        let site_count = targets.iter().map(|target| target.index()).max().unwrap() + 1;
        CircuitDraft {
            nodes: vec![CircuitNodeDraft {
                position: Some(BlockPos::new(0, 0, 0)),
                raw_state: Some(
                    Block::RedstoneWire {
                        wire: RedstoneWire::default(),
                    }
                    .get_id(),
                ),
                kind: CircuitNodeKind::Wire(0),
                state: ComponentState::Wire(RedstoneWire::default()),
                output_route: None,
            }],
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: vec![Profile {
                raw_state: Block::Air.get_id(),
                registry_type: Block::Air.registry_id(),
                solid: false,
                transparent: true,
                cube: false,
                electrical: ProfileElectrical::None,
            }],
            guards: vec![Guard::Always],
            contacts: vec![],
            routes: vec![route(deliveries)],
            turbo_callback_indices: vec![index],
            wires: vec![WirePlan {
                node: NodeId::new(0),
                canonical_neighbors: targets,
                shape_route: None,
                turbo_route: Some(RouteId::new(0)),
            }],
            dust_paths: vec![],
            wire_relations: vec![],
            sites: (0..site_count)
                .map(|index| SiteSpec {
                    position: BlockPos::new(index as i32 + 2, 0, 0),
                    source: SiteSource::Constant(crate::redpiler::program_graph::ProfileId::new(0)),
                    source_strength: 0,
                })
                .collect(),
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
        }
    }

    #[test]
    fn builds_all_ordered_ranges_with_shared_and_gapped_sequences() {
        let targets = targets();
        let deliveries = targets
            .iter()
            .enumerate()
            .flat_map(|(slot, target)| [delivery(*target, slot as u32 * 4), delivery(*target, slot as u32 * 4)])
            .collect::<Vec<_>>();
        let index = build_turbo_callback_index(&targets, &route(deliveries.clone())).unwrap();
        assert_eq!(index.offsets[0], 0);
        assert_eq!(index.offsets[24], deliveries.len() as u32);
        for slot in 0..24 {
            let start = index.offsets[slot] as usize;
            let end = index.offsets[slot + 1] as usize;
            assert_eq!(end - start, 2);
            assert!(deliveries[start..end].iter().all(|row| row.target == targets[slot]));
            assert_eq!(deliveries[start].sequence, deliveries[start + 1].sequence);
        }
    }

    #[test]
    fn accepts_empty_target_ranges_without_using_sequences_as_offsets() {
        let targets = targets();
        let route = route(vec![delivery(targets[1], 900), delivery(targets[23], 900)]);
        let index = build_turbo_callback_index(&targets, &route).unwrap();
        assert_eq!(index.offsets[0], 0);
        assert_eq!(index.offsets[1], 0);
        assert_eq!(index.offsets[2], 1);
        assert_eq!(index.offsets[23], 1);
        assert_eq!(index.offsets[24], 2);
    }

    #[test]
    fn unsupported_layouts_fall_back_to_linear_lookup() {
        let targets = targets();
        let all = targets.map(|target| delivery(target, 0)).to_vec();
        assert!(build_turbo_callback_index(&targets, &route(all.clone())).is_some());

        let mut repeated = targets;
        repeated[4] = repeated[3];
        assert!(build_turbo_callback_index(&repeated, &route(all.clone())).is_none());
        assert!(build_turbo_callback_index(&targets, &route(vec![])).is_some());
        assert!(build_turbo_callback_index(&targets, &route(vec![all[1], all[0]])).is_none());
        assert!(build_turbo_callback_index(&targets, &route(vec![all[0], all[2]])).is_some());
        assert!(build_turbo_callback_index(&targets, &route(vec![all[0], all[2], all[1]])).is_none());
        let no_route = WirePlan {
            node: NodeId::new(0),
            canonical_neighbors: targets,
            shape_route: None,
            turbo_route: None,
        };
        assert!(index_wire(&no_route, &[]).is_none());
    }

    #[test]
    fn validator_rejects_corrupt_ranges_and_missing_routes() {
        let targets = targets();
        let rows = targets.map(|target| delivery(target, 0)).to_vec();
        let index = build_turbo_callback_index(&targets, &route(rows.clone())).unwrap();
        let mutations: [fn(&mut [u32; 25]); 4] = [
            |offsets: &mut [u32; 25]| offsets[0] = 1,
            |offsets: &mut [u32; 25]| offsets[2] = 0,
            |offsets: &mut [u32; 25]| offsets[1] = u32::MAX,
            |offsets: &mut [u32; 25]| offsets[24] -= 1,
        ];
        for mutate in mutations {
            let mut corrupt = index;
            mutate(&mut corrupt.offsets);
            assert!(matches!(
                graph(Some(corrupt), rows.clone()).validate(),
                Err(crate::redpiler::program_graph::GraphBuildError::InvalidTurboCallbackIndex { .. })
            ));
        }

        let mut wrong = rows.clone();
        wrong[0].target = targets[1];
        assert!(matches!(
            graph(Some(index), wrong).validate(),
            Err(crate::redpiler::program_graph::GraphBuildError::InvalidTurboCallbackIndex { .. })
        ));

        let mut no_route = graph(Some(index), rows.clone());
        no_route.wires[0].turbo_route = None;
        assert!(matches!(
            no_route.validate(),
            Err(crate::redpiler::program_graph::GraphBuildError::InvalidTurboCallbackIndex { .. })
        ));
        let mut missing_route = graph(Some(index), rows.clone());
        missing_route.wires[0].turbo_route = Some(RouteId::new(4));
        assert!(missing_route.validate().is_err());

        let mut wrong_length = graph(Some(index), rows.clone());
        wrong_length.turbo_callback_indices = vec![None, None];
        assert!(matches!(
            wrong_length.validate(),
            Err(crate::redpiler::program_graph::GraphBuildError::StateTableLength { .. })
        ));

        let mut disabled = graph(None, rows);
        disabled.turbo_callback_indices.clear();
        disabled.validate().unwrap();
    }

    #[test]
    fn builder_is_idempotent_cancellation_is_atomic_and_disabled_entry_is_untouched() {
        use crate::redpiler::passes::{PassContext, PassEntry, PassPipeline, run_graph_pipeline};
        use crate::redpiler::program_graph::EntryState;
        use crate::redpiler::{CompilerOptions, TaskMonitor};

        let targets = targets();
        let rows = targets.map(|target| delivery(target, 0)).to_vec();
        let mut graph = graph(None, rows.clone());
        let monitor = TaskMonitor::default();
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        let context = PassContext {
            monitor: &monitor,
            options: &options,
            entry: &entry,
        };
        IndexTurboCallbacks.run(&mut graph, &context).unwrap();
        let expected = graph.turbo_callback_indices.clone();
        assert!(expected[0].is_some());
        IndexTurboCallbacks.run(&mut graph, &context).unwrap();
        assert_eq!(graph.turbo_callback_indices, expected);

        let mut cancelled = graph.clone();
        let cancelled_monitor = TaskMonitor::default();
        cancelled_monitor.cancel();
        let cancelled_context = PassContext {
            monitor: &cancelled_monitor,
            options: &options,
            entry: &entry,
        };
        assert!(IndexTurboCallbacks.run(&mut cancelled, &cancelled_context).is_err());
        assert_eq!(cancelled.turbo_callback_indices, expected);

        let mut disabled = graph.clone();
        let before = disabled.turbo_callback_indices.clone();
        run_graph_pipeline(
            &mut disabled,
            &options,
            &entry,
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(IndexTurboCallbacks),
                enabled: false,
            }]),
            &monitor,
        )
        .unwrap();
        assert_eq!(disabled.turbo_callback_indices, before);
    }

    #[test]
    fn later_route_mutation_invalidates_the_index() {
        use crate::redpiler::passes::{PassEntry, PassPipeline, run_graph_pipeline};
        use crate::redpiler::program_graph::EntryState;
        use crate::redpiler::{CompilerOptions, TaskMonitor};

        struct ClearRoute;
        impl crate::redpiler::passes::GraphPass for ClearRoute {
            fn name(&self) -> &'static str {
                "clear-route"
            }

            fn validate_input(&self, _: &CompileGraph) -> Result<(), String> {
                Ok(())
            }

            fn run(&self, graph: &mut CompileGraph, _: &PassContext<'_>) -> Result<(), String> {
                graph.routes[0].deliveries = Box::new([]);
                Ok(())
            }
        }

        let targets = targets();
        let rows = targets.map(|target| delivery(target, 0)).to_vec();
        let mut graph = graph(None, rows);
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        run_graph_pipeline(
            &mut graph,
            &options,
            &entry,
            &PassPipeline::new(vec![
                PassEntry {
                    pass: Box::new(IndexTurboCallbacks),
                    enabled: true,
                },
                PassEntry {
                    pass: Box::new(ClearRoute),
                    enabled: true,
                },
            ]),
            &TaskMonitor::default(),
        )
        .unwrap();
        assert!(graph.turbo_callback_indices.is_empty());
    }
}
