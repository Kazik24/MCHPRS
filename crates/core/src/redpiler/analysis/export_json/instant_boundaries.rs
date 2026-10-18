//! Cold inspection tables for the experimental boundary catalog; no runtime state is copied.
use super::*;
use crate::redpiler::program_graph::{BoundaryDirection, BoundaryOwner, BoundaryPortKind, CellDeclineReason, ComparatorPortRole, FixedReadRole};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Tables {
    /// These rows identify the actors, payloads and reset nodes admitted by each cell.
    pub(super) cells: Vec<Value>,
    /// These rows identify connecting wire/conductor groups without merging cell ownership.
    pub(super) corridors: Vec<Value>,
    /// These rows retain separate input/output bindings to original graph witnesses.
    pub(super) ports: Vec<Value>,
    /// These rows account for actors retained outside the candidate cells.
    pub(super) declines: Vec<Value>,
}

pub(super) fn export(graph: &CompileGraph) -> Tables {
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Tables::default();
    };
    Tables {
        cells: catalog.cells.iter().enumerate().map(|(index, cell)| {
            let mut resets: Vec<_> = cell.uses.iter().map(|use_| use_.reset).collect();
            resets.sort_unstable();
            resets.dedup();
            json!({"id":typed_id("instant_cell",index),
                "actors":cell.uses.iter().map(|use_|typed_id("piston",use_.actor.index())).collect::<Vec<_>>(),
                "resources":cell.resources.iter().map(|resource|typed_id("resource",resource.resource.index())).collect::<Vec<_>>(),
                "resets":resets.iter().map(|reset|typed_id("node",reset.index())).collect::<Vec<_>>()})
        }).collect(),
        corridors: catalog.corridors.iter().enumerate().map(|(index, corridor)| json!({
            "id":typed_id("instant_corridor",index),
            "wires":corridor.wires.iter().map(|wire|typed_id("node",wire.index())).collect::<Vec<_>>(),
            "conductive_sites":corridor.conductive_sites.iter().map(|site|typed_id("site",site.index())).collect::<Vec<_>>()
        })).collect(),
        ports: catalog.ports.iter().enumerate().map(|(index, port)| json!({
            "id":typed_id("instant_port",index),
            "owner":match port.owner { BoundaryOwner::Cell(index)=>typed_id("instant_cell",index as usize),BoundaryOwner::Corridor(index)=>typed_id("instant_corridor",index as usize) },
            "direction":match port.direction { BoundaryDirection::Input=>"input",BoundaryDirection::Output=>"output" },
            "binding":binding(port.kind)
        })).collect(),
        declines: catalog.declines.iter().enumerate().map(|(index, decline)|json!({
            "id":typed_id("instant_decline",index),
            "actors":decline.actors.iter().map(|actor|typed_id("piston",actor.index())).collect::<Vec<_>>(),
            "reason":decline_reason(decline.reason)
        })).collect(),
    }
}

fn occurrence(value: crate::redpiler::program_graph::RouteOccurrence) -> Value {
    json!({"route":typed_id("route",value.route.index()),"ordinal":value.ordinal})
}

