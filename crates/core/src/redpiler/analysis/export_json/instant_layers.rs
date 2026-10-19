//! Cold inspection tables for causal-layer analysis; these rows never enable execution.
//! Expression, input and use indices are region-local; pin indices are cell-local and validated in their owner context.
use super::*;
use crate::redpiler::program_graph::{
    BoundaryPortKind, CausalLayer, LayerDecline, LayerDeclineReason, LayerExpr, LayerInputRole, LayerInputSource, LayerLifecycleOccurrence,
    LayerLifecyclePhase, LayerLifecycleReason, LayerOrder, LayerResourceOutput, LayerWitness, LayeredRegion, StateTest, UseCause,
};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Tables {
    pub(super) regions: Vec<Value>,
    pub(super) declines: Vec<Value>,
}

pub(super) fn export(graph: &CompileGraph) -> Tables {
    let Some(plan) = &graph.plans.instant.layers else {
        return Tables::default();
    };
    Tables {
        regions: plan.regions.iter().map(|row| region(row, graph)).collect(),
        declines: plan
            .declines
            .iter()
            .enumerate()
            .map(|(index, decline)| decline_row(index, decline, graph))
            .collect(),
    }
}

fn region(value: &LayeredRegion, graph: &CompileGraph) -> Value {
    json!({
        "id": typed_id("instant_layer_region", value.id.0 as usize),
        "stationary_entry": value.stationary_entry.0,
        "cells": value.cells.iter().map(|cell| typed_id("instant_cell", *cell as usize)).collect::<Vec<_>>(),
        "corridors": value.corridors.iter().map(|corridor| typed_id("instant_corridor", *corridor as usize)).collect::<Vec<_>>(),
        "resource_outputs": value.resource_outputs.iter().map(resource_output).collect::<Vec<_>>(),
        "cold_lifecycle_occurrences": value.cold_lifecycle_occurrences.iter().map(lifecycle_occurrence).collect::<Vec<_>>(),
        "inputs": value.inputs.iter().map(|input| json!({
            "source": input_source(input.source, graph),
            "role": input_role(input.role),
        })).collect::<Vec<_>>(),
        "outputs": value.outputs.iter().map(|output| json!({
            "port": typed_id("instant_port", output.port as usize),
            "emitted_when": output.emitted_when.0,
        })).collect::<Vec<_>>(),
        "graph_outputs": value.graph_outputs.iter().map(|port| typed_id("instant_port", *port as usize)).collect::<Vec<_>>(),
        "expressions": value.expressions.iter().map(expression).collect::<Vec<_>>(),
        "pin_values": value.pin_values.iter().map(|pin| json!({
            "cell": typed_id("instant_cell", pin.cell as usize),
            "pin": pin.pin.0,
            "value": pin.value.0,
        })).collect::<Vec<_>>(),
        "layers": value.layers.iter().map(layer).collect::<Vec<_>>(),
    })
}

fn input_source(value: LayerInputSource, graph: &CompileGraph) -> Value {
    match value {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let port = match port {
                crate::redpiler::program_graph::PowerPort::Output => json!({"kind":"output"}),
                crate::redpiler::program_graph::PowerPort::Stored => json!({"kind":"stored"}),
                crate::redpiler::program_graph::PowerPort::Weak(direction) => json!({"kind":"weak","face":face(direction)}),
                crate::redpiler::program_graph::PowerPort::Strong(direction) => json!({"kind":"strong","face":face(direction)}),
                crate::redpiler::program_graph::PowerPort::Conducted { include_dust } => json!({"kind":"conducted","include_dust":include_dust}),
            };
            json!({"kind":"source_above","site":typed_id("site",site.index()),"port":port,"threshold":threshold})
        }
        LayerInputSource::BoundaryEvent { port, occurrence } => json!({
            "kind":"boundary_event",
            "port":typed_id("instant_port",port as usize),
            "occurrence":occurrence_dto(occurrence),
        }),
        LayerInputSource::BoundaryRequest { port, occurrence, actor } => json!({
            "kind":"boundary_request", "port":typed_id("instant_port",port as usize),
            "occurrence":occurrence_dto(occurrence), "actor":typed_id("piston",actor.index()),
        }),
        LayerInputSource::BoundaryState { port, test } => json!({
            "kind":"boundary_state", "port":typed_id("instant_port",port as usize),
            "test":serde_json::to_value(state_test_dto(test)).expect("state test DTO serialization is infallible"),
        }),
        LayerInputSource::PowerContribution { port, link } => {
            let index = graph
                .plans
                .instant
                .boundaries
                .as_ref()
                .and_then(|catalog| catalog.ports.get(port as usize))
                .and_then(|row| match row.kind {
                    BoundaryPortKind::Power { link } => Some(link as usize),
                    _ => None,
                })
                .filter(|index| graph.power_links.get(*index).is_some_and(|candidate| *candidate == link))
                .expect("layer power input matches its catalog power port");
            json!({"kind":"power_contribution","port":typed_id("instant_port",port as usize),"link":typed_id("power_link",index)})
        }
        LayerInputSource::LocalResetOutput { observer, link } => {
            let index = graph
                .power_links
                .iter()
                .position(|candidate| *candidate == link)
                .expect("local reset input retains an original power link");
            json!({
                "kind":"local_reset_output",
                "observer":typed_id("node", observer.index()),
                "link":typed_id("power_link", index),
            })
        }
        LayerInputSource::State(test) => json!({
            "kind":"state",
            "test":serde_json::to_value(state_test_dto(test)).expect("state test DTO serialization is infallible"),
        }),
    }
}

