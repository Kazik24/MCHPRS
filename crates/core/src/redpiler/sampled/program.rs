use super::{Runtime, ScratchError};
use mchprs_blocks::{BlockPos, blocks::Block};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::num::NonZeroUsize;

pub(super) const MAX_ARRANGEMENT_STATES: usize = 65_536;

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
    /// component indices in `Program::positions` notified when this wire changes.
    pub(super) watchers: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct MoveStepSpec {
    /// component index into `Program::positions` for this movement callback.
    pub(super) node: usize,
    /// indicating this step was selected because the component watches a moved cell.
    pub(super) watcher: bool,
    /// indicating this step evaluates a watched wire.
    pub(super) wire: bool,
    /// watched-wire indices consulted by this step.
    pub(super) wires: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct MoveStep {
    /// component index into `Program::positions` for this movement callback.
    pub(super) node: usize,
    /// indicating this step was selected because the component watches a moved cell.
    pub(super) watcher: bool,
    /// indicating this step evaluates a watched wire.
    pub(super) wire: bool,
    /// slice into `Program::movement_wires` listing wires consulted by this step.
    pub(super) wires: std::ops::Range<usize>,
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
        self.token_states.get(axis).map_or(physical, |states| states.iter().copied().max().unwrap_or(0) + 1)
    }
}

fn valid_states(states: &[usize], physical: usize) -> bool {
    states.len() == physical && states.first() == Some(&0)
        && states.iter().copied().max().is_some_and(|max| max < physical && (0..=max).all(|digit| states.contains(&digit)))
}

#[derive(Clone, Copy)]
pub(super) struct ArrangementFlip {
    /// component index into `Program::positions` whose input changes.
    pub(super) target: usize,
    /// component or token whose movement changes the input.
    pub(super) source: ArrangementSource,
    /// signal attenuation for this source-to-target connection.
    pub(super) attenuation: u8,
    /// index into `Program::arrangement_links` for the connection being changed.
    pub(super) link: usize,
    /// indicating the source is entering the target's input footprint (`false` means leaving).
    pub(super) entering: bool,
    /// indicating this is a side input rather than the primary input.
    pub(super) side: bool,
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
pub(super) struct MovementAxis {
    /// token index into `Program::tokens` moving along this axis.
    pub(super) token: usize,
    /// mixed-radix stride used to extract this token's state from a group arrangement index.
    pub(super) stride: usize,
    pub(super) states: Vec<usize>,
    /// per-token-cell slices into the group's departure-step storage.
    pub(super) depart: Vec<std::ops::Range<usize>>,
    /// per-token-cell slices into the group's arrival-step storage.
    pub(super) arrive: Vec<std::ops::Range<usize>>,
}

#[derive(Clone)]
pub(super) struct MovementTransition {
    pub(super) group: usize,
    pub(super) delta: usize,
    pub(super) depart: usize,
    pub(super) arrive: usize,
    pub(super) states: usize,
}

#[derive(Clone)]
pub(super) struct MovementGroup {
    /// token axes and their movement-step ranges for one joint arrangement group.
    pub(super) axes: Vec<MovementAxis>,
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
    // Components and output instructions
    /// component-indexed world positions; index is the canonical component ID throughout the program.
    pub(super) positions: Vec<BlockPos>,
    /// world-position to component index lookup matching `positions`.
    pub(super) component_indices: HashMap<BlockPos, usize>,
    /// initial component blocks, parallel to `positions` by component index.
    pub(super) initial: Vec<Block>,
    /// component indices of external input sources such as levers and buttons.
    pub(super) sources: Vec<usize>,
    /// per component, slicing `operations` for its two output transitions.
    pub(super) output_programs: Vec<[std::ops::Range<usize>; 2]>,
    /// flattened compiled output operations referenced by `output_programs` ranges.
    pub(super) operations: Vec<Operation>,

