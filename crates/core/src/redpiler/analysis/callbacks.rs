//! Compile ordered notification procedures over the prepared electrical sites.
#![allow(dead_code)] // Runtime delivery is introduced by the scheduler step.

use super::electrical::ElectricalDraft;
use super::geometry::GeometryDraft;
use super::{AnalysisError, PreparationBudget};
use crate::redpiler::TaskMonitor;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, DeliveryKind, Guard, GuardId, LinkKind, LinkSpec, OperandView, PistonLane, PistonRoutes,
    PistonSpec, Route, RouteDelivery, RouteId, SiteId, SiteSource, SiteSpec, SiteTest,
};
use crate::redstone;
use crate::world::World;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallbackError {
    Preparation(AnalysisError),
    UnloadedTarget {
        position: BlockPos,
    },
    UninventoriedReceiver {
        source: BlockPos,
        target: BlockPos,
        block: u32,
    },
    UnsupportedComponent {
        source: BlockPos,
        target: BlockPos,
        block: u32,
        reason: &'static str,
    },
    InvalidReceiverReference {
        receiver: CallbackReceiver,
    },
    TooManyEntries {
        table: &'static str,
    },
}

impl fmt::Display for CallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation(error) => error.fmt(f),
            Self::UnloadedTarget { position } => {
                write!(f, "callback route reaches unloaded position {position:?}")
            }
            Self::UninventoriedReceiver { source, target, block } => {
                write!(f, "callback from {source:?} reaches uncompiled receiver {block} at {target:?}")
            }
            Self::UnsupportedComponent {
                source,
                target,
                block,
                reason,
            } => write!(
                f,
                "callback from {source:?} reaches unsupported {} at {target:?}: {reason}",
                Block::from_id(*block).get_name()
            ),
            Self::InvalidReceiverReference { receiver } => {
                write!(f, "callback route has invalid receiver {receiver:?}")
            }
            Self::TooManyEntries { table } => {
                write!(f, "callback {table} exceed graph ID capacity")
            }
        }
    }
}

impl std::error::Error for CallbackError {}

impl From<AnalysisError> for CallbackError {
    fn from(error: AnalysisError) -> Self {
        Self::Preparation(error)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallbackDraft {
    pub(crate) routes: Vec<Route>,
    pub(crate) links: Vec<LinkSpec>,
    pub(crate) pistons: Vec<PistonSpec>,
}

#[derive(Debug, Clone, Copy)]
enum Filter {
    Any,
    Around { source: BlockPos, diagonal: bool, skip_pistons: bool },
    ExcludeObservers,
    ObserversOnly,
    ComparatorsOnly,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    position: BlockPos,
    kind: DeliveryKind,
    filter: Filter,
    condition: Option<GuardId>,
    solid_at: Option<BlockPos>,
    view: OperandView,
}

impl Event {
    fn update(position: BlockPos, direction: Option<BlockFace>) -> Self {
        Self {
            position,
            kind: DeliveryKind::Update { direction },
            filter: Filter::Any,
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        }
    }

    fn shape(position: BlockPos, direction: BlockFace) -> Self {
        Self {
            position,
            kind: DeliveryKind::Shape { direction },
            filter: Filter::Any,
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        }
    }
}

/// Compile routes and their callback links; no circuit state or World is changed.
pub(crate) fn prepare_callbacks<W: World>(
    world: &W,
    geometry: &GeometryDraft,
    electrical: &mut ElectricalDraft,
    monitor: &TaskMonitor,
) -> Result<CallbackDraft, CallbackError> {
    let budget = PreparationBudget::new(monitor, electrical.remaining_cells);
    budget.check()?;
    let geometry_table_bytes = monitor.preparation_stage_estimate("geometry");
    let contact_at: FxHashMap<_, _> = electrical
        .contacts
        .iter()
        .enumerate()
        .map(|(index, contact)| (contact.position, crate::redpiler::program_graph::ContactId::new(index as u32)))
        .collect();
    let piston_nodes: FxHashMap<_, _> = geometry.pistons.iter().map(|piston| (piston.id, piston.node)).collect();
    let profile_at = electrical
        .profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| (profile.raw_state, crate::redpiler::program_graph::ProfileId::new(index as u32)))
        .collect();
    let contact_variant_bytes = electrical
        .contacts
        .iter()
        .map(|contact| {
            contact
                .variants
                .len()
                .saturating_mul(std::mem::size_of::<crate::redpiler::program_graph::ContactVariant>())
        })
        .sum();
    let mut builder = Builder {
        budget,
        world,
        geometry,
        electrical,
        geometry_table_bytes,
        contact_variant_bytes,
        contact_at,
        piston_nodes,
        profile_at,
        site_at: FxHashMap::default(),
        routes: Vec::new(),
        links: Vec::new(),
        delivery_entries: 0,
    };
    for (index, site) in builder.electrical.sites.iter().enumerate() {
        builder.site_at.insert(site.position, SiteId::new(index as u32));
    }

    builder.compile_component_routes()?;
    builder.compile_wire_routes()?;
    let pistons = builder.compile_piston_routes()?;
    builder.budget.check()?;
    builder.electrical.remaining_cells = builder.budget.remaining;
    Ok(CallbackDraft {
        routes: builder.routes,
        links: builder.links,
        pistons,
    })
}

struct Builder<'a, W> {
    budget: PreparationBudget<'a>,
    world: &'a W,
    geometry: &'a GeometryDraft,
    electrical: &'a mut ElectricalDraft,
    geometry_table_bytes: usize,
    contact_variant_bytes: usize,
    contact_at: FxHashMap<BlockPos, crate::redpiler::program_graph::ContactId>,
    piston_nodes: FxHashMap<crate::redpiler::program_graph::PistonId, crate::redpiler::program_graph::NodeId>,
    profile_at: FxHashMap<u32, crate::redpiler::program_graph::ProfileId>,
    site_at: FxHashMap<BlockPos, SiteId>,
    routes: Vec<Route>,
    links: Vec<LinkSpec>,
    delivery_entries: usize,
}

