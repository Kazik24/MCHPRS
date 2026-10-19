//! Simplifies pure guard expressions, removes zero or exactly dominated power
//! reads and impossible wire relations, and drops only callback deliveries whose Graph handler is proven inert.
//!
//! Live reads of immutable constant profiles may fold; captured reads, contacts,
//! component state, and all stateful handlers remain general. Power dominance
//! requires the complete source/query/face/guard key. Callback link multiplicity
//! is checked before mutation, and surviving route order and sequence numbers are
//! unchanged. The proof is local Boolean algebra plus the explicit Graph dispatch
//! table; tests cover forward references, dynamic near-matches, callback links,
//! power-key distinctions, and optimized Graph/interpreter execution. The pass
//! depends on a validated guard DAG and parser-validated callback receivers. Its
//! compile cost is linear in guard and link tables plus exact interning; runtime
//! gains come from shorter guards and fewer proven no-op deliveries. Shape
//! callbacks to components are retained because captured Turbo delivery can use
//! their routes to dispatch an Update.

use super::utils::child_before_parent_order;
use super::{GraphPass, PassContext};
use crate::redpiler::TaskMonitor;
use crate::redpiler::components::NodeType;
#[cfg(test)]
use crate::redpiler::passes::Pass;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, CompileGraph, DeliveryKind, Guard, GuardId, LinkKind, OperandView, PowerLink, PowerPort,
    PowerTarget, RouteId, SiteSource, SiteTest,
};
#[cfg(test)]
use crate::redpiler::program_graph::{InputPort, LinkSpec};
use mchprs_blocks::BlockFace;
use mchprs_blocks::blocks::Block;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::hash::{Hash, Hasher};

/// Removes redundant guards and merges compatible update routes.
pub(crate) struct GuardAndUpdateReduction;

