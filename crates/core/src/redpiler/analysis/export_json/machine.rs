//! Complete inspection form of the finite program; integer vectors follow explicit owner tables.
use super::*;
use crate::redpiler::program_graph::{
    InstantKernel, InstantMachine, KernelBoundary, KernelEffect, KernelExpr, KernelExprId, KernelInput, KernelOutput, KernelStateExpr,
    KernelStateSlot,
};
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
        "role": "compile_time_certificate_only",
        "controls": machine.controls.iter().copied().map(node).collect::<Vec<_>>(),
        "owners": {
            "components": (0..graph.nodes.len()).map(|i| typed_id("node",i)).collect::<Vec<_>>(),
            "pistons": (0..graph.pistons.len()).map(|i| typed_id("piston",i)).collect::<Vec<_>>(),
            "resources": (0..graph.resources.len()).map(|i| typed_id("resource",i)).collect::<Vec<_>>(),
        },
        "states": states, "transitions": transitions,
    })
}

fn expression_id(id: KernelExprId) -> u32 {
    id.0
}

fn state_roots(state: &KernelStateExpr) -> Value {
    json!({
        "components": state.components.iter().copied().map(expression_id).collect::<Vec<_>>(),
        "pistons": state.pistons.iter().copied().map(expression_id).collect::<Vec<_>>(),
        "resources": state.resources.iter().map(|&(contact, enabled)| [expression_id(contact), expression_id(enabled)]).collect::<Vec<_>>(),
        "future": {
            "tick_len": expression_id(state.future.tick_len),
            "ticks": state.future.ticks.iter().map(|tick| json!({
                "node": expression_id(tick.node), "delay": expression_id(tick.delay), "priority": expression_id(tick.priority),
            })).collect::<Vec<_>>(),
            "request_len": expression_id(state.future.request_len),
            "requests": state.future.requests.iter().map(|request| json!({
                "piston": expression_id(request.piston), "action": expression_id(request.action),
            })).collect::<Vec<_>>(),
            "deferred_len": expression_id(state.future.deferred_len),
            "deferred": state.future.deferred.iter().map(|event| json!({
                "piston": expression_id(event.piston), "revision": expression_id(event.revision),
                "resource": expression_id(event.resource), "resource_revision": expression_id(event.resource_revision),
                "contact": expression_id(event.contact), "delay": expression_id(event.delay),
                "extending": expression_id(event.extending),
            })).collect::<Vec<_>>(),
        }
    })
}

fn effect_json(effect: &KernelEffect) -> Value {
    json!({
        "enabled": expression_id(effect.enabled),
        "piston": typed_id("piston", effect.piston.index()),
        "action": action(effect.action),
        "extended": effect.extended,
    })
}

pub(super) fn export_kernel(kernel: &InstantKernel, graph: &CompileGraph) -> Value {
    let expressions = kernel
        .expressions
        .iter()
        .map(|expression| match *expression {
            KernelExpr::Constant(value) => json!({ "kind": "constant", "value": value.to_string() }),
            KernelExpr::Select { control, low, high } => json!({
                "kind": "select", "control": control,
                "low": expression_id(low), "high": expression_id(high),
            }),
        })
        .collect::<Vec<_>>();
    let phases = kernel
        .phases
        .iter()
        .map(|phase| {
            json!({
                "components": phase.components.iter().map(|delta| json!({
                    "node": typed_id("node", delta.node.index()),
                    "changed": expression_id(delta.changed), "value": expression_id(delta.value),
                })).collect::<Vec<_>>(),
                "dirty": phase.dirty.iter().map(|&(node, touched)| json!({
                    "node": typed_id("node", node.index()), "touched": expression_id(touched),
                })).collect::<Vec<_>>(),
                "piston_revisions": phase.piston_revisions.iter().map(|&(piston, increment)| json!({
                    "piston": typed_id("piston", piston.index()), "increment": expression_id(increment),
                })).collect::<Vec<_>>(),
                "resource_revisions": phase.resource_revisions.iter().map(|&(resource, increment)| json!({
                    "resource": typed_id("resource", resource.index()), "increment": expression_id(increment),
                })).collect::<Vec<_>>(),
                "effects": phase.effects.iter().map(effect_json).collect::<Vec<_>>(),
                "scheduled": expression_id(phase.scheduled),
                "accepted": expression_id(phase.accepted),
                "published": expression_id(phase.published),
                "state": state_roots(&phase.state),
            })
        })
        .collect::<Vec<_>>();
    let boundary = graph
        .plans
        .instant
        .regions
        .first()
        .map(KernelBoundary::from_region)
        .map(|boundary| boundary_json(&boundary));
    json!({
        "encoding": "compact-instant-kernel-v1",
        "boundary": boundary,
        "trigger": typed_id("node", kernel.trigger.index()),
        "data_controls": kernel.data_controls.iter().map(|&node| typed_id("node", node.index())).collect::<Vec<_>>(),
        "owners": {
            "components": (0..graph.nodes.len()).map(|index| typed_id("node", index)).collect::<Vec<_>>(),
            "pistons": (0..graph.pistons.len()).map(|index| typed_id("piston", index)).collect::<Vec<_>>(),
            "resources": (0..graph.resources.len()).map(|index| typed_id("resource", index)).collect::<Vec<_>>(),
            "contacts": (0..graph.contacts.len()).map(|index| typed_id("contact", index)).collect::<Vec<_>>(),
        },
        "prefix": kernel.prefix,
        "period": kernel.period,
        "estimated_runtime_bytes": kernel.estimated_bytes(),
        "expressions": expressions,
        "entry": state_roots(&kernel.entry),
        "phases": phases,
    })
}