impl<W: World> Builder<'_, W> {
    fn compile_component_routes(&mut self) -> Result<(), CallbackError> {
        for index in 0..self.electrical.nodes.len() {
            self.budget.check()?;
            let Some(position) = self.electrical.nodes[index].position else {
                continue;
            };
            let node = crate::redpiler::program_graph::NodeId::try_from_index(index).ok_or(CallbackError::TooManyEntries { table: "nodes" })?;
            let kind = self.electrical.nodes[index].kind.clone();
            let block = self.world.get_block(position);
            let events = match kind {
                CircuitNodeKind::Observer { facing, .. } => observer_events(position, facing),
                CircuitNodeKind::Electrical(ty) => match ty {
                    NodeType::Lever | NodeType::Button => control_events(block, position),
                    NodeType::PressurePlate => vec![around_events(position, true), around_events(position.offset(BlockFace::Bottom), true)]
                        .into_iter()
                        .flatten()
                        .collect(),
                    NodeType::Torch => around_events(position, false),
                    NodeType::Repeater { .. } => diode_events(block, position, false),
                    NodeType::Comparator { .. } => diode_events(block, position, true),
                    NodeType::CopperBulb => component_output_events(position, false),
                    NodeType::CommandBlock { .. } => component_output_events(position, true),
                    _ => continue,
                },
                _ => continue,
            };
            let route = self.compile_route(node, position, events)?;
            self.electrical.nodes[node.index()].output_route = Some(route);
        }
        Ok(())
    }

    fn compile_wire_routes(&mut self) -> Result<(), CallbackError> {
        for wire_index in 0..self.electrical.wires.len() {
            self.budget.check()?;
            let node = self.electrical.wires[wire_index].node;
            let canonical_neighbors = self.electrical.wires[wire_index].canonical_neighbors;
            let position = self.electrical.nodes[node.index()].position.expect("wire nodes have positions");
            let events = wire_shape_events(position);
            let shape_route = self.compile_route(node, position, events)?;
            self.electrical.wires[wire_index].shape_route = Some(shape_route);

            let mut candidates = Vec::with_capacity(canonical_neighbors.len());
            for &target in &canonical_neighbors {
                let target_pos = self.electrical.sites[target.index()].position;
                let mut event = Event::update(target_pos, None);
                event.view = OperandView::Captured;
                candidates.push(event);
            }
            let turbo_route = self.compile_route(node, position, candidates)?;
            self.electrical.wires[wire_index].turbo_route = Some(turbo_route);
        }
        Ok(())
    }

    fn compile_piston_routes(&mut self) -> Result<Vec<PistonSpec>, CallbackError> {
        let mut specs = Vec::with_capacity(self.geometry.pistons.len());
        for piston in self.geometry.pistons.iter().copied() {
            self.budget.consume(1)?;
            let mut base_changed = Vec::new();
            append_notify(&mut base_changed, piston.position);
            let mut near_changed = Vec::new();
            let near_pos = self.electrical.contacts[piston.near.index()].position;
            append_notify(&mut near_changed, near_pos);
            let far_changed = if matches!(piston.lane, PistonLane::SingleResource { .. }) {
                let mut events = Vec::new();
                append_notify(&mut events, self.electrical.contacts[piston.far.index()].position);
                Some(self.compile_route(piston.node, piston.position, events)?)
            } else {
                None
            };
            let far_shape_changed = if matches!(piston.lane, PistonLane::SingleResource { .. }) {
                let mut events = Vec::new();
                append_shape_changed(&mut events, self.electrical.contacts[piston.far.index()].position);
                Some(self.compile_route(piston.node, piston.position, events)?)
            } else {
                None
            };

            let mut base_restored = vec![Event::update(piston.position, None)];
            append_notify(&mut base_restored, piston.position);
            let mut head_restored = Vec::new();
            append_notify(&mut head_restored, near_pos);
            let (resource_restored_near, resource_restored_far) = if matches!(piston.lane, PistonLane::SingleResource { .. }) {
                let mut near_events = vec![Event::update(near_pos, None)];
                append_notify(&mut near_events, near_pos);
                let far_pos = self.electrical.contacts[piston.far.index()].position;
                let mut far_events = vec![Event::update(far_pos, None)];
                append_notify(&mut far_events, far_pos);
                (
                    Some(self.compile_route(piston.node, piston.position, near_events)?),
                    Some(self.compile_route(piston.node, piston.position, far_events)?),
                )
            } else {
                (None, None)
            };

            let routes = PistonRoutes {
                base_changed: self.compile_route(piston.node, piston.position, base_changed)?,
                near_changed: self.compile_route(piston.node, piston.position, near_changed)?,
                far_changed,
                far_shape_changed,
                base_restored: self.compile_route(piston.node, piston.position, base_restored)?,
                head_restored: self.compile_route(piston.node, piston.position, head_restored)?,
                resource_restored_near,
                resource_restored_far,
            };
            specs.push(PistonSpec {
                node: piston.node,
                position: piston.position,
                facing: piston.facing,
                sticky: piston.sticky,
                base: piston.base,
                near: piston.near,
                far: piston.far,
                lane: piston.lane,
                routes,
            });
        }
        Ok(specs)
    }

    fn compile_route(
        &mut self,
        source: crate::redpiler::program_graph::NodeId,
        source_position: BlockPos,
        events: Vec<Event>,
    ) -> Result<RouteId, CallbackError> {
        self.budget.consume(1)?;
        let route_id = RouteId::try_from_index(self.routes.len()).ok_or(CallbackError::TooManyEntries { table: "routes" })?;
        let mut deliveries = Vec::with_capacity(events.len());
        for (sequence, event) in events.into_iter().enumerate() {
            self.budget.check()?;
            let sequence = u32::try_from(sequence).map_err(|_| CallbackError::TooManyEntries { table: "deliveries" })?;
            let target = self.ensure_site(event.position)?;
            let mut event = event;
            if let Some(position) = event.solid_at {
                let Some(solid) = self.solid_guard(position, event.view)? else {
                    continue;
                };
                event.condition = Some(match event.condition {
                    Some(condition) => self.combine_guards(solid, Some(condition))?,
                    None => solid,
                });
            }
            let branches = self.branches(target, event, source_position)?;
            for (guard, receiver) in branches {
                self.budget.consume(1)?;
                deliveries.push(RouteDelivery {
                    target,
                    kind: event.kind,
                    guard,
                    view: event.view,
                    sequence,
                    receiver,
                });
                self.budget
                    .check_estimated_bytes("callbacks", self.callback_capacity_estimate(deliveries.capacity()))?;
                let Some(receiver) = receiver else {
                    continue;
                };
                let to = self.receiver_node(receiver)?;
                let kind = match event.kind {
                    DeliveryKind::Update { direction } => LinkKind::Update { direction, guard },
                    DeliveryKind::Shape { direction } => LinkKind::Shape { direction, guard },
                };
                self.budget.consume(1)?;
                self.links.push(LinkSpec {
                    from: source,
                    to,
                    kind,
                    callback: CallbackEdgeRef { route: route_id, sequence },
                });
                self.budget
                    .check_estimated_bytes("callbacks", self.callback_capacity_estimate(deliveries.capacity()))?;
            }
        }
        let delivery_count = deliveries.len();
        self.routes.push(Route {
            source,
            deliveries: deliveries.into_boxed_slice(),
        });
        self.delivery_entries = self.delivery_entries.saturating_add(delivery_count);
        Ok(route_id)
    }

    fn callback_capacity_estimate(&self, pending_delivery_capacity: usize) -> usize {
        use crate::redpiler::program_graph::{
            CircuitNodeDraft, ComparatorInputPlan, ContactSpec, Guard, PistonSpec, Profile, Route, RouteDelivery, SiteSpec, WirePlan, WireRelation,
        };
        let bytes = |capacity: usize, element_size: usize| capacity.saturating_mul(element_size);
        self.geometry_table_bytes
            .saturating_add(bytes(self.electrical.nodes.capacity(), std::mem::size_of::<CircuitNodeDraft>()))
            .saturating_add(bytes(self.electrical.profiles.capacity(), std::mem::size_of::<Profile>()))
            .saturating_add(bytes(
                self.electrical.guards.capacity(),
                std::mem::size_of::<Guard>() + std::mem::size_of::<GuardId>() * 4,
            ))
            .saturating_add(bytes(self.electrical.contacts.capacity(), std::mem::size_of::<ContactSpec>()))
            .saturating_add(self.contact_variant_bytes)
            .saturating_add(bytes(self.electrical.sites.capacity(), std::mem::size_of::<SiteSpec>()))
            .saturating_add(bytes(self.electrical.wires.capacity(), std::mem::size_of::<WirePlan>()))
            .saturating_add(bytes(
                self.electrical.links.capacity(),
                std::mem::size_of::<crate::redpiler::analysis::electrical::ElectricalLink>(),
            ))
            .saturating_add(bytes(self.electrical.wire_relations.capacity(), std::mem::size_of::<WireRelation>()))
            .saturating_add(bytes(self.electrical.comparators.capacity(), std::mem::size_of::<ComparatorInputPlan>()))
            .saturating_add(bytes(self.geometry.pistons.len(), std::mem::size_of::<PistonSpec>()))
            .saturating_add(bytes(self.routes.capacity() + 1, std::mem::size_of::<Route>()))
            .saturating_add(bytes(
                self.delivery_entries + pending_delivery_capacity,
                std::mem::size_of::<RouteDelivery>(),
            ))
            .saturating_add(bytes(self.links.capacity(), std::mem::size_of::<LinkSpec>()))
    }

    fn branches(
        &mut self,
        target: SiteId,
        event: Event,
        source_position: BlockPos,
    ) -> Result<Vec<(GuardId, Option<CallbackReceiver>)>, CallbackError> {
        let site = self.electrical.sites[target.index()];
        let condition = event.condition;
        let mut branches = Vec::new();
        match site.source {
            SiteSource::Contact(contact_id) => {
                let contact = self.electrical.contacts[contact_id.index()].clone();
                for variant in contact.variants.iter() {
                    self.budget.check()?;
                    let block = Block::from_id(self.electrical.profiles[variant.profile.index()].raw_state);
                    if !self.allowed(event.filter, block, event.position) {
                        continue;
                    }
                    if let Some(reason) = super::unsupported_component_reason(block) {
                        return Err(CallbackError::UnsupportedComponent {
                            source: source_position,
                            target: event.position,
                            block: block.get_id(),
                            reason,
                        });
                    }
                    let guard = if event.view == OperandView::Live {
                        variant.when
                    } else {
                        self.push_guard(Guard::Site {
                            site: target,
                            view: OperandView::Captured,
                            test: SiteTest::Profile(variant.profile),
                        })?
                    };
                    let guard = self.combine_guards(guard, condition)?;
                    let receiver = match event.kind {
                        DeliveryKind::Shape { .. } => Some(CallbackReceiver::Contact(contact_id)),
                        DeliveryKind::Update { .. } => {
                            if matches!(block, Block::MovingPiston { .. }) {
                                None
                            } else if !redstone::has_neighbor_update(block) {
                                None
                            } else {
                                variant.receiver
                            }
                        }
                    };
                    if receiver.is_none() && redstone::has_neighbor_update(block) {
                        return Err(CallbackError::UninventoriedReceiver {
                            source: source_position,
                            target: event.position,
                            block: block.get_id(),
                        });
                    }
                    branches.push((guard, receiver));
                }
            }
            SiteSource::Component(node) => {
                let block = self.world.get_block(event.position);
                if self.allowed(event.filter, block, event.position) {
                    if let Some(reason) = super::unsupported_component_reason(block) {
                        return Err(CallbackError::UnsupportedComponent {
                            source: source_position,
                            target: event.position,
                            block: block.get_id(),
                            reason,
                        });
                    }
                    let guard = self.combine_guards(self.always_guard(), condition)?;
                    let receiver = match event.kind {
                        DeliveryKind::Shape { .. } => Some(CallbackReceiver::Component(node)),
                        DeliveryKind::Update { .. } => redstone::has_neighbor_update(block).then_some(CallbackReceiver::Component(node)),
                    };
                    branches.push((guard, receiver));
                }
            }
            SiteSource::Constant(profile) => {
                let block = Block::from_id(self.electrical.profiles[profile.index()].raw_state);
                if self.allowed(event.filter, block, event.position) {
                    if let Some(reason) = super::unsupported_component_reason(block) {
                        return Err(CallbackError::UnsupportedComponent {
                            source: source_position,
                            target: event.position,
                            block: block.get_id(),
                            reason,
                        });
                    }
                    let requires_receiver = match event.kind {
                        DeliveryKind::Update { .. } => redstone::has_neighbor_update(block),
                        DeliveryKind::Shape { .. } => {
                            crate::interaction::attachment_support(block, event.position).is_some()
                                && !crate::interaction::is_valid_position(block, self.world, event.position)
                        }
                    };
                    if requires_receiver {
                        return Err(CallbackError::UninventoriedReceiver {
                            source: source_position,
                            target: event.position,
                            block: block.get_id(),
                        });
                    }
                    let guard = self.combine_guards(self.always_guard(), condition)?;
                    branches.push((guard, None));
                }
            }
        }
        Ok(branches)
    }

    fn allowed(&self, filter: Filter, block: Block, target: BlockPos) -> bool {
        match filter {
            Filter::Any => true,
            Filter::Around {
                source,
                diagonal,
                skip_pistons,
            } => {
                !diagonal
                    || !(target == source && block.is_copper_bulb()
                        || matches!(block, Block::Observer { .. })
                        || (skip_pistons && matches!(block, Block::Piston { .. })))
            }
            Filter::ExcludeObservers => !matches!(block, Block::Observer { .. }),
            Filter::ObserversOnly => matches!(block, Block::Observer { .. }),
            Filter::ComparatorsOnly => matches!(block, Block::RedstoneComparator { .. }),
        }
    }

    fn ensure_site(&mut self, position: BlockPos) -> Result<SiteId, CallbackError> {
        self.budget.check()?;
        if let Some(&site) = self.site_at.get(&position) {
            return Ok(site);
        }
        if self.world.get_chunk(position.x.div_euclid(16), position.z.div_euclid(16)).is_none() {
            return Err(CallbackError::UnloadedTarget { position });
        }
        self.budget.consume(1)?;
        let block = self.world.get_block(position);
        let source =
            if let Some(&contact) = self.contact_at.get(&position) {
                SiteSource::Contact(contact)
            } else if let Some((index, _)) =
                self.electrical.nodes.iter().enumerate().find(|(_, node)| {
                    node.position == Some(position) && !matches!(node.kind, CircuitNodeKind::Piston(_) | CircuitNodeKind::Contact(_))
                })
            {
                SiteSource::Component(crate::redpiler::program_graph::NodeId::new(index as u32))
            } else {
                SiteSource::Constant(self.profile_for(block)?)
            };
        let site = SiteId::try_from_index(self.electrical.sites.len()).ok_or(CallbackError::TooManyEntries { table: "sites" })?;
        self.budget.consume(1)?;
        self.electrical.sites.push(SiteSpec {
            position,
            source,
            source_strength: redstone::source_strength(block, self.world, position),
        });
        self.site_at.insert(position, site);
        Ok(site)
    }

    fn profile_for(&mut self, block: Block) -> Result<crate::redpiler::program_graph::ProfileId, CallbackError> {
        if let Some(&profile) = self.profile_at.get(&block.get_id()) {
            return Ok(profile);
        }
        let profile = crate::redpiler::program_graph::ProfileId::try_from_index(self.electrical.profiles.len())
            .ok_or(CallbackError::TooManyEntries { table: "profiles" })?;
        self.budget.consume(1)?;
        self.electrical.profiles.push(crate::redpiler::program_graph::Profile {
            raw_state: block.get_id(),
            registry_type: if matches!(block, Block::Unknown { .. }) {
                u32::MAX
            } else {
                block.registry_id()
            },
            solid: block.is_solid(),
            transparent: block.is_transparent(),
            cube: block.is_cube(),
            electrical: if matches!(block, Block::RedstoneBlock {}) {
                crate::redpiler::program_graph::ProfileElectrical::RedstoneSource
            } else {
                crate::redpiler::program_graph::ProfileElectrical::None
            },
        });
        self.profile_at.insert(block.get_id(), profile);
        Ok(profile)
    }

    fn receiver_node(&self, receiver: CallbackReceiver) -> Result<crate::redpiler::program_graph::NodeId, CallbackError> {
        match receiver {
            CallbackReceiver::Component(node) => Ok(node),
            receiver @ (CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => self
                .piston_nodes
                .get(&piston)
                .copied()
                .ok_or(CallbackError::InvalidReceiverReference { receiver }),
            CallbackReceiver::Contact(contact) => {
                self.electrical
                    .contacts
                    .get(contact.index())
                    .map(|contact| contact.node)
                    .ok_or(CallbackError::InvalidReceiverReference {
                        receiver: CallbackReceiver::Contact(contact),
                    })
            }
        }
    }

    fn always_guard(&self) -> GuardId {
        GuardId::new(0)
    }

    fn combine_guards(&mut self, first: GuardId, second: Option<GuardId>) -> Result<GuardId, CallbackError> {
        let Some(second) = second else {
            return Ok(first);
        };
        if first == self.always_guard() {
            return Ok(second);
        }
        self.push_guard(Guard::All(vec![first, second].into_boxed_slice()))
    }

    fn push_guard(&mut self, value: Guard) -> Result<GuardId, CallbackError> {
        self.budget.consume(1)?;
        let guard = GuardId::try_from_index(self.electrical.guards.len()).ok_or(CallbackError::TooManyEntries { table: "guards" })?;
        self.electrical.guards.push(value);
        Ok(guard)
    }

    fn solid_guard(&mut self, position: BlockPos, view: OperandView) -> Result<Option<GuardId>, CallbackError> {
        let site_id = self.ensure_site(position)?;
        let site = self.electrical.sites[site_id.index()];
        match site.source {
            SiteSource::Constant(profile) => Ok(self.electrical.profiles[profile.index()].solid.then_some(self.always_guard())),
            SiteSource::Component(_) => Ok(self.world.get_block(position).is_solid().then_some(self.always_guard())),
            SiteSource::Contact(contact) => {
                let variants = self.electrical.contacts[contact.index()].variants.clone();
                let solid_profiles: Vec<_> = variants
                    .iter()
                    .filter_map(|variant| {
                        self.electrical.profiles[variant.profile.index()]
                            .solid
                            .then_some((variant.profile, variant.when))
                    })
                    .collect();
                let mut guards = Vec::new();
                for (profile, live_guard) in solid_profiles {
                    guards.push(if view == OperandView::Live {
                        live_guard
                    } else {
                        self.push_guard(Guard::Site {
                            site: site_id,
                            view: OperandView::Captured,
                            test: SiteTest::Profile(profile),
                        })?
                    });
                }
                if guards.is_empty() {
                    return Ok(None);
                }
                if guards.contains(&self.always_guard()) {
                    return Ok(Some(self.always_guard()));
                }
                if guards.len() == 1 {
                    return Ok(guards.first().copied());
                }
                Ok(Some(self.push_guard(Guard::Any(guards.into_boxed_slice()))?))
            }
        }
    }
}

