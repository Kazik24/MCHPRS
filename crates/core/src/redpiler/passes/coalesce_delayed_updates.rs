//! O5 coalescing of repeated ordinary delayed-receiver notifications.
//!
//! Within a contiguous run of unconditional live updates to fixed torches,
//! comparators, observers and proven always-unlocked repeaters, retain only the
//! first equivalent receiver sample. Only observers inspect callback direction.
//! These handlers only inspect power/state and insert a pending tick;
//! they cannot write power, dispatch callbacks, or consume work synchronously.
//! Consequently later identical samples either repeat a no-op or find the tick
//! already pending. Other receivers can only insert their own first tick, whose
//! deadline, priority and insertion position remain unchanged. Imported pending
//! work is unaffected. This relies on validated fixed component bindings and
//! ordinary Graph dispatch; derived plans must be rebuilt after this rewrite.
//! Repeaters require an initially false lock bit and no structural Side input;
//! hence their lock stays false and updates cannot enter the set_node lock branch.
//!
//! Captured operands, conditional guards, contacts, Turbo routes and every other
//! handler are barriers. In particular a wire walk, possible repeater lock change or
//! piston/BUD sample can change a later sample and must not be crossed. This is
//! deliberately NOT a proof that whole wire walks or piston updates coalesce.
//! O5 selects it before all plan builders. It uses existing deliveries/links,
//! so lowering, JSON export, projection and reset need no new representation.
//! Discovery is linear in nodes, deliveries and links, with temporary storage
//! linear in nodes, removable occurrences and the longest route. Tests cover
//! pending entry states, FIFO order, barriers, duplicate link multiplicity and
//! exact state/future execution in `graph/tests/coalesce_delayed_updates.rs`.

use super::{GraphPass, PassContext};
use crate::redpiler::PreparationTableStatistics;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, CompileGraph, ComponentState, DeliveryKind, Guard, InputPort, LinkKind, NodeId, OperandView,
    PowerTarget, RouteDelivery, RouteId, SiteSource,
};
use rustc_hash::{FxHashMap, FxHashSet};

/// Coalesce equivalent delayed samples at O5 before building execution plans.
pub(crate) struct CoalesceDelayedUpdates;

type LinkKey = (CallbackEdgeRef, NodeId, NodeId, LinkKind);

impl GraphPass for CoalesceDelayedUpdates {
    fn name(&self) -> &'static str {
        "experimental-coalesce-delayed-updates"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let turbo: FxHashSet<_> = graph.wires.iter().filter_map(|wire| wire.turbo_route).collect();
        let mut side_inputs = vec![false; graph.nodes.len()];
        for link in &graph.power_links {
            if let PowerTarget::Input { node, port: InputPort::Side } = link.to {
                side_inputs[node.index()] = true;
            }
        }
        let mut removals = Vec::new();
        let mut links = FxHashMap::<LinkKey, usize>::default();
        let mut before = 0;
        let mut removed = 0;
        let mut eligible = 0;
        let mut repeated = 0;
        let mut turbo_rows = 0;
        for (index, route) in graph.routes.iter().enumerate() {
            let id = RouteId::try_from_index(index).ok_or("route table exceeds typed ID range")?;
            let mut seen = FxHashSet::default();
            let mut route_seen = FxHashSet::default();
            let mut ordinals = Vec::new();
            before += route.deliveries.len();
            if turbo.contains(&id) {
                turbo_rows += route.deliveries.len();
            }
            for (ordinal, &delivery) in route.deliveries.iter().enumerate() {
                if context.monitor.cancelled() {
                    return Err("cancelled while coalescing delayed updates".into());
                }
                let Some(node) = (!turbo.contains(&id)).then(|| delayed_receiver(graph, &side_inputs, delivery)).flatten() else {
                    seen.clear();
                    continue;
                };
                eligible += 1;
                let key = (
                    node,
                    match graph.nodes[node.index()].kind {
                        CircuitNodeKind::Observer { .. } => delivery.kind,
                        _ => DeliveryKind::Update { direction: None },
                    },
                );
                if !route_seen.insert(key) {
                    repeated += 1;
                }
                if seen.insert(key) {
                    continue;
                }
                ordinals.push(ordinal);
                removed += 1;
                // Implicit ordinary receivers have no explicit callback link.
                if delivery.receiver.is_some() {
                    *links
                        .entry((
                            CallbackEdgeRef {
                                route: id,
                                sequence: delivery.sequence,
                            },
                            route.source,
                            node,
                            delivery.link_kind(),
                        ))
                        .or_default() += 1;
                }
            }
            if !ordinals.is_empty() {
                removals.push((index, ordinals));
            }
        }
        if context.monitor.cancelled() {
            return Err("cancelled before applying delayed-update coalescing".into());
        }
        graph.links.retain(|link| {
            if let Some(count) = links.get_mut(&(link.callback, link.from, link.to, link.kind)).filter(|count| **count > 0) {
                *count -= 1;
                false
            } else {
                true
            }
        });
        for (index, ordinals) in removals {
            let route = &mut graph.routes[index];
            let mut ordinals = ordinals.into_iter().peekable();
            route.deliveries = std::mem::take(&mut route.deliveries)
                .into_vec()
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, row)| {
                    if ordinals.peek() == Some(&ordinal) {
                        ordinals.next();
                        None
                    } else {
                        Some(row)
                    }
                })
                .collect();
        }
        context.monitor.record_preparation_tables(
            [
                ("deliveries before", before),
                ("coalesced occurrences", removed),
                ("deliveries after", before - removed),
                ("eligible ordinary occurrences", eligible),
                ("eligible repetitions across barriers", repeated - removed),
                ("protected Turbo occurrences", turbo_rows),
            ]
            .into_iter()
            .map(|(table, entries)| PreparationTableStatistics {
                stage: "experimental delayed updates",
                table: table.into(),
                entries,
                capacity: entries,
                estimated_bytes: 0,
            })
            .collect(),
        );
        Ok(())
    }
}

fn delayed_receiver(graph: &CompileGraph, side_inputs: &[bool], delivery: RouteDelivery) -> Option<NodeId> {
    if delivery.view != OperandView::Live
        || !matches!(delivery.kind, DeliveryKind::Update { .. })
        || graph.guards[delivery.guard.index()] != Guard::Always
    {
        return None;
    }
    let SiteSource::Component(site_node) = graph.sites[delivery.target.index()].source else {
        return None;
    };
    let node = match delivery.receiver {
        Some(CallbackReceiver::Component(node)) if node == site_node => node,
        None => site_node,
        _ => return None,
    };
    match graph.nodes[node.index()].kind {
        CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Comparator { .. }) | CircuitNodeKind::Observer { .. } => Some(node),
        CircuitNodeKind::Electrical(NodeType::Repeater { .. })
            if !side_inputs[node.index()]
                && matches!(&graph.nodes[node.index()].state, ComponentState::Electrical(state) if !state.repeater_locked) =>
        {
            Some(node)
        }
        _ => None,
    }
}
