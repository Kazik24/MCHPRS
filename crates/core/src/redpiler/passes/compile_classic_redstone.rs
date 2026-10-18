//! Certify whole fixed wire regions and replace private propagation with committed signals.
//! One-ingress, delayed-receiver regions are checked against ordered Graph samples for
//! every old/new strength pair; compact masks select receiver scheduling at original
//! route occurrences. Unsupported boundaries and exhausted proof budgets stay general.
//! Runs after rewrites and before instant plans, only with the O2+ classic option.
//! See docs/notes/redpiler-pistons-v4/ALGORITHMS_AND_RECOVERY.md for the algorithm.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
#[cfg(test)]
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) struct CompileClassicRedstone;

impl PlanBuilder for CompileClassicRedstone {
    fn name(&self) -> &'static str {
        "compile-classic-redstone"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let analysis = ClassicAnalysis::new(graph);
        let mut reference = None;
        let mut plans = Vec::new();
        let mut declines = BTreeMap::<String, usize>::new();
        let mut proof_work = 0usize;
        let mut receiver_owners = BTreeSet::new();
        for members in regions(graph) {
            if context.monitor.cancelled() {
                return Err("cancelled during classic region discovery".into());
            }
            let mut plan = match analysis.infer(&members) {
                Ok(plan) => plan,
                Err(reason) => {
                    *declines.entry(reason.into()).or_default() += 1;
                    continue;
                }
            };
            if plan.wires.iter().any(|wire| {
                context
                    .entry
                    .pending_ticks
                    .iter()
                    .any(|tick| graph.nodes[wire.node.index()].position == Some(tick.tick.pos))
            }) {
                *declines.entry("imported private-wire work".into()).or_default() += 1;
                continue;
            }
            if plan.inputs.iter().any(|input| receiver_owners.contains(&input.receiver)) {
                *declines.entry("receiver already owned by another net".into()).or_default() += 1;
                continue;
            }
            if proof_work >= CLASSIC_PROOF_WORK * 16 {
                *declines.entry("total classic proof work budget".into()).or_default() += 1;
                continue;
            }
            if reference.is_none() {
                reference = Some(crate::redpiler::backend::graph::classic::ClassicReference::new(graph)?);
            }
            let reference = reference.as_mut().unwrap();
            let before = reference.work();
            let result = reference.prove(&mut plan, context.monitor);
            proof_work += reference.work() - before;
            match result {
                Ok(_) => {}
                Err(reason) => {
                    if context.monitor.cancelled() {
                        return Err(reason);
                    }
                    *declines.entry(reason).or_default() += 1;
                    continue;
                }
            }
            receiver_owners.extend(plan.inputs.iter().map(|input| input.receiver));
            plans.push(plan);
        }
        let mut tables = vec![
            crate::redpiler::PreparationTableStatistics {
                stage: "classic redstone",
                table: "projected direct tables".into(),
                entries: plans.len(),
                capacity: plans.len(),
                estimated_bytes: ClassicProgram::estimate_plans(&plans, graph.nodes.len(), graph.routes.len()),
            },
            crate::redpiler::PreparationTableStatistics {
                stage: "classic redstone",
                table: "committed signal state".into(),
                entries: plans.len(),
                capacity: plans.len(),
                estimated_bytes: plans.len(),
            },
            table("accepted nets", plans.len()),
            table("private wires", plans.iter().map(|plan| plan.wires.len()).sum()),
            table("direct receiver inputs", plans.iter().map(|plan| plan.inputs.len()).sum()),
            table("entry occurrences", plans.iter().map(|plan| plan.entries.len()).sum()),
            table(
                "retained boundary operations",
                plans.iter().flat_map(|plan| plan.entries.iter()).map(|entry| entry.ops.len()).sum(),
            ),
            table("reference proof work", proof_work),
        ];
        for (reason, count) in declines {
            tables.push(table(format!("declined: {reason}"), count));
        }
        context.monitor.record_preparation_tables(tables);
        Ok(PlanOutput::ClassicNets(plans))
    }
}

fn table(name: impl Into<String>, entries: usize) -> crate::redpiler::PreparationTableStatistics {
    crate::redpiler::PreparationTableStatistics {
        stage: "classic redstone",
        table: name.into(),
        entries,
        capacity: entries,
        estimated_bytes: 0,
    }
}

