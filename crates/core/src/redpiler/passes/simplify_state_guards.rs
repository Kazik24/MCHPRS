//! Simplifies direct same-field state tests in `All` and `Any` guards.
//!
//! It removes contradictory equalities, redundant equality/inequality pairs,
//! and complete finite-domain tests for piston booleans and availability. It
//! never infers resource-location exhaustiveness because runtime writer closure
//! is not established. Replacements are Boolean-equivalent for every field
//! value; callback delivery witnesses are retained when the graph validator
//! requires their syntax. The pass runs after O1 guard normalization, keeps
//! route and dust metadata intact, and costs linear guard/reference traversal
//! plus bounded work per direct join. Exhaustive set tests and graph-reference
//! tests cover the rewrite.

use super::utils::{child_before_parent_order, guard_children};
use super::{GraphPass, PassContext};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CompileGraph, DeliveryKind, Guard, GuardId, LinkKind, OperandView, PistonAvailability, PistonId, ResourceId,
    RouteDelivery, SiteSource, SiteTest, StateTest,
};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

const MAX_DIRECT_CHILDREN: usize = 64;

type CallbackLinkKey = (
    CallbackEdgeRef,
    crate::redpiler::program_graph::NodeId,
    crate::redpiler::program_graph::NodeId,
    LinkKind,
);

/// Rewrites only literals that inspect the same scalar field.
pub(crate) struct SimplifyStateGuards;

impl GraphPass for SimplifyStateGuards {
    fn name(&self) -> &'static str {
        "simplify-state-guards"
    }

    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String> {
        if graph.guards.first() != Some(&Guard::Always) {
            return Err("guard 0 must remain the shared Always predicate".into());
        }
        Ok(())
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let mut interner = GuardInterner::new();
        let mut old_to_new = vec![GuardId::new(0); graph.guards.len()];
        for old_index in child_before_parent_order(&graph.guards, context.monitor)? {
            if context.monitor.cancelled() {
                return Err("cancelled while simplifying state guards".into());
            }
            let rewritten = match &graph.guards[old_index] {
                Guard::Always => interner.constant(true)?,
                Guard::State(test) => interner.intern(Guard::State(*test))?,
                Guard::Site { site, view, test } => interner.intern(Guard::Site {
                    site: *site,
                    view: *view,
                    test: *test,
                })?,
                Guard::Not(child) => {
                    let child = old_to_new[child.index()];
                    match &interner.guards[child.index()] {
                        Guard::Always => interner.constant(false)?,
                        Guard::Not(grandchild) => *grandchild,
                        _ => interner.intern(Guard::Not(child))?,
                    }
                }
                Guard::All(children) | Guard::Any(children) => {
                    let is_all = matches!(&graph.guards[old_index], Guard::All(_));
                    rewrite_join(children, is_all, graph, &old_to_new, &mut interner, context.monitor)?
                }
            };
            old_to_new[old_index] = rewritten;
            if old_index == 0 && rewritten != GuardId::new(0) {
                return Err("guard 0 did not remain the shared Always predicate".into());
            }
        }

        let callback_guards = callback_delivery_rewrites(graph, &old_to_new, &mut interner, context.monitor)?;
        let mut references = stage_references(graph, &old_to_new, &callback_guards, context.monitor)?;
        let roots = references.roots();
        let (guards, compact_map) = compact_guards(&interner.guards, &roots, context.monitor)?;
        references.remap(&compact_map, context.monitor)?;

        if context.monitor.cancelled() {
            return Err("cancelled before applying state-guard rewrite".into());
        }
        references.apply(graph, guards);
        Ok(())
    }
}

struct GuardInterner {
    guards: Vec<Guard>,
    ids: FxHashMap<Guard, GuardId>,
}

impl GuardInterner {
    fn new() -> Self {
        let always = Guard::Always;
        Self {
            guards: vec![always.clone()],
            ids: FxHashMap::from_iter([(always, GuardId::new(0))]),
        }
    }

    fn constant(&mut self, value: bool) -> Result<GuardId, String> {
        if value {
            self.intern(Guard::Always)
        } else {
            self.intern(Guard::Not(GuardId::new(0)))
        }
    }