    // Electrical connections
    /// ordinary source-to-target input links; IDs index this vector.
    pub(super) links: Vec<ElectricalLink>,
    /// per target component, listing incoming indices into `links`.
    pub(super) incoming: Vec<Vec<usize>>,
    /// per source component, listing outgoing indices into `links`.
    pub(super) outgoing: Vec<Vec<usize>>,
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
    pub(super) pistons: Vec<super::compiler::PistonActor>,
    /// reverse map from component index to `piston_bindings` index plus one.
    pub(super) piston_by_component: Vec<Option<NonZeroUsize>>,
    /// piston projection bindings and their home/far cell state.
    pub(super) piston_bindings: Vec<PistonBinding>,
    /// movable blocks and their legal positions/cells.
    pub(super) tokens: Vec<super::compiler::PistonToken>,
    /// component-indexed flags for nodes that can source movable-token inputs.
    pub(super) movable_sources: Vec<bool>,
    /// per token, piston component indices whose motion can move that token.
    pub(super) token_piston_components: Vec<Vec<usize>>,
    /// indexed token, token cell, then source component/attenuation dependencies.
    pub(super) token_sources: Vec<Vec<Vec<(usize, u8)>>>,
    /// indexed token and cell, with separate ordered component updates for departure and arrival.
    pub(super) token_updates: Vec<Vec<[Vec<usize>; 2]>>,
    /// indexed token and cell, listing component indices watching movement at that cell.
    pub(super) token_watchers: Vec<Vec<Vec<usize>>>,
    /// per source component, listing components notified by source changes.
    pub(super) source_watchers: Vec<Vec<usize>>,
    /// per piston component, listing components notified by changes around its home cell.
    pub(super) home_watchers: Vec<SmallVec<[usize; 2]>>,

    // Arrangements and movement instructions
    /// joint token-position states that change component input connectivity.
    pub(super) input_arrangements: Vec<InputArrangement>,
    /// per target component, listing its token-dependent input-arrangement indices.
    pub(super) input_groups_by_target: Vec<Vec<usize>>,
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