fn control_events(block: Block, position: BlockPos) -> Vec<Event> {
    let support = match block {
        Block::Lever { lever } => match lever.face {
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        },
        Block::StoneButton { button } => match button.face {
            ButtonFace::Floor => BlockFace::Bottom,
            ButtonFace::Ceiling => BlockFace::Top,
            ButtonFace::Wall => button.facing.opposite().block_face(),
        },
        _ => return Vec::new(),
    };
    around_events(position, true)
        .into_iter()
        .chain(around_events(position.offset(support), true))
        .collect()
}

fn around_events(position: BlockPos, skip_pistons: bool) -> Vec<Event> {
    let mut events = Vec::new();
    redstone::surrounding_notifications(position, |target, direction, diagonal| {
        events.push(Event {
            position: target,
            kind: DeliveryKind::Update { direction: Some(direction) },
            filter: Filter::Around {
                source: position,
                diagonal,
                skip_pistons,
            },
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        });
    });
    events
}

fn diode_events(block: Block, position: BlockPos, comparator: bool) -> Vec<Event> {
    let facing = match block {
        Block::RedstoneRepeater { repeater } => repeater.facing.block_face(),
        Block::RedstoneComparator { comparator } => comparator.facing.block_face(),
        _ => return Vec::new(),
    };
    let front = position.offset(facing.opposite());
    let first_direction = if comparator { facing.opposite() } else { facing };
    let mut events = Vec::new();
    redstone::diode_notifications(front, first_direction, |target, direction| {
        events.push(Event::update(target, direction));
    });
    events
}