fn expression(value: &LayerExpr) -> Value {
    match value {
        LayerExpr::Constant(value) => json!({"kind":"constant","value":value}),
        LayerExpr::Input(input) => json!({"kind":"input","input":input.0}),
        LayerExpr::Not(value) => json!({"kind":"not","value":value.0}),
        LayerExpr::All(values) => json!({"kind":"all","values":values.iter().map(|value|value.0).collect::<Vec<_>>()}),
        LayerExpr::Any(values) => json!({"kind":"any","values":values.iter().map(|value|value.0).collect::<Vec<_>>()}),
    }
}

fn layer(value: &CausalLayer) -> Value {
    json!({
        "order": match value.order { LayerOrder::Fifo => "fifo" },
        "uses": value.uses.iter().map(|use_| json!({
            "use_id": use_.use_id.0,
            "actor": typed_id("piston", use_.actor.index()),
            "requested": use_.requested.0,
            "accepted": use_.accepted.0,
            "consumed_before": use_.consumed_before.0,
            "consumed_after": use_.consumed_after.0,
            "cause": cause(&use_.cause),
        })).collect::<Vec<_>>(),
    })
}

fn cause(value: &UseCause) -> Value {
    json!({
        "kind":"boundary",
        "occurrences":value.boundary_occurrences.iter().map(|occurrence|occurrence_dto(*occurrence)).collect::<Vec<_>>()
    })
}

fn resource_output(value: &LayerResourceOutput) -> Value {
    json!({
        "cell":typed_id("instant_cell", value.cell as usize),
        "resource":typed_id("resource", value.resource.index()),
        "source":typed_id("contact", value.source.index()),
        "destination":typed_id("contact", value.destination.index()),
        "transfer_when":value.transfer_when.0,
        "source_absent_after":value.source_absent_after.0,
    })
}

fn lifecycle_occurrence(value: &LayerLifecycleOccurrence) -> Value {
    let (reason, extra) = match value.reason {
        LayerLifecycleReason::InertCellCallback => ("inert_cell_callback", Value::Null),
        LayerLifecycleReason::ResetObserverUpdate { observer } => ("reset_observer_update", json!({"observer":typed_id("node", observer.index())})),
        LayerLifecycleReason::OrdinaryGraphLifecycle => ("ordinary_graph_lifecycle", Value::Null),
    };
    let mut row = json!({
        "actor":typed_id("piston", value.actor.index()),
        "occurrence":occurrence_dto(value.occurrence),
        "phase":lifecycle_phase(value.phase),
        "reason":{"kind":reason},
    });
    if let Some(fields) = extra.as_object() {
        for (field, value) in fields {
            row["reason"][field] = value.clone();
        }
    }
    row
}

fn lifecycle_phase(value: LayerLifecyclePhase) -> &'static str {
    match value {
        LayerLifecyclePhase::RetractionBaseChanged => "retraction_base_changed",
        LayerLifecyclePhase::RetractionFarChanged => "retraction_far_changed",
        LayerLifecyclePhase::RetractionNearChanged => "retraction_near_changed",
        LayerLifecyclePhase::DeferredBaseRestoration => "deferred_base_restoration",
        LayerLifecyclePhase::DeferredHeadRestoration => "deferred_head_restoration",
        LayerLifecyclePhase::DeferredResourceRestoration => "deferred_resource_restoration",
        LayerLifecyclePhase::ResetObserverLifecycle => "reset_observer_lifecycle",
        LayerLifecyclePhase::ResetOutput => "reset_output",
        LayerLifecyclePhase::ResetFarShapeChange => "reset_far_shape_change",
    }
}

fn state_test_dto(test: StateTest) -> StateTestDto {
    match test {
        StateTest::PistonExtended(piston) => StateTestDto::PistonExtended {
            piston: typed_id("piston", piston.index()),
        },
        StateTest::PistonHeadPresent(piston) => StateTestDto::PistonHeadPresent {
            piston: typed_id("piston", piston.index()),
        },
        StateTest::PistonAvailability { piston, is } => StateTestDto::PistonAvailability {
            piston: typed_id("piston", piston.index()),
            availability: piston_availability(is),
        },
        StateTest::ResourceAt { resource, contact } => StateTestDto::ResourceAt {
            resource: typed_id("resource", resource.index()),
            contact: typed_id("contact", contact.index()),
        },
        StateTest::ResourceEnabled(resource) => StateTestDto::ResourceEnabled {
            resource: typed_id("resource", resource.index()),
        },
    }
}

fn occurrence_dto(value: crate::redpiler::program_graph::RouteOccurrence) -> Value {
    json!({"route":typed_id("route",value.route.index()),"ordinal":value.ordinal})
}

fn input_role(value: LayerInputRole) -> &'static str {
    match value {
        LayerInputRole::RequestSample => "request_sample",
        LayerInputRole::ActivationEvent => "activation_event",
        LayerInputRole::AcceptanceRevalidation => "acceptance_revalidation",
    }
}

fn decline_row(index: usize, value: &LayerDecline, graph: &CompileGraph) -> Value {
    json!({
        "id": typed_id("instant_layer_decline", index),
        "cells": value.cells.iter().map(|cell| typed_id("instant_cell", *cell as usize)).collect::<Vec<_>>(),
        "corridors": value.corridors.iter().map(|corridor| typed_id("instant_corridor", *corridor as usize)).collect::<Vec<_>>(),
        "witness": witness(value.witness, graph),
        "reason": decline_reason(value.reason),
    })
}