    // Geometry and projection indexes
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
    pub(super) fn compile_movement_flips(&mut self) -> Result<(), ScratchError> {
        for group in &self.input_arrangements {
            let invalid = |reason| ScratchError {
                position: self.positions.get(group.target).copied(),
                reason,
            };
            if group.target >= self.initial.len() || group.tokens.iter().any(|&token| token >= self.tokens.len()) {
                return Err(invalid("invalid movement group reference"));
            }
            let mut seen = std::collections::HashSet::new();
            if group.tokens.iter().any(|&token| !seen.insert(token)) {
                return Err(invalid("duplicate token in movement group"));
            }
            if !group.token_states.is_empty() && (group.token_states.len() != group.tokens.len()
                || group.token_states.iter().zip(&group.tokens).any(|(states, &token)| !valid_states(states, self.tokens[token].cells.len() + 1))) {
                return Err(invalid("invalid electrical token states"));
            }
            let count = group
                .tokens
                .iter().enumerate()
                .try_fold(1usize, |count, (axis, &token)| count.checked_mul(group.radix(axis, self.tokens[token].cells.len() + 1)));
            if count != Some(group.inputs.len()) || group.inputs.len() > MAX_ARRANGEMENT_STATES {
                return Err(invalid("invalid movement group dimensions"));
            }
            for &(source, attenuation) in group.inputs.iter().flatten() {
                let valid = match source {
                    ArrangementSource::Component(node) => node < self.initial.len(),
                    ArrangementSource::Token(token) => token < self.tokens.len(),
                };
                if !valid || attenuation > 15 {
                    return Err(invalid("invalid movement group source"));
                }
            }
        }
        for (token, cells) in self.token_sources.iter().enumerate() {
            if token >= self.tokens.len() || cells.len() != self.tokens[token].cells.len() {
                return Err(ScratchError {
                    position: None,
                    reason: "invalid token source dimensions",
                });
            }
            for &(target, attenuation) in cells.iter().flatten() {
                if target >= self.initial.len() || attenuation > 15 {
                    return Err(ScratchError {
                        position: self.tokens[token].position,
                        reason: "invalid token source reference",
                    });
                }
            }
        }
        for (token, cells) in self.move_steps.iter().enumerate() {
            if token >= self.tokens.len() || cells.len() != self.tokens[token].cells.len() {
                return Err(ScratchError {
                    position: None,
                    reason: "invalid movement step dimensions",
                });
            }
            for range in cells.iter().flatten() {
                let Some(steps) = self.movement_steps.get(range.clone()) else {
                    return Err(ScratchError {
                        position: self.tokens[token].position,
                        reason: "invalid movement step range",
                    });
                };
                for step in steps {
                    let watched = |node| self.watched_wires.iter().any(|wire| wire.wire == node);
                    let wires = self.movement_wires.get(step.wires.clone());
                    if step.node >= self.initial.len()
                        || (step.wire && !watched(step.node))
                        || wires.is_none_or(|wires| wires.iter().any(|&wire| !watched(wire)))
                    {
                        return Err(ScratchError {
                            position: self.tokens[token].position,
                            reason: "invalid movement step reference",
                        });
                    }
                }
            }
        }
        let mut groups = Vec::new();
        let mut flips = Vec::new();
        let mut links = Vec::new();
        let mut grouped: Vec<(Vec<usize>, Vec<Vec<usize>>, Vec<Vec<(usize, ArrangementSource, u8, bool, bool)>>)> = Vec::new();
        for input in &self.input_arrangements {
            let states: Vec<_> = input.tokens.iter().enumerate().map(|(axis, &token)|
                (0..=self.tokens[token].cells.len()).map(|digit| input.digit(axis, digit)).collect::<Vec<_>>()).collect();
            let index = if let Some(index) = grouped.iter().position(|(tokens, mapping, _)| *tokens == input.tokens && *mapping == states) {
                index
            } else {
                grouped.push((input.tokens.clone(), states, vec![Vec::new(); input.inputs.len()]));
                grouped.len() - 1
            };
            for (state, entries) in input.inputs.iter().enumerate() {
                grouped[index].2[state].extend(
                    entries
                        .iter()
                        .map(|&(source, attenuation)| (input.target, source, attenuation, input.side, input.silent)),
                );
            }
        }
        for (tokens, states, maps) in grouped {
            let (group, entries) = self.movement_group(&tokens, &states, &maps, groups.len(), links.len(), &mut flips);
            groups.push(group);
            links.extend(entries);
        }
        for (token, cells) in self.token_sources.iter().enumerate() {
            let mut maps = vec![vec![]];
            maps.extend(cells.iter().map(|links| {
                links
                    .iter()
                    .filter_map(|&(target, attenuation)| {
                        let source = ArrangementSource::Token(token);
                        (!self
                            .input_arrangements
                            .iter()
                            .any(|group| group.target == target && !group.side && group.conditional_sources.contains(&source)))
                        .then_some((target, source, attenuation, false, matches!(self.initial[target], Block::Piston { .. })))
                    })
                    .collect()
            }));
            let states = vec![(0..=cells.len()).collect()];
            let (group, entries) = self.movement_group(&[token], &states, &maps, groups.len(), links.len(), &mut flips);
            groups.push(group);
            links.extend(entries);
        }
        self.movement_groups = groups;
        self.movement_flips = flips;
        self.arrangement_links = links;
        self.outgoing_arrangements = vec![vec![]; self.initial.len()];
        self.input_groups_by_target = vec![vec![]; self.initial.len()];
        for (index, group) in self.input_arrangements.iter().enumerate() {
            self.input_groups_by_target[group.target].push(index);
        }
        for (index, link) in self.arrangement_links.iter().enumerate() {
            if let ArrangementSource::Component(source) = link.source {
                self.outgoing_arrangements[source].push(index);
            }
        }
        self.token_movement_groups = vec![vec![]; self.tokens.len()];
        for shared in [false, true] {
            for (group, record) in self.movement_groups.iter().enumerate() {
                if (record.axes.len() > 1) != shared {
                    continue;
                }
                for (axis, record) in record.axes.iter().enumerate() {
                    self.token_movement_groups[record.token].push((group, axis));
                }
            }
        }
        self.movement_ranges.clear();
        let mut tables = Vec::with_capacity(self.movement_groups.len());
        for group in &self.movement_groups {
            let mut axes = Vec::with_capacity(group.axes.len());
            for axis in &group.axes {
                let depart = self.movement_ranges.len();
                self.movement_ranges.extend(axis.depart.iter().cloned());
                let arrive = self.movement_ranges.len();
                self.movement_ranges.extend(axis.arrive.iter().cloned());
                axes.push((depart, arrive));
            }
            tables.push(axes);
        }
        self.movement_transitions.clear();
        self.movement_cells.clear();
        for (token, associations) in self.token_movement_groups.iter().enumerate() {
            let mut cells = Vec::with_capacity(self.tokens[token].cells.len());
            for cell in 0..self.tokens[token].cells.len() {
                let start = self.movement_transitions.len();
                for &(group, axis) in associations {
                    let axis_record = &self.movement_groups[group].axes[axis];
                    let delta = axis_record.states[cell + 1].checked_mul(axis_record.stride).ok_or(ScratchError {
                        position: self.tokens[token].position,
                        reason: "movement transition capacity overflow",
                    })?;
                    let (depart, arrive) = tables[group][axis];
                    self.movement_transitions.push(MovementTransition { group, delta, depart, arrive, states: axis_record.depart.len() });
                }
                cells.push(start..self.movement_transitions.len());
            }
            self.movement_cells.push(cells);
        }
        self.validate_input_bound()
    }

