//! Versioned inspection JSON for the richer graph drafts.
use super::electrical::{ComparatorInputPlan, ComparatorOverride, WireRelationKind};
use super::{AnalysisLimits, AnalysisReport};
use crate::redpiler::components::{NodeState, NodeType};
use crate::redpiler::passes;
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeKind, CompileGraph, ComponentState, Guard, InputPort, LinkKind,
    LinkSpec, OperandView, PistonAvailability, PistonLane, PistonRoutes, PowerLink, PowerPort,
    PowerTarget, Profile, ProfileElectrical, SiteSource, SiteTest, StateTest,
};
use crate::redpiler::{CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument, RedstoneWireSide};
use mchprs_blocks::{BlockFace, BlockPos};
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A stable machine-readable category accompanies every failed preparation or write.
#[derive(Debug)]
pub(crate) struct ExportError {
    pub(crate) code: &'static str,
    message: String,
}

impl ExportError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportError {}

/// Counts the graph records shown in the command's success message.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExportCounts {
    pub(crate) nodes: usize,
    pub(crate) power_links: usize,
    pub(crate) sites: usize,
    pub(crate) guards: usize,
    pub(crate) resources: usize,
}

/// GraphExport is the concrete v1 JSON contract; execution traces stay unavailable.
#[derive(Debug, Serialize)]
pub(crate) struct GraphExport {
    format: &'static str,
    version: VersionDto,
    stage: &'static str,
    capabilities: Vec<&'static str>,
    source: SourceDto,
    graph: GraphTablesDto,
    snapshots: Vec<SnapshotDto>,
    trace: Option<Vec<TraceRecordDto>>,
    diagnostics: Vec<DiagnosticDto>,
}

#[derive(Debug, Serialize)]
struct VersionDto {
    major: u32,
    minor: u32,
}

#[derive(Debug, Serialize)]
struct SourceDto {
    revision: Option<String>,
    bounds: BoundsDto,
}

#[derive(Debug, Serialize)]
struct BoundsDto {
    min: PositionDto,
    max: PositionDto,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct PositionDto {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Debug, Serialize)]
struct GraphTablesDto {
    nodes: Vec<NodeDto>,
    profiles: Vec<ProfileDto>,
    sites: Vec<SiteDto>,
    guards: Vec<GuardDto>,
    contacts: Vec<ContactDto>,
    pistons: Vec<PistonDto>,
    resources: Vec<ResourceDto>,
    power_links: Vec<PowerLinkDto>,
    wire_plans: Vec<WirePlanDto>,
    dust_paths: Vec<DustPathDto>,
    wire_relations: Vec<WireRelationDto>,
    comparator_plans: Vec<ComparatorPlanDto>,
    callback_links: Vec<CallbackLinkDto>,
    routes: Option<Vec<RouteDto>>,
}

#[derive(Debug, Serialize)]
struct NodeDto {
    id: String,
    kind: NodeKindDto,
    position: Option<PositionDto>,
    output_route: Option<String>,
}

/// NodeKind's tags distinguish the fixed electrical and specialized graph nodes.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NodeKindDto {
    Electrical { component: ElectricalKindDto },
    Observer { watched: String, facing: String },
    Wire { plan: String },
    Piston { piston: String },
    Contact { contact: String },
}

/// ElectricalKind describes each fixed component type and its rule parameters.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ElectricalKindDto {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    Observer {
        watched: PositionDto,
    },
    Comparator {
        mode: String,
        far_input: Option<u8>,
        facing_diode: bool,
    },
    Lamp,
    CopperBulb,
    Button,
    Lever,
    PressurePlate,
    Trapdoor,
    Wire,
    Constant,
    CommandBlock {
        repeating: bool,
        chain: bool,
        automatic: bool,
        initial_tick: bool,
    },
    NoteBlock {
        instrument: String,
        note: u32,
    },
}

#[derive(Debug, Serialize)]
struct ProfileDto {
    id: String,
    raw_state: u32,
    name: &'static str,
    registry_id: u32,
    solid: bool,
    transparent: bool,
    cube: bool,
    electrical: &'static str,
}