fn witness(value: LayerWitness, graph: &CompileGraph) -> Value {
    match value {
        LayerWitness::Actor(actor) => json!({"kind":"actor","piston":typed_id("piston",actor.index())}),
        LayerWitness::Callback(occurrence) => json!({"kind":"callback","occurrence":occurrence_dto(occurrence)}),
        LayerWitness::Power(link) => {
            let index = graph
                .power_links
                .iter()
                .position(|candidate| *candidate == link)
                .expect("layer power witness comes from the graph");
            json!({"kind":"power","link":typed_id("power_link",index)})
        }
        LayerWitness::PowerAtCut { actor, link, phase } => {
            let index = graph
                .power_links
                .iter()
                .position(|candidate| *candidate == link)
                .expect("layer power witness comes from the graph");
            json!({
                "kind":"power_at_cut","actor":typed_id("piston",actor.index()),
                "link":typed_id("power_link",index),"phase":input_role(phase)
            })
        }
        LayerWitness::Wire(node) => json!({"kind":"wire","node":typed_id("node",node.index())}),
    }
}

fn decline_reason(value: LayerDeclineReason) -> &'static str {
    match value {
        LayerDeclineReason::ProofBudgetExceeded => "proof_budget_exceeded",
        LayerDeclineReason::SignalArrivalUnproven => "signal_arrival_unproven",
        LayerDeclineReason::CallbackSemanticsUnsupported => "callback_semantics_unsupported",
        LayerDeclineReason::SharedOwnerOrderUnknown => "shared_owner_order_unknown",
        LayerDeclineReason::BoundaryEffectUnsupported => "boundary_effect_unsupported",
    }
}

fn invalid(message: &str) -> ExportError {
    ExportError::new("instant_layers", message)
}

