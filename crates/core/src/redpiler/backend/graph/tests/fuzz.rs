//! Exhaustive one-actor baseline differential campaign.
use super::*;
use crate::plot::PlotWorld;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneObserver, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use mchprs_world::{PistonAction, PistonEvent, TickPriority};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

const ORIGIN: BlockPos = BlockPos::new(8, 20, 8);
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorForm {
    StickyEmpty,
    StickyRedstone,
    StickyWool,
    OrdinaryEmpty,
}

impl ActorForm {
    fn sticky(self) -> bool {
        !matches!(self, Self::OrdinaryEmpty)
    }

    fn payload(self) -> Option<Block> {
        match self {
            Self::StickyRedstone => Some(Block::RedstoneBlock),
            Self::StickyWool => Some(Block::from_name("white_wool").unwrap()),
            Self::StickyEmpty | Self::OrdinaryEmpty => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Template {
    form: ActorForm,
    facing: BlockFacing,
    extended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Normal,
    Quasi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    NormalUse,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotifyTarget {
    Base,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    SetControl { input: Input, powered: bool, delivery: Delivery },
    Notify { target: NotifyTarget, direction: Option<BlockFace> },
    Tick { count: u8 },
}

#[derive(Debug, Clone)]
struct Protocol {
    id: &'static str,
    actions: Vec<Action>,
}

fn set(input: Input, powered: bool, delivery: Delivery) -> Action {
    Action::SetControl { input, powered, delivery }
}

fn notify(target: NotifyTarget) -> Action {
    Action::Notify { target, direction: None }
}

fn tick(count: u8) -> Action {
    Action::Tick { count }
}

fn protocols() -> Vec<Protocol> {
    let normal = Input::Normal;
    let quasi = Input::Quasi;
    let use_input = Delivery::NormalUse;
    let silent = Delivery::Silent;
    vec![
        Protocol {
            id: "idle",
            actions: vec![tick(8)],
        },
        Protocol {
            id: "pulse_one",
            actions: vec![set(normal, true, use_input), tick(1), set(normal, false, use_input), tick(8)],
        },
        Protocol {
            id: "pulse_two",
            actions: vec![set(normal, true, use_input), tick(2), set(normal, false, use_input), tick(8)],
        },
        Protocol {
            id: "pulse_three",
            actions: vec![set(normal, true, use_input), tick(3), set(normal, false, use_input), tick(8)],
        },
        Protocol {
            id: "held",
            actions: vec![set(normal, true, use_input), tick(8), set(normal, false, use_input), tick(8)],
        },
        Protocol {
            id: "cancel_before_acceptance",
            actions: vec![set(normal, true, use_input), set(normal, false, silent), tick(1)],
        },
        Protocol {
            id: "qc_data_then_sample",
            actions: vec![
                set(quasi, true, silent),
                tick(1),
                notify(NotifyTarget::Base),
                set(quasi, false, silent),
                tick(1),
                notify(NotifyTarget::Base),
                tick(3),
            ],
        },
        Protocol {
            id: "duplicate_and_alternating_samples",
            actions: vec![
                notify(NotifyTarget::Base),
                notify(NotifyTarget::Base),
                notify(NotifyTarget::Base),
                set(normal, true, silent),
                notify(NotifyTarget::Base),
                notify(NotifyTarget::Base),
                set(normal, false, silent),
                notify(NotifyTarget::Base),
                notify(NotifyTarget::Base),
                tick(4),
            ],
        },
    ]
}

#[derive(Debug, Clone)]
struct Fixture {
    template: Template,
    base: BlockPos,
    near: BlockPos,
    far: BlockPos,
    normal: BlockPos,
    quasi: BlockPos,
    blocks: Vec<(BlockPos, Block)>,
    watched: Vec<BlockPos>,
}

fn horizontal_control_sides(facing: BlockFacing) -> (BlockFace, BlockFace) {
    match facing {
        BlockFacing::North | BlockFacing::South => (BlockFace::East, BlockFace::West),
        BlockFacing::East | BlockFacing::West => (BlockFace::North, BlockFace::South),
        BlockFacing::Up | BlockFacing::Down => (BlockFace::North, BlockFace::East),
    }
}

fn wall_lever(support: BlockFace, powered: bool) -> Block {
    let facing: BlockDirection = support.opposite().unwrap_direction();
    Block::Lever {
        lever: Lever::new(LeverFace::Wall, facing, powered),
    }
}

fn fixture(template: Template) -> Fixture {
    let facing: BlockFace = template.facing.into();
    let base = ORIGIN;
    let near = base.offset(facing);
    let far = near.offset(facing);
    let (normal_side, quasi_side) = horizontal_control_sides(template.facing);
    let normal = base.offset(normal_side);
    let normal_support = normal.offset(normal_side);
    let quasi = base.offset(BlockFace::Top).offset(quasi_side);
    let quasi_support = quasi.offset(quasi_side);
    let piston = Block::Piston {
        piston: RedstonePiston {
            facing: template.facing,
            sticky: template.form.sticky(),
            extended: template.extended,
        },
    };
    let mut blocks = vec![
        (base, piston),
        (normal_support, Block::Stone {}),
        (normal, wall_lever(normal_side, false)),
        (quasi_support, Block::Stone {}),
        (quasi, wall_lever(quasi_side, false)),
    ];
    if template.extended {
        blocks.push((
            near,
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: template.facing,
                    sticky: template.form.sticky(),
                    short: false,
                },
            },
        ));
    }
    if let Some(payload) = template.form.payload() {
        blocks.push((if template.extended { far } else { near }, payload));
    }
    let watched = vec![base, near, far, normal, normal_support, quasi, quasi_support];
    Fixture {
        template,
        base,
        near,
        far,
        normal,
        quasi,
        blocks,
        watched,
    }
}

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}

fn load_fixture(fixture: &Fixture) -> PlotWorld {
    let mut world = empty_world();
    for &(position, block) in &fixture.blocks {
        world.set_block(position, block);
    }
    world
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Operation {
    Callback { target: BlockPos, direction: Option<BlockFace> },
    Sample { actor: BlockPos, extended: bool, powered: bool },
    Request { actor: BlockPos, action: PistonAction },
    AcceptanceAttempt { actor: BlockPos, action: PistonAction },
    Accepted { actor: BlockPos, action: PistonAction },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingTick {
    position: BlockPos,
    ticks_left: u32,
    priority: TickPriority,
    block_type: Option<u32>,
    order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeadlineKind {
    Base,
    Head,
    Resource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Deadline {
    position: BlockPos,
    kind: DeadlineKind,
    due_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceObservation {
    source: BlockPos,
    contact: Option<BlockPos>,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Boundary {
    tick: u64,
    ordinal: usize,
    action: usize,
    normal_strength: u8,
    quasi_strength: u8,
    pose: Option<bool>,
    availability: PistonAvailability,
    cells: Vec<(BlockPos, Block)>,
    resource: Option<ResourceObservation>,
    pending_ticks: Vec<PendingTick>,
    requests: Vec<PistonEvent>,
    deadlines: Vec<Deadline>,
    operations: Vec<Operation>,
}

#[derive(Debug, Default, Clone, Copy)]
struct EpisodeCoverage {
    samples: usize,
    same_value_samples: usize,
    requests: usize,
    attempts: usize,
    accepted_extensions: usize,
    accepted_retractions: usize,
    pending_publication_boundaries: usize,
}

struct ReplayResult {
    divergence: Option<String>,
    descriptor: String,
    coverage: EpisodeCoverage,
    trace: Option<String>,
    qc_trace: Option<String>,
}

fn first_difference(expected: &Boundary, actual: &Boundary) -> Option<String> {
    macro_rules! differs {
        ($field:ident) => {
            if expected.$field != actual.$field {
                return Some(format!(
                    "{}: expected {:?}, actual {:?}",
                    stringify!($field),
                    expected.$field,
                    actual.$field
                ));
            }
        };
    }
    differs!(tick);
    differs!(ordinal);
    differs!(action);
    differs!(normal_strength);
    differs!(quasi_strength);
    differs!(pose);
    differs!(availability);
    differs!(cells);
    differs!(resource);
    differs!(pending_ticks);
    differs!(requests);
    differs!(deadlines);
    differs!(operations);
    None
}

fn difference_field(witness: &str) -> &'static str {
    [
        "tick",
        "ordinal",
        "action",
        "normal_strength",
        "quasi_strength",
        "pose",
        "availability",
        "cells",
        "resource",
        "pending_ticks",
        "requests",
        "deadlines",
        "operations",
    ]
    .into_iter()
    .find(|field| witness.contains(&format!("{field}: expected")))
    .unwrap_or("unknown")
}

fn semantic_callback(block: &str) -> bool {
    matches!(
        block,
        "redstone_wire"
            | "redstone_torch"
            | "redstone_wall_torch"
            | "redstone_repeater"
            | "redstone_comparator"
            | "redstone_lamp"
            | "observer"
            | "piston"
            | "sticky_piston"
            | "piston_head"
            | "copper_bulb"
    )
}

pub(super) fn native_operations(entries: &[Value], watched: &[BlockPos]) -> Vec<Operation> {
    entries
        .iter()
        .filter_map(|entry| {
            let kind = entry["kind"].as_str()?;
            let data = &entry["data"];
            match kind {
                "callback" if semantic_callback(data["block"].as_str()?) => {
                    let target: BlockPos = serde_json::from_value(data["pos"].clone()).ok()?;
                    if !watched.contains(&target) {
                        return None;
                    }
                    let direction = serde_json::from_value(data["dir"].clone()).ok()?;
                    Some(Operation::Callback { target, direction })
                }
                "piston_sample" => Some(Operation::Sample {
                    actor: serde_json::from_value(data["pos"].clone()).ok()?,
                    extended: data["extended"].as_bool()?,
                    powered: data["powered"].as_bool()?,
                }),
                "event_enqueue" => {
                    let event: PistonEvent = serde_json::from_value(data.clone()).ok()?;
                    Some(Operation::Request {
                        actor: event.pos,
                        action: event.action,
                    })
                }
                "event_execute" => {
                    let event: PistonEvent = serde_json::from_value(data.clone()).ok()?;
                    Some(Operation::AcceptanceAttempt {
                        actor: event.pos,
                        action: event.action,
                    })
                }
                "event_applied" => {
                    let event: PistonEvent = serde_json::from_value(data.clone()).ok()?;
                    Some(Operation::Accepted {
                        actor: event.pos,
                        action: event.action,
                    })
                }
                _ => None,
            }
        })
        .collect()
}

fn update_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool, silent: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("fixture lever missing at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    if !silent {
        redstone::update_surrounding_blocks(world, position);
        let support = match lever.face {
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        redstone::update_surrounding_blocks(world, position.offset(support));
    }
}

fn set_graph_control(backend: &mut GraphBackend, position: BlockPos, powered: bool, delivery: Delivery) -> Result<(), GraphExecutionError> {
    let node = backend.positions[&position];
    let state = backend.node_state(node);
    if state.powered == powered {
        return Ok(());
    }
    if delivery == Delivery::NormalUse {
        backend.use_block(position)
    } else {
        let ComponentState::Electrical(state) = &mut backend.state.components[node.index()] else {
            unreachable!("lever is a fixed electrical component")
        };
        state.powered = powered;
        state.output_strength = if powered { 15 } else { 0 };
        Ok(())
    }
}

fn graph_notify(backend: &mut GraphBackend, position: BlockPos, direction: Option<BlockFace>) -> Result<(), GraphExecutionError> {
    let site = backend
        .program
        .sites()
        .iter()
        .enumerate()
        .filter(|(_, site)| site.position == position)
        .find_map(|(index, _)| backend.resolve_site(SiteId::new(index as u32), None));
    let Some(site) = site.filter(|site| redstone::has_neighbor_update(site.block)) else {
        return Ok(());
    };
    match site.receiver {
        Some(CallbackReceiver::Component(node)) => {
            let receiver = CallbackReceiver::Component(node);
            if backend.has_semantic_callback(receiver) {
                backend
                    .observed_operations
                    .push(GraphObservedOperation::Callback { target: position, direction });
            }
            backend.update_component(node, DeliveryKind::Update { direction }, None)
        }
        Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => {
            backend
                .observed_operations
                .push(GraphObservedOperation::Callback { target: position, direction });
            backend.sample_piston(piston)
        }
        Some(CallbackReceiver::Contact(_)) | None => Ok(()),
    }
}

fn native_channels(world: &PlotWorld, base: BlockPos, facing: BlockFace) -> (u8, u8) {
    let is_powered = |position: BlockPos, side: BlockFace| redstone::get_redstone_power(world.get_block(position), world, position, side);
    let normal = BlockFace::values()
        .into_iter()
        .filter(|&side| side != facing)
        .map(|side| is_powered(base.offset(side), side))
        .chain(std::iter::once(is_powered(base, BlockFace::Bottom)))
        .max()
        .unwrap_or(0);
    let above = base.offset(BlockFace::Top);
    let quasi = BlockFace::values()
        .into_iter()
        .filter(|&side| side != BlockFace::Bottom)
        .map(|side| is_powered(above.offset(side), side))
        .max()
        .unwrap_or(0);
    (normal, quasi)
}

fn native_availability(world: &PlotWorld, fixture: &Fixture) -> PistonAvailability {
    if matches!(world.get_block(fixture.base), Block::MovingPiston { .. }) {
        if matches!(world.get_block(fixture.near), Block::PistonHead { .. }) {
            return PistonAvailability::BasePendingHeadPresent;
        }
        return PistonAvailability::BasePending;
    }
    if matches!(world.get_block(fixture.near), Block::MovingPiston { .. }) {
        return PistonAvailability::HeadPending;
    }
    PistonAvailability::Ready
}

fn native_pose(world: &PlotWorld, fixture: &Fixture) -> Option<bool> {
    match world.get_block(fixture.base) {
        Block::Piston { piston } => Some(piston.extended),
        _ => None,
    }
}

fn native_resource(world: &PlotWorld, fixture: &Fixture) -> Option<ResourceObservation> {
    let material = fixture.template.form.payload()?;
    for position in [fixture.near, fixture.far] {
        let block = world.get_block(position);
        if block == material {
            return Some(ResourceObservation {
                source: if fixture.template.extended { fixture.far } else { fixture.near },
                contact: Some(position),
                enabled: true,
            });
        }
        if matches!(block, Block::MovingPiston { .. }) {
            if let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity(position) {
                if Block::from_id(entity.block_state) == material && !entity.source {
                    return Some(ResourceObservation {
                        source: if fixture.template.extended { fixture.far } else { fixture.near },
                        contact: Some(position),
                        enabled: false,
                    });
                }
            }
        }
    }
    None
}

fn graph_piston(backend: &GraphBackend, base: BlockPos) -> (PistonSpec, PistonState) {
    let (index, spec) = backend
        .program
        .pistons()
        .iter()
        .enumerate()
        .find(|(_, spec)| backend.program.nodes()[spec.node.index()].position == Some(base))
        .expect("compiled fixture piston");
    (*spec, backend.state.pistons[index])
}

fn graph_resource(backend: &GraphBackend, fixture: &Fixture) -> Option<ResourceObservation> {
    fixture.template.form.payload()?;
    let (spec, _) = graph_piston(backend, fixture.base);
    let PistonLane::SingleResource { far } = spec.lane else {
        return None;
    };
    let near = spec.near;
    let (index, _) = backend
        .program
        .resources()
        .iter()
        .enumerate()
        .find(|(_, resource)| resource.contacts.contains(&near) && resource.contacts.contains(&far))?;
    let state = backend.state.resources[index];
    let contact = backend.program.contacts()[state.at.index()].position;
    Some(ResourceObservation {
        source: if fixture.template.extended { fixture.far } else { fixture.near },
        contact: Some(contact),
        enabled: state.enabled,
    })
}

fn graph_block(backend: &GraphBackend, position: BlockPos) -> Block {
    if let Some((_, spec)) = backend
        .program
        .pistons()
        .iter()
        .enumerate()
        .find(|(_, spec)| backend.program.nodes()[spec.node.index()].position == Some(position))
    {
        let (_, state) = graph_piston(backend, position);
        return Block::Piston {
            piston: RedstonePiston {
                facing: spec.facing.into(),
                sticky: spec.sticky,
                extended: state.extended,
            },
        };
    }
    if let Some((index, _)) = backend
        .program
        .sites()
        .iter()
        .enumerate()
        .find(|(_, site)| site.position == position && matches!(site.source, SiteSource::Contact(_)))
    {
        return backend
            .resolve_site(SiteId::new(index as u32), None)
            .map(|site| site.block)
            .unwrap_or(Block::Air);
    }
    if let Some(&node) = backend.positions.get(&position) {
        return backend.node_block(node);
    }
    backend
        .program
        .sites()
        .iter()
        .enumerate()
        .filter(|(_, site)| site.position == position)
        .find_map(|(index, _)| backend.resolve_site(SiteId::new(index as u32), None).map(|site| site.block))
        .unwrap_or(Block::Air)
}

fn native_cell(world: &PlotWorld, position: BlockPos) -> Block {
    match world.get_block(position) {
        Block::MovingPiston { .. } => Block::Air,
        block => block,
    }
}

fn graph_cell(backend: &GraphBackend, fixture: &Fixture, position: BlockPos) -> Block {
    let (_, state) = graph_piston(backend, fixture.base);
    if position == fixture.base && state.availability.base_pending() {
        return Block::Air;
    }
    if position == fixture.near && state.availability == PistonAvailability::HeadPending {
        return Block::Air;
    }
    if let Some(resource) = graph_resource(backend, fixture) {
        if resource.contact == Some(position) && !resource.enabled {
            return Block::Air;
        }
    }
    graph_block(backend, position)
}

fn pending_ticks(world: &PlotWorld) -> Vec<PendingTick> {
    world
        .scheduler()
        .iter_entries()
        .enumerate()
        .map(|(order, tick)| PendingTick {
            position: tick.pos,
            ticks_left: tick.ticks_left,
            priority: tick.tick_priority,
            block_type: tick.block_type,
            order,
        })
        .collect()
}

fn graph_pending_ticks(backend: &GraphBackend) -> Vec<PendingTick> {
    backend
        .scheduled_ticks()
        .into_iter()
        .enumerate()
        .map(|(order, tick)| PendingTick {
            position: tick.pos,
            ticks_left: tick.ticks_left,
            priority: tick.tick_priority,
            block_type: tick.block_type,
            order,
        })
        .collect()
}

fn native_requests(world: &PlotWorld) -> Vec<PistonEvent> {
    world.piston_state().events.iter().copied().collect()
}

fn graph_requests(backend: &GraphBackend) -> Vec<PistonEvent> {
    backend
        .future
        .piston_requests
        .iter()
        .map(|request| {
            let spec = backend.program.pistons()[request.piston.index()];
            PistonEvent {
                pos: backend.program.nodes()[spec.node.index()].position.unwrap(),
                sticky: spec.sticky,
                facing: spec.facing,
                action: request.action,
            }
        })
        .collect()
}

fn native_deadlines(world: &PlotWorld, fixture: &Fixture) -> Vec<Deadline> {
    let tick = world.piston_state().logical_tick;
    let mut deadlines = Vec::new();
    for motion in &world.piston_state().motions {
        let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity(motion.pos) else {
            continue;
        };
        let kind = if entity.source {
            if motion.pos == fixture.base {
                DeadlineKind::Base
            } else {
                DeadlineKind::Head
            }
        } else {
            DeadlineKind::Resource
        };
        let visits = ((1.0 - motion.progress).max(0.0) / 0.5).ceil() as u64 + 1;
        deadlines.push(Deadline {
            position: motion.pos,
            kind,
            due_tick: tick + visits,
        });
    }
    deadlines.sort_by_key(|deadline| {
        (
            deadline.due_tick,
            deadline.position.x,
            deadline.position.y,
            deadline.position.z,
            deadline.kind as u8,
        )
    });
    deadlines
}

fn graph_deadlines(backend: &GraphBackend, fixture: &Fixture) -> Vec<Deadline> {
    let mut deadlines = Vec::new();
    for event in &backend.future.deferred_pistons {
        let state = backend.state.pistons[event.piston.index()];
        let actor_valid = state.revision == event.revision;
        let spec = backend.program.pistons()[event.piston.index()];
        let base = backend.program.contacts()[spec.base.index()].position;
        let near = backend.program.contacts()[spec.near.index()].position;
        if event.extending {
            if actor_valid {
                deadlines.push(Deadline {
                    position: near,
                    kind: DeadlineKind::Head,
                    due_tick: event.due_tick,
                });
            }
        } else {
            if actor_valid {
                deadlines.push(Deadline {
                    position: base,
                    kind: DeadlineKind::Base,
                    due_tick: event.due_tick,
                });
            }
        }
        if let Some(resource_event) = event.resource {
            let resource = backend.state.resources[resource_event.resource.index()];
            if resource.revision == resource_event.revision && !resource.enabled {
                deadlines.push(Deadline {
                    position: backend.program.contacts()[resource_event.contact.index()].position,
                    kind: DeadlineKind::Resource,
                    due_tick: event.due_tick,
                });
            }
        }
    }
    let _ = fixture;
    deadlines.sort_by_key(|deadline| {
        (
            deadline.due_tick,
            deadline.position.x,
            deadline.position.y,
            deadline.position.z,
            deadline.kind as u8,
        )
    });
    deadlines
}

pub(super) fn graph_operations_since(backend: &GraphBackend, start: usize, watched: &[BlockPos]) -> Vec<Operation> {
    backend.observed_operations[start..]
        .iter()
        .filter_map(|operation| match operation {
            GraphObservedOperation::Callback { target, direction } if watched.contains(target) => Some(Operation::Callback {
                target: *target,
                direction: *direction,
            }),
            GraphObservedOperation::Sample { actor, extended, powered } if watched.contains(actor) => Some(Operation::Sample {
                actor: *actor,
                extended: *extended,
                powered: *powered,
            }),
            GraphObservedOperation::Request { actor, action } if watched.contains(actor) => Some(Operation::Request {
                actor: *actor,
                action: *action,
            }),
            GraphObservedOperation::AcceptanceAttempt { actor, action } if watched.contains(actor) => Some(Operation::AcceptanceAttempt {
                actor: *actor,
                action: *action,
            }),
            GraphObservedOperation::Accepted { actor, action } if watched.contains(actor) => Some(Operation::Accepted {
                actor: *actor,
                action: *action,
            }),
            _ => None,
        })
        .collect()
}

fn callback_operations(operations: &[Operation]) -> Vec<Operation> {
    operations
        .iter()
        .filter(|operation| matches!(operation, Operation::Callback { .. }))
        .cloned()
        .collect()
}

fn observe_native(world: &PlotWorld, fixture: &Fixture, ordinal: usize, action: usize, operations: Vec<Operation>) -> Boundary {
    let (normal_strength, quasi_strength) = native_channels(world, fixture.base, fixture.template.facing.into());
    Boundary {
        tick: world.piston_state().logical_tick,
        ordinal,
        action,
        normal_strength,
        quasi_strength,
        pose: native_pose(world, fixture),
        availability: native_availability(world, fixture),
        cells: [fixture.base, fixture.near, fixture.far, fixture.normal, fixture.quasi]
            .into_iter()
            .map(|position| (position, native_cell(world, position)))
            .collect(),
        resource: native_resource(world, fixture),
        pending_ticks: pending_ticks(world),
        requests: native_requests(world),
        deadlines: native_deadlines(world, fixture),
        operations,
    }
}

fn observe_graph(backend: &GraphBackend, fixture: &Fixture, ordinal: usize, action: usize, operations: Vec<Operation>) -> Boundary {
    let (spec, state) = graph_piston(backend, fixture.base);
    Boundary {
        tick: backend.future.clock.logical_tick,
        ordinal,
        action,
        normal_strength: backend.input_strength(spec.node, InputPort::Main),
        quasi_strength: backend.input_strength(spec.node, InputPort::Quasi),
        pose: (!state.availability.base_pending()).then_some(state.extended),
        availability: state.availability,
        cells: [fixture.base, fixture.near, fixture.far, fixture.normal, fixture.quasi]
            .into_iter()
            .map(|position| (position, graph_cell(backend, fixture, position)))
            .collect(),
        resource: graph_resource(backend, fixture),
        pending_ticks: graph_pending_ticks(backend),
        requests: graph_requests(backend),
        deadlines: graph_deadlines(backend, fixture),
        operations,
    }
}

fn native_action(world: &mut PlotWorld, fixture: &Fixture, action: &Action) {
    match *action {
        Action::SetControl { input, powered, delivery } => {
            let position = match input {
                Input::Normal => fixture.normal,
                Input::Quasi => fixture.quasi,
            };
            update_native_lever(world, position, powered, delivery == Delivery::Silent);
        }
        Action::Notify { target, direction } => {
            let position = match target {
                NotifyTarget::Base => fixture.base,
            };
            redstone::update(world.get_block(position), world, position, direction);
        }
        Action::Tick { .. } => unreachable!("ticks are expanded by replay"),
    }
}

fn graph_action(backend: &mut GraphBackend, fixture: &Fixture, action: &Action) -> Result<(), GraphExecutionError> {
    match *action {
        Action::SetControl { input, powered, delivery } => {
            let position = match input {
                Input::Normal => fixture.normal,
                Input::Quasi => fixture.quasi,
            };
            set_graph_control(backend, position, powered, delivery)
        }
        Action::Notify { target, direction } => {
            let position = match target {
                NotifyTarget::Base => fixture.base,
            };
            graph_notify(backend, position, direction)
        }
        Action::Tick { .. } => unreachable!("ticks are expanded by replay"),
    }
}

fn replay(template: Template, protocol: &Protocol) -> Result<ReplayResult, String> {
    let fixture = fixture(template);
    let mut native = load_fixture(&fixture);
    let graph_world = load_fixture(&fixture);
    let ticks = native.scheduler().iter_entries().collect();
    let mut backend = GraphBackend::compile(&graph_world, BOUNDS, ticks, Arc::default()).map_err(|error| format!("compile: {error}"))?;
    backend.set_test_recording(true);
    let mut ordinal = 0usize;
    let mut first_failure = None;
    let mut coverage = EpisodeCoverage::default();
    let mut trace = None;
    let mut qc_trace = String::new();
    let initial_expected = observe_native(&native, &fixture, ordinal, 0, Vec::new());
    let initial_actual = observe_graph(&backend, &fixture, ordinal, 0, Vec::new());
    if let Some(reason) = first_difference(&initial_expected, &initial_actual) {
        first_failure = Some(format!("initial {reason}"));
    }

    for (action_index, action) in protocol.actions.iter().enumerate() {
        let action_number = action_index + 1;
        match *action {
            Action::Tick { count } => {
                for _ in 0..count {
                    ordinal += 1;
                    let graph_start = backend.observed_operations.len();
                    let native_entries = capture_at(&fixture.watched, || native.tick_interpreted());
                    let graph_result = backend.tick();
                    if let Err(error) = graph_result {
                        return Err(format!("tick execution at {ordinal}: {error}"));
                    }
                    let native_ops = native_operations(&native_entries, &fixture.watched);
                    let graph_ops = graph_operations_since(&backend, graph_start, &fixture.watched);
                    let expected = observe_native(&native, &fixture, ordinal, action_number, native_ops.clone());
                    let actual = observe_graph(&backend, &fixture, ordinal, action_number, graph_ops.clone());
                    if !actual.deadlines.is_empty() {
                        coverage.pending_publication_boundaries += 1;
                    }
                    if protocol.id == "qc_data_then_sample"
                        && template.form == ActorForm::StickyEmpty
                        && template.facing == BlockFacing::East
                        && !template.extended
                        && ordinal <= 6
                    {
                        use std::fmt::Write;
                        writeln!(
                            &mut qc_trace,
                            "boundary {ordinal}: main={}, qc={}, ops={graph_ops:?}, requests={:?}",
                            actual.normal_strength, actual.quasi_strength, actual.requests
                        )
                        .unwrap();
                    }
                    if protocol.id == "pulse_one"
                        && template.form == ActorForm::StickyRedstone
                        && template.facing == BlockFacing::East
                        && !template.extended
                        && ordinal == 2
                    {
                        trace = Some(format!(
                            "{template:?}/{} after tick {}: native operations={native_ops:?}; graph operations={graph_ops:?}; graph deadlines={:?}",
                            protocol.id, ordinal, actual.deadlines
                        ));
                    }
                    if first_failure.is_none() {
                        first_failure = first_difference(&expected, &actual)
                            .map(|reason| format!("boundary {ordinal} after tick: {reason}; expected={expected:?}; actual={actual:?}"));
                    }
                }
            }
            _ => {
                ordinal += 1;
                let graph_start = backend.observed_operations.len();
                let native_entries = capture_at(&fixture.watched, || native_action(&mut native, &fixture, action));
                graph_action(&mut backend, &fixture, action).map_err(|error| format!("input execution at boundary {ordinal}: {error}"))?;
                let native_ops = native_operations(&native_entries, &fixture.watched);
                let graph_ops = graph_operations_since(&backend, graph_start, &fixture.watched);
                let expected = observe_native(&native, &fixture, ordinal, action_number, native_ops);
                let actual = observe_graph(&backend, &fixture, ordinal, action_number, graph_ops);
                if !actual.deadlines.is_empty() {
                    coverage.pending_publication_boundaries += 1;
                }
                if protocol.id == "qc_data_then_sample"
                    && template.form == ActorForm::StickyEmpty
                    && template.facing == BlockFacing::East
                    && !template.extended
                    && ordinal <= 6
                {
                    use std::fmt::Write;
                    writeln!(
                        &mut qc_trace,
                        "boundary {ordinal} after {action:?}: main={}, qc={}, ops={:?}, requests={:?}",
                        actual.normal_strength, actual.quasi_strength, actual.operations, actual.requests
                    )
                    .unwrap();
                }
                if first_failure.is_none() {
                    first_failure = first_difference(&expected, &actual)
                        .map(|reason| format!("boundary {ordinal} after {action:?}: {reason}; expected={expected:?}; actual={actual:?}"));
                }
            }
        }
    }
    for operation in &backend.observed_operations {
        match operation {
            GraphObservedOperation::Sample { extended, powered, .. } => {
                coverage.samples += 1;
                coverage.same_value_samples += usize::from(extended == powered);
            }
            GraphObservedOperation::Request { .. } => coverage.requests += 1,
            GraphObservedOperation::AcceptanceAttempt { .. } => coverage.attempts += 1,
            GraphObservedOperation::Accepted { action, .. } => match action {
                PistonAction::Extend => coverage.accepted_extensions += 1,
                PistonAction::Retract | PistonAction::RetractWithoutPull => coverage.accepted_retractions += 1,
            },
            GraphObservedOperation::Callback { .. } => {}
        }
    }
    Ok(ReplayResult {
        divergence: first_failure,
        descriptor: format!("fixture={fixture:?}; script={:?}", protocol.actions),
        coverage,
        trace,
        qc_trace: (!qc_trace.is_empty()).then_some(qc_trace),
    })
}

fn templates() -> Vec<Template> {
    let forms = [
        ActorForm::StickyEmpty,
        ActorForm::StickyRedstone,
        ActorForm::StickyWool,
        ActorForm::OrdinaryEmpty,
    ];
    let facings = [
        BlockFacing::North,
        BlockFacing::East,
        BlockFacing::South,
        BlockFacing::West,
        BlockFacing::Up,
        BlockFacing::Down,
    ];
    forms
        .into_iter()
        .flat_map(|form| {
            facings
                .into_iter()
                .flat_map(move |facing| [false, true].into_iter().map(move |extended| Template { form, facing, extended }))
        })
        .collect()
}

#[test]
fn comparison_detects_observable_differences() {
    let fixture = fixture(Template {
        form: ActorForm::StickyRedstone,
        facing: BlockFacing::East,
        extended: false,
    });
    let initial = Boundary {
        tick: 0,
        ordinal: 0,
        action: 0,
        normal_strength: 1,
        quasi_strength: 0,
        pose: Some(false),
        availability: PistonAvailability::Ready,
        cells: vec![(fixture.base, Block::Air)],
        resource: Some(ResourceObservation {
            source: fixture.near,
            contact: Some(fixture.near),
            enabled: true,
        }),
        pending_ticks: vec![PendingTick {
            position: fixture.base,
            ticks_left: 1,
            priority: TickPriority::Normal,
            block_type: Some(1),
            order: 0,
        }],
        requests: vec![PistonEvent {
            pos: fixture.base,
            sticky: true,
            facing: BlockFace::East,
            action: PistonAction::Extend,
        }],
        deadlines: vec![Deadline {
            position: fixture.near,
            kind: DeadlineKind::Head,
            due_tick: 3,
        }],
        operations: vec![
            Operation::Callback {
                target: fixture.near,
                direction: Some(BlockFace::South),
            },
            Operation::Callback {
                target: fixture.base,
                direction: Some(BlockFace::North),
            },
            Operation::AcceptanceAttempt {
                actor: fixture.base,
                action: PistonAction::Extend,
            },
            Operation::Accepted {
                actor: fixture.base,
                action: PistonAction::Extend,
            },
        ],
    };
    let mut changed = initial.clone();
    changed.operations[0] = Operation::Callback {
        target: fixture.near,
        direction: Some(BlockFace::North),
    };
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.operations.swap(0, 1);
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.operations.pop();
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.normal_strength = 15;
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.resource.as_mut().unwrap().enabled = false;
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.resource.as_mut().unwrap().contact = Some(fixture.far);
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.deadlines[0].due_tick += 1;
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.pending_ticks[0].ticks_left += 1;
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.requests[0].action = PistonAction::Retract;
    assert!(first_difference(&initial, &changed).is_some());
    changed = initial.clone();
    changed.operations.pop();
    assert!(first_difference(&initial, &changed).is_some());
}

#[test]
fn isolated_piston_baselines_match_interpreter() {
    let templates = templates();
    let protocols = protocols();
    assert_eq!(templates.len(), 48);
    assert_eq!(protocols.len(), 8);
    let candidates = templates.len() * protocols.len();
    let mut admitted = 0usize;
    let mut executed = 0usize;
    let mut rejected = Vec::new();
    let mut incomplete = Vec::new();
    let mut divergent = 0usize;
    let mut divergence_fields = BTreeMap::new();
    let mut first_divergence = None;
    let mut campaign_coverage = EpisodeCoverage::default();
    let mut protocol_divergences = vec![0usize; protocols.len()];
    let mut example_trace = None;
    let mut example_qc_trace = None;

    for template in templates {
        for (protocol_index, protocol) in protocols.iter().enumerate() {
            match replay(template, protocol) {
                Ok(result) => {
                    admitted += 1;
                    executed += 1;
                    campaign_coverage.samples += result.coverage.samples;
                    campaign_coverage.same_value_samples += result.coverage.same_value_samples;
                    campaign_coverage.requests += result.coverage.requests;
                    campaign_coverage.attempts += result.coverage.attempts;
                    campaign_coverage.accepted_extensions += result.coverage.accepted_extensions;
                    campaign_coverage.accepted_retractions += result.coverage.accepted_retractions;
                    campaign_coverage.pending_publication_boundaries += result.coverage.pending_publication_boundaries;
                    if example_trace.is_none() {
                        example_trace = result.trace;
                    }
                    if example_qc_trace.is_none() {
                        example_qc_trace = result.qc_trace;
                    }
                    if let Some(witness) = result.divergence {
                        protocol_divergences[protocol_index] += 1;
                        divergent += 1;
                        *divergence_fields.entry(difference_field(&witness)).or_insert(0usize) += 1;
                        if first_divergence.is_none() {
                            first_divergence = Some(format!("case={template:?}/{} witness={witness}; {}", protocol.id, result.descriptor));
                        }
                    }
                }
                Err(error) if error.starts_with("compile:") => {
                    rejected.push(format!("case={template:?}/{} {error}", protocol.id));
                }
                Err(error) => {
                    incomplete.push(format!("case={template:?}/{} {error}", protocol.id));
                }
            }
        }
    }

    eprintln!(
        "C0 baseline: candidates={candidates}, admitted={admitted}, executed={executed}, expected_rejected=0, unexpected_rejected={}, divergent={}, incomplete={}; coverage: forms=4, facings=6, poses=2, protocols=8, samples={}, same_value_samples={}, requests={}, acceptance_attempts={}, accepted_extend={}, accepted_retract={}, cancelled={}, pending_publication_boundaries={}",
        rejected.len(),
        divergent,
        incomplete.len(),
        campaign_coverage.samples,
        campaign_coverage.same_value_samples,
        campaign_coverage.requests,
        campaign_coverage.attempts,
        campaign_coverage.accepted_extensions,
        campaign_coverage.accepted_retractions,
        campaign_coverage
            .attempts
            .saturating_sub(campaign_coverage.accepted_extensions + campaign_coverage.accepted_retractions),
        campaign_coverage.pending_publication_boundaries,
    );
    eprintln!(
        "per-protocol divergent episodes: {}",
        protocols
            .iter()
            .zip(&protocol_divergences)
            .map(|(protocol, count)| format!("{}={count}/48", protocol.id))
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!(
        "first-difference fields: {}",
        divergence_fields
            .iter()
            .map(|(field, count)| format!("{field}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some(trace) = example_trace {
        eprintln!("operation trace sample: {trace}");
    }
    if let Some(trace) = example_qc_trace {
        eprintln!("QC data/sample trace: {trace}");
    }
    for failure in rejected.iter().chain(incomplete.iter()) {
        eprintln!("{failure}");
    }
    assert!(
        rejected.is_empty(),
        "positive baseline unexpectedly rejected; first: {:?}",
        rejected.first()
    );
    assert!(incomplete.is_empty(), "baseline execution incomplete; first: {:?}", incomplete.first());
    assert!(
        divergent == 0,
        "native/graph baseline has {divergent} divergent episodes; first: {}",
        first_divergence.as_deref().unwrap_or("none")
    );
}

fn assert_replay_matches_interpreter(template: Template, protocol: Protocol) {
    let result = replay(template, &protocol).expect("fixture should compile and replay");
    assert_eq!(result.divergence, None, "{}: {}", protocol.id, result.descriptor);
}

#[test]
fn extension_publication_does_not_resample_the_installing_head() {
    let template = Template {
        form: ActorForm::StickyEmpty,
        facing: BlockFacing::North,
        extended: false,
    };
    let protocol = protocols().into_iter().find(|protocol| protocol.id == "pulse_one").unwrap();

    assert_replay_matches_interpreter(template, protocol);
}

#[test]
fn settled_retraction_notifies_the_present_head_before_removal() {
    let template = Template {
        form: ActorForm::StickyEmpty,
        facing: BlockFacing::North,
        extended: true,
    };
    let protocol = Protocol {
        id: "settled_retraction",
        actions: vec![
            set(Input::Normal, true, Delivery::NormalUse),
            set(Input::Normal, false, Delivery::NormalUse),
            tick(1),
        ],
    };

    assert_replay_matches_interpreter(template, protocol);
}

#[test]
fn moving_base_updates_do_not_dispatch_as_stationary_piston_samples() {
    let template = Template {
        form: ActorForm::StickyEmpty,
        facing: BlockFacing::North,
        extended: true,
    };
    let protocol = Protocol {
        id: "moving_base_update",
        actions: vec![
            set(Input::Normal, true, Delivery::NormalUse),
            set(Input::Normal, false, Delivery::NormalUse),
            tick(1),
            notify(NotifyTarget::Base),
        ],
    };

    assert_replay_matches_interpreter(template, protocol);
}

#[test]
fn payload_extension_preserves_native_near_notification_order() {
    let template = Template {
        form: ActorForm::StickyRedstone,
        facing: BlockFacing::North,
        extended: false,
    };
    let protocol = Protocol {
        id: "payload_extension",
        actions: vec![set(Input::Normal, true, Delivery::NormalUse), tick(1)],
    };

    assert_replay_matches_interpreter(template, protocol);
}

#[test]
fn dropped_payload_reuse_keeps_far_resource_live() {
    let mut divergences = Vec::new();
    for form in [ActorForm::StickyRedstone, ActorForm::StickyWool] {
        let mut fixture = fixture(Template {
            form,
            facing: BlockFacing::East,
            extended: false,
        });
        let consumer = fixture.far.offset(BlockFace::North);
        fixture.blocks.push((consumer.offset(BlockFace::Bottom), Block::Stone {}));
        fixture.blocks.push((
            consumer,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::South,
                    powered: false,
                },
            },
        ));
        fixture.watched.push(consumer);
        let mut native = load_fixture(&fixture);
        let graph_world = load_fixture(&fixture);
        let ticks = native.scheduler().iter_entries().collect();
        let mut backend = GraphBackend::compile(&graph_world, BOUNDS, ticks, Arc::default())
            .unwrap_or_else(|error| panic!("dropped payload fixture rejected: {error}"));
        backend.set_test_recording(true);

        update_native_lever(&mut native, fixture.normal, true, false);
        backend.use_block(fixture.normal).unwrap();
        native.tick_interpreted();
        backend.tick().unwrap();
        assert!(matches!(native.get_block(fixture.base), Block::Piston { piston } if piston.extended));
        assert!(graph_piston(&backend, fixture.base).1.extended);
        update_native_lever(&mut native, fixture.normal, false, false);
        backend.use_block(fixture.normal).unwrap();
        assert!(matches!(
            native.piston_state().events.back(),
            Some(event) if event.action == PistonAction::RetractWithoutPull
        ));
        assert!(
            backend
                .future
                .piston_requests
                .iter()
                .any(|request| { request.action == PistonAction::RetractWithoutPull })
        );
        for _ in 0..6 {
            native.tick_interpreted();
            backend.tick().unwrap();
        }

        let mut failure = None;
        let native_restored = matches!(native.get_block(fixture.base), Block::Piston { piston } if !piston.extended)
            && native.get_block(fixture.far) == form.payload().unwrap();
        let graph_restored = graph_resource(&backend, &fixture).is_some_and(|resource| resource.contact == Some(fixture.far) && resource.enabled)
            && graph_piston(&backend, fixture.base).1.availability == PistonAvailability::Ready
            && !graph_piston(&backend, fixture.base).1.extended;
        if !native_restored || !graph_restored {
            failure = Some(format!(
                "{form:?} did not reach a retracted base with its enabled Far resource: native_base={:?}, native_far={:?}, graph_resource={:?}, graph_piston={:?}",
                native.get_block(fixture.base),
                native.get_block(fixture.far),
                graph_resource(&backend, &fixture),
                graph_piston(&backend, fixture.base).1
            ));
        }

        let input_start = backend.observed_operations.len();
        let native_input = capture_at(&fixture.watched, || update_native_lever(&mut native, fixture.normal, true, false));
        backend.use_block(fixture.normal).unwrap();
        let expected_input = native_operations(&native_input, &fixture.watched);
        let actual_input = graph_operations_since(&backend, input_start, &fixture.watched);
        assert!(native.piston_state().events.iter().any(|event| { event.action == PistonAction::Extend }));
        assert!(
            backend
                .future
                .piston_requests
                .iter()
                .any(|request| request.action == PistonAction::Extend)
        );
        if expected_input != actual_input && failure.is_none() {
            failure = Some(format!(
                "{form:?} re-extension input operations differ: native={expected_input:?}, graph={actual_input:?}"
            ));
        }

        let tick_start = backend.observed_operations.len();
        let native_tick = capture_at(&fixture.watched, || native.tick_interpreted());
        backend.tick().unwrap();
        let expected_tick = native_operations(&native_tick, &fixture.watched);
        let actual_tick = graph_operations_since(&backend, tick_start, &fixture.watched);
        let native_observer_calls = callback_operations(&expected_tick)
            .into_iter()
            .filter(|operation| matches!(operation, Operation::Callback { target, .. } if *target == consumer))
            .count();
        let graph_observer_calls = callback_operations(&actual_tick)
            .into_iter()
            .filter(|operation| matches!(operation, Operation::Callback { target, .. } if *target == consumer))
            .count();
        let reextended =
            matches!(native.get_block(fixture.base), Block::Piston { piston } if piston.extended) && graph_piston(&backend, fixture.base).1.extended;
        let resource_still_live = native.get_block(fixture.far) == form.payload().unwrap()
            && graph_resource(&backend, &fixture).is_some_and(|resource| resource.contact == Some(fixture.far) && resource.enabled);
        if (!reextended || !resource_still_live || native_observer_calls != graph_observer_calls) && failure.is_none() {
            failure = Some(format!(
                "{form:?} second extension failed or changed/re-notified the dropped Far resource: reextended={reextended}, native_far={:?}, graph_resource={:?}, observer_callbacks(native/graph)={native_observer_calls}/{graph_observer_calls}, trace(native/graph)={expected_tick:?}/{actual_tick:?}",
                native.get_block(fixture.far),
                graph_resource(&backend, &fixture)
            ));
        }
        if native_observer_calls != 0 || graph_observer_calls != 0 {
            failure.get_or_insert_with(|| {
                format!(
                    "{form:?} second extension unexpectedly notified the Far observer: native={native_observer_calls}, graph={graph_observer_calls}"
                )
            });
        }
        if let Some(failure) = failure {
            divergences.push(failure);
        }
    }
    assert!(divergences.is_empty(), "{}", divergences.join("\n"));
}

#[path = "continuation.rs"]
mod continuation;
