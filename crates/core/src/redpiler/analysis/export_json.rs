//! Versioned inspection JSON for the richer graph drafts.
mod classic;
mod fixed_signals;
mod fixed_classic;
mod instant_boundaries;
mod instant_layers;
use super::electrical::{ComparatorInputPlan, ComparatorOverride, WireRelationKind};
use super::{AnalysisLimits, AnalysisReport};
use crate::redpiler::components::{NodeState, NodeType};
use crate::redpiler::passes;
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeKind, CompileGraph, ComponentState, Guard, InputPort, LinkKind, LinkSpec, OperandView, PistonAvailability,
    PistonLane, PistonRoutes, PowerLink, PowerPort, PowerTarget, Profile, ProfileElectrical, SiteSource, SiteTest, StateTest, WireQueryFacts, WireCallbackSelection,
};
use crate::redpiler::{CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument, RedstoneWireSide};
use mchprs_blocks::{BlockFace, BlockPos};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
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
    /// These regions retain the causal proofs from which supported pull kernels are lowered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_layer_regions: Vec<serde_json::Value>,
    /// These whole components remain outside layer compilation with explicit witnesses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_layer_declines: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_cells: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_corridors: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_boundary_ports: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instant_declines: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    classic_nets: Vec<serde_json::Value>,
    /// Live reads execute directly; settled traces expose electrical provenance only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixed_signal_queries: Vec<fixed_signals::QueryDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixed_classic: Option<fixed_classic::PlanDto>,
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
    instant_regions: Vec<InstantRegionDto>,
    instant_proxies: Vec<InstantProxyDto>,
}