/// Check typed graph references and every region-local index without interpreting runtime effects.
pub(super) fn validate(graph: &GraphTablesDto) -> Result<(), ExportError> {
    if graph.instant_layer_regions.is_empty() && graph.instant_layer_declines.is_empty() {
        return Ok(());
    }

    let mut ids = HashSet::new();
    macro_rules! add_ids {
        ($rows:expr) => {
            ids.extend($rows.iter().map(|row| row.id.as_str()));
        };
    }
    add_ids!(graph.nodes);
    add_ids!(graph.profiles);
    add_ids!(graph.sites);
    add_ids!(graph.guards);
    add_ids!(graph.contacts);
    add_ids!(graph.pistons);
    add_ids!(graph.resources);
    add_ids!(graph.power_links);
    add_ids!(graph.wire_plans);
    add_ids!(graph.dust_paths);
    add_ids!(graph.wire_relations);
    add_ids!(graph.comparator_plans);
    add_ids!(graph.callback_links);
    ids.extend(graph.instant_cells.iter().filter_map(|row| row["id"].as_str()));
    ids.extend(graph.instant_corridors.iter().filter_map(|row| row["id"].as_str()));
    ids.extend(graph.instant_boundary_ports.iter().filter_map(|row| row["id"].as_str()));
    if let Some(routes) = &graph.routes {
        ids.extend(routes.iter().map(|row| row.id.as_str()));
    }

    let layer_tables = [
        ("instant_layer_region", &graph.instant_layer_regions),
        ("instant_layer_decline", &graph.instant_layer_declines),
    ];
    for (prefix, rows) in layer_tables {
        let mut own = HashSet::new();
        for row in rows {
            let id = row["id"].as_str().ok_or_else(|| invalid("layer row has no string ID"))?;
            let suffix = id
                .strip_prefix(&format!("{prefix}:"))
                .ok_or_else(|| invalid("layer row has the wrong ID kind"))?;
            if suffix.parse::<usize>().is_err() || (suffix.len() > 1 && suffix.starts_with('0')) || !own.insert(id) {
                return Err(invalid("layer row has a malformed or duplicate ID"));
            }
            if ids.contains(id) {
                return Err(invalid("layer row ID collides with a graph ID"));
            }
            ids.insert(id);
        }
    }

    let cell_rows: HashMap<_, _> = graph
        .instant_cells
        .iter()
        .filter_map(|row| row["id"].as_str().map(|id| (id, row)))
        .collect();
    let corridor_rows: HashMap<_, _> = graph
        .instant_corridors
        .iter()
        .filter_map(|row| row["id"].as_str().map(|id| (id, row)))
        .collect();
    let ports: HashMap<_, _> = graph
        .instant_boundary_ports
        .iter()
        .filter_map(|row| row["id"].as_str().map(|id| (id, row)))
        .collect();
    let nodes: HashMap<_, _> = graph.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let sites: HashMap<_, _> = graph.sites.iter().map(|site| (site.id.as_str(), site)).collect();
    let guards: HashMap<_, _> = graph.guards.iter().map(|guard| (guard.id.as_str(), guard)).collect();
    let pistons: HashSet<_> = graph.pistons.iter().map(|piston| piston.id.as_str()).collect();
    let piston_rows: HashMap<_, _> = graph.pistons.iter().map(|piston| (piston.id.as_str(), piston)).collect();
    let contacts: HashMap<_, _> = graph.contacts.iter().map(|contact| (contact.id.as_str(), contact)).collect();
    let links: HashMap<_, _> = graph.power_links.iter().map(|link| (link.id.as_str(), link)).collect();
    let routes: HashMap<_, _> = graph
        .routes
        .as_ref()
        .into_iter()
        .flatten()
        .map(|route| (route.id.as_str(), route))
        .collect();

    let mut owned_cells = HashSet::new();
    let mut owned_corridors = HashSet::new();
    for row in &graph.instant_layer_regions {
        let id = row["id"].as_str().ok_or_else(|| invalid("layer region has no ID"))?;
        let region_cells = owned_refs(row, "cells", "instant_cell", &cell_rows, &mut owned_cells)?;
        let region_corridors = owned_refs(row, "corridors", "instant_corridor", &corridor_rows, &mut owned_corridors)?;
        let mut region_actors = HashSet::new();
        let mut region_resources = HashSet::new();
        let mut region_resets = HashSet::new();
        for cell in &region_cells {
            for actor in array(cell_rows[*cell], "actors")? {
                let actor = actor.as_str().ok_or_else(|| invalid("catalog cell actor is malformed"))?;
                region_actors.insert(actor);
            }
            region_resources.extend(array(cell_rows[*cell], "resources")?.iter().filter_map(Value::as_str));
            region_resets.extend(array(cell_rows[*cell], "resets")?.iter().filter_map(Value::as_str));
        }
        let region_actor_nodes: HashSet<_> = region_actors
            .iter()
            .filter_map(|actor| piston_rows.get(actor).map(|piston| piston.node.as_str()))
            .collect();

        let inputs = array(row, "inputs")?;
        for input in inputs {
            if !matches!(
                input["role"].as_str(),
                Some("request_sample" | "activation_event" | "acceptance_revalidation")
            ) {
                return Err(invalid("layer input has an invalid role"));
            }
            let source = &input["source"];
            match source["kind"].as_str() {
                Some("source_above") => {
                    let site = graph
                        .sites
                        .iter()
                        .find(|site| source["site"] == site.id)
                        .ok_or_else(|| invalid("threshold input references a missing site"))?;
                    if !source["threshold"].as_u64().is_some_and(|value| value < 15) {
                        return Err(invalid("threshold input is outside four-bit strength"));
                    }
                    let port = &source["port"];
                    let valid = match port["kind"].as_str() {
                        Some("output" | "stored") => true,
                        Some("weak" | "strong") => matches!(port["face"].as_str(), Some("top" | "bottom" | "north" | "south" | "east" | "west")),
                        _ => false,
                    };
                    if !valid {
                        return Err(invalid("threshold input has an invalid directional port"));
                    }
                    if let SiteSourceDto::Component { node } = &site.source {
                        for corridor in &region_corridors {
                            if array(corridor_rows[*corridor], "wires")?.iter().any(|wire| wire == node) {
                                return Err(invalid("threshold input exposes an owned internal wire"));
                            }
                        }
                    }
                }
                Some("boundary_event" | "boundary_request" | "power_contribution") => {
                    let port = source["port"].as_str().ok_or_else(|| invalid("layer input has no port"))?;
                    let row_port = ports.get(port).ok_or_else(|| invalid("layer input references a missing port"))?;
                    if row_port["direction"] != "input" || !region_owns(row_port["owner"].as_str(), &region_cells, &region_corridors) {
                        return Err(invalid("layer input port is outside its input boundary"));
                    }
                    if source["kind"] == "boundary_event" || source["kind"] == "boundary_request" {
                        let (route, ordinal) = occurrence(&source["occurrence"], &routes)?;
                        if row_port["binding"]["kind"] != "callback" || row_port["binding"]["occurrence"] != source["occurrence"] {
                            return Err(invalid("event input does not match its callback boundary"));
                        }
                        if source["kind"] == "boundary_request" {
                            let actor = source["actor"].as_str().ok_or_else(|| invalid("queued boundary request has no actor"))?;
                            let delivery = &routes[route.as_str()].deliveries[ordinal as usize];
                            let owner_matches = row_port["owner"]
                                .as_str()
                                .and_then(|owner| cell_rows.get(owner))
                                .and_then(|cell| cell["actors"].as_array())
                                .is_some_and(|actors| actors.iter().any(|candidate| candidate == actor));
                            let receiver_matches = piston_rows.get(actor).is_some_and(|piston| {
                                queued_receiver_matches(
                                    delivery.receiver.as_ref(),
                                    &sites[delivery.target.as_str()].source,
                                    actor,
                                    &piston.node,
                                    &contacts,
                                )
                            });
                            if !region_actors.contains(actor)
                                || !owner_matches
                                || !receiver_matches
                                || delivery.kind != "update"
                                || input["role"] != "activation_event"
                            {
                                return Err(invalid(&format!(
                                    "queued boundary request does not match its owned actor callback: actor={actor}, route={route}:{ordinal}, receiver={:?}",
                                    delivery.receiver
                                )));
                            }
                        }
                    } else {
                        let link = source["link"].as_str().ok_or_else(|| invalid("power input has no link"))?;
                        if row_port["binding"]["kind"] != "power"
                            || row_port["binding"]["link"] != link
                            || !graph.power_links.iter().any(|row| row.id == link)
                        {
                            return Err(invalid("power input does not match its power boundary"));
                        }
                    }
                }
                Some("boundary_state") => {
                    let test = &source["test"];
                    validate_state_test(test, &ids)?;
                    let port = source["port"]
                        .as_str()
                        .and_then(|id| ports.get(id))
                        .ok_or_else(|| invalid("boundary state port is missing"))?;
                    if port["direction"] != "input"
                        || !region_owns(port["owner"].as_str(), &region_cells, &region_corridors)
                        || port["binding"]["kind"] != "guard_state"
                        || port["binding"]["test"] != *test
                    {
                        return Err(invalid("boundary state does not match its declared input guard"));
                    }
                }
                Some("state") => {
                    let test = &source["test"];
                    validate_state_test(test, &ids)?;
                    match test["kind"].as_str() {
                        Some("piston_extended" | "piston_head_present" | "piston_availability") => {
                            if !region_actors.contains(test["piston"].as_str().unwrap_or_default()) {
                                return Err(invalid("state input reads an actor outside its cells"));
                            }
                        }
                        Some("resource_at" | "resource_enabled") => {
                            let resource = test["resource"].as_str().unwrap_or_default();
                            if !region_resources.contains(resource) {
                                return Err(invalid("state input reads a resource outside its cells"));
                            }
                            if let Some(contact) = test["contact"].as_str() {
                                let resource_row = graph
                                    .resources
                                    .iter()
                                    .find(|row| row.id == resource)
                                    .ok_or_else(|| invalid("state input resource is missing"))?;
                                if !resource_row.contacts.iter().any(|candidate| candidate == contact) {
                                    return Err(invalid("state input reads a resource at a contact it does not own"));
                                }
                            }
                        }
                        _ => return Err(invalid("state input has an unknown test")),
                    }
                }
                Some("local_reset_output") => {
                    let observer_id = source["observer"].as_str().ok_or_else(|| invalid("reset source has no observer"))?;
                    let observer = nodes
                        .get(observer_id)
                        .ok_or_else(|| invalid("reset source references a missing observer"))?;
                    if !region_resets.contains(observer_id) || !matches!(&observer.kind, NodeKindDto::Observer { .. }) {
                        return Err(invalid("reset source observer is outside the region resets"));
                    }
                    let link_id = source["link"].as_str().ok_or_else(|| invalid("reset source has no power link"))?;
                    let link = links
                        .get(link_id)
                        .ok_or_else(|| invalid("reset source references a missing power link"))?;
                    let site = sites
                        .get(link.from.site.as_str())
                        .ok_or_else(|| invalid("reset source power site is missing"))?;
                    let guard = guards.get(link.guard.as_str()).ok_or_else(|| invalid("reset source guard is missing"))?;
                    let source_node_matches = matches!(&site.source, SiteSourceDto::Component { node } if node == observer_id);
                    let source_port_matches = matches!(&link.from.port, PowerPortDto::Output)
                        || matches!(&link.from.port, PowerPortDto::Strong { face } if face == "bottom");
                    let target_matches = match &link.to {
                        PowerTargetDto::Input { node, port } => *port == "quasi" && region_actor_nodes.contains(node.as_str()),
                        PowerTargetDto::Conductor { site, .. } => {
                            matches!(&link.from.port, PowerPortDto::Strong { face } if face == "bottom")
                                && link.face == "bottom"
                                && sites
                                    .get(site.as_str())
                                    .is_some_and(|site| matches!(&site.source, SiteSourceDto::Constant { .. }))
                        }
                    };
                    if !source_node_matches
                        || !source_port_matches
                        || link.attenuation != 0
                        || !matches!(&guard.expression, GuardExpressionDto::Always)
                        || !target_matches
                    {
                        return Err(invalid("reset source does not match its original quasi link"));
                    }
                }
                _ => return Err(invalid("layer input has an invalid source")),
            }
        }
        for output in array(row, "outputs")? {
            let port = output["port"].as_str().ok_or_else(|| invalid("layer output has no port"))?;
            let row_port = ports.get(port).ok_or_else(|| invalid("layer output references a missing port"))?;
            if row_port["direction"] != "output" || !region_owns(row_port["owner"].as_str(), &region_cells, &region_corridors) {
                return Err(invalid("layer output port is outside its output boundary"));
            }
            root(output["emitted_when"].as_u64(), array(row, "expressions")?.len(), "layer output")?;
        }
        if let Some(observations) = row.get("graph_outputs") {
            let observations = observations.as_array().ok_or_else(|| invalid("Graph outputs must be an array"))?;
            let mut seen = HashSet::new();
            for observation in observations {
                let id = observation.as_str().ok_or_else(|| invalid("Graph output has no port"))?;
                let port = ports.get(id).ok_or_else(|| invalid("Graph output references a missing port"))?;
                if !seen.insert(id)
                    || port["direction"] != "output"
                    || !region_owns(port["owner"].as_str(), &region_cells, &region_corridors)
                    || !matches!(
                        port["binding"]["kind"].as_str(),
                        Some("guard_site" | "wire_relation" | "wire_neighborhood" | "captured_wire")
                    )
                {
                    return Err(invalid("Graph output is not a unique geometry observation on this boundary"));
                }
            }
        }
        let resource_outputs = array(row, "resource_outputs")?;
        if resource_outputs.is_empty() {
            return Err(invalid("compiled layer region must expose resource transfer obligations"));
        }
        for output in resource_outputs {
            let cell = output["cell"].as_str().ok_or_else(|| invalid("resource output has no cell"))?;
            let resource = output["resource"].as_str().ok_or_else(|| invalid("resource output has no resource"))?;
            let source = output["source"]
                .as_str()
                .ok_or_else(|| invalid("resource output has no source contact"))?;
            let destination = output["destination"]
                .as_str()
                .ok_or_else(|| invalid("resource output has no destination contact"))?;
            let cell_row = cell_rows.get(cell).ok_or_else(|| invalid("resource output references a missing cell"))?;
            let resource_row = graph
                .resources
                .iter()
                .find(|row| row.id == resource)
                .ok_or_else(|| invalid("resource output references a missing resource"))?;
            if !region_cells.contains(cell)
                || source == destination
                || !array(cell_row, "resources")?.iter().any(|value| value == resource)
                || !resource_row.contacts.iter().any(|contact| contact == source)
                || !resource_row.contacts.iter().any(|contact| contact == destination)
            {
                return Err(invalid("resource output is outside its cell or resource"));
            }
        }

        let expressions = array(row, "expressions")?;
        for (index, expression) in expressions.iter().enumerate() {
            match expression["kind"].as_str() {
                Some("constant") if expression["value"].is_boolean() => {}
                Some("input") => local_index(expression["input"].as_u64(), inputs.len(), "layer expression input")?,
                Some("not") => local_index(expression["value"].as_u64(), index, "layer expression operand")?,
                Some("all" | "any") => {
                    for operand in array(expression, "values")? {
                        local_index(operand.as_u64(), index, "layer expression operand")?;
                    }
                }
                _ => return Err(invalid("layer expression is malformed or has an unknown kind")),
            }
        }
        root(row["stationary_entry"].as_u64(), expressions.len(), "layer stationary entry")?;
        for output in resource_outputs {
            root(output["transfer_when"].as_u64(), expressions.len(), "resource transfer predicate")?;
            root(output["source_absent_after"].as_u64(), expressions.len(), "resource vacancy predicate")?;
        }
        for pin in array(row, "pin_values")? {
            let cell = pin["cell"].as_str().ok_or_else(|| invalid("layer pin value has no cell"))?;
            if !region_cells.contains(&cell) || !pin["pin"].as_u64().is_some_and(|pin| pin <= u32::MAX as u64) {
                return Err(invalid("layer pin value is outside its cell"));
            }
            root(pin["value"].as_u64(), expressions.len(), "layer pin value")?;
        }

        let mut use_actors = HashMap::new();
        let mut use_occurrences = HashSet::new();
        for layer in array(row, "layers")? {
            if layer["order"] != "fifo" {
                return Err(invalid("layer has an invalid order"));
            }
            for use_row in array(layer, "uses")? {
                let use_id = use_row["use_id"]
                    .as_u64()
                    .filter(|value| *value <= u32::MAX as u64)
                    .ok_or_else(|| invalid("layer use has an invalid local ID"))? as u32;
                let actor = use_row["actor"].as_str().ok_or_else(|| invalid("layer use has no actor"))?;
                if !pistons.contains(actor) || !region_actors.contains(actor) {
                    return Err(invalid("layer use actor is outside its region"));
                }
                if use_actors.insert(use_id, actor).is_some_and(|previous| previous != actor) {
                    return Err(invalid("layer use ID changes actor"));
                }
                for field in ["requested", "accepted", "consumed_before", "consumed_after"] {
                    root(use_row[field].as_u64(), expressions.len(), "layer use")?;
                }
                let occurrence_key = validate_cause(&use_row["cause"], &routes)?;
                // A source row can run again at another FIFO prefix with a different request or live state.
                let prefix = ["requested", "accepted", "consumed_before", "consumed_after"].map(|field| use_row[field].as_u64());
                if !use_occurrences.insert((use_id, actor, occurrence_key, prefix)) {
                    return Err(invalid("layer repeats an exact use occurrence"));
                }
            }
        }
        for lifecycle in array(row, "cold_lifecycle_occurrences")? {
            let actor = lifecycle["actor"].as_str().ok_or_else(|| invalid("lifecycle row has no actor"))?;
            if !region_actors.contains(actor) || !pistons.contains(actor) {
                return Err(invalid("lifecycle actor is outside its cells"));
            }
            let (route, _) = occurrence(&lifecycle["occurrence"], &routes)?;
            let phase = lifecycle["phase"].as_str().ok_or_else(|| invalid("lifecycle row has no phase"))?;
            let reason = lifecycle["reason"]["kind"]
                .as_str()
                .ok_or_else(|| invalid("lifecycle row has no reason"))?;
            let reset_reason = reason == "reset_observer_update";
            let valid_reason = match phase {
                "retraction_base_changed" | "retraction_far_changed" | "retraction_near_changed" => reason == "inert_cell_callback",
                "reset_observer_lifecycle" => reset_reason,
                "reset_output" | "reset_far_shape_change" => reason == "ordinary_graph_lifecycle",
                "deferred_base_restoration" | "deferred_head_restoration" | "deferred_resource_restoration" => reason == "ordinary_graph_lifecycle",
                _ => false,
            };
            if !valid_reason {
                return Err(invalid("lifecycle row has an invalid phase and reason"));
            }
            if reset_reason {
                let observer_id = lifecycle["reason"]["observer"]
                    .as_str()
                    .ok_or_else(|| invalid("reset lifecycle reason has no observer"))?;
                let observer = nodes.get(observer_id).ok_or_else(|| invalid("reset lifecycle observer is missing"))?;
                if !region_resets.contains(observer_id) || !matches!(&observer.kind, NodeKindDto::Observer { .. }) {
                    return Err(invalid("reset lifecycle observer is outside its resets"));
                }
            }
            let actor_routes = piston_rows.get(actor).and_then(|piston| piston.routes.as_ref());
            let route_matches = match phase {
                "retraction_base_changed" => actor_routes.is_some_and(|routes| routes.base_changed == route),
                "retraction_far_changed" => actor_routes.is_some_and(|routes| routes.far_changed.as_deref() == Some(route.as_str())),
                "retraction_near_changed" => actor_routes.is_some_and(|routes| routes.near_changed == route),
                "deferred_base_restoration" => actor_routes.is_some_and(|routes| routes.base_restored == route),
                "deferred_head_restoration" => actor_routes.is_some_and(|routes| routes.head_restored == route),
                "deferred_resource_restoration" => actor_routes.is_some_and(|routes| {
                    routes.resource_restored_near.as_deref() == Some(route.as_str())
                        || routes.resource_restored_far.as_deref() == Some(route.as_str())
                }),
                "reset_far_shape_change" => {
                    actor_routes.is_some_and(|routes| routes.far_shape_changed.as_deref() == Some(route.as_str()))
                        && piston_rows.get(actor).is_some_and(|piston| {
                            routes
                                .get(route.as_str())
                                .is_some_and(|route_row| route_row.source.as_str() == piston.node.as_str())
                        })
                }
                "reset_output" => region_resets.iter().any(|observer_id| {
                    nodes.get(observer_id).is_some_and(|observer| {
                        matches!(&observer.kind, NodeKindDto::Observer { .. })
                            && observer.output_route.as_deref() == Some(route.as_str())
                            && routes
                                .get(route.as_str())
                                .is_some_and(|route_row| route_row.source.as_str() == *observer_id)
                    })
                }),
                "reset_observer_lifecycle" => lifecycle["reason"]["observer"].as_str().is_some_and(|observer_id| {
                    nodes.get(observer_id).is_some_and(|observer| {
                        let NodeKindDto::Observer { watched, .. } = &observer.kind else {
                            return false;
                        };
                        sites.get(watched.as_str()).is_some_and(|site| {
                            let SiteSourceDto::Contact { contact } = &site.source else {
                                return false;
                            };
                            graph.contacts.iter().find(|row| row.id == *contact).is_some_and(|contact| {
                                contact.variants.iter().any(|variant| {
                                    matches!(&variant.receiver, Some(ReceiverDto::PistonBase { piston })
                                    if piston_rows.get(piston.as_str()).and_then(|piston| piston.routes.as_ref()).is_some_and(|routes| {
                                        routes.base_changed == route || routes.base_restored == route
                                    }))
                                })
                            })
                        })
                    })
                }),
                _ => false,
            };
            if !route_matches {
                return Err(invalid("lifecycle occurrence does not match its declared route phase"));
            }
        }
        let _ = id;
    }

    for row in &graph.instant_layer_declines {
        let declined_cells = owned_refs(row, "cells", "instant_cell", &cell_rows, &mut owned_cells)?;
        let declined_corridors = owned_refs(row, "corridors", "instant_corridor", &corridor_rows, &mut owned_corridors)?;
        let mut actors = HashSet::new();
        let mut wires = HashSet::new();
        for cell in &declined_cells {
            actors.extend(array(cell_rows[*cell], "actors")?.iter().filter_map(Value::as_str));
        }
        for corridor in &declined_corridors {
            wires.extend(array(corridor_rows[*corridor], "wires")?.iter().filter_map(Value::as_str));
        }
        let reason = row["reason"].as_str().ok_or_else(|| invalid("layer decline has no reason"))?;
        if !matches!(
            reason,
            "signal_arrival_unproven"
                | "callback_semantics_unsupported"
                | "shared_owner_order_unknown"
                | "boundary_effect_unsupported"
                | "proof_budget_exceeded"
        ) {
            return Err(invalid("layer decline has an unknown reason"));
        }
        validate_witness(&row["witness"], &actors, &wires, &nodes, &routes, &graph.power_links)?;
    }
    Ok(())
}