#[cfg(test)]
#[derive(Debug, Default, serde::Serialize)]
struct Census {
    wires: usize,
    regions: usize,
    largest_region: usize,
    structural_candidates: usize,
    candidate_wires: usize,
    candidate_receivers: usize,
    candidate_lengths: BTreeMap<usize, usize>,
    candidate_details: Vec<serde_json::Value>,
    declines: BTreeMap<String, usize>,
}

/// Undirected potential electrical connectivity; guards do not freeze saved contacts.
fn regions(graph: &CompileGraph) -> Vec<Vec<NodeId>> {
    let mut adjacency = vec![Vec::new(); graph.nodes.len()];
    for relation in &graph.wire_relations {
        if let SiteSource::Component(other) = graph.sites[relation.to.index()].source
            && matches!(graph.nodes[other.index()].kind, CircuitNodeKind::Wire(_))
        {
            adjacency[relation.from.index()].push(other);
            adjacency[other.index()].push(relation.from);
        }
    }
    let mut seen = vec![false; graph.nodes.len()];
    let mut groups = Vec::new();
    for wire in &graph.wires {
        if seen[wire.node.index()] {
            continue;
        }
        seen[wire.node.index()] = true;
        let mut queue = VecDeque::from([wire.node]);
        let mut group = Vec::new();
        while let Some(node) = queue.pop_front() {
            group.push(node);
            for &other in &adjacency[node.index()] {
                if !seen[other.index()] {
                    seen[other.index()] = true;
                    queue.push_back(other);
                }
            }
        }
        group.sort();
        groups.push(group);
    }
    groups
}

