//! Removes ordinary notifications whose dispatch handler is an identity.
//!
//! A live Update with an explicit non-facing direction cannot schedule an
//! observer tick; a Shape dispatched to a non-wire component returns immediately,
//! including captured operands and contact-backed components. An explicit Contact
//! receiver also returns immediately, for either delivery kind and operand view.
//! These are identities for every component state and pending queue, so guards,
//! power inputs, imported ticks, and future execution need no additional proof.
//! The observer Update rule requires a live immutable component site. Every Turbo
//! route remains general: its stored Contact or non-wire Shape row can admit a
//! captured piston or component receiver, dispatched as Update(None). Ordinary
//! guards and capture checks are pure; skipped checks neither capture a site nor
//! change queues, contact selection, error handling or later callback order.
//! Surviving occurrences retain order, duplicates, IDs, guards and sequence gaps;
//! corresponding explicit links are removed with exact multiset multiplicity.
//! Dust certificates and Turbo indexes remain valid: no power/site binding or
//! Turbo row changes, and only dispatch identities disappear from other routes.
//! The rule relies on validated component bindings and the current Graph dispatch
//! in update_component/update_observer. Tests exhaust all six observer facings and
//! directions, None, captured/Turbo exclusions, duplicate links, and cancellation.
//! Discovery and rewriting are linear in routes, deliveries and links, with
//! temporary storage proportional to removals plus route count. The result is
//! idempotent and removes dispatch work without adding runtime representation.

use super::{GraphPass, PassContext};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, CompileGraph, DeliveryKind, LinkKind, NodeId, OperandView, RouteDelivery, RouteId, SiteSource,
};
use rustc_hash::FxHashMap;

/// Prunes ordinary callbacks whose dispatch handler is an identity.
pub(crate) struct PruneUnhandledUpdates;

type LinkKey = (CallbackEdgeRef, NodeId, NodeId, LinkKind);

struct RemovalPlan {
    /// Original occurrence ordinals to delete, indexed by unchanged route ID.
    route_ordinals: Vec<Vec<usize>>,
    /// Required explicit link deletions for each exact callback owner and kind.
    link_counts: FxHashMap<LinkKey, usize>,
}

impl GraphPass for PruneUnhandledUpdates {
    // todo I do not like this pattern, can we implement it on clean way?
    fn preserves_dust_paths(&self) -> bool {
        true
    }

    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }

    fn preserves_wire_query_facts(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "prune-unhandled-updates"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let turbo_routes = find_turbo_routes(graph, context.monitor)?;
        let removals = plan_removals(graph, &turbo_routes, context.monitor)?;
        if context.monitor.cancelled() {
            return Err("cancelled before pruning unhandled updates".into());
        }
        apply_removals(graph, removals);
        Ok(())
    }
}

fn find_turbo_routes(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<bool>, String> {
    let mut turbo_routes = vec![false; graph.routes.len()];
    for wire in &graph.wires {
        if monitor.cancelled() {
            return Err("cancelled while finding Turbo routes".into());
        }
        if let Some(route) = wire.turbo_route {
            turbo_routes[route.index()] = true;
        }
    }
    Ok(turbo_routes)
}

fn plan_removals(graph: &CompileGraph, turbo_routes: &[bool], monitor: &TaskMonitor) -> Result<RemovalPlan, String> {
    let mut route_ordinals = Vec::with_capacity(graph.routes.len());
    let mut link_counts = FxHashMap::<LinkKey, usize>::default();
    for (index, route) in graph.routes.iter().enumerate() {
        let mut ordinals = Vec::new();
        for (ordinal, delivery) in route.deliveries.iter().copied().enumerate() {
            if monitor.cancelled() {
                return Err("cancelled while classifying unhandled updates".into());
            }
            if turbo_routes[index] || !unhandled_delivery(graph, delivery) {
                continue;
            }
            ordinals.push(ordinal);
            let receiver_node = match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => Some(node),
                Some(CallbackReceiver::Contact(contact)) => Some(graph.contacts[contact.index()].node),
                _ => None,
            };
            if let Some(node) = receiver_node {
                let route_id = RouteId::try_from_index(index).ok_or("route table exceeds typed ID range")?;
                let key = (
                    CallbackEdgeRef {
                        route: route_id,
                        sequence: delivery.sequence,
                    },
                    route.source,
                    node,
                    delivery.link_kind(),
                );
                *link_counts.entry(key).or_default() += 1;
            }
        }
        route_ordinals.push(ordinals);
    }
    Ok(RemovalPlan { route_ordinals, link_counts })
}