    fn intern(&mut self, guard: Guard) -> Result<GuardId, String> {
        if let Some(id) = self.ids.get(&guard) {
            return Ok(*id);
        }
        let index = u32::try_from(self.guards.len()).map_err(|_| "state-guard table exceeds typed ID range".to_owned())?;
        if index == u32::MAX {
            return Err("state-guard table exceeds typed ID range".into());
        }
        let id = GuardId::new(index);
        self.ids.insert(guard.clone(), id);
        self.guards.push(guard);
        Ok(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StateField {
    Extended(PistonId),
    HeadPresent(PistonId),
    Availability(PistonId),
    ResourceContact(ResourceId),
    ResourceEnabled(ResourceId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FieldLiteral {
    field: StateField,
    value: u32,
    equal: bool,
}

fn rewrite_join(
    children: &[GuardId],
    is_all: bool,
    graph: &CompileGraph,
    old_to_new: &[GuardId],
    interner: &mut GuardInterner,
    monitor: &TaskMonitor,
) -> Result<GuardId, String> {
    let identity = is_all;
    let absorber = !is_all;
    let mut normalized = Vec::with_capacity(children.len());
    for old_child in children {
        let child = old_to_new[old_child.index()];
        let value = match &interner.guards[child.index()] {
            Guard::Always => Some(true),
            Guard::Not(always) if *always == GuardId::new(0) => Some(false),
            _ => None,
        };
        if value == Some(absorber) {
            return interner.constant(absorber);
        }
        if value != Some(identity) && !normalized.contains(&child) {
            normalized.push(child);
        }
    }

    if normalized.len() <= MAX_DIRECT_CHILDREN {
        simplify_direct_state_groups(&mut normalized, is_all, graph, interner, monitor)?;
    }

    let mut retained = Vec::with_capacity(normalized.len());
    for child in normalized {
        let value = match &interner.guards[child.index()] {
            Guard::Always => Some(true),
            Guard::Not(always) if *always == GuardId::new(0) => Some(false),
            _ => None,
        };
        if value == Some(absorber) {
            return interner.constant(absorber);
        }
        if value != Some(identity) && !retained.contains(&child) {
            retained.push(child);
        }
    }
    match retained.as_slice() {
        [] => interner.constant(identity),
        [only] => Ok(*only),
        _ => {
            let children = retained.into_boxed_slice();
            interner.intern(if is_all { Guard::All(children) } else { Guard::Any(children) })
        }
    }
}

fn simplify_direct_state_groups(
    children: &mut Vec<GuardId>,
    is_all: bool,
    graph: &CompileGraph,
    interner: &mut GuardInterner,
    monitor: &TaskMonitor,
) -> Result<(), String> {
    let mut groups = Vec::<(StateField, Vec<(usize, FieldLiteral)>)>::new();
    for (index, child) in children.iter().enumerate() {
        let Some(literal) = field_literal(&interner.guards[child.index()], &interner.guards, graph) else {
            continue;
        };
        if let Some((_, group)) = groups.iter_mut().find(|(field, _)| *field == literal.field) {
            group.push((index, literal));
        } else {
            groups.push((literal.field, vec![(index, literal)]));
        }
    }

    let mut removed = vec![false; children.len()];
    let mut replacements = Vec::<(usize, Vec<GuardId>)>::new();
    for (field, group) in groups {
        if monitor.cancelled() {
            return Err("cancelled while simplifying state-field groups".into());
        }
        let literals: Vec<FieldLiteral> = group.iter().map(|(_, literal)| *literal).collect();
        let Some(reduction) = reduce_group(is_all, &literals, finite_domain(field)) else {
            continue;
        };
        let old_cost = literals.iter().map(literal_cost).sum::<usize>();
        let new_cost = match &reduction {
            GroupReduction::Constant(true) => 1,
            GroupReduction::Constant(false) => 2,
            GroupReduction::Literals(literals) => literals.iter().map(literal_cost).sum(),
        };
        let new_count = match &reduction {
            GroupReduction::Constant(_) => 1,
            GroupReduction::Literals(literals) => literals.len(),
        };
        if new_cost >= old_cost && new_count >= group.len() {
            continue;
        }

        let replacement = match reduction {
            GroupReduction::Constant(value) => vec![interner.constant(value)?],
            GroupReduction::Literals(literals) => literals
                .into_iter()
                .map(|literal| intern_literal(literal, graph, interner))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let first = group[0].0;
        for (index, _) in &group {
            removed[*index] = true;
        }
        replacements.push((first, replacement));
    }

    if replacements.is_empty() {
        return Ok(());
    }
    let mut replacement_at = FxHashMap::default();
    for (index, ids) in replacements {
        replacement_at.insert(index, ids);
    }
    let mut output = Vec::with_capacity(children.len());
    for (index, child) in children.iter().copied().enumerate() {
        if let Some(ids) = replacement_at.remove(&index) {
            output.extend(ids);
        } else if !removed[index] {
            output.push(child);
        }
    }
    *children = output;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum GroupReduction {
    Constant(bool),
    Literals(Vec<FieldLiteral>),
}

fn reduce_group(is_all: bool, literals: &[FieldLiteral], domain: Option<&[u32]>) -> Option<GroupReduction> {
    let equal: BTreeSet<u32> = literals.iter().filter(|literal| literal.equal).map(|literal| literal.value).collect();
    let not_equal: BTreeSet<u32> = literals.iter().filter(|literal| !literal.equal).map(|literal| literal.value).collect();
    let field = literals.first()?.field;
    let reduction = if is_all {
        if equal.len() > 1 || equal.iter().any(|value| not_equal.contains(value)) {
            GroupReduction::Constant(false)
        } else if let Some(value) = equal.first() {
            GroupReduction::Literals(vec![FieldLiteral {
                field,
                value: *value,
                equal: true,
            }])
        } else if let Some(domain) = domain {
            if domain.iter().all(|value| not_equal.contains(value)) {
                GroupReduction::Constant(false)
            } else if domain.iter().filter(|value| !not_equal.contains(value)).count() == 1 {
                GroupReduction::Literals(vec![FieldLiteral {
                    field,
                    value: *domain.iter().find(|value| !not_equal.contains(value)).unwrap(),
                    equal: true,
                }])
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else if not_equal.len() > 1 || equal.iter().any(|value| not_equal.contains(value)) {
        GroupReduction::Constant(true)
    } else if let Some(value) = not_equal.first() {
        GroupReduction::Literals(vec![FieldLiteral {
            field,
            value: *value,
            equal: false,
        }])
    } else if let Some(domain) = domain {
        if domain.iter().all(|value| equal.contains(value)) {
            GroupReduction::Constant(true)
        } else if domain.iter().filter(|value| !equal.contains(value)).count() == 1 {
            GroupReduction::Literals(vec![FieldLiteral {
                field,
                value: *domain.iter().find(|value| !equal.contains(value)).unwrap(),
                equal: false,
            }])
        } else {
            return None;
        }
    } else {
        return None;
    };

    let original_cost: usize = literals.iter().map(literal_cost).sum();
    let distinct_originals: BTreeSet<(u32, bool)> = literals.iter().map(|literal| (literal.value, literal.equal)).collect();
    let replacement_cost = match &reduction {
        GroupReduction::Constant(true) => 1,
        GroupReduction::Constant(false) => 2,
        GroupReduction::Literals(rewritten) => rewritten.iter().map(literal_cost).sum(),
    };
    let replacement_count = match &reduction {
        GroupReduction::Constant(_) => 1,
        GroupReduction::Literals(rewritten) => rewritten.len(),
    };
    if distinct_originals.len() <= replacement_count && original_cost <= replacement_cost {
        None
    } else {
        Some(reduction)
    }
}

fn literal_cost(literal: &FieldLiteral) -> usize {
    if literal.equal { 1 } else { 2 }
}

fn finite_domain(field: StateField) -> Option<&'static [u32]> {
    const BOOLEAN: &[u32] = &[0, 1];
    const AVAILABILITY: &[u32] = &[0, 1, 2, 3, 4];
    match field {
        StateField::Extended(_) | StateField::HeadPresent(_) | StateField::ResourceEnabled(_) => Some(BOOLEAN),
        StateField::Availability(_) => Some(AVAILABILITY),
        StateField::ResourceContact(_) => None,
    }
}

fn field_literal(guard: &Guard, guards: &[Guard], graph: &CompileGraph) -> Option<FieldLiteral> {
    match guard {
        Guard::State(test) => state_literal(*test, graph, true),
        Guard::Not(child) => match &guards[child.index()] {
            Guard::State(test) => state_literal(*test, graph, false),
            _ => None,
        },
        _ => None,
    }
}

fn state_literal(test: StateTest, graph: &CompileGraph, equal: bool) -> Option<FieldLiteral> {
    let (field, value) = match test {
        StateTest::PistonExtended(piston) => (StateField::Extended(piston), 1),
        StateTest::PistonHeadPresent(piston) => (StateField::HeadPresent(piston), 1),
        StateTest::PistonAvailability { piston, is } => (StateField::Availability(piston), availability_value(is)),
        StateTest::ResourceAt { resource, contact } => {
            let slot = graph
                .resources
                .get(resource.index())?
                .contacts
                .iter()
                .position(|candidate| *candidate == contact)?;
            (StateField::ResourceContact(resource), u32::try_from(slot).ok()?)
        }
        StateTest::ResourceEnabled(resource) => (StateField::ResourceEnabled(resource), 1),
    };
    Some(FieldLiteral { field, value, equal })
}

fn availability_value(value: PistonAvailability) -> u32 {
    match value {
        PistonAvailability::Ready => 0,
        PistonAvailability::HeadPending => 1,
        PistonAvailability::PublishingHeadRemoval => 2,
        PistonAvailability::BasePendingHeadPresent => 3,
        PistonAvailability::BasePending => 4,
    }
}

fn intern_literal(literal: FieldLiteral, graph: &CompileGraph, interner: &mut GuardInterner) -> Result<GuardId, String> {
    let test = match literal.field {
        StateField::Extended(piston) => StateTest::PistonExtended(piston),
        StateField::HeadPresent(piston) => StateTest::PistonHeadPresent(piston),
        StateField::Availability(piston) => StateTest::PistonAvailability {
            piston,
            is: availability_from_value(literal.value).ok_or_else(|| "invalid piston availability value".to_owned())?,
        },
        StateField::ResourceEnabled(resource) => StateTest::ResourceEnabled(resource),
        StateField::ResourceContact(resource) => StateTest::ResourceAt {
            resource,
            contact: *graph
                .resources
                .get(resource.index())
                .and_then(|spec| spec.contacts.get(literal.value as usize))
                .ok_or_else(|| "resource contact literal is outside its declared contacts".to_owned())?,
        },
    };
    let state = interner.intern(Guard::State(test))?;
    let positive = match literal.field {
        StateField::Extended(_) | StateField::HeadPresent(_) | StateField::ResourceEnabled(_) => {
            if literal.value > 1 {
                return Err("invalid boolean state value".into());
            }
            literal.equal == (literal.value == 1)
        }
        StateField::Availability(_) | StateField::ResourceContact(_) => literal.equal,
    };
    if positive { Ok(state) } else { interner.intern(Guard::Not(state)) }
}

fn availability_from_value(value: u32) -> Option<PistonAvailability> {
    match value {
        0 => Some(PistonAvailability::Ready),
        1 => Some(PistonAvailability::HeadPending),
        2 => Some(PistonAvailability::PublishingHeadRemoval),
        3 => Some(PistonAvailability::BasePendingHeadPresent),
        4 => Some(PistonAvailability::BasePending),
        _ => None,
    }
}

fn callback_delivery_rewrites(
    graph: &CompileGraph,
    old_to_new: &[GuardId],
    interner: &mut GuardInterner,
    monitor: &TaskMonitor,
) -> Result<FxHashMap<CallbackLinkKey, GuardId>, String> {
    let mut witnesses = FxHashMap::<CallbackLinkKey, Vec<GuardId>>::default();
    for (route_index, route) in graph.routes.iter().enumerate() {
        let route_id =
            crate::redpiler::program_graph::RouteId::try_from_index(route_index).ok_or_else(|| "route table exceeds typed ID range".to_owned())?;
        for delivery in route.deliveries.iter() {
            if monitor.cancelled() {
                return Err("cancelled while preserving callback guard witnesses".into());
            }
            let Some(receiver) = delivery.receiver else { continue };
            let key = delivery_key(route_id, route.source, *delivery, receiver, graph);
            let found = callback_witnesses(graph, *delivery, receiver);
            if found.is_empty() {
                continue;
            }
            let existing = witnesses.entry(key).or_default();
            for witness in found {
                if !existing.contains(&witness) {
                    existing.push(witness);
                }
            }
        }
    }

    let mut rewritten = FxHashMap::default();
    for (route_index, route) in graph.routes.iter().enumerate() {
        let route_id =
            crate::redpiler::program_graph::RouteId::try_from_index(route_index).ok_or_else(|| "route table exceeds typed ID range".to_owned())?;
        for delivery in route.deliveries.iter() {
            let Some(receiver) = delivery.receiver else { continue };
            let key = delivery_key(route_id, route.source, *delivery, receiver, graph);
            if rewritten.contains_key(&key) {
                continue;
            }
            let Some(required) = witnesses.get(&key) else { continue };
            let mut root = old_to_new[delivery.guard.index()];
            let mut retained = vec![root];
            for old_witness in required {
                let witness = old_to_new[old_witness.index()];
                if !guard_implies(&interner.guards, root, witness) && !retained.contains(&witness) {
                    retained.push(witness);
                }
            }
            if retained.len() > 1 {
                root = interner.intern(Guard::All(retained.into_boxed_slice()))?;
            }
            // Every other consumer already uses this ordinary remap as its fallback.
            if root != old_to_new[delivery.guard.index()] {
                rewritten.insert(key, root);
            }
        }
    }
    Ok(rewritten)
}

fn callback_witnesses(graph: &CompileGraph, delivery: RouteDelivery, receiver: CallbackReceiver) -> Vec<GuardId> {
    let SiteSource::Contact(contact) = graph.sites[delivery.target.index()].source else {
        return Vec::new();
    };
    let variants = &graph.contacts[contact.index()].variants;
    match delivery.view {
        OperandView::Live => variants
            .iter()
            .filter(|variant| match delivery.kind {
                DeliveryKind::Shape { .. } => true,
                DeliveryKind::Update { .. } => variant.receiver == Some(receiver),
            })
            .find(|variant| graph.guards[variant.when.index()] == Guard::Always || guard_implies(&graph.guards, delivery.guard, variant.when))
            .filter(|variant| graph.guards[variant.when.index()] != Guard::Always)
            .map(|variant| vec![variant.when])
            .unwrap_or_default(),
        OperandView::Captured => variants
            .iter()
            .filter(|variant| match delivery.kind {
                DeliveryKind::Shape { .. } => true,
                DeliveryKind::Update { .. } => variant.receiver == Some(receiver),
            })
            .find_map(|variant| find_site_profile_guard(&graph.guards, delivery.guard, delivery.target, variant.profile).map(|id| vec![id]))
            .unwrap_or_default(),
    }
}

fn find_site_profile_guard(
    guards: &[Guard],
    root: GuardId,
    site: crate::redpiler::program_graph::SiteId,
    profile: crate::redpiler::program_graph::ProfileId,
) -> Option<GuardId> {
    match &guards[root.index()] {
        Guard::Site {
            site: found_site,
            view: OperandView::Captured,
            test: SiteTest::Profile(found_profile),
        } if *found_site == site && *found_profile == profile => Some(root),
        Guard::All(children) => children.iter().find_map(|child| find_site_profile_guard(guards, *child, site, profile)),
        Guard::Any(children) if !children.is_empty() => {
            let mut found = None;
            for child in children.iter() {
                let witness = find_site_profile_guard(guards, *child, site, profile)?;
                found.get_or_insert(witness);
            }
            found
        }
        Guard::Always | Guard::State(_) | Guard::Site { .. } | Guard::Not(_) | Guard::Any(_) => None,
    }
}

fn guard_implies(guards: &[Guard], root: GuardId, expected: GuardId) -> bool {
    if root == expected {
        return true;
    }
    match &guards[root.index()] {
        Guard::All(children) => children.iter().any(|child| guard_implies(guards, *child, expected)),
        Guard::Any(children) => !children.is_empty() && children.iter().all(|child| guard_implies(guards, *child, expected)),
        _ => false,
    }
}

fn delivery_key(
    route: crate::redpiler::program_graph::RouteId,
    from: crate::redpiler::program_graph::NodeId,
    delivery: RouteDelivery,
    receiver: CallbackReceiver,
    graph: &CompileGraph,
) -> CallbackLinkKey {
    let to = match receiver {
        CallbackReceiver::Component(node) => node,
        CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => graph.pistons[piston.index()].node,
        CallbackReceiver::Contact(contact) => graph.contacts[contact.index()].node,
    };
    (
        CallbackEdgeRef {
            route,
            sequence: delivery.sequence,
        },
        from,
        to,
        delivery.link_kind(),
    )
}

struct ReferenceRewrite {
    contacts: Vec<Vec<GuardId>>,
    comparators: Vec<Option<GuardId>>,
    relations: Vec<GuardId>,
    power_links: Vec<GuardId>,
    links: Vec<GuardId>,
    deliveries: Vec<Vec<GuardId>>,
}

fn stage_references(
    graph: &CompileGraph,
    old_to_new: &[GuardId],
    callback_guards: &FxHashMap<CallbackLinkKey, GuardId>,
    monitor: &TaskMonitor,
) -> Result<ReferenceRewrite, String> {
    let mut contacts = Vec::with_capacity(graph.contacts.len());
    for contact in &graph.contacts {
        if monitor.cancelled() {
            return Err("cancelled while staging contact guard references".into());
        }
        contacts.push(contact.variants.iter().map(|variant| old_to_new[variant.when.index()]).collect());
    }
    let comparators = graph
        .comparator_inputs
        .iter()
        .map(|input| input.far_override.map(|(_, _, guard)| old_to_new[guard.index()]))
        .collect();
    let relations = graph.wire_relations.iter().map(|relation| old_to_new[relation.guard.index()]).collect();
    let power_links = graph.power_links.iter().map(|link| old_to_new[link.guard.index()]).collect();
    let links = graph
        .links
        .iter()
        .map(|link| {
            let key = (link.callback, link.from, link.to, link.kind);
            callback_guards
                .get(&key)
                .copied()
                .or_else(|| {
                    let guard = match link.kind {
                        LinkKind::Update { guard, .. } | LinkKind::Shape { guard, .. } => guard,
                    };
                    Some(old_to_new[guard.index()])
                })
                .ok_or_else(|| "callback link has no corresponding rewritten delivery".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut deliveries = Vec::with_capacity(graph.routes.len());
    for (route_index, route) in graph.routes.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while staging route guard references".into());
        }
        let route_id =
            crate::redpiler::program_graph::RouteId::try_from_index(route_index).ok_or_else(|| "route table exceeds typed ID range".to_owned())?;
        deliveries.push(
            route
                .deliveries
                .iter()
                .map(|delivery| {
                    if let Some(receiver) = delivery.receiver {
                        let key = delivery_key(route_id, route.source, *delivery, receiver, graph);
                        if let Some(guard) = callback_guards.get(&key) {
                            return *guard;
                        }
                    }
                    old_to_new[delivery.guard.index()]
                })
                .collect(),
        );
    }
    Ok(ReferenceRewrite {
        contacts,
        comparators,
        relations,
        power_links,
        links,
        deliveries,
    })
}

impl ReferenceRewrite {
    fn roots(&self) -> Vec<GuardId> {
        self.contacts
            .iter()
            .flatten()
            .chain(self.comparators.iter().flatten())
            .chain(self.relations.iter())
            .chain(self.power_links.iter())
            .chain(self.links.iter())
            .chain(self.deliveries.iter().flatten())
            .copied()
            .collect()
    }

    fn remap(&mut self, map: &[GuardId], monitor: &TaskMonitor) -> Result<(), String> {
        let mut work = 0usize;
        let remap = |id: &mut GuardId, work: &mut usize| -> Result<(), String> {
            *work += 1;
            if *work % 256 == 0 && monitor.cancelled() {
                return Err("cancelled while compacting guard references".into());
            }
            *id = *map
                .get(id.index())
                .ok_or_else(|| "guard reference was not reachable during compaction".to_owned())?;
            Ok(())
        };
        for id in self.contacts.iter_mut().flatten() {
            remap(id, &mut work)?;
        }
        for id in self.comparators.iter_mut().flatten() {
            remap(id, &mut work)?;
        }
        for id in &mut self.relations {
            remap(id, &mut work)?;
        }
        for id in &mut self.power_links {
            remap(id, &mut work)?;
        }
        for id in &mut self.links {
            remap(id, &mut work)?;
        }
        for id in self.deliveries.iter_mut().flatten() {
            remap(id, &mut work)?;
        }
        Ok(())
    }

    fn apply(self, graph: &mut CompileGraph, guards: Vec<Guard>) {
        for (contact, ids) in graph.contacts.iter_mut().zip(self.contacts) {
            for (variant, id) in contact.variants.iter_mut().zip(ids) {
                variant.when = id;
            }
        }
        for (input, id) in graph.comparator_inputs.iter_mut().zip(self.comparators) {
            if let (Some((site, value, _)), Some(id)) = (input.far_override, id) {
                input.far_override = Some((site, value, id));
            }
        }
        for (relation, id) in graph.wire_relations.iter_mut().zip(self.relations) {
            relation.guard = id;
        }
        for (link, id) in graph.power_links.iter_mut().zip(self.power_links) {
            link.guard = id;
        }
        for (link, id) in graph.links.iter_mut().zip(self.links) {
            link.kind = match link.kind {
                LinkKind::Update { direction, .. } => LinkKind::Update { direction, guard: id },
                LinkKind::Shape { direction, .. } => LinkKind::Shape { direction, guard: id },
            };
        }
        for (route, ids) in graph.routes.iter_mut().zip(self.deliveries) {
            for (delivery, id) in route.deliveries.iter_mut().zip(ids) {
                delivery.guard = id;
            }
        }
        graph.guards = guards;
    }
}

fn compact_guards(guards: &[Guard], roots: &[GuardId], monitor: &TaskMonitor) -> Result<(Vec<Guard>, Vec<GuardId>), String> {
    let mut used = vec![false; guards.len()];
    if !used.is_empty() {
        used[0] = true;
    }
    let mut pending = roots.to_vec();
    let mut visited = 0usize;
    while let Some(id) = pending.pop() {
        if id.index() >= guards.len() {
            return Err("guard reference exceeds rewritten table during compaction".into());
        }
        if used[id.index()] {
            continue;
        }
        used[id.index()] = true;
        pending.extend(guard_children(&guards[id.index()]).iter().copied());
        visited += 1;
        if visited % 256 == 0 && monitor.cancelled() {
            return Err("cancelled while compacting guards".into());
        }
    }
    let mut mapping = vec![GuardId::new(0); guards.len()];
    let mut count = 0u32;
    for (index, is_used) in used.iter().copied().enumerate() {
        if is_used {
            mapping[index] = GuardId::new(count);
            count = count
                .checked_add(1)
                .ok_or_else(|| "compacted guard table exceeds typed ID range".to_owned())?;
        }
    }
    let mut compacted = Vec::with_capacity(count as usize);
    for (index, is_used) in used.into_iter().enumerate() {
        if !is_used {
            continue;
        }
        if compacted.len() % 256 == 0 && monitor.cancelled() {
            return Err("cancelled while building compact guard table".into());
        }
        compacted.push(remap_guard(&guards[index], &mapping));
    }
    if compacted.first() != Some(&Guard::Always) {
        return Err("guard 0 did not remain the shared Always predicate after compaction".into());
    }
    Ok((compacted, mapping))
}

fn remap_guard(guard: &Guard, map: &[GuardId]) -> Guard {
    match guard {
        Guard::Always => Guard::Always,
        Guard::State(test) => Guard::State(*test),
        Guard::Site { site, view, test } => Guard::Site {
            site: *site,
            view: *view,
            test: *test,
        },
        Guard::Not(child) => Guard::Not(map[child.index()]),
        Guard::All(children) => Guard::All(children.iter().map(|child| map[child.index()]).collect()),
        Guard::Any(children) => Guard::Any(children.iter().map(|child| map[child.index()]).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_literal_encoding_preserves_both_values_and_polarities() {
        let graph = test_graph();
        for field in [
            StateField::Extended(PistonId::new(0)),
            StateField::HeadPresent(PistonId::new(0)),
            StateField::ResourceEnabled(ResourceId::new(0)),
        ] {
            for value in [0, 1] {
                for equal in [false, true] {
                    let mut interner = GuardInterner::new();
                    let id = intern_literal(FieldLiteral { field, value, equal }, &graph, &mut interner).unwrap();
                    for actual in [0, 1] {
                        let matched = match &interner.guards[id.index()] {
                            Guard::State(_) => actual == 1,
                            Guard::Not(child) => {
                                assert!(matches!(interner.guards[child.index()], Guard::State(_)));
                                actual == 0
                            }
                            guard => panic!("unexpected boolean literal: {guard:?}"),
                        };
                        assert_eq!(matched, (actual == value) == equal);
                    }
                }
            }
        }
    }

    #[test]
    fn finite_domain_reductions_match_exhaustive_boolean_and_availability_values() {
        for (field, domain, literal_kinds) in [
            (StateField::Extended(PistonId::new(3)), &[0, 1][..], 4usize),
            (StateField::HeadPresent(PistonId::new(11)), &[0, 1][..], 4usize),
            (StateField::ResourceEnabled(ResourceId::new(2)), &[0, 1][..], 4usize),
            (StateField::Availability(PistonId::new(7)), &[0, 1, 2, 3, 4][..], 10usize),
        ] {
            for mask in 0..(1usize << literal_kinds) {
                let literals: Vec<_> = (0..literal_kinds)
                    .filter(|index| mask & (1 << index) != 0)
                    .map(|index| FieldLiteral {
                        field,
                        value: (index / 2) as u32,
                        equal: index % 2 == 0,
                    })
                    .collect();
                if literals.len() < 2 {
                    continue;
                }
                for is_all in [false, true] {
                    let reduced = reduce_group(is_all, &literals, Some(domain));
                    for value in domain {
                        let original = evaluate_literals(&literals, *value, is_all);
                        let rewritten = match &reduced {
                            None => original,
                            Some(GroupReduction::Constant(value)) => *value,
                            Some(GroupReduction::Literals(literals)) => evaluate_literals(literals, *value, is_all),
                        };
                        assert_eq!(rewritten, original, "field={field:?} literals={literals:?} value={value} all={is_all}");
                    }
                }
            }
        }
    }

    #[test]
    fn resource_equality_conflicts_are_safe_without_domain_exhaustiveness() {
        let field = StateField::ResourceContact(ResourceId::new(4));
        for size in [1u32, 2, 3, 64, 65] {
            let one = [FieldLiteral {
                field,
                value: 0,
                equal: true,
            }];
            assert_eq!(
                reduce_group(false, &one, None),
                None,
                "a size-{size} resource domain is not assumed exhaustive"
            );
            if size == 1 {
                assert_eq!(reduce_group(true, &one, None), None, "the only resource contact is not folded to true");
                continue;
            }
            let literals = vec![
                FieldLiteral {
                    field,
                    value: 0,
                    equal: true,
                },
                FieldLiteral {
                    field,
                    value: size - 1,
                    equal: true,
                },
            ];
            assert_eq!(reduce_group(true, &literals, None), Some(GroupReduction::Constant(false)));
        }
        let exclusions = [
            FieldLiteral {
                field,
                value: 0,
                equal: false,
            },
            FieldLiteral {
                field,
                value: 1,
                equal: false,
            },
        ];
        assert_eq!(reduce_group(false, &exclusions, None), Some(GroupReduction::Constant(true)));
    }

    #[test]
    fn groups_over_budget_and_literals_separated_by_another_join_are_retained() {
        use crate::redpiler::program_graph::{DeliveryKind, OperandView, RouteDelivery, SiteId};

        let mut graph = test_graph();
        let mut children = Vec::with_capacity(MAX_DIRECT_CHILDREN + 1);
        for index in 0..MAX_DIRECT_CHILDREN - 1 {
            let piston = PistonId::new(index as u32 + 1);
            let guard = GuardId::new(graph.guards.len() as u32);
            graph.guards.push(Guard::State(StateTest::PistonAvailability {
                piston,
                is: PistonAvailability::Ready,
            }));
            children.push(guard);
        }
        let first = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::State(StateTest::PistonAvailability {
            piston: PistonId::new(0),
            is: PistonAvailability::Ready,
        }));
        let second = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::State(StateTest::PistonAvailability {
            piston: PistonId::new(0),
            is: PistonAvailability::BasePending,
        }));
        children.extend([first, second]);
        let root = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Any(children.into_boxed_slice()));
        graph.routes[0].deliveries = vec![RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard: root,
            view: OperandView::Live,
            sequence: 8,
            receiver: None,
        }]
        .into_boxed_slice();
        run_pass(&mut graph);
        let root = graph.routes[0].deliveries[0].guard;
        assert!(matches!(graph.guards[root.index()], Guard::Any(ref children) if children.len() == MAX_DIRECT_CHILDREN + 1));

        let mut nested = test_graph();
        nested.guards.extend([
            Guard::State(StateTest::PistonAvailability {
                piston: PistonId::new(0),
                is: PistonAvailability::Ready,
            }),
            Guard::State(StateTest::PistonAvailability {
                piston: PistonId::new(0),
                is: PistonAvailability::BasePending,
            }),
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
            Guard::Any(vec![GuardId::new(2), GuardId::new(3)].into_boxed_slice()),
            Guard::All(vec![GuardId::new(1), GuardId::new(4)].into_boxed_slice()),
        ]);
        nested.routes[0].deliveries = vec![RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard: GuardId::new(5),
            view: OperandView::Live,
            sequence: 9,
            receiver: None,
        }]
        .into_boxed_slice();
        run_pass(&mut nested);
        let Guard::All(root_children) = &nested.guards[nested.routes[0].deliveries[0].guard.index()] else {
            panic!("the outer join is retained");
        };
        assert!(matches!(nested.guards[root_children[1].index()], Guard::Any(ref children) if children.len() == 2));
    }

    #[test]
    fn callback_witnesses_respect_live_and_captured_views() {
        use crate::redpiler::program_graph::{ContactSpec, ContactVariant, ProfileId, RouteDelivery, SiteId};
        use mchprs_blocks::BlockPos;

        let mut graph = test_graph();
        let state = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::State(StateTest::PistonExtended(PistonId::new(0))));
        let profile = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: OperandView::Captured,
            test: SiteTest::Profile(ProfileId::new(0)),
        });
        graph.contacts.push(ContactSpec {
            node: crate::redpiler::program_graph::NodeId::new(0),
            position: BlockPos::new(0, 0, 0),
            variants: vec![ContactVariant {
                when: state,
                profile: ProfileId::new(0),
                receiver: Some(CallbackReceiver::Component(crate::redpiler::program_graph::NodeId::new(0))),
            }]
            .into_boxed_slice(),
        });
        graph.sites[0].source = SiteSource::Contact(crate::redpiler::program_graph::ContactId::new(0));
        let receiver = CallbackReceiver::Component(crate::redpiler::program_graph::NodeId::new(0));
        let live = RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard: state,
            view: OperandView::Live,
            sequence: 1,
            receiver: Some(receiver),
        };
        let captured = RouteDelivery {
            guard: profile,
            view: OperandView::Captured,
            ..live
        };
        assert_eq!(callback_witnesses(&graph, live, receiver), [state]);
        assert_eq!(callback_witnesses(&graph, captured, receiver), [profile]);
        assert!(
            callback_witnesses(
                &graph,
                RouteDelivery {
                    view: OperandView::Live,
                    ..captured
                },
                receiver
            )
            .is_empty()
        );
        assert!(callback_witnesses(&graph, RouteDelivery { guard: state, ..captured }, receiver).is_empty());

        let second_profile = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: OperandView::Captured,
            test: SiteTest::Profile(ProfileId::new(1)),
        });
        let negated_profile = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Not(profile));
        let combined = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::All(vec![negated_profile, second_profile].into_boxed_slice()));
        graph.contacts[0].variants = vec![
            ContactVariant {
                when: state,
                profile: ProfileId::new(0),
                receiver: Some(receiver),
            },
            ContactVariant {
                when: state,
                profile: ProfileId::new(1),
                receiver: Some(receiver),
            },
        ]
        .into_boxed_slice();
        assert_eq!(find_site_profile_guard(&graph.guards, combined, SiteId::new(0), ProfileId::new(0)), None);
        assert_eq!(
            callback_witnesses(&graph, RouteDelivery { guard: combined, ..captured }, receiver),
            [second_profile],
            "a profile beneath Not is not a positive captured witness"
        );
    }

    #[test]
    fn field_identity_includes_owner_and_literals_map_resource_contacts_by_declared_order() {
        let mut graph = test_graph();
        graph.resources.push(crate::redpiler::program_graph::ResourceSpec {
            material: crate::redpiler::program_graph::ProfileId::new(0),
            contacts: vec![
                crate::redpiler::program_graph::ContactId::new(17),
                crate::redpiler::program_graph::ContactId::new(3),
            ]
            .into_boxed_slice(),
        });
        let a = field_literal(
            &Guard::State(StateTest::ResourceAt {
                resource: ResourceId::new(0),
                contact: crate::redpiler::program_graph::ContactId::new(17),
            }),
            &graph.guards,
            &graph,
        )
        .unwrap();
        let b = field_literal(
            &Guard::State(StateTest::ResourceAt {
                resource: ResourceId::new(0),
                contact: crate::redpiler::program_graph::ContactId::new(3),
            }),
            &graph.guards,
            &graph,
        )
        .unwrap();
        assert_eq!(a.value, 0);
        assert_eq!(b.value, 1);
        assert_ne!(
            a.field,
            FieldLiteral {
                field: StateField::ResourceContact(ResourceId::new(1)),
                ..a
            }
            .field
        );
    }

    #[test]
    fn pass_remaps_all_guard_consumers_compacts_and_is_idempotent() {
        use crate::redpiler::program_graph::{
            CallbackEdgeRef, CallbackReceiver, ComparatorInputPlan, ComparatorOverride, ContactSpec, ContactVariant, DeliveryKind, LinkSpec, NodeId,
            OperandView, PowerLink, PowerPort, PowerTarget, ProfileId, RouteDelivery, RouteId, SiteId, WireRelation, WireRelationKind,
        };
        use mchprs_blocks::BlockFace;

        let mut graph = test_graph();
        graph.guards.push(Guard::State(StateTest::PistonExtended(PistonId::new(0))));
        graph.guards.push(Guard::Not(GuardId::new(1)));
        graph.guards.push(Guard::All(vec![GuardId::new(1), GuardId::new(2)].into_boxed_slice()));
        graph.guards.push(Guard::State(StateTest::PistonHeadPresent(PistonId::new(0)))); // unreachable
        let false_guard = GuardId::new(3);
        graph.contacts.push(ContactSpec {
            node: NodeId::new(0),
            position: mchprs_blocks::BlockPos::new(0, 0, 0),
            variants: vec![ContactVariant {
                when: GuardId::new(1),
                profile: ProfileId::new(0),
                receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
            }]
            .into_boxed_slice(),
        });
        graph.sites[0].source = SiteSource::Contact(crate::redpiler::program_graph::ContactId::new(0));
        graph.comparator_inputs.push(ComparatorInputPlan {
            node: NodeId::new(0),
            rear: SiteId::new(0),
            direct_override: None,
            far_override: Some((SiteId::new(0), ComparatorOverride::Literal(0), false_guard)),
            far_threshold: 0,
        });
        graph.wire_relations.push(WireRelation {
            from: NodeId::new(0),
            to: SiteId::new(0),
            kind: WireRelationKind::Step,
            direction: BlockFace::Top,
            guard: false_guard,
        });
        graph.power_links.push(PowerLink {
            from: SiteId::new(0),
            from_port: PowerPort::Stored,
            to: PowerTarget::Conductor {
                site: SiteId::new(0),
                include_dust: false,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: false_guard,
        });
        graph.routes[0].deliveries = vec![RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard: false_guard,
            view: OperandView::Live,
            sequence: 5,
            receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
        }]
        .into_boxed_slice();
        graph.links.push(LinkSpec {
            from: NodeId::new(0),
            to: NodeId::new(0),
            kind: LinkKind::Update {
                direction: None,
                guard: false_guard,
            },
            callback: CallbackEdgeRef {
                route: RouteId::new(0),
                sequence: 5,
            },
        });

        run_pass(&mut graph);
        assert_eq!(graph.guards[0], Guard::Always);
        assert_eq!(graph.guards[2], Guard::Not(GuardId::new(0)));
        assert!(matches!(graph.guards[3], Guard::All(ref children) if children.as_ref() == [GuardId::new(2), GuardId::new(1)]));
        assert_eq!(graph.contacts[0].variants[0].when, GuardId::new(1));
        assert_eq!(graph.comparator_inputs[0].far_override.unwrap().2, GuardId::new(2));
        assert_eq!(graph.wire_relations[0].guard, GuardId::new(2));
        assert_eq!(graph.power_links[0].guard, GuardId::new(2));
        assert_eq!(
            graph.links[0].kind,
            LinkKind::Update {
                direction: None,
                guard: GuardId::new(3)
            }
        );
        assert_eq!(graph.routes[0].deliveries[0].guard, GuardId::new(3));
        assert!(guard_implies(
            &graph.guards,
            graph.routes[0].deliveries[0].guard,
            graph.contacts[0].variants[0].when
        ));
        assert_eq!(graph.routes[0].deliveries[0].sequence, 5);
        let rewritten = graph.clone();
        run_pass(&mut graph);
        assert_eq!(graph, rewritten);
    }

    #[test]
    fn ordinary_callback_remaps_need_no_exception_entries() {
        use crate::redpiler::program_graph::{DeliveryKind, LinkSpec, NodeId, RouteId, SiteId};
        use mchprs_blocks::BlockFace;

        let mut graph = test_graph();
        graph.guards.extend([Guard::Always, Guard::Not(GuardId::new(0))]);
        let mut interner = GuardInterner::new();
        let never = interner.constant(false).unwrap();
        let mapping = [GuardId::new(0), GuardId::new(0), never];
        let deliveries: Vec<_> = (0..128)
            .map(|sequence| RouteDelivery {
                target: SiteId::new(0),
                kind: DeliveryKind::Update {
                    direction: Some(if sequence % 2 == 0 { BlockFace::North } else { BlockFace::South }),
                },
                guard: GuardId::new(if sequence % 3 == 0 { 2 } else { 1 }),
                view: OperandView::Live,
                sequence,
                receiver: Some(CallbackReceiver::Component(NodeId::new(0))),
            })
            .collect();
        graph.links = deliveries
            .iter()
            .map(|delivery| LinkSpec {
                from: NodeId::new(0),
                to: NodeId::new(0),
                callback: CallbackEdgeRef {
                    route: RouteId::new(0),
                    sequence: delivery.sequence,
                },
                kind: delivery.link_kind(),
            })
            .collect();
        graph.routes[0].deliveries = deliveries.clone().into_boxed_slice();
        let monitor = TaskMonitor::default();
        let exceptions = callback_delivery_rewrites(&graph, &mapping, &mut interner, &monitor).unwrap();
        assert!(exceptions.is_empty(), "ordinary remaps must stay out of the exception map");

        let staged = stage_references(&graph, &mapping, &exceptions, &monitor).unwrap();
        let expected: Vec<_> = deliveries.iter().map(|delivery| mapping[delivery.guard.index()]).collect();
        assert_eq!(staged.links, expected);
        assert_eq!(staged.deliveries[0], expected);
        staged.apply(&mut graph, interner.guards);
        for (actual, original) in graph.routes[0].deliveries.iter().zip(deliveries) {
            assert_eq!(
                *actual,
                RouteDelivery {
                    guard: mapping[original.guard.index()],
                    ..original
                }
            );
        }
    }

    #[test]
    fn false_callback_guard_keeps_its_validation_witness() {
        let mut interner = GuardInterner::new();
        let witness = interner.intern(Guard::State(StateTest::PistonExtended(PistonId::new(0)))).unwrap();
        let never = interner.constant(false).unwrap();
        let root = interner.intern(Guard::All(vec![never, witness].into_boxed_slice())).unwrap();
        assert!(guard_implies(&interner.guards, root, witness));
        assert!(matches!(interner.guards[root.index()], Guard::All(ref children) if children.as_ref() == [never, witness]));
    }

    #[test]
    fn cancellation_before_apply_leaves_graph_unchanged() {
        let mut graph = test_graph();
        let before = graph.clone();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        let options = crate::redpiler::CompilerOptions::default();
        let entry = crate::redpiler::program_graph::EntryState {
            logical_tick: 0,
            pending_ticks: Vec::new(),
        };
        let context = PassContext::new(&monitor, &options, &entry);
        assert!(SimplifyStateGuards.run(&mut graph, &context).is_err());
        assert_eq!(graph, before);
    }

    fn evaluate_literals(literals: &[FieldLiteral], value: u32, is_all: bool) -> bool {
        let mut result = is_all;
        for literal in literals {
            let test = literal.equal == (literal.value == value);
            result = if is_all { result && test } else { result || test };
        }
        result
    }

    fn run_pass(graph: &mut CompileGraph) {
        let monitor = TaskMonitor::default();
        let options = crate::redpiler::CompilerOptions::default();
        let entry = crate::redpiler::program_graph::EntryState {
            logical_tick: 0,
            pending_ticks: Vec::new(),
        };
        let context = PassContext::new(&monitor, &options, &entry);
        SimplifyStateGuards.run(graph, &context).unwrap();
    }

    fn test_graph() -> CompileGraph {
        use crate::redpiler::components::{NodeState, NodeType};
        use crate::redpiler::program_graph::{CircuitNodeDraft, ComponentState, Route, SiteSpec};
        use mchprs_blocks::BlockPos;
        use mchprs_blocks::blocks::Block;

        let position = BlockPos::new(0, 0, 0);
        CompileGraph {
            nodes: vec![CircuitNodeDraft {
                position: Some(position),
                raw_state: Some(Block::RedstoneBlock.get_id()),
                kind: crate::redpiler::program_graph::CircuitNodeKind::Electrical(NodeType::Constant),
                state: ComponentState::Electrical(NodeState::simple(false)),
                output_route: Some(crate::redpiler::program_graph::RouteId::new(0)),
            }],
            links: Vec::new(),
            power_links: Vec::new(),
            comparator_inputs: Vec::new(),
            profiles: Vec::new(),
            guards: vec![Guard::Always],
            contacts: Vec::new(),
            routes: vec![Route {
                source: crate::redpiler::program_graph::NodeId::new(0),
                deliveries: Vec::new().into_boxed_slice(),
            }],

            wires: Vec::new(),

            wire_relations: Vec::new(),
            sites: vec![SiteSpec {
                position,
                source: SiteSource::Component(crate::redpiler::program_graph::NodeId::new(0)),
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
