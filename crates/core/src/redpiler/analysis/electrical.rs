//! Read-only power-channel discovery over the stationary geometry draft.
#![allow(dead_code)] // Step 05 assembles this power plan with callback routes before graph activation.
use super::geometry::{GeometryDraft, GeometryView};
use super::{AnalysisError, PreparationBudget};
use crate::plot::PLOT_BLOCK_HEIGHT;
use crate::redpiler::TaskMonitor;
use crate::redpiler::components::{NodeState, NodeType, identify_block};
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeDraft, CircuitNodeKind, ComponentState, ContactId, ContactSpec, ContactVariant, Guard, GuardId, InputPort, NodeId,
    OperandView, PowerPort, Profile, ProfileElectrical, ProfileId, SiteId, SiteSource, SiteSpec, SiteTest, StateTest, WirePlan,
};
pub(crate) use crate::redpiler::program_graph::{ComparatorInputPlan, ComparatorOverride, WireRelation, WireRelationKind};
use crate::redstone::{comparator, power};
use crate::world::{World, try_for_each_block_optimized, wire_cache};
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElectricalError {
    Preparation(AnalysisError),
    UnsupportedComponent { position: BlockPos, block: u32, reason: &'static str },
    DynamicComparatorOverrideNotInventoried { comparator: BlockPos, source: BlockPos },
}

impl fmt::Display for ElectricalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation(error) => error.fmt(f),
            Self::UnsupportedComponent { position, block, reason } => {
                write!(f, "unsupported {} at {position:?}: {reason}", Block::from_id(*block).get_name())
            }
            Self::DynamicComparatorOverrideNotInventoried { comparator, source } => write!(
                f,
                "comparator at {comparator:?} has dynamic override source at {source:?} outside the compiled electrical inventory"
            ),
        }
    }
}

impl std::error::Error for ElectricalError {}