#[derive(Debug, Serialize)]
struct SiteDto {
    id: String,
    position: PositionDto,
    source: SiteSourceDto,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SiteSourceDto {
    Component { node: String },
    Contact { contact: String },
    Constant { profile: String },
}

#[derive(Debug, Serialize)]
struct GuardDto {
    id: String,
    #[serde(flatten)]
    expression: GuardExpressionDto,
}

/// GuardExpression retains the source operands and the live/captured view.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GuardExpressionDto {
    Always,
    State {
        test: StateTestDto,
    },
    Site {
        site: String,
        view: &'static str,
        test: SiteTestDto,
    },
    Not {
        guard: String,
    },
    All {
        guards: Vec<String>,
    },
    Any {
        guards: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StateTestDto {
    PistonExtended {
        piston: String,
    },
    PistonHeadPresent {
        piston: String,
    },
    PistonAvailability {
        piston: String,
        availability: &'static str,
    },
    ResourceAt {
        resource: String,
        contact: String,
    },
    ResourceEnabled {
        resource: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SiteTestDto {
    Solid,
    Transparent,
    IsWire,
    Profile { profile: String },
}

#[derive(Debug, Serialize)]
struct ContactDto {
    id: String,
    node: String,
    position: PositionDto,
    variants: Vec<ContactVariantDto>,
}

#[derive(Debug, Serialize)]
struct ContactVariantDto {
    r#when: String,
    profile: String,
    receiver: Option<ReceiverDto>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReceiverDto {
    Component { node: String },
    PistonBase { piston: String },
    PistonHead { piston: String },
    Contact { contact: String },
}

#[derive(Debug, Serialize)]
struct PistonDto {
    id: String,
    node: String,
    position: PositionDto,
    facing: String,
    sticky: bool,
    base: String,
    near: String,
    far: String,
    lane: PistonLaneDto,
    routes: Option<PistonRouteBindingsDto>,
}

/// PistonLane preserves the empty/single-resource distinction from geometry.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PistonLaneDto {
    Empty,
    SingleResource { far: String },
}

#[derive(Debug, Serialize)]
struct PistonRouteBindingsDto {
    base_changed: String,
    near_changed: String,
    far_changed: Option<String>,
    far_shape_changed: Option<String>,
    base_restored: String,
    head_restored: String,
    resource_restored_near: Option<String>,
    resource_restored_far: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResourceDto {
    id: String,
    material: String,
    contacts: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PowerLinkDto {
    id: String,
    from: PowerSourceDto,
    to: PowerTargetDto,
    face: String,
    attenuation: u8,
    guard: String,
}

#[derive(Debug, Serialize)]
struct PowerSourceDto {
    site: String,
    port: PowerPortDto,
}

/// PowerPort preserves direction and both conductor dust policies as separate variants.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PowerPortDto {
    Output,
    Stored,
    Weak { face: String },
    Strong { face: String },
    Conducted { include_dust: bool },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PowerTargetDto {
    Input { node: String, port: &'static str },
    Conductor { site: String, include_dust: bool },
}

#[derive(Debug, Serialize)]
struct WirePlanDto {
    id: String,
    node: String,
    canonical_neighbors: Vec<String>,
    shape_route: Option<String>,
    turbo_route: Option<String>,
}

#[derive(Debug, Serialize)]
struct DustPathDto {
    id: String,
    input_node: String,
    input_site: String,
    input_face: String,
    wires: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WireRelationDto {
    id: String,
    from: String,
    to: String,
    kind: WireRelationKindDto,
    direction: String,
    guard: String,
}

/// WireRelationKind distinguishes step, ascent, and descent dependencies.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum WireRelationKindDto {
    Step,
    Ascend,
    Descend,
}

#[derive(Debug, Serialize)]
struct ComparatorPlanDto {
    id: String,
    node: String,
    rear: String,
    direct_override: Option<ComparatorOverrideDto>,
    far_override: Option<FarOverrideDto>,
    far_threshold: u8,
}

#[derive(Debug, Serialize)]
struct FarOverrideDto {
    site: String,
    value: ComparatorOverrideDto,
    guard: String,
}

/// ComparatorOverride preserves fixed values separately from live component output.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ComparatorOverrideDto {
    Literal { value: u8 },
    LiveComponentOutput { node: String },
}

#[derive(Debug, Serialize)]
struct RouteDto {
    id: String,
    source: String,
    deliveries: Vec<RouteDeliveryDto>,
}

#[derive(Debug, Serialize)]
struct RouteDeliveryDto {
    ordinal: u32,
    sequence: u32,
    target: String,
    kind: &'static str,
    direction: Option<String>,
    guard: String,
    view: &'static str,
    receiver: Option<ReceiverDto>,
}

#[derive(Debug, Serialize)]
struct CallbackLinkDto {
    id: String,
    from: String,
    to: String,
    kind: &'static str,
    direction: Option<String>,
    guard: String,
    route: String,
    sequence: u32,
}

#[derive(Debug, Serialize)]
struct SnapshotDto {
    id: &'static str,
    components: Vec<ComponentSnapshotDto>,
    pistons: Vec<PistonSnapshotDto>,
    resources: Vec<ResourceSnapshotDto>,
    execution: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ComponentSnapshotDto {
    node: String,
    state: ComponentStateDto,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ComponentStateDto {
    Electrical {
        powered: bool,
        locked: bool,
        output_strength: u8,
        pending_tick: bool,
    },
    Wire {
        power: u8,
        north: &'static str,
        south: &'static str,
        east: &'static str,
        west: &'static str,
    },
    Specialized,
}

#[derive(Debug, Serialize)]
struct PistonSnapshotDto {
    piston: String,
    extended: bool,
    head_present: bool,
    availability: &'static str,
    revision: String,
}

#[derive(Debug, Serialize)]
struct ResourceSnapshotDto {
    resource: String,
    selected_contact: String,
    enabled: bool,
    revision: String,
}

#[derive(Debug, Serialize)]
struct TraceRecordDto {
    sequence: String,
    tick: String,
    phase: String,
    kind: String,
    details: serde_json::Value,
    snapshot: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiagnosticDto {
    code: &'static str,
    message: &'static str,
    positions: Vec<PositionDto>,
}

impl GraphExport {
    /// counts returns the graph sizes shown to the initiating player.
    pub(crate) fn counts(&self) -> ExportCounts {
        ExportCounts {
            nodes: self.graph.nodes.len(),
            power_links: self.graph.power_links.len(),
            sites: self.graph.sites.len(),
            guards: self.graph.guards.len(),
            resources: self.graph.resources.len(),
        }
    }

    /// to_bytes validates every typed reference before deterministic pretty serialization.
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, ExportError> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| ExportError::new("serialization", error.to_string()))
    }

    fn validate(&self) -> Result<(), ExportError> {
        if self.format != "mchprs.redpiler.graph" || self.version.major != 1 {
            return Err(ExportError::new(
                "schema",
                "invalid graph JSON format header",
            ));
        }
        if self.stage != "callback_draft" {
            return Err(ExportError::new(
                "stage",
                "draft export has an invalid stage",
            ));
        }
        let nodes = unique_ids("node", self.graph.nodes.iter().map(|item| item.id.as_str()))?;
        let profiles = unique_ids(
            "profile",
            self.graph.profiles.iter().map(|item| item.id.as_str()),
        )?;
        let sites = unique_ids("site", self.graph.sites.iter().map(|item| item.id.as_str()))?;
        let guards = unique_ids(
            "guard",
            self.graph.guards.iter().map(|item| item.id.as_str()),
        )?;
        let contacts = unique_ids(
            "contact",
            self.graph.contacts.iter().map(|item| item.id.as_str()),
        )?;
        let pistons = unique_ids(
            "piston",
            self.graph.pistons.iter().map(|item| item.id.as_str()),
        )?;
        let resources = unique_ids(
            "resource",
            self.graph.resources.iter().map(|item| item.id.as_str()),
        )?;
        let wires = unique_ids(
            "wire",
            self.graph.wire_plans.iter().map(|item| item.id.as_str()),
        )?;
        let routes = self
            .graph
            .routes
            .as_ref()
            .map(|items| unique_ids("route", items.iter().map(|item| item.id.as_str())))
            .transpose()?
            .unwrap_or_default();
        unique_ids(
            "power_link",
            self.graph.power_links.iter().map(|item| item.id.as_str()),
        )?;
        unique_ids(
            "wire_relation",
            self.graph
                .wire_relations
                .iter()
                .map(|item| item.id.as_str()),
        )?;
        unique_ids(
            "callback_link",
            self.graph
                .callback_links
                .iter()
                .map(|item| item.id.as_str()),
        )?;
        unique_ids(
            "comparator_plan",
            self.graph
                .comparator_plans
                .iter()
                .map(|item| item.id.as_str()),
        )?;

        for node in &self.graph.nodes {
            check_optional_ref(&routes, node.output_route.as_deref(), "node output route")?;
            match &node.kind {
                NodeKindDto::Electrical { .. } => {}
                NodeKindDto::Observer { watched, .. } => {
                    check_ref(&sites, watched, "observer site")?
                }
                NodeKindDto::Wire { plan } => check_ref(&wires, plan, "wire plan")?,
                NodeKindDto::Piston { piston } => check_ref(&pistons, piston, "node piston")?,
                NodeKindDto::Contact { contact } => check_ref(&contacts, contact, "node contact")?,
            }
        }
        for site in &self.graph.sites {
            match &site.source {
                SiteSourceDto::Component { node } => check_ref(&nodes, node, "site component")?,
                SiteSourceDto::Contact { contact } => {
                    check_ref(&contacts, contact, "site contact")?
                }
                SiteSourceDto::Constant { profile } => {
                    check_ref(&profiles, profile, "site profile")?
                }
            }
        }
        for (index, guard) in self.graph.guards.iter().enumerate() {
            let current = typed_id("guard", index);
            match &guard.expression {
                GuardExpressionDto::Always => {}
                GuardExpressionDto::State { test } => match test {
                    StateTestDto::PistonExtended { piston }
                    | StateTestDto::PistonHeadPresent { piston }
                    | StateTestDto::PistonAvailability { piston, .. } => {
                        check_ref(&pistons, piston, "guard piston")?
                    }
                    StateTestDto::ResourceAt { resource, contact } => {
                        check_ref(&resources, resource, "guard resource")?;
                        check_ref(&contacts, contact, "guard contact")?;
                    }
                    StateTestDto::ResourceEnabled { resource } => {
                        check_ref(&resources, resource, "guard resource")?
                    }
                },
                GuardExpressionDto::Site { site, test, .. } => {
                    check_ref(&sites, site, "guard site")?;
                    if let SiteTestDto::Profile { profile } = test {
                        check_ref(&profiles, profile, "guard profile")?;
                    }
                }
                GuardExpressionDto::Not { guard } => check_prior_guard(&guards, guard, &current)?,
                GuardExpressionDto::All { guards: references }
                | GuardExpressionDto::Any { guards: references } => {
                    for reference in references {
                        check_prior_guard(&guards, reference, &current)?;
                    }
                }
            }
        }
        for contact in &self.graph.contacts {
            check_ref(&nodes, &contact.node, "contact node")?;
            for variant in &contact.variants {
                check_ref(&guards, &variant.r#when, "contact guard")?;
                check_ref(&profiles, &variant.profile, "contact profile")?;
                if let Some(receiver) = &variant.receiver {
                    match receiver {
                        ReceiverDto::Component { node } => {
                            check_ref(&nodes, node, "receiver node")?
                        }
                        ReceiverDto::PistonBase { piston } | ReceiverDto::PistonHead { piston } => {
                            check_ref(&pistons, piston, "receiver piston")?
                        }
                        ReceiverDto::Contact { contact } => {
                            check_ref(&contacts, contact, "receiver contact")?
                        }
                    }
                }
            }
        }
        for piston in &self.graph.pistons {
            check_ref(&nodes, &piston.node, "piston node")?;
            check_ref(&contacts, &piston.base, "piston base")?;
            check_ref(&contacts, &piston.near, "piston near")?;
            check_ref(&contacts, &piston.far, "piston far")?;
            if let PistonLaneDto::SingleResource { far } = &piston.lane {
                check_ref(&contacts, far, "piston lane")?;
            }
            if let Some(bindings) = &piston.routes {
                for id in [
                    Some(bindings.base_changed.as_str()),
                    Some(bindings.near_changed.as_str()),
                    bindings.far_changed.as_deref(),
                    bindings.far_shape_changed.as_deref(),
                    Some(bindings.base_restored.as_str()),
                    Some(bindings.head_restored.as_str()),
                    bindings.resource_restored_near.as_deref(),
                    bindings.resource_restored_far.as_deref(),
                ] {
                    check_optional_ref(&routes, id, "piston route")?;
                }
            }
        }
        for resource in &self.graph.resources {
            check_ref(&profiles, &resource.material, "resource material")?;
            for contact in &resource.contacts {
                check_ref(&contacts, contact, "resource contact")?;
            }
        }
        for link in &self.graph.power_links {
            check_ref(&sites, &link.from.site, "power source")?;
            check_ref(&guards, &link.guard, "power guard")?;
            match &link.to {
                PowerTargetDto::Input { node, .. } => check_ref(&nodes, node, "power target")?,
                PowerTargetDto::Conductor { site, .. } => {
                    check_ref(&sites, site, "conductor target")?
                }
            }
        }
        for wire in &self.graph.wire_plans {
            check_ref(&nodes, &wire.node, "wire node")?;
            if wire.canonical_neighbors.len() != 24 {
                return Err(ExportError::new(
                    "validation",
                    "wire plan must contain exactly 24 neighbor sites",
                ));
            }
            for site in &wire.canonical_neighbors {
                check_ref(&sites, site, "wire neighbor")?;
            }
            check_optional_ref(&routes, wire.shape_route.as_deref(), "wire shape route")?;
            check_optional_ref(&routes, wire.turbo_route.as_deref(), "wire turbo route")?;
        }
        for relation in &self.graph.wire_relations {
            check_ref(&nodes, &relation.from, "wire relation source")?;
            check_ref(&sites, &relation.to, "wire relation target")?;
            check_ref(&guards, &relation.guard, "wire relation guard")?;
        }
        for link in &self.graph.callback_links {
            check_ref(&nodes, &link.from, "callback source")?;
            check_ref(&nodes, &link.to, "callback receiver")?;
            check_ref(&guards, &link.guard, "callback guard")?;
            check_ref(&routes, &link.route, "callback route")?;
        }
        for plan in &self.graph.comparator_plans {
            check_ref(&nodes, &plan.node, "comparator node")?;
            check_ref(&sites, &plan.rear, "comparator rear site")?;
            if plan.far_threshold > 15 {
                return Err(ExportError::new(
                    "validation",
                    "comparator threshold exceeds 15",
                ));
            }
            validate_override(plan.direct_override.as_ref(), &nodes)?;
            if let Some(far) = &plan.far_override {
                check_ref(&sites, &far.site, "comparator far site")?;
                check_ref(&guards, &far.guard, "comparator far guard")?;
                validate_override(Some(&far.value), &nodes)?;
            }
        }
        if let Some(routes) = &self.graph.routes {
            for route in routes {
                check_ref(&nodes, &route.source, "route source")?;
                for delivery in &route.deliveries {
                    check_ref(&sites, &delivery.target, "route target")?;
                    check_ref(&guards, &delivery.guard, "route guard")?;
                    if let Some(receiver) = &delivery.receiver {
                        match receiver {
                            ReceiverDto::Component { node } => {
                                check_ref(&nodes, node, "route receiver")?
                            }
                            ReceiverDto::PistonBase { piston }
                            | ReceiverDto::PistonHead { piston } => {
                                check_ref(&pistons, piston, "route receiver")?
                            }
                            ReceiverDto::Contact { contact } => {
                                check_ref(&contacts, contact, "route receiver")?
                            }
                        }
                    }
                }
            }
        }
        for snapshot in &self.snapshots {
            for component in &snapshot.components {
                check_ref(&nodes, &component.node, "snapshot component")?;
                match &component.state {
                    ComponentStateDto::Electrical {
                        output_strength, ..
                    } if *output_strength > 15 => {
                        return Err(ExportError::new(
                            "validation",
                            "component strength exceeds 15",
                        ));
                    }
                    ComponentStateDto::Wire { power, .. } if *power > 15 => {
                        return Err(ExportError::new("validation", "wire strength exceeds 15"));
                    }
                    _ => {}
                }
            }
            for piston in &snapshot.pistons {
                check_ref(&pistons, &piston.piston, "snapshot piston")?;
            }
            for resource in &snapshot.resources {
                check_ref(&resources, &resource.resource, "snapshot resource")?;
                check_ref(
                    &contacts,
                    &resource.selected_contact,
                    "snapshot resource contact",
                )?;
            }
        }
        Ok(())
    }
}

/// Prepare a read-only graph with the same optimization tiers used by compilation.
pub(crate) fn prepare<W: World>(
    world: &W,
    bounds: (BlockPos, BlockPos),
    ticks: &[mchprs_world::TickEntry],
    options: &CompilerOptions,
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
) -> Result<GraphExport, ExportError> {
    let mut prepared = passes::rich::prepare(world, bounds, ticks.to_vec(), monitor, limits)
        .map_err(map_preparation_error)?;
    let pipeline = passes::default_graph_pipeline(options);
    passes::run_graph_pipeline(
        &mut prepared.graph,
        options,
        &prepared.entry,
        &pipeline,
        monitor,
    )
    .map_err(map_pass_error)?;
    prepared.finish_pipeline_statistics(monitor);
    let export = from_graph(&prepared.report, &prepared.graph)?;
    check_export_cancellation(monitor)?;
    monitor.inc_progress();
    Ok(export)
}

/// prepare_and_write serializes and validates first, then publishes exactly one artifact.
pub(crate) fn prepare_and_write<W: World>(
    world: &W,
    bounds: (BlockPos, BlockPos),
    ticks: &[mchprs_world::TickEntry],
    options: &CompilerOptions,
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
    directory: &Path,
    plot_x: i32,
    plot_z: i32,
) -> Result<(PathBuf, ExportCounts), ExportError> {
    let export = prepare(world, bounds, ticks, options, monitor, limits)?;
    let counts = export.counts();
    let bytes = export.to_bytes()?;
    check_export_cancellation(monitor)?;
    let path = write_artifact(directory, plot_x, plot_z, &bytes)
        .map_err(|error| ExportError::new("write", error.to_string()))?;
    Ok((path, counts))
}

fn check_export_cancellation(monitor: &TaskMonitor) -> Result<(), ExportError> {
    if monitor.cancelled() {
        return Err(ExportError::new("cancelled", "graph export cancelled"));
    }
    Ok(())
}

fn map_preparation_error(error: passes::rich::PreparationError) -> ExportError {
    use passes::rich::PreparationError;
    let (code, message) = match &error {
        PreparationError::Analysis(error) => {
            let code = if matches!(error, super::AnalysisError::Cancelled) {
                "cancelled"
            } else {
                "analysis"
            };
            (code, error.to_string())
        }
        PreparationError::Geometry(error) => {
            let code = match error {
                super::geometry::GeometryError::Analysis(super::AnalysisError::Cancelled) => {
                    "cancelled"
                }
                super::geometry::GeometryError::Analysis(_) => "analysis",
                super::geometry::GeometryError::Entry(_) => "admission",
                super::geometry::GeometryError::Rejected(_) => "geometry",
            };
            (code, error.to_string())
        }
        PreparationError::Electrical(error) => {
            let code = match error {
                super::electrical::ElectricalError::Preparation(
                    super::AnalysisError::Cancelled,
                ) => "cancelled",
                super::electrical::ElectricalError::Preparation(_) => "analysis",
                _ => "electrical",
            };
            (code, error.to_string())
        }
        PreparationError::Callbacks(error) => {
            let code = match error {
                super::callbacks::CallbackError::Preparation(super::AnalysisError::Cancelled) => {
                    "cancelled"
                }
                super::callbacks::CallbackError::Preparation(_) => "analysis",
                _ => "callbacks",
            };
            (code, error.to_string())
        }
    };
    ExportError::new(code, message)
}

fn map_pass_error(error: passes::PassPipelineError) -> ExportError {
    let code = if matches!(&error, passes::PassPipelineError::Cancelled) {
        "cancelled"
    } else {
        "pass"
    };
    ExportError::new(code, error.to_string())
}

pub(super) fn from_graph(
    report: &AnalysisReport,
    source: &CompileGraph,
) -> Result<GraphExport, ExportError> {
    let graph = GraphTablesDto {
        nodes: source
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                Ok(NodeDto {
                    id: typed_id("node", index),
                    kind: node_kind(&node.kind)?,
                    position: node.position.map(position),
                    output_route: node
                        .output_route
                        .map(|route| typed_id("route", route.index())),
                })
            })
            .collect::<Result<_, ExportError>>()?,
        profiles: source
            .profiles
            .iter()
            .enumerate()
            .map(|(index, item)| profile_dto(index, item))
            .collect(),
        sites: source
            .sites
            .iter()
            .enumerate()
            .map(|(index, item)| SiteDto {
                id: typed_id("site", index),
                position: position(item.position),
                source: match item.source {
                    SiteSource::Component(node) => SiteSourceDto::Component {
                        node: typed_id("node", node.index()),
                    },
                    SiteSource::Contact(contact) => SiteSourceDto::Contact {
                        contact: typed_id("contact", contact.index()),
                    },
                    SiteSource::Constant(profile) => SiteSourceDto::Constant {
                        profile: typed_id("profile", profile.index()),
                    },
                },
            })
            .collect(),
        guards: source
            .guards
            .iter()
            .enumerate()
            .map(|(index, item)| GuardDto {
                id: typed_id("guard", index),
                expression: guard_dto(item),
            })
            .collect(),
        contacts: source
            .contacts
            .iter()
            .enumerate()
            .map(|(index, item)| ContactDto {
                id: typed_id("contact", index),
                node: typed_id("node", item.node.index()),
                position: position(item.position),
                variants: item
                    .variants
                    .iter()
                    .map(|variant| ContactVariantDto {
                        r#when: typed_id("guard", variant.when.index()),
                        profile: typed_id("profile", variant.profile.index()),
                        receiver: variant.receiver.map(receiver_dto),
                    })
                    .collect(),
            })
            .collect(),
        pistons: source
            .pistons
            .iter()
            .enumerate()
            .map(|(index, item)| PistonDto {
                id: typed_id("piston", index),
                node: typed_id("node", item.node.index()),
                position: position(item.position),
                facing: face(item.facing),
                sticky: item.sticky,
                base: typed_id("contact", item.base.index()),
                near: typed_id("contact", item.near.index()),
                far: typed_id("contact", item.far.index()),
                lane: match item.lane {
                    PistonLane::Empty => PistonLaneDto::Empty,
                    PistonLane::SingleResource { far } => PistonLaneDto::SingleResource {
                        far: typed_id("contact", far.index()),
                    },
                },
                routes: Some(piston_routes_dto(item.routes)),
            })
            .collect(),
        resources: source
            .resources
            .iter()
            .enumerate()
            .map(|(index, item)| ResourceDto {
                id: typed_id("resource", index),
                material: typed_id("profile", item.material.index()),
                contacts: item
                    .contacts
                    .iter()
                    .map(|contact| typed_id("contact", contact.index()))
                    .collect(),
            })
            .collect(),
        power_links: source
            .power_links
            .iter()
            .enumerate()
            .map(|(index, item)| power_link(index, item))
            .collect(),
        wire_plans: source
            .wires
            .iter()
            .enumerate()
            .map(|(index, item)| WirePlanDto {
                id: typed_id("wire", index),
                node: typed_id("node", item.node.index()),
                canonical_neighbors: item
                    .canonical_neighbors
                    .iter()
                    .map(|site| typed_id("site", site.index()))
                    .collect(),
                shape_route: item
                    .shape_route
                    .map(|route| typed_id("route", route.index())),
                turbo_route: item
                    .turbo_route
                    .map(|route| typed_id("route", route.index())),
            })
            .collect(),
        dust_paths: source
            .dust_paths
            .iter()
            .enumerate()
            .map(|(index, path)| DustPathDto {
                id: typed_id("dust_path", index),
                input_node: typed_id("node", path.input_node.index()),
                input_site: typed_id("site", path.input_site.index()),
                input_face: face(path.input_face),
                wires: path
                    .wires
                    .iter()
                    .map(|node| typed_id("node", node.index()))
                    .collect(),
            })
            .collect(),
        wire_relations: source
            .wire_relations
            .iter()
            .enumerate()
            .map(|(index, item)| WireRelationDto {
                id: typed_id("wire_relation", index),
                from: typed_id("node", item.from.index()),
                to: typed_id("site", item.to.index()),
                kind: match item.kind {
                    WireRelationKind::Step => WireRelationKindDto::Step,
                    WireRelationKind::Ascend => WireRelationKindDto::Ascend,
                    WireRelationKind::Descend => WireRelationKindDto::Descend,
                },
                direction: face(item.direction),
                guard: typed_id("guard", item.guard.index()),
            })
            .collect(),
        comparator_plans: source
            .comparator_inputs
            .iter()
            .enumerate()
            .map(|(index, item)| comparator_plan(index, item))
            .collect(),
        callback_links: source
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| callback_link_dto(index, link))
            .collect(),
        routes: Some(
            source
                .routes
                .iter()
                .enumerate()
                .map(|(index, route)| RouteDto {
                    id: typed_id("route", index),
                    source: typed_id("node", route.source.index()),
                    deliveries: route
                        .deliveries
                        .iter()
                        .enumerate()
                        .map(|(ordinal, delivery)| RouteDeliveryDto {
                            ordinal: ordinal as u32,
                            sequence: delivery.sequence,
                            target: typed_id("site", delivery.target.index()),
                            kind: match delivery.kind {
                                crate::redpiler::program_graph::DeliveryKind::Update { .. } => {
                                    "update"
                                }
                                crate::redpiler::program_graph::DeliveryKind::Shape { .. } => {
                                    "shape"
                                }
                            },
                            direction: match delivery.kind {
                                crate::redpiler::program_graph::DeliveryKind::Update {
                                    direction,
                                } => direction.map(face),
                                crate::redpiler::program_graph::DeliveryKind::Shape {
                                    direction,
                                } => Some(face(direction)),
                            },
                            guard: typed_id("guard", delivery.guard.index()),
                            view: operand_view(delivery.view),
                            receiver: delivery.receiver.map(receiver_dto),
                        })
                        .collect(),
                })
                .collect(),
        ),
    };
    let snapshot = SnapshotDto {
        id: "snapshot:initial",
        components: source
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| ComponentSnapshotDto {
                node: typed_id("node", index),
                state: component_state(&node.state),
            })
            .collect(),
        pistons: source
            .initial_pistons
            .iter()
            .enumerate()
            .map(|(index, state)| PistonSnapshotDto {
                piston: typed_id("piston", index),
                extended: state.extended,
                head_present: state.head_present,
                availability: piston_availability(state.availability),
                revision: state.revision.to_string(),
            })
            .collect(),
        resources: source
            .initial_resources
            .iter()
            .enumerate()
            .map(|(index, state)| ResourceSnapshotDto {
                resource: typed_id("resource", index),
                selected_contact: typed_id("contact", state.at.index()),
                enabled: state.enabled,
                revision: state.revision.to_string(),
            })
            .collect(),
        execution: None,
    };
    let export = GraphExport {
        format: "mchprs.redpiler.graph",
        version: VersionDto { major: 1, minor: 1 },
        stage: "callback_draft",
        capabilities: vec![
            "electrical",
            "geometry",
            "guarded_contacts",
            "initial_snapshot",
            "wire_relations",
            "comparator_plans",
            "callback_routes",
            "callback_links",
            "piston_routes",
        ],
        source: SourceDto {
            revision: None,
            bounds: BoundsDto {
                min: position(report.bounds.0),
                max: position(report.bounds.1),
            },
        },
        graph,
        snapshots: vec![snapshot],
        trace: None,
        diagnostics: Vec::new(),
    };
    export.validate()?;
    Ok(export)
}

fn node_kind(kind: &CircuitNodeKind) -> Result<NodeKindDto, ExportError> {
    Ok(match kind {
        CircuitNodeKind::Electrical(component) => NodeKindDto::Electrical {
            component: electrical_kind(component)?,
        },
        CircuitNodeKind::Observer { watched, facing } => NodeKindDto::Observer {
            watched: typed_id("site", watched.index()),
            facing: face(*facing),
        },
        CircuitNodeKind::Wire(plan) => NodeKindDto::Wire {
            plan: typed_id("wire", *plan),
        },
        CircuitNodeKind::Piston(piston) => NodeKindDto::Piston {
            piston: typed_id("piston", piston.index()),
        },
        CircuitNodeKind::Contact(contact) => NodeKindDto::Contact {
            contact: typed_id("contact", contact.index()),
        },
    })
}

fn electrical_kind(kind: &NodeType) -> Result<ElectricalKindDto, ExportError> {
    Ok(match kind {
        NodeType::Repeater {
            delay,
            facing_diode,
        } => ElectricalKindDto::Repeater {
            delay: *delay,
            facing_diode: *facing_diode,
        },
        NodeType::Torch => ElectricalKindDto::Torch,
        NodeType::Observer { watched } => ElectricalKindDto::Observer {
            watched: position(*watched),
        },
        NodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => ElectricalKindDto::Comparator {
            mode: comparator_mode(*mode).into(),
            far_input: *far_input,
            facing_diode: *facing_diode,
        },
        NodeType::Lamp => ElectricalKindDto::Lamp,
        NodeType::CopperBulb => ElectricalKindDto::CopperBulb,
        NodeType::Button => ElectricalKindDto::Button,
        NodeType::Lever => ElectricalKindDto::Lever,
        NodeType::PressurePlate => ElectricalKindDto::PressurePlate,
        NodeType::Trapdoor => ElectricalKindDto::Trapdoor,
        NodeType::Wire => ElectricalKindDto::Wire,
        NodeType::Constant => ElectricalKindDto::Constant,
        NodeType::CommandBlock {
            repeating,
            chain,
            automatic,
            initial_tick,
        } => ElectricalKindDto::CommandBlock {
            repeating: *repeating,
            chain: *chain,
            automatic: *automatic,
            initial_tick: *initial_tick,
        },
        NodeType::NoteBlock { instrument, note } => ElectricalKindDto::NoteBlock {
            instrument: instrument_name(*instrument).into(),
            note: *note,
        },
    })
}

fn profile_dto(index: usize, profile: &Profile) -> ProfileDto {
    ProfileDto {
        id: typed_id("profile", index),
        raw_state: profile.raw_state,
        name: Block::from_id(profile.raw_state).get_name(),
        registry_id: profile.registry_type,
        solid: profile.solid,
        transparent: profile.transparent,
        cube: profile.cube,
        electrical: match profile.electrical {
            ProfileElectrical::None => "none",
            ProfileElectrical::RedstoneSource => "redstone_source",
            ProfileElectrical::Component => "component",
        },
    }
}

fn guard_dto(guard: &Guard) -> GuardExpressionDto {
    match guard {
        Guard::Always => GuardExpressionDto::Always,
        Guard::State(test) => GuardExpressionDto::State {
            test: match test {
                StateTest::PistonExtended(piston) => StateTestDto::PistonExtended {
                    piston: typed_id("piston", piston.index()),
                },
                StateTest::PistonHeadPresent(piston) => StateTestDto::PistonHeadPresent {
                    piston: typed_id("piston", piston.index()),
                },
                StateTest::PistonAvailability { piston, is } => StateTestDto::PistonAvailability {
                    piston: typed_id("piston", piston.index()),
                    availability: piston_availability(*is),
                },
                StateTest::ResourceAt { resource, contact } => StateTestDto::ResourceAt {
                    resource: typed_id("resource", resource.index()),
                    contact: typed_id("contact", contact.index()),
                },
                StateTest::ResourceEnabled(resource) => StateTestDto::ResourceEnabled {
                    resource: typed_id("resource", resource.index()),
                },
            },
        },
        Guard::Site { site, view, test } => GuardExpressionDto::Site {
            site: typed_id("site", site.index()),
            view: operand_view(*view),
            test: match test {
                SiteTest::Solid => SiteTestDto::Solid,
                SiteTest::Transparent => SiteTestDto::Transparent,
                SiteTest::IsWire => SiteTestDto::IsWire,
                SiteTest::Profile(profile) => SiteTestDto::Profile {
                    profile: typed_id("profile", profile.index()),
                },
            },
        },
        Guard::Not(guard) => GuardExpressionDto::Not {
            guard: typed_id("guard", guard.index()),
        },
        Guard::All(guards) => GuardExpressionDto::All {
            guards: guards
                .iter()
                .map(|guard| typed_id("guard", guard.index()))
                .collect(),
        },
        Guard::Any(guards) => GuardExpressionDto::Any {
            guards: guards
                .iter()
                .map(|guard| typed_id("guard", guard.index()))
                .collect(),
        },
    }
}

fn receiver_dto(receiver: CallbackReceiver) -> ReceiverDto {
    match receiver {
        CallbackReceiver::Component(node) => ReceiverDto::Component {
            node: typed_id("node", node.index()),
        },
        CallbackReceiver::PistonBase(piston) => ReceiverDto::PistonBase {
            piston: typed_id("piston", piston.index()),
        },
        CallbackReceiver::PistonHead(piston) => ReceiverDto::PistonHead {
            piston: typed_id("piston", piston.index()),
        },
        CallbackReceiver::Contact(contact) => ReceiverDto::Contact {
            contact: typed_id("contact", contact.index()),
        },
    }
}

fn power_link(index: usize, link: &PowerLink) -> PowerLinkDto {
    PowerLinkDto {
        id: typed_id("power_link", index),
        from: PowerSourceDto {
            site: typed_id("site", link.from.index()),
            port: match link.from_port {
                PowerPort::Output => PowerPortDto::Output,
                PowerPort::Stored => PowerPortDto::Stored,
                PowerPort::Weak(face_value) => PowerPortDto::Weak {
                    face: face(face_value),
                },
                PowerPort::Strong(face_value) => PowerPortDto::Strong {
                    face: face(face_value),
                },
                PowerPort::Conducted { include_dust } => PowerPortDto::Conducted { include_dust },
            },
        },
        to: match link.to {
            PowerTarget::Input { node, port } => PowerTargetDto::Input {
                node: typed_id("node", node.index()),
                port: input_port(port),
            },
            PowerTarget::Conductor { site, include_dust } => PowerTargetDto::Conductor {
                site: typed_id("site", site.index()),
                include_dust,
            },
        },
        face: face(link.face),
        attenuation: link.attenuation,
        guard: typed_id("guard", link.guard.index()),
    }
}

fn comparator_plan(index: usize, plan: &ComparatorInputPlan) -> ComparatorPlanDto {
    ComparatorPlanDto {
        id: typed_id("comparator_plan", index),
        node: typed_id("node", plan.node.index()),
        rear: typed_id("site", plan.rear.index()),
        direct_override: plan.direct_override.map(comparator_override),
        far_override: plan
            .far_override
            .map(|(site, value, guard)| FarOverrideDto {
                site: typed_id("site", site.index()),
                value: comparator_override(value),
                guard: typed_id("guard", guard.index()),
            }),
        far_threshold: plan.far_threshold,
    }
}

fn comparator_override(value: ComparatorOverride) -> ComparatorOverrideDto {
    match value {
        ComparatorOverride::Literal(value) => ComparatorOverrideDto::Literal { value },
        ComparatorOverride::LiveComponentOutput(node) => {
            ComparatorOverrideDto::LiveComponentOutput {
                node: typed_id("node", node.index()),
            }
        }
    }
}

fn component_state(state: &ComponentState) -> ComponentStateDto {
    match state {
        ComponentState::Electrical(state) => electrical_state(state),
        ComponentState::Wire(wire) => ComponentStateDto::Wire {
            power: wire.power,
            north: wire_side(wire.north),
            south: wire_side(wire.south),
            east: wire_side(wire.east),
            west: wire_side(wire.west),
        },
        ComponentState::Specialized => ComponentStateDto::Specialized,
    }
}

fn electrical_state(state: &NodeState) -> ComponentStateDto {
    ComponentStateDto::Electrical {
        powered: state.powered,
        locked: state.repeater_locked,
        output_strength: state.output_strength,
        pending_tick: state.pending_tick,
    }
}

fn validate_override(
    value: Option<&ComparatorOverrideDto>,
    nodes: &HashSet<String>,
) -> Result<(), ExportError> {
    match value {
        Some(ComparatorOverrideDto::Literal { value }) if *value > 15 => Err(ExportError::new(
            "validation",
            "comparator override exceeds 15",
        )),
        Some(ComparatorOverrideDto::LiveComponentOutput { node }) => {
            check_ref(nodes, node, "comparator override")
        }
        _ => Ok(()),
    }
}

fn unique_ids<'a>(
    table: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<HashSet<String>, ExportError> {
    let prefix = format!("{table}:");
    let mut found = HashSet::new();
    for id in ids {
        let Some(index) = id
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.parse::<usize>().ok())
        else {
            return Err(ExportError::new(
                "validation",
                format!("invalid {table} ID {id}"),
            ));
        };
        let _ = index;
        if !found.insert(id.to_owned()) {
            return Err(ExportError::new(
                "validation",
                format!("duplicate {table} ID {id}"),
            ));
        }
    }
    Ok(found)
}

fn check_ref(ids: &HashSet<String>, id: &str, owner: &str) -> Result<(), ExportError> {
    if ids.contains(id) {
        Ok(())
    } else {
        Err(ExportError::new(
            "validation",
            format!("{owner} references missing ID {id}"),
        ))
    }
}

fn check_optional_ref(
    ids: &HashSet<String>,
    id: Option<&str>,
    owner: &str,
) -> Result<(), ExportError> {
    id.map_or(Ok(()), |id| check_ref(ids, id, owner))
}

fn check_prior_guard(
    ids: &HashSet<String>,
    reference: &str,
    current: &str,
) -> Result<(), ExportError> {
    check_ref(ids, reference, "guard expression")?;
    if id_index(reference) < id_index(current) {
        Ok(())
    } else {
        Err(ExportError::new(
            "validation",
            format!("guard {current} must reference an earlier guard, got {reference}"),
        ))
    }
}

fn id_index(id: &str) -> usize {
    id.split_once(':')
        .and_then(|(_, index)| index.parse().ok())
        .unwrap_or(usize::MAX)
}

fn input_port(port: InputPort) -> &'static str {
    match port {
        InputPort::Main => "main",
        InputPort::Side => "side",
        InputPort::Quasi => "quasi",
    }
}

fn piston_routes_dto(routes: PistonRoutes) -> PistonRouteBindingsDto {
    PistonRouteBindingsDto {
        base_changed: typed_id("route", routes.base_changed.index()),
        near_changed: typed_id("route", routes.near_changed.index()),
        far_changed: routes
            .far_changed
            .map(|route| typed_id("route", route.index())),
        far_shape_changed: routes
            .far_shape_changed
            .map(|route| typed_id("route", route.index())),
        base_restored: typed_id("route", routes.base_restored.index()),
        head_restored: typed_id("route", routes.head_restored.index()),
        resource_restored_near: routes
            .resource_restored_near
            .map(|route| typed_id("route", route.index())),
        resource_restored_far: routes
            .resource_restored_far
            .map(|route| typed_id("route", route.index())),
    }
}

fn callback_link_dto(index: usize, link: &LinkSpec) -> CallbackLinkDto {
    let callback = link.callback;
    let (kind, direction, guard) = match link.kind {
        LinkKind::Update { direction, guard } => ("update", direction.map(face), guard),
        LinkKind::Shape { direction, guard } => ("shape", Some(face(direction)), guard),
    };
    CallbackLinkDto {
        id: typed_id("callback_link", index),
        from: typed_id("node", link.from.index()),
        to: typed_id("node", link.to.index()),
        kind,
        direction,
        guard: typed_id("guard", guard.index()),
        route: typed_id("route", callback.route.index()),
        sequence: callback.sequence,
    }
}

fn operand_view(view: OperandView) -> &'static str {
    match view {
        OperandView::Live => "live",
        OperandView::Captured => "captured",
    }
}

fn piston_availability(value: PistonAvailability) -> &'static str {
    match value {
        PistonAvailability::Ready => "ready",
        PistonAvailability::HeadPending => "head_pending",
        PistonAvailability::PublishingHeadRemoval => "publishing_head_removal",
        PistonAvailability::BasePendingHeadPresent => "base_pending_head_present",
        PistonAvailability::BasePending => "base_pending",
    }
}