fn queued_receiver_matches(
    receiver: Option<&ReceiverDto>,
    fallback: &SiteSourceDto,
    actor: &str,
    actor_node: &str,
    contacts: &HashMap<&str, &ContactDto>,
) -> bool {
    let mut pending: Vec<_> = receiver.into_iter().collect();
    let mut visited = HashSet::new();
    if receiver.is_none() {
        match fallback {
            SiteSourceDto::Component { node } => return node == actor_node,
            SiteSourceDto::Contact { contact } => {
                visited.insert(contact.as_str());
                if let Some(contact) = contacts.get(contact.as_str()) {
                    pending.extend(contact.variants.iter().filter_map(|variant| variant.receiver.as_ref()));
                }
            }
            _ => {}
        }
    }
    while let Some(receiver) = pending.pop() {
        match receiver {
            ReceiverDto::PistonBase { piston } | ReceiverDto::PistonHead { piston } if piston == actor => return true,
            ReceiverDto::Component { node } if node == actor_node => return true,
            ReceiverDto::Contact { contact } if visited.insert(contact.as_str()) => {
                if let Some(contact) = contacts.get(contact.as_str()) {
                    pending.extend(contact.variants.iter().filter_map(|variant| variant.receiver.as_ref()));
                }
            }
            _ => {}
        }
    }
    false
}

