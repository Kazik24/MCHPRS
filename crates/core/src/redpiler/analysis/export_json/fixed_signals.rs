//! Keep executable live-state reads distinct from non-executable settled equations.
use super::*;

#[derive(Debug, Serialize)]
pub(super) struct QueryDto {
    id: String,
    semantics: &'static str,
    to: PowerTargetDto,
    constant: u8,
    inputs: Vec<InputDto>,
}

#[derive(Debug, Serialize)]
struct InputDto {
    node: String,
    attenuation: u8,
    require_cross: bool,
}

pub(super) fn export(graph: &CompileGraph) -> Vec<QueryDto> {
    graph.plans.fixed_signals.live.iter().map(|query| ("live_state", query))
        .chain(graph.plans.fixed_signals.traced.iter().map(|query| ("settled_provenance", query)))
        .enumerate().map(|(index, (semantics, query))| QueryDto {
            id: typed_id("fixed_signal", index), semantics,
            to: match query.target {
                PowerTarget::Input { node, port } => PowerTargetDto::Input { node: typed_id("node", node.index()), port: input_port(port) },
                PowerTarget::Conductor { site, include_dust } => PowerTargetDto::Conductor { site: typed_id("site", site.index()), include_dust },
            },
            constant: query.constant,
            inputs: query.inputs.iter().map(|input| InputDto {
                node: typed_id("node", input.node.index()), attenuation: input.attenuation, require_cross: input.require_cross,
            }).collect(),
        }).collect()
}

pub(super) fn validate(queries: &[QueryDto], nodes: &HashSet<String>, sites: &HashSet<String>) -> Result<(), ExportError> {
    unique_ids("fixed_signal", queries.iter().map(|query| query.id.as_str()))?;
    for query in queries {
        if query.constant > 15 || !matches!(query.semantics, "live_state" | "settled_provenance") {
            return Err(ExportError::new("validation", "invalid fixed signal equation"));
        }
        match &query.to {
            PowerTargetDto::Input { node, .. } => check_ref(nodes, node, "fixed signal target")?,
            PowerTargetDto::Conductor { site, .. } => check_ref(sites, site, "fixed signal conductor")?,
        }
        for input in &query.inputs {
            check_ref(nodes, &input.node, "fixed signal input")?;
            if input.attenuation >= 15 {
                return Err(ExportError::new("validation", "invalid fixed signal attenuation"));
            }
        }
    }
    Ok(())
}
