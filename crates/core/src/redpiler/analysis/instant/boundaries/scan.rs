//! Power, callback, guard, geometry, contact, comparator, and fixed-read boundary scans.
use super::{BoundaryScan, Budget, OwnerMap, ScannedPort};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// Reuse the same typed dependency scan for legacy region projections and the new catalog.
pub(crate) fn scan_boundaries(graph: &CompileGraph, owners: &OwnerMap, monitor: &TaskMonitor) -> Result<BoundaryScan, String> {
    let mut budget = Budget::bounded(monitor);
    let mut guards = GuardSupport::default();
    let mut conducted_sites = FxHashSet::default();
    for link in &graph.power_links {
        budget.visit()?;
        if let PowerTarget::Conductor { site, .. } = link.to
            && !conducted_sites.contains(&site)
        {
            budget.retain(&site)?;
            conducted_sites.insert(site);
        }
    }
    let mut result = scan_common(graph, owners, &mut budget, Some(&mut guards), Some(&conducted_sites))?;
    let mut unique: FxHashSet<_> = result.ports.iter().map(|port| (port.owner, port.direction, port.kind)).collect();
    scan_catalog_wire_reads(graph, owners, &mut guards, &mut result, &mut unique, &mut budget, &conducted_sites)?;
    scan_comparators(graph, owners, &mut guards, &mut result, &mut unique, &mut budget)?;
    scan_fixed_reads(graph, owners, &mut result, &mut unique, &mut budget)?;
    scan_contacts(graph, owners, &mut guards, &mut result, &mut unique, &mut budget)?;
    budget.check_cancelled()?;
    Ok(result)
}

/// Legacy O3 consumes only its historical power/callback/wire-neighbor/observer rows.
pub(crate) fn scan_legacy_boundaries(graph: &CompileGraph, owners: &OwnerMap, monitor: &TaskMonitor) -> Result<BoundaryScan, String> {
    let mut budget = Budget::unbounded(monitor);
    scan_common(graph, owners, &mut budget, None, None)
}

fn scan_common(
    graph: &CompileGraph,
    owners: &OwnerMap,
    budget: &mut Budget<'_>,
    mut guards: Option<&mut GuardSupport>,
    conducted_sites: Option<&FxHashSet<SiteId>>,
) -> Result<BoundaryScan, String> {
    let mut result = BoundaryScan {
        ports: Vec::new(),
        owned_routes: Vec::new(),
    };
    let mut unique = FxHashSet::default();
    for (index, link) in graph.power_links.iter().enumerate() {
        budget.visit()?;
        let source = owners.sites[link.from.index()];
        let target = match link.to {
            PowerTarget::Input { node, .. } => owners.nodes[node.index()],
            PowerTarget::Conductor { site, .. } => owners.sites[site.index()],
        };
        push_transfer(
            &mut result,
            &mut unique,
            budget,
            source,
            target,
            BoundaryPortKind::Power { link: index as u32 },
        )?;
        if let Some(guards) = guards.as_deref_mut() {
            append_guard_support(graph, owners, guards, &mut result, &mut unique, budget, source, link.guard)?;
            append_guard_support(graph, owners, guards, &mut result, &mut unique, budget, target, link.guard)?;
        }
    }
    for (route_index, route) in graph.routes.iter().enumerate() {
        budget.visit()?;
        let source = owners.nodes[route.source.index()];
        if let Some(owner) = source {
            let row = (owner, RouteId::new(route_index as u32));
            budget.retain(&row)?;
            result.owned_routes.push(row);
        }
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            budget.visit()?;
            let target = delivery_owner(graph, owners, delivery);
            let occurrence = RouteOccurrence {
                route: RouteId::new(route_index as u32),
                ordinal: ordinal as u32,
            };
            push_transfer(
                &mut result,
                &mut unique,
                budget,
                source,
                target,
                BoundaryPortKind::Callback { occurrence },
            )?;
            if let Some(guards) = guards.as_deref_mut() {
                append_guard_support(graph, owners, guards, &mut result, &mut unique, budget, source, delivery.guard)?;
                append_guard_support(graph, owners, guards, &mut result, &mut unique, budget, target, delivery.guard)?;
            }
        }
    }
    for wire in &graph.wires {
        budget.visit()?;
        let reader = owners.nodes[wire.node.index()];
        for &neighbor in &wire.canonical_neighbors {
            budget.visit()?;
            if is_static_read_site(graph, owners, neighbor, conducted_sites) {
                continue;
            }
            push_read(
                &mut result,
                &mut unique,
                budget,
                reader,
                owners.sites[neighbor.index()],
                BoundaryPortKind::WireNeighborhood { wire: wire.node, neighbor },
            )?;
        }
    }
    scan_observers(graph, owners, &mut result, &mut unique, budget)?;
    budget.check_cancelled()?;
    Ok(result)
}