fn binding(kind: BoundaryPortKind) -> Value {
    match kind {
        BoundaryPortKind::Power { link } => json!({"kind":"power","link":typed_id("power_link",link as usize)}),
        BoundaryPortKind::Callback { occurrence: row } => json!({"kind":"callback","occurrence":occurrence(row)}),
        BoundaryPortKind::WireRelation { relation } => json!({"kind":"wire_relation","relation":typed_id("wire_relation",relation as usize)}),
        BoundaryPortKind::Comparator { plan, role } => {
            json!({"kind":"comparator","plan":typed_id("comparator_plan",plan as usize),"role":match role {
                ComparatorPortRole::Rear=>"rear",ComparatorPortRole::DirectOverride=>"direct_override",
                ComparatorPortRole::FarOverrideSite=>"far_override_site",ComparatorPortRole::FarOverrideGuard=>"far_override_guard",
                ComparatorPortRole::FarOverrideOutput=>"far_override_output"
            }})
        }
        BoundaryPortKind::GuardSite { guard, site, view, test } => {
            let expression = serde_json::to_value(guard_dto(&Guard::Site { site, view, test })).expect("guard DTO serialization is infallible");
            json!({"kind":"guard_site","guard":typed_id("guard",guard.index()),"site":typed_id("site",site.index()),"view":operand_view(view),"test":expression["test"]})
        }
        BoundaryPortKind::GuardState { guard, test } => {
            let expression = serde_json::to_value(guard_dto(&Guard::State(test))).expect("guard DTO serialization is infallible");
            json!({"kind":"guard_state","guard":typed_id("guard",guard.index()),"test":expression["test"]})
        }
        BoundaryPortKind::ContactAlternative { contact, ordinal } => {
            json!({"kind":"contact_alternative","contact":typed_id("contact",contact.index()),"ordinal":ordinal})
        }
        BoundaryPortKind::ObserverWatch { observer, site } => {
            json!({"kind":"observer_watch","observer":typed_id("node",observer.index()),"site":typed_id("site",site.index())})
        }
        BoundaryPortKind::CapturedWire {
            wire,
            occurrence: row,
            neighbor,
        } => {
            json!({"kind":"captured_wire","wire":typed_id("node",wire.index()),"occurrence":occurrence(row),"neighbor":typed_id("site",neighbor.index())})
        }
        BoundaryPortKind::WireNeighborhood { wire, neighbor } => {
            json!({"kind":"wire_neighborhood","wire":typed_id("node",wire.index()),"neighbor":typed_id("site",neighbor.index())})
        }
        BoundaryPortKind::FixedRead { node, role, site } => {
            json!({"kind":"fixed_read","node":typed_id("node",node.index()),"site":typed_id("site",site.index()),"role":match role {
                FixedReadRole::NoteSupport=>"note_support",FixedReadRole::NoteObstruction=>"note_obstruction"
            }})
        }
    }
}

fn decline_reason(reason: CellDeclineReason) -> Value {
    match reason {
        CellDeclineReason::UnrecognizedActor(actor) => json!({"code":"unrecognized_actor","piston":typed_id("piston",actor.index())}),
        CellDeclineReason::DifferentPullSources(resource) => {
            json!({"code":"different_pull_sources","resource":typed_id("resource",resource.index())})
        }
        CellDeclineReason::NoPullSource(resource) => json!({"code":"no_pull_source","resource":typed_id("resource",resource.index())}),
        CellDeclineReason::MissingDeferredWake(actor) => json!({"code":"missing_deferred_wake","piston":typed_id("piston",actor.index())}),
        CellDeclineReason::MissingResetWake(actor) => json!({"code":"missing_reset_wake","piston":typed_id("piston",actor.index())}),
        CellDeclineReason::PinTableBudgetExceeded => json!({"code":"pin_table_budget_exceeded"}),
        CellDeclineReason::ConflictingSourceContact(contact) => {
            json!({"code":"conflicting_source_contact","contact":typed_id("contact",contact.index())})
        }
        CellDeclineReason::UnsupportedMaterial(resource) => json!({"code":"unsupported_material","resource":typed_id("resource",resource.index())}),
    }
}

fn invalid(message: &str) -> ExportError {
    ExportError::new("instant_boundaries", message)
}

