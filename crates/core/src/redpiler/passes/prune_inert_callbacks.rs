//! Removes stored callback rows that cannot resolve to a receiver.
//!
//! Ordinary delivery evaluates only pure guards/capture checks before resolving
//! a receiver. A missing receiver is inert except for Component Updates, whose
//! dispatcher supplies a fallback handler. Turbo lookup requires an explicit
//! receiver, so removing these rows cannot change its first selectable candidate.
//! Explicit receivers, including no-op handlers, always remain. Eligibility does
//! not depend on power, guards, contact selection, saved state, or pending work.
//!
//! Stable filtering preserves surviving rows verbatim, including duplicates and
//! sequence gaps; empty routes, links, captures, wire geometry, state, and IDs
//! remain intact. Dust certificates remain valid; the pipeline invalidates Turbo
//! offsets and must rebuild them after pruning. The default order is after
//! opt-01 and before indexing. Rule and composition tests below cover the full
//! site/kind/view boundary, malformed inputs, cancellation, and idempotence.
//! Discovery is linear in delivery rows, with temporary storage bounded by one
//! changed route. Runtime saves row visits and pure checks, never handler calls.

use super::{GraphPass, PassContext};
use crate::redpiler::program_graph::{CompileGraph, DeliveryKind, SiteSource};

/// Drops only deliveries without an explicit or ordinary fallback receiver.
pub(crate) struct PruneInertCallbacks;