impl GraphPass for GuardAndUpdateReduction {
    fn name(&self) -> &'static str {
        "guard-and-update-reduction"
    }

    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String> {
        if graph.guards.first() != Some(&Guard::Always) {
            return Err("guard 0 must remain the shared Always predicate".into());
        }
        Ok(())
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let rewrite = normalize_guards(graph, context.monitor)?;
        let power_links = reduce_power_links(graph, &rewrite, context.monitor)?;
        let removals = find_delivery_removals(graph, &rewrite, context.monitor)?;
        let mut link_removals = plan_link_removals(graph, &rewrite, &removals)?;

        if context.monitor.cancelled() {
            return Err("cancelled before applying guard and delivery reductions".into());
        }
        apply_rewrite(graph, rewrite, power_links, &removals, &mut link_removals);
        debug_assert!(link_removals.values().all(|count| *count == 0));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PredicateFact {
    Always,
    Never,
    Unknown,
}

struct GuardRewrite {
    /// These are the exact interned expressions retained by the rewritten graph.
    guards: Vec<Guard>,
    /// This maps each old guard owner to its normalized expression ID.
    old_to_new: Vec<GuardId>,
    /// This classifies normalized predicates for safe dead-delivery pruning.
    facts: Vec<PredicateFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemovalReason {
    ImpossibleGuard,
    IdentityHandler,
}

struct DeliveryRemoval {
    /// This identifies the route whose ordered occurrence is removed.
    route: RouteId,
    /// This is the original route ordinal, which distinguishes duplicate events.
    ordinal: u32,
    /// This preserves the parser's native event sequence as proof provenance.
    original_sequence: u32,
    /// This records the local reason that makes this occurrence unobservable.
    reason: RemovalReason,
}

type CallbackLinkKey = (
    CallbackEdgeRef,
    crate::redpiler::program_graph::NodeId,
    crate::redpiler::program_graph::NodeId,
    LinkKind,
);
type PowerKey = (crate::redpiler::program_graph::SiteId, PowerPort, PowerTarget, BlockFace, GuardId);

fn normalize_guards(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<GuardRewrite, String> {
    let order = child_before_parent_order(&graph.guards, monitor)?;
    let mut interner = GuardInterner::new();
    let mut old_to_new = vec![GuardId::new(0); graph.guards.len()];

    for old_index in order {
        if monitor.cancelled() {
            return Err("cancelled while normalizing guards".into());
        }
        let normalized = match &graph.guards[old_index] {
            Guard::Always => interner.constant(PredicateFact::Always)?,
            Guard::State(test) => interner.intern(Guard::State(*test), PredicateFact::Unknown)?,
            Guard::Site { site, view, test } => {
                let fact = immutable_site_fact(graph, *site, *view, *test);
                match fact {
                    PredicateFact::Always | PredicateFact::Never => interner.constant(fact)?,
                    PredicateFact::Unknown => interner.intern(
                        Guard::Site {
                            site: *site,
                            view: *view,
                            test: *test,
                        },
                        PredicateFact::Unknown,
                    )?,
                }
            }
            Guard::Not(child) => {
                let child = old_to_new[child.index()];
                match interner.facts[child.index()] {
                    PredicateFact::Always => interner.constant(PredicateFact::Never)?,
                    PredicateFact::Never => interner.constant(PredicateFact::Always)?,
                    PredicateFact::Unknown => match &interner.guards[child.index()] {
                        Guard::Not(grandchild) => *grandchild,
                        _ => interner.intern(Guard::Not(child), PredicateFact::Unknown)?,
                    },
                }
            }
            Guard::All(children) | Guard::Any(children) => {
                let is_all = matches!(&graph.guards[old_index], Guard::All(_));
                match join_form(children, is_all, &old_to_new, &interner.facts) {
                    JoinForm::Existing(id) => id,
                    JoinForm::Constant(fact) => interner.constant(fact)?,
                    JoinForm::New(guard) => interner.intern(guard, PredicateFact::Unknown)?,
                }
            }
        };
        old_to_new[old_index] = normalized;
        if old_index == 0 && normalized != GuardId::new(0) {
            return Err("guard 0 did not remain the shared Always predicate".into());
        }
    }

    Ok(GuardRewrite {
        guards: interner.guards,
        old_to_new,
        facts: interner.facts,
    })
}

enum JoinForm {
    /// This join simplifies to an expression already interned by a child.
    Existing(GuardId),
    /// This join folds to a Boolean constant.
    Constant(PredicateFact),
    /// This join retains its normalized ordered children.
    New(Guard),
}

fn join_form(children: &[GuardId], is_all: bool, old_to_new: &[GuardId], facts: &[PredicateFact]) -> JoinForm {
    let identity = if is_all { PredicateFact::Always } else { PredicateFact::Never };
    let absorber = if is_all { PredicateFact::Never } else { PredicateFact::Always };
    let mut normalized_children = Vec::with_capacity(children.len());
    for old_child in children {
        let child = old_to_new[old_child.index()];
        let fact = facts[child.index()];
        if fact == absorber {
            return JoinForm::Constant(absorber);
        }
        if fact != identity && !normalized_children.contains(&child) {
            normalized_children.push(child);
        }
    }
    match normalized_children.as_slice() {
        [] => JoinForm::Constant(identity),
        [child] => JoinForm::Existing(*child),
        _ => {
            let children = normalized_children.into_boxed_slice();
            let guard = if is_all { Guard::All(children) } else { Guard::Any(children) };
            JoinForm::New(guard)
        }
    }
}

fn immutable_site_fact(graph: &CompileGraph, site: crate::redpiler::program_graph::SiteId, view: OperandView, test: SiteTest) -> PredicateFact {
    if view != OperandView::Live {
        return PredicateFact::Unknown;
    }
    let SiteSource::Constant(profile_id) = graph.sites[site.index()].source else {
        return PredicateFact::Unknown;
    };
    let profile = graph.profiles[profile_id.index()];
    let matches = match test {
        SiteTest::Solid => profile.solid,
        SiteTest::Transparent => profile.transparent,
        SiteTest::IsWire => matches!(Block::from_id(profile.raw_state), Block::RedstoneWire { .. }),
        SiteTest::Profile(expected) => profile_id == expected,
    };
    if matches { PredicateFact::Always } else { PredicateFact::Never }
}

struct GuardInterner {
    /// This stores normalized expressions in deterministic first-seen order.
    guards: Vec<Guard>,
    /// This stores each normalized predicate's compile-time truth classification.
    facts: Vec<PredicateFact>,
    /// This maps each expression hash to the head of its exact-equality chain.
    bucket_heads: FxHashMap<u64, u32>,
    /// This links hash-collision candidates without one heap allocation per key.
    collision_next: Vec<u32>,
}

impl GuardInterner {
    fn new() -> Self {
        let always = Guard::Always;
        let mut bucket_heads = FxHashMap::default();
        bucket_heads.insert(guard_hash(&always), 0);
        Self {
            guards: vec![always],
            facts: vec![PredicateFact::Always],
            bucket_heads,
            collision_next: vec![u32::MAX],
        }
    }

    fn constant(&mut self, fact: PredicateFact) -> Result<GuardId, String> {
        let guard = match fact {
            PredicateFact::Always => Guard::Always,
            PredicateFact::Never => Guard::Not(GuardId::new(0)),
            PredicateFact::Unknown => return Err("unknown predicate is not a constant".into()),
        };
        self.intern(guard, fact)
    }

    fn intern(&mut self, guard: Guard, fact: PredicateFact) -> Result<GuardId, String> {
        let hash = guard_hash(&guard);
        let mut candidate = self.bucket_heads.get(&hash).copied();
        while let Some(index) = candidate {
            if self.guards[index as usize] == guard {
                return Ok(GuardId::new(index));
            }
            let next = self.collision_next[index as usize];
            candidate = (next != u32::MAX).then_some(next);
        }

        let index = u32::try_from(self.guards.len()).map_err(|_| "normalized guard table exceeds typed ID range".to_owned())?;
        if index == u32::MAX {
            return Err("normalized guard table exceeds collision index range".into());
        }
        let id = GuardId::new(index);
        let next = self.bucket_heads.insert(hash, index).unwrap_or(u32::MAX);
        self.guards.push(guard);
        self.facts.push(fact);
        self.collision_next.push(next);
        Ok(id)
    }
}

fn guard_hash(guard: &Guard) -> u64 {
    let mut hasher = FxHasher::default();
    guard.hash(&mut hasher);
    hasher.finish()
}

fn reduce_power_links(graph: &CompileGraph, rewrite: &GuardRewrite, monitor: &TaskMonitor) -> Result<Vec<PowerLink>, String> {
    let mut minimum = FxHashMap::<PowerKey, u8>::default();
    for link in &graph.power_links {
        if monitor.cancelled() {
            return Err("cancelled while checking power contributions".into());
        }
        let guard = rewrite.old_to_new[link.guard.index()];
        if link.attenuation == 15 || rewrite.facts[guard.index()] == PredicateFact::Never {
            continue;
        }
        let key = power_key(*link, guard);
        minimum
            .entry(key)
            .and_modify(|least| *least = (*least).min(link.attenuation))
            .or_insert(link.attenuation);
    }

    let mut retained_keys = FxHashSet::default();
    let mut reduced = Vec::with_capacity(minimum.len());
    for link in &graph.power_links {
        if monitor.cancelled() {
            return Err("cancelled while retaining power contributions".into());
        }
        let guard = rewrite.old_to_new[link.guard.index()];
        if link.attenuation == 15 || rewrite.facts[guard.index()] == PredicateFact::Never {
            continue;
        }
        let key = power_key(*link, guard);
        if minimum.get(&key) == Some(&link.attenuation) && retained_keys.insert(key) {
            reduced.push(PowerLink { guard, ..*link });
        }
    }
    Ok(reduced)
}

fn power_key(link: PowerLink, guard: GuardId) -> PowerKey {
    (link.from, link.from_port, link.to, link.face, guard)
}

fn find_delivery_removals(graph: &CompileGraph, rewrite: &GuardRewrite, monitor: &TaskMonitor) -> Result<Vec<DeliveryRemoval>, String> {
    let mut removals = Vec::new();
    for (route_index, route) in graph.routes.iter().enumerate() {
        let route_id = RouteId::try_from_index(route_index).ok_or_else(|| "route table exceeds typed ID range".to_owned())?;
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            if monitor.cancelled() {
                return Err("cancelled while classifying route deliveries".into());
            }
            let guard = rewrite.old_to_new[delivery.guard.index()];
            let reason = if rewrite.facts[guard.index()] == PredicateFact::Never {
                Some(RemovalReason::ImpossibleGuard)
            } else {
                identity_handler(graph, *delivery).then_some(RemovalReason::IdentityHandler)
            };
            if let Some(reason) = reason {
                removals.push(DeliveryRemoval {
                    route: route_id,
                    ordinal: u32::try_from(ordinal).map_err(|_| "route occurrence exceeds diagnostic range".to_owned())?,
                    original_sequence: delivery.sequence,
                    reason,
                });
            }
        }
    }
    Ok(removals)
}

fn identity_handler(graph: &CompileGraph, delivery: crate::redpiler::program_graph::RouteDelivery) -> bool {
    let source = graph.sites[delivery.target.index()].source;
    let receiver = delivery.receiver.or_else(|| match (source, delivery.kind) {
        (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
        _ => None,
    });
    match (delivery.kind, receiver) {
        (DeliveryKind::Update { .. }, Some(CallbackReceiver::Component(node))) => {
            matches!(
                graph.nodes[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant)
            )
        }
        (DeliveryKind::Shape { .. }, None) => matches!(source, SiteSource::Constant(_)),
        _ => false,
    }
}

fn plan_link_removals(
    graph: &CompileGraph,
    rewrite: &GuardRewrite,
    removals: &[DeliveryRemoval],
) -> Result<FxHashMap<CallbackLinkKey, usize>, String> {
    let mut needed = FxHashMap::<CallbackLinkKey, usize>::default();
    for removal in removals {
        let route = &graph.routes[removal.route.index()];
        let delivery = route.deliveries[removal.ordinal as usize];
        let Some(receiver) = delivery.receiver else {
            continue;
        };
        let to = receiver_node(graph, receiver);
        let kind = remap_link_kind(delivery.link_kind(), &rewrite.old_to_new);
        let key = (
            CallbackEdgeRef {
                route: removal.route,
                sequence: removal.original_sequence,
            },
            route.source,
            to,
            kind,
        );
        *needed.entry(key).or_default() += 1;
    }

    let mut available = FxHashMap::<CallbackLinkKey, usize>::default();
    for link in &graph.links {
        let kind = remap_link_kind(link.kind, &rewrite.old_to_new);
        *available.entry((link.callback, link.from, link.to, kind)).or_default() += 1;
    }
    for (key, count) in &needed {
        if available.get(key).copied().unwrap_or_default() < *count {
            return Err("callback removals do not match the validated link multiset".into());
        }
    }
    Ok(needed)
}

fn receiver_node(graph: &CompileGraph, receiver: CallbackReceiver) -> crate::redpiler::program_graph::NodeId {
    match receiver {
        CallbackReceiver::Component(node) => node,
        CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => graph.pistons[piston.index()].node,
        CallbackReceiver::Contact(contact) => graph.contacts[contact.index()].node,
    }
}

fn remap_link_kind(kind: LinkKind, mapping: &[GuardId]) -> LinkKind {
    match kind {
        LinkKind::Update { direction, guard } => LinkKind::Update {
            direction,
            guard: mapping[guard.index()],
        },
        LinkKind::Shape { direction, guard } => LinkKind::Shape {
            direction,
            guard: mapping[guard.index()],
        },
    }
}

fn apply_rewrite(
    graph: &mut CompileGraph,
    rewrite: GuardRewrite,
    power_links: Vec<PowerLink>,
    removals: &[DeliveryRemoval],
    link_removals: &mut FxHashMap<CallbackLinkKey, usize>,
) {
    for contact in &mut graph.contacts {
        for variant in contact.variants.iter_mut() {
            variant.when = rewrite.old_to_new[variant.when.index()];
        }
    }
    for input in &mut graph.comparator_inputs {
        if let Some((_, _, guard)) = &mut input.far_override {
            *guard = rewrite.old_to_new[guard.index()];
        }
    }
    graph.wire_relations.retain_mut(|relation| {
        relation.guard = rewrite.old_to_new[relation.guard.index()];
        rewrite.facts[relation.guard.index()] != PredicateFact::Never
    });
    graph.wire_relations.shrink_to_fit();
    graph.power_links = power_links;
    graph.guards = rewrite.guards;

    for link in &mut graph.links {
        link.kind = remap_link_kind(link.kind, &rewrite.old_to_new);
    }
    graph.links.retain_mut(|link| {
        let key = (link.callback, link.from, link.to, link.kind);
        if let Some(remaining) = link_removals.get_mut(&key).filter(|count| **count > 0) {
            *remaining -= 1;
            false
        } else {
            true
        }
    });

    let mut next_removal = 0;
    for (route_index, route) in graph.routes.iter_mut().enumerate() {
        let first = next_removal;
        while next_removal < removals.len() && removals[next_removal].route.index() == route_index {
            next_removal += 1;
        }
        if first == next_removal {
            for delivery in route.deliveries.iter_mut() {
                delivery.guard = rewrite.old_to_new[delivery.guard.index()];
            }
            continue;
        }

        let old = std::mem::replace(&mut route.deliveries, Vec::new().into_boxed_slice());
        let mut kept = Vec::with_capacity(old.len() - (next_removal - first));
        let mut removal_index = first;
        for (ordinal, mut delivery) in old.into_vec().into_iter().enumerate() {
            if removal_index < next_removal && removals[removal_index].ordinal as usize == ordinal {
                debug_assert!(matches!(
                    removals[removal_index].reason,
                    RemovalReason::ImpossibleGuard | RemovalReason::IdentityHandler
                ));
                removal_index += 1;
            } else {
                delivery.guard = rewrite.old_to_new[delivery.guard.index()];
                kept.push(delivery);
            }
        }
        debug_assert_eq!(removal_index, next_removal);
        route.deliveries = kept.into_boxed_slice();
    }
    debug_assert_eq!(next_removal, removals.len());
}

#[cfg(test)]
mod tests {
    use super::super::{PassEntry, PassPipeline};
    use super::*;
    use crate::redpiler::CompilerOptions;
    use crate::redpiler::components::NodeState;
    use crate::redpiler::program_graph::{
        CircuitNodeDraft, ComponentState, EntryState, Profile, ProfileElectrical, Route, RouteDelivery, SiteId, SiteSpec,
    };
    use mchprs_blocks::BlockPos;

    #[test]
    fn impossible_wire_relations_are_removed_but_captured_relations_remain() {
        use crate::redpiler::program_graph::{NodeId, WirePlan, WireRelation, WireRelationKind};
        let mut graph = inert_graph(NodeType::Wire);
        let wire = Default::default();
        graph.nodes[0].raw_state = Some(Block::RedstoneWire { wire }.get_id());
        graph.nodes[0].kind = CircuitNodeKind::Wire(0);
        graph.nodes[0].state = ComponentState::Wire(wire);
        graph.wires.push(WirePlan {
            node: NodeId::new(0),
            canonical_neighbors: [SiteId::new(0); 24],
            shape_route: None,
            turbo_route: Some(RouteId::new(0)),
            execution: Default::default(),
        });
        graph.guards.extend([
            Guard::Not(GuardId::new(0)),
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Captured,
                test: SiteTest::IsWire,
            },
            Guard::All(vec![GuardId::new(1), GuardId::new(2)].into_boxed_slice()),
        ]);
        graph.wire_relations = (0..4)
            .map(|guard| WireRelation {
                from: NodeId::new(0),
                to: SiteId::new(0),
                kind: WireRelationKind::Step,
                direction: BlockFace::North,
                guard: GuardId::new(guard),
            })
            .collect();
        graph.validate().unwrap();
        run_pass(&mut graph);
        assert_eq!(graph.wire_relations.len(), 2);
        assert_eq!(graph.wire_relations[0].guard, GuardId::new(0));
        assert!(matches!(
            graph.guards[graph.wire_relations[1].guard.index()],
            Guard::Site {
                view: OperandView::Captured,
                ..
            }
        ));
        let once = graph.clone();
        run_pass(&mut graph);
        assert_eq!(graph, once);
    }

    #[test]
    fn optimization_levels_select_rewrites_before_dependent_plan_builders() {
        use crate::redpiler::passes::{Pass, default_graph_pipeline};
        let expected = [
            (1, "guard-and-update-reduction"),
            (1, "prune-inert-callbacks"),
            (1, "prune-unhandled-updates"),
            (2, "simplify-state-guards"),
            (3, "simplify-instant-resets"),
            (3, "specialize-instant-pistons"),
            (3, "compile-builtin-resets"),
            (5, "experimental-coalesce-delayed-updates"),
            (1, "index-turbo-callbacks"),
            (1, "contract-static-dust-paths"),
            (2, "specialize-wire-queries"),
            (2, "compile-wire-hops"),
            (2, "compile-wire-callbacks"),
            (3, "compile-instant-resets"),
            (3, "identify-instant-regions"),
            (3, "compile-instant-proxies"),
            (2, "compile-fixed-signals"),
            (2, "compile-fixed-classic"),
            (2, "compile-mixed-classic"),
            (2, "compile-classic-redstone"),
        ];
        for level in [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), None] {
            let pipeline = default_graph_pipeline(&CompilerOptions {
                optimization_level: level,
                ..Default::default()
            });
            #[cfg(not(feature = "unsafe-turbo-fixed-point"))]
            assert_eq!(pipeline.entries.len(), expected.len());
            for (index, (entry, &(minimum, name))) in pipeline.entries.iter().zip(&expected).enumerate() {
                assert_eq!(entry.pass.name(), name);
                let enabled = name != "compile-classic-redstone" && level.is_none_or(|level| level >= minimum);
                assert_eq!(entry.enabled, enabled, "{name} at {level:?}");
                assert_eq!(matches!(entry.pass, Pass::Rewrite(_)), index < 8);
            }
        }
    }

    #[test]
    fn guard_normalization_handles_forward_references_and_dynamic_reads() {
        let mut graph = inert_graph(NodeType::Constant);
        let air = Block::Air;
        graph.profiles.push(Profile {
            raw_state: air.get_id(),
            registry_type: air.registry_id(),
            solid: air.is_solid(),
            transparent: air.is_transparent(),
            cube: air.is_cube(),
            electrical: ProfileElectrical::None,
        });
        graph.sites.push(SiteSpec {
            position: BlockPos::new(1, 0, 0),
            source: SiteSource::Constant(crate::redpiler::program_graph::ProfileId::new(0)),
            source_strength: 0,
        });
        graph.guards = vec![
            Guard::Always,
            Guard::Not(GuardId::new(2)), // Forward reference to the constant site test.
            Guard::Site {
                site: SiteId::new(1),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
            Guard::Site {
                site: SiteId::new(1),
                view: OperandView::Captured,
                test: SiteTest::Solid,
            },
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Captured,
                test: SiteTest::Solid,
            },
            Guard::All(vec![GuardId::new(0), GuardId::new(4), GuardId::new(4)].into_boxed_slice()),
            Guard::All(vec![GuardId::new(4), GuardId::new(5)].into_boxed_slice()),
            Guard::Any(Vec::new().into_boxed_slice()),
            Guard::Site {
                site: SiteId::new(1),
                view: OperandView::Live,
                test: SiteTest::Transparent,
            },
            Guard::Site {
                site: SiteId::new(1),
                view: OperandView::Live,
                test: SiteTest::IsWire,
            },
            Guard::Site {
                site: SiteId::new(1),
                view: OperandView::Live,
                test: SiteTest::Profile(crate::redpiler::program_graph::ProfileId::new(0)),
            },
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
            Guard::Not(GuardId::new(14)), // Double negation also crosses forward references.
            Guard::Not(GuardId::new(4)),
        ];
        let rewrite = normalize_guards(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(rewrite.old_to_new[0], GuardId::new(0));
        assert_eq!(rewrite.facts[rewrite.old_to_new[2].index()], PredicateFact::Never);
        assert_eq!(rewrite.facts[rewrite.old_to_new[1].index()], PredicateFact::Always);
        assert_eq!(rewrite.facts[rewrite.old_to_new[3].index()], PredicateFact::Unknown);
        assert_eq!(rewrite.facts[rewrite.old_to_new[8].index()], PredicateFact::Never);
        assert_eq!(rewrite.old_to_new[6], rewrite.old_to_new[4]);
        assert_ne!(rewrite.old_to_new[5], rewrite.old_to_new[4]);
        assert_eq!(rewrite.old_to_new[12], rewrite.old_to_new[4]);
        assert_eq!(rewrite.old_to_new[13], rewrite.old_to_new[4]);
        assert_ne!(rewrite.old_to_new[14], rewrite.old_to_new[4]);
        assert_eq!(
            rewrite.facts[rewrite.old_to_new[9].index()],
            if air.is_transparent() {
                PredicateFact::Always
            } else {
                PredicateFact::Never
            }
        );
        assert_eq!(rewrite.facts[rewrite.old_to_new[10].index()], PredicateFact::Never);
        assert_eq!(rewrite.facts[rewrite.old_to_new[11].index()], PredicateFact::Always);
        assert!(matches!(
            rewrite.guards[rewrite.old_to_new[7].index()],
            Guard::All(ref children)
                if children.as_ref() == [rewrite.old_to_new[4], rewrite.old_to_new[5]]
        ));
    }

    #[test]
    fn power_pruning_keeps_only_exact_key_minimums() {
        let mut graph = inert_graph(NodeType::Constant);
        graph.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: OperandView::Live,
            test: SiteTest::Solid,
        });
        let base = PowerLink {
            from: SiteId::new(0),
            from_port: PowerPort::Strong(BlockFace::Top),
            to: PowerTarget::Input {
                node: crate::redpiler::program_graph::NodeId::new(0),
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 2,
            guard: GuardId::new(0),
        };
        graph.power_links = vec![
            PowerLink { attenuation: 5, ..base },
            base,
            base,
            PowerLink { attenuation: 15, ..base },
            PowerLink {
                from_port: PowerPort::Stored,
                ..base
            },
            PowerLink {
                from_port: PowerPort::Conducted { include_dust: false },
                ..base
            },
            PowerLink {
                from_port: PowerPort::Conducted { include_dust: true },
                ..base
            },
            PowerLink {
                to: PowerTarget::Input {
                    node: crate::redpiler::program_graph::NodeId::new(0),
                    port: InputPort::Side,
                },
                ..base
            },
            PowerLink {
                face: BlockFace::Bottom,
                ..base
            },
            PowerLink {
                guard: GuardId::new(1),
                ..base
            },
            PowerLink {
                to: PowerTarget::Conductor {
                    site: SiteId::new(0),
                    include_dust: false,
                },
                ..base
            },
            PowerLink {
                to: PowerTarget::Conductor {
                    site: SiteId::new(0),
                    include_dust: true,
                },
                ..base
            },
        ];
        let rewrite = normalize_guards(&graph, &TaskMonitor::default()).unwrap();
        let reduced = reduce_power_links(&graph, &rewrite, &TaskMonitor::default()).unwrap();
        assert_eq!(reduced.len(), 9);
        assert_eq!(reduced[0].attenuation, 2);
        assert_eq!(reduced[1].from_port, PowerPort::Stored);
        assert_eq!(reduced[2].from_port, PowerPort::Conducted { include_dust: false });
        assert_eq!(reduced[3].from_port, PowerPort::Conducted { include_dust: true });
        assert_eq!(
            reduced[4].to,
            PowerTarget::Input {
                node: crate::redpiler::program_graph::NodeId::new(0),
                port: InputPort::Side,
            }
        );
        assert_eq!(reduced[5].face, BlockFace::Bottom);
        assert_eq!(reduced[6].guard, GuardId::new(1));
        assert_eq!(
            reduced[8].to,
            PowerTarget::Conductor {
                site: SiteId::new(0),
                include_dust: true
            }
        );
    }

    #[test]
    fn attenuation_dominance_holds_for_every_redstone_strength() {
        for strength in 0u8..=15 {
            for stronger in 0u8..=15 {
                for weaker in stronger..=15 {
                    assert!(strength.saturating_sub(stronger) >= strength.saturating_sub(weaker));
                }
            }
        }
    }

    #[test]
    fn inert_control_updates_are_removed_but_duplicate_torch_updates_remain() {
        let mut control = inert_graph(NodeType::Lever);
        let mut first = update_delivery(GuardId::new(0), 4);
        first.receiver = None;
        control.routes[0].deliveries = vec![first, update_delivery(GuardId::new(0), 7)].into_boxed_slice();
        control.links = vec![callback_link(&control.routes[0].deliveries[1])];
        control.validate().unwrap();
        run_pass(&mut control);
        assert!(control.routes[0].deliveries.is_empty());
        assert!(control.links.is_empty());

        let mut torch = inert_graph(NodeType::Torch);
        torch.guards.push(Guard::Not(GuardId::new(0)));
        torch.routes[0].deliveries = vec![
            update_delivery(GuardId::new(1), 4),
            update_delivery(GuardId::new(0), 9),
            update_delivery(GuardId::new(0), 9),
        ]
        .into_boxed_slice();
        torch.links = torch.routes[0].deliveries.iter().map(|delivery| callback_link(delivery)).collect();
        torch.validate().unwrap();
        run_pass(&mut torch);
        assert_eq!(torch.routes[0].deliveries.len(), 2);
        assert_eq!(torch.routes[0].deliveries[0].sequence, 9);
        assert_eq!(torch.routes[0].deliveries[1].sequence, 9);
        assert_eq!(torch.links.len(), 2);
        torch.validate().unwrap();
        let reduced = torch.clone();
        run_pass(&mut torch);
        assert_eq!(torch, reduced, "the rewrite is idempotent");
    }

    #[test]
    fn pipeline_runs_reduction_and_disabled_entry_keeps_input() {
        let mut graph = inert_graph(NodeType::Lever);
        graph.routes[0].deliveries = vec![update_delivery(GuardId::new(0), 3)].into_boxed_slice();
        graph.links = vec![callback_link(&graph.routes[0].deliveries[0])];
        let input = graph.clone();
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: Vec::new(),
        };
        let disabled = PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(GuardAndUpdateReduction),
            enabled: false,
        }]);
        super::super::run_graph_pipeline(&mut graph, &options, &entry, &disabled, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, input);

        let enabled = PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(GuardAndUpdateReduction),
            enabled: true,
        }]);
        super::super::run_graph_pipeline(
            &mut graph,
            &CompilerOptions {
                optimization_level: None,
                ..Default::default()
            },
            &entry,
            &enabled,
            &TaskMonitor::default(),
        )
        .unwrap();
        assert!(graph.routes[0].deliveries.is_empty());
        graph.validate().unwrap();
        let reduced = graph.clone();
        run_pass(&mut graph);
        assert_eq!(graph, reduced, "the configured rewrite is idempotent");
    }

    #[test]
    fn cancelled_reduction_leaves_the_graph_untouched() {
        let mut graph = inert_graph(NodeType::Lever);
        graph.routes[0].deliveries = vec![update_delivery(GuardId::new(0), 3)].into_boxed_slice();
        let input = graph.clone();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        let options = CompilerOptions {
            optimization_level: None,
            ..Default::default()
        };
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: Vec::new(),
        };
        let context = PassContext {
            monitor: &monitor,
            options: &options,
            entry: &entry,
        };
        assert!(GuardAndUpdateReduction.run(&mut graph, &context).is_err());
        assert_eq!(graph, input);
    }

    fn run_pass(graph: &mut CompileGraph) {
        let monitor = TaskMonitor::default();
        let options = CompilerOptions {
            optimization_level: None,
            ..Default::default()
        };
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: Vec::new(),
        };
        let context = PassContext {
            monitor: &monitor,
            options: &options,
            entry: &entry,
        };
        GuardAndUpdateReduction.run(graph, &context).unwrap();
        graph.validate().unwrap();
    }

    fn update_delivery(guard: GuardId, sequence: u32) -> RouteDelivery {
        RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard,
            view: OperandView::Live,
            sequence,
            receiver: Some(CallbackReceiver::Component(crate::redpiler::program_graph::NodeId::new(0))),
        }
    }

    fn callback_link(delivery: &RouteDelivery) -> LinkSpec {
        LinkSpec {
            from: crate::redpiler::program_graph::NodeId::new(0),
            to: crate::redpiler::program_graph::NodeId::new(0),
            kind: LinkKind::Update {
                direction: None,
                guard: delivery.guard,
            },
            callback: CallbackEdgeRef {
                route: RouteId::new(0),
                sequence: delivery.sequence,
            },
        }
    }

    fn inert_graph(kind: NodeType) -> CompileGraph {
        let block = Block::RedstoneBlock;
        let position = BlockPos::new(0, 0, 0);
        let node = crate::redpiler::program_graph::NodeId::new(0);
        let route = RouteId::new(0);
        CompileGraph {
            nodes: vec![CircuitNodeDraft {
                position: Some(position),
                raw_state: Some(block.get_id()),
                kind: CircuitNodeKind::Electrical(kind),
                state: ComponentState::Electrical(NodeState::simple(false)),
                output_route: Some(route),
            }],
            links: Vec::new(),
            power_links: Vec::new(),
            comparator_inputs: Vec::new(),
            profiles: Vec::new(),
            guards: vec![Guard::Always],
            contacts: Vec::new(),
            routes: vec![Route {
                source: node,
                deliveries: Vec::new().into_boxed_slice(),
            }],

            wires: Vec::new(),

            wire_relations: Vec::new(),
            sites: vec![SiteSpec {
                position,
                source: SiteSource::Component(node),
                source_strength: 0,
            }],
            pistons: Vec::new(),
            resources: Vec::new(),
            initial_pistons: Vec::new(),
            initial_resources: Vec::new(),

            plans: Default::default(),
        }
    }
}