fn array<'a>(row: &'a Value, field: &str) -> Result<&'a [Value], ExportError> {
    row[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("layer field must be an array"))
}

fn validate_state_test(value: &Value, ids: &HashSet<&str>) -> Result<(), ExportError> {
    let reference = |field: &str, prefix: &str| {
        value[field]
            .as_str()
            .filter(|id| id.starts_with(&format!("{prefix}:")) && ids.contains(id))
            .ok_or_else(|| invalid("state input has a missing or mistyped reference"))
    };
    match value["kind"].as_str() {
        Some("piston_extended" | "piston_head_present") => {
            reference("piston", "piston")?;
        }
        Some("piston_availability") => {
            reference("piston", "piston")?;
            if !matches!(
                value["availability"].as_str(),
                Some("ready" | "head_pending" | "publishing_head_removal" | "base_pending_head_present" | "base_pending")
            ) {
                return Err(invalid("state input has an invalid piston availability"));
            }
        }
        Some("resource_at") => {
            reference("resource", "resource")?;
            reference("contact", "contact")?;
        }
        Some("resource_enabled") => {
            reference("resource", "resource")?;
        }
        _ => return Err(invalid("state input has an unknown test")),
    }
    Ok(())
}

fn owned_refs<'a>(
    row: &'a Value,
    field: &str,
    prefix: &str,
    available: &HashMap<&'a str, &'a Value>,
    owned: &mut HashSet<&'a str>,
) -> Result<HashSet<&'a str>, ExportError> {
    let mut local = HashSet::new();
    for item in array(row, field)? {
        let id = item.as_str().ok_or_else(|| invalid("layer ownership reference must be a string"))?;
        if !id.starts_with(&format!("{prefix}:")) || !available.contains_key(id) || !local.insert(id) || !owned.insert(id) {
            return Err(invalid("layer region ownership is missing, duplicated, or overlapping"));
        }
    }
    Ok(local)
}