fn scan_catalog_wire_reads(
    graph: &CompileGraph,
    owners: &OwnerMap,
    guards: &mut GuardSupport,
    result: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
    conducted_sites: &FxHashSet<SiteId>,
) -> Result<(), String> {
    for (index, relation) in graph.wire_relations.iter().enumerate() {
        budget.visit()?;
        let reader = owners.nodes[relation.from.index()];
        let value = owners.sites[relation.to.index()];
        push_read(
            result,
            unique,
            budget,
            reader,
            value,
            BoundaryPortKind::WireRelation { relation: index as u32 },
        )?;
        append_guard_support(graph, owners, guards, result, unique, budget, reader, relation.guard)?;
        append_guard_support(graph, owners, guards, result, unique, budget, value, relation.guard)?;
    }

    for wire in &graph.wires {
        budget.visit()?;
        let reader = owners.nodes[wire.node.index()];
        if let Some(route_id) = wire.turbo_route {
            for (ordinal, delivery) in graph.routes[route_id.index()].deliveries.iter().enumerate() {
                budget.visit()?;
                if delivery.view != OperandView::Captured {
                    continue;
                }
                if is_static_read_site(graph, owners, delivery.target, Some(conducted_sites)) {
                    continue;
                }
                let occurrence = RouteOccurrence {
                    route: route_id,
                    ordinal: ordinal as u32,
                };
                push_read(
                    result,
                    unique,
                    budget,
                    reader,
                    owners.sites[delivery.target.index()],
                    BoundaryPortKind::CapturedWire {
                        wire: wire.node,
                        occurrence,
                        neighbor: delivery.target,
                    },
                )?;
                append_guard_support(graph, owners, guards, result, unique, budget, reader, delivery.guard)?;
                append_guard_support(
                    graph,
                    owners,
                    guards,
                    result,
                    unique,
                    budget,
                    owners.sites[delivery.target.index()],
                    delivery.guard,
                )?;
            }
        }
    }
    Ok(())
}

fn is_static_read_site(graph: &CompileGraph, owners: &OwnerMap, site: SiteId, conducted_sites: Option<&FxHashSet<SiteId>>) -> bool {
    matches!(graph.sites[site.index()].source, SiteSource::Constant(_))
        && owners.sites[site.index()].is_none()
        && !conducted_sites.is_some_and(|sites| sites.contains(&site))
}

fn scan_contacts(
    graph: &CompileGraph,
    owners: &OwnerMap,
    guards: &mut GuardSupport,
    result: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    let mut cursor = 0;
    let mut visited = FxHashSet::default();
    while cursor < result.ports.len() {
        budget.visit()?;
        let port = result.ports[cursor];
        cursor += 1;
        for contact in port_contacts(graph, port.kind).into_iter().flatten() {
            budget.visit()?;
            let contact_owner = owners.contacts[contact.index()];
            let callback = matches!(port.kind, BoundaryPortKind::Callback { .. });
            let boundary = if callback {
                Some((port.owner, port.direction))
            } else {
                match (contact_owner, port.direction) {
                    (Some(owner), BoundaryDirection::Output) if port.owner == owner => Some((owner, BoundaryDirection::Output)),
                    (Some(owner), BoundaryDirection::Input) if port.owner != owner => Some((port.owner, BoundaryDirection::Input)),
                    (None, BoundaryDirection::Input) => Some((port.owner, BoundaryDirection::Input)),
                    _ => None,
                }
            };
            let Some((owner, direction)) = boundary else { continue };
            let key = (contact, owner, direction);
            if visited.contains(&key) {
                continue;
            }
            budget.retain(&key)?;
            visited.insert(key);
            for (ordinal, variant) in graph.contacts[contact.index()].variants.iter().enumerate() {
                budget.visit()?;
                push_port(
                    result,
                    unique,
                    budget,
                    owner,
                    direction,
                    BoundaryPortKind::ContactAlternative {
                        contact,
                        ordinal: ordinal as u32,
                    },
                )?;
                append_guard_support(graph, owners, guards, result, unique, budget, Some(owner), variant.when)?;
            }
        }
    }

    Ok(())
}