/// Validate IDs and ownership here; the draft validator independently rebuilds semantic witnesses.
pub(super) fn validate(graph: &GraphTablesDto) -> Result<(), ExportError> {
    let tables = [
        ("instant_cell", &graph.instant_cells),
        ("instant_corridor", &graph.instant_corridors),
        ("instant_port", &graph.instant_boundary_ports),
        ("instant_decline", &graph.instant_declines),
    ];
    if tables.iter().all(|(_, rows)| rows.is_empty()) {
        return Ok(());
    }
    if !graph.instant_regions.is_empty() || !graph.instant_proxies.is_empty() {
        return Err(invalid("boundary catalog and legacy regions cannot both own the graph"));
    }
    let mut ids = HashSet::new();
    macro_rules! add_ids {
        ($field:ident) => {
            ids.extend(graph.$field.iter().map(|row| row.id.as_str()));
        };
    }
    add_ids!(nodes);
    add_ids!(sites);
    add_ids!(profiles);
    add_ids!(guards);
    add_ids!(contacts);
    add_ids!(pistons);
    add_ids!(resources);
    add_ids!(power_links);
    add_ids!(wire_relations);
    add_ids!(comparator_plans);
    if let Some(routes) = &graph.routes {
        ids.extend(routes.iter().map(|row| row.id.as_str()));
    }
    for (prefix, rows) in &tables {
        let mut own = HashSet::new();
        for row in *rows {
            let id = row["id"].as_str().ok_or_else(|| invalid("boundary row has no string ID"))?;
            let suffix = id
                .strip_prefix(&format!("{prefix}:"))
                .ok_or_else(|| invalid("boundary row has the wrong ID kind"))?;
            if suffix.parse::<usize>().is_err() || (suffix.len() > 1 && suffix.starts_with('0')) || !own.insert(id) {
                return Err(invalid("boundary row has a malformed or duplicate ID"));
            }
            ids.insert(id);
        }
    }
    for (_, rows) in &tables {
        for row in *rows {
            validate_references(row, &ids)?;
        }
    }
    let nodes: HashMap<_, _> = graph.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut actors = HashSet::new();
    let mut resources = HashSet::new();
    let mut resets = HashSet::new();
    for cell in &graph.instant_cells {
        for actor in array(cell, "actors")? {
            insert_owner(actor, "piston:", &mut actors)?;
        }
        for resource in array(cell, "resources")? {
            insert_owner(resource, "resource:", &mut resources)?;
        }
        for reset in array(cell, "resets")? {
            let reset = insert_owner(reset, "node:", &mut resets)?;
            if !matches!(nodes[reset].kind, NodeKindDto::Observer { .. }) {
                return Err(invalid("cell reset is not an observer"));
            }
        }
    }
    let mut wires = HashSet::new();
    let mut sites = HashSet::new();
    let classic_wires: HashSet<_> = graph
        .classic_nets
        .iter()
        .flat_map(|net| net["wires"].as_array().into_iter().flatten())
        .filter_map(|wire| wire["node"].as_str())
        .collect();
    for corridor in &graph.instant_corridors {
        for wire in array(corridor, "wires")? {
            let wire = insert_owner(wire, "node:", &mut wires)?;
            if !matches!(nodes[wire].kind, NodeKindDto::Wire { .. }) || classic_wires.contains(wire) {
                return Err(invalid("corridor contains a non-wire or classic-owned wire"));
            }
        }
        for site in array(corridor, "conductive_sites")? {
            insert_owner(site, "site:", &mut sites)?;
        }
    }
    let mut declined = HashSet::new();
    for decline in &graph.instant_declines {
        for actor in array(decline, "actors")? {
            let actor = insert_owner(actor, "piston:", &mut declined)?;
            if actors.contains(actor) {
                return Err(invalid("declined actor is also admitted"));
            }
        }
        if !decline["reason"]["code"].is_string() {
            return Err(invalid("decline has no reason code"));
        }
    }
    for port in &graph.instant_boundary_ports {
        let owner = port["owner"].as_str().ok_or_else(|| invalid("port has no owner"))?;
        if !(owner.starts_with("instant_cell:") || owner.starts_with("instant_corridor:"))
            || !matches!(port["direction"].as_str(), Some("input" | "output"))
        {
            return Err(invalid("port owner or direction is invalid"));
        }
        validate_binding(&port["binding"], graph)?;
    }
    Ok(())
}

fn array<'a>(row: &'a Value, field: &str) -> Result<&'a [Value], ExportError> {
    row[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("boundary field must be an array"))
}

fn insert_owner<'a>(value: &'a Value, prefix: &str, owners: &mut HashSet<&'a str>) -> Result<&'a str, ExportError> {
    let id = value
        .as_str()
        .filter(|id| id.starts_with(prefix))
        .ok_or_else(|| invalid("boundary owner has the wrong reference kind"))?;
    if !owners.insert(id) {
        return Err(invalid("boundary ownership overlaps"));
    }
    Ok(id)
}

