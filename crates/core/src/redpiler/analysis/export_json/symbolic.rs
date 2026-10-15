//! Inspection and validation of grouped symbolic region event programs.
use super::{ExportError, GraphTablesDto, typed_id};
use crate::redpiler::program_graph::{ActorRead, CompileGraph, EventRoutine, ResetRead, SymbolicRegion, SymbolicRegionDecline};
use mchprs_redpiler_opt::Op;
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn export(region: &SymbolicRegion, graph: &CompileGraph) -> Value {
    let actors = region
        .actors
        .iter()
        .map(|actor| {
            json!({
                "actor": typed_id("piston", actor.actor.index()),
                "reads": reads_json(&actor.reads, graph),
                "routines": [routine_json(&actor.routines[0], "sample"), routine_json(&actor.routines[1], "accept")],
            })
        })
        .collect::<Vec<_>>();
    let resets = region
        .resets
        .iter()
        .map(|reset| {
            json!({
                "node": typed_id("node", reset.node.index()),
                "facing": super::face(reset.facing),
                "reads": reset_reads_json(&reset.reads),
                "routines": [routine_json(&reset.routines[0], "update"), routine_json(&reset.routines[1], "tick")],
            })
        })
        .collect::<Vec<_>>();
    json!({
        "region": typed_id("instant_region", region.region),
        "boundary": super::machine::boundary_json(&region.boundary),
        "actors": actors,
        "resets": resets,
    })
}

pub(super) fn export_decline(decline: &SymbolicRegionDecline) -> Value {
    json!({
        "region": typed_id("instant_region", decline.region),
        "actor_count": decline.actor_count,
        "reset_count": decline.reset_count,
        "reason": decline.reason,
    })
}

fn reads_json(reads: &[ActorRead], graph: &CompileGraph) -> Vec<Value> {
    reads
        .iter()
        .map(|read| match read {
            ActorRead::Power(link) => json!({
                "kind": "power",
                "link": typed_id("power_link", graph.power_links.iter().position(|candidate| *candidate == *link).expect("validated power binding")),
            }),
            _ => json!({ "kind": read_name(*read) }),
        })
        .collect()
}

fn reset_reads_json(reads: &[ResetRead]) -> Vec<Value> {
    reads.iter().map(|read| json!({"kind": reset_read_name(*read)})).collect()
}

fn routine_json(routine: &EventRoutine, event: &str) -> Value {
    let expressions: Vec<_> = routine
        .plan
        .ops()
        .iter()
        .map(|op| match *op {
            Op::Input(input) => json!({"op": "input", "input": input}),
            Op::Const(value) => json!({"op": "constant", "value": value}),
            Op::Buffer(a) => json!({"op": "buffer", "args": [a]}),
            Op::Not(a) => json!({"op": "not", "args": [a]}),
            Op::And(a, b) => json!({"op": "and", "args": [a,b]}),
            Op::Or(a, b) => json!({"op": "or", "args": [a,b]}),
            Op::Xor(a, b) => json!({"op": "xor", "args": [a,b]}),
            Op::Select(c, a, b) => json!({"op": "select", "args": [c,a,b]}),
        })
        .collect();
    json!({"event": event, "expressions": expressions, "roots": routine.plan.roots()})
}

fn read_name(read: ActorRead) -> &'static str {
    match read {
        ActorRead::Extended => "extended",
        ActorRead::BasePending => "base_pending",
        ActorRead::Ready => "ready",
        ActorRead::LaneAvailable => "lane_available",
        ActorRead::RetractionAvailable => "retraction_available",
        ActorRead::EarlyRetractionAvailable => "early_retraction_available",
        ActorRead::EarlyResource => "early_resource",
        ActorRead::Power(_) => unreachable!("typed power references are exported separately"),
    }
}

fn reset_read_name(read: ResetRead) -> &'static str {
    match read {
        ResetRead::Powered => "reset_powered",
        ResetRead::Pending => "reset_pending",
        ResetRead::Qualifies => "reset_qualifies",
    }
}