fn port_contacts(graph: &CompileGraph, kind: BoundaryPortKind) -> [Option<ContactId>; 2] {
    let at_site = |site: SiteId| match graph.sites[site.index()].source {
        SiteSource::Contact(contact) => Some(contact),
        _ => None,
    };
    let pair = |first: Option<ContactId>, second: Option<ContactId>| {
        if first == second { [first, None] } else { [first, second] }
    };
    match kind {
        BoundaryPortKind::Power { link } => {
            let link = graph.power_links[link as usize];
            let target = match link.to {
                PowerTarget::Conductor { site, .. } => at_site(site),
                PowerTarget::Input { .. } => None,
            };
            pair(at_site(link.from), target)
        }
        BoundaryPortKind::Callback { occurrence } => {
            let delivery = graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            let receiver = match delivery.receiver {
                Some(CallbackReceiver::Contact(contact)) => Some(contact),
                _ => None,
            };
            pair(receiver, at_site(delivery.target))
        }
        BoundaryPortKind::WireRelation { relation } => pair(at_site(graph.wire_relations[relation as usize].to), None),
        BoundaryPortKind::Comparator { plan, role } => {
            let plan = &graph.comparator_inputs[plan as usize];
            let site = match role {
                ComparatorPortRole::Rear => Some(plan.rear),
                ComparatorPortRole::FarOverrideSite => plan.far_override.map(|(site, _, _)| site),
                _ => None,
            };
            pair(site.and_then(at_site), None)
        }
        BoundaryPortKind::GuardSite { site, .. }
        | BoundaryPortKind::ObserverWatch { site, .. }
        | BoundaryPortKind::CapturedWire { neighbor: site, .. }
        | BoundaryPortKind::WireNeighborhood { neighbor: site, .. }
        | BoundaryPortKind::FixedRead { site, .. } => pair(at_site(site), None),
        BoundaryPortKind::GuardState {
            test: StateTest::ResourceAt { contact, .. },
            ..
        } => pair(Some(contact), None),
        BoundaryPortKind::ContactAlternative { .. } | BoundaryPortKind::GuardState { .. } => [None, None],
    }
}

pub(super) fn delivery_owner(graph: &CompileGraph, owners: &OwnerMap, delivery: &RouteDelivery) -> Option<usize> {
    match delivery.receiver {
        Some(CallbackReceiver::Component(node)) => owners.nodes[node.index()],
        Some(CallbackReceiver::Contact(contact)) => owners.contacts[contact.index()],
        Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => owners.actors[actor.index()],
        None => match graph.sites[delivery.target.index()].source {
            SiteSource::Component(node) => owners.nodes[node.index()],
            SiteSource::Contact(contact) => owners.contacts[contact.index()],
            SiteSource::Constant(_) => None,
        },
    }
}

fn push_transfer(
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
    source: Option<usize>,
    target: Option<usize>,
    kind: BoundaryPortKind,
) -> Result<(), String> {
    if source == target {
        return Ok(());
    }
    if let Some(owner) = source {
        push_port(scan, unique, budget, owner, BoundaryDirection::Output, kind)?;
    }
    if let Some(owner) = target {
        push_port(scan, unique, budget, owner, BoundaryDirection::Input, kind)?;
    }
    Ok(())
}

fn push_read(
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
    reader: Option<usize>,
    source: Option<usize>,
    kind: BoundaryPortKind,
) -> Result<(), String> {
    if reader == source {
        return Ok(());
    }
    if let Some(owner) = source {
        push_port(scan, unique, budget, owner, BoundaryDirection::Output, kind)?;
    }
    if let Some(owner) = reader {
        push_port(scan, unique, budget, owner, BoundaryDirection::Input, kind)?;
    }
    Ok(())
}

