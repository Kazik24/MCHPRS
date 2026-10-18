//! Inspection export for the fixed-classic executable tables.
use super::*;
use crate::redpiler::program_graph::{
    FixedClassicKind, FixedClassicOpKind, FixedClassicPlans, FixedClassicProgram, FixedClassicSite, FixedClassicSource,
};
use std::mem::size_of;

#[derive(Debug, Serialize)]
pub(super) struct PlanDto {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inventory: Option<InventoryDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    program: Option<ProgramDto>,
}

#[derive(Debug, Serialize)]
struct InventoryDto {
    tables: Vec<TableInventoryDto>,
    program_object_bytes: usize,
    table_payload_bytes: usize,
    accounted_bytes: usize,
    excludes: &'static str,
}

#[derive(Debug, Serialize)]
struct TableInventoryDto {
    table: &'static str,
    entries: usize,
    payload_bytes: usize,
}

#[derive(Debug, Serialize)]
struct ProgramDto {
    nodes: Vec<NodeDto>,
    sites: Vec<SiteDto>,
    wires: Vec<WireDto>,
    route_ranges: Vec<RouteRangeDto>,
    route_ops: Vec<RouteOpDto>,
    operands: Vec<OperandDto>,
}

#[derive(Debug, Serialize)]
struct NodeDto {
    node: String,
    kind: KindDto,
    inputs: Vec<NamedQueryDto>,
    output_route: Option<String>,
    comparator: Option<ComparatorDto>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum KindDto {
    Wire { wire_index: u32 },
    Torch,
    Repeater { delay: u8, facing_diode: bool },
    Comparator { mode: &'static str, facing_diode: bool },
    Lamp,
    Hopper,
    Lever,
    Button,
    PressurePlate,
    Constant,
}

#[derive(Debug, Serialize)]
struct ComparatorDto {
    direct_override: Option<ComparatorOverrideDto>,
    far_override: Option<ComparatorOverrideDto>,
    far_threshold: u8,
}

#[derive(Debug, Serialize)]
struct SiteDto {
    site: String,
    #[serde(flatten)]
    source: SiteKindDto,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SiteKindDto {
    Inert,
    Wire { node: String, wire_index: u32 },
    Gate { node: String },
}

#[derive(Debug, Serialize)]
struct WireDto {
    node: String,
    root: String,
    /// Canonical offsets in `world::wire_cache::positions()` order.
    aliases: [NeighborAliasDto; 24],
    seed_mask: u32,
    turbo_mask: u32,
    admitted_mask: u32,
    raw_sides: RawSidesDto,
    no_dust: Vec<FaceQueryDto>,
    shape_route: Option<String>,
}

#[derive(Debug, Serialize)]
struct NeighborAliasDto {
    slot: u8,
    offset: [i8; 3],
    site: String,
}

#[derive(Debug, Serialize)]
struct RawSidesDto {
    packed: u8,
    order: [&'static str; 4],
    side_ids: [u8; 4],
}

#[derive(Debug, Serialize)]
struct RouteRangeDto {
    route: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
struct RouteOpDto {
    index: usize,
    route: String,
    ordinal: u32,
    target: String,
    #[serde(flatten)]
    operation: OperationDto,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OperationDto {
    Seed,
    Shape { direction: String },
    Update { direction: Option<String> },
}

#[derive(Debug, Serialize)]
struct OperandDto {
    index: usize,
    #[serde(flatten)]
    source: OperandSourceDto,
    attenuation: u8,
    require_cross: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
enum OperandSourceDto {
    LiveOutput { node: String },
    CapturedPowered { site: String, node: String },
}

#[derive(Debug, Serialize)]
struct NamedQueryDto {
    port: &'static str,
    #[serde(flatten)]
    query: QueryDto,
}

#[derive(Debug, Serialize)]
struct FaceQueryDto {
    face: String,
    #[serde(flatten)]
    query: QueryDto,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct QueryDto {
    constant: u8,
    start: u32,
    end: u32,
}

const NEIGHBOR_OFFSETS: [[i8; 3]; 24] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-2, 0, 0],
    [-1, -1, 0],
    [-1, 1, 0],
    [-1, 0, -1],
    [-1, 0, 1],
    [2, 0, 0],
    [1, -1, 0],
    [1, 1, 0],
    [1, 0, -1],
    [1, 0, 1],
    [0, -2, 0],
    [0, -1, -1],
    [0, -1, 1],
    [0, 2, 0],
    [0, 1, -1],
    [0, 1, 1],
    [0, 0, -2],
    [0, 0, 2],
];

pub(super) fn export(plans: &FixedClassicPlans) -> Option<PlanDto> {
    if plans.program.is_none() && plans.decline.is_none() {
        return None;
    }
    let program = plans.program.as_deref();
    Some(PlanDto {
        status: if program.is_some() { "compiled" } else { "declined" },
        decline: plans.decline.clone(),
        inventory: program.map(inventory),
        program: program.map(program_dto),
    })
}

fn inventory(program: &FixedClassicProgram) -> InventoryDto {
    inventory_counts(
        program.nodes.len(),
        program.sites.len(),
        program.wires.len(),
        program.routes.len(),
        program.ops.len(),
        program.operands.len(),
    )
}

fn inventory_counts(nodes: usize, sites: usize, wires: usize, routes: usize, ops: usize, operands: usize) -> InventoryDto {
    let tables = vec![
        table("nodes", nodes, size_of::<crate::redpiler::program_graph::FixedClassicNode>()),
        table("sites", sites, size_of::<FixedClassicSite>()),
        table("wires", wires, size_of::<crate::redpiler::program_graph::FixedClassicWire>()),
        table("route_ranges", routes, size_of::<std::ops::Range<usize>>()),
        table("route_ops", ops, size_of::<crate::redpiler::program_graph::FixedClassicRouteOp>()),
        table("operands", operands, size_of::<crate::redpiler::program_graph::FixedClassicOperand>()),
    ];
    let table_payload_bytes = tables.iter().map(|table| table.payload_bytes).sum::<usize>();
    let program_object_bytes = size_of::<FixedClassicProgram>();
    InventoryDto {
        tables,
        program_object_bytes,
        table_payload_bytes,
        accounted_bytes: program_object_bytes + table_payload_bytes,
        excludes: "Arc control block and allocator overhead",
    }
}

fn table(name: &'static str, entries: usize, element_bytes: usize) -> TableInventoryDto {
    TableInventoryDto {
        table: name,
        entries,
        payload_bytes: entries * element_bytes,
    }
}

fn program_dto(program: &FixedClassicProgram) -> ProgramDto {
    let nodes = program
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| NodeDto {
            node: typed_id("node", index),
            kind: match node.kind {
                FixedClassicKind::Wire(wire_index) => KindDto::Wire { wire_index },
                FixedClassicKind::Torch => KindDto::Torch,
                FixedClassicKind::Repeater { delay, facing_diode } => KindDto::Repeater { delay, facing_diode },
                FixedClassicKind::Comparator { mode, facing_diode } => KindDto::Comparator {
                    mode: match mode {
                        ComparatorMode::Compare => "compare",
                        ComparatorMode::Subtract => "subtract",
                    },
                    facing_diode,
                },
                FixedClassicKind::Lamp => KindDto::Lamp,
                FixedClassicKind::Hopper => KindDto::Hopper,
                FixedClassicKind::Lever => KindDto::Lever,
                FixedClassicKind::Button => KindDto::Button,
                FixedClassicKind::PressurePlate => KindDto::PressurePlate,
                FixedClassicKind::Constant => KindDto::Constant,
            },
            inputs: [InputPort::Main, InputPort::Side, InputPort::Quasi]
                .into_iter()
                .enumerate()
                .map(|(port_index, port)| NamedQueryDto {
                    port: input_port(port),
                    query: query(program.nodes[index].inputs[port_index]),
                })
                .collect(),
            output_route: node.output_route.map(|route| typed_id("route", route.index())),
            comparator: node.comparator.map(|comparator| ComparatorDto {
                direct_override: comparator.direct_override.map(super::comparator_override),
                far_override: comparator.far_override.map(super::comparator_override),
                far_threshold: comparator.far_threshold,
            }),
        })
        .collect();
    let sites = program
        .sites
        .iter()
        .enumerate()
        .map(|(index, site)| SiteDto {
            site: typed_id("site", index),
            source: match site {
                FixedClassicSite::Inert => SiteKindDto::Inert,
                FixedClassicSite::Wire { node, index } => SiteKindDto::Wire {
                    node: typed_id("node", node.index()),
                    wire_index: *index,
                },
                FixedClassicSite::Gate(node) => SiteKindDto::Gate {
                    node: typed_id("node", node.index()),
                },
            },
        })
        .collect();
    let wires = program
        .wires
        .iter()
        .map(|wire| WireDto {
            node: typed_id("node", wire.node.index()),
            root: typed_id("site", wire.root.index()),
            aliases: std::array::from_fn(|slot| NeighborAliasDto {
                slot: slot as u8,
                offset: NEIGHBOR_OFFSETS[slot],
                site: typed_id("site", wire.neighbors[slot].index()),
            }),
            seed_mask: wire.seed_mask,
            turbo_mask: wire.turbo_mask,
            admitted_mask: wire.admitted,
            raw_sides: RawSidesDto {
                packed: wire.raw_sides,
                order: ["north", "south", "east", "west"],
                side_ids: std::array::from_fn(|side| (wire.raw_sides >> (side * 2)) & 3),
            },
            no_dust: BlockFace::values()
                .into_iter()
                .zip(wire.no_dust)
                .map(|(direction, query_value)| FaceQueryDto {
                    face: face(direction),
                    query: query(query_value),
                })
                .collect(),
            shape_route: wire.shape_route.map(|route| typed_id("route", route.index())),
        })
        .collect();
    let route_ranges = program
        .routes
        .iter()
        .enumerate()
        .map(|(index, range)| RouteRangeDto {
            route: typed_id("route", index),
            start: range.start,
            end: range.end,
        })
        .collect();
    let mut route_ops = Vec::with_capacity(program.ops.len());
    for (route_index, range) in program.routes.iter().enumerate() {
        for (index, operation) in range.clone().zip(&program.ops[range.clone()]) {
            route_ops.push(RouteOpDto {
                index,
                route: typed_id("route", route_index),
                ordinal: operation.ordinal,
                target: typed_id("node", operation.target.index()),
                operation: match operation.kind {
                    FixedClassicOpKind::Seed => OperationDto::Seed,
                    FixedClassicOpKind::Shape(direction) => OperationDto::Shape { direction: face(direction) },
                    FixedClassicOpKind::Update { direction } => OperationDto::Update {
                        direction: direction.map(face),
                    },
                },
            });
        }
    }
    let operands = program
        .operands
        .iter()
        .enumerate()
        .map(|(index, operand)| OperandDto {
            index,
            source: match operand.source {
                FixedClassicSource::LiveOutput(node) => OperandSourceDto::LiveOutput {
                    node: typed_id("node", node.index()),
                },
                FixedClassicSource::CapturedPowered { site, node } => OperandSourceDto::CapturedPowered {
                    site: typed_id("site", site.index()),
                    node: typed_id("node", node.index()),
                },
            },
            attenuation: operand.attenuation,
            require_cross: operand.require_cross,
        })
        .collect();
    ProgramDto {
        nodes,
        sites,
        wires,
        route_ranges,
        route_ops,
        operands,
    }
}

fn query(value: crate::redpiler::program_graph::FixedClassicQuery) -> QueryDto {
    QueryDto {
        constant: value.constant,
        start: value.start,
        end: value.end,
    }
}

fn kind_matches(kind: &KindDto, source: &NodeKindDto) -> bool {
    match (kind, source) {
        (KindDto::Wire { wire_index }, NodeKindDto::Wire { plan }) => plan == &typed_id("wire", *wire_index as usize),
        (kind, NodeKindDto::Electrical { component }) => match (kind, component) {
            (KindDto::Torch, ElectricalKindDto::Torch)
            | (KindDto::Lamp, ElectricalKindDto::Lamp)
            | (KindDto::Hopper, ElectricalKindDto::Hopper)
            | (KindDto::Lever, ElectricalKindDto::Lever)
            | (KindDto::Button, ElectricalKindDto::Button)
            | (KindDto::PressurePlate, ElectricalKindDto::PressurePlate)
            | (KindDto::Constant, ElectricalKindDto::Constant) => true,
            (
                KindDto::Repeater { delay, facing_diode },
                ElectricalKindDto::Repeater {
                    delay: source_delay,
                    facing_diode: source_diode,
                },
            ) => delay == source_delay && facing_diode == source_diode,
            (
                KindDto::Comparator { mode, facing_diode },
                ElectricalKindDto::Comparator {
                    mode: source_mode,
                    facing_diode: source_diode,
                    ..
                },
            ) => mode == source_mode && facing_diode == source_diode,
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn validate(
    plan: &Option<PlanDto>,
    graph: &GraphTablesDto,
    nodes: &HashSet<String>,
    sites: &HashSet<String>,
    routes: &HashSet<String>,
) -> Result<(), ExportError> {
    let Some(plan) = plan else { return Ok(()) };
    let invalid = || ExportError::new("validation", "invalid fixed classic executable plan");
    match (plan.status, plan.program.as_ref(), plan.decline.as_ref()) {
        ("declined", None, Some(reason)) if !reason.is_empty() => return Ok(()),
        ("compiled", Some(_), None) => {}
        _ => return Err(invalid()),
    }
    let program = plan.program.as_ref().ok_or_else(invalid)?;
    if program.nodes.len() != graph.nodes.len() || program.sites.len() != graph.sites.len() || graph.routes.is_none() {
        return Err(invalid());
    }
    let site_rows: HashMap<_, _> = graph.sites.iter().map(|site| (site.id.as_str(), site)).collect();
    let node_rows: HashMap<_, _> = graph.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let route_rows = graph.routes.as_ref().ok_or_else(invalid)?;
    if program.route_ranges.len() != route_rows.len() || program.wires.len() != graph.wire_plans.len() {
        return Err(invalid());
    }
    let check_query = |query: &QueryDto| -> Result<(), ExportError> {
        if query.constant > 15 || query.start > query.end || query.end as usize > program.operands.len() {
            return Err(invalid());
        }
        Ok(())
    };
    for (index, node) in program.nodes.iter().enumerate() {
        let expected = typed_id("node", index);
        let Some(source) = graph.nodes.get(index) else { return Err(invalid()) };
        if node.node != expected
            || source.id != expected
            || node.output_route != source.output_route
            || !kind_matches(&node.kind, &source.kind)
            || node.comparator.is_some() != matches!(&node.kind, KindDto::Comparator { .. })
        {
            return Err(invalid());
        }
        check_ref(nodes, &node.node, "fixed classic node")?;
        if node.inputs.len() != 3 || node.inputs.iter().map(|input| input.port).ne(["main", "side", "quasi"]) {
            return Err(invalid());
        }
        for query in &node.inputs {
            check_query(&query.query)?;
        }
        if let Some(route) = &node.output_route {
            check_ref(routes, route, "fixed classic output route")?;
        }
        if let Some(comparator) = &node.comparator {
            for value in [&comparator.direct_override, &comparator.far_override].into_iter().flatten() {
                validate_override(Some(value), nodes)?;
            }
            if comparator.far_threshold > 15 {
                return Err(invalid());
            }
        }
    }
    for (index, site) in program.sites.iter().enumerate() {
        let Some(source) = graph.sites.get(index) else { return Err(invalid()) };
        if site.site != typed_id("site", index) || source.id != site.site {
            return Err(invalid());
        }
        check_ref(sites, &site.site, "fixed classic site")?;
        match (&site.source, &source.source) {
            (SiteKindDto::Inert, SiteSourceDto::Constant { .. }) => {}
            (SiteKindDto::Wire { node, wire_index }, SiteSourceDto::Component { node: source_node }) => {
                check_ref(nodes, node, "fixed classic wire site")?;
                if node != source_node || *wire_index as usize >= program.wires.len() || program.wires[*wire_index as usize].node != *node {
                    return Err(invalid());
                }
            }
            (SiteKindDto::Gate { node }, SiteSourceDto::Component { node: source_node }) => {
                check_ref(nodes, node, "fixed classic gate site")?;
                if node != source_node {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    for (index, wire) in program.wires.iter().enumerate() {
        let Some(source) = graph.wire_plans.get(index) else {
            return Err(invalid());
        };
        if source.id != typed_id("wire", index) || source.node != wire.node || source.canonical_neighbors.len() != wire.aliases.len() {
            return Err(invalid());
        }
        check_ref(nodes, &wire.node, "fixed classic wire")?;
        check_ref(sites, &wire.root, "fixed classic wire root")?;
        if !matches!(site_rows.get(wire.root.as_str()).map(|site| &site.source), Some(SiteSourceDto::Component { node }) if node == &wire.node) {
            return Err(invalid());
        }
        if wire.aliases.iter().enumerate().any(|(slot, alias)| {
            alias.slot as usize != slot
                || alias.offset != NEIGHBOR_OFFSETS[slot]
                || alias.site != source.canonical_neighbors[slot]
                || !sites.contains(&alias.site)
        }) || [wire.seed_mask, wire.turbo_mask, wire.admitted_mask]
            .iter()
            .any(|mask| mask & !0x00ff_ffff != 0)
            || wire.no_dust.len() != 6
        {
            return Err(invalid());
        }
        if let Some(route) = &wire.shape_route {
            check_ref(routes, route, "fixed classic shape route")?;
        }
        for query in &wire.no_dust {
            check_query(&query.query)?;
        }
    }
    for operand in &program.operands {
        if operand.attenuation > 15 {
            return Err(invalid());
        }
        match &operand.source {
            OperandSourceDto::LiveOutput { node } => check_ref(nodes, node, "fixed classic live operand")?,
            OperandSourceDto::CapturedPowered { site, node } => {
                check_ref(sites, site, "fixed classic captured site")?;
                check_ref(nodes, node, "fixed classic captured node")?;
                if !matches!(site_rows.get(site.as_str()).map(|site| &site.source), Some(SiteSourceDto::Component { node: source_node }) if source_node == node)
                {
                    return Err(invalid());
                }
            }
        }
    }
    let mut next_op = 0;
    for (index, range) in program.route_ranges.iter().enumerate() {
        let route_id = typed_id("route", index);
        if range.route != route_id
            || !routes.contains(&range.route)
            || route_rows.get(index).map_or(true, |route| route.id != route_id)
            || range.start != next_op
            || range.end < range.start
            || range.end > program.route_ops.len()
        {
            return Err(invalid());
        }
        let mut previous_ordinal = None;
        for (offset, op) in program.route_ops[range.start..range.end].iter().enumerate() {
            if op.index != range.start + offset || op.route != route_id {
                return Err(invalid());
            }
            check_ref(nodes, &op.target, "fixed classic route target")?;
            let ordinal = op.ordinal as usize;
            if ordinal >= route_rows[index].deliveries.len() || route_rows[index].deliveries[ordinal].ordinal != op.ordinal {
                return Err(invalid());
            }
            if previous_ordinal.is_some_and(|previous| op.ordinal <= previous) {
                return Err(invalid());
            }
            if !operation_matches(op, &route_rows[index].deliveries[ordinal], &site_rows, &node_rows) {
                return Err(invalid());
            }
            previous_ordinal = Some(op.ordinal);
        }
        next_op = range.end;
    }
    if next_op != program.route_ops.len() {
        return Err(invalid());
    }
    let Some(actual_inventory) = &plan.inventory else {
        return Err(invalid());
    };
    let expected_inventory = inventory_counts(
        program.nodes.len(),
        program.sites.len(),
        program.wires.len(),
        program.route_ranges.len(),
        program.route_ops.len(),
        program.operands.len(),
    );
    if actual_inventory.program_object_bytes != expected_inventory.program_object_bytes
        || actual_inventory.table_payload_bytes != expected_inventory.table_payload_bytes
        || actual_inventory.accounted_bytes != expected_inventory.accounted_bytes
        || actual_inventory.excludes != expected_inventory.excludes
        || actual_inventory.tables.len() != expected_inventory.tables.len()
        || actual_inventory.tables.iter().zip(&expected_inventory.tables).any(|(actual, expected)| {
            actual.table != expected.table || actual.entries != expected.entries || actual.payload_bytes != expected.payload_bytes
        })
    {
        return Err(invalid());
    }
    Ok(())
}

fn operation_matches(
    operation: &RouteOpDto,
    delivery: &RouteDeliveryDto,
    sites: &HashMap<&str, &super::SiteDto>,
    nodes: &HashMap<&str, &super::NodeDto>,
) -> bool {
    let target = match delivery.receiver.as_ref() {
        Some(super::ReceiverDto::Component { node }) => Some(node.as_str()),
        Some(_) => None,
        None => sites.get(delivery.target.as_str()).and_then(|site| match &site.source {
            super::SiteSourceDto::Component { node } => Some(node.as_str()),
            _ => None,
        }),
    };
    if target != Some(operation.target.as_str()) {
        return false;
    }
    let is_wire = matches!(
        nodes.get(operation.target.as_str()).map(|node| &node.kind),
        Some(super::NodeKindDto::Wire { .. })
    );
    match &operation.operation {
        OperationDto::Seed => delivery.kind == "update" && is_wire,
        OperationDto::Shape { direction } => delivery.kind == "shape" && delivery.direction.as_deref() == Some(direction.as_str()) && is_wire,
        OperationDto::Update { direction } => delivery.kind == "update" && delivery.direction.as_deref() == direction.as_deref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn exports_declines_and_fixed_table_byte_inventory() {
        let declined = FixedClassicPlans {
            program: None,
            decline: Some("fixed input is dynamic".into()),
        };
        let declined = serde_json::to_value(export(&declined).unwrap()).unwrap();
        assert_eq!(declined["status"], "declined");
        assert_eq!(declined["decline"], "fixed input is dynamic");
        assert!(declined.get("program").is_none());

        let empty = FixedClassicProgram {
            nodes: Box::new([]),
            sites: Box::new([]),
            wires: Box::new([]),
            routes: Box::new([]),
            ops: Box::new([]),
            operands: Box::new([]),
        };
        let compiled = FixedClassicPlans {
            program: Some(Arc::new(empty)),
            decline: None,
        };
        let compiled = serde_json::to_value(export(&compiled).unwrap()).unwrap();
        assert_eq!(compiled["status"], "compiled");
        assert_eq!(compiled["inventory"]["table_payload_bytes"], 0);
        assert_eq!(compiled["inventory"]["accounted_bytes"], compiled["inventory"]["program_object_bytes"]);
        assert_eq!(compiled["program"]["route_ops"].as_array().unwrap().len(), 0);
    }
}
