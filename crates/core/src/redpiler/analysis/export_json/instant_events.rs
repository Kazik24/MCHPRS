//! Export the actual FIFO-local entry programs, separately from atomic-wave evidence.
use super::*;
use crate::redpiler::program_graph::{CellGateKind, CellPin, CellRead, KernelProgram};
use serde_json::{Value, json};

pub(super) fn export(graph: &CompileGraph) -> Vec<Value> {
    let links: HashMap<_, _> = graph.power_links.iter().enumerate().map(|(index, link)| (*link, index)).collect();
    graph
        .plans
        .instant_events
        .iter()
        .enumerate()
        .map(|(index, kernel)| {
            let KernelProgram::Events(entries) = &kernel.program else {
                unreachable!("validated event program")
            };
            json!({
                "id":typed_id("instant_event_kernel",index),
                "actors":kernel.actors.iter().map(|actor|typed_id("piston",actor.index())).collect::<Vec<_>>(),
                "wire_propagation_removed":false,
                "entries":entries.iter().map(|entry|json!({
                    "actor":typed_id("piston",entry.actor.index()),"valid":entry.valid.index(),
                    "pins":entry.pins.iter().map(|pin|match pin {
                        CellPin::Read(CellRead::State(test))=>json!({"kind":"state","test":super::instant_layers::state_test_dto(*test)}),
                        CellPin::Read(CellRead::Contribution(link))=>json!({"kind":"contribution","link":typed_id("power_link",links[link])}),
                        CellPin::Read(CellRead::Activation(_))=>unreachable!("activation cannot reach an acceptance predicate"),
                        CellPin::Gate {kind,inputs}=>json!({"kind":"gate","gate":match kind {
                            CellGateKind::Buffer=>"buffer",CellGateKind::Invert=>"invert",CellGateKind::All=>"all",CellGateKind::Any=>"any",
                        },"inputs":inputs.iter().map(|input|input.index()).collect::<Vec<_>>()}),
                    }).collect::<Vec<_>>()
                })).collect::<Vec<_>>()
            })
        })
        .collect()
}

pub(super) fn export_memory_reads(graph: &CompileGraph) -> Vec<Value> {
    let links: HashMap<_, _> = graph.power_links.iter().enumerate().map(|(index, link)| (*link, index)).collect();
    graph
        .plans
        .bud_memory_reads
        .iter()
        .enumerate()
        .map(|(index, read)| {
            json!({
                "id":typed_id("bud_memory_read",index),"storage":typed_id("piston",read.storage.index()),
                "reader":typed_id("piston",read.reader.index()),"resource":typed_id("resource",read.resource.index()),
                "source":typed_id("contact",read.source.index()),"link":typed_id("power_link",links[&read.link]),
                "updates":"original_graph_fifo","read":"enabled_at_source",
            })
        })
        .collect()
}

pub(super) fn validate(graph: &GraphTablesDto) -> Result<(), ExportError> {
    let invalid = || ExportError::new("instant_events", "invalid FIFO-local instant kernel");
    let ids: HashSet<_> = graph
        .pistons
        .iter()
        .map(|row| row.id.as_str())
        .chain(graph.resources.iter().map(|row| row.id.as_str()))
        .chain(graph.contacts.iter().map(|row| row.id.as_str()))
        .chain(graph.power_links.iter().map(|row| row.id.as_str()))
        .collect();
    let mut owned = HashSet::new();
    let mut readers = HashSet::new();
    for (index, read) in graph.bud_memory_reads.iter().enumerate() {
        if read["id"] != typed_id("bud_memory_read", index) || read["updates"] != "original_graph_fifo" || read["read"] != "enabled_at_source" {
            return Err(invalid());
        }
        for (field, prefix) in [
            ("storage", "piston:"),
            ("reader", "piston:"),
            ("resource", "resource:"),
            ("source", "contact:"),
            ("link", "power_link:"),
        ] {
            read[field]
                .as_str()
                .filter(|id| id.starts_with(prefix) && ids.contains(id))
                .ok_or_else(invalid)?;
        }
        if read["storage"] == read["reader"] || !readers.insert(read["reader"].as_str().unwrap()) {
            return Err(invalid());
        }
    }
    for (index, kernel) in graph.instant_event_kernels.iter().enumerate() {
        if kernel["id"] != typed_id("instant_event_kernel", index) || kernel["wire_propagation_removed"] != false {
            return Err(invalid());
        }
        let actors = kernel["actors"].as_array().ok_or_else(invalid)?;
        let entries = kernel["entries"].as_array().ok_or_else(invalid)?;
        if actors.is_empty() || actors.len() != entries.len() {
            return Err(invalid());
        }
        for (actor, entry) in actors.iter().zip(entries) {
            let actor = actor
                .as_str()
                .filter(|id| id.starts_with("piston:") && ids.contains(id))
                .ok_or_else(invalid)?;
            if entry["actor"] != actor || !owned.insert(actor) {
                return Err(invalid());
            }
            let pins = entry["pins"].as_array().ok_or_else(invalid)?;
            if entry["valid"].as_u64().is_none_or(|root| root >= pins.len() as u64) {
                return Err(invalid());
            }
            for (ordinal, pin) in pins.iter().enumerate() {
                match pin["kind"].as_str() {
                    Some("state") => super::instant_layers::validate_state_test(&pin["test"], &ids)?,
                    Some("contribution") => {
                        pin["link"]
                            .as_str()
                            .filter(|id| id.starts_with("power_link:") && ids.contains(id))
                            .ok_or_else(invalid)?;
                    }
                    Some("gate") => {
                        let inputs = pin["inputs"].as_array().ok_or_else(invalid)?;
                        if inputs.iter().any(|input| input.as_u64().is_none_or(|input| input >= ordinal as u64)) {
                            return Err(invalid());
                        }
                        match pin["gate"].as_str() {
                            Some("all" | "any") => {}
                            Some("buffer" | "invert") if inputs.len() == 1 => {}
                            _ => return Err(invalid()),
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
        }
    }
    Ok(())
}