fn push_port(
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
    owner: usize,
    direction: BoundaryDirection,
    kind: BoundaryPortKind,
) -> Result<(), String> {
    let key = (owner, direction, kind);
    if unique.contains(&key) {
        return Ok(());
    }
    let row = ScannedPort { owner, direction, kind };
    budget.retain(&key)?;
    budget.retain(&row)?;
    unique.insert(key);
    scan.ports.push(row);
    Ok(())
}

#[derive(Clone, Copy)]
enum GuardLeaf {
    Site {
        /// This original guard atom owns the site-read predicate.
        atom: GuardId,
        /// This site is the electrical or geometry value read by the atom.
        site: SiteId,
        /// This selects the live or captured operand view used by the atom.
        view: OperandView,
        /// This predicate is applied to the selected site value.
        test: SiteTest,
    },
    State {
        /// This original guard atom owns the component-state predicate.
        atom: GuardId,
        /// This is the actor or resource state read by the atom.
        test: StateTest,
    },
}

#[derive(Default)]
struct GuardSupport {
    /// This memoizes bounded iterative leaf collection by original guard ID.
    leaves: FxHashMap<GuardId, Vec<GuardLeaf>>,
}

fn append_guard_support(
    graph: &CompileGraph,
    owners: &OwnerMap,
    guards: &mut GuardSupport,
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
    evaluator: Option<usize>,
    guard: GuardId,
) -> Result<(), String> {
    let leaves = guard_leaves(graph, guards, budget, guard)?;
    for &leaf in leaves {
        budget.visit()?;
        match leaf {
            GuardLeaf::Site { atom, site, view, test } => push_read(
                scan,
                unique,
                budget,
                evaluator,
                owners.sites[site.index()],
                BoundaryPortKind::GuardSite {
                    guard: atom,
                    site,
                    view,
                    test,
                },
            )?,
            GuardLeaf::State { atom, test } => {
                for_each_state_owner(owners, test, |source| {
                    push_read(
                        scan,
                        unique,
                        budget,
                        evaluator,
                        source,
                        BoundaryPortKind::GuardState { guard: atom, test },
                    )
                })?;
            }
        };
    }
    Ok(())
}

fn guard_leaves<'a>(graph: &CompileGraph, cache: &'a mut GuardSupport, budget: &mut Budget<'_>, root: GuardId) -> Result<&'a [GuardLeaf], String> {
    if !cache.leaves.contains_key(&root) {
        budget.retain(&root)?;
        let mut pending = vec![root];
        let mut visited = FxHashSet::default();
        let mut leaves = Vec::new();
        while let Some(id) = pending.pop() {
            budget.visit()?;
            if !visited.insert(id) {
                continue;
            }
            budget.retain(&id)?;
            match graph.guards.get(id.index()).ok_or("guard references missing during boundary scan")? {
                Guard::Always => {}
                Guard::State(test) => {
                    let leaf = GuardLeaf::State { atom: id, test: *test };
                    budget.retain(&leaf)?;
                    leaves.push(leaf);
                }
                Guard::Site { site, view, test } => {
                    let leaf = GuardLeaf::Site {
                        atom: id,
                        site: *site,
                        view: *view,
                        test: *test,
                    };
                    budget.retain(&leaf)?;
                    leaves.push(leaf);
                }
                Guard::Not(child) => {
                    budget.retain(child)?;
                    pending.push(*child);
                }
                Guard::All(children) | Guard::Any(children) => {
                    for child in children.iter().rev() {
                        budget.retain(child)?;
                        pending.push(*child);
                    }
                }
            }
        }
        budget.retain(&root)?;
        budget.retain(&leaves)?;
        cache.leaves.insert(root, leaves);
    }
    Ok(cache.leaves.get(&root).expect("inserted guard support"))
}

fn for_each_state_owner(owners: &OwnerMap, test: StateTest, mut visit: impl FnMut(Option<usize>) -> Result<(), String>) -> Result<(), String> {
    match test {
        StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
            visit(owners.actors[actor.index()])
        }
        StateTest::ResourceEnabled(resource) => visit(owners.resources[resource.index()]),
        StateTest::ResourceAt { resource, contact } => {
            let first = owners.resources[resource.index()];
            let second = owners.contacts[contact.index()];
            visit(first)?;
            if second != first {
                visit(second)?;
            }
            Ok(())
        }
    }
}

