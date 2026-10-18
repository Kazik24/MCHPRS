//! Additive typed references; source callbacks remain explicit proof provenance.
use super::*;
use serde_json::{Value, json};

pub(super) fn owned(graph: &CompileGraph, node: crate::redpiler::program_graph::NodeId) -> bool {
    graph.plans.classic_nets.iter().any(|net| net.wires.iter().any(|wire| wire.node == node))
}

pub(super) fn export(graph: &CompileGraph) -> Vec<Value> {
    graph.plans.classic_nets.iter().enumerate().map(|(index,net)|json!({
        "id":typed_id("classic_net",index),"source":typed_id("node",net.source.index()),
        "initial_signal":net.initial_signal,"pair_index":"old * 16 + new",
        "outside_main":net.outside.iter().map(|input|json!({"receiver":typed_id("node",input.receiver.index()),"node":typed_id("node",input.node.index()),"attenuation":input.attenuation})).collect::<Vec<_>>(),
        "guarded_wires":net.guarded_wires.iter().map(|node|typed_id("node",node.index())).collect::<Vec<_>>(),
        "guard_failure":"materialize_committed_signal_then_permanent_general_execution",
        "wires":net.wires.iter().map(|wire|json!({"node":typed_id("node",wire.node.index()),"attenuation":wire.attenuation})).collect::<Vec<_>>(),
        "inputs":net.inputs.iter().map(|input|json!({"receiver":typed_id("node",input.receiver.index()),"attenuation":input.attenuation,"source_attenuation":input.source_attenuation,"guarded":input.guarded})).collect::<Vec<_>>(),
        "entries":net.entries.iter().map(|entry|json!({"route":typed_id("route",entry.route.index()),"ordinal":entry.ordinal,"wire":typed_id("node",entry.wire.index()),"commit":entry.commit,
            "ops":entry.ops.iter().map(|op|json!({"receiver":typed_id("node",op.receiver.index()),"sample":op.sample,"nonzero":match op.sample { crate::redpiler::program_graph::ClassicSample::Boolean { nonzero }=>Some(nonzero), _=>None },"when":op.when.map(|mask|format!("{mask:016x}"))})).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })).collect()
}

pub(super) fn validate(nets: &[Value], nodes: &HashSet<String>, routes: &HashSet<String>, graph: &GraphTablesDto) -> Result<(), ExportError> {
    let invalid = || ExportError::new("validation", "invalid classic signal net");
    unique_ids("classic_net", nets.iter().map(|net| net["id"].as_str().unwrap_or("")))?;
    let mut wires: HashSet<_> = graph
        .dust_paths
        .iter()
        .flat_map(|path| path.wires.iter().map(String::as_str))
        .chain(graph.instant_regions.iter().flat_map(|region| region.wires.iter().map(String::as_str)))
        .collect();
    let mut receivers = HashSet::new();
    let mut entries = HashSet::new();
    for net in nets {
        check_ref(nodes, net["source"].as_str().ok_or_else(invalid)?, "classic source")?;
        if net["initial_signal"].as_u64().is_none_or(|signal| signal > 15) {
            return Err(invalid());
        }
        let private = net["wires"].as_array().ok_or_else(invalid)?;
        let mut local = HashSet::new();
        let mut local_receivers = HashSet::new();
        let mut guarded_receivers = HashSet::new();
        let mut outside_receivers = HashSet::new();
        let mut outside_terms = HashSet::new();
        if private.is_empty() {
            return Err(invalid());
        }
        for wire in private {
            let node = wire["node"].as_str().ok_or_else(invalid)?;
            check_ref(nodes, node, "classic private wire")?;
            if !wires.insert(node)
                || !graph
                    .nodes
                    .iter()
                    .any(|record| record.id == node && matches!(record.kind, NodeKindDto::Wire { .. }))
                || wire["attenuation"].as_u64().is_none_or(|weight| weight > 15)
            {
                return Err(invalid());
            }
            local.insert(node);
        }
        if let Some(guards) = net.get("guarded_wires") {
            let mut seen = HashSet::new();
            for guard in guards.as_array().ok_or_else(invalid)? {
                let node = guard.as_str().ok_or_else(invalid)?;
                check_ref(nodes, node, "classic guarded wire")?;
                if local.contains(node)
                    || !seen.insert(node)
                    || !graph
                        .nodes
                        .iter()
                        .any(|record| record.id == node && matches!(record.kind, NodeKindDto::Wire { .. }))
                {
                    return Err(invalid());
                }
            }
        }
        for input in net["inputs"].as_array().ok_or_else(invalid)? {
            let receiver = input["receiver"].as_str().ok_or_else(invalid)?;
            check_ref(nodes, receiver, "classic direct input")?;
            if !receivers.insert(receiver)
                || ["attenuation", "source_attenuation"]
                    .iter()
                    .any(|key| input[key].as_u64().is_none_or(|weight| weight > 15))
            {
                return Err(invalid());
            }
            local_receivers.insert(receiver);
            if let Some(guarded) = input.get("guarded") {
                if guarded.as_bool().ok_or_else(invalid)? {
                    guarded_receivers.insert(receiver);
                }
            }
        }
        if let Some(outside) = net.get("outside_main") {
            for term in outside.as_array().ok_or_else(invalid)? {
                let node = term["node"].as_str().ok_or_else(invalid)?;
                let receiver = term["receiver"].as_str().ok_or_else(invalid)?;
                check_ref(nodes, node, "classic outside main source")?;
                if local.contains(node)
                    || Some(node) == net["source"].as_str()
                    || !outside_terms.insert((receiver, node))
                    || !local_receivers.contains(receiver)
                    || term["attenuation"].as_u64().is_none_or(|weight| weight >= 15)
                {
                    return Err(invalid());
                }
                outside_receivers.insert(receiver);
            }
        }
        if guarded_receivers != outside_receivers {
            return Err(invalid());
        }
        for entry in net["entries"].as_array().ok_or_else(invalid)? {
            let route = entry["route"].as_str().ok_or_else(invalid)?;
            let ordinal = entry["ordinal"].as_u64().ok_or_else(invalid)?;
            check_ref(routes, route, "classic entry route")?;
            let wire = entry["wire"].as_str().ok_or_else(invalid)?;
            check_ref(nodes, wire, "classic entry wire")?;
            if entry["commit"].as_bool().is_none() || !entries.insert((route, ordinal)) {
                return Err(invalid());
            }
            let delivery = graph
                .routes
                .as_ref()
                .and_then(|routes| routes.iter().find(|record| record.id == route))
                .and_then(|route| route.deliveries.get(ordinal as usize))
                .ok_or_else(invalid)?;
            if !local.contains(wire)
                || entry["commit"] == true && delivery.kind != "update"
                || entry["commit"] == false && !entry["ops"].as_array().ok_or_else(invalid)?.is_empty()
            {
                return Err(invalid());
            }
            for op in entry["ops"].as_array().ok_or_else(invalid)? {
                check_ref(nodes, op["receiver"].as_str().ok_or_else(invalid)?, "classic receiver operation")?;
                if !local_receivers.contains(op["receiver"].as_str().ok_or_else(invalid)?) {
                    return Err(invalid());
                }
                let masks = op["when"].as_array().ok_or_else(invalid)?;
                let valid_sample = match op["sample"]["kind"].as_str() {
                    Some("boolean") => op["sample"]["nonzero"].as_bool().is_some() && op["sample"]["nonzero"] == op["nonzero"],
                    Some("comparator") => {
                        op["sample"]["strength"].as_u64().is_some_and(|value| value <= 15) && op["sample"]["powered"].as_bool().is_some()
                    }
                    None => op["nonzero"].as_bool().is_some(),
                    _ => false,
                };
                if !valid_sample
                    || masks.len() != 4
                    || masks.iter().any(|mask| {
                        mask.as_str()
                            .is_none_or(|mask| mask.len() != 16 || u64::from_str_radix(mask, 16).is_err())
                    })
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}
