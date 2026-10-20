//! Lossless inspection of the flat CPU program, including masks above JavaScript's integer range.
use super::*;
use crate::redpiler::program_graph::{CpuPredicate, CpuRead};
use serde_json::{Value, json};

fn predicate(p: CpuPredicate) -> Value {
    json!({"read_start":p.read_start,"read_len":p.read_len,"clause_start":p.clause_start,"clause_len":p.clause_len})
}

pub(super) fn export(graph: &CompileGraph) -> Option<Value> {
    let plans = &graph.plans.cpu_kernels;
    if plans.entries.is_empty() {
        return None;
    }
    Some(json!({
        "method":"live_bitmask_dnf","effects":"original_graph_fifo",
        "entries":plans.entries.iter().map(|e| json!({"actor":typed_id("piston",e.actor.index()),"main":predicate(e.main),"quasi":predicate(e.quasi),"valid":predicate(e.valid)})).collect::<Vec<_>>(),
        "reads":plans.reads.iter().map(|r| match *r {
            CpuRead::State(test)=>json!({"kind":"state","test":super::instant_layers::state_test_dto(test)}),
            CpuRead::Strength{node,threshold}=>json!({"kind":"strength","node":typed_id("node",node.index()),"threshold":threshold}),
            CpuRead::WireEmission{node,face:direction,threshold}=>json!({"kind":"wire_emission","node":typed_id("node",node.index()),"face":face(direction),"threshold":threshold}),
            CpuRead::Guard(guard)=>json!({"kind":"guard","guard":typed_id("guard",guard.index())}),
            CpuRead::Input{node,port}=>json!({"kind":"input","node":typed_id("node",node.index()),"port":input_port(port)}),
            CpuRead::Source{site,port,threshold}=>json!({"kind":"source","site":typed_id("site",site.index()),"threshold":threshold,"port":match port {
                PowerPort::Output=>json!({"kind":"output"}),PowerPort::Stored=>json!({"kind":"stored"}),
                PowerPort::Weak(f)=>json!({"kind":"weak","face":face(f)}),PowerPort::Strong(f)=>json!({"kind":"strong","face":face(f)}),
                PowerPort::Conducted{include_dust}=>json!({"kind":"conducted","include_dust":include_dust}),
            }}),
        }).collect::<Vec<_>>(),
        "require_true":plans.require_true.iter().map(|m|format!("{m:016x}")).collect::<Vec<_>>(),
        "require_false":plans.require_false.iter().map(|m|format!("{m:016x}")).collect::<Vec<_>>(),
    }))
}

pub(super) fn validate(graph: &GraphTablesDto) -> Result<(), ExportError> {
    let Some(program) = &graph.cpu_kernels else {
        return Ok(());
    };
    let invalid = || ExportError::new("cpu_kernels", "invalid packed CPU kernel program");
    if program["method"] != "live_bitmask_dnf" || program["effects"] != "original_graph_fifo" {
        return Err(invalid());
    }
    let ids: HashSet<_> = graph
        .nodes
        .iter()
        .map(|r| r.id.as_str())
        .chain(graph.sites.iter().map(|r| r.id.as_str()))
        .chain(graph.guards.iter().map(|r| r.id.as_str()))
        .chain(graph.pistons.iter().map(|r| r.id.as_str()))
        .chain(graph.resources.iter().map(|r| r.id.as_str()))
        .chain(graph.contacts.iter().map(|r| r.id.as_str()))
        .collect();
    let reference = |value: &Value, prefix: &str| -> Result<(), ExportError> {
        value
            .as_str()
            .filter(|id| id.starts_with(prefix) && ids.contains(id))
            .ok_or_else(invalid)
            .map(|_| ())
    };
    let reads = program["reads"].as_array().ok_or_else(invalid)?;
    for read in reads {
        match read["kind"].as_str() {
            Some("state") => super::instant_layers::validate_state_test(&read["test"], &ids)?,
            Some("guard") => reference(&read["guard"], "guard:")?,
            Some("input" | "strength" | "wire_emission") => {
                reference(&read["node"], "node:")?;
                if read["kind"] == "wire_emission" && !matches!(read["face"].as_str(), Some("north" | "south" | "east" | "west" | "top" | "bottom")) {
                    return Err(invalid());
                }
                if read["kind"] == "input" && !matches!(read["port"].as_str(), Some("main" | "quasi")) {
                    return Err(invalid());
                }
            }
            Some("source") => {
                reference(&read["site"], "site:")?;
                match read["port"]["kind"].as_str() {
                    Some("stored" | "output") => {}
                    Some("weak" | "strong")
                        if matches!(
                            read["port"]["face"].as_str(),
                            Some("north" | "south" | "east" | "west" | "top" | "bottom")
                        ) => {}
                    Some("conducted") if read["port"]["include_dust"].is_boolean() => {}
                    _ => return Err(invalid()),
                }
            }
            _ => return Err(invalid()),
        }
        if matches!(read["kind"].as_str(), Some("strength" | "source" | "wire_emission")) && read["threshold"].as_u64().is_none_or(|n| n > 14) {
            return Err(invalid());
        }
    }
    let masks = |key: &str| -> Result<Vec<u64>, ExportError> {
        program[key]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|m| {
                let s = m
                    .as_str()
                    .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(invalid)?;
                u64::from_str_radix(s, 16).map_err(|_| invalid())
            })
            .collect()
    };
    let yes = masks("require_true")?;
    let no = masks("require_false")?;
    if yes.len() != no.len() {
        return Err(invalid());
    }
    let mut owned = HashSet::new();
    let event_actors: HashSet<_> = graph
        .instant_event_kernels
        .iter()
        .flat_map(|k| k["actors"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    for entry in program["entries"].as_array().ok_or_else(invalid)? {
        reference(&entry["actor"], "piston:")?;
        let actor = entry["actor"].as_str().unwrap();
        if !owned.insert(actor) || !event_actors.contains(actor) {
            return Err(invalid());
        }
        for key in ["main", "quasi", "valid"] {
            let p = &entry[key];
            let number = |field: &str| p[field].as_u64().ok_or_else(invalid);
            let r = number("read_start")?;
            let n = number("read_len")?;
            let c = number("clause_start")?;
            let l = number("clause_len")?;
            if n > 64 || l > 64 || r > reads.len() as u64 || n > reads.len() as u64 - r || c > yes.len() as u64 || l > yes.len() as u64 - c {
                return Err(invalid());
            }
            let allowed = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
            for i in c as usize..(c + l) as usize {
                if yes[i] & no[i] != 0 || (yes[i] | no[i]) & !allowed != 0 {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}
