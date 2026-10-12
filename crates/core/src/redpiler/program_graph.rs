//! Typed construction and immutable program storage for the graph-native model.
//!
//! This graph is deliberately separate from `compile_graph::CompileGraph`, the
//! legacy electrical graph consumed by its rewrites, packed backend, and export.
#![allow(dead_code)] // This representation is consumed by the parser and backend in later steps.

use crate::redpiler::compile_graph::{NodeState, NodeType};
use mchprs_blocks::blocks::RedstoneWire;
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;

macro_rules! graph_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) const fn new(index: u32) -> Self {
                Self(index)
            }

            pub(crate) fn try_from_index(index: usize) -> Option<Self> {
                u32::try_from(index).ok().map(Self)
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

graph_id!(NodeId);
graph_id!(SiteId);
graph_id!(PistonId);
graph_id!(ResourceId);
graph_id!(ContactId);
graph_id!(ProfileId);
graph_id!(GuardId);
graph_id!(RouteId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InputPort {
    Main,
    Side,
    Quasi,
    StrongIncident,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PowerPort {
    Output,
    Stored,
    Weak(BlockFace),
    Strong(BlockFace),
    Conducted { include_dust: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LinkKind {
    Power {
        from: PowerPort,
        to: InputPort,
        attenuation: u8,
        guard: GuardId,
    },
    Update {
        direction: Option<BlockFace>,
        guard: GuardId,
    },
    Shape {
        direction: BlockFace,
        guard: GuardId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinkSpec {
    pub(crate) from: NodeId,
    pub(crate) to: NodeId,
    pub(crate) kind: LinkKind,
    pub(crate) callback: Option<CallbackEdgeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CallbackEdgeRef {
    pub(crate) route: RouteId,
    pub(crate) sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CircuitNodeKind {
    Electrical(NodeType),
    Observer { watched: SiteId, facing: BlockFace },
    Wire(usize),
    Piston(PistonId),
    Contact(ContactId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComponentState {
    Electrical(NodeState),
    Wire(RedstoneWire),
    Specialized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitNodeDraft {
    pub(crate) position: Option<BlockPos>,
    pub(crate) kind: CircuitNodeKind,
    pub(crate) state: ComponentState,
    pub(crate) output_route: Option<RouteId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeSpec {
    pub(crate) position: Option<BlockPos>,
    pub(crate) kind: CircuitNodeKind,
    pub(crate) incoming: Range<usize>,
    pub(crate) output_route: Option<RouteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileElectrical {
    None,
    RedstoneSource,
    Component,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Profile {
    pub(crate) raw_state: u32,
    pub(crate) registry_type: u32,
    pub(crate) solid: bool,
    pub(crate) transparent: bool,
    pub(crate) cube: bool,
    pub(crate) electrical: ProfileElectrical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandView {
    Live,
    Captured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateTest {
    PistonExtended(PistonId),
    PistonAvailability {
        piston: PistonId,
        is: PistonAvailability,
    },
    ResourceAt {
        resource: ResourceId,
        contact: ContactId,
    },
    ResourceEnabled(ResourceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SiteTest {
    Solid,
    Transparent,
    Cube,
    IsWire,
    Profile(ProfileId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Guard {
    Always,
    State(StateTest),
    Site {
        site: SiteId,
        view: OperandView,
        test: SiteTest,
    },
    Not(GuardId),
    All(Box<[GuardId]>),
    Any(Box<[GuardId]>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallbackReceiver {
    Component(NodeId),
    PistonBase(PistonId),
    PistonHead(PistonId),
    Contact(ContactId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContactVariant {
    pub(crate) when: GuardId,
    pub(crate) profile: ProfileId,
    pub(crate) receiver: Option<CallbackReceiver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContactSpec {
    pub(crate) node: NodeId,
    pub(crate) position: BlockPos,
    pub(crate) variants: Box<[ContactVariant]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceSpec {
    pub(crate) material: ProfileId,
    pub(crate) contacts: Box<[ContactId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceState {
    pub(crate) at: ContactId,
    pub(crate) enabled: bool,
    pub(crate) revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PistonLane {
    Empty,
    SingleResource { far: ContactId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PistonRoutes {
    pub(crate) base_changed: RouteId,
    pub(crate) near_changed: RouteId,
    pub(crate) far_changed: Option<RouteId>,
    pub(crate) base_restored: RouteId,
    pub(crate) head_restored: RouteId,
    pub(crate) resource_restored_near: Option<RouteId>,
    pub(crate) resource_restored_far: Option<RouteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PistonSpec {
    pub(crate) node: NodeId,
    pub(crate) facing: BlockFace,
    pub(crate) sticky: bool,
    pub(crate) base: ContactId,
    pub(crate) near: ContactId,
    pub(crate) lane: PistonLane,
    pub(crate) routes: PistonRoutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PistonAvailability {
    Ready,
    HeadPending,
    PublishingHeadRemoval,
    BasePending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PistonState {
    pub(crate) extended: bool,
    pub(crate) availability: PistonAvailability,
    pub(crate) revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SiteSource {
    Component(NodeId),
    Contact(ContactId),
    Constant(ProfileId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SiteSpec {
    pub(crate) position: BlockPos,
    pub(crate) source: SiteSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WirePlan {
    pub(crate) node: NodeId,
    pub(crate) canonical_neighbors: [SiteId; 24],
    pub(crate) shape_route: Option<RouteId>,
    /// Canonical captured candidates are ordered by TURBO_ORDER at delivery.
    pub(crate) turbo_route: Option<RouteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DeliveryKind {
    Update { direction: Option<BlockFace> },
    Shape { direction: BlockFace },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouteDelivery {
    pub(crate) target: SiteId,
    pub(crate) kind: DeliveryKind,
    pub(crate) guard: GuardId,
    pub(crate) view: OperandView,
    pub(crate) sequence: u32,
    pub(crate) receiver: Option<CallbackReceiver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Route {
    pub(crate) source: NodeId,
    pub(crate) deliveries: Box<[RouteDelivery]>,
}

impl RouteDelivery {
    fn link_kind(self) -> LinkKind {
        match self.kind {
            DeliveryKind::Update { direction } => LinkKind::Update {
                direction,
                guard: self.guard,
            },
            DeliveryKind::Shape { direction } => LinkKind::Shape {
                direction,
                guard: self.guard,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitDraft {
    pub(crate) nodes: Vec<CircuitNodeDraft>,
    pub(crate) links: Vec<LinkSpec>,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) guards: Vec<Guard>,
    pub(crate) contacts: Vec<ContactSpec>,
    pub(crate) routes: Vec<Route>,
    pub(crate) wires: Vec<WirePlan>,
    pub(crate) sites: Vec<SiteSpec>,
    pub(crate) pistons: Vec<PistonSpec>,
    pub(crate) resources: Vec<ResourceSpec>,
    pub(crate) initial_pistons: Vec<PistonState>,
    pub(crate) initial_resources: Vec<ResourceState>,
}

impl CircuitDraft {
    pub(crate) fn lower(mut self) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        self.validate()?;

        // Links are grouped by destination for dense incoming ranges. Their order
        // is not callback order; ordered deliveries live only in Route tables.
        self.links.sort_by_key(|link| link.to.index());
        let mut nodes = Vec::with_capacity(self.nodes.len());
        let mut start = 0;
        for (index, node) in self.nodes.iter().enumerate() {
            let id = NodeId::try_from_index(index)
                .ok_or(GraphBuildError::TooManyEntries { table: "nodes" })?;
            let incoming_start = start;
            while start < self.links.len() && self.links[start].to == id {
                start += 1;
            }
            if start > self.links.len() || incoming_start > start {
                return Err(GraphBuildError::InvalidIncomingRange {
                    node: Some(id),
                    start: incoming_start,
                    end: start,
                    links: self.links.len(),
                });
            }
            nodes.push(NodeSpec {
                position: node.position,
                kind: node.kind.clone(),
                incoming: incoming_start..start,
                output_route: node.output_route,
            });
        }
        if start != self.links.len() {
            return Err(GraphBuildError::InvalidIncomingRange {
                node: self.links.get(start).map(|link| link.to),
                start,
                end: self.links.len(),
                links: self.links.len(),
            });
        }

        let state = CircuitState {
            components: self
                .nodes
                .into_iter()
                .map(|node| node.state)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            pistons: self.initial_pistons.into_boxed_slice(),
            resources: self.initial_resources.into_boxed_slice(),
        };
        let program = CircuitProgram {
            nodes: nodes.into_boxed_slice(),
            links: self.links.into_boxed_slice(),
            profiles: self.profiles.into_boxed_slice(),
            guards: self.guards.into_boxed_slice(),
            contacts: self.contacts.into_boxed_slice(),
            routes: self.routes.into_boxed_slice(),
            wires: self.wires.into_boxed_slice(),
            sites: self.sites.into_boxed_slice(),
            pistons: self.pistons.into_boxed_slice(),
            resources: self.resources.into_boxed_slice(),
        };
        Ok((program, state))
    }

    fn validate(&self) -> Result<(), GraphBuildError> {
        for (name, len) in [
            ("nodes", self.nodes.len()),
            ("sites", self.sites.len()),
            ("pistons", self.pistons.len()),
            ("resources", self.resources.len()),
            ("contacts", self.contacts.len()),
            ("profiles", self.profiles.len()),
            ("guards", self.guards.len()),
            ("routes", self.routes.len()),
            ("wires", self.wires.len()),
        ] {
            if u32::try_from(len).is_err() {
                return Err(GraphBuildError::TooManyEntries { table: name });
            }
        }

        self.validate_nodes()?;
        self.validate_guards()?;
        self.validate_sites_and_wires()?;
        self.validate_contacts_and_pistons()?;
        self.validate_routes()?;
        self.validate_links()?;
        Ok(())
    }

    fn validate_nodes(&self) -> Result<(), GraphBuildError> {
        for (index, node) in self.nodes.iter().enumerate() {
            let id = NodeId::try_from_index(index).expect("node count was checked");
            let valid_state = match (&node.kind, &node.state) {
                (CircuitNodeKind::Electrical(ty), ComponentState::Electrical(state)) => {
                    if matches!(ty, NodeType::Wire | NodeType::Observer { .. }) {
                        return Err(GraphBuildError::UseSpecializedNode { node: id });
                    }
                    validate_strength(
                        id,
                        state.output_strength,
                        match ty {
                            NodeType::Comparator { far_input, .. } => *far_input,
                            _ => None,
                        },
                    )?;
                    true
                }
                (CircuitNodeKind::Observer { watched, .. }, ComponentState::Electrical(state)) => {
                    check_id("sites", watched.index(), self.sites.len())?;
                    validate_strength(id, state.output_strength, None)?;
                    true
                }
                (CircuitNodeKind::Wire(plan), ComponentState::Wire(state)) => {
                    if state.power > 15 {
                        return Err(GraphBuildError::InvalidStrength {
                            node: id,
                            strength: state.power,
                        });
                    }
                    let wire = self
                        .wires
                        .get(*plan)
                        .ok_or(GraphBuildError::InvalidReference {
                            table: "wires",
                            index: *plan,
                            len: self.wires.len(),
                        })?;
                    if wire.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Piston(piston), ComponentState::Specialized) => {
                    let spec = self.pistons.get(piston.index()).ok_or(
                        GraphBuildError::InvalidReference {
                            table: "pistons",
                            index: piston.index(),
                            len: self.pistons.len(),
                        },
                    )?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Contact(contact), ComponentState::Specialized) => {
                    let spec = self.contacts.get(contact.index()).ok_or(
                        GraphBuildError::InvalidReference {
                            table: "contacts",
                            index: contact.index(),
                            len: self.contacts.len(),
                        },
                    )?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                _ => false,
            };
            if !valid_state {
                return Err(GraphBuildError::InvalidNodeState { node: id });
            }
            if let Some(route) = node.output_route {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != id {
                    return Err(GraphBuildError::RouteSourceMismatch { route, node: id });
                }
            }
        }
        Ok(())
    }

    fn validate_guards(&self) -> Result<(), GraphBuildError> {
        for guard in &self.guards {
            match guard {
                Guard::Always => {}
                Guard::State(test) => self.validate_state_test(*test)?,
                Guard::Site { site, test, .. } => {
                    check_id("sites", site.index(), self.sites.len())?;
                    if let SiteTest::Profile(profile) = test {
                        check_id("profiles", profile.index(), self.profiles.len())?;
                    }
                }
                Guard::Not(other) => check_id("guards", other.index(), self.guards.len())?,
                Guard::All(others) | Guard::Any(others) => {
                    for other in others.iter() {
                        check_id("guards", other.index(), self.guards.len())?;
                    }
                }
            }
        }

        let mut marks = vec![0u8; self.guards.len()];
        for root_index in 0..self.guards.len() {
            if marks[root_index] != 0 {
                continue;
            }
            let root = GuardId::try_from_index(root_index).expect("guard count was checked");
            let mut stack = vec![(root, false)];
            while let Some((id, leaving)) = stack.pop() {
                let guard_count = marks.len();
                let mark = marks
                    .get_mut(id.index())
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "guards",
                        index: id.index(),
                        len: guard_count,
                    })?;
                if leaving {
                    *mark = 2;
                    continue;
                }
                if *mark == 1 {
                    return Err(GraphBuildError::CyclicGuard { guard: id });
                }
                if *mark == 2 {
                    continue;
                }
                *mark = 1;
                stack.push((id, true));
                if let Some(guard) = self.guards.get(id.index()) {
                    for child in guard_children(guard).iter().rev() {
                        if marks.get(child.index()) == Some(&1) {
                            return Err(GraphBuildError::CyclicGuard { guard: *child });
                        }
                        if marks.get(child.index()) == Some(&0) {
                            stack.push((*child, false));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_state_test(&self, test: StateTest) -> Result<(), GraphBuildError> {
        match test {
            StateTest::PistonExtended(piston) | StateTest::PistonAvailability { piston, .. } => {
                check_id("pistons", piston.index(), self.pistons.len())?;
            }
            StateTest::ResourceAt { resource, contact } => {
                check_id("contacts", contact.index(), self.contacts.len())?;
                let spec = self.resources.get(resource.index()).ok_or(
                    GraphBuildError::InvalidReference {
                        table: "resources",
                        index: resource.index(),
                        len: self.resources.len(),
                    },
                )?;
                if !spec.contacts.contains(&contact) {
                    return Err(GraphBuildError::InvalidResourceSelection { resource, contact });
                }
            }
            StateTest::ResourceEnabled(resource) => {
                check_id("resources", resource.index(), self.resources.len())?;
            }
        }
        Ok(())
    }

    fn validate_sites_and_wires(&self) -> Result<(), GraphBuildError> {
        for site in &self.sites {
            match site.source {
                SiteSource::Component(node) => self.validate_component_binding(node)?,
                SiteSource::Contact(contact) => {
                    check_id("contacts", contact.index(), self.contacts.len())?;
                }
                SiteSource::Constant(profile) => {
                    check_id("profiles", profile.index(), self.profiles.len())?;
                }
            }
        }

        for (index, wire) in self.wires.iter().enumerate() {
            let node =
                self.nodes
                    .get(wire.node.index())
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "nodes",
                        index: wire.node.index(),
                        len: self.nodes.len(),
                    })?;
            if node.kind != CircuitNodeKind::Wire(index) {
                return Err(GraphBuildError::NodeKindMismatch { node: wire.node });
            }
            for site in wire.canonical_neighbors {
                check_id("sites", site.index(), self.sites.len())?;
            }
            for route in wire.shape_route.into_iter().chain(wire.turbo_route) {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != wire.node {
                    return Err(GraphBuildError::RouteSourceMismatch {
                        route,
                        node: wire.node,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_contacts_and_pistons(&self) -> Result<(), GraphBuildError> {
        for (index, contact) in self.contacts.iter().enumerate() {
            let id = ContactId::try_from_index(index).expect("contact count was checked");
            let node =
                self.nodes
                    .get(contact.node.index())
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "nodes",
                        index: contact.node.index(),
                        len: self.nodes.len(),
                    })?;
            if node.kind != CircuitNodeKind::Contact(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: contact.node });
            }
            if contact.variants.is_empty() {
                return Err(GraphBuildError::EmptyContact { contact: id });
            }
            for variant in contact.variants.iter() {
                check_id("guards", variant.when.index(), self.guards.len())?;
                self.validate_contact_guard(id, variant.when)?;
                check_id("profiles", variant.profile.index(), self.profiles.len())?;
                if let Some(receiver) = variant.receiver {
                    self.validate_receiver(receiver)?;
                }
            }
        }

        for (index, piston) in self.pistons.iter().enumerate() {
            let id = PistonId::try_from_index(index).expect("piston count was checked");
            let node =
                self.nodes
                    .get(piston.node.index())
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "nodes",
                        index: piston.node.index(),
                        len: self.nodes.len(),
                    })?;
            if node.kind != CircuitNodeKind::Piston(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: piston.node });
            }
            check_id("contacts", piston.base.index(), self.contacts.len())?;
            check_id("contacts", piston.near.index(), self.contacts.len())?;
            if let PistonLane::SingleResource { far } = piston.lane {
                check_id("contacts", far.index(), self.contacts.len())?;
            }
            self.validate_piston_routes(piston.node, piston.lane, piston.routes)?;
        }

        if self.initial_pistons.len() != self.pistons.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "pistons",
                descriptions: self.pistons.len(),
                states: self.initial_pistons.len(),
            });
        }
        if self.initial_resources.len() != self.resources.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "resources",
                descriptions: self.resources.len(),
                states: self.initial_resources.len(),
            });
        }

        for (index, resource) in self.resources.iter().enumerate() {
            let id = ResourceId::try_from_index(index).expect("resource count was checked");
            check_id("profiles", resource.material.index(), self.profiles.len())?;
            if resource.contacts.is_empty() {
                return Err(GraphBuildError::EmptyResource { resource: id });
            }
            for contact in resource.contacts.iter() {
                check_id("contacts", contact.index(), self.contacts.len())?;
            }
            let state = self
                .initial_resources
                .get(index)
                .copied()
                .expect("resource state count was validated");
            if !resource.contacts.contains(&state.at) {
                return Err(GraphBuildError::InvalidResourceSelection {
                    resource: id,
                    contact: state.at,
                });
            }
        }

        let mut selected_contacts = HashMap::with_capacity(self.initial_resources.len());
        for (index, state) in self.initial_resources.iter().enumerate() {
            let resource = ResourceId::try_from_index(index).expect("resource count was checked");
            if let Some(first) = selected_contacts.insert(state.at, resource) {
                return Err(GraphBuildError::ResourcesShareContact {
                    first,
                    second: resource,
                    contact: state.at,
                });
            }
        }
        Ok(())
    }

    fn validate_contact_guard(
        &self,
        contact: ContactId,
        root: GuardId,
    ) -> Result<(), GraphBuildError> {
        let mut visited = vec![false; self.guards.len()];
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if visited[id.index()] {
                continue;
            }
            visited[id.index()] = true;
            let guard = &self.guards[id.index()];
            if let Guard::Site { site, .. } = guard {
                return Err(GraphBuildError::ContactGuardReadsSite {
                    contact,
                    guard: id,
                    site: *site,
                });
            }
            pending.extend(guard_children(guard).iter().copied());
        }
        Ok(())
    }

    fn validate_receiver(&self, receiver: CallbackReceiver) -> Result<(), GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => self.validate_component_binding(node),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => {
                check_id("pistons", piston.index(), self.pistons.len())
            }
            CallbackReceiver::Contact(contact) => {
                check_id("contacts", contact.index(), self.contacts.len())
            }
        }
    }

    fn validate_component_binding(&self, node: NodeId) -> Result<(), GraphBuildError> {
        let kind = &self
            .nodes
            .get(node.index())
            .ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: node.index(),
                len: self.nodes.len(),
            })?
            .kind;
        if matches!(
            kind,
            CircuitNodeKind::Electrical(_)
                | CircuitNodeKind::Observer { .. }
                | CircuitNodeKind::Wire(_)
        ) {
            Ok(())
        } else {
            Err(GraphBuildError::InvalidComponentBinding { node })
        }
    }

    fn validate_piston_routes(
        &self,
        source: NodeId,
        lane: PistonLane,
        routes: PistonRoutes,
    ) -> Result<(), GraphBuildError> {
        let has_resource_routes =
            routes.resource_restored_near.is_some() && routes.resource_restored_far.is_some();
        if matches!(lane, PistonLane::SingleResource { .. }) != has_resource_routes
            || routes.resource_restored_near.is_some() != routes.resource_restored_far.is_some()
        {
            return Err(GraphBuildError::PistonResourceRestorationMismatch { node: source });
        }
        for route in [
            Some(routes.base_changed),
            Some(routes.near_changed),
            routes.far_changed,
            Some(routes.base_restored),
            Some(routes.head_restored),
            routes.resource_restored_near,
            routes.resource_restored_far,
        ]
        .into_iter()
        .flatten()
        {
            check_id("routes", route.index(), self.routes.len())?;
            if self.routes[route.index()].source != source {
                return Err(GraphBuildError::RouteSourceMismatch {
                    route,
                    node: source,
                });
            }
        }
        Ok(())
    }

    fn validate_routes(&self) -> Result<(), GraphBuildError> {
        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            check_id("nodes", route.source.index(), self.nodes.len())?;
            let mut previous_sequence = 0;
            for (delivery_index, delivery) in route.deliveries.iter().enumerate() {
                check_id("sites", delivery.target.index(), self.sites.len())?;
                check_id("guards", delivery.guard.index(), self.guards.len())?;
                if delivery_index > 0 && delivery.sequence < previous_sequence {
                    return Err(GraphBuildError::UnorderedRoute { route: route_id });
                }
                previous_sequence = delivery.sequence;
                if let Some(receiver) = delivery.receiver {
                    self.validate_receiver(receiver)?;
                    self.validate_delivery_receiver(route_id, delivery, receiver)?;
                }
            }
        }
        Ok(())
    }

    fn receiver_node(&self, receiver: CallbackReceiver) -> Result<NodeId, GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => Ok(node),
            CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id) => self
                .pistons
                .get(id.index())
                .map(|piston| piston.node)
                .ok_or(GraphBuildError::InvalidReference {
                    table: "pistons",
                    index: id.index(),
                    len: self.pistons.len(),
                }),
            CallbackReceiver::Contact(id) => self
                .contacts
                .get(id.index())
                .map(|contact| contact.node)
                .ok_or(GraphBuildError::InvalidReference {
                    table: "contacts",
                    index: id.index(),
                    len: self.contacts.len(),
                }),
        }
    }

    fn validate_delivery_receiver(
        &self,
        route: RouteId,
        delivery: &RouteDelivery,
        receiver: CallbackReceiver,
    ) -> Result<(), GraphBuildError> {
        let site = self.sites[delivery.target.index()];
        let valid = match (delivery.kind, site.source, receiver) {
            (
                DeliveryKind::Shape { .. },
                SiteSource::Contact(contact),
                CallbackReceiver::Contact(target),
            ) => contact == target && self.contact_guard_matches(delivery, contact, None),
            (
                DeliveryKind::Shape { .. },
                SiteSource::Component(node),
                CallbackReceiver::Component(target),
            ) => node == target,
            (
                DeliveryKind::Update { .. },
                SiteSource::Component(node),
                CallbackReceiver::Component(target),
            ) => node == target,
            (DeliveryKind::Update { .. }, SiteSource::Contact(contact), receiver) => self.contacts
                [contact.index()]
            .variants
            .iter()
            .any(|variant| {
                variant.receiver == Some(receiver)
                    && self.contact_guard_matches(delivery, contact, Some(variant.profile))
            }),
            (_, SiteSource::Constant(_), _) => false,
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(GraphBuildError::CallbackEdgeMismatch {
                route,
                sequence: delivery.sequence,
            })
        }
    }

    fn contact_guard_matches(
        &self,
        delivery: &RouteDelivery,
        contact: ContactId,
        profile: Option<ProfileId>,
    ) -> bool {
        match delivery.view {
            OperandView::Live => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| {
                    matches!(self.guards[variant.when.index()], Guard::Always)
                        || self.guard_implies(delivery.guard, variant.when)
                }),
            OperandView::Captured => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| {
                    self.guard_implies_site_profile(
                        delivery.guard,
                        delivery.target,
                        variant.profile,
                    )
                }),
        }
    }

    fn guard_implies(&self, root: GuardId, expected: GuardId) -> bool {
        if root == expected {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children
                .iter()
                .any(|child| self.guard_implies(*child, expected)),
            Guard::Any(children) => {
                !children.is_empty()
                    && children
                        .iter()
                        .all(|child| self.guard_implies(*child, expected))
            }
            _ => false,
        }
    }

    fn guard_implies_site_profile(&self, root: GuardId, site: SiteId, profile: ProfileId) -> bool {
        if matches!(
            self.guards[root.index()],
                Guard::Site {
                    site: candidate_site,
                    view: OperandView::Captured,
                    test: SiteTest::Profile(candidate_profile),
                } if candidate_site == site && candidate_profile == profile
        ) {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children
                .iter()
                .any(|child| self.guard_implies_site_profile(*child, site, profile)),
            Guard::Any(children) => {
                !children.is_empty()
                    && children
                        .iter()
                        .all(|child| self.guard_implies_site_profile(*child, site, profile))
            }
            _ => false,
        }
    }

    fn validate_links(&self) -> Result<(), GraphBuildError> {
        let mut callback_edges =
            HashMap::<(CallbackEdgeRef, NodeId, NodeId, LinkKind), usize>::new();
        for link in &self.links {
            check_id("nodes", link.from.index(), self.nodes.len())?;
            check_id("nodes", link.to.index(), self.nodes.len())?;
            let guard = match link.kind {
                LinkKind::Power {
                    attenuation, guard, ..
                } => {
                    if attenuation > 15 {
                        return Err(GraphBuildError::InvalidAttenuation {
                            from: link.from,
                            to: link.to,
                            attenuation,
                        });
                    }
                    guard
                }
                LinkKind::Update { guard, .. } | LinkKind::Shape { guard, .. } => guard,
            };
            check_id("guards", guard.index(), self.guards.len())?;
            match (link.kind, link.callback) {
                (LinkKind::Power { .. }, None) => {}
                (_, Some(reference)) => {
                    let route = self.routes.get(reference.route.index()).ok_or(
                        GraphBuildError::InvalidReference {
                            table: "routes",
                            index: reference.route.index(),
                            len: self.routes.len(),
                        },
                    )?;
                    if route.source != link.from || matches!(link.kind, LinkKind::Power { .. }) {
                        return Err(GraphBuildError::CallbackEdgeMismatch {
                            route: reference.route,
                            sequence: reference.sequence,
                        });
                    }
                    *callback_edges
                        .entry((reference, link.from, link.to, link.kind))
                        .or_default() += 1;
                }
                (_, None) => {
                    return Err(GraphBuildError::CallbackEdgeMissingReference {
                        from: link.from,
                        to: link.to,
                    });
                }
            }
        }

        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            for delivery in route.deliveries.iter() {
                let Some(receiver) = delivery.receiver else {
                    continue;
                };
                let to = self.receiver_node(receiver)?;
                let key = (
                    CallbackEdgeRef {
                        route: route_id,
                        sequence: delivery.sequence,
                    },
                    route.source,
                    to,
                    delivery.link_kind(),
                );
                let Some(count) = callback_edges.get_mut(&key) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                let Some(remaining) = count.checked_sub(1) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                *count = remaining;
            }
        }
        if let Some(((reference, _, _, _), _)) =
            callback_edges.iter().find(|(_, count)| **count != 0)
        {
            return Err(GraphBuildError::CallbackEdgeMismatch {
                route: reference.route,
                sequence: reference.sequence,
            });
        }
        Ok(())
    }
}

fn guard_children(guard: &Guard) -> &[GuardId] {
    match guard {
        Guard::Not(id) => std::slice::from_ref(id),
        Guard::All(ids) | Guard::Any(ids) => ids,
        _ => &[],
    }
}

fn validate_strength(
    node: NodeId,
    output: u8,
    far_input: Option<u8>,
) -> Result<(), GraphBuildError> {
    for strength in std::iter::once(output).chain(far_input) {
        if strength > 15 {
            return Err(GraphBuildError::InvalidStrength { node, strength });
        }
    }
    Ok(())
}

fn check_id(table: &'static str, index: usize, len: usize) -> Result<(), GraphBuildError> {
    if index >= len {
        Err(GraphBuildError::InvalidReference { table, index, len })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum GraphBuildError {
    TooManyEntries {
        table: &'static str,
    },
    InvalidReference {
        table: &'static str,
        index: usize,
        len: usize,
    },
    InvalidStrength {
        node: NodeId,
        strength: u8,
    },
    InvalidAttenuation {
        from: NodeId,
        to: NodeId,
        attenuation: u8,
    },
    InvalidNodeState {
        node: NodeId,
    },
    InvalidComponentBinding {
        node: NodeId,
    },
    UseSpecializedNode {
        node: NodeId,
    },
    NodeKindMismatch {
        node: NodeId,
    },
    CyclicGuard {
        guard: GuardId,
    },
    UnorderedRoute {
        route: RouteId,
    },
    RouteSourceMismatch {
        route: RouteId,
        node: NodeId,
    },
    CallbackEdgeMismatch {
        route: RouteId,
        sequence: u32,
    },
    CallbackEdgeMissingReference {
        from: NodeId,
        to: NodeId,
    },
    PistonResourceRestorationMismatch {
        node: NodeId,
    },
    ContactGuardReadsSite {
        contact: ContactId,
        guard: GuardId,
        site: SiteId,
    },
    EmptyContact {
        contact: ContactId,
    },
    EmptyResource {
        resource: ResourceId,
    },
    InvalidResourceSelection {
        resource: ResourceId,
        contact: ContactId,
    },
    ResourcesShareContact {
        first: ResourceId,
        second: ResourceId,
        contact: ContactId,
    },
    StateTableLength {
        table: &'static str,
        descriptions: usize,
        states: usize,
    },
    InvalidIncomingRange {
        node: Option<NodeId>,
        start: usize,
        end: usize,
        links: usize,
    },
}

impl std::fmt::Display for GraphBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyEntries { table } => write!(f, "{table} table exceeds u32 graph IDs"),
            Self::InvalidReference { table, index, len } => {
                write!(f, "{table} reference {index} is out of range 0..{len}")
            }
            Self::InvalidStrength { node, strength } => {
                write!(
                    f,
                    "node {} has strength {strength}; expected 0..15",
                    node.index()
                )
            }
            Self::InvalidAttenuation {
                from,
                to,
                attenuation,
            } => write!(
                f,
                "power link {} -> {} has attenuation {attenuation}; expected 0..15",
                from.index(),
                to.index()
            ),
            Self::InvalidNodeState { node } => {
                write!(
                    f,
                    "node {} has state that does not match its kind",
                    node.index()
                )
            }
            Self::InvalidComponentBinding { node } => write!(
                f,
                "generic component binding references specialized node {}",
                node.index()
            ),
            Self::UseSpecializedNode { node } => write!(
                f,
                "node {} must use its typed wire or observer form",
                node.index()
            ),
            Self::NodeKindMismatch { node } => write!(
                f,
                "description for node {} does not match its graph kind",
                node.index()
            ),
            Self::CyclicGuard { guard } => {
                write!(f, "guard {} participates in a cycle", guard.index())
            }
            Self::UnorderedRoute { route } => write!(
                f,
                "route {} has deliveries out of sequence order",
                route.index()
            ),
            Self::RouteSourceMismatch { route, node } => write!(
                f,
                "route {} is assigned to the wrong source node {}",
                route.index(),
                node.index()
            ),
            Self::CallbackEdgeMismatch { route, sequence } => write!(
                f,
                "route {} delivery {sequence} does not match its callback edge",
                route.index()
            ),
            Self::CallbackEdgeMissingReference { from, to } => write!(
                f,
                "callback edge {} -> {} is missing its route reference",
                from.index(),
                to.index()
            ),
            Self::PistonResourceRestorationMismatch { node } => write!(
                f,
                "piston node {} has restoration routes that do not match its resource lane",
                node.index()
            ),
            Self::ContactGuardReadsSite {
                contact,
                guard,
                site,
            } => write!(
                f,
                "contact {} selector guard {} reads site {}",
                contact.index(),
                guard.index(),
                site.index()
            ),
            Self::EmptyContact { contact } => {
                write!(f, "contact {} has no selectable variants", contact.index())
            }
            Self::EmptyResource { resource } => {
                write!(f, "resource {} has no possible contacts", resource.index())
            }
            Self::InvalidResourceSelection { resource, contact } => write!(
                f,
                "resource {} selects contact {} outside its description",
                resource.index(),
                contact.index()
            ),
            Self::ResourcesShareContact {
                first,
                second,
                contact,
            } => write!(
                f,
                "resources {} and {} both select contact {} initially",
                first.index(),
                second.index(),
                contact.index()
            ),
            Self::StateTableLength {
                table,
                descriptions,
                states,
            } => write!(
                f,
                "{table} table has {descriptions} descriptions but {states} initial states"
            ),
            Self::InvalidIncomingRange {
                node,
                start,
                end,
                links,
            } => write!(
                f,
                "node {node:?} has incoming range {start}..{end} for {links} links",
            ),
        }
    }
}

impl std::error::Error for GraphBuildError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitProgram {
    nodes: Box<[NodeSpec]>,
    links: Box<[LinkSpec]>,
    profiles: Box<[Profile]>,
    guards: Box<[Guard]>,
    contacts: Box<[ContactSpec]>,
    routes: Box<[Route]>,
    wires: Box<[WirePlan]>,
    sites: Box<[SiteSpec]>,
    pistons: Box<[PistonSpec]>,
    resources: Box<[ResourceSpec]>,
}

impl CircuitProgram {
    pub(crate) fn nodes(&self) -> &[NodeSpec] {
        &self.nodes
    }

    pub(crate) fn links(&self) -> &[LinkSpec] {
        &self.links
    }

    pub(crate) fn sites(&self) -> &[SiteSpec] {
        &self.sites
    }

    pub(crate) fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub(crate) fn guards(&self) -> &[Guard] {
        &self.guards
    }

    pub(crate) fn contacts(&self) -> &[ContactSpec] {
        &self.contacts
    }

    pub(crate) fn routes(&self) -> &[Route] {
        &self.routes
    }

    pub(crate) fn wires(&self) -> &[WirePlan] {
        &self.wires
    }

    pub(crate) fn pistons(&self) -> &[PistonSpec] {
        &self.pistons
    }

    pub(crate) fn resources(&self) -> &[ResourceSpec] {
        &self.resources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitState {
    pub(crate) components: Box<[ComponentState]>,
    pub(crate) pistons: Box<[PistonState]>,
    pub(crate) resources: Box<[ResourceState]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservedProperties {
    Passive,
    Electrical { powered: bool, locked: bool },
    Wire(RedstoneWire),
    Piston { extended: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedSite {
    pub(crate) profile: ProfileId,
    pub(crate) receiver: Option<CallbackReceiver>,
    pub(crate) properties: ObservedProperties,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkSite {
    pub(crate) observed: ObservedSite,
    pub(crate) visited: bool,
    pub(crate) x_bias: i32,
    pub(crate) z_bias: i32,
    pub(crate) layer: u32,
}

/// Per-invocation dust scratch. It is constructed separately from persistent
/// circuit state, so nested walks cannot overwrite a caller's captured sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireWalk {
    pub(crate) sites: Vec<Option<WalkSite>>,
    pub(crate) queues: [VecDeque<SiteId>; 3],
    pub(crate) layer: u32,
}

impl WireWalk {
    pub(crate) fn new(site_count: usize) -> Self {
        Self {
            sites: vec![None; site_count],
            queues: std::array::from_fn(|_| VecDeque::new()),
            layer: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_with_mixed_links() -> CircuitDraft {
        let guard = GuardId::new(0);
        let site = SiteId::new(0);
        let contact = ContactId::new(0);
        let second_contact = ContactId::new(1);
        let piston = PistonId::new(0);
        let route = RouteId::new(0);
        let profile = ProfileId::new(0);
        let piston_node = NodeId::new(2);
        let contact_node = NodeId::new(3);
        let draft = CircuitDraft {
            nodes: vec![
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 0)),
                    kind: CircuitNodeKind::Electrical(NodeType::Lever),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(1, 0, 0)),
                    kind: CircuitNodeKind::Electrical(NodeType::Lamp),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(2, 0, 0)),
                    kind: CircuitNodeKind::Piston(piston),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(3, 0, 0)),
                    kind: CircuitNodeKind::Contact(contact),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(4, 0, 0)),
                    kind: CircuitNodeKind::Contact(second_contact),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 1)),
                    kind: CircuitNodeKind::Wire(0),
                    state: ComponentState::Wire(RedstoneWire {
                        power: 4,
                        ..Default::default()
                    }),
                    output_route: None,
                },
            ],
            links: vec![
                LinkSpec {
                    from: NodeId::new(0),
                    to: NodeId::new(1),
                    kind: LinkKind::Power {
                        from: PowerPort::Output,
                        to: InputPort::Main,
                        attenuation: 15,
                        guard,
                    },
                    callback: None,
                },
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Update {
                        direction: Some(BlockFace::East),
                        guard,
                    },
                    callback: Some(CallbackEdgeRef { route, sequence: 0 }),
                },
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Update {
                        direction: Some(BlockFace::East),
                        guard,
                    },
                    callback: Some(CallbackEdgeRef { route, sequence: 1 }),
                },
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Shape {
                        direction: BlockFace::West,
                        guard,
                    },
                    callback: Some(CallbackEdgeRef { route, sequence: 2 }),
                },
            ],
            profiles: vec![Profile {
                raw_state: 0,
                registry_type: 0,
                solid: false,
                transparent: true,
                cube: false,
                electrical: ProfileElectrical::None,
            }],
            guards: vec![Guard::Always],
            contacts: vec![
                ContactSpec {
                    node: contact_node,
                    position: BlockPos::new(3, 0, 0),
                    variants: vec![ContactVariant {
                        when: guard,
                        profile,
                        receiver: None,
                    }]
                    .into_boxed_slice(),
                },
                ContactSpec {
                    node: NodeId::new(4),
                    position: BlockPos::new(4, 0, 0),
                    variants: vec![ContactVariant {
                        when: guard,
                        profile,
                        receiver: None,
                    }]
                    .into_boxed_slice(),
                },
            ],
            routes: vec![Route {
                source: piston_node,
                deliveries: vec![
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Update {
                            direction: Some(BlockFace::East),
                        },
                        guard,
                        view: OperandView::Live,
                        sequence: 0,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Update {
                            direction: Some(BlockFace::East),
                        },
                        guard,
                        view: OperandView::Live,
                        sequence: 1,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Shape {
                            direction: BlockFace::West,
                        },
                        guard,
                        view: OperandView::Live,
                        sequence: 2,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                ]
                .into_boxed_slice(),
            }],
            wires: vec![WirePlan {
                node: NodeId::new(5),
                canonical_neighbors: [site; 24],
                shape_route: None,
                turbo_route: None,
            }],
            sites: vec![SiteSpec {
                position: BlockPos::new(0, 0, 0),
                source: SiteSource::Component(NodeId::new(1)),
            }],
            pistons: vec![PistonSpec {
                node: piston_node,
                facing: BlockFace::East,
                sticky: false,
                base: contact,
                near: contact,
                lane: PistonLane::Empty,
                routes: PistonRoutes {
                    base_changed: route,
                    near_changed: route,
                    far_changed: None,
                    base_restored: route,
                    head_restored: route,
                    resource_restored_near: None,
                    resource_restored_far: None,
                },
            }],
            resources: vec![ResourceSpec {
                material: profile,
                contacts: vec![contact, second_contact].into_boxed_slice(),
            }],
            initial_pistons: vec![PistonState {
                extended: false,
                availability: PistonAvailability::Ready,
                revision: 0,
            }],
            initial_resources: vec![ResourceState {
                at: contact,
                enabled: true,
                revision: 0,
            }],
        };
        draft
    }

    #[test]
    fn lowering_groups_links_without_losing_callback_duplicates_or_direction() {
        let (program, state) = draft_with_mixed_links().lower().unwrap();
        assert_eq!(program.nodes()[1].incoming, 0..4);
        assert_eq!(program.links().len(), 4);
        assert!(matches!(
            program.links()[0].kind,
            LinkKind::Power {
                attenuation: 15,
                ..
            }
        ));
        assert_eq!(
            program
                .links()
                .iter()
                .filter(|link| matches!(link.kind, LinkKind::Update { .. }))
                .count(),
            2
        );
        assert!(matches!(
            program.links()[3].kind,
            LinkKind::Shape {
                direction: BlockFace::West,
                ..
            }
        ));
        assert_eq!(
            program.routes[0].deliveries[0].kind,
            DeliveryKind::Update {
                direction: Some(BlockFace::East)
            }
        );
        assert_eq!(
            program.routes[0].deliveries[1].kind,
            DeliveryKind::Update {
                direction: Some(BlockFace::East)
            }
        );
        assert_eq!(
            program.routes[0].deliveries[2].kind,
            DeliveryKind::Shape {
                direction: BlockFace::West
            }
        );
        assert_eq!(state.pistons.len(), 1);
        assert_eq!(state.resources.len(), 1);
        assert_eq!(state.resources[0].at, ContactId::new(0));
        assert!(matches!(&state.components[5], ComponentState::Wire(wire) if wire.power == 4));
    }

    #[test]
    fn invalid_link_reference_and_strength_fail_before_program_creation() {
        let mut invalid_reference = draft_with_mixed_links();
        invalid_reference.links[0].to = NodeId::new(99);
        assert!(matches!(
            invalid_reference.lower(),
            Err(GraphBuildError::InvalidReference { table: "nodes", .. })
        ));

        let mut invalid_strength = draft_with_mixed_links();
        invalid_strength.nodes[0].state = ComponentState::Electrical(NodeState::with_strength(16));
        assert!(matches!(
            invalid_strength.lower(),
            Err(GraphBuildError::InvalidStrength { strength: 16, .. })
        ));

        let mut invalid_attenuation = draft_with_mixed_links();
        invalid_attenuation.links[0].kind = LinkKind::Power {
            from: PowerPort::Output,
            to: InputPort::Main,
            attenuation: 16,
            guard: GuardId::new(0),
        };
        assert!(matches!(
            invalid_attenuation.lower(),
            Err(GraphBuildError::InvalidAttenuation {
                attenuation: 16,
                ..
            })
        ));

        let mut invalid_callback = draft_with_mixed_links();
        invalid_callback.links[1].callback = Some(CallbackEdgeRef {
            route: RouteId::new(0),
            sequence: 99,
        });
        assert!(matches!(
            invalid_callback.lower(),
            Err(GraphBuildError::CallbackEdgeMismatch { .. })
        ));

        let mut missing_callback = draft_with_mixed_links();
        missing_callback.links[1].callback = None;
        assert!(matches!(
            missing_callback.lower(),
            Err(GraphBuildError::CallbackEdgeMissingReference { .. })
        ));
    }

    #[test]
    fn duplicate_callback_delivery_without_a_matching_edge_is_rejected() {
        let mut duplicate = draft_with_mixed_links();
        let duplicate_delivery = duplicate.routes[0].deliveries[0];
        let mut deliveries = duplicate.routes[0].deliveries.to_vec();
        deliveries.insert(1, duplicate_delivery);
        duplicate.routes[0].deliveries = deliveries.into_boxed_slice();

        assert!(matches!(
            duplicate.lower(),
            Err(GraphBuildError::CallbackEdgeMismatch { sequence: 0, .. })
        ));
    }

    #[test]
    fn resource_lane_requires_both_destination_restoration_routes() {
        let mut missing = draft_with_mixed_links();
        missing.pistons[0].lane = PistonLane::SingleResource {
            far: ContactId::new(1),
        };
        assert!(matches!(
            missing.lower(),
            Err(GraphBuildError::PistonResourceRestorationMismatch { .. })
        ));
    }

    #[test]
    fn invalid_contact_binding_and_guard_cycle_are_rejected() {
        let mut invalid_contact = draft_with_mixed_links();
        invalid_contact.contacts[0].node = NodeId::new(1);
        assert!(matches!(
            invalid_contact.lower(),
            Err(GraphBuildError::NodeKindMismatch { .. })
        ));

        let mut invalid_state = draft_with_mixed_links();
        invalid_state.nodes[2].state = ComponentState::Electrical(NodeState::default());
        match invalid_state.lower() {
            Err(GraphBuildError::InvalidNodeState { node }) => {
                assert_eq!(node, NodeId::new(2));
            }
            result => panic!("unexpected validation result: {result:?}"),
        }

        let mut cyclic_guards = draft_with_mixed_links();
        cyclic_guards.guards = vec![Guard::Not(GuardId::new(0))];
        assert!(matches!(
            cyclic_guards.lower(),
            Err(GraphBuildError::CyclicGuard { .. })
        ));

        let mut self_dependent_contact = draft_with_mixed_links();
        let site = SiteId::new(1);
        self_dependent_contact.sites.push(SiteSpec {
            position: BlockPos::new(3, 1, 0),
            source: SiteSource::Contact(ContactId::new(0)),
        });
        self_dependent_contact.guards.push(Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Solid,
        });
        self_dependent_contact.contacts[0].variants[0].when = GuardId::new(1);
        assert!(matches!(
            self_dependent_contact.lower(),
            Err(GraphBuildError::ContactGuardReadsSite {
                contact: ContactId(0),
                site: SiteId(1),
                ..
            })
        ));

        let mut conflicting_resources = draft_with_mixed_links();
        conflicting_resources.resources.push(ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0)].into_boxed_slice(),
        });
        conflicting_resources.initial_resources.push(ResourceState {
            at: ContactId::new(0),
            enabled: true,
            revision: 0,
        });
        assert!(matches!(
            conflicting_resources.lower(),
            Err(GraphBuildError::ResourcesShareContact { .. })
        ));
    }

    #[test]
    fn walk_state_is_fresh_and_separate_from_circuit_state() {
        let outer = WireWalk::new(2);
        let nested = WireWalk::new(2);
        assert!(outer.sites.iter().all(Option::is_none));
        assert_eq!(outer, nested);
    }

    #[test]
    fn deep_audit_missing_duplicate_callback_edge_returns_error_without_panicking() {
        let mut draft = draft_with_mixed_links();
        draft.routes[0].deliveries[1].sequence = 0;
        draft.links.retain(|link| {
            link.callback
                != Some(CallbackEdgeRef {
                    route: RouteId::new(0),
                    sequence: 1,
                })
        });
        let result = std::panic::catch_unwind(|| draft.lower());
        assert!(
            result.is_ok(),
            "invalid multiplicity must return a validation error, not panic"
        );
        assert!(matches!(
            result.unwrap(),
            Err(GraphBuildError::CallbackEdgeMismatch { .. })
        ));
    }

    fn deep_audit_contact_delivery_draft(bypass_selection: bool) -> CircuitDraft {
        let mut draft = draft_with_mixed_links();
        let selector = GuardId::new(draft.guards.len() as u32);
        draft
            .guards
            .push(Guard::State(StateTest::ResourceEnabled(ResourceId::new(0))));
        draft.contacts[0].variants[0].when = selector;
        draft.contacts[0].variants[0].receiver = Some(CallbackReceiver::Component(NodeId::new(1)));
        draft.sites[0].source = SiteSource::Contact(ContactId::new(0));
        let delivery_guard = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(if bypass_selection {
            Guard::Any(vec![selector, GuardId::new(0)].into_boxed_slice())
        } else {
            Guard::Not(selector)
        });
        let mut delivery = draft.routes[0].deliveries[0];
        delivery.guard = delivery_guard;
        draft.routes[0].deliveries = vec![delivery].into_boxed_slice();
        draft.links.retain(|link| {
            link.callback.is_none()
                || link.callback
                    == Some(CallbackEdgeRef {
                        route: RouteId::new(0),
                        sequence: 0,
                    })
        });
        for link in &mut draft.links {
            if let LinkKind::Update { guard, .. } = &mut link.kind {
                *guard = delivery_guard;
            }
        }
        draft
    }

    #[test]
    fn deep_audit_negated_receiver_selection_is_rejected() {
        assert!(
            matches!(
                deep_audit_contact_delivery_draft(false).lower(),
                Err(GraphBuildError::CallbackEdgeMismatch { .. })
            ),
            "containing a selector under Not does not imply that receiver is selected"
        );
    }

    #[test]
    fn deep_audit_optional_receiver_selection_is_rejected() {
        assert!(
            matches!(
                deep_audit_contact_delivery_draft(true).lower(),
                Err(GraphBuildError::CallbackEdgeMismatch { .. })
            ),
            "Any(selector, Always) permits delivering to an unselected receiver"
        );
    }

    #[test]
    fn deep_audit_component_sites_cannot_alias_specialized_owners() {
        for target in [NodeId::new(2), NodeId::new(3)] {
            let mut draft = draft_with_mixed_links();
            draft.sites[0].source = SiteSource::Component(target);
            for delivery in draft.routes[0].deliveries.iter_mut() {
                delivery.receiver = Some(CallbackReceiver::Component(target));
            }
            for link in draft
                .links
                .iter_mut()
                .filter(|link| link.callback.is_some())
            {
                link.to = target;
            }
            assert!(
                matches!(
                    draft.lower(),
                    Err(GraphBuildError::InvalidComponentBinding { node }) if node == target
                ),
                "piston/contact owners need their specialized site and receiver bindings: {target:?}"
            );

            let mut receiver_alias = draft_with_mixed_links();
            for delivery in receiver_alias.routes[0].deliveries.iter_mut() {
                delivery.receiver = Some(CallbackReceiver::Component(target));
            }
            for link in receiver_alias
                .links
                .iter_mut()
                .filter(|link| link.callback.is_some())
            {
                link.to = target;
            }
            assert!(
                matches!(
                    receiver_alias.lower(),
                    Err(GraphBuildError::InvalidComponentBinding { node }) if node == target
                ),
                "component receivers cannot alias specialized owners: {target:?}"
            );
        }
    }
}