fn observer_events(position: BlockPos, facing: BlockFace) -> Vec<Event> {
    let mut events = Vec::new();
    redstone::observer_notifications(facing.into(), position, |target, direction| {
        events.push(Event::update(target, direction));
    });
    events
}

fn component_output_events(position: BlockPos, skip_pistons: bool) -> Vec<Event> {
    let mut events = around_events(position, skip_pistons);
    let horizontal = [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West];
    for face in horizontal {
        let adjacent = position.offset(face);
        let far = adjacent.offset(face);
        let mut event = Event::update(far, None);
        event.filter = Filter::ComparatorsOnly;
        event.solid_at = Some(adjacent);
        events.push(event);
    }
    events
}

fn wire_shape_events(position: BlockPos) -> Vec<Event> {
    let mut events = Vec::new();
    redstone::wire_update_notifications(position, |target, direction, indirect| {
        events.push(Event {
            position: target,
            kind: DeliveryKind::Update { direction: Some(direction) },
            filter: if indirect { Filter::ExcludeObservers } else { Filter::Any },
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        });
    });
    events
}

fn append_notify(events: &mut Vec<Event>, position: BlockPos) {
    append_shape_changed(events, position);
    redstone::piston::neighbor_notifications(position, |target, direction| {
        events.push(Event {
            position: target,
            kind: DeliveryKind::Update { direction: Some(direction) },
            filter: Filter::ExcludeObservers,
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        });
    });
}

fn append_shape_changed(events: &mut Vec<Event>, position: BlockPos) {
    redstone::piston::shape_notifications(position, |target, direction| {
        events.push(Event::shape(target, direction));
        events.push(Event {
            position: target,
            kind: DeliveryKind::Update {
                direction: Some(direction.opposite()),
            },
            filter: Filter::ObserversOnly,
            condition: None,
            solid_at: None,
            view: OperandView::Live,
        });
    });
}