fn wire_side(side: RedstoneWireSide) -> &'static str {
    match side {
        RedstoneWireSide::Up => "up",
        RedstoneWireSide::Side => "side",
        RedstoneWireSide::None => "none",
    }
}

fn comparator_mode(mode: ComparatorMode) -> &'static str {
    match mode {
        ComparatorMode::Compare => "compare",
        ComparatorMode::Subtract => "subtract",
    }
}

fn instrument_name(instrument: Instrument) -> &'static str {
    match instrument {
        Instrument::Harp => "harp",
        Instrument::Basedrum => "basedrum",
        Instrument::Snare => "snare",
        Instrument::Hat => "hat",
        Instrument::Bass => "bass",
        Instrument::Flute => "flute",
        Instrument::Bell => "bell",
        Instrument::Guitar => "guitar",
        Instrument::Chime => "chime",
        Instrument::Xylophone => "xylophone",
        Instrument::IronXylophone => "iron_xylophone",
        Instrument::CowBell => "cow_bell",
        Instrument::Didgeridoo => "didgeridoo",
        Instrument::Bit => "bit",
        Instrument::Banjo => "banjo",
        Instrument::Pling => "pling",
    }
}

fn face(value: BlockFace) -> String {
    match value {
        BlockFace::Bottom => "bottom",
        BlockFace::Top => "top",
        BlockFace::North => "north",
        BlockFace::South => "south",
        BlockFace::West => "west",
        BlockFace::East => "east",
    }
    .into()
}

fn position(value: BlockPos) -> PositionDto {
    PositionDto {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

fn typed_id(table: &str, index: usize) -> String {
    format!("{table}:{index}")
}

/// write_artifact writes a complete temporary file and atomically renames it to a generated path.
pub(crate) fn write_artifact(
    directory: &Path,
    plot_x: i32,
    plot_z: i32,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let revision = format!("{timestamp:x}-{:x}-{sequence:x}", std::process::id());
    let destination = directory.join(format!("plot-{plot_x}-{plot_z}-rev-{revision}.json"));
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "generated graph export path already exists",
        ));
    }
    publish_temp_file(directory, &destination, &revision, bytes)?;
    Ok(destination)
}

pub(crate) fn publish_temp_file(
    directory: &Path,
    destination: &Path,
    revision: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let temporary = directory.join(format!(".graph-{revision}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "graph export path already exists",
            ));
        }
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