fn region_owns(owner: Option<&str>, cells: &HashSet<&str>, corridors: &HashSet<&str>) -> bool {
    owner.is_some_and(|owner| cells.contains(owner) || corridors.contains(owner))
}

fn root(index: Option<u64>, len: usize, owner: &str) -> Result<(), ExportError> {
    local_index(index, len, owner)
}

fn local_index(index: Option<u64>, len: usize, owner: &str) -> Result<(), ExportError> {
    if index.is_some_and(|index| index < len as u64) {
        Ok(())
    } else {
        Err(invalid(owner))
    }
}

fn validate_cause<'a>(value: &Value, routes: &HashMap<&'a str, &'a RouteDto>) -> Result<Vec<(String, u64)>, ExportError> {
    match value["kind"].as_str() {
        Some("boundary") => {
            let mut occurrences = Vec::new();
            for raw in array(value, "occurrences")? {
                let (route, ordinal) = occurrence(raw, routes)?;
                occurrences.push((route, ordinal));
            }
            if occurrences.is_empty() {
                Err(invalid("boundary cause has no occurrences"))
            } else {
                Ok(occurrences)
            }
        }
        _ => Err(invalid("layer use has an invalid cause")),
    }
}

fn occurrence<'a>(value: &Value, routes: &HashMap<&'a str, &'a RouteDto>) -> Result<(String, u64), ExportError> {
    let route = value["route"].as_str().ok_or_else(|| invalid("route occurrence has no route"))?;
    let ordinal = value["ordinal"].as_u64().ok_or_else(|| invalid("route occurrence has no ordinal"))?;
    if !routes.get(route).is_some_and(|route| ordinal < route.deliveries.len() as u64) {
        return Err(invalid("route occurrence is outside its delivery table"));
    }
    Ok((route.to_owned(), ordinal))
}