pub(super) fn boundary_json(boundary: &KernelBoundary) -> Value {
    let occurrence = |event: crate::redpiler::program_graph::RouteOccurrence| json!({ "route": event.route.index(), "ordinal": event.ordinal });
    let inputs = boundary.inputs.iter().map(|input| match input {
        KernelInput::Power(link) => json!({ "kind": "power", "link": link }),
        KernelInput::Callback(event) => json!({ "kind": "callback", "event": occurrence(*event) }),
    });
    let state = boundary.state.iter().map(|slot| match slot {
        KernelStateSlot::Reset(id) => json!({ "kind": "reset", "node": typed_id("node", id.index()) }),
        KernelStateSlot::Piston(id) => json!({ "kind": "piston", "piston": typed_id("piston", id.index()) }),
        KernelStateSlot::Resource(id) => json!({ "kind": "resource", "resource": typed_id("resource", id.index()) }),
        KernelStateSlot::Wire(id) => json!({ "kind": "wire", "node": typed_id("node", id.index()) }),
    });
    let outputs = boundary.outputs.iter().map(|output| match output {
        KernelOutput::Power(link) => json!({ "kind": "power", "link": link }),
        KernelOutput::Callback(event) => json!({ "kind": "callback", "event": occurrence(*event) }),
        KernelOutput::Observer(id) => json!({ "kind": "observer", "node": typed_id("node", id.index()) }),
    });
    json!({
        "inputs": inputs.collect::<Vec<_>>(),
        "state": state.collect::<Vec<_>>(),
        "outputs": outputs.collect::<Vec<_>>(),
        "effects": boundary.effects.iter().copied().map(occurrence).collect::<Vec<_>>(),
        "projection_roots": boundary.projection_roots.iter().map(|id| typed_id("site", id.index())).collect::<Vec<_>>(),
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

pub(super) fn validate_kernel(value: &Value, document: &GraphExport) -> Result<(), ExportError> {
    let invalid = || ExportError::new("validation", "invalid compact instant kernel export");
    let graph = &document.graph;
    let mut ids = HashSet::new();
    ids.extend(graph.nodes.iter().map(|node| node.id.as_str()));
    ids.extend(graph.pistons.iter().map(|piston| piston.id.as_str()));
    ids.extend(graph.resources.iter().map(|resource| resource.id.as_str()));
    ids.extend(graph.contacts.iter().map(|contact| contact.id.as_str()));
    ids.extend(graph.sites.iter().map(|site| site.id.as_str()));
    fn references(value: &Value, ids: &HashSet<&str>) -> bool {
        match value {
            Value::String(value) if value.contains(':') => ids.contains(value.as_str()),
            Value::Array(values) => values.iter().all(|value| references(value, ids)),
            Value::Object(values) => values.values().all(|value| references(value, ids)),
            _ => true,
        }
    }
    let expressions = value["expressions"].as_array().ok_or_else(invalid)?;
    let controls = value["data_controls"].as_array().ok_or_else(invalid)?;
    let node_ids: HashSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let piston_ids: HashSet<_> = graph.pistons.iter().map(|piston| piston.id.as_str()).collect();
    let resource_ids: HashSet<_> = graph.resources.iter().map(|resource| resource.id.as_str()).collect();
    let trigger = value["trigger"].as_str().ok_or_else(invalid)?;
    let prefix = value["prefix"].as_u64().ok_or_else(invalid)?;
    let period = value["period"].as_u64().ok_or_else(invalid)?;
    let phases = value["phases"].as_array().ok_or_else(invalid)?;
    if value["encoding"] != "compact-instant-kernel-v1"
        || !references(value, &ids)
        || controls.len() > 3
        || expressions.is_empty()
        || expressions.len() > 1_000_000
        || !node_ids.contains(trigger)
        || controls
            .iter()
            .any(|control| !control.as_str().is_some_and(|id| node_ids.contains(id) && id != trigger))
        || controls.iter().filter_map(Value::as_str).collect::<HashSet<_>>().len() != controls.len()
        || period == 0
        || prefix.checked_add(period) != Some(phases.len() as u64)
        || phases.len() > 128
    {
        return Err(invalid());
    }
    for (index, expression) in expressions.iter().enumerate() {
        match expression["kind"].as_str() {
            Some("constant") => {
                let canonical_u64 = |value: &str| {
                    !value.is_empty()
                        && value.bytes().all(|byte| byte.is_ascii_digit())
                        && (value == "0" || !value.starts_with('0'))
                        && value.parse::<u64>().is_ok()
                };
                if !expression["value"].as_str().is_some_and(canonical_u64) {
                    return Err(invalid());
                }
            }
            Some("select") => {
                let control = expression["control"].as_u64().ok_or_else(invalid)?;
                let low = expression["low"].as_u64().ok_or_else(invalid)?;
                let high = expression["high"].as_u64().ok_or_else(invalid)?;
                if control >= controls.len() as u64 || low >= index as u64 || high >= index as u64 {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    let root_valid = |value: &Value| value.as_u64().is_some_and(|root| root < expressions.len() as u64);
    fn owners_match<'a>(values: &Value, expected: impl Iterator<Item = &'a str>) -> bool {
        let expected: HashSet<_> = expected.collect();
        let Some(values) = values.as_array() else { return false };
        values.len() == expected.len()
            && values.iter().all(|owner| owner.as_str().is_some_and(|owner| expected.contains(owner)))
            && values.iter().filter_map(Value::as_str).collect::<HashSet<_>>().len() == values.len()
    }
    let owners = &value["owners"];
    if !owners_match(&owners["components"], graph.nodes.iter().map(|record| record.id.as_str()))
        || !owners_match(&owners["pistons"], graph.pistons.iter().map(|record| record.id.as_str()))
        || !owners_match(&owners["resources"], graph.resources.iter().map(|record| record.id.as_str()))
        || !owners_match(&owners["contacts"], graph.contacts.iter().map(|record| record.id.as_str()))
    {
        return Err(invalid());
    }
    let state_valid = |state: &Value| {
        let Some(components) = state["components"].as_array() else {
            return false;
        };
        let Some(pistons) = state["pistons"].as_array() else { return false };
        let Some(resources) = state["resources"].as_array() else { return false };
        let future = &state["future"];
        let Some(ticks) = future["ticks"].as_array() else { return false };
        let Some(requests) = future["requests"].as_array() else { return false };
        let Some(deferred) = future["deferred"].as_array() else { return false };
        components.len() == graph.nodes.len()
            && pistons.len() == graph.pistons.len()
            && resources.len() == graph.resources.len()
            && components.iter().all(&root_valid)
            && pistons.iter().all(&root_valid)
            && resources
                .iter()
                .all(|pair| pair.as_array().is_some_and(|pair| pair.len() == 2 && pair.iter().all(&root_valid)))
            && root_valid(&future["tick_len"])
            && ticks.len() <= 128
            && ticks
                .iter()
                .all(|tick| ["node", "delay", "priority"].iter().all(|field| root_valid(&tick[field])))
            && root_valid(&future["request_len"])
            && requests.len() <= 64
            && requests
                .iter()
                .all(|request| ["piston", "action"].iter().all(|field| root_valid(&request[field])))
            && root_valid(&future["deferred_len"])
            && deferred.len() <= 128
            && deferred.iter().all(|event| {
                ["piston", "revision", "resource", "resource_revision", "contact", "delay", "extending"]
                    .iter()
                    .all(|field| root_valid(&event[field]))
            })
    };
    if !state_valid(&value["entry"]) {
        return Err(invalid());
    }
    for phase in phases {
        if !state_valid(&phase["state"]) || !["scheduled", "accepted", "published"].iter().all(|field| root_valid(&phase[field])) {
            return Err(invalid());
        }
        for delta in phase["components"].as_array().ok_or_else(invalid)? {
            if !delta["node"].as_str().is_some_and(|node| node_ids.contains(node)) || !root_valid(&delta["changed"]) || !root_valid(&delta["value"]) {
                return Err(invalid());
            }
        }
        for delta in phase["dirty"].as_array().ok_or_else(invalid)? {
            if !delta["node"].as_str().is_some_and(|node| node_ids.contains(node)) || !root_valid(&delta["touched"]) {
                return Err(invalid());
            }
        }
        for delta in phase["piston_revisions"].as_array().ok_or_else(invalid)? {
            if !delta["piston"].as_str().is_some_and(|piston| piston_ids.contains(piston)) || !root_valid(&delta["increment"]) {
                return Err(invalid());
            }
        }
        for delta in phase["resource_revisions"].as_array().ok_or_else(invalid)? {
            if !delta["resource"].as_str().is_some_and(|resource| resource_ids.contains(resource)) || !root_valid(&delta["increment"]) {
                return Err(invalid());
            }
        }
        for effect in phase["effects"].as_array().ok_or_else(invalid)? {
            if !effect["piston"].as_str().is_some_and(|piston| piston_ids.contains(piston))
                || !root_valid(&effect["enabled"])
                || !effect["action"]
                    .as_str()
                    .is_some_and(|action| matches!(action, "extend" | "retract" | "retract_without_pull"))
                || !effect["extended"].is_boolean()
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