impl GraphPass for PruneInertCallbacks {
    fn name(&self) -> &'static str {
        "prune-inert-callbacks"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let sites = &graph.sites;
        for route in &mut graph.routes {
            if context.monitor.cancelled() {
                return Err("cancelled while pruning inert callbacks".into());
            }
            let keep = |delivery: &crate::redpiler::program_graph::RouteDelivery| {
                delivery.receiver.is_some()
                    || matches!(
                        (delivery.kind, sites[delivery.target.index()].source),
                        (DeliveryKind::Update { .. }, SiteSource::Component(_))
                    )
            };
            if route.deliveries.iter().all(keep) {
                continue;
            }
            let mut deliveries = std::mem::take(&mut route.deliveries).into_vec();
            deliveries.retain(keep);
            route.deliveries = deliveries.into_boxed_slice();
        }
        Ok(())
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }

    fn preserves_wire_query_facts(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::passes::{PassEntry, PassPipeline, PassPipelineError, default_graph_pipeline, run_graph_pipeline};
    use crate::redpiler::program_graph::{
        CallbackEdgeRef, CallbackReceiver, CircuitNodeDraft, CircuitNodeKind, ComponentState, ContactId, ContactSpec, ContactVariant, EntryState,
        Guard, GuardId, LinkSpec, NodeId, OperandView, Profile, ProfileElectrical, ProfileId, Route, RouteDelivery, RouteId, SiteId, SiteSpec,
        SiteTest, WirePlan,
    };
    use crate::redpiler::{CompilerOptions, TaskMonitor};
    use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneWire};
    use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};

    fn graph() -> CompileGraph {
        let block = Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        };
        let component = NodeId::new(0);
        let contact = ContactId::new(0);
        let profile = ProfileId::new(0);
        CompileGraph {
            nodes: vec![
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 0)),
                    raw_state: Some(block.get_id()),
                    kind: CircuitNodeKind::Electrical(NodeType::Lever),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: Some(RouteId::new(0)),
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(1, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Contact(contact),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
            ],
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
            guards: vec![
                Guard::Always,
                Guard::Not(GuardId::new(0)),
                Guard::Site {
                    site: SiteId::new(1),
                    view: OperandView::Captured,
                    test: SiteTest::Profile(profile),
                },
            ],
            contacts: vec![ContactSpec {
                node: NodeId::new(1),
                position: BlockPos::new(1, 0, 0),
                variants: vec![ContactVariant {
                    when: GuardId::new(0),
                    profile,
                    receiver: Some(CallbackReceiver::Contact(contact)),
                }]
                .into_boxed_slice(),
            }],
            routes: vec![Route {
                source: component,
                deliveries: Box::new([]),
            }],
            turbo_callback_indices: vec![],
            wires: vec![],
            dust_paths: vec![],
            wire_relations: vec![],
            sites: [
                SiteSource::Component(component),
                SiteSource::Contact(contact),
                SiteSource::Constant(profile),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, source)| SiteSpec {
                position: BlockPos::new(index as i32, 0, 0),
                source,
                source_strength: 0,
            })
            .collect(),
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
        }
    }

    fn delivery(target: u32, kind: DeliveryKind, view: OperandView, sequence: u32) -> RouteDelivery {
        RouteDelivery {
            target: SiteId::new(target),
            kind,
            guard: GuardId::new(0),
            view,
            sequence,
            receiver: None,
        }
    }

    fn set_deliveries(graph: &mut CompileGraph, deliveries: Vec<RouteDelivery>) {
        graph.links = deliveries
            .iter()
            .filter_map(|delivery| {
                let to = match delivery.receiver? {
                    CallbackReceiver::Component(node) => node,
                    CallbackReceiver::Contact(contact) => graph.contacts[contact.index()].node,
                    _ => unreachable!("fixture has no pistons"),
                };
                Some(LinkSpec {
                    from: NodeId::new(0),
                    to,
                    kind: delivery.link_kind(),
                    callback: CallbackEdgeRef {
                        route: RouteId::new(0),
                        sequence: delivery.sequence,
                    },
                })
            })
            .collect();
        graph.routes[0].deliveries = deliveries.into_boxed_slice();
        graph.validate().unwrap();
    }

    fn run(graph: &mut CompileGraph, enabled: bool, monitor: &TaskMonitor) -> Result<(), PassPipelineError> {
        run_graph_pipeline(
            graph,
            &CompilerOptions::default(),
            &EntryState {
                logical_tick: 27,
                pending_ticks: vec![],
            },
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(PruneInertCallbacks),
                enabled,
            }]),
            monitor,
        )
    }

    #[test]
    fn prunes_only_receiverless_non_component_updates_across_sites_kinds_and_views() {
        let mut graph = graph();
        let mut rows = Vec::new();
        let mut expected = Vec::new();
        for target in 0..3 {
            for kind in [
                DeliveryKind::Update { direction: None },
                DeliveryKind::Update {
                    direction: Some(BlockFace::North),
                },
                DeliveryKind::Shape { direction: BlockFace::West },
            ] {
                for view in [OperandView::Live, OperandView::Captured] {
                    let row = delivery(target, kind, view, rows.len() as u32 * 3);
                    if target == 0 && matches!(kind, DeliveryKind::Update { .. }) {
                        expected.push(row);
                    }
                    rows.push(row);
                    if target == 2 {
                        continue;
                    }
                    let mut explicit = row;
                    explicit.receiver = Some(if target == 0 {
                        CallbackReceiver::Component(NodeId::new(0))
                    } else {
                        CallbackReceiver::Contact(ContactId::new(0))
                    });
                    if target == 1 && view == OperandView::Captured {
                        explicit.guard = GuardId::new(2);
                    }
                    rows.push(explicit);
                    rows.push(explicit);
                    expected.push(explicit);
                    expected.push(explicit);
                }
            }
        }
        set_deliveries(&mut graph, rows);
        let mut expected_graph = graph.clone();
        expected_graph.routes[0].deliveries = expected.into_boxed_slice();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, expected_graph, "only eligible rows may change; links and duplicates must remain");
    }

    #[test]
    fn empty_routes_keep_their_ids_owners_and_references() {
        let mut graph = graph();
        set_deliveries(
            &mut graph,
            vec![delivery(2, DeliveryKind::Update { direction: None }, OperandView::Captured, 19)],
        );
        graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: Box::new([]),
        });
        let mut expected = graph.clone();
        expected.routes[0].deliveries = Box::new([]);
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, expected);
        assert_eq!(graph.nodes[0].output_route, Some(RouteId::new(0)));
    }

    #[test]
    fn guarded_alternatives_and_meaningful_duplicates_keep_their_original_order() {
        let mut graph = graph();
        let mut failed = delivery(0, DeliveryKind::Update { direction: None }, OperandView::Captured, 31);
        failed.receiver = Some(CallbackReceiver::Component(NodeId::new(0)));
        failed.guard = GuardId::new(1);
        let passing = RouteDelivery {
            guard: GuardId::new(0),
            ..failed
        };
        let inert = delivery(2, DeliveryKind::Update { direction: None }, OperandView::Captured, 31);
        set_deliveries(&mut graph, vec![inert, failed, inert, passing, passing, inert]);
        let links = graph.links.clone();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(&*graph.routes[0].deliveries, &[failed, passing, passing]);
        assert_eq!(graph.links, links);
    }

    #[test]
    fn pruning_is_idempotent_disabled_is_untouched_and_cancellation_stops_before_mutation() {
        let mut graph = graph();
        set_deliveries(
            &mut graph,
            vec![
                delivery(2, DeliveryKind::Update { direction: None }, OperandView::Live, 4),
                delivery(0, DeliveryKind::Update { direction: None }, OperandView::Live, 17),
            ],
        );
        let before = graph.clone();
        run(&mut graph, false, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, before);
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(matches!(run(&mut graph, true, &monitor), Err(PassPipelineError::Cancelled)));
        assert_eq!(graph, before);
        let options = CompilerOptions {
            optimization_level: Some(1),
            ..Default::default()
        };
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        assert!(
            PruneInertCallbacks
                .run(
                    &mut graph,
                    &PassContext {
                        monitor: &monitor,
                        options: &options,
                        entry: &entry
                    }
                )
                .is_err()
        );
        assert_eq!(graph, before);
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        let once = graph.clone();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, once);
    }

    #[test]
    fn invalid_sites_and_guards_are_rejected_before_pruning() {
        for corrupt_site in [false, true] {
            let mut graph = graph();
            let mut row = delivery(2, DeliveryKind::Shape { direction: BlockFace::Top }, OperandView::Live, 0);
            if corrupt_site {
                row.target = SiteId::new(99);
            } else {
                row.guard = GuardId::new(99);
            }
            graph.routes[0].deliveries = Box::new([row]);
            let before = graph.clone();
            assert!(matches!(
                run(&mut graph, true, &TaskMonitor::default()),
                Err(PassPipelineError::InvalidInput(_))
            ));
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn custom_order_invalidates_existing_turbo_offsets_and_can_rebuild_them() {
        let mut graph = graph();
        graph.nodes[0].kind = CircuitNodeKind::Wire(0);
        graph.nodes[0].raw_state = Some(
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            }
            .get_id(),
        );
        graph.nodes[0].state = ComponentState::Wire(RedstoneWire::default());
        let targets = std::array::from_fn(|slot| SiteId::new(slot as u32 + 3));
        for target in targets {
            graph.sites.push(SiteSpec {
                position: BlockPos::new(target.index() as i32, 1, 0),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 0,
            });
        }
        graph.wires.push(WirePlan {
            node: NodeId::new(0),
            canonical_neighbors: targets,
            shape_route: None,
            turbo_route: Some(RouteId::new(0)),
            query_facts: None,
            instant_reset: None,
        });
        set_deliveries(
            &mut graph,
            targets
                .into_iter()
                .map(|target| delivery(target.index() as u32, DeliveryKind::Update { direction: None }, OperandView::Captured, 6))
                .collect(),
        );
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        let index_pipeline = || {
            let indexing_options = CompilerOptions {
                optimization_level: Some(1),
                ..Default::default()
            };
            let mut pipeline = default_graph_pipeline(&indexing_options);
            pipeline.entries.retain(|entry| entry.pass.name() == "index-turbo-callbacks");
            assert_eq!(pipeline.entries.len(), 1);
            pipeline
        };
        run_graph_pipeline(&mut graph, &options, &entry, &index_pipeline(), &TaskMonitor::default()).unwrap();
        assert!(graph.turbo_callback_indices[0].is_some());
        run(&mut graph, false, &TaskMonitor::default()).unwrap();
        assert!(graph.turbo_callback_indices[0].is_some());
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert!(graph.turbo_callback_indices.is_empty());
        assert!(graph.routes[0].deliveries.is_empty());
        run_graph_pipeline(&mut graph, &options, &entry, &index_pipeline(), &TaskMonitor::default()).unwrap();
        assert_eq!(graph.turbo_callback_indices[0].unwrap().offsets, [0; 25]);
        assert!(PruneInertCallbacks.preserves_dust_paths());
        assert!(!PruneInertCallbacks.preserves_turbo_callback_indices());
    }

    #[test]
    fn composition_preserves_existing_dust_certificates_and_lowering() {
        use crate::plot::PlotWorld;
        use crate::redpiler::analysis::AnalysisLimits;
        use crate::world::World;
        use crate::world::storage::Chunk;

        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        for x in 3..=9 {
            world.set_block(BlockPos::new(x, 19, 5), Block::Stone {});
        }
        world.set_block(
            BlockPos::new(5, 20, 5),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        for distance in 0..3 {
            world.set_block(
                BlockPos::new(6 + distance, 20, 5),
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15 - distance as u8,
                        ..Default::default()
                    },
                },
            );
        }
        for x in 6..=8 {
            let position = BlockPos::new(x, 20, 5);
            let Block::RedstoneWire { wire } = world.get_block(position) else {
                unreachable!()
            };
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: crate::redstone::wire::get_regulated_sides(wire, &world, position),
                },
            );
        }
        let monitor = TaskMonitor::default();
        let mut prepared = crate::redpiler::passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &monitor,
            AnalysisLimits::default(),
        )
        .unwrap();
        let options = CompilerOptions {
            optimization_level: Some(1),
            ..Default::default()
        };
        let mut pipeline = default_graph_pipeline(&options);
        pipeline.entries.retain(|entry| entry.pass.name() != PruneInertCallbacks.name());
        run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        assert_eq!(prepared.graph.dust_paths.len(), 1);
        let mut expected = prepared.graph.clone();
        run(&mut prepared.graph, true, &monitor).unwrap();
        assert_eq!(prepared.graph.dust_paths, expected.dust_paths);
        assert_eq!(prepared.graph.sites, expected.sites);
        assert_eq!(prepared.graph.wires, expected.wires);
        assert_eq!(prepared.graph.power_links, expected.power_links);
        assert_eq!(prepared.graph.nodes, expected.nodes);
        for (old_route, new_route) in expected.routes.iter_mut().zip(&prepared.graph.routes) {
            old_route.deliveries = new_route.deliveries.clone();
        }
        expected.turbo_callback_indices.clear();
        assert_eq!(prepared.graph, expected);
        prepared.lower(&monitor).unwrap();
    }
}