    fn validate_input_bound(&self) -> Result<(), ScratchError> {
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
                let mut components = Vec::new();
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

    fn movement_group(
        &self,
        tokens: &[usize],
        states: &[Vec<usize>],
        maps: &[Vec<(usize, ArrangementSource, u8, bool, bool)>],
        group: usize,
        link_start: usize,
        flips: &mut Vec<ArrangementFlip>,
    ) -> (MovementGroup, Vec<ArrangementLink>) {
        let mut conditions = Vec::new();
        for &entry in maps.iter().flatten() {
            if !conditions.contains(&entry) {
                conditions.push(entry);
            }
        }
        let mut stride = 1;
        let axes = tokens
            .iter()
            .enumerate().map(|(index, &token)| {
                let radix = states[index].iter().copied().max().unwrap_or(0) + 1;
                let mut depart = vec![0..0; maps.len()];
                let mut arrive = vec![0..0; maps.len()];
                for state in 0..maps.len() {
                    let digit = state / stride % radix;
                    if digit == 0 {
                        continue;
                    }
                    let empty = state - digit * stride;
                    let start = flips.len();
                    for (index, &(target, source, attenuation, side, silent)) in conditions.iter().enumerate() {
                        let entry = (target, source, attenuation, side, silent);
                        if maps[state].contains(&entry) != maps[empty].contains(&entry) {
                            let flip = ArrangementFlip {
                                target,
                                source,
                                attenuation,
                                link: link_start + index,
                                entering: maps[empty].contains(&entry),
                                side,
                            };
                            flips.push(flip);
                        }
                    }
                    depart[state] = start..flips.len();
                    let start = flips.len();
                    for index in depart[state].clone() {
                        let flip = flips[index];
                        flips.push(ArrangementFlip {
                            entering: !flip.entering,
                            ..flip
                        });
                    }
                    arrive[state] = start..flips.len();
                }
                let axis = MovementAxis {
                    token,
                    stride,
                    states: states[index].clone(),
                    depart,
                    arrive,
                };
                stride *= radix;
                axis
            })
            .collect();
        let links = conditions
            .iter()
            .map(|&(target, source, attenuation, side, silent)| ArrangementLink {
                target,
                side,
                silent,
                source,
                attenuation,
                group,
                active: maps
                    .iter()
                    .map(|map| map.contains(&(target, source, attenuation, side, silent)))
                    .collect(),
            })
            .collect();
        (MovementGroup { axes }, links)
    }

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
            if !matches!(block, Block::Lever { .. } | Block::StoneButton { .. }) {
                return Err(invalid("sampled program source is not a lever or button"));
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
                if !matches!(
                    initial.get(watcher),
                    Some(Block::RedstoneLamp { .. } | Block::Piston { .. } | Block::Observer { .. })
                ) {
                    return Err(invalid("sampled watched-wire receiver is not a lamp, piston or observer"));
                }
            }
        }
        for operation in output_programs.iter().flatten().flatten() {
            validate_operation(*operation, &initial, links.len(), watched_wires.len(), None)?;
        }
        let mut incoming = vec![vec![]; initial.len()];
        let mut outgoing = vec![vec![]; initial.len()];
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
            command_blocks: HashMap::new(),
            command_output_state: None,
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
            movement_wires: Vec::new(),
            input_arrangements: Vec::new(),
            movement_groups: Vec::new(),
            movement_flips: Vec::new(),
            token_movement_groups: Vec::new(),
            movement_cells: Vec::new(),
            movement_transitions: Vec::new(),
            movement_ranges: Vec::new(),
            arrangement_links: Vec::new(),
            source_watchers: vec![vec![]; initial.len()],
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
            input_groups_by_target: vec![vec![]; component_count],
            effective_links: Vec::new(),
            side_outputs: Vec::new(),
            watched_wires,
            repeaters,
        })
    }

    pub(super) fn validate(&self) -> Result<(), ScratchError> {
        validate_histogram_bound(&self.initial)?;
        let invalid = |reason| ScratchError { position: None, reason };
        let nodes = self.initial.len();
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
        for token in &self.tokens {
            if token.position.is_some_and(|pos| !token.cells.contains(&pos)) {
                return Err(invalid("invalid captured token position"));
            }
        }
        let valid_source = |source| match source {
            ArrangementSource::Component(node) => node < nodes,
            ArrangementSource::Token(token) => token < self.tokens.len(),
        };
        let mut group_states = Vec::with_capacity(self.movement_groups.len());
        for (group, record) in self.movement_groups.iter().enumerate() {
            let mut states = 1usize;
            for axis in &record.axes {
                if axis.token >= self.tokens.len() || axis.stride != states || !valid_states(&axis.states, self.tokens[axis.token].cells.len() + 1) {
                    return Err(invalid("invalid movement axis reference"));
                }
                states = states
                    .checked_mul(axis.states.iter().copied().max().unwrap_or(0) + 1)
                    .ok_or_else(|| invalid("movement group capacity overflow"))?;
            }
            if states > MAX_ARRANGEMENT_STATES || record.axes.iter().any(|axis| axis.depart.len() != states || axis.arrive.len() != states) {
                return Err(invalid("invalid sealed movement group dimensions"));
            }
            for axis in &record.axes {
                for range in axis.depart.iter().chain(&axis.arrive) {
                    let flips = self
                        .movement_flips
                        .get(range.clone())
                        .ok_or_else(|| invalid("invalid movement flip range"))?;
                    for flip in flips {
                        let link = self
                            .arrangement_links
                            .get(flip.link)
                            .ok_or_else(|| invalid("invalid movement flip link"))?;
                        if link.group != group
                            || flip.target != link.target
                            || flip.source != link.source
                            || flip.attenuation != link.attenuation
                            || flip.side != link.side
                        {
                            return Err(invalid("movement flip does not match its link"));
                        }
                    }
                }
            }
            group_states.push(states);
        }
        if self.movement_cells.len() != self.tokens.len() || self.token_movement_groups.len() != self.tokens.len() {
            return Err(invalid("invalid movement plan dimensions"));
        }
        for (token, cells) in self.movement_cells.iter().enumerate() {
            if cells.len() != self.tokens[token].cells.len() {
                return Err(invalid("invalid movement plan cells"));
            }
            for (cell, range) in cells.iter().enumerate() {
                let records = self.movement_transitions.get(range.clone()).ok_or_else(|| invalid("invalid movement plan range"))?;
                let associations = &self.token_movement_groups[token];
                if records.len() != associations.len() {
                    return Err(invalid("invalid movement plan length"));
                }
                for (record, &(group, axis)) in records.iter().zip(associations) {
                    let axis = self.movement_groups.get(group).and_then(|g| g.axes.get(axis)).ok_or_else(|| invalid("invalid movement plan axis"))?;
                    let depart = record.depart.checked_add(record.states).and_then(|end| self.movement_ranges.get(record.depart..end));
                    let arrive = record.arrive.checked_add(record.states).and_then(|end| self.movement_ranges.get(record.arrive..end));
                    if axis.token != token || record.group != group || axis.states[cell + 1].checked_mul(axis.stride) != Some(record.delta)
                        || depart != Some(axis.depart.as_slice()) || arrive != Some(axis.arrive.as_slice()) {
                        return Err(invalid("invalid movement plan transition"));
                    }
                }
            }
        }
        for link in &self.arrangement_links {
            if link.target >= nodes || !valid_source(link.source) || link.attenuation > 15 || link.group >= self.movement_groups.len() {
                return Err(invalid("invalid sealed arrangement link"));
            }
            if link.active.len() != group_states[link.group] {
                return Err(invalid("invalid arrangement mask dimensions"));
            }
        }
        let mut watched = vec![false; nodes];
        for record in &self.watched_wires {
            if record.wire >= nodes || record.watchers.iter().any(|&node| node >= nodes) {
                return Err(invalid("invalid watcher reference"));
            }
            watched[record.wire] = true;
        }
        for (token, cells) in self.move_steps.iter().enumerate() {
            if token >= self.tokens.len() || cells.len() != self.tokens[token].cells.len() {
                return Err(invalid("invalid sealed movement step dimensions"));
            }
            for range in cells.iter().flatten() {
                let steps = self
                    .movement_steps
                    .get(range.clone())
                    .ok_or_else(|| invalid("invalid movement step range"))?;
                for step in steps {
                    let wires = self
                        .movement_wires
                        .get(step.wires.clone())
                        .ok_or_else(|| invalid("invalid movement wire range"))?;
                    if step.node >= nodes || (step.wire && !watched[step.node]) || wires.iter().any(|&wire| wire >= nodes || !watched[wire]) {
                        return Err(invalid("invalid sealed movement step reference"));
                    }
                }
            }
        }
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
        for list in self.source_watchers.iter().map(Vec::as_slice).chain(self.home_watchers.iter().map(SmallVec::as_slice)).chain(self.reactive_neighbours.iter().map(Vec::as_slice)) {
            if list.iter().any(|&node| node >= nodes) {
                return Err(invalid("invalid notification reference"));
            }
        }
        Ok(())
    }

    pub(super) fn set_move_steps(&mut self, programs: Vec<Vec<[Vec<MoveStepSpec>; 2]>>) {
        self.move_steps.clear();
        self.movement_steps.clear();
        self.movement_wires.clear();
        for cells in programs {
            let mut compiled = Vec::with_capacity(cells.len());
            for edges in cells {
                compiled.push(edges.map(|steps| {
                    let start = self.movement_steps.len();
                    for step in steps {
                        let wires = self.movement_wires.len();
                        self.movement_wires.extend(step.wires);
                        self.movement_steps.push(MoveStep {
                            node: step.node,
                            watcher: step.watcher,
                            wire: step.wire,
                            wires: wires..self.movement_wires.len(),
                        });
                    }
                    start..self.movement_steps.len()
                }));
            }
            self.move_steps.push(compiled);
        }
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

    pub(super) fn start(&self) -> Runtime {
        self.clone().into_runtime()
    }

    pub(super) fn into_runtime(mut self) -> Runtime {
        self.prepare_projection();
        self.into_runtime_prepared()
    }

    pub(super) fn into_runtime_prepared(self) -> Runtime {
        let cell_count = self.projection_positions.len();
        let motion_capacity = self
            .pistons
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.tokens.len()))
            .expect("validated motion capacity");
        let mut runtime = Runtime {
            blocks: self.initial.clone(),
            contributions: self
                .links
                .iter()
                .map(|link| {
                    let strength: u8 = match self.initial[link.source] {
                        block if block.pressure_plate_powered().is_some() => u8::from(block.pressure_plate_powered() == Some(true)) * 15,
                        Block::Lever { lever } => {
                            if lever.powered {
                                15
                            } else {
                                0
                            }
                        }
                        Block::StoneButton { button } => {
                            if button.powered {
                                15
                            } else {
                                0
                            }
                        }
                        Block::RedstoneComparator { comparator } => {
                            if comparator.powered {
                                self.comparators
                                    .iter()
                                    .find(|record| record.component == link.source)
                                    .map_or(0, |record| record.stored)
                            } else {
                                0
                            }
                        }
                        Block::RedstoneWire { wire } => wire.power,
                        Block::RedstoneBlock => {
                            if self.tokens.iter().any(|token| token.position == Some(self.positions[link.source])) {
                                0
                            } else {
                                15
                            }
                        }
                        Block::Observer { observer } => {
                            if observer.powered {
                                15
                            } else {
                                0
                            }
                        }
                        Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                            if lit {
                                15
                            } else {
                                0
                            }
                        }
                        Block::RedstoneRepeater { repeater } => {
                            if repeater.powered {
                                15
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                    strength.saturating_sub(link.attenuation)
                })
                .collect(),
            arrangement_contributions: vec![0; self.arrangement_links.len()],
            input_levels: vec![[super::input::InputState::default(); 2]; self.initial.len()],
            input_histograms: Vec::new(),
            comparator_outputs: {
                let mut outputs = vec![0; self.initial.len()];
                for record in &self.comparators {
                    outputs[record.component] = record.stored;
                }
                outputs
            },
            reactive_wire_strengths: self
                .reactive_wires
                .iter()
                .map(|record| match self.initial[record.wire] {
                    Block::RedstoneWire { wire } => wire.power,
                    _ => unreachable!(),
                })
                .collect(),
            watched_strengths: self
                .watched_wires
                .iter()
                .map(|record| match self.initial[record.wire] {
                    Block::RedstoneWire { wire } => wire.power,
                    _ => unreachable!(),
                })
                .collect(),
            pending: super::scheduler::Scheduler::new(self.initial.len().checked_mul(2).expect("validated scheduled capacity").max(64)),
            pending_counts: vec![0; self.initial.len()],
            pending_passive_positions: Vec::new(),
            phase: 1,
            updated_in_phase: vec![0; self.initial.len()],
            piston_state: mchprs_world::PistonState::default(),
            piston_extended: self.pistons.iter().map(|p| p.extended).collect(),
            piston_head_present: self.pistons.iter().map(|p| p.head_present).collect(),
            piston_head_moving: vec![false; self.pistons.len()],
            piston_base_moving: vec![false; self.pistons.len()],
            token_positions: self.tokens.iter().map(|t| t.position).collect(),
            group_arrangements: self
                .movement_groups
                .iter()
                .map(|group| {
                    group
                        .axes
                        .iter()
                        .map(|axis| {
                            self.tokens[axis.token]
                                .position
                                .and_then(|pos| self.tokens[axis.token].cells.iter().position(|&cell| cell == pos))
                                .map_or(0, |cell| axis.states[cell + 1])
                                * axis.stride
                        })
                        .sum()
                })
                .collect(),
            piston_events: std::collections::VecDeque::with_capacity(self.pistons.len() * 3),
            queued_piston_events: vec![0; self.pistons.len()],
            motions: Vec::with_capacity(motion_capacity),
            active_motions: std::collections::VecDeque::with_capacity(motion_capacity),
            free_motion_slots: Vec::with_capacity(motion_capacity),
            piston_motion: vec![None; self.pistons.len()],
            cell_motion: vec![None; cell_count],
            motion_time: 0,
            motion_phase: false,
            landing_cursor: None,
            movement_switched: Vec::with_capacity(self.initial.len()),
            switched: vec![false; self.initial.len()],
            movement_changed_wires: Vec::with_capacity(self.watched_wires.len()),
            changed_wires: vec![false; self.initial.len()],
            command_blocks: self.command_blocks.clone(),
            command_output_state: self.command_output_state.clone(),
            command_emissions: Vec::new(),
            notes: Vec::with_capacity(self.initial.iter().filter(|block| matches!(block, Block::NoteBlock { .. })).count()),
            note_queued: vec![false; self.initial.len()],
            dirty_cells: (0..cell_count).collect(),
            dirty: vec![true; cell_count],
            needs_full_publication: false,
            accepted_piston_actions: 0,
            token_movements: 0,
            motion_completions: 0,
            operations: 0,
            deliveries: 0,
            scheduled_repeater_ticks: 0,
            tick_operations: 0,
            failure: None,
            program: std::sync::Arc::new(self),
        };
        for (component, block) in runtime.program.initial.iter().enumerate() {
            let channels = match block {
                Block::RedstoneComparator { .. } => 2,
                Block::RedstoneWire { .. } => 1,
                _ => 0,
            };
            if channels != 0 {
                for state in &mut runtime.input_levels[component][..channels] {
                    state.analog = u32::try_from(runtime.input_histograms.len()).expect("validated analog channel capacity");
                    runtime.input_histograms.push(super::input::Histogram::default());
                }
            }
        }
        for (index, link) in runtime.program.arrangement_links.iter().enumerate() {
            if link.active[runtime.group_arrangements[link.group]] {
                runtime.arrangement_contributions[index] = runtime.arrangement_output(link.source).saturating_sub(link.attenuation);
            }
        }
        for (index, link) in runtime.program.links.iter().enumerate() {
            if runtime.program.effective_links[index] {
                runtime.input_levels[link.target][0].change(0, runtime.contributions[index], &mut runtime.input_histograms);
            }
        }
        for (index, link) in runtime.program.arrangement_links.iter().enumerate() {
            runtime.input_levels[link.target][usize::from(link.side)].change(
                0,
                runtime.arrangement_contributions[index],
                &mut runtime.input_histograms,
            );
        }
        for source in 0..runtime.program.side_outputs.len() {
            let level = runtime.output(source);
            for index in 0..runtime.program.side_outputs[source].len() {
                let (target, attenuation) = runtime.program.side_outputs[source][index];
                runtime.input_levels[target][1].change(0, level.saturating_sub(attenuation), &mut runtime.input_histograms);
            }
        }
        runtime
    }
}
