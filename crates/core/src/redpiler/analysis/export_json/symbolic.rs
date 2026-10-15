//! Inspection of symbolic event routines; expression roots retain their effect order.
use super::*;
use crate::redpiler::program_graph::{ActorRead, SymbolicActor};
use mchprs_redpiler_opt::Op;
use serde_json::{Value, json};

pub(super) fn export(actor: &SymbolicActor, graph: &CompileGraph) -> Value {
    let reads: Vec<_> = actor
        .reads
        .iter()
        .map(|read| match read {
            ActorRead::Power(link) => json!({"kind": "power", "link": typed_id("power_link",
            graph.power_links.iter().position(|candidate| candidate == link).expect("validated power binding"))}),
            ActorRead::Guard(guard) => json!({"kind": "guard", "guard": typed_id("guard", guard.index())}),
            _ => json!({"kind": read_name(*read)}),
        })
        .collect();
    let routines: Vec<_> = actor
        .routines
        .iter()
        .zip(["sample", "accept", "reset_update", "reset_tick"])
        .map(|(routine, event)| {
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
        })
        .collect();
    json!({"actor": typed_id("piston", actor.actor.index()), "reset": typed_id("node", actor.reset.index()),
        "boundary": super::machine::boundary_json(&actor.boundary), "reads": reads, "routines": routines})
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
        ActorRead::ResetPowered => "reset_powered",
        ActorRead::ResetPending => "reset_pending",
        ActorRead::ResetQualifies => "reset_qualifies",
        ActorRead::Power(_) | ActorRead::Guard(_) => unreachable!("typed references are exported separately"),
    }
}

pub(super) fn validate(values: &[Value], graph: &GraphTablesDto) -> Result<(), ExportError> {
    let invalid = || ExportError::new("validation", "invalid symbolic actor event program");
    let mut seen = HashSet::new();
    for actor in values {
        let owner = actor["actor"].as_str().ok_or_else(invalid)?;
        if !seen.insert(owner) || !graph.pistons.iter().any(|item| item.id == owner) || !graph.nodes.iter().any(|item| actor["reset"] == item.id) {
            return Err(invalid());
        }
        let reads = actor["reads"].as_array().ok_or_else(invalid)?;
        if reads.len() > crate::redpiler::program_graph::MAX_ACTOR_INPUTS {
            return Err(invalid());
        }
        for read in reads {
            let kind = read["kind"].as_str().ok_or_else(invalid)?;
            let valid = match kind {
                "power" => graph.power_links.iter().any(|link| read["link"] == link.id),
                "guard" => graph.guards.iter().any(|guard| read["guard"] == guard.id),
                "extended"
                | "base_pending"
                | "ready"
                | "lane_available"
                | "retraction_available"
                | "early_retraction_available"
                | "early_resource"
                | "reset_powered"
                | "reset_pending"
                | "reset_qualifies" => true,
                _ => false,
            };
            if !valid {
                return Err(invalid());
            }
        }
        let routines = actor["routines"].as_array().ok_or_else(invalid)?;
        if routines.len() != 4 {
            return Err(invalid());
        }
        for (routine, (event, root_count)) in routines
            .iter()
            .zip([("sample", 3), ("accept", 3), ("reset_update", 1), ("reset_tick", 2)])
        {
            if routine["event"] != event {
                return Err(invalid());
            }
            let expressions = routine["expressions"].as_array().ok_or_else(invalid)?;
            if expressions.len() > crate::redpiler::program_graph::MAX_ACTOR_OPS {
                return Err(invalid());
            }
            let mut ops = Vec::new();
            for (index, expression) in expressions.iter().enumerate() {
                let args: Vec<_> = expression["args"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| {
                                value
                                    .as_u64()
                                    .and_then(|id| usize::try_from(id).ok())
                                    .filter(|&id| id < index)
                                    .ok_or_else(invalid)
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let op = match (expression["op"].as_str(), args.as_slice()) {
                    (Some("input"), []) => Op::Input(expression["input"].as_u64().and_then(|id| usize::try_from(id).ok()).ok_or_else(invalid)?),
                    (Some("constant"), []) => Op::Const(expression["value"].as_bool().ok_or_else(invalid)?),
                    (Some("buffer"), [a]) => Op::Buffer(*a),
                    (Some("not"), [a]) => Op::Not(*a),
                    (Some("and"), [a, b]) => Op::And(*a, *b),
                    (Some("or"), [a, b]) => Op::Or(*a, *b),
                    (Some("xor"), [a, b]) => Op::Xor(*a, *b),
                    (Some("select"), [c, a, b]) => Op::Select(*c, *a, *b),
                    _ => return Err(invalid()),
                };
                ops.push(op);
            }
            let roots = routine["roots"]
                .as_array()
                .ok_or_else(invalid)?
                .iter()
                .map(|root| root.as_u64().and_then(|id| usize::try_from(id).ok()).ok_or_else(invalid))
                .collect::<Result<Vec<_>, _>>()?;
            if roots.len() != root_count {
                return Err(invalid());
            }
            mchprs_redpiler_opt::reference(&mchprs_redpiler_opt::Circuit::new((0..reads.len()).collect(), ops, roots)).map_err(|_| invalid())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{
        PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    };
    use crate::world::storage::Chunk;

    #[test]
    fn symbolic_event_export_preserves_program_and_rejects_malformed_references() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/INSTANT_OBSERVER.schem");
        let clip = load_schematic(std::io::Cursor::new(std::fs::read(path).unwrap())).unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..2).flat_map(|x| (0..2).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(
            &mut world,
            &clip,
            BlockPos::new(4 + clip.offset_x, 30 + clip.offset_y, 4 + clip.offset_z),
            false,
        );
        let options = CompilerOptions {
            optimization_level: Some(4),
            stable_instant_inputs: true,
            ..Default::default()
        };
        let mut export = super::super::prepare(
            &world,
            (
                BlockPos::new(4, 30, 4),
                BlockPos::new(4 + clip.size_x as i32 - 1, 30 + clip.size_y as i32 - 1, 4 + clip.size_z as i32 - 1),
            ),
            &[],
            &options,
            &TaskMonitor::default(),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert_eq!(export.graph.symbolic_actors.len(), 1);
        assert!(export.graph.instant_machine.is_none());
        export.to_bytes().unwrap();
        let valid = export.graph.symbolic_actors[0].clone();
        let mut bad = valid.clone();
        bad["actor"] = json!("piston:999999");
        assert!(validate(&[bad], &export.graph).is_err());
        let mut bad = valid.clone();
        bad["routines"][0]["roots"][0] = json!(999999);
        assert!(validate(&[bad], &export.graph).is_err());
        let mut bad = valid.clone();
        bad["routines"][0]["expressions"][0] = json!({"op": "not", "args": [0]});
        assert!(validate(&[bad], &export.graph).is_err());
        let mut bad = valid.clone();
        bad["reads"][0] = json!({"kind": "power", "link": "power_link:999999"});
        export.graph.symbolic_actors = vec![bad];
        assert!(export.to_bytes().is_err());
        assert!(validate(&[valid.clone(), valid], &export.graph).is_err());
    }
}