fn validate_references(value: &Value, ids: &HashSet<&str>) -> Result<(), ExportError> {
    match value {
        Value::String(id) if id.contains(':') => {
            if !ids.contains(id.as_str()) {
                return Err(invalid("boundary table has a dangling reference"));
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_references(value, ids)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_references(value, ids)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_binding(value: &Value, graph: &GraphTablesDto) -> Result<(), ExportError> {
    let reference = |field: &str, prefix: &str| {
        value[field]
            .as_str()
            .filter(|id| id.starts_with(prefix))
            .ok_or_else(|| invalid("boundary binding has the wrong reference kind"))
    };
    match value["kind"].as_str() {
        Some("power") => {
            reference("link", "power_link:")?;
        }
        Some("wire_relation") => {
            reference("relation", "wire_relation:")?;
        }
        Some("callback") => {
            validate_occurrence(&value["occurrence"], graph)?;
        }
        Some("comparator") => {
            reference("plan", "comparator_plan:")?;
            if !matches!(
                value["role"].as_str(),
                Some("rear" | "direct_override" | "far_override_site" | "far_override_guard" | "far_override_output")
            ) {
                return Err(invalid("unknown comparator boundary role"));
            }
        }
        Some("guard_site" | "guard_state") => {
            let id = reference("guard", "guard:")?;
            let guard = graph
                .guards
                .iter()
                .find(|guard| guard.id == id)
                .ok_or_else(|| invalid("missing boundary guard"))?;
            let guard = serde_json::to_value(guard).map_err(|_| invalid("cannot inspect boundary guard"))?;
            let expected_kind = if value["kind"] == "guard_site" { "site" } else { "state" };
            if guard["kind"] != expected_kind || guard["test"] != value["test"] {
                return Err(invalid("boundary predicate differs from its guard atom"));
            }
            if expected_kind == "site" && (guard["site"] != value["site"] || guard["view"] != value["view"]) {
                return Err(invalid("boundary site predicate differs from its guard atom"));
            }
        }
        Some("contact_alternative") => {
            let id = reference("contact", "contact:")?;
            let contact = graph
                .contacts
                .iter()
                .find(|contact| contact.id == id)
                .ok_or_else(|| invalid("missing contact alternative"))?;
            if value["ordinal"].as_u64().is_none_or(|ordinal| ordinal >= contact.variants.len() as u64) {
                return Err(invalid("contact alternative ordinal is out of range"));
            }
        }
        Some("observer_watch") => {
            let id = reference("observer", "node:")?;
            let node = graph
                .nodes
                .iter()
                .find(|node| node.id == id)
                .ok_or_else(|| invalid("missing observer boundary"))?;
            if !matches!(&node.kind,NodeKindDto::Observer{watched,..} if value["site"]==watched.as_str()) {
                return Err(invalid("observer boundary does not match its watched site"));
            }
        }
        Some("captured_wire" | "wire_neighborhood") => {
            let id = reference("wire", "node:")?;
            let wire = graph
                .wire_plans
                .iter()
                .find(|wire| wire.node == id)
                .ok_or_else(|| invalid("boundary wire has no neighborhood"))?;
            if !wire.canonical_neighbors.iter().any(|site| value["neighbor"] == site.as_str()) {
                return Err(invalid("boundary wire neighbor is not canonical"));
            }
            if value["kind"] == "captured_wire" {
                let delivery = validate_occurrence(&value["occurrence"], graph)?;
                if wire.turbo_route.as_deref() != value["occurrence"]["route"].as_str()
                    || delivery.view != "captured"
                    || value["neighbor"] != delivery.target
                {
                    return Err(invalid("captured wire binding differs from its Turbo delivery"));
                }
            }
        }
        Some("fixed_read") => {
            let id = reference("node", "node:")?;
            reference("site", "site:")?;
            if !graph.nodes.iter().any(|node| {
                node.id == id
                    && matches!(
                        node.kind,
                        NodeKindDto::Electrical {
                            component: ElectricalKindDto::NoteBlock { .. }
                        }
                    )
            }) {
                return Err(invalid("fixed read is not owned by a note block"));
            }
            if !matches!(value["role"].as_str(), Some("note_support" | "note_obstruction")) {
                return Err(invalid("unknown fixed boundary read"));
            }
        }
        _ => return Err(invalid("unknown boundary binding kind")),
    }
    Ok(())
}

fn validate_occurrence<'a>(value: &Value, graph: &'a GraphTablesDto) -> Result<&'a RouteDeliveryDto, ExportError> {
    let route = value["route"].as_str().ok_or_else(|| invalid("callback occurrence has no route"))?;
    let route = graph
        .routes
        .as_ref()
        .and_then(|routes| routes.iter().find(|row| row.id == route))
        .ok_or_else(|| invalid("callback occurrence has no route"))?;
    let ordinal = value["ordinal"]
        .as_u64()
        .and_then(|ordinal| usize::try_from(ordinal).ok())
        .ok_or_else(|| invalid("invalid callback occurrence ordinal"))?;
    route
        .deliveries
        .get(ordinal)
        .ok_or_else(|| invalid("callback occurrence ordinal is out of range"))
}