fn apply_removals(graph: &mut CompileGraph, mut removals: RemovalPlan) {
    graph.links.retain(|link| {
        if let Some(count) = removals
            .link_counts
            .get_mut(&(link.callback, link.from, link.to, link.kind))
            .filter(|count| **count > 0)
        {
            *count -= 1;
            false
        } else {
            true
        }
    });
    for (route, ordinals) in graph.routes.iter_mut().zip(removals.route_ordinals) {
        if ordinals.is_empty() {
            continue;
        }
        let mut ordinals = ordinals.into_iter().peekable();
        let old = std::mem::take(&mut route.deliveries);
        route.deliveries = old
            .into_vec()
            .into_iter()
            .enumerate()
            .filter_map(|(ordinal, delivery)| {
                if ordinals.peek() == Some(&ordinal) {
                    ordinals.next();
                    None
                } else {
                    Some(delivery)
                }
            })
            .collect();
    }
}

fn unhandled_delivery(graph: &CompileGraph, delivery: RouteDelivery) -> bool {
    if matches!(delivery.receiver, Some(CallbackReceiver::Contact(_))) {
        return true;
    }
    if matches!(delivery.kind, DeliveryKind::Shape { .. }) {
        let node = match delivery.receiver {
            Some(CallbackReceiver::Component(node)) => node,
            None => match graph.sites[delivery.target.index()].source {
                SiteSource::Component(node) => node,
                _ => return false,
            },
            _ => return false,
        };
        return matches!(
            graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }
        );
    }
    if delivery.view != OperandView::Live {
        return false;
    }
    let SiteSource::Component(node) = graph.sites[delivery.target.index()].source else {
        return false;
    };
    match (delivery.kind, &graph.nodes[node.index()].kind) {
        (
            DeliveryKind::Update { direction: Some(direction) },
            CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. },
        ) => direction != *facing,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::passes::{PassEntry, PassPipeline, run_graph_pipeline};
    use crate::redpiler::program_graph::{
        CircuitDraft, CircuitNodeDraft, ComponentState, ContactId, ContactSpec, ContactVariant, EntryState, Guard, GuardId, LinkSpec,
        PistonAvailability, PistonId, PistonLane, PistonRoutes, PistonSpec, PistonState, Profile, ProfileElectrical, ProfileId, Route, SiteId,
        SiteSpec, SiteTest, WirePlan,
    };
    use crate::redpiler::{CompilerOptions, TaskMonitor};
    use mchprs_blocks::blocks::{Block, RedstoneWire};
    use mchprs_blocks::{BlockFace, BlockPos};

    const FACES: [BlockFace; 6] = [
        BlockFace::Bottom,
        BlockFace::Top,
        BlockFace::North,
        BlockFace::South,
        BlockFace::West,
        BlockFace::East,
    ];

    fn delivery(kind: DeliveryKind, sequence: u32) -> RouteDelivery {
        RouteDelivery {
            target: SiteId::new(0),
            kind,
            guard: GuardId::new(0),
            view: OperandView::Live,
            sequence,
            receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
        }
    }

    fn graph(kind: CircuitNodeKind, deliveries: Vec<RouteDelivery>) -> CompileGraph {
        let block = Block::Stone {};
        let mut graph = CircuitDraft {
            nodes: vec![CircuitNodeDraft {
                position: Some(BlockPos::new(0, 0, 0)),
                raw_state: Some(block.get_id()),
                kind,
                state: ComponentState::Electrical(NodeState::simple(false)),
                output_route: Some(RouteId::new(0)),
            }],
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: vec![Profile {
                raw_state: block.get_id(),
                registry_type: block.registry_id(),
                solid: block.is_solid(),
                transparent: block.is_transparent(),
                cube: block.is_cube(),
                electrical: ProfileElectrical::None,
            }],
            guards: vec![Guard::Always],
            contacts: vec![],
            routes: vec![Route {
                source: NodeId::new(0),
                deliveries: deliveries.into_boxed_slice(),
            }],
            turbo_callback_indices: vec![],
            wires: vec![],
            dust_paths: vec![],
            wire_relations: vec![],
            sites: vec![SiteSpec {
                position: BlockPos::new(0, 0, 0),
                source: SiteSource::Component(NodeId::new(0)),
                source_strength: 0,
            }],
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
            instant_regions: Vec::new(),
            instant_proxies: Vec::new(),
            instant_machine: None,
        };
        rebuild_links(&mut graph);
        graph
    }

    fn rebuild_links(graph: &mut CompileGraph) {
        let contacts = &graph.contacts;
        graph.links = graph
            .routes
            .iter()
            .enumerate()
            .flat_map(|(index, route)| {
                route.deliveries.iter().filter_map(move |delivery| {
                    let node = match delivery.receiver? {
                        CallbackReceiver::Component(node) => node,
                        CallbackReceiver::Contact(contact) => contacts[contact.index()].node,
                        _ => return None,
                    };
                    Some(LinkSpec {
                        from: route.source,
                        to: node,
                        kind: delivery.link_kind(),
                        callback: CallbackEdgeRef {
                            route: RouteId::try_from_index(index).unwrap(),
                            sequence: delivery.sequence,
                        },
                    })
                })
            })
            .collect();
    }

    fn observer() -> CircuitNodeKind {
        CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::North,
        }
    }

    fn run(graph: &mut CompileGraph, monitor: &TaskMonitor) -> Result<(), String> {
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 27,
            pending_ticks: vec![],
        };
        run_graph_pipeline(
            graph,
            &options,
            &entry,
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(PruneUnhandledUpdates),
                enabled: true,
            }]),
            monitor,
        )
        .map_err(|error| error.to_string())
    }

    #[test]
    fn exhaustive_observer_directions_keep_facing_none_and_captured_updates() {
        for facing in FACES {
            for powered in [false, true] {
                let deliveries = FACES
                    .into_iter()
                    .map(Some)
                    .chain([None])
                    .enumerate()
                    .flat_map(|(index, direction)| {
                        let live = delivery(DeliveryKind::Update { direction }, index as u32 * 3);
                        [
                            live,
                            RouteDelivery {
                                view: OperandView::Captured,
                                ..live
                            },
                        ]
                    })
                    .collect::<Vec<_>>();
                let expected = deliveries
                    .iter()
                    .copied()
                    .filter(|row| {
                        row.view == OperandView::Captured
                            || matches!(row.kind, DeliveryKind::Update { direction: None })
                            || row.kind == DeliveryKind::Update { direction: Some(facing) }
                    })
                    .collect::<Vec<_>>();
                let mut graph = graph(
                    CircuitNodeKind::Observer {
                        watched: SiteId::new(0),
                        facing,
                    },
                    deliveries,
                );
                graph.nodes[0].state = ComponentState::Electrical(NodeState::simple(powered));
                run(&mut graph, &TaskMonitor::default()).unwrap();
                assert_eq!(graph.routes[0].deliveries.as_ref(), expected);
                assert_eq!(graph.links.len(), expected.len());
                graph.validate().unwrap();
            }
        }
    }

    #[test]
    fn shapes_to_fixed_nonwire_components_are_inert_but_updates_remain() {
        for kind in [
            observer(),
            CircuitNodeKind::Electrical(NodeType::Torch),
            CircuitNodeKind::Electrical(NodeType::Lamp),
        ] {
            let mut deliveries = vec![delivery(DeliveryKind::Update { direction: None }, 0)];
            for (index, direction) in FACES.into_iter().enumerate() {
                let live = delivery(DeliveryKind::Shape { direction }, index as u32 + 1);
                deliveries.extend([
                    live,
                    RouteDelivery {
                        view: OperandView::Captured,
                        ..live
                    },
                ]);
            }
            let expected = deliveries.iter().copied().filter(|row| row.sequence == 0).collect::<Vec<_>>();
            let mut graph = graph(kind, deliveries);
            run(&mut graph, &TaskMonitor::default()).unwrap();
            assert_eq!(graph.routes[0].deliveries.as_ref(), expected);
            graph.validate().unwrap();
        }
    }

    #[test]
    fn implicit_receivers_need_no_link_and_surviving_duplicates_keep_ids_and_sequences() {
        let dead = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::South),
            },
            4,
        );
        let kept = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::North),
            },
            4,
        );
        let implicit_dead = RouteDelivery {
            receiver: None,
            sequence: 11,
            ..dead
        };
        let implicit_shape = RouteDelivery {
            receiver: None,
            sequence: 11,
            kind: DeliveryKind::Shape { direction: BlockFace::Top },
            ..dead
        };
        let mut graph = graph(observer(), vec![dead, kept, dead, kept, implicit_dead, implicit_shape]);
        graph.guards.push(Guard::All(vec![GuardId::new(0)].into_boxed_slice()));
        graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: vec![RouteDelivery {
                guard: GuardId::new(1),
                ..kept
            }]
            .into_boxed_slice(),
        });
        rebuild_links(&mut graph);
        let original_nodes = graph.nodes.clone();
        let original_sites = graph.sites.clone();
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.routes[0].deliveries.as_ref(), [kept, kept]);
        assert_eq!(graph.routes[1].deliveries[0].guard, GuardId::new(1));
        assert_eq!(graph.links.len(), 3);
        assert_eq!(graph.links.iter().filter(|link| link.callback.route == RouteId::new(0)).count(), 2);
        assert_eq!(graph.nodes, original_nodes);
        assert_eq!(graph.sites, original_sites);
        graph.validate().unwrap();
        let once = graph.clone();
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, once);
    }

    #[test]
    fn component_updates_at_contacts_and_turbo_routes_remain_general() {
        let dead = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::South),
            },
            0,
        );
        let mut contact = graph(observer(), vec![dead]);
        contact.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(1, 0, 0)),
            raw_state: None,
            kind: CircuitNodeKind::Contact(ContactId::new(0)),
            state: ComponentState::Specialized,
            output_route: None,
        });
        contact.contacts.push(ContactSpec {
            node: NodeId::new(1),
            position: BlockPos::new(1, 0, 0),
            variants: vec![ContactVariant {
                when: GuardId::new(0),
                profile: ProfileId::new(0),
                receiver: dead.receiver,
            }]
            .into_boxed_slice(),
        });
        contact.sites[0].source = SiteSource::Contact(ContactId::new(0));
        let before = contact.clone();
        run(&mut contact, &TaskMonitor::default()).unwrap();
        assert_eq!(contact, before);

        let mut turbo = graph(observer(), vec![dead, delivery(DeliveryKind::Shape { direction: BlockFace::Top }, 1)]);
        add_wire(&mut turbo, Some(RouteId::new(0)));
        // The shared route must belong to the wire, even when its deliveries are live.
        turbo.routes[0].source = NodeId::new(1);
        turbo.nodes[0].output_route = None;
        turbo.nodes[1].output_route = Some(RouteId::new(0));
        rebuild_links(&mut turbo);
        let before = turbo.clone();
        run(&mut turbo, &TaskMonitor::default()).unwrap();
        assert_eq!(turbo, before);
    }

    fn add_contact(graph: &mut CompileGraph, receiver: Option<CallbackReceiver>) -> SiteId {
        let contact = ContactId::try_from_index(graph.contacts.len()).unwrap();
        let node = NodeId::try_from_index(graph.nodes.len()).unwrap();
        let position = BlockPos::new(contact.index() as i32 + 3, 0, 0);
        graph.nodes.push(CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind: CircuitNodeKind::Contact(contact),
            state: ComponentState::Specialized,
            output_route: None,
        });
        graph.contacts.push(ContactSpec {
            node,
            position,
            variants: vec![ContactVariant {
                when: GuardId::new(0),
                profile: ProfileId::new(0),
                receiver,
            }]
            .into_boxed_slice(),
        });
        let site = SiteId::try_from_index(graph.sites.len()).unwrap();
        graph.sites.push(SiteSpec {
            position,
            source: SiteSource::Contact(contact),
            source_strength: 0,
        });
        site
    }

    #[test]
    fn ordinary_contact_noops_and_nonwire_shapes_preserve_meaningful_alternatives() {
        let mut graph = graph(observer(), vec![]);
        let target = add_contact(&mut graph, Some(CallbackReceiver::Contact(ContactId::new(0))));
        graph.contacts[0].variants = vec![
            graph.contacts[0].variants[0].clone(),
            ContactVariant {
                when: GuardId::new(0),
                profile: ProfileId::new(0),
                receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
            },
        ]
        .into_boxed_slice();
        graph.guards.extend([
            Guard::Not(GuardId::new(0)),
            Guard::Site {
                site: target,
                view: OperandView::Captured,
                test: SiteTest::Profile(ProfileId::new(0)),
            },
            Guard::All(vec![GuardId::new(1), GuardId::new(2)].into_boxed_slice()),
        ]);
        let mut rows = Vec::new();
        for kind in FACES.into_iter().map(|direction| DeliveryKind::Shape { direction }).chain([
            DeliveryKind::Update { direction: None },
            DeliveryKind::Update {
                direction: Some(BlockFace::North),
            },
        ]) {
            for (view, guards) in [(OperandView::Live, [0, 1]), (OperandView::Captured, [2, 3])] {
                for guard in guards {
                    let row = RouteDelivery {
                        target,
                        kind,
                        guard: GuardId::new(guard),
                        view,
                        sequence: rows.len() as u32 * 3,
                        receiver: Some(CallbackReceiver::Contact(ContactId::new(0))),
                    };
                    rows.extend([row, row]);
                    if matches!(kind, DeliveryKind::Shape { .. }) {
                        rows.push(RouteDelivery {
                            target: SiteId::new(0),
                            receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
                            ..row
                        });
                    }
                }
            }
        }
        let meaningful = RouteDelivery {
            target,
            kind: DeliveryKind::Update {
                direction: Some(BlockFace::North),
            },
            guard: GuardId::new(0),
            view: OperandView::Live,
            sequence: 900,
            receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
        };
        let captured = RouteDelivery {
            guard: GuardId::new(2),
            view: OperandView::Captured,
            ..meaningful
        };
        rows.extend([meaningful, meaningful, captured, captured]);
        graph.routes[0].deliveries = rows.into_boxed_slice();
        rebuild_links(&mut graph);
        let before = graph.clone();
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.routes[0].deliveries.as_ref(), [meaningful, meaningful, captured, captured]);
        assert_eq!(graph.links.len(), 4);
        assert!(graph.links.iter().all(|link| link.to == NodeId::new(0) && link.callback.sequence == 900));
        assert_eq!(graph.contacts, before.contacts);
        assert_eq!(graph.guards, before.guards);
        assert_eq!(graph.nodes, before.nodes);
        assert_eq!(graph.sites, before.sites);
        graph.validate().unwrap();
        let once = graph.clone();
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, once);
    }

    #[test]
    fn turbo_contact_shape_can_admit_a_captured_piston_base_receiver() {
        let mut graph = graph(observer(), vec![]);
        add_wire(&mut graph, Some(RouteId::new(0)));
        let piston_node = NodeId::new(2);
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(3, 0, 0)),
            raw_state: None,
            kind: CircuitNodeKind::Piston(PistonId::new(0)),
            state: ComponentState::Specialized,
            output_route: None,
        });
        let target = add_contact(&mut graph, Some(CallbackReceiver::PistonBase(PistonId::new(0))));
        add_contact(&mut graph, None);
        add_contact(&mut graph, None);
        let route = RouteId::new(1);
        graph.routes.push(Route {
            source: piston_node,
            deliveries: Box::new([]),
        });
        graph.pistons.push(PistonSpec {
            node: piston_node,
            position: BlockPos::new(3, 0, 0),
            facing: BlockFace::East,
            sticky: false,
            base: ContactId::new(0),
            near: ContactId::new(1),
            far: ContactId::new(2),
            lane: PistonLane::Empty,
            routes: PistonRoutes {
                base_changed: route,
                near_changed: route,
                far_changed: None,
                far_shape_changed: None,
                base_restored: route,
                head_restored: route,
                resource_restored_near: None,
                resource_restored_far: None,
            },
        });
        graph.initial_pistons.push(PistonState {
            extended: true,
            head_present: true,
            availability: PistonAvailability::Ready,
            revision: 0,
        });
        graph.guards.push(Guard::Site {
            site: target,
            view: OperandView::Captured,
            test: SiteTest::Profile(ProfileId::new(0)),
        });
        let row = RouteDelivery {
            target,
            kind: DeliveryKind::Shape { direction: BlockFace::Top },
            guard: GuardId::new(1),
            view: OperandView::Captured,
            sequence: 9,
            receiver: Some(CallbackReceiver::Contact(ContactId::new(0))),
        };
        graph.routes[0].source = NodeId::new(1);
        graph.routes[0].deliveries = vec![row, row].into_boxed_slice();
        graph.nodes[0].output_route = None;
        graph.wires[0].canonical_neighbors = [target; 24];
        rebuild_links(&mut graph);
        let before = graph.clone();
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, before);
        // Turbo admits the stored Contact row, then dispatches this captured receiver.
        assert_eq!(
            graph.contacts[0].variants[0].receiver,
            Some(CallbackReceiver::PistonBase(PistonId::new(0)))
        );
        assert_eq!(graph.links[0].to, graph.contacts[0].node);
    }

    fn add_wire(graph: &mut CompileGraph, turbo_route: Option<RouteId>) {
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(2, 0, 0)),
            raw_state: Some(
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                }
                .get_id(),
            ),
            kind: CircuitNodeKind::Wire(0),
            state: ComponentState::Wire(RedstoneWire::default()),
            output_route: None,
        });
        graph.wires.push(WirePlan {
            node: NodeId::new(1),
            canonical_neighbors: [SiteId::new(0); 24],
            shape_route: None,
            turbo_route,
            query_facts: None,
            instant_reset: None,
            hop_root: None,
        });
    }

    #[test]
    fn custom_order_preserves_existing_turbo_index_ranges() {
        use super::super::{PassEntry, default_graph_pipeline, run_graph_pipeline};

        let dead = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::South),
            },
            0,
        );
        let mut graph = graph(observer(), vec![dead, dead]);
        add_wire(&mut graph, Some(RouteId::new(0)));
        graph.routes[0].source = NodeId::new(1);
        graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: vec![dead].into_boxed_slice(),
        });
        graph.nodes[0].output_route = Some(RouteId::new(1));
        for index in 1..24 {
            graph.sites.push(SiteSpec {
                position: BlockPos::new(index, 1, 0),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 0,
            });
        }
        graph.wires[0].canonical_neighbors = std::array::from_fn(|index| SiteId::new(index as u32));
        rebuild_links(&mut graph);
        let options = CompilerOptions::default();
        let mut pipeline = default_graph_pipeline(&options);
        pipeline.entries.retain(|item| item.pass.name() == "index-turbo-callbacks");
        pipeline.entries[0].enabled = true;
        pipeline.entries.push(PassEntry {
            pass: Box::new(PruneUnhandledUpdates),
            enabled: true,
        });
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        run_graph_pipeline(&mut graph, &options, &entry, &pipeline, &TaskMonitor::default()).unwrap();
        let mut expected = [2; 25];
        expected[0] = 0;
        assert_eq!(graph.turbo_callback_indices[0].as_ref().unwrap().offsets, expected);
        assert_eq!(graph.routes[0].deliveries.as_ref(), [dead, dead]);
        assert!(graph.routes[1].deliveries.is_empty());
        assert!(PruneUnhandledUpdates.preserves_dust_paths());
        graph.validate().unwrap();
    }

    #[test]
    fn wire_shapes_and_other_component_updates_remain_general() {
        let shape = delivery(DeliveryKind::Shape { direction: BlockFace::Top }, 0);
        let update = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::South),
            },
            1,
        );
        let mut graph = graph(CircuitNodeKind::Electrical(NodeType::Torch), vec![shape, update]);
        add_wire(&mut graph, None);
        graph.sites.push(SiteSpec {
            position: BlockPos::new(2, 0, 0),
            source: SiteSource::Component(NodeId::new(1)),
            source_strength: 0,
        });
        let wire_shape = RouteDelivery {
            target: SiteId::new(1),
            receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
            sequence: 2,
            ..shape
        };
        graph.routes[0].deliveries = vec![shape, update, wire_shape, wire_shape].into_boxed_slice();
        rebuild_links(&mut graph);
        run(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.routes[0].deliveries.as_ref(), [update, wire_shape, wire_shape]);
        graph.validate().unwrap();
    }

    #[test]
    fn cancellation_and_malformed_link_multiplicity_leave_graph_untouched() {
        let dead = delivery(
            DeliveryKind::Update {
                direction: Some(BlockFace::South),
            },
            0,
        );
        let mut graph = graph(observer(), vec![dead, dead]);
        let before = graph.clone();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(run(&mut graph, &monitor).is_err());
        assert_eq!(graph, before);

        graph.links.pop();
        let before = graph.clone();
        assert!(run(&mut graph, &TaskMonitor::default()).is_err());
        assert_eq!(graph, before);
    }
}