#[derive(Debug, Serialize)]
struct InstantRegionDto {
    /// This declares the opt-in falling-edge-to-reset input stability contract for this region.
    stable_inputs: bool,
    /// These wires remain ordered internal state owned by the composed region.
    wires: Vec<String>,
    /// This gives the boundary a stable table reference in this export.
    id: String,
    /// These references retain all actors in the ownership closure.
    actors: Vec<String>,
    /// These references retain conserved payload identities.
    resources: Vec<String>,
    /// These references retain the original reset clock owners.
    resets: Vec<String>,
    /// These references cover all permitted occupied positions.
    contacts: Vec<String>,
    /// These sites expose all region state and geometry to live or captured queries.
    readable_sites: Vec<String>,
    /// These links supply the original Main, Quasi, and conducted reads.
    power_inputs: Vec<String>,
    /// These links expose region power to other owners.
    power_readers: Vec<String>,
    /// These routes originate in the region.
    routes: Vec<String>,
    /// These occurrences enter from another owner in their original order.
    incoming: Vec<RouteOccurrenceDto>,
    /// These occurrences leave the region in their original order.
    outgoing: Vec<RouteOccurrenceDto>,
    /// These wires can capture or sample region sites.
    wire_readers: Vec<String>,
    /// These observers can see intermediate region changes.
    observers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RouteOccurrenceDto {
    /// This identifies the original route without duplicating its deliveries.
    route: String,
    /// This identifies an occurrence rather than collapsing duplicate target deliveries.
    ordinal: u32,
}

#[derive(Debug, Serialize)]
struct InstantProxyDto {
    /// This identifies a certified ordered region program.
    id: String,
    /// This identifies its complete compile-time boundary.
    region: String,
    /// These independent certificates retain private-lane occupancy fast paths.
    private_cells: Vec<PrivateInstantCellDto>,
    /// These programs replace selected raw route deliveries during lowering.
    routes: Vec<InstantRouteDto>,
}

#[derive(Debug, Serialize)]
struct PrivateInstantCellDto {
    /// This identifies the only actor that can occupy this payload lane.
    actor: String,
    /// This retains the original conserved payload identity.
    resource: String,
    /// This identifies the original self-reset clock.
    reset: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InstantOperationDto {
    Dispatch,
    Sample {
        /// This identifies the actor sampled at this exact route position.
        piston: String,
    },
    ArmReset {
        /// This identifies the original clock armed at this exact route position.
        node: String,
    },
}

#[derive(Debug, Serialize)]
struct InstantRouteDto {
    /// This route retains the complete original rows as validation witnesses.
    route: String,
    /// Each operation corresponds to the original row at the same ordinal.
    operations: Vec<InstantOperationDto>,
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
    Electrical {
        component: ElectricalKindDto,
    },
    Observer {
        watched: String,
        facing: String,
        /// This marks a compiled reset clock while retaining the observer's projected identity.
        #[serde(skip_serializing_if = "Option::is_none")]
        builtin_reset: Option<bool>,
    },
    Wire {
        plan: String,
    },
    Piston {
        piston: String,
    },
    Contact {
        contact: String,
    },
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
    Hopper,
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
    State { test: StateTestDto },
    Site { site: String, view: &'static str, test: SiteTestDto },
    Not { guard: String },
    All { guards: Vec<String> },
    Any { guards: Vec<String> },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StateTestDto {
    PistonExtended { piston: String },
    PistonHeadPresent { piston: String },
    PistonAvailability { piston: String, availability: &'static str },
    ResourceAt { resource: String, contact: String },
    ResourceEnabled { resource: String },
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
    #[serde(skip_serializing_if = "Option::is_none")]
    query_facts: Option<WireQueryFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_selection: Option<WireCallbackSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instant_reset: Option<InstantResetDto>,
    /// This selects compiled hop execution using the existing canonical neighbors.
    #[serde(skip_serializing_if = "Option::is_none")]
    hop_root: Option<String>,
}

#[derive(Debug, Serialize)]
struct InstantResetDto {
    /// This references the actor whose private reset footprint was certified.
    piston: String,
    /// This references the existing movable-resource owner.
    resource: String,
    /// This references the existing base observer and its scheduler target.
    observer: String,
    /// This references the second wire whose general Turbo execution is absorbed.
    partner: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    execution: Option<&'static str>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    execution: Option<&'static str>,
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
        serde_json::to_vec_pretty(self).map_err(|error| ExportError::new("serialization", error.to_string()))
    }

    fn validate(&self) -> Result<(), ExportError> {
        if self.format != "mchprs.redpiler.graph" || self.version.major != 1 {
            return Err(ExportError::new("schema", "invalid graph JSON format header"));
        }
        if self.stage != "callback_draft" {
            return Err(ExportError::new("stage", "draft export has an invalid stage"));
        }
        instant_boundaries::validate(&self.graph)?;
        instant_layers::validate(&self.graph)?;
        let nodes = unique_ids("node", self.graph.nodes.iter().map(|item| item.id.as_str()))?;
        let profiles = unique_ids("profile", self.graph.profiles.iter().map(|item| item.id.as_str()))?;
        let sites = unique_ids("site", self.graph.sites.iter().map(|item| item.id.as_str()))?;
        let guards = unique_ids("guard", self.graph.guards.iter().map(|item| item.id.as_str()))?;
        let contacts = unique_ids("contact", self.graph.contacts.iter().map(|item| item.id.as_str()))?;
        let pistons = unique_ids("piston", self.graph.pistons.iter().map(|item| item.id.as_str()))?;
        let resources = unique_ids("resource", self.graph.resources.iter().map(|item| item.id.as_str()))?;
        let wires = unique_ids("wire", self.graph.wire_plans.iter().map(|item| item.id.as_str()))?;
        let node_records: HashMap<_, _> = self.graph.nodes.iter().map(|item| (item.id.as_str(), item)).collect();
        let site_records: HashMap<_, _> = self.graph.sites.iter().map(|item| (item.id.as_str(), item)).collect();
        let routes = self
            .graph
            .routes
            .as_ref()
            .map(|items| unique_ids("route", items.iter().map(|item| item.id.as_str())))
            .transpose()?
            .unwrap_or_default();
        unique_ids("power_link", self.graph.power_links.iter().map(|item| item.id.as_str()))?;
        unique_ids("wire_relation", self.graph.wire_relations.iter().map(|item| item.id.as_str()))?;
        unique_ids("callback_link", self.graph.callback_links.iter().map(|item| item.id.as_str()))?;
        unique_ids("comparator_plan", self.graph.comparator_plans.iter().map(|item| item.id.as_str()))?;

        for node in &self.graph.nodes {
            check_optional_ref(&routes, node.output_route.as_deref(), "node output route")?;
            match &node.kind {
                NodeKindDto::Electrical { .. } => {}
                NodeKindDto::Observer { watched, builtin_reset, .. } => {
                    check_ref(&sites, watched, "observer site")?;
                    if *builtin_reset == Some(true) && node.output_route.is_some() {
                        return Err(ExportError::new("invalid_builtin_reset", "built-in reset has an output callback route"));
                    }
                }
                NodeKindDto::Wire { plan } => check_ref(&wires, plan, "wire plan")?,
                NodeKindDto::Piston { piston } => check_ref(&pistons, piston, "node piston")?,
                NodeKindDto::Contact { contact } => check_ref(&contacts, contact, "node contact")?,
            }
        }
        for site in &self.graph.sites {
            match &site.source {
                SiteSourceDto::Component { node } => check_ref(&nodes, node, "site component")?,
                SiteSourceDto::Contact { contact } => check_ref(&contacts, contact, "site contact")?,
                SiteSourceDto::Constant { profile } => check_ref(&profiles, profile, "site profile")?,
            }
        }
        for (index, guard) in self.graph.guards.iter().enumerate() {
            let current = typed_id("guard", index);
            match &guard.expression {
                GuardExpressionDto::Always => {}
                GuardExpressionDto::State { test } => match test {
                    StateTestDto::PistonExtended { piston }
                    | StateTestDto::PistonHeadPresent { piston }
                    | StateTestDto::PistonAvailability { piston, .. } => check_ref(&pistons, piston, "guard piston")?,
                    StateTestDto::ResourceAt { resource, contact } => {
                        check_ref(&resources, resource, "guard resource")?;
                        check_ref(&contacts, contact, "guard contact")?;
                    }
                    StateTestDto::ResourceEnabled { resource } => check_ref(&resources, resource, "guard resource")?,
                },
                GuardExpressionDto::Site { site, test, .. } => {
                    check_ref(&sites, site, "guard site")?;
                    if let SiteTestDto::Profile { profile } = test {
                        check_ref(&profiles, profile, "guard profile")?;
                    }
                }
                GuardExpressionDto::Not { guard } => check_prior_guard(&guards, guard, &current)?,
                GuardExpressionDto::All { guards: references } | GuardExpressionDto::Any { guards: references } => {
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
                        ReceiverDto::Component { node } => check_ref(&nodes, node, "receiver node")?,
                        ReceiverDto::PistonBase { piston } | ReceiverDto::PistonHead { piston } => check_ref(&pistons, piston, "receiver piston")?,
                        ReceiverDto::Contact { contact } => check_ref(&contacts, contact, "receiver contact")?,
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
                PowerTargetDto::Conductor { site, .. } => check_ref(&sites, site, "conductor target")?,
            }
        }
        let mut instant_wires = HashSet::new();
        for wire in &self.graph.wire_plans {
            if wire.callback_selection.is_some_and(|selection| !selection.valid()) {
                return Err(ExportError::new("validation", "invalid compiled wire callback masks"));
            }
            check_ref(&nodes, &wire.node, "wire node")?;
            if let Some(root) = &wire.hop_root {
                check_ref(&sites, root, "wire hop root")?;
                let site = site_records[root.as_str()];
                if !matches!(&site.source, SiteSourceDto::Component { node } if node == &wire.node) {
                    return Err(ExportError::new("validation", "wire hop root must bind its component"));
                }
            }
            if wire.canonical_neighbors.len() != 24 {
                return Err(ExportError::new("validation", "wire plan must contain exactly 24 neighbor sites"));
            }
            for site in &wire.canonical_neighbors {
                check_ref(&sites, site, "wire neighbor")?;
            }
            check_optional_ref(&routes, wire.shape_route.as_deref(), "wire shape route")?;
            check_optional_ref(&routes, wire.turbo_route.as_deref(), "wire turbo route")?;
            validate_wire_query_facts(wire.query_facts.as_ref())?;
            if let Some(instant) = &wire.instant_reset {
                check_ref(&pistons, &instant.piston, "instant reset piston")?;
                check_ref(&resources, &instant.resource, "instant reset resource")?;
                check_ref(&nodes, &instant.observer, "instant reset observer")?;
                check_ref(&nodes, &instant.partner, "instant reset partner")?;
                if !matches!(node_records[wire.node.as_str()].kind, NodeKindDto::Wire { .. })
                    || !matches!(node_records[instant.partner.as_str()].kind, NodeKindDto::Wire { .. })
                    || !matches!(node_records[instant.observer.as_str()].kind, NodeKindDto::Observer { .. })
                    || !instant_wires.insert(wire.node.as_str())
                    || !instant_wires.insert(instant.partner.as_str())
                {
                    return Err(ExportError::new(
                        "validation",
                        "instant reset has invalid or overlapping component bindings",
                    ));
                }
            }
        }
        validate_dust_paths(&self.graph.dust_paths, &nodes, &sites, &node_records, &site_records)?;
        classic::validate(&self.graph.classic_nets,&nodes,&routes,&self.graph)?;
        fixed_signals::validate(&self.graph.fixed_signal_queries, &nodes, &sites)?;
        fixed_classic::validate(&self.graph.fixed_classic, &self.graph, &nodes, &sites, &routes)?;
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
                return Err(ExportError::new("validation", "comparator threshold exceeds 15"));
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
                            ReceiverDto::Component { node } => check_ref(&nodes, node, "route receiver")?,
                            ReceiverDto::PistonBase { piston } | ReceiverDto::PistonHead { piston } => check_ref(&pistons, piston, "route receiver")?,
                            ReceiverDto::Contact { contact } => check_ref(&contacts, contact, "route receiver")?,
                        }
                    }
                }
            }
        }
        let regions = unique_ids("instant_region", self.graph.instant_regions.iter().map(|region| region.id.as_str()))?;
        let _ = unique_ids("instant_proxy", self.graph.instant_proxies.iter().map(|proxy| proxy.id.as_str()))?;
        let power = unique_ids("power_link", self.graph.power_links.iter().map(|link| link.id.as_str()))?;
        let route_rows: HashMap<_, _> = self
            .graph
            .routes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|route| (route.id.as_str(), route.deliveries.len()))
            .collect();
        for region in &self.graph.instant_regions {
            for actor in &region.actors {
                check_ref(&pistons, actor, "region actor")?;
            }
            for resource in &region.resources {
                check_ref(&resources, resource, "region resource")?;
            }
            for node in region
                .resets
                .iter()
                .chain(&region.wires)
                .chain(&region.wire_readers)
                .chain(&region.observers)
            {
                check_ref(&nodes, node, "region component")?;
            }
            for contact in &region.contacts {
                check_ref(&contacts, contact, "region contact")?;
            }
            for site in &region.readable_sites {
                check_ref(&sites, site, "region readable site")?;
            }
            for link in region.power_inputs.iter().chain(&region.power_readers) {
                check_ref(&power, link, "region power")?;
            }
            for route in &region.routes {
                check_ref(&routes, route, "region route")?;
            }
            for event in region.incoming.iter().chain(&region.outgoing) {
                if !route_rows.get(event.route.as_str()).is_some_and(|&len| (event.ordinal as usize) < len) {
                    return Err(ExportError::new("validation", "invalid region route occurrence"));
                }
            }
        }
        let mut assigned = HashSet::new();
        for proxy in &self.graph.instant_proxies {
            check_ref(&regions, &proxy.region, "proxy region")?;
            let region = self
                .graph
                .instant_regions
                .iter()
                .find(|region| region.id == proxy.region)
                .expect("checked region");
            for cell in &proxy.private_cells {
                if !region.actors.contains(&cell.actor) || !region.resources.contains(&cell.resource) || !region.resets.contains(&cell.reset) {
                    return Err(ExportError::new("validation", "private cell lies outside its proxy region"));
                }
            }
            for route in &proxy.routes {
                if !region.routes.contains(&route.route)
                    || !assigned.insert(&route.route)
                    || route_rows.get(route.route.as_str()) != Some(&route.operations.len())
                {
                    return Err(ExportError::new("validation", "invalid proxy route program"));
                }
                for operation in &route.operations {
                    let valid = match operation {
                        InstantOperationDto::Dispatch => true,
                        InstantOperationDto::Sample { piston } => region.actors.contains(piston),
                        InstantOperationDto::ArmReset { node } => region.resets.contains(node),
                    };
                    if !valid {
                        return Err(ExportError::new("validation", "proxy operation targets another region"));
                    }
                }
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
    let mut prepared = passes::rich::prepare(world, bounds, ticks.to_vec(), monitor, limits).map_err(map_preparation_error)?;
    let pipeline = passes::default_graph_pipeline(options);
    passes::run_graph_pipeline(&mut prepared.graph, options, &prepared.entry, &pipeline, monitor).map_err(map_pass_error)?;
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
    let path = write_artifact(directory, plot_x, plot_z, &bytes).map_err(|error| ExportError::new("write", error.to_string()))?;
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
                super::geometry::GeometryError::Analysis(super::AnalysisError::Cancelled) => "cancelled",
                super::geometry::GeometryError::Analysis(_) => "analysis",
                super::geometry::GeometryError::Entry(_) => "admission",
                super::geometry::GeometryError::Rejected(_) => "geometry",
            };
            (code, error.to_string())
        }
        PreparationError::Electrical(error) => {
            let code = match error {
                super::electrical::ElectricalError::Preparation(super::AnalysisError::Cancelled) => "cancelled",
                super::electrical::ElectricalError::Preparation(_) => "analysis",
                _ => "electrical",
            };
            (code, error.to_string())
        }
        PreparationError::Callbacks(error) => {
            let code = match error {
                super::callbacks::CallbackError::Preparation(super::AnalysisError::Cancelled) => "cancelled",
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

pub(crate) fn from_graph(report: &AnalysisReport, source: &CompileGraph) -> Result<GraphExport, ExportError> {
    if source.plans.instant.boundaries.is_some() {
        source.validate_with_monitor(&TaskMonitor::default()).map_err(|error| ExportError::new("instant_boundaries", error.to_string()))?;
    }
    let boundaries = instant_boundaries::export(source);
    let layers = instant_layers::export(source);
    let graph = GraphTablesDto {
        instant_layer_regions: layers.regions,
        instant_layer_declines: layers.declines,
        instant_cells: boundaries.cells,
        instant_corridors: boundaries.corridors,
        instant_boundary_ports: boundaries.ports,
        instant_declines: boundaries.declines,
        classic_nets: classic::export(source),
        fixed_signal_queries: fixed_signals::export(source),
        fixed_classic: fixed_classic::export(&source.plans.fixed_classic),
        nodes: source
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                Ok(NodeDto {
                    id: typed_id("node", index),
                    kind: node_kind(&node.kind)?,
                    position: node.position.map(position),
                    output_route: node.output_route.map(|route| typed_id("route", route.index())),
                })
            })
            .collect::<Result<_, ExportError>>()?,
        profiles: source.profiles.iter().enumerate().map(|(index, item)| profile_dto(index, item)).collect(),
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
                contacts: item.contacts.iter().map(|contact| typed_id("contact", contact.index())).collect(),
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
                canonical_neighbors: item.canonical_neighbors.iter().map(|site| typed_id("site", site.index())).collect(),
                shape_route: item.shape_route.map(|route| typed_id("route", route.index())),
                turbo_route: item.turbo_route.map(|route| typed_id("route", route.index())),
                query_facts: item.execution.query_facts,
                callback_selection: item.execution.callback_selection,
                hop_root: item.execution.hop_root.map(|site| typed_id("site", site.index())),
                instant_reset: item.execution.instant_reset.map(|plan| InstantResetDto {
                    piston: typed_id("piston", plan.piston.index()),
                    resource: typed_id("resource", plan.resource.index()),
                    observer: typed_id("node", plan.observer.index()),
                    partner: typed_id("node", plan.partner.index()),
                }),
            })
            .collect(),
        dust_paths: source
            .plans
            .dust_paths
            .iter()
            .enumerate()
            .map(|(index, path)| DustPathDto {
                id: typed_id("dust_path", index),
                input_node: typed_id("node", path.input_node.index()),
                input_site: typed_id("site", path.input_site.index()),
                input_face: face(path.input_face),
                wires: path.wires.iter().map(|node| typed_id("node", node.index())).collect(),
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
        instant_regions: source
            .plans
            .instant
            .regions
            .iter()
            .enumerate()
            .map(|(index, region)| InstantRegionDto {
                id: typed_id("instant_region", index),
                stable_inputs: region.stable_inputs,
                wires: region.wires.iter().map(|id| typed_id("node", id.index())).collect(),
                actors: region.actors.iter().map(|id| typed_id("piston", id.index())).collect(),
                resources: region.resources.iter().map(|id| typed_id("resource", id.index())).collect(),
                resets: region.resets.iter().map(|id| typed_id("node", id.index())).collect(),
                contacts: region.contacts.iter().map(|id| typed_id("contact", id.index())).collect(),
                readable_sites: region.readable_sites.iter().map(|id| typed_id("site", id.index())).collect(),
                power_inputs: region.power_inputs.iter().map(|&id| typed_id("power_link", id as usize)).collect(),
                power_readers: region.power_readers.iter().map(|&id| typed_id("power_link", id as usize)).collect(),
                routes: region.routes.iter().map(|id| typed_id("route", id.index())).collect(),
                incoming: region
                    .incoming
                    .iter()
                    .map(|event| RouteOccurrenceDto {
                        route: typed_id("route", event.route.index()),
                        ordinal: event.ordinal,
                    })
                    .collect(),
                outgoing: region
                    .outgoing
                    .iter()
                    .map(|event| RouteOccurrenceDto {
                        route: typed_id("route", event.route.index()),
                        ordinal: event.ordinal,
                    })
                    .collect(),
                wire_readers: region.wire_readers.iter().map(|id| typed_id("node", id.index())).collect(),
                observers: region.observers.iter().map(|id| typed_id("node", id.index())).collect(),
            })
            .collect(),
        instant_proxies: source
            .plans
            .instant
            .proxies
            .iter()
            .enumerate()
            .map(|(index, proxy)| InstantProxyDto {
                id: typed_id("instant_proxy", index),
                region: typed_id("instant_region", proxy.region),
                private_cells: proxy
                    .private_cells
                    .iter()
                    .map(|cell| PrivateInstantCellDto {
                        actor: typed_id("piston", cell.actor.index()),
                        resource: typed_id("resource", cell.resource.index()),
                        reset: typed_id("node", cell.reset.index()),
                    })
                    .collect(),
                routes: proxy
                    .routes
                    .iter()
                    .map(|route| InstantRouteDto {
                        route: typed_id("route", route.route.index()),
                        operations: route
                            .steps
                            .iter()
                            .map(|step| match step.kind {
                                crate::redpiler::program_graph::InstantStepKind::Dispatch => InstantOperationDto::Dispatch,
                                crate::redpiler::program_graph::InstantStepKind::Sample(piston) => InstantOperationDto::Sample {
                                    piston: typed_id("piston", piston.index()),
                                },
                                crate::redpiler::program_graph::InstantStepKind::ArmReset(node) => InstantOperationDto::ArmReset {
                                    node: typed_id("node", node.index()),
                                },
                            })
                            .collect(),
                    })
                    .collect(),
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
            .map(|(index, link)| {
                let mut dto = callback_link_dto(index, link);
                if classic::owned(source,link.from) || classic::owned(source,link.to) { dto.execution=Some("classic_provenance"); }
                dto
            })
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
                            execution: (classic::owned(source,route.source) || source.plans.classic_nets.iter().any(|net|net.entries.iter().any(|entry|entry.route.index()==index && entry.ordinal as usize==ordinal))).then_some("classic_provenance"),
                            ordinal: ordinal as u32,
                            sequence: delivery.sequence,
                            target: typed_id("site", delivery.target.index()),
                            kind: match delivery.kind {
                                crate::redpiler::program_graph::DeliveryKind::Update { .. } => "update",
                                crate::redpiler::program_graph::DeliveryKind::Shape { .. } => "shape",
                            },
                            direction: match delivery.kind {
                                crate::redpiler::program_graph::DeliveryKind::Update { direction } => direction.map(face),
                                crate::redpiler::program_graph::DeliveryKind::Shape { direction } => Some(face(direction)),
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
    let mut capabilities = vec![
        "electrical",
        "geometry",
        "guarded_contacts",
        "initial_snapshot",
        "wire_relations",
        "comparator_plans",
        "callback_routes",
        "callback_links",
        "piston_routes",
    ];
    if !source.plans.dust_paths.is_empty() {
        capabilities.push("dust_path_certificates");
    }
    if !source.plans.classic_nets.is_empty() { capabilities.push("classic_signal_nets"); }
    if source.wires.iter().any(|wire| wire.execution.query_facts.is_some()) {
        capabilities.push("wire_query_facts");
    }
    if !source.plans.instant.regions.is_empty() {
        capabilities.push("instant_regions");
    }
    if source.plans.instant.boundaries.is_some() {
        capabilities.push("instant_boundaries");
    }
    if source.plans.instant.layers.is_some() {
        capabilities.push("instant_layers");
    }
    if !source.plans.instant.proxies.is_empty() {
        capabilities.push("instant_proxies");
    }
    if source.wires.iter().any(|wire| wire.execution.instant_reset.is_some()) {
        capabilities.push("instant_reset_plans");
    }
    if source.plans.fixed_classic.program.is_some() {
        capabilities.push("fixed_classic_program");
    } else if source.plans.fixed_classic.decline.is_some() {
        capabilities.push("fixed_classic_declined");
    }
    let export = GraphExport {
        format: "mchprs.redpiler.graph",
        version: VersionDto { major: 1, minor: 3 },
        stage: "callback_draft",
        capabilities,
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
        CircuitNodeKind::Observer { watched, facing } | CircuitNodeKind::BuiltinReset { watched, facing } => NodeKindDto::Observer {
            watched: typed_id("site", watched.index()),
            facing: face(*facing),
            builtin_reset: matches!(kind, CircuitNodeKind::BuiltinReset { .. }).then_some(true),
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
        NodeType::Repeater { delay, facing_diode } => ElectricalKindDto::Repeater {
            delay: *delay,
            facing_diode: *facing_diode,
        },
        NodeType::Torch => ElectricalKindDto::Torch,
        NodeType::Observer { watched } => ElectricalKindDto::Observer { watched: position(*watched) },
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
        NodeType::Hopper => ElectricalKindDto::Hopper,
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
            guards: guards.iter().map(|guard| typed_id("guard", guard.index())).collect(),
        },
        Guard::Any(guards) => GuardExpressionDto::Any {
            guards: guards.iter().map(|guard| typed_id("guard", guard.index())).collect(),
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
                PowerPort::Weak(face_value) => PowerPortDto::Weak { face: face(face_value) },
                PowerPort::Strong(face_value) => PowerPortDto::Strong { face: face(face_value) },
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
        far_override: plan.far_override.map(|(site, value, guard)| FarOverrideDto {
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
        ComparatorOverride::LiveComponentOutput(node) => ComparatorOverrideDto::LiveComponentOutput {
            node: typed_id("node", node.index()),
        },
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

fn validate_override(value: Option<&ComparatorOverrideDto>, nodes: &HashSet<String>) -> Result<(), ExportError> {
    match value {
        Some(ComparatorOverrideDto::Literal { value }) if *value > 15 => Err(ExportError::new("validation", "comparator override exceeds 15")),
        Some(ComparatorOverrideDto::LiveComponentOutput { node }) => check_ref(nodes, node, "comparator override"),
        _ => Ok(()),
    }
}

fn unique_ids<'a>(table: &str, ids: impl Iterator<Item = &'a str>) -> Result<HashSet<String>, ExportError> {
    let prefix = format!("{table}:");
    let mut found = HashSet::new();
    for id in ids {
        let Some(index) = id.strip_prefix(&prefix).and_then(|suffix| suffix.parse::<usize>().ok()) else {
            return Err(ExportError::new("validation", format!("invalid {table} ID {id}")));
        };
        let _ = index;
        if !found.insert(id.to_owned()) {
            return Err(ExportError::new("validation", format!("duplicate {table} ID {id}")));
        }
    }
    Ok(found)
}

fn check_ref(ids: &HashSet<String>, id: &str, owner: &str) -> Result<(), ExportError> {
    if ids.contains(id) {
        Ok(())
    } else {
        Err(ExportError::new("validation", format!("{owner} references missing ID {id}")))
    }
}

fn check_optional_ref(ids: &HashSet<String>, id: Option<&str>, owner: &str) -> Result<(), ExportError> {
    id.map_or(Ok(()), |id| check_ref(ids, id, owner))
}

fn validate_dust_paths(
    paths: &[DustPathDto],
    nodes: &HashSet<String>,
    sites: &HashSet<String>,
    node_records: &HashMap<&str, &NodeDto>,
    site_records: &HashMap<&str, &SiteDto>,
) -> Result<(), ExportError> {
    unique_ids("dust_path", paths.iter().map(|path| path.id.as_str()))?;
    let mut assigned_wires = HashSet::new();
    for path in paths {
        check_ref(nodes, &path.input_node, "dust path input")?;
        check_ref(sites, &path.input_site, "dust path input site")?;
        if !matches!(
            node_records.get(path.input_node.as_str()).map(|node| &node.kind),
            Some(NodeKindDto::Electrical { .. })
        ) {
            return Err(ExportError::new("validation", format!("{} input must be an electrical node", path.id)));
        }
        if !matches!(
            site_records
                .get(path.input_site.as_str())
                .map(|site| &site.source),
            Some(SiteSourceDto::Component { node }) if node == &path.input_node
        ) {
            return Err(ExportError::new(
                "validation",
                format!("{} input site must be owned by its input node", path.id),
            ));
        }
        if !matches!(path.input_face.as_str(), "north" | "south" | "east" | "west") {
            return Err(ExportError::new("validation", format!("{} input face must be horizontal", path.id)));
        }
        if path.wires.len() < 2 {
            return Err(ExportError::new(
                "validation",
                format!("{} must contain at least two wire nodes", path.id),
            ));
        }
        let mut path_wires = HashSet::new();
        for wire in &path.wires {
            check_ref(nodes, wire, "dust path wire")?;
            if !matches!(node_records.get(wire.as_str()).map(|node| &node.kind), Some(NodeKindDto::Wire { .. })) {
                return Err(ExportError::new("validation", format!("{} references non-wire node {wire}", path.id)));
            }
            if !path_wires.insert(wire.as_str()) || !assigned_wires.insert(wire.as_str()) {
                return Err(ExportError::new("validation", format!("{} repeats or shares wire node {wire}", path.id)));
            }
        }
    }
    Ok(())
}

fn check_prior_guard(ids: &HashSet<String>, reference: &str, current: &str) -> Result<(), ExportError> {
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
    id.split_once(':').and_then(|(_, index)| index.parse().ok()).unwrap_or(usize::MAX)
}

fn validate_wire_query_facts(facts: Option<&WireQueryFacts>) -> Result<(), ExportError> {
    let Some(facts) = facts else {
        return Ok(());
    };
    let properties = facts.solid | facts.transparent | facts.wire | facts.zero_no_dust;
    if facts.known >> 24 != 0
        || properties & !facts.known != 0
        || facts.zero_no_dust & facts.solid != 0
        || !facts.raw_sides_valid()
        || facts
            .transfer
            .is_some_and(|transfer| !transfer.seed.valid() || !transfer.turbo.valid() || transfer.turbo.direct & 0xc != 0)
    {
        return Err(ExportError::new("validation", "invalid wire query fact masks"));
    }
    Ok(())
}

#[cfg(test)]
mod wire_query_fact_validation_tests {
    use super::*;

    #[test]
    fn compiled_hop_exports_validate_the_wire_root_binding() {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let options = CompilerOptions {
            optimization_level: Some(2),
            ..Default::default()
        };
        let mut export = prepare(&world, bounds, &[], &options, &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
        let index = export.graph.wire_plans.iter().position(|wire| wire.hop_root.is_some()).unwrap();
        export.validate().unwrap();
        let saved = export.graph.wire_plans[index].hop_root.clone();
        export.graph.wire_plans[index].hop_root = Some("site:999999".into());
        assert!(export.validate().is_err());
        export.graph.wire_plans[index].hop_root = Some(export.graph.wire_plans[index + 1].hop_root.clone().unwrap());
        assert!(export.validate().is_err());
        export.graph.wire_plans[index].hop_root = saved;
        export.validate().unwrap();
    }

    #[test]
    fn instant_reset_export_retains_typed_bindings_and_rejects_wrong_roles() {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let options = CompilerOptions {
            optimization_level: Some(3),
            ..Default::default()
        };
        let mut export = prepare(&world, bounds, &[], &options, &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
        let index = export
            .graph
            .wire_plans
            .iter()
            .position(|wire| wire.instant_reset.is_some())
            .expect("the original fixture is certified");
        export.validate().unwrap();
        let json = serde_json::to_value(&export).unwrap();
        assert!(export.capabilities.contains(&"instant_reset_plans"));
        assert!(
            json["graph"]["wire_plans"][index]["instant_reset"]["piston"]
                .as_str()
                .unwrap()
                .starts_with("piston:")
        );
        let observer = export.graph.wire_plans[index].instant_reset.as_ref().unwrap().observer.clone();
        export.graph.wire_plans[index].instant_reset.as_mut().unwrap().observer = "node:999999".into();
        assert!(export.validate().is_err());
        export.graph.wire_plans[index].instant_reset.as_mut().unwrap().observer = observer.clone();
        let partner = export.graph.wire_plans[index].instant_reset.as_ref().unwrap().partner.clone();
        export.graph.wire_plans[index].instant_reset.as_mut().unwrap().partner = observer;
        assert!(export.validate().is_err());
        export.graph.wire_plans[index].instant_reset.as_mut().unwrap().partner = partner;
        export.validate().unwrap();
    }

    #[test]
    fn serializes_stationary_hopper_component_kind() {
        assert_eq!(
            serde_json::to_value(electrical_kind(&NodeType::Hopper).unwrap()).unwrap()["type"],
            "hopper"
        );
    }

    fn facts() -> WireQueryFacts {
        WireQueryFacts {
            known: 0xffffff,
            solid: 1,
            transparent: 2,
            wire: 4,
            zero_no_dust: 0xfffffe,
            raw_sides: None,
            isolated: false,
            transfer: None,
        }
    }

    #[test]
    fn validates_compiled_wire_inputs_without_requiring_the_additive_field() {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let mut export = prepare(
            &world,
            bounds,
            &[],
            &CompilerOptions {
                optimization_level: Some(2),
                ..Default::default()
            },
            &TaskMonitor::default(),
            AnalysisLimits::default(),
        )
        .unwrap();
        let facts = export.graph.wire_plans.iter_mut().find_map(|wire| wire.query_facts.as_mut()).unwrap();
        let original = *facts;
        assert!(original.transfer.is_some());
        for corrupt in 0..5 {
            let mut bad = original;
            let transfer = bad.transfer.as_mut().unwrap();
            match corrupt {
                0 => transfer.seed.direct |= 1 << 24,
                1 => transfer.seed.direct |= 1 << 23,
                2 => transfer.seed.conditional |= 1 << 4,
                3 => {
                    transfer.seed.direct |= 1 << 7;
                    transfer.seed.conditional |= 1 << 7;
                }
                _ => transfer.turbo.direct |= 1 << 2,
            }
            assert!(validate_wire_query_facts(Some(&bad)).is_err());
        }
        facts.transfer = None;
        export.validate().unwrap();
    }

    #[test]
    fn validates_raw_side_encoding_without_requiring_it_in_older_exports() {
        let mut facts = facts();
        for bits in 0..=255u8 {
            facts.raw_sides = Some(bits);
            let valid = (0..4).all(|index| (bits >> (2 * index)) & 3 != 3);
            assert_eq!(validate_wire_query_facts(Some(&facts)).is_ok(), valid);
        }
        facts.raw_sides = Some(0);
        facts.known &= !1;
        assert!(validate_wire_query_facts(Some(&facts)).is_err());
        facts.raw_sides = None;
        facts.solid = 0;
        validate_wire_query_facts(Some(&facts)).unwrap();
    }

    #[test]
    fn accepts_valid_query_fact_masks_and_absent_older_field() {
        validate_wire_query_facts(Some(&facts())).unwrap();
        validate_wire_query_facts(None).unwrap();
        let mut wire = WirePlanDto {
            id: "wire:0".into(),
            node: "node:0".into(),
            canonical_neighbors: vec!["site:0".into(); 24],
            shape_route: None,
            turbo_route: None,
            query_facts: None,
            callback_selection: None,
            instant_reset: None,
            hop_root: None,
        };
        assert!(serde_json::to_value(&wire).unwrap().get("query_facts").is_none());
        wire.query_facts = Some(facts());
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["query_facts"]["known"], 0xffffff);
        assert_eq!(json["query_facts"]["zero_no_dust"], 0xfffffe);
        assert_eq!(json["query_facts"]["isolated"], false);
        wire.query_facts.as_mut().unwrap().isolated = true;
        assert_eq!(serde_json::to_value(&wire).unwrap()["query_facts"]["isolated"], true);
    }

    #[test]
    fn rejects_out_of_range_unknown_and_contradictory_query_fact_masks() {
        for bad in [
            WireQueryFacts { known: 1 << 24, ..facts() },
            WireQueryFacts { solid: 1 << 24, ..facts() },
            WireQueryFacts {
                transparent: 1 << 24,
                ..facts()
            },
            WireQueryFacts { wire: 1 << 24, ..facts() },
            WireQueryFacts {
                zero_no_dust: 1 << 24,
                ..facts()
            },
            WireQueryFacts { known: 0, ..facts() },
            WireQueryFacts { zero_no_dust: 1, ..facts() },
        ] {
            assert!(validate_wire_query_facts(Some(&bad)).is_err());
        }
    }
}



#[cfg(test)]
mod dust_path_validation_tests {
    use super::*;

    fn nodes() -> Vec<NodeDto> {
        vec![
            NodeDto {
                id: "node:0".into(),
                kind: NodeKindDto::Electrical {
                    component: ElectricalKindDto::Lever,
                },
                position: None,
                output_route: None,
            },
            NodeDto {
                id: "node:1".into(),
                kind: NodeKindDto::Wire { plan: "wire:0".into() },
                position: None,
                output_route: None,
            },
            NodeDto {
                id: "node:2".into(),
                kind: NodeKindDto::Wire { plan: "wire:1".into() },
                position: None,
                output_route: None,
            },
        ]
    }

    fn path(id: &str, wires: &[&str]) -> DustPathDto {
        DustPathDto {
            id: id.into(),
            input_node: "node:0".into(),
            input_site: "site:0".into(),
            input_face: "east".into(),
            wires: wires.iter().map(|wire| (*wire).into()).collect(),
        }
    }

    fn validate(paths: &[DustPathDto]) -> Result<(), ExportError> {
        let nodes = nodes();
        let sites = vec![SiteDto {
            id: "site:0".into(),
            position: PositionDto { x: 0, y: 0, z: 0 },
            source: SiteSourceDto::Component { node: "node:0".into() },
        }];
        let node_ids = nodes.iter().map(|node| node.id.clone()).collect();
        let site_ids = sites.iter().map(|site| site.id.clone()).collect();
        let node_records = nodes.iter().map(|node| (node.id.as_str(), node)).collect();
        let site_records = sites.iter().map(|site| (site.id.as_str(), site)).collect();
        validate_dust_paths(paths, &node_ids, &site_ids, &node_records, &site_records)
    }

    #[test]
    fn accepts_a_well_formed_dust_path_certificate() {
        validate(&[path("dust_path:0", &["node:1", "node:2"])]).expect("valid dust path references should export");
    }

    #[test]
    fn rejects_dangling_dust_path_references() {
        let error = validate(&[path("dust_path:0", &["node:1", "node:99"])]).expect_err("unknown wire node must fail export validation");
        assert!(error.to_string().contains("missing ID node:99"));
    }

    #[test]
    fn rejects_non_horizontal_dust_path_input_faces() {
        let mut certificate = path("dust_path:0", &["node:1", "node:2"]);
        certificate.input_face = "up".into();
        assert!(
            validate(&[certificate])
                .unwrap_err()
                .to_string()
                .contains("input face must be horizontal")
        );
    }

    #[test]
    fn rejects_duplicate_dust_path_ids_and_wire_ownership() {
        let duplicate_ids = [path("dust_path:0", &["node:1", "node:2"]), path("dust_path:0", &["node:1", "node:2"])];
        assert!(validate(&duplicate_ids).unwrap_err().to_string().contains("duplicate dust_path ID"));

        let shared_wire = [path("dust_path:0", &["node:1", "node:2"]), path("dust_path:1", &["node:1", "node:2"])];
        assert!(validate(&shared_wire).unwrap_err().to_string().contains("repeats or shares wire node"));
    }
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
        far_changed: routes.far_changed.map(|route| typed_id("route", route.index())),
        far_shape_changed: routes.far_shape_changed.map(|route| typed_id("route", route.index())),
        base_restored: typed_id("route", routes.base_restored.index()),
        head_restored: typed_id("route", routes.head_restored.index()),
        resource_restored_near: routes.resource_restored_near.map(|route| typed_id("route", route.index())),
        resource_restored_far: routes.resource_restored_far.map(|route| typed_id("route", route.index())),
    }
}

fn callback_link_dto(index: usize, link: &LinkSpec) -> CallbackLinkDto {
    let callback = link.callback;
    let (kind, direction, guard) = match link.kind {
        LinkKind::Update { direction, guard } => ("update", direction.map(face), guard),
        LinkKind::Shape { direction, guard } => ("shape", Some(face(direction)), guard),
    };
    CallbackLinkDto {
        execution: None,
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
pub(crate) fn write_artifact(directory: &Path, plot_x: i32, plot_z: i32, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let revision = format!("{timestamp:x}-{:x}-{sequence:x}", std::process::id());
    let destination = directory.join(format!("plot-{plot_x}-{plot_z}-rev-{revision}.json"));
    if destination.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "generated graph export path already exists"));
    }
    publish_temp_file(directory, &destination, &revision, bytes)?;
    Ok(destination)
}

pub(crate) fn publish_temp_file(directory: &Path, destination: &Path, revision: &str, bytes: &[u8]) -> io::Result<()> {
    let temporary = directory.join(format!(".graph-{revision}.tmp"));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if destination.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "graph export path already exists"));
        }
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
