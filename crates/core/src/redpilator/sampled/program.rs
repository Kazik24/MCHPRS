use super::{
    ScratchError,
    movement::{ArrangementFlip, MoveStep, MovementGroup, MovementTransition},
};
use mchprs_blocks::{BlockPos, blocks::Block};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::num::NonZeroUsize;

// Notification IDs retain the physical head route separately from a direct base callback.
pub(super) const PISTON_HEAD_NOTIFICATION: usize = 1 << 31;

pub(super) fn notification_component(notification: usize) -> usize {
    notification & !PISTON_HEAD_NOTIFICATION
}

pub(super) fn valid_notification(initial: &[Block], notification: usize) -> bool {
    let component = notification_component(notification);
    component < initial.len() && (notification & PISTON_HEAD_NOTIFICATION == 0 || matches!(initial[component], Block::Piston { .. }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Contribution(usize),
    ArrangementContribution(usize),
    RefreshWire(usize),
    CheckWatchedWire(usize),
    UpdateLamp(usize),
    UpdateHopper(usize),
    UpdateTrapdoor(usize),
    UpdateCommandBlock(usize),
    UpdateNoteBlock(usize),
    UpdateCopperBulb(usize),
    UpdateRepeater(usize),
    UpdatePiston(usize),
    UpdateTorch(usize),
    UpdateComparator(usize),
}

#[derive(Clone, Copy)]
pub(super) struct ElectricalLink {
    /// component index whose redstone output feeds `target`.
    pub(super) source: usize,
    /// component index receiving the attenuated signal.
    pub(super) target: usize,
    /// signal strength subtracted along this connection, in levels 0–15.
    pub(super) attenuation: u8,
}

#[derive(Clone)]
pub(super) struct WatchedWire {
    /// index into `Program::positions` for the wire being monitored.
    pub(super) wire: usize,
    /// component notification IDs, with bit 31 marking forwarding through a live piston head.
    pub(super) watchers: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct RepeaterRecord {
    /// component index into `Program::positions` for this repeater.
    pub(super) component: usize,
    /// indicating whether the repeater output is connected to a diode.
    pub(super) output_is_diode: bool,
    /// component indices supplying repeater side-lock power; `None` means not populated.
    pub(super) side_sources: Option<Vec<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(super) enum ArrangementSource {
    Component(usize),
    Token(usize),
}

#[derive(Clone)]
pub(super) struct InputArrangement {
    /// component index into `Program::positions` whose input is described.
    pub(super) target: usize,
    /// indicating this arrangement describes a comparator side input rather than rear input.
    pub(super) side: bool,
    /// indicating the input change is silent and should not trigger an ordinary notification.
    pub(super) silent: bool,
    /// token indices into `Program::tokens` whose positions distinguish arrangement states.
    pub(super) tokens: Vec<usize>,
    /// Physical token digits mapped to equivalent electrical states; empty means identity.
    pub(super) token_states: Vec<Vec<usize>>,
    /// conditional sources gating the arrangement's input.
    pub(super) conditional_sources: Vec<ArrangementSource>,
    /// per-arrangement source/attenuation lists; outer slots follow the encoded token-position combinations.
    pub(super) inputs: Vec<Vec<(ArrangementSource, u8)>>,
}

impl InputArrangement {
    pub(super) fn digit(&self, axis: usize, physical: usize) -> usize {
        self.token_states.get(axis).map_or(physical, |states| states[physical])
    }

    pub(super) fn radix(&self, axis: usize, physical: usize) -> usize {
        self.token_states
            .get(axis)
            .map_or(physical, |states| states.iter().copied().max().unwrap_or(0) + 1)
    }
}

#[derive(Clone)]
pub(super) struct ArrangementLink {
    /// component index into `Program::positions` receiving this conditional signal.
    pub(super) target: usize,
    /// indicating this is a side input rather than the primary input.
    pub(super) side: bool,
    /// indicating changes to this link do not independently notify the target.
    pub(super) silent: bool,
    /// component or movable token supplying the signal.
    pub(super) source: ArrangementSource,
    /// signal attenuation for this source-to-target connection.
    pub(super) attenuation: u8,
    /// index into `Program::input_arrangements` selecting this link's arrangement group.
    pub(super) group: usize,
    /// active-state lookup indexed by the group's encoded arrangement state.
    pub(super) active: Vec<bool>,
}

#[derive(Clone)]
pub(super) struct ComparatorRecord {
    /// component index into `Program::positions` for this comparator.
    pub(super) component: usize,
    /// comparator output strength captured from its block entity, in levels 0–15.
    pub(super) stored: u8,
    /// static or dynamic rear override, when present.
    pub(super) rear_override: Option<ComparatorInput>,
    /// far-side override through a solid rear block, when present.
    pub(super) far_override: Option<ComparatorInput>,
    /// indicating the output faces a diode in a direction that changes output behavior.
    pub(super) output_is_diode: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComparatorInput {
    Static(u8),
    CopperBulb(usize),
    CommandBlock(usize),
}

#[derive(Clone)]
pub(super) struct ReactiveWire {
    /// watched-wire index into `Program::watched_wires`.
    pub(super) wire: usize,
    /// source component indices and attenuation levels contributing to this wire.
    pub(super) sources: Vec<(usize, u8)>,
    /// reactive component indices in `Program::positions` whose behavior depends on this wire.
    pub(super) recipients: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct PistonBinding {
    /// piston component index into `Program::positions`.
    pub(super) component: usize,
    /// projection-cell index for the piston base/home position.
    pub(super) home_cell: usize,
    /// projection-cell index for the piston head/far position.
    pub(super) far_cell: usize,
    /// index into the related piston binding, when paired with another piston.
    pub(super) mate: Option<usize>,
    /// projection-cell indices containing heads at the home position.
    pub(super) home_heads: Vec<usize>,
    /// projection-cell indices containing heads at the far position.
    pub(super) far_heads: Vec<usize>,
    /// indicating the home side cannot accept a piston head.
    pub(super) home_blocked: bool,
    /// indicating the far side cannot accept a piston head.
    pub(super) far_blocked: bool,
}

#[derive(Clone)]
pub(crate) struct Program {
    /// Runtime semantics; incompatible relaxed modes cannot share a prepared program.
    pub(crate) relaxed_mode: bool,
    pub(crate) movement_folding: bool,
    /// Position-keyed sampling artifacts survive topology changes between compilations.
    pub(crate) source_order_cache: Option<std::sync::Arc<super::SourceOrderCache>>,
    pub(super) routing: Option<std::sync::Arc<super::routing::WireGraph>>,
    // Components and output instructions
    /// component-indexed world positions; index is the canonical component ID throughout the program.
    pub(super) positions: Vec<BlockPos>,
    /// world-position to component index lookup matching `positions`.
    pub(super) component_indices: HashMap<BlockPos, usize>,
    /// initial component blocks, parallel to `positions` by component index.
    pub(super) initial: Vec<Block>,
    /// component indices of external lever, button and pressure-plate inputs.
    pub(super) sources: Vec<usize>,
    /// per component, slicing `operations` for its two output transitions.
    pub(super) output_programs: Vec<[std::ops::Range<usize>; 2]>,
    /// flattened compiled output operations referenced by `output_programs` ranges.
    pub(super) operations: Vec<Operation>,

    // Electrical connections
    /// ordinary source-to-target input links; IDs index this vector.
    pub(super) links: Vec<ElectricalLink>,
    /// per target component, listing incoming indices into `links`.
    pub(super) incoming: Vec<SmallVec<[usize; 2]>>,
    /// per source component, listing outgoing indices into `links`.
    pub(super) outgoing: Vec<SmallVec<[usize; 2]>>,
    /// indexed by ordinary link ID, indicating whether that link contributes to its target.
    pub(super) effective_links: Vec<bool>,
    /// comparator/repeater side-input connections, separate from ordinary incoming links.
    pub(super) side_links: Vec<ElectricalLink>,
    /// per source component, with target component and attenuation for side outputs.
    pub(super) side_outputs: Vec<Vec<(usize, u8)>>,

    // Repeaters, comparators, and watched wires
    /// per-repeater output and side-lock metadata.
    pub(super) repeaters: Vec<RepeaterRecord>,
    /// reverse map from component index to `repeaters` record index plus one.
    pub(super) repeater_by_component: Vec<Option<NonZeroUsize>>,
    /// per-comparator captured output and override metadata.
    pub(super) comparators: Vec<ComparatorRecord>,
    /// reverse map from component index to `comparators` record index plus one.
    pub(super) comparator_by_component: Vec<Option<NonZeroUsize>>,
    /// per-component lists of comparator indices depending on that component's override value.
    pub(super) comparators_by_override: Vec<Vec<usize>>,
    /// records mapping monitored wire components to their watcher components.
    pub(super) watched_wires: Vec<WatchedWire>,
    /// reverse map from component index to `watched_wires` record index plus one.
    pub(super) watched_by_component: Vec<Option<NonZeroUsize>>,

    // Reactive dependencies
    /// cached summary indicating at least one component has reactive behavior.
    pub(super) has_reactive: bool,
    /// component-indexed flags identifying nodes that require reactive reevaluation.
    pub(super) reactive: Vec<bool>,
    /// watched wires and their sources/recipient components.
    pub(super) reactive_wires: Vec<ReactiveWire>,
    /// per-source component lists of reactive-wire indices to refresh when that source changes.
    pub(super) reactive_by_source: Vec<Vec<usize>>,
    /// per-component lists of neighboring reactive component indices.
    pub(super) reactive_neighbours: Vec<Vec<usize>>,

    // Pistons, tokens, and movement notifications
    /// discovered piston bases, orientations, and group identities.
    pub(super) pistons: Vec<super::pistons::PistonActor>,
    /// reverse map from component index to `piston_bindings` index plus one.
    pub(super) piston_by_component: Vec<Option<NonZeroUsize>>,
    /// piston projection bindings and their home/far cell state.
    pub(super) piston_bindings: Vec<PistonBinding>,
    /// movable blocks and their legal positions/cells.
    pub(super) tokens: Vec<super::pistons::PistonToken>,
    /// component-indexed flags for nodes that can source movable-token inputs.
    pub(super) movable_sources: Vec<bool>,
    /// per token, piston component indices whose motion can move that token.
    pub(super) token_piston_components: Vec<SmallVec<[usize; 2]>>,
    /// indexed token, token cell, then source component/attenuation dependencies.
    pub(super) token_sources: Vec<Vec<Vec<(usize, u8)>>>,
    /// indexed token and cell, with separate ordered notification IDs for departure and arrival.
    pub(super) token_updates: Vec<Vec<[Vec<usize>; 2]>>,
    /// indexed token and cell, listing notification IDs watching movement at that cell.
    pub(super) token_watchers: Vec<Vec<Vec<usize>>>,
    /// per source component, listing notification IDs reached by source changes.
    pub(super) source_watchers: Vec<SmallVec<[usize; 2]>>,
    /// per piston component, listing notification IDs reached around its home cell.
    pub(super) home_watchers: Vec<SmallVec<[usize; 2]>>,

    // Arrangements and movement instructions
    /// joint token-position states that change component input connectivity.
    pub(super) input_arrangements: Vec<InputArrangement>,
    /// per target component, listing its token-dependent input-arrangement indices.
    pub(super) input_groups_by_target: Vec<SmallVec<[usize; 2]>>,
    /// token-dependent electrical links and their active arrangement states.
    pub(super) arrangement_links: Vec<ArrangementLink>,
    /// per source component, listing outgoing indices into `arrangement_links`.
    pub(super) outgoing_arrangements: Vec<Vec<usize>>,
    /// grouped token movement axes and their callback ranges.
    pub(super) movement_groups: Vec<MovementGroup>,
    /// per token, mapping movement-group indices to that token's group-axis index.
    pub(super) token_movement_groups: Vec<Vec<(usize, usize)>>,
    /// ordered conditional-link enter/leave events for token movement.
    pub(super) movement_flips: Vec<ArrangementFlip>,
    /// indexed token and cell, slicing `movement_steps` for departure and arrival.
    pub(super) move_steps: Vec<Vec<[std::ops::Range<usize>; 2]>>,
    /// flattened ordered movement callbacks referenced by `move_steps` ranges.
    pub(super) movement_steps: Vec<MoveStep>,
    /// flattened watched-wire indices referenced by `MoveStep::wires` ranges.
    pub(super) movement_wires: Vec<usize>,
    pub(super) movement_cells: Vec<Vec<std::ops::Range<usize>>>,
    pub(super) movement_transitions: Vec<MovementTransition>,
    pub(super) movement_ranges: Vec<std::ops::Range<usize>>,
    /// Per token/cell, ordered copies of proven-fixed departure/arrival flips in `movement_flips`.
    pub(super) movement_fixed_ranges: Vec<Vec<Option<[std::ops::Range<usize>; 2]>>>,

    // Geometry and projection indexes
    pub(super) shape_programs: HashMap<BlockPos, std::ops::Range<usize>>,
    pub(super) shape_steps: Vec<super::shapes::ShapeStep>,
    pub(super) shape_maps: Vec<[u8; 256]>,
    pub(super) cell_shape_steps: Vec<Option<std::ops::Range<usize>>>,
    /// captured non-air blocks outside the component list.
    pub(super) geometry: Vec<(BlockPos, Block)>,
    /// world-position to captured scenery block lookup matching `geometry`.
    pub(super) geometry_indices: HashMap<BlockPos, Block>,
    /// non-component scenery positions reserved for piston projection.
    pub(super) piston_scenery: Vec<BlockPos>,
    /// world positions represented by projection-cell indices.
    pub(super) projection_positions: Vec<BlockPos>,
    /// world-position to projection-cell index lookup.
    pub(super) projection_indices: HashMap<BlockPos, usize>,
    /// component-indexed projection-cell IDs, parallel to `positions`.
    pub(super) component_cells: Vec<usize>,
    /// projection-cell-indexed reverse mapping to a component index plus one.
    pub(super) cell_components: Vec<Option<NonZeroUsize>>,
    /// per projection cell, token indices occupying that cell in any legal position.
    pub(super) token_cells: Vec<Vec<usize>>,
    /// per token, projection-cell indices it can occupy.
    pub(super) cell_tokens: Vec<SmallVec<[usize; 2]>>,
    /// per projection cell, piston indices whose base or home occupies that cell.
    pub(super) cell_pistons: Vec<SmallVec<[usize; 2]>>,
    /// per projection cell, observer component indices watching that cell.
    pub(super) cell_observers: Vec<SmallVec<[usize; 2]>>,
    /// projection-cell-indexed scenery blocks, with air for cells without static geometry.
    pub(super) cell_geometry: Vec<Block>,

    // Captured command state
    /// captured command-block state keyed by world position.
    pub(super) command_blocks: HashMap<BlockPos, Box<mchprs_blocks::block_entities::CommandBlockEntity>>,
    /// world-level command output state needed to restore interpreter behavior.
    pub(super) command_output_state: Option<crate::world::CommandBlockOutputState>,
}

/// Validate one output instruction before runtime execution.
///
/// Inputs: The operation, initial component blocks and counts of available link/watcher/arrangement records.
///
/// Match the opcode, check referenced indexes, and require receiver instructions to point at the
/// corresponding supported block kind.
fn validate_operation(
    operation: Operation,
    initial: &[Block],
    links: usize,
    watched_wires: usize,
    arrangements: Option<usize>,
) -> Result<(), ScratchError> {
    let invalid = |reason| ScratchError { position: None, reason };
    match operation {
        Operation::ArrangementContribution(link) if arrangements.is_some_and(|count| link >= count) => {
            return Err(invalid("sampled arrangement contribution link is out of bounds"));
        }
        Operation::Contribution(link) if link >= links => {
            return Err(invalid("sampled program contribution link is out of bounds"));
        }
        Operation::CheckWatchedWire(record) if record >= watched_wires => {
            return Err(invalid("sampled program watched-wire index is out of bounds"));
        }
        Operation::RefreshWire(slot) if !matches!(initial.get(slot), Some(Block::RedstoneWire { .. })) => {
            return Err(invalid("sampled program RefreshWire target is not a wire"));
        }
        Operation::UpdateLamp(slot) if !matches!(initial.get(slot), Some(Block::RedstoneLamp { .. })) => {
            return Err(invalid("sampled program UpdateLamp target is not a lamp"));
        }
        Operation::UpdateHopper(slot) if !matches!(initial.get(slot), Some(Block::Hopper { .. })) => {
            return Err(invalid("sampled program UpdateHopper target is not a hopper"));
        }
        Operation::UpdateTrapdoor(slot) if !matches!(initial.get(slot), Some(Block::IronTrapdoor { .. })) => {
            return Err(invalid("sampled program UpdateTrapdoor target is not an iron trapdoor"));
        }
        Operation::UpdateCommandBlock(slot) if !initial.get(slot).is_some_and(|block| block.is_command_block()) => {
            return Err(invalid("sampled program UpdateCommandBlock target is not a command block"));
        }
        Operation::UpdateNoteBlock(slot) if !matches!(initial.get(slot), Some(Block::NoteBlock { .. })) => {
            return Err(invalid("sampled program UpdateNoteBlock target is not a note block"));
        }
        Operation::UpdateCopperBulb(slot) if !initial.get(slot).is_some_and(|block| block.is_copper_bulb()) => {
            return Err(invalid("sampled program UpdateCopperBulb target is not a copper bulb"));
        }
        Operation::UpdateRepeater(slot) if !matches!(initial.get(slot), Some(Block::RedstoneRepeater { .. })) => {
            return Err(invalid("sampled program UpdateRepeater target is not a repeater"));
        }
        Operation::UpdatePiston(slot) if !matches!(initial.get(slot), Some(Block::Piston { .. })) => {
            return Err(invalid("sampled program UpdatePiston target is not a piston"));
        }
        Operation::UpdateComparator(slot) if !matches!(initial.get(slot), Some(Block::RedstoneComparator { .. })) => {
            return Err(invalid("sampled UpdateComparator target is not a comparator"));
        }
        Operation::UpdateTorch(slot) if !matches!(initial.get(slot), Some(Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. })) => {
            return Err(invalid("sampled program UpdateTorch target is not a torch"));
        }
        _ => {}
    }
    Ok(())
}

fn validate_histogram_bound(blocks: &[Block]) -> Result<(), ScratchError> {
    if blocks
        .iter()
        .try_fold(0u32, |count, block| {
            count.checked_add(match block {
                Block::RedstoneComparator { .. } => 2,
                Block::RedstoneWire { .. } => 1,
                _ => 0,
            })
        })
        .is_none()
    {
        return Err(ScratchError {
            position: None,
            reason: "compiled analog channel capacity exceeded",
        });
    }
    Ok(())
}

impl Program {
    pub(in crate::redpilator) fn component_count(&self) -> usize {
        self.positions.len()
    }

    pub(super) fn validate_input_bound(&self) -> Result<(), ScratchError> {
        let count = self
            .links
            .len()
            .checked_add(self.arrangement_links.len())
            .and_then(|n| n.checked_add(self.side_links.len()));
        let mut count = count;
        for record in &self.repeaters {
            count = count.and_then(|n| n.checked_add(record.side_sources.as_ref().map_or(0, Vec::len)));
        }
        if count.is_none_or(|count| u32::try_from(count).is_err()) {
            return Err(ScratchError {
                position: None,
                reason: "compiled input fan-in exceeds counter capacity",
            });
        }
        Ok(())
    }

    /// Build indexes connecting program components to visible world cells.
    ///
    /// Inputs: This program's components, static geometry, movable tokens, pistons and electrical records.
    ///
    /// Compact unused instructions, deduplicate projected positions, index cell occupants/watchers and
    /// diode records, and exclude fixed links shadowed by conditional arrangements.
    pub(super) fn prepare_projection(&mut self) {
        let used: usize = self.output_programs.iter().flatten().map(|range| range.len()).sum();
        if used < self.operations.len() {
            self.compact_output_programs(false);
        }
        self.geometry_indices = self.geometry.iter().copied().collect();
        let mut positions = self.positions.clone();
        positions.extend(self.tokens.iter().flat_map(|token| token.cells.iter().copied()));
        positions.extend(self.pistons.iter().flat_map(|piston| [piston.base, piston.home, piston.far]));
        positions.sort_by_key(|position| (position.x, position.y, position.z));
        positions.dedup();
        self.projection_indices = positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        self.projection_positions = positions;
        let cells = self.projection_positions.len();
        self.cell_shape_steps = self
            .projection_positions
            .iter()
            .map(|pos| self.shape_programs.get(pos).cloned())
            .collect();
        self.component_cells = self.positions.iter().map(|pos| self.projection_indices[pos]).collect();
        self.cell_components = vec![None; cells];
        for (component, &cell) in self.component_cells.iter().enumerate() {
            self.cell_components[cell] = NonZeroUsize::new(component + 1);
        }
        self.cell_geometry = self
            .projection_positions
            .iter()
            .map(|pos| self.geometry_indices.get(pos).copied().unwrap_or(Block::Air))
            .collect();
        self.token_cells = self
            .tokens
            .iter()
            .map(|token| token.cells.iter().map(|pos| self.projection_indices[pos]).collect())
            .collect();
        self.cell_tokens = vec![SmallVec::new(); cells];
        for (token, cells) in self.token_cells.iter().enumerate() {
            for &cell in cells {
                self.cell_tokens[cell].push(token);
            }
        }
        self.cell_pistons = vec![SmallVec::new(); cells];
        for (piston, record) in self.pistons.iter().enumerate() {
            for pos in [record.base, record.home] {
                self.cell_pistons[self.projection_indices[&pos]].push(piston);
            }
        }
        self.cell_observers = vec![SmallVec::new(); cells];
        for (component, block) in self.initial.iter().enumerate() {
            if let Block::Observer { observer } = block
                && let Some(&cell) = self.projection_indices.get(&self.positions[component].offset(observer.facing.into()))
            {
                self.cell_observers[cell].push(component);
            }
        }
        self.piston_by_component = vec![None; self.initial.len()];
        self.piston_bindings = self
            .pistons
            .iter()
            .enumerate()
            .map(|(index, piston)| {
                let component = self.component_indices[&piston.base];
                self.piston_by_component[component] = NonZeroUsize::new(index + 1);
                let home_cell = self.projection_indices[&piston.home];
                let far_cell = self.projection_indices[&piston.far];
                let heads = |cell: usize| {
                    self.cell_pistons[cell]
                        .iter()
                        .copied()
                        .filter(|&other| other != index && self.pistons[other].home == self.projection_positions[cell])
                        .collect()
                };
                PistonBinding {
                    component,
                    home_cell,
                    far_cell,
                    mate: self
                        .pistons
                        .iter()
                        .position(|other| other.group == piston.group && other.far == piston.far && other.facing == piston.facing.opposite()),
                    home_heads: heads(home_cell),
                    far_heads: heads(far_cell),
                    home_blocked: self.piston_scenery.contains(&piston.home),
                    far_blocked: self.piston_scenery.contains(&piston.far),
                }
            })
            .collect();
        self.token_piston_components = self
            .token_cells
            .iter()
            .map(|cells| {
                let mut components = SmallVec::new();
                for &cell in cells {
                    for &piston in &self.cell_pistons[cell] {
                        if self.piston_bindings[piston].home_cell == cell {
                            let component = self.piston_bindings[piston].component;
                            if !components.contains(&component) {
                                components.push(component);
                            }
                        }
                    }
                }
                components
            })
            .collect();
        self.repeater_by_component = vec![None; self.initial.len()];
        for (index, record) in self.repeaters.iter().enumerate() {
            self.repeater_by_component[record.component] = NonZeroUsize::new(index + 1);
        }
        self.comparator_by_component = vec![None; self.initial.len()];
        self.comparators_by_override = vec![vec![]; self.initial.len()];
        for (index, record) in self.comparators.iter().enumerate() {
            self.comparator_by_component[record.component] = NonZeroUsize::new(index + 1);
            for source in [record.rear_override, record.far_override]
                .into_iter()
                .flatten()
                .filter_map(|input| match input {
                    ComparatorInput::CopperBulb(source) | ComparatorInput::CommandBlock(source) => Some(source),
                    ComparatorInput::Static(_) => None,
                })
            {
                if !self.comparators_by_override[source].contains(&record.component) {
                    self.comparators_by_override[source].push(record.component);
                }
            }
        }
        self.watched_by_component = vec![None; self.initial.len()];
        for (index, record) in self.watched_wires.iter().enumerate() {
            self.watched_by_component[record.wire].get_or_insert(NonZeroUsize::new(index + 1).unwrap());
        }
        self.movable_sources = vec![false; self.initial.len()];
        for token in &self.tokens {
            if matches!(token.block, Block::RedstoneBlock)
                && let Some(component) = token.position.and_then(|pos| self.component_indices.get(&pos))
            {
                self.movable_sources[*component] = true;
            }
        }
        self.has_reactive = self.reactive.iter().any(|&flag| flag);
        let shadowed = |target: usize, source, side| {
            self.input_groups_by_target[target].iter().any(|&group| {
                let group = &self.input_arrangements[group];
                group.side == side && group.conditional_sources.contains(&ArrangementSource::Component(source))
            })
        };
        self.effective_links = self.links.iter().map(|link| !shadowed(link.target, link.source, false)).collect();
        self.side_outputs = vec![vec![]; self.initial.len()];
        let mut sides = self.side_links.clone();
        sides.extend(self.repeaters.iter().flat_map(|record| {
            record.side_sources.iter().flatten().map(move |&source| ElectricalLink {
                source,
                target: record.component,
                attenuation: 0,
            })
        }));
        for link in sides {
            if shadowed(link.target, link.source, true) {
                continue;
            }
            let edges = &mut self.side_outputs[link.source];
            if let Some((_, attenuation)) = edges.iter_mut().find(|(target, _)| *target == link.target) {
                *attenuation = (*attenuation).min(link.attenuation);
            } else {
                edges.push((link.target, link.attenuation));
            }
        }
    }

    /// Remove unused instruction storage and optionally share identical edge programs.
    ///
    /// Inputs: This program's operation buffer, output ranges and the share_edges flag.
    ///
    /// Optionally alias equal rise/fall ranges, merge live intervals, copy only those intervals and
    /// translate every output range to its new offset.
    pub(super) fn compact_output_programs(&mut self, share_edges: bool) {
        let old = std::mem::take(&mut self.operations);
        for pair in &mut self.output_programs {
            if share_edges && old[pair[0].clone()] == old[pair[1].clone()] {
                pair[1] = pair[0].clone();
            }
        }
        let mut ranges: Vec<_> = self.output_programs.iter().flatten().filter(|range| !range.is_empty()).cloned().collect();
        ranges.sort_unstable_by_key(|range| range.start);
        let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
        for range in ranges {
            if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        let mut starts = Vec::with_capacity(merged.len());
        for range in &merged {
            starts.push(self.operations.len());
            self.operations.extend_from_slice(&old[range.clone()]);
        }
        for range in self.output_programs.iter_mut().flatten() {
            if range.start == range.end {
                *range = 0..0;
            } else {
                let index = merged.partition_point(|part| part.start <= range.start) - 1;
                let start = starts[index] + range.start - merged[index].start;
                *range = start..start + range.len();
            }
        }
    }

    /// Construct and validate the initial immutable circuit model.
    ///
    /// Inputs: Parallel position/block arrays, input IDs, output programs, electrical links, watched wires and
    /// repeater records.
    ///
    /// Check dimensions, identities and operation types, flatten output instructions, then initialize
    /// dependency indexes and optional runtime tables with consistent sizes.
    pub(super) fn new(
        positions: Vec<BlockPos>,
        initial: Vec<Block>,
        sources: Vec<usize>,
        output_programs: Vec<[Vec<Operation>; 2]>,
        links: Vec<ElectricalLink>,
        mut watched_wires: Vec<WatchedWire>,
        repeaters: Vec<RepeaterRecord>,
    ) -> Result<Self, ScratchError> {
        let invalid = |reason| ScratchError { position: None, reason };
        validate_histogram_bound(&initial)?;
        if initial.len() >= PISTON_HEAD_NOTIFICATION {
            return Err(invalid("sampled component IDs exceed notification range"));
        }
        if positions.len() != initial.len() {
            return Err(invalid("sampled program position/initial length mismatch"));
        }
        if output_programs.len() != initial.len() {
            return Err(invalid("sampled output program count mismatch"));
        }
        let mut unique_positions = std::collections::HashSet::new();
        if positions.iter().any(|position| !unique_positions.insert(*position)) {
            return Err(invalid("sampled program has duplicate component positions"));
        }
        for block in &initial {
            match block {
                Block::Lever { .. }
                | Block::StoneButton { .. }
                | Block::RedstoneLamp { .. }
                | Block::Hopper { .. }
                | Block::IronTrapdoor { .. }
                | Block::NoteBlock { .. }
                | Block::RedstoneComparator { .. }
                | Block::RedstoneBlock
                | Block::Piston { .. }
                | Block::Observer { .. }
                | Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. } => {}
                block if block.is_command_block() || block.is_copper_bulb() || block.pressure_plate_powered().is_some() => {}
                Block::RedstoneWire { wire } if wire.power <= 15 => {}
                Block::RedstoneWire { .. } => return Err(invalid("sampled program wire power exceeds 15")),
                Block::RedstoneRepeater { repeater } if (1..=4).contains(&repeater.delay) => {}
                Block::RedstoneRepeater { .. } => return Err(invalid("sampled repeater delay is outside 1..=4")),
                _ => return Err(invalid("sampled program contains unsupported component")),
            }
        }
        let mut unique_sources = std::collections::HashSet::new();
        for &source in &sources {
            let Some(block) = initial.get(source) else {
                return Err(invalid("sampled program source index is out of bounds"));
            };
            if !unique_sources.insert(source) {
                return Err(invalid("sampled program source is duplicated"));
            }
            if !matches!(block, Block::Lever { .. } | Block::StoneButton { .. }) && block.pressure_plate_powered().is_none() {
                return Err(invalid("sampled program source is not a lever, button or pressure plate"));
            }
        }
        let mut repeater_components = std::collections::HashSet::new();
        for record in &repeaters {
            if !matches!(initial.get(record.component), Some(Block::RedstoneRepeater { .. })) {
                return Err(invalid("sampled repeater record index is not a repeater"));
            }
            if !repeater_components.insert(record.component) {
                return Err(invalid("sampled repeater record is duplicated"));
            }
            if record.side_sources.iter().flatten().any(|&source| {
                !matches!(
                    initial.get(source),
                    Some(Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. })
                )
            }) {
                return Err(invalid("sampled repeater side source is not a repeater"));
            }
        }
        for (index, block) in initial.iter().enumerate() {
            if matches!(block, Block::RedstoneRepeater { .. }) && !repeater_components.contains(&index) {
                return Err(invalid("sampled repeater has no repeater record"));
            }
        }
        for link in &links {
            let Some(source) = initial.get(link.source) else {
                return Err(invalid("sampled program link source is out of bounds"));
            };
            let Some(target) = initial.get(link.target) else {
                return Err(invalid("sampled program link target is out of bounds"));
            };
            if !matches!(
                source,
                Block::Lever { .. }
                    | Block::StoneButton { .. }
                    | Block::RedstoneWire { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::RedstoneBlock
                    | Block::Observer { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            ) && !source.is_copper_bulb()
                && !source.is_command_block()
                && source.pressure_plate_powered().is_none()
            {
                return Err(invalid("sampled program link source must be lever, button, wire or repeater"));
            }
            if !matches!(
                target,
                Block::RedstoneWire { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::Hopper { .. }
                    | Block::IronTrapdoor { .. }
                    | Block::NoteBlock { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::Piston { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            ) && !target.is_copper_bulb()
                && !target.is_command_block()
            {
                return Err(invalid("sampled program link target must be wire, lamp or repeater"));
            }
            if link.attenuation > 15 {
                return Err(invalid("sampled program attenuation exceeds 15"));
            }
        }
        for record in &mut watched_wires {
            if !matches!(initial.get(record.wire), Some(Block::RedstoneWire { .. })) {
                return Err(invalid("sampled watched-wire index is not a wire"));
            }
            let mut recipients = std::collections::HashSet::new();
            record.watchers.retain(|watcher| recipients.insert(*watcher));
            for &watcher in &record.watchers {
                if !valid_notification(&initial, watcher)
                    || !matches!(
                        initial.get(notification_component(watcher)),
                        Some(Block::RedstoneLamp { .. } | Block::Piston { .. } | Block::Observer { .. })
                    )
                {
                    return Err(invalid("sampled watched-wire receiver is not a lamp, piston or observer"));
                }
            }
        }
        for operation in output_programs.iter().flatten().flatten() {
            validate_operation(*operation, &initial, links.len(), watched_wires.len(), None)?;
        }
        let mut incoming = vec![SmallVec::new(); initial.len()];
        let mut outgoing = vec![SmallVec::new(); initial.len()];
        for (id, link) in links.iter().enumerate() {
            incoming[link.target].push(id);
            outgoing[link.source].push(id);
        }
        let outgoing_arrangements = vec![vec![]; initial.len()];
        let component_count = initial.len();
        let component_indices = positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        let mut operations = Vec::new();
        let output_programs = output_programs
            .into_iter()
            .map(|programs| {
                programs.map(|program| {
                    let start = operations.len();
                    operations.extend(program);
                    start..operations.len()
                })
            })
            .collect();
        Ok(Self {
            relaxed_mode: false,
            movement_folding: false,
            source_order_cache: None,
            command_blocks: HashMap::new(),
            command_output_state: None,
            routing: None,
            piston_by_component: Vec::new(),
            piston_bindings: Vec::new(),
            repeater_by_component: Vec::new(),
            comparator_by_component: Vec::new(),
            watched_by_component: Vec::new(),
            movable_sources: Vec::new(),
            has_reactive: false,
            comparators: Vec::new(),
            comparators_by_override: vec![vec![]; component_count],
            side_links: Vec::new(),
            reactive: vec![false; initial.len()],
            reactive_wires: Vec::new(),
            reactive_by_source: vec![vec![]; initial.len()],
            reactive_neighbours: vec![vec![]; initial.len()],
            pistons: Vec::new(),
            tokens: Vec::new(),
            piston_scenery: Vec::new(),
            geometry: Vec::new(),
            component_indices,
            geometry_indices: HashMap::new(),
            projection_positions: Vec::new(),
            projection_indices: HashMap::new(),
            component_cells: Vec::new(),
            cell_components: Vec::new(),
            token_cells: Vec::new(),
            cell_tokens: Vec::new(),
            cell_pistons: Vec::new(),
            cell_observers: Vec::new(),
            cell_geometry: Vec::new(),
            token_piston_components: Vec::new(),
            token_sources: Vec::new(),
            token_updates: Vec::new(),
            token_watchers: Vec::new(),
            move_steps: Vec::new(),
            movement_steps: Vec::new(),
            shape_programs: HashMap::new(),
            shape_steps: Vec::new(),
            shape_maps: Vec::new(),
            cell_shape_steps: Vec::new(),
            movement_wires: Vec::new(),
            input_arrangements: Vec::new(),
            movement_groups: Vec::new(),
            movement_flips: Vec::new(),
            token_movement_groups: Vec::new(),
            movement_cells: Vec::new(),
            movement_transitions: Vec::new(),
            movement_ranges: Vec::new(),
            movement_fixed_ranges: Vec::new(),
            arrangement_links: Vec::new(),
            source_watchers: vec![SmallVec::new(); initial.len()],
            home_watchers: Vec::new(),
            positions,
            initial,
            sources,
            output_programs,
            operations,
            links,
            incoming,
            outgoing,
            outgoing_arrangements,
            input_groups_by_target: vec![SmallVec::new(); component_count],
            effective_links: Vec::new(),
            side_outputs: Vec::new(),
            watched_wires,
            repeaters,
        })
    }

    /// Reject inconsistent program records before execution or encoding.
    ///
    /// Inputs: This program's components, operations, electrical records, movement tables and notification lists.
    ///
    /// Check storage bounds and IDs, delegate movement-table checks to its owner, then validate watchers,
    /// comparator overrides and reactive callback references.
    pub(super) fn validate(&self) -> Result<(), ScratchError> {
        validate_histogram_bound(&self.initial)?;
        let invalid = |reason| ScratchError { position: None, reason };
        let nodes = self.initial.len();
        if nodes >= PISTON_HEAD_NOTIFICATION {
            return Err(invalid("sampled component IDs exceed notification range"));
        }
        let motion_capacity = self.pistons.len().checked_mul(2).and_then(|n| n.checked_add(self.tokens.len()));
        if nodes.checked_mul(2).is_none() || self.pistons.len().checked_mul(3).is_none() || motion_capacity.and_then(|n| n.checked_mul(2)).is_none() {
            return Err(invalid("runtime storage capacity overflow"));
        }
        self.validate_input_bound()?;
        for range in self.output_programs.iter().flatten() {
            let operations = self
                .operations
                .get(range.clone())
                .ok_or_else(|| invalid("invalid output program range"))?;
            for &operation in operations {
                validate_operation(
                    operation,
                    &self.initial,
                    self.links.len(),
                    self.watched_wires.len(),
                    Some(self.arrangement_links.len()),
                )?;
            }
        }
        for link in self.links.iter().chain(&self.side_links) {
            if link.source >= nodes || link.target >= nodes || link.attenuation > 15 {
                return Err(invalid("invalid electrical link reference"));
            }
        }
        let mut piston_components = vec![false; nodes];
        for piston in &self.pistons {
            let Some(&component) = self.component_indices.get(&piston.base) else {
                return Err(invalid("piston has no component"));
            };
            if !matches!(self.initial[component], Block::Piston { .. }) || piston_components[component] {
                return Err(invalid("invalid or duplicate piston component"));
            }
            piston_components[component] = true;
        }
        let notifications = self
            .source_watchers
            .iter()
            .flatten()
            .chain(self.home_watchers.iter().flat_map(|row| row.iter()))
            .chain(self.watched_wires.iter().flat_map(|record| record.watchers.iter()))
            .chain(self.movement_steps.iter().map(|step| &step.node));
        for &notification in notifications {
            if notification & PISTON_HEAD_NOTIFICATION != 0
                && valid_notification(&self.initial, notification)
                && !piston_components[notification_component(notification)]
            {
                return Err(invalid("piston-head notification has no piston record"));
            }
        }
        for token in &self.tokens {
            if token.position.is_some_and(|pos| !token.cells.contains(&pos)) {
                return Err(invalid("invalid captured token position"));
            }
        }
        let mut shape_port_maps = self.shape_steps.iter().flat_map(|step| &step.states.ports).flatten();
        if shape_port_maps.any(|&map| map >= self.shape_maps.len()) {
            return Err(invalid("invalid shape map reference"));
        }
        self.validate_movement()?;
        let mut watched = vec![false; nodes];
        for record in &self.watched_wires {
            if record.wire >= nodes || record.watchers.iter().any(|&node| !valid_notification(&self.initial, node)) {
                return Err(invalid("invalid watcher reference"));
            }
            watched[record.wire] = true;
        }
        self.validate_move_steps(&watched)?;
        let mut comparators = vec![false; nodes];
        for record in &self.comparators {
            if !matches!(self.initial.get(record.component), Some(Block::RedstoneComparator { .. }))
                || record.stored > 15
                || comparators[record.component]
            {
                return Err(invalid("invalid comparator record"));
            }
            comparators[record.component] = true;
            for input in [record.rear_override, record.far_override].into_iter().flatten() {
                let valid = match input {
                    ComparatorInput::Static(level) => level <= 15,
                    ComparatorInput::CopperBulb(source) => self.initial.get(source).is_some_and(|block| block.is_copper_bulb()),
                    ComparatorInput::CommandBlock(source) => self.initial.get(source).is_some_and(|block| block.is_command_block()),
                };
                if !valid {
                    return Err(invalid("invalid comparator override reference"));
                }
            }
        }
        let notification_lists = self
            .source_watchers
            .iter()
            .map(SmallVec::as_slice)
            .chain(self.home_watchers.iter().map(SmallVec::as_slice));
        for list in notification_lists {
            if list.iter().any(|&node| !valid_notification(&self.initial, node)) {
                return Err(invalid("invalid notification reference"));
            }
        }
        if self.reactive_neighbours.iter().flatten().any(|&node| node >= nodes) {
            return Err(invalid("invalid reactive notification reference"));
        }
        Ok(())
    }

    pub(crate) fn enable_relaxed_wire_shapes(&mut self) -> Result<(), ScratchError> {
        let invalid = || ScratchError {
            position: None,
            reason: "invalid sampled shape program",
        };
        let keep = self
            .shape_steps
            .iter()
            .map(|step| match self.initial.get(step.node) {
                Some(Block::Observer { .. }) => Ok(true),
                Some(Block::RedstoneWire { .. }) => Ok(false),
                _ => Err(invalid()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut retained = Vec::with_capacity(keep.len() + 1);
        retained.push(0);
        for &keep in &keep {
            retained.push(retained.last().unwrap() + usize::from(keep));
        }
        if self
            .shape_programs
            .values()
            .any(|range| range.start > range.end || range.end >= retained.len())
        {
            return Err(invalid());
        }
        let mut index = 0;
        self.shape_steps.retain(|_| {
            let include = keep[index];
            index += 1;
            include
        });
        for range in self.shape_programs.values_mut() {
            *range = retained[range.start]..retained[range.end];
        }
        // Keep explicit empty programs: removing their keys would activate fallback notifications.
        self.shape_maps.clear();
        self.relaxed_mode = true;
        self.prepare_projection();
        self.validate()
    }

    pub(super) fn set_output_program(&mut self, component: usize, powered: bool, operations: Vec<Operation>) {
        let start = self.operations.len();
        self.operations.extend(operations);
        self.output_programs[component][usize::from(powered)] = start..self.operations.len();
    }

    pub(super) fn output_program(&self, component: usize, powered: bool) -> &[Operation] {
        &self.operations[self.output_programs[component][usize::from(powered)].clone()]
    }

    #[cfg(test)]
    pub(super) fn output_vectors(&self) -> Vec<[Vec<Operation>; 2]> {
        self.output_programs
            .iter()
            .map(|programs| programs.clone().map(|range| self.operations[range].to_vec()))
            .collect()
    }

    pub(super) fn push_link(&mut self, link: ElectricalLink) {
        let index = self.links.len();
        self.incoming[link.target].push(index);
        self.outgoing[link.source].push(index);
        self.links.push(link);
    }
}

#[cfg(test)]
#[path = "../tests/sampled/source_cache.rs"]
mod source_cache_tests;

#[cfg(test)]
#[path = "../tests/sampled/relaxed_shapes.rs"]
mod relaxed_shape_tests;