pub(super) fn validate(values: &[Value], declines: &[Value], graph: &GraphTablesDto) -> Result<(), ExportError> {
    let invalid = || ExportError::new("validation", "invalid symbolic region event program");
    let mut seen_regions = HashSet::new();
    for region in values {
        let region_id = region["region"].as_str().ok_or_else(invalid)?;
        let Some(owner) = graph.instant_regions.iter().find(|item| item.id == region_id) else {
            return Err(invalid());
        };
        if !seen_regions.insert(region_id) || !super::machine::validate_boundary_json(&region["boundary"], graph) {
            return Err(invalid());
        }
        let actors = region["actors"].as_array().ok_or_else(invalid)?;
        let actor_ids = actors
            .iter()
            .map(|actor| actor["actor"].as_str().ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        if actor_ids != owner.actors.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err(invalid());
        }
        for actor in actors {
            let reads = actor["reads"].as_array().ok_or_else(invalid)?;
            validate_reads(reads, graph, ReadFamily::Actor).map_err(|_| invalid())?;
            validate_routines(&actor["routines"], reads.len(), &[("sample", 3), ("accept", 3)]).map_err(|_| invalid())?;
        }
        let resets = region["resets"].as_array().ok_or_else(invalid)?;
        let reset_ids = resets
            .iter()
            .map(|reset| reset["node"].as_str().ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        if reset_ids != owner.resets.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err(invalid());
        }
        for reset in resets {
            let reset_id = reset["node"].as_str().ok_or_else(invalid)?;
            let Some(node) = graph.nodes.iter().find(|node| node.id == reset_id) else {
                return Err(invalid());
            };
            let super::NodeKindDto::Observer { facing, .. } = &node.kind else {
                return Err(invalid());
            };
            if reset["facing"].as_str() != Some(facing) {
                return Err(invalid());
            }
            let reads = reset["reads"].as_array().ok_or_else(invalid)?;
            validate_reads(reads, graph, ReadFamily::Reset).map_err(|_| invalid())?;
            validate_routines(&reset["routines"], reads.len(), &[("update", 1), ("tick", 2)]).map_err(|_| invalid())?;
        }
    }
    for decline in declines {
        let region_id = decline["region"].as_str().ok_or_else(invalid)?;
        let Some(owner) = graph.instant_regions.iter().find(|item| item.id == region_id) else {
            return Err(invalid());
        };
        let reason = decline["reason"].as_str();
        if !seen_regions.insert(region_id)
            || decline["actor_count"].as_u64() != Some(owner.actors.len() as u64)
            || decline["reset_count"].as_u64() != Some(owner.resets.len() as u64)
            || !matches!(reason, Some("actor_event_plan_unsupported" | "reset_event_plan_unsupported"))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ReadFamily {
    Actor,
    Reset,
}

fn validate_reads(reads: &[Value], graph: &GraphTablesDto, family: ReadFamily) -> Result<(), ()> {
    if reads.len() > crate::redpiler::program_graph::MAX_ACTOR_INPUTS {
        return Err(());
    }
    for read in reads {
        let kind = read["kind"].as_str().ok_or(())?;
        let valid = match (family, kind) {
            (ReadFamily::Actor, "power") => graph.power_links.iter().any(|link| read["link"] == link.id),
            (
                ReadFamily::Actor,
                "extended" | "base_pending" | "ready" | "lane_available" | "retraction_available" | "early_retraction_available" | "early_resource",
            ) => true,
            (ReadFamily::Reset, "reset_powered" | "reset_pending" | "reset_qualifies") => true,
            _ => false,
        };
        if !valid {
            return Err(());
        }
    }
    Ok(())
}

fn validate_routines(values: &Value, read_count: usize, expected: &[(&str, usize)]) -> Result<(), ()> {
    let routines = values.as_array().ok_or(())?;
    if routines.len() != expected.len() {
        return Err(());
    }
    for (routine, (event, root_count)) in routines.iter().zip(expected) {
        if routine["event"] != *event {
            return Err(());
        }
        let expressions = routine["expressions"].as_array().ok_or(())?;
        if expressions.len() > crate::redpiler::program_graph::MAX_ACTOR_OPS {
            return Err(());
        }
        let mut ops = Vec::new();
        for (index, expression) in expressions.iter().enumerate() {
            let args: Vec<_> = expression["args"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .map(|value| value.as_u64().and_then(|id| usize::try_from(id).ok()).filter(|&id| id < index).ok_or(()))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let op = match (expression["op"].as_str(), args.as_slice()) {
                (Some("input"), []) => {
                    let input = expression["input"].as_u64().and_then(|id| usize::try_from(id).ok()).ok_or(())?;
                    if input >= read_count {
                        return Err(());
                    }
                    Op::Input(input)
                }
                (Some("constant"), []) => Op::Const(expression["value"].as_bool().ok_or(())?),
                (Some("buffer"), [a]) => Op::Buffer(*a),
                (Some("not"), [a]) => Op::Not(*a),
                (Some("and"), [a, b]) => Op::And(*a, *b),
                (Some("or"), [a, b]) => Op::Or(*a, *b),
                (Some("xor"), [a, b]) => Op::Xor(*a, *b),
                (Some("select"), [c, a, b]) => Op::Select(*c, *a, *b),
                _ => return Err(()),
            };
            ops.push(op);
        }
        let roots = routine["roots"]
            .as_array()
            .ok_or(())?
            .iter()
            .map(|root| root.as_u64().and_then(|id| usize::try_from(id).ok()).ok_or(()))
            .collect::<Result<Vec<_>, _>>()?;
        if roots.len() != *root_count {
            return Err(());
        }
        mchprs_redpiler_opt::reference(&mchprs_redpiler_opt::Circuit::new((0..read_count).collect(), ops, roots)).map_err(|_| ())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{GraphTablesDto, InstantRegionDto, NodeDto, NodeKindDto};
    use super::*;

    fn routine(event: &str, roots: usize) -> Value {
        json!({
            "event": event,
            "expressions": [{"op": "constant", "value": false}],
            "roots": vec![0; roots],
        })
    }

    #[test]
    fn grouped_region_export_rejects_a_reset_facing_that_disagrees_with_its_owner() {
        let graph = GraphTablesDto {
            nodes: vec![NodeDto {
                id: "node:0".into(),
                kind: NodeKindDto::Observer {
                    watched: "site:0".into(),
                    facing: "north".into(),
                    builtin_reset: None,
                },
                position: None,
                output_route: None,
            }],
            profiles: vec![],
            sites: vec![],
            guards: vec![],
            contacts: vec![],
            pistons: vec![],
            resources: vec![],
            power_links: vec![],
            wire_plans: vec![],
            dust_paths: vec![],
            wire_relations: vec![],
            comparator_plans: vec![],
            callback_links: vec![],
            routes: None,
            instant_regions: vec![InstantRegionDto {
                stable_inputs: true,
                wires: vec![],
                id: "instant_region:0".into(),
                actors: vec!["piston:0".into()],
                resources: vec![],
                resets: vec!["node:0".into()],
                contacts: vec![],
                readable_sites: vec![],
                power_inputs: vec![],
                power_readers: vec![],
                routes: vec![],
                incoming: vec![],
                outgoing: vec![],
                wire_readers: vec![],
                observers: vec![],
            }],
            instant_proxies: vec![],
            symbolic_regions: vec![],
            symbolic_region_declines: vec![],
            instant_machine: None,
            instant_kernel: None,
        };
        let actor = json!({
            "actor": "piston:0",
            "reads": [],
            "routines": [routine("sample", 3), routine("accept", 3)],
        });
        let reset = json!({
            "node": "node:0",
            "facing": "north",
            "reads": [],
            "routines": [routine("update", 1), routine("tick", 2)],
        });
        let mut region = json!({
            "region": "instant_region:0",
            "boundary": {"inputs": [], "state": [], "outputs": [], "effects": [], "projection_roots": []},
            "actors": [actor],
            "resets": [reset],
        });
        assert!(validate(&[region.clone()], &[], &graph).is_ok());

        region["resets"][0]["facing"] = json!("south");
        assert!(validate(&[region], &[], &graph).is_err());
    }
}