fn scan_comparators(
    graph: &CompileGraph,
    owners: &OwnerMap,
    guards: &mut GuardSupport,
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    for (index, plan) in graph.comparator_inputs.iter().enumerate() {
        budget.visit()?;
        let comparator = owners.nodes[plan.node.index()];
        let plan_id = index as u32;
        push_read(
            scan,
            unique,
            budget,
            comparator,
            owners.sites[plan.rear.index()],
            BoundaryPortKind::Comparator {
                plan: plan_id,
                role: ComparatorPortRole::Rear,
            },
        )?;
        if let Some(ComparatorOverride::LiveComponentOutput(source_node)) = plan.direct_override {
            push_transfer(
                scan,
                unique,
                budget,
                owners.nodes[source_node.index()],
                comparator,
                BoundaryPortKind::Comparator {
                    plan: plan_id,
                    role: ComparatorPortRole::DirectOverride,
                },
            )?;
        }
        if let Some((site, override_source, guard)) = plan.far_override {
            push_read(
                scan,
                unique,
                budget,
                comparator,
                owners.sites[site.index()],
                BoundaryPortKind::Comparator {
                    plan: plan_id,
                    role: ComparatorPortRole::FarOverrideSite,
                },
            )?;
            if let ComparatorOverride::LiveComponentOutput(source_node) = override_source {
                push_transfer(
                    scan,
                    unique,
                    budget,
                    owners.nodes[source_node.index()],
                    comparator,
                    BoundaryPortKind::Comparator {
                        plan: plan_id,
                        role: ComparatorPortRole::FarOverrideOutput,
                    },
                )?;
            }
            if let Some(owner) = comparator {
                push_port(
                    scan,
                    unique,
                    budget,
                    owner,
                    BoundaryDirection::Input,
                    BoundaryPortKind::Comparator {
                        plan: plan_id,
                        role: ComparatorPortRole::FarOverrideGuard,
                    },
                )?;
            }
            append_guard_support(graph, owners, guards, scan, unique, budget, comparator, guard)?;
        }
    }
    Ok(())
}

fn scan_observers(
    graph: &CompileGraph,
    owners: &OwnerMap,
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    for (index, node) in graph.nodes.iter().enumerate() {
        budget.visit()?;
        let (CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. }) = node.kind else {
            continue;
        };
        let observer = owners.nodes.get(index).copied().flatten();
        push_read(
            scan,
            unique,
            budget,
            observer,
            owners.sites[watched.index()],
            BoundaryPortKind::ObserverWatch {
                observer: NodeId::new(index as u32),
                site: watched,
            },
        )?;
    }
    Ok(())
}

fn scan_fixed_reads(
    graph: &CompileGraph,
    owners: &OwnerMap,
    scan: &mut BoundaryScan,
    unique: &mut FxHashSet<(usize, BoundaryDirection, BoundaryPortKind)>,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    let mut site_at = FxHashMap::default();
    for (index, site) in graph.sites.iter().enumerate() {
        budget.visit()?;
        let entry = (site.position, SiteId::new(index as u32));
        budget.retain(&entry)?;
        site_at.insert(entry.0, entry.1);
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        budget.visit()?;
        if !matches!(
            node.kind,
            CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::NoteBlock { .. })
        ) {
            continue;
        }
        let Some(position) = node.position else { continue };
        let reader = owners.nodes[index];
        for (offset, role) in [
            (mchprs_blocks::BlockFace::Bottom, FixedReadRole::NoteSupport),
            (mchprs_blocks::BlockFace::Top, FixedReadRole::NoteObstruction),
        ] {
            let Some(&site) = site_at.get(&position.offset(offset)) else { continue };
            push_read(
                scan,
                unique,
                budget,
                reader,
                owners.sites[site.index()],
                BoundaryPortKind::FixedRead {
                    node: NodeId::new(index as u32),
                    role,
                    site,
                },
            )?;
        }
    }
    Ok(())
}