fn validate_witness(
    value: &Value,
    actors: &HashSet<&str>,
    wires: &HashSet<&str>,
    nodes: &HashMap<&str, &NodeDto>,
    routes: &HashMap<&str, &RouteDto>,
    power_links: &[PowerLinkDto],
) -> Result<(), ExportError> {
    match value["kind"].as_str() {
        Some("actor") => {
            let actor = value["piston"].as_str().ok_or_else(|| invalid("actor witness has no piston"))?;
            if !actors.contains(actor) {
                return Err(invalid("actor witness is outside the declined cells"));
            }
        }
        Some("callback") => {
            occurrence(&value["occurrence"], routes)?;
        }
        Some("power") => {
            let link = value["link"].as_str().ok_or_else(|| invalid("power witness has no link"))?;
            if !power_links.iter().any(|row| row.id == link) {
                return Err(invalid("power witness references a missing link"));
            }
        }
        Some("power_at_cut") => {
            let actor = value["actor"].as_str().ok_or_else(|| invalid("power witness has no actor"))?;
            let link = value["link"].as_str().ok_or_else(|| invalid("power witness has no link"))?;
            if !actors.contains(actor)
                || !power_links.iter().any(|row| row.id == link)
                || !matches!(
                    value["phase"].as_str(),
                    Some("request_sample" | "activation_event" | "acceptance_revalidation")
                )
            {
                return Err(invalid("power-at-cut witness has invalid references or phase"));
            }
        }
        Some("wire") => {
            let node = value["node"].as_str().ok_or_else(|| invalid("wire witness has no node"))?;
            if !wires.contains(node) || !nodes.get(node).is_some_and(|node| matches!(&node.kind, NodeKindDto::Wire { .. })) {
                return Err(invalid("wire witness is not a wire node"));
            }
        }
        _ => return Err(invalid("layer decline has an invalid witness")),
    }
    Ok(())
}