#[cfg(test)]
fn census(graph: &CompileGraph) -> Census {
    let groups = regions(graph);
    let mut report = Census {
        wires: graph.wires.len(),
        regions: groups.len(),
        ..Default::default()
    };
    // Resolve conducted dependencies to their actual variable component leaves.
    let mut conducted = BTreeMap::<SiteId, Vec<&PowerLink>>::new();
    for link in &graph.power_links {
        if let PowerTarget::Conductor { site, .. } = link.to {
            conducted.entry(site).or_default().push(link);
        }
    }
    fn leaves(
        graph: &CompileGraph,
        conducted: &BTreeMap<SiteId, Vec<&PowerLink>>,
        site: SiteId,
        port: PowerPort,
        seen: &mut BTreeSet<SiteId>,
        nodes: &mut BTreeSet<NodeId>,
        contact: &mut bool,
    ) {
        if !seen.insert(site) {
            return;
        }
        match graph.sites[site.index()].source {
            SiteSource::Contact(_) => *contact = true,
            SiteSource::Component(node) => {
                nodes.insert(node);
            }
            SiteSource::Constant(_) => {}
        }
        if matches!(port, PowerPort::Conducted { .. }) {
            for link in conducted.get(&site).into_iter().flatten() {
                leaves(graph, conducted, link.from, link.from_port, seen, nodes, contact);
            }
        }
    }
    // Cache the complete source leaves once per link rather than rescanning conduction per region.
    let inputs: Vec<_> = graph
        .power_links
        .iter()
        .map(|link| {
            let mut nodes = BTreeSet::new();
            let mut contact = false;
            leaves(
                graph,
                &conducted,
                link.from,
                link.from_port,
                &mut BTreeSet::new(),
                &mut nodes,
                &mut contact,
            );
            (link, nodes, contact)
        })
        .collect();
    for group in groups {
        report.largest_region = report.largest_region.max(group.len());
        let members: BTreeSet<_> = group.iter().copied().collect();
        let mut ingress = BTreeSet::new();
        let mut receivers = BTreeSet::new();
        let mut reasons = BTreeSet::<&str>::new();
        for &node in &group {
            let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                unreachable!()
            };
            for site in graph.wires[index].canonical_neighbors {
                match graph.sites[site.index()].source {
                    SiteSource::Contact(_) => {
                        reasons.insert("contact in canonical neighborhood");
                    }
                    SiteSource::Component(other)
                        if !members.contains(&other) && matches!(graph.nodes[other.index()].kind, CircuitNodeKind::Wire(_)) =>
                    {
                        reasons.insert("callback-only external wire");
                    }
                    _ => {}
                }
            }
        }
        for (link, sources, contact) in &inputs {
            if let PowerTarget::Input { node, .. } = link.to {
                if members.contains(&node) {
                    ingress.extend(sources.difference(&members).copied());
                    if *contact {
                        reasons.insert("contact electrical ingress");
                    }
                } else if sources.iter().any(|source| members.contains(source)) {
                    receivers.insert(node);
                }
            }
        }
        if ingress.len() != 1 {
            reasons.insert("not exactly one variable electrical ingress");
        }
        if ingress.iter().any(|node| {
            !matches!(
                graph.nodes[node.index()].kind,
                CircuitNodeKind::Electrical(
                    NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Torch | NodeType::Repeater { .. }
                )
            )
        }) {
            reasons.insert("unsupported ingress component");
        }
        for &receiver in &receivers {
            if !matches!(
                graph.nodes[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
            ) {
                reasons.insert("electrical receiver outside first family");
            }
            for (link, sources, contact) in &inputs {
                if let PowerTarget::Input { node, port } = link.to
                    && node == receiver
                {
                    if *contact || sources.iter().any(|source| !members.contains(source) && !ingress.contains(source)) {
                        reasons.insert("receiver has independent variable input");
                    }
                    if port == InputPort::Side && !sources.is_empty() {
                        reasons.insert("variable repeater locking input");
                    }
                }
            }
        }
        for node in &graph.nodes {
            if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind
                && let SiteSource::Component(watched) = graph.sites[watched.index()].source
                && members.contains(&watched)
            {
                reasons.insert("observer watches private wire");
            }
        }
        for &node in &group {
            let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                unreachable!()
            };
            for route in [graph.wires[index].shape_route, graph.wires[index].turbo_route].into_iter().flatten() {
                for delivery in &graph.routes[route.index()].deliveries {
                    if let Some(receiver) = delivery.receiver {
                        match receiver {
                            CallbackReceiver::Component(other)
                                if members.contains(&other) || receivers.contains(&other) || ingress.contains(&other) => {}
                            _ => {
                                reasons.insert("unowned synchronous callback boundary");
                            }
                        }
                    }
                }
            }
        }
        if reasons.is_empty() {
            report.structural_candidates += 1;
            report.candidate_wires += group.len();
            report.candidate_receivers += receivers.len();
            *report.candidate_lengths.entry(group.len()).or_default() += 1;
            let mut boundary = BTreeSet::new();
            for &node in &group {
                let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                    unreachable!()
                };
                for route in [graph.wires[index].shape_route, graph.wires[index].turbo_route].into_iter().flatten() {
                    for delivery in &graph.routes[route.index()].deliveries {
                        if let Some(CallbackReceiver::Component(node)) = delivery.receiver
                            && matches!(
                                graph.nodes[node.index()].kind,
                                CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
                            )
                        {
                            boundary.insert(node);
                        }
                    }
                }
            }
            let boundary=boundary.into_iter().map(|node| {
                let mut main=BTreeSet::new();
                for (link,sources,_) in &inputs {
                    if link.to==(PowerTarget::Input { node,port:InputPort::Main }) { main.extend(sources.iter().copied()); }
                }
                serde_json::json!({"node":node.index(),"position":graph.nodes[node.index()].position,"is_ingress":ingress.contains(&node),"main_sources":main.iter().map(|source|serde_json::json!({"node":source.index(),"position":graph.nodes[source.index()].position,"is_private":members.contains(source),"is_ingress":ingress.contains(source)})).collect::<Vec<_>>()})
            }).collect::<Vec<_>>();
            report.candidate_details.push(serde_json::json!({"wires":group.len(),"first_wire":graph.nodes[group[0].index()].position,"ingress":ingress.iter().map(|node|serde_json::json!({"node":node.index(),"position":graph.nodes[node.index()].position})).collect::<Vec<_>>(),"boundary":boundary,"static_decline":ClassicAnalysis::new(graph).infer(&group).err()}));
        } else {
            for reason in reasons {
                *report.declines.entry(reason.into()).or_default() += 1;
            }
        }
    }
    report
}

#[cfg(test)]
pub(super) fn record_component_census(graph: &CompileGraph) {
    let Some(path) = std::env::var_os("MCHPRS_CLASSIC_COMPONENT_CENSUS") else {
        return;
    };
    use std::io::Write;
    let row = serde_json::json!({"test":std::thread::current().name(),"report":census(graph)});
    let mut output = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(output, "{}", serde_json::to_string(&row).unwrap()).unwrap();
}

#[cfg(test)]
mod tests;