impl From<AnalysisError> for ElectricalError {
    fn from(error: AnalysisError) -> Self {
        Self::Preparation(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElectricalTarget {
    Input { node: NodeId, port: InputPort },
    Conductor { site: SiteId, include_dust: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ElectricalLink {
    pub(crate) from: SiteId,
    pub(crate) from_port: PowerPort,
    pub(crate) to: ElectricalTarget,
    /// The literal query face is retained even when the selected port also names it.
    pub(crate) face: BlockFace,
    pub(crate) attenuation: u8,
    pub(crate) guard: GuardId,
}

#[derive(Debug, Clone)]
pub(crate) struct ElectricalDraft {
    pub(crate) remaining_cells: usize,
    pub(crate) nodes: Vec<CircuitNodeDraft>,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) guards: Vec<Guard>,
    pub(crate) contacts: Vec<ContactSpec>,
    pub(crate) sites: Vec<SiteSpec>,
    pub(crate) wires: Vec<WirePlan>,
    pub(crate) links: Vec<ElectricalLink>,
    pub(crate) wire_relations: Vec<WireRelation>,
    pub(crate) comparators: Vec<ComparatorInputPlan>,
}

#[derive(Debug, Clone)]
struct Component {
    position: BlockPos,
    node: NodeId,
    ty: NodeType,
    state: NodeState,
    block: Block,
}

#[derive(Clone, Copy)]
struct WireComponent {
    position: BlockPos,
    node: NodeId,
    state: RedstoneWire,
}

#[derive(Debug, Clone, Copy)]
struct Root {
    position: BlockPos,
    face: BlockFace,
    port: InputPort,
    condition: RootCondition,
    wire_only: bool,
    stored_wire: bool,
    comparator_side: bool,
}

#[derive(Debug, Clone, Copy)]
enum RootCondition {
    Always,
    WireAscend { source: BlockPos, neighbor: BlockPos },
    WireDescend { neighbor: BlockPos },
}

/// Compile directional channel reads, including the local alternatives exposed by geometry.
pub(crate) fn prepare_electrical(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    geometry: &GeometryDraft,
    monitor: &TaskMonitor,
) -> Result<ElectricalDraft, ElectricalError> {
    let mut budget = PreparationBudget::new(monitor, geometry.remaining_cells);
    let retained_geometry_bytes = monitor.preparation_stage_estimate("geometry");
    let bounds = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
    let mut block_positions = Vec::new();
    try_for_each_block_optimized(world, bounds.0, bounds.1, |position| {
        budget.consume(1)?;
        if block_at(world, position) != Block::Air {
            block_positions.push(position);
        }
        budget.check()
    })?;
    block_positions.sort_by_key(|p| (p.y, p.z, p.x));
    block_positions.dedup();

    let contact_at: FxHashMap<_, _> = geometry
        .contacts
        .iter()
        .enumerate()
        .map(|(index, contact)| (contact.position, (ContactId::new(index as u32), contact)))
        .collect();
    let mut components = Vec::new();
    let mut component_at = FxHashMap::default();
    let mut nodes = geometry.nodes.clone();
    for &position in &block_positions {
        budget.consume(1)?;
        let block = block_at(world, position);
        let Some((ty, state)) = identify_block(block, position, world) else {
            continue;
        };
        let node = NodeId::new(nodes.len() as u32);
        component_at.insert(position, node);
        components.push(Component {
            position,
            node,
            ty: ty.clone(),
            state: state.clone(),
            block,
        });
        // Specialized nodes receive their site-backed plans after sites are interned below.
        budget.consume(1)?;
        nodes.push(CircuitNodeDraft {
            position: Some(position),
            raw_state: Some(block.get_id()),
            kind: CircuitNodeKind::Electrical(ty.clone()),
            state: ComponentState::Electrical(state.clone()),
            output_route: None,
        });
    }

    budget.consume(block_positions.len())?;
    let mut positions: FxHashSet<BlockPos> = block_positions.iter().copied().collect();
    for contact in &geometry.contacts {
        budget.check()?;
        add_site_position(&mut positions, contact.position, &mut budget)?;
    }
    for observation in &geometry.observations {
        budget.check()?;
        add_site_position(&mut positions, observation.position, &mut budget)?;
    }
    for component in &components {
        budget.check()?;
        for face in BlockFace::values() {
            let adjacent = component.position.offset(face);
            add_site_position(&mut positions, adjacent, &mut budget)?;
            for side in BlockFace::values() {
                add_site_position(&mut positions, adjacent.offset(side), &mut budget)?;
            }
        }
    }
    for piston in &geometry.pistons {
        budget.check()?;
        for center in [piston.position, piston.position.offset(BlockFace::Top)] {
            add_site_position(&mut positions, center, &mut budget)?;
            for face in BlockFace::values() {
                let adjacent = center.offset(face);
                add_site_position(&mut positions, adjacent, &mut budget)?;
                for side in BlockFace::values() {
                    add_site_position(&mut positions, adjacent.offset(side), &mut budget)?;
                }
            }
        }
    }
    let wires: Vec<_> = components
        .iter()
        .filter_map(|component| {
            let Block::RedstoneWire { wire } = component.block else {
                return None;
            };
            matches!(component.ty, NodeType::Wire).then_some(WireComponent {
                position: component.position,
                node: component.node,
                state: wire,
            })
        })
        .collect();
    for wire in &wires {
        budget.check()?;
        for neighbor in wire_cache::positions(wire.position) {
            add_site_position(&mut positions, neighbor, &mut budget)?;
        }
    }
    let mut positions: Vec<_> = positions.into_iter().collect();
    positions.sort_by_key(|p| (p.y, p.z, p.x));

    let mut profiles = geometry.profiles.clone();
    for profile in &mut profiles {
        budget.check()?;
        let block = Block::from_id(profile.raw_state);
        profile.electrical = if matches!(block, Block::RedstoneBlock) {
            ProfileElectrical::RedstoneSource
        } else if is_component(block) {
            ProfileElectrical::Component
        } else {
            ProfileElectrical::None
        };
    }
    let mut profile_at: FxHashMap<u32, ProfileId> = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| (profile.raw_state, ProfileId::new(index as u32)))
        .collect();
    let mut profile_for = |block: Block| {
        let raw_state = block.get_id();
        *profile_at.entry(raw_state).or_insert_with(|| {
            let id = ProfileId::new(profiles.len() as u32);
            let electrical = if matches!(block, Block::RedstoneBlock) {
                ProfileElectrical::RedstoneSource
            } else if is_component(block) {
                ProfileElectrical::Component
            } else {
                ProfileElectrical::None
            };
            profiles.push(Profile {
                raw_state,
                registry_type: if matches!(block, Block::Unknown { .. }) {
                    u32::MAX
                } else {
                    block.registry_id()
                },
                solid: block.is_solid(),
                transparent: block.is_transparent(),
                cube: block.is_cube(),
                electrical,
            });
            id
        })
    };

    let mut guard_table = vec![Guard::Always];
    let always = GuardId::new(0);
    let mut sites = Vec::with_capacity(positions.len());
    let mut site_at = FxHashMap::default();
    for position in positions {
        budget.consume(1)?;
        let site = SiteId::new(sites.len() as u32);
        let block = block_at(world, position);
        let source = if let Some((contact, _)) = contact_at.get(&position) {
            SiteSource::Contact(*contact)
        } else if let Some(component) = component_at.get(&position) {
            SiteSource::Component(*component)
        } else {
            SiteSource::Constant(profile_for(block))
        };
        site_at.insert(position, site);
        sites.push(SiteSpec {
            position,
            source,
            source_strength: crate::redstone::source_strength(block, world, position),
        });
    }
    let site = |position: BlockPos| site_at[&position];

    let mut wire_plans = Vec::new();
    for wire in &wires {
        budget.consume(1)?;
        let mut canonical_neighbors = [SiteId::new(0); 24];
        for (index, neighbor) in wire_cache::positions(wire.position).into_iter().enumerate() {
            canonical_neighbors[index] = site(neighbor);
        }
        let plan = wire_plans.len();
        wire_plans.push(WirePlan {
            node: wire.node,
            canonical_neighbors,
            shape_route: None,
            turbo_route: None,
            query_facts: None,
            instant_reset: None,
            hop_root: None,
        });
        let node = &mut nodes[wire.node.index()];
        node.kind = CircuitNodeKind::Wire(plan);
        node.state = ComponentState::Wire(wire.state);
    }
    for component in &components {
        budget.check()?;
        if let NodeType::Observer { watched } = component.ty {
            let Block::Observer { observer } = component.block else {
                unreachable!();
            };
            let node = &mut nodes[component.node.index()];
            node.kind = CircuitNodeKind::Observer {
                watched: site(watched),
                facing: observer.facing.into(),
            };
        }
    }

    let mut contacts = Vec::with_capacity(geometry.contacts.len());
    let mut contact_guards = FxHashMap::default();
    for (index, contact) in geometry.contacts.iter().enumerate() {
        budget.check()?;
        let previous_guards = guard_table.len();
        let id = ContactId::new(index as u32);
        let variants = contact
            .variants
            .iter()
            .enumerate()
            .map(|(variant_index, variant)| {
                let when = variant_guard(variant.view, contact, id, geometry, &mut guard_table);
                contact_guards.insert((id, variant_index), when);
                let receiver = match variant.view {
                    GeometryView::PistonBase { piston, .. } | GeometryView::MovingPistonBase(piston) => Some(CallbackReceiver::PistonBase(piston)),
                    GeometryView::MovingPistonHead(_) => None,
                    GeometryView::PistonHead(piston) => Some(CallbackReceiver::PistonHead(piston)),
                    GeometryView::Fixed => component_at.get(&contact.position).copied().map(CallbackReceiver::Component),
                    GeometryView::Air | GeometryView::Resource(_) => None,
                };
                ContactVariant {
                    when,
                    profile: variant.profile,
                    receiver,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let added_guards = guard_table.len() - previous_guards;
        budget.consume(variants.len() + added_guards)?;
        contacts.push(ContactSpec {
            node: contact.node,
            position: contact.position,
            variants,
        });
    }

    let mut links = Vec::new();
    let mut wire_relations = Vec::new();
    let fixed_table_bytes = electrical_fixed_table_bytes(&nodes, &profiles, &sites, &contacts, &wire_plans).saturating_add(retained_geometry_bytes);
    compile_wire_relations(&wires, &site, &mut guard_table, &mut wire_relations, &mut budget, fixed_table_bytes)?;
    for component in &components {
        budget.check()?;
        let mut roots = consumer_roots(component.block, component.position, world);
        if matches!(component.ty, NodeType::Wire) {
            roots = wire_power_roots(component.position);
        }
        for root in roots {
            compile_root(
                world,
                geometry,
                root,
                component.node,
                matches!(component.ty, NodeType::Wire),
                always,
                &site,
                &contact_at,
                &contact_guards,
                &mut guard_table,
                &mut links,
                &mut budget,
            )?;
            check_electrical_growth(&budget, &guard_table, links.capacity(), &wire_relations, fixed_table_bytes)?;
        }
    }

    let mut comparators = Vec::new();
    for component in &components {
        budget.check()?;
        let Block::RedstoneComparator { comparator: state } = component.block else {
            continue;
        };
        let rear_pos = component.position.offset(state.facing.block_face());
        let rear_block = block_at(world, rear_pos);
        let direct = if comparator::has_override(rear_block) {
            Some(comparator_override(rear_block, component.position, rear_pos, world, &component_at)?)
        } else {
            None
        };
        let far = if direct.is_none() {
            let far_pos = rear_pos.offset(state.facing.block_face());
            let far_block = block_at(world, far_pos);
            let can_be_solid = contact_at
                .get(&rear_pos)
                .is_some_and(|(_, contact)| contact.variants.iter().any(|variant| geometry.profiles[variant.profile.index()].solid))
                || (!contact_at.contains_key(&rear_pos) && rear_block.is_solid());
            if can_be_solid && comparator::has_override(far_block) {
                let override_value = comparator_override(far_block, component.position, far_pos, world, &component_at)?;
                let guard = if contact_at.contains_key(&rear_pos) {
                    push_guard(
                        &mut guard_table,
                        Guard::Site {
                            site: site(rear_pos),
                            view: OperandView::Live,
                            test: SiteTest::Solid,
                        },
                    )
                } else {
                    always
                };
                Some((site(far_pos), override_value, guard))
            } else {
                None
            }
        } else {
            None
        };
        budget.consume(1)?;
        comparators.push(ComparatorInputPlan {
            node: component.node,
            rear: site(rear_pos),
            direct_override: direct,
            far_override: far,
            far_threshold: 15,
        });
    }

    // Geometry piston ports intentionally remain independent; a QC link is only a sampled input.
    for piston in &geometry.pistons {
        budget.check()?;
        let normal = piston_power_roots(piston.position, piston.facing, false);
        let quasi = piston_power_roots(piston.position, piston.facing, true);
        for root in normal.into_iter().chain(quasi) {
            compile_root(
                world,
                geometry,
                root,
                piston.node,
                false,
                always,
                &site,
                &contact_at,
                &contact_guards,
                &mut guard_table,
                &mut links,
                &mut budget,
            )?;
        }
    }

    // Direct comparator overrides replace rear power and therefore have no Main power link.
    let overridden_inputs: FxHashSet<_> = comparators
        .iter()
        .filter(|plan| plan.direct_override.is_some())
        .map(|plan| plan.node)
        .collect();
    links.retain(|link| {
        let ElectricalTarget::Input { node, port: InputPort::Main } = link.to else {
            return true;
        };
        !overridden_inputs.contains(&node)
    });

    budget.check()?;
    Ok(ElectricalDraft {
        remaining_cells: budget.remaining,
        nodes,
        profiles,
        guards: guard_table,
        contacts,
        sites,
        wires: wire_plans,
        links,
        wire_relations,
        comparators,
    })
}

fn add_site_position(positions: &mut FxHashSet<BlockPos>, position: BlockPos, budget: &mut PreparationBudget<'_>) -> Result<(), AnalysisError> {
    if !positions.contains(&position) {
        budget.consume(1)?;
        positions.insert(position);
    }
    Ok(())
}

fn consumer_roots(block: Block, position: BlockPos, world: &impl World) -> Vec<Root> {
    let root = |face, port| Root {
        position: position.offset(face),
        face,
        port,
        condition: RootCondition::Always,
        wire_only: false,
        stored_wire: false,
        comparator_side: false,
    };
    match block {
        Block::RedstoneRepeater { repeater } => {
            let facing = repeater.facing;
            let mut rear = root(facing.block_face(), InputPort::Main);
            rear.stored_wire = true;
            let mut roots = vec![rear];
            for side in [facing.rotate(), facing.rotate_ccw()] {
                let source = position.offset(side.block_face());
                if crate::redstone::is_diode(block_at(world, source)) {
                    roots.push(root(side.block_face(), InputPort::Side));
                }
            }
            roots
        }
        Block::RedstoneComparator { comparator } => {
            let facing = comparator.facing;
            let mut roots = Vec::new();
            let rear = position.offset(facing.block_face());
            if !comparator::has_override(block_at(world, rear)) {
                let mut rear = root(facing.block_face(), InputPort::Main);
                rear.stored_wire = true;
                roots.push(rear);
            }
            for side in [facing.rotate(), facing.rotate_ccw()] {
                let mut side = root(side.block_face(), InputPort::Side);
                side.stored_wire = true;
                side.comparator_side = true;
                roots.push(side);
            }
            roots
        }
        Block::RedstoneTorch { .. } => vec![Root {
            position: position.offset(BlockFace::Bottom),
            face: BlockFace::Top,
            port: InputPort::Main,
            condition: RootCondition::Always,
            wire_only: false,
            stored_wire: false,
            comparator_side: false,
        }],
        Block::RedstoneWallTorch { facing, .. } => vec![Root {
            position: position.offset(facing.opposite().block_face()),
            face: facing.opposite().block_face(),
            port: InputPort::Main,
            condition: RootCondition::Always,
            wire_only: false,
            stored_wire: false,
            comparator_side: false,
        }],
        Block::RedstoneWire { .. } => wire_power_roots(position),
        block
            if matches!(
                block,
                Block::RedstoneLamp { .. } | Block::Hopper { .. } | Block::IronTrapdoor { .. } | Block::NoteBlock { .. }
            ) || block.is_command_block()
                || block.is_copper_bulb() =>
        {
            BlockFace::values().into_iter().map(|face| root(face, InputPort::Main)).collect()
        }
        _ => Vec::new(),
    }
}

fn comparator_override(
    block: Block,
    comparator: BlockPos,
    position: BlockPos,
    world: &impl World,
    component_at: &FxHashMap<BlockPos, NodeId>,
) -> Result<ComparatorOverride, ElectricalError> {
    if let Some(reason) = super::unsupported_component_reason(block) {
        return Err(ElectricalError::UnsupportedComponent {
            position,
            block: block.get_id(),
            reason,
        });
    }
    if block.is_copper_bulb() || block.is_command_block() {
        let Some(&node) = component_at.get(&position) else {
            return Err(ElectricalError::DynamicComparatorOverrideNotInventoried {
                comparator,
                source: position,
            });
        };
        Ok(ComparatorOverride::LiveComponentOutput(node))
    } else {
        Ok(ComparatorOverride::Literal(comparator::get_override(block, world, position)))
    }
}

fn wire_power_roots(position: BlockPos) -> Vec<Root> {
    let mut roots: Vec<_> = BlockFace::values()
        .into_iter()
        .map(|face| Root {
            position: position.offset(face),
            face,
            port: InputPort::Main,
            condition: RootCondition::Always,
            wire_only: false,
            stored_wire: false,
            comparator_side: false,
        })
        .collect();
    for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
        let neighbor = position.offset(face);
        roots.push(Root {
            position: neighbor.offset(BlockFace::Top),
            face,
            port: InputPort::Main,
            condition: RootCondition::WireAscend { source: position, neighbor },
            wire_only: true,
            stored_wire: false,
            comparator_side: false,
        });
        roots.push(Root {
            position: neighbor.offset(BlockFace::Bottom),
            face,
            port: InputPort::Main,
            condition: RootCondition::WireDescend { neighbor },
            wire_only: true,
            stored_wire: false,
            comparator_side: false,
        });
    }
    roots
}

fn piston_power_roots(position: BlockPos, facing: BlockFace, quasi: bool) -> Vec<Root> {
    if !quasi {
        return BlockFace::values()
            .into_iter()
            .filter(|&face| face != facing)
            .map(|face| Root {
                position: position.offset(face),
                face,
                port: InputPort::Main,
                condition: RootCondition::Always,
                wire_only: false,
                stored_wire: false,
                comparator_side: false,
            })
            .collect();
    }
    let above = position.offset(BlockFace::Top);
    std::iter::once(Root {
        position,
        face: BlockFace::Bottom,
        port: InputPort::Quasi,
        condition: RootCondition::Always,
        wire_only: false,
        stored_wire: false,
        comparator_side: false,
    })
    .chain(
        BlockFace::values()
            .into_iter()
            .filter(|&face| face != BlockFace::Bottom)
            .map(|face| Root {
                position: above.offset(face),
                face,
                port: InputPort::Quasi,
                condition: RootCondition::Always,
                wire_only: false,
                stored_wire: false,
                comparator_side: false,
            }),
    )
    .collect()
}

fn compile_root(
    world: &impl World,
    geometry: &GeometryDraft,
    root: Root,
    receiver: NodeId,
    is_wire_receiver: bool,
    always: GuardId,
    site_for: &impl Fn(BlockPos) -> SiteId,
    contact_at: &FxHashMap<BlockPos, (ContactId, &super::geometry::GeometryContact)>,
    contact_guards: &FxHashMap<(ContactId, usize), GuardId>,
    guards: &mut Vec<Guard>,
    links: &mut Vec<ElectricalLink>,
    budget: &mut PreparationBudget<'_>,
) -> Result<(), AnalysisError> {
    budget.check()?;
    let previous_links = links.len();
    let previous_guards = guards.len();
    let variants: Vec<_> = if let Some((contact_id, contact)) = contact_at.get(&root.position) {
        contact
            .variants
            .iter()
            .enumerate()
            .map(|(variant_index, variant)| {
                let guard = contact_guards[&(*contact_id, variant_index)];
                (Block::from_id(geometry.profiles[variant.profile.index()].raw_state), guard)
            })
            .collect()
    } else {
        vec![(block_at(world, root.position), always)]
    };
    let root_site = site_for(root.position);
    let route_guard = match root.condition {
        RootCondition::Always => always,
        RootCondition::WireAscend { source, neighbor } => {
            let above_solid = push_guard(
                guards,
                Guard::Site {
                    site: site_for(source.offset(BlockFace::Top)),
                    view: OperandView::Live,
                    test: SiteTest::Solid,
                },
            );
            let neighbor_transparent = push_guard(
                guards,
                Guard::Site {
                    site: site_for(neighbor),
                    view: OperandView::Live,
                    test: SiteTest::Transparent,
                },
            );
            let headroom = not_guard(guards, above_solid);
            let opaque_neighbor = not_guard(guards, neighbor_transparent);
            join_guards(guards, &[headroom, opaque_neighbor])
        }
        RootCondition::WireDescend { neighbor } => {
            let neighbor_solid = push_guard(
                guards,
                Guard::Site {
                    site: site_for(neighbor),
                    view: OperandView::Live,
                    test: SiteTest::Solid,
                },
            );
            not_guard(guards, neighbor_solid)
        }
    };
    for (block, branch_guard) in variants {
        let branch_guard = all_guard(guards, branch_guard, route_guard);
        if root.comparator_side && !is_comparator_side_source(block) {
            continue;
        }
        if root.wire_only {
            if matches!(block, Block::RedstoneWire { .. }) {
                links.push(ElectricalLink {
                    from: root_site,
                    from_port: PowerPort::Output,
                    to: ElectricalTarget::Input {
                        node: receiver,
                        port: root.port,
                    },
                    face: root.face,
                    attenuation: 1,
                    guard: branch_guard,
                });
            }
            continue;
        }
        if block.is_solid() {
            links.push(ElectricalLink {
                from: root_site,
                from_port: PowerPort::Conducted {
                    include_dust: !is_wire_receiver,
                },
                to: ElectricalTarget::Input {
                    node: receiver,
                    port: root.port,
                },
                face: root.face,
                attenuation: 0,
                guard: branch_guard,
            });
            let include_dust = !is_wire_receiver;
            for face in BlockFace::values() {
                let source_position = root.position.offset(face);
                let source_variants: Vec<_> = if let Some((contact_id, contact)) = contact_at.get(&source_position) {
                    contact
                        .variants
                        .iter()
                        .enumerate()
                        .map(|(variant_index, variant)| {
                            let source_guard = contact_guards[&(*contact_id, variant_index)];
                            (Block::from_id(geometry.profiles[variant.profile.index()].raw_state), source_guard)
                        })
                        .collect()
                } else {
                    vec![(block_at(world, source_position), always)]
                };
                for (source_block, source_guard) in source_variants {
                    let can_emit = (include_dust && matches!(source_block, Block::RedstoneWire { .. }))
                        || power::emits_strong_power(source_block, world, source_position, face, !is_wire_receiver);
                    if source_block != Block::Air && can_emit {
                        links.push(ElectricalLink {
                            from: site_for(source_position),
                            from_port: PowerPort::Strong(face),
                            to: ElectricalTarget::Conductor {
                                site: root_site,
                                include_dust,
                            },
                            face,
                            attenuation: 0,
                            guard: all_guard(guards, branch_guard, source_guard),
                        });
                    }
                }
            }
        } else if block != Block::Air
            && (matches!(block, Block::RedstoneWire { .. }) || power::emits_weak_power(block, world, root.position, root.face, !is_wire_receiver))
        {
            let wire_input = is_wire_receiver && matches!(block, Block::RedstoneWire { .. });
            let stored_wire = root.stored_wire && matches!(block, Block::RedstoneWire { .. });
            links.push(ElectricalLink {
                from: root_site,
                from_port: if stored_wire {
                    PowerPort::Stored
                } else if wire_input {
                    PowerPort::Output
                } else {
                    PowerPort::Weak(root.face)
                },
                to: ElectricalTarget::Input {
                    node: receiver,
                    port: root.port,
                },
                face: root.face,
                attenuation: u8::from(wire_input),
                guard: branch_guard,
            });
        }
    }
    let added_links = links.len() - previous_links;
    let added_guards = guards.len() - previous_guards;
    budget.consume(added_links + added_guards)
}

fn is_comparator_side_source(block: Block) -> bool {
    crate::redstone::is_diode(block) || matches!(block, Block::RedstoneBlock | Block::RedstoneWire { .. })
}

fn compile_wire_relations(
    wires: &[WireComponent],
    site_for: &impl Fn(BlockPos) -> SiteId,
    guards: &mut Vec<Guard>,
    relations: &mut Vec<WireRelation>,
    budget: &mut PreparationBudget<'_>,
    fixed_table_bytes: usize,
) -> Result<(), AnalysisError> {
    for (index, wire) in wires.iter().enumerate() {
        budget.check()?;
        let previous_guards = guards.len();
        let previous_relations = relations.len();
        let above = push_guard(
            guards,
            Guard::Site {
                site: site_for(wire.position.offset(BlockFace::Top)),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
        );
        let ascent_headroom = not_guard(guards, above);
        for direction in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
            let neighbor = wire.position.offset(direction);
            let neighbor_site = site_for(neighbor);
            let same_wire = push_guard(
                guards,
                Guard::Site {
                    site: neighbor_site,
                    view: OperandView::Live,
                    test: SiteTest::IsWire,
                },
            );
            relations.push(WireRelation {
                from: wire.node,
                to: neighbor_site,
                kind: WireRelationKind::Step,
                direction,
                guard: same_wire,
            });

            let neighbor_transparent = push_guard(
                guards,
                Guard::Site {
                    site: neighbor_site,
                    view: OperandView::Live,
                    test: SiteTest::Transparent,
                },
            );
            let neighbor_solid = push_guard(
                guards,
                Guard::Site {
                    site: neighbor_site,
                    view: OperandView::Live,
                    test: SiteTest::Solid,
                },
            );
            let up_site = site_for(neighbor.offset(BlockFace::Top));
            let up_wire = push_guard(
                guards,
                Guard::Site {
                    site: up_site,
                    view: OperandView::Live,
                    test: SiteTest::IsWire,
                },
            );
            let transparent = not_guard(guards, neighbor_transparent);
            let ascent = join_guards(guards, &[up_wire, ascent_headroom, transparent]);
            relations.push(WireRelation {
                from: wire.node,
                to: up_site,
                kind: WireRelationKind::Ascend,
                direction,
                guard: ascent,
            });

            let down_site = site_for(neighbor.offset(BlockFace::Bottom));
            let down_wire = push_guard(
                guards,
                Guard::Site {
                    site: down_site,
                    view: OperandView::Live,
                    test: SiteTest::IsWire,
                },
            );
            let descent_space = not_guard(guards, neighbor_solid);
            let descent = join_guards(guards, &[down_wire, descent_space]);
            relations.push(WireRelation {
                from: wire.node,
                to: down_site,
                kind: WireRelationKind::Descend,
                direction,
                guard: descent,
            });
        }
        let added_guards = guards.len() - previous_guards;
        let added_relations = relations.len() - previous_relations;
        budget.consume(added_guards + added_relations)?;
        if index % 256 == 0 {
            check_electrical_growth(budget, guards, 0, relations, fixed_table_bytes)?;
        }
    }
    Ok(())
}

fn electrical_fixed_table_bytes(
    nodes: &Vec<CircuitNodeDraft>,
    profiles: &Vec<Profile>,
    sites: &Vec<SiteSpec>,
    contacts: &Vec<ContactSpec>,
    wires: &Vec<WirePlan>,
) -> usize {
    fn bytes<T>(capacity: usize) -> usize {
        capacity.saturating_mul(std::mem::size_of::<T>())
    }
    bytes::<CircuitNodeDraft>(nodes.capacity())
        .saturating_add(bytes::<Profile>(profiles.capacity()))
        .saturating_add(bytes::<SiteSpec>(sites.capacity()))
        .saturating_add(bytes::<ContactSpec>(contacts.capacity()))
        .saturating_add(
            contacts
                .iter()
                .map(|contact| contact.variants.len().saturating_mul(std::mem::size_of::<ContactVariant>()))
                .sum::<usize>(),
        )
        .saturating_add(bytes::<WirePlan>(wires.capacity()))
}

fn check_electrical_growth(
    budget: &PreparationBudget<'_>,
    guards: &Vec<Guard>,
    link_capacity: usize,
    relations: &Vec<WireRelation>,
    fixed_table_bytes: usize,
) -> Result<(), AnalysisError> {
    let guard_bytes = guards
        .capacity()
        .saturating_mul(std::mem::size_of::<Guard>() + std::mem::size_of::<GuardId>() * 4);
    let link_bytes = link_capacity.saturating_mul(std::mem::size_of::<ElectricalLink>());
    let relation_bytes = relations.capacity().saturating_mul(std::mem::size_of::<WireRelation>());
    budget.check_estimated_bytes(
        "electrical",
        fixed_table_bytes
            .saturating_add(guard_bytes)
            .saturating_add(link_bytes)
            .saturating_add(relation_bytes),
    )
}

fn variant_guard(
    view: GeometryView,
    contact: &super::geometry::GeometryContact,
    contact_id: ContactId,
    _geometry: &GeometryDraft,
    guards: &mut Vec<Guard>,
) -> GuardId {
    match view {
        GeometryView::Fixed => push_guard(guards, Guard::Always),
        GeometryView::PistonBase { piston, extended } => stationary_base_guard(guards, piston, extended),
        GeometryView::PistonHead(piston) => {
            let present = push_guard(guards, Guard::State(StateTest::PistonHeadPresent(piston)));
            let available = [
                crate::redpiler::program_graph::PistonAvailability::Ready,
                crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent,
            ]
            .map(|is| push_guard(guards, Guard::State(StateTest::PistonAvailability { piston, is })));
            let available = any_guards(guards, &available);
            join_guards(guards, &[present, available])
        }
        GeometryView::MovingPistonHead(piston) => push_guard(
            guards,
            Guard::State(StateTest::PistonAvailability {
                piston,
                is: crate::redpiler::program_graph::PistonAvailability::HeadPending,
            }),
        ),
        GeometryView::MovingPistonBase(piston) => moving_base_guard(guards, piston),
        GeometryView::Resource(resource) => {
            let at = push_guard(
                guards,
                Guard::State(StateTest::ResourceAt {
                    resource,
                    contact: contact_id,
                }),
            );
            let enabled = push_guard(guards, Guard::State(StateTest::ResourceEnabled(resource)));
            join_guards(guards, &[at, enabled])
        }
        GeometryView::Air => {
            let mut absent = Vec::new();
            for variant in contact.variants.iter().filter(|v| v.view != GeometryView::Air) {
                let selected = match variant.view {
                    GeometryView::PistonBase { piston, extended } => stationary_base_guard(guards, piston, extended),
                    GeometryView::PistonHead(piston) => {
                        let present = push_guard(guards, Guard::State(StateTest::PistonHeadPresent(piston)));
                        let available = [
                            crate::redpiler::program_graph::PistonAvailability::Ready,
                            crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent,
                        ]
                        .map(|is| push_guard(guards, Guard::State(StateTest::PistonAvailability { piston, is })));
                        let available = any_guards(guards, &available);
                        join_guards(guards, &[present, available])
                    }
                    GeometryView::MovingPistonHead(piston) => push_guard(
                        guards,
                        Guard::State(StateTest::PistonAvailability {
                            piston,
                            is: crate::redpiler::program_graph::PistonAvailability::HeadPending,
                        }),
                    ),
                    GeometryView::Resource(resource) => {
                        let at = push_guard(
                            guards,
                            Guard::State(StateTest::ResourceAt {
                                resource,
                                contact: contact_id,
                            }),
                        );
                        let enabled = push_guard(guards, Guard::State(StateTest::ResourceEnabled(resource)));
                        join_guards(guards, &[at, enabled])
                    }
                    GeometryView::MovingPistonBase(piston) => moving_base_guard(guards, piston),
                    GeometryView::Fixed => push_guard(guards, Guard::Always),
                    GeometryView::Air => unreachable!(),
                };
                absent.push(not_guard(guards, selected));
            }
            join_guards(guards, &absent)
        }
    }
}

fn stationary_base_guard(guards: &mut Vec<Guard>, piston: crate::redpiler::program_graph::PistonId, extended: bool) -> GuardId {
    use crate::redpiler::program_graph::PistonAvailability::*;
    let pose = push_guard(guards, Guard::State(StateTest::PistonExtended(piston)));
    let pose = if extended { pose } else { not_guard(guards, pose) };
    let available =
        [Ready, HeadPending, PublishingHeadRemoval].map(|is| push_guard(guards, Guard::State(StateTest::PistonAvailability { piston, is })));
    let available = any_guards(guards, &available);
    join_guards(guards, &[pose, available])
}

fn moving_base_guard(guards: &mut Vec<Guard>, piston: crate::redpiler::program_graph::PistonId) -> GuardId {
    let availability = [
        crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent,
        crate::redpiler::program_graph::PistonAvailability::BasePending,
    ]
    .map(|is| push_guard(guards, Guard::State(StateTest::PistonAvailability { piston, is })));
    any_guards(guards, &availability)
}

fn push_guard(guards: &mut Vec<Guard>, guard: Guard) -> GuardId {
    let id = GuardId::new(guards.len() as u32);
    guards.push(guard);
    id
}

fn not_guard(guards: &mut Vec<Guard>, guard: GuardId) -> GuardId {
    push_guard(guards, Guard::Not(guard))
}

fn join_guards(guards: &mut Vec<Guard>, children: &[GuardId]) -> GuardId {
    match children {
        [] => push_guard(guards, Guard::Always),
        [only] => *only,
        _ => push_guard(guards, Guard::All(children.to_vec().into_boxed_slice())),
    }
}

fn any_guards(guards: &mut Vec<Guard>, children: &[GuardId]) -> GuardId {
    match children {
        [] => {
            let always = push_guard(guards, Guard::Always);
            not_guard(guards, always)
        }
        [only] => *only,
        _ => push_guard(guards, Guard::Any(children.to_vec().into_boxed_slice())),
    }
}

fn all_guard(guards: &mut Vec<Guard>, left: GuardId, right: GuardId) -> GuardId {
    if left == right {
        return left;
    }
    if matches!(guards[left.index()], Guard::Always) {
        return right;
    }
    if matches!(guards[right.index()], Guard::Always) {
        return left;
    }
    join_guards(guards, &[left, right])
}

fn is_component(block: Block) -> bool {
    matches!(
        block,
        Block::RedstoneRepeater { .. }
            | Block::RedstoneComparator { .. }
            | Block::RedstoneTorch { .. }
            | Block::RedstoneWallTorch { .. }
            | Block::Observer { .. }
            | Block::RedstoneWire { .. }
            | Block::StoneButton { .. }
            | Block::RedstoneLamp { .. }
            | Block::Lever { .. }
            | Block::StonePressurePlate { .. }
            | Block::IronTrapdoor { .. }
            | Block::NoteBlock { .. }
            | Block::RedstoneBlock
    ) || block.pressure_plate_powered().is_some()
        || block.is_command_block()
        || block.is_copper_bulb()
        || comparator::has_override(block)
}

fn block_at(world: &impl World, position: BlockPos) -> Block {
    if (0..PLOT_BLOCK_HEIGHT).contains(&position.y) {
        world.get_block(position)
    } else {
        Block::Air
    }
}

pub(crate) fn attenuate_power(strength: u8, attenuation: u8) -> u8 {
    strength.saturating_sub(attenuation)
}

#[cfg(test)]
mod memory_tests {
    use super::all_guard;
    use crate::redpiler::program_graph::{Guard, GuardId, OperandView, SiteId, SiteTest};

    #[test]
    fn all_guard_reuses_identity_terms() {
        let always = GuardId::new(0);
        let site_guard = GuardId::new(1);
        let mut guards = vec![
            Guard::Always,
            Guard::Site {
                site: SiteId::new(4),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
        ];

        assert_eq!(all_guard(&mut guards, always, always), always);
        assert_eq!(all_guard(&mut guards, site_guard, always), site_guard);
        assert_eq!(all_guard(&mut guards, always, site_guard), site_guard);
        assert_eq!(all_guard(&mut guards, site_guard, site_guard), site_guard);
        assert_eq!(guards.len(), 2);
    }
}
