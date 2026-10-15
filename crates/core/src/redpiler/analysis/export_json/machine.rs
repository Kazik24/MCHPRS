//! Complete inspection form of the finite program; integer vectors follow explicit owner tables.
use super::*;
use crate::redpiler::program_graph::InstantMachine;
use mchprs_world::PistonAction;
use serde_json::{Value, json};

fn action(value: PistonAction) -> &'static str {
    match value {
        PistonAction::Extend => "extend",
        PistonAction::Retract => "retract",
        PistonAction::RetractWithoutPull => "retract_without_pull",
    }
}

pub(super) fn export(machine: &InstantMachine, graph: &CompileGraph) -> Value {
    let node = |id: crate::redpiler::program_graph::NodeId| typed_id("node", id.index());
    let piston = |id: crate::redpiler::program_graph::PistonId| typed_id("piston", id.index());
    let resource = |id: crate::redpiler::program_graph::ResourceId| typed_id("resource", id.index());
    let contact = |id: crate::redpiler::program_graph::ContactId| typed_id("contact", id.index());
    let states: Vec<_> = machine
        .states
        .iter()
        .map(|s| {
            json!({
                "components": s.components, "pistons": s.pistons,
                "resources": s.resources.iter().map(|&(at, enabled)| (contact(at), enabled)).collect::<Vec<_>>(),
                "ticks": s.ticks.iter().map(|t| (node(t.node), t.delay, t.priority as u8)).collect::<Vec<_>>(),
                "requests": s.requests.iter().map(|&(p,a)| (piston(p), action(a))).collect::<Vec<_>>(),
                "deferred": s.deferred.iter().map(|d| json!({
                    "piston": piston(d.piston), "revision_offset": d.revision.to_string(),
                    "resource": d.resource.map(|(r,revision,at)| (resource(r),revision.to_string(),contact(at))),
                    "delay": d.delay, "extending": d.extending,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let transitions: Vec<Vec<_>> = machine
        .transitions
        .iter()
        .map(|row| {
            row.iter()
                .map(|entry| {
                    entry.as_ref().map(|e| json!({
        "next": e.next,
        "components": e.components.iter().map(|&(n,v)| (node(n),v)).collect::<Vec<_>>(),
        "pistons": e.pistons.iter().map(|&(p,v,r)| (piston(p),v,r.to_string())).collect::<Vec<_>>(),
        "resources": e.resources.iter().map(|&(r,at,enabled,revision)| (resource(r),contact(at),enabled,revision.to_string())).collect::<Vec<_>>(),
        "dirty": e.dirty.iter().copied().map(node).collect::<Vec<_>>(),
        "effects": e.effects.iter().map(|&(p,a,extended)| (piston(p),action(a),extended)).collect::<Vec<_>>(),
        "scheduled": e.scheduled, "accepted": e.accepted, "published": e.published,
    }))
                })
                .collect()
        })
        .collect();
    json!({
        "encoding": "closed-instant-machine-v1",
        "controls": machine.controls.iter().copied().map(node).collect::<Vec<_>>(),
        "owners": {
            "components": (0..graph.nodes.len()).map(|i| typed_id("node",i)).collect::<Vec<_>>(),
            "pistons": (0..graph.pistons.len()).map(|i| typed_id("piston",i)).collect::<Vec<_>>(),
            "resources": (0..graph.resources.len()).map(|i| typed_id("resource",i)).collect::<Vec<_>>(),
        },
        "states": states, "transitions": transitions,
    })
}

pub(super) fn validate(value: &Value, document: &GraphExport) -> Result<(), ExportError> {
    let invalid = || ExportError::new("validation", "invalid closed instant machine export");
    let graph = &document.graph;
    let mut ids = HashSet::new();
    ids.extend(graph.nodes.iter().map(|n| n.id.as_str()));
    ids.extend(graph.pistons.iter().map(|n| n.id.as_str()));
    ids.extend(graph.resources.iter().map(|n| n.id.as_str()));
    ids.extend(graph.contacts.iter().map(|n| n.id.as_str()));
    fn references(value: &Value, ids: &HashSet<&str>) -> bool {
        match value {
            Value::String(s) if s.contains(':') => ids.contains(s.as_str()),
            Value::Array(values) => values.iter().all(|v| references(v, ids)),
            Value::Object(values) => values.values().all(|v| references(v, ids)),
            _ => true,
        }
    }
    if value["encoding"] != "closed-instant-machine-v1" || !references(value, &ids) {
        return Err(invalid());
    }
    let controls = value["controls"].as_array().ok_or_else(invalid)?;
    let states = value["states"].as_array().ok_or_else(invalid)?;
    let rows = value["transitions"].as_array().ok_or_else(invalid)?;
    if controls.is_empty() || controls.len() > 4 || states.is_empty() || states.len() > 2048 || rows.len() != states.len() {
        return Err(invalid());
    }
    if controls
        .iter()
        .any(|control| !control.as_str().is_some_and(|id| graph.nodes.iter().any(|n| n.id == id)))
    {
        return Err(invalid());
    }
    for (field, count) in [
        ("components", graph.nodes.len()),
        ("pistons", graph.pistons.len()),
        ("resources", graph.resources.len()),
    ] {
        if value["owners"][field].as_array().map(Vec::len) != Some(count) || states.iter().any(|s| s[field].as_array().map(Vec::len) != Some(count)) {
            return Err(invalid());
        }
    }
    for row in rows {
        let row = row.as_array().ok_or_else(invalid)?;
        if row.len() != controls.len() + 1
            || row
                .iter()
                .any(|e| !e.is_null() && !e["next"].as_u64().is_some_and(|n| n < states.len() as u64))
        {
            return Err(invalid());
        }
    }
    Ok(())
}
