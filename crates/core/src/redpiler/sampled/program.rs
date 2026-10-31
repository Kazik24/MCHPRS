use super::{Runtime, ScratchError};
use crate::world::scheduler::TickScheduler;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::HashMap;

#[derive(Clone)]
pub(super) enum Operation {
    Contribution(usize),
    ArrangementContribution(usize),
    RefreshWire(usize),
    CheckWatchedWire(usize),
    UpdateLamp(usize),
    UpdateRepeater(usize),
    UpdatePiston(usize),
    UpdateTorch(usize),
    UpdateComparator(usize),
}

#[derive(Clone)]
pub(super) struct ElectricalLink {
    pub(super) source: usize,
    pub(super) target: usize,
    pub(super) attenuation: u8,
}

#[derive(Clone)]
pub(super) struct WatchedWire {
    pub(super) wire: usize,
    pub(super) watchers: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct MoveStep {
    pub(super) node: usize,
    pub(super) watcher: bool,
    pub(super) wire: bool,
    pub(super) wires: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct RepeaterRecord {
    pub(super) component: usize,
    pub(super) output_is_diode: bool,
    pub(super) side_sources: Option<Vec<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(super) enum ArrangementSource {
    Component(usize),
    Token(usize),
}

#[derive(Clone)]
pub(super) struct InputArrangement {
    pub(super) target: usize,
    pub(super) side: bool,
    pub(super) silent: bool,
    pub(super) tokens: Vec<usize>,
    pub(super) conditional_sources: Vec<ArrangementSource>,
    pub(super) inputs: Vec<Vec<(ArrangementSource, u8)>>,
}

#[derive(Clone, Copy)]
pub(super) struct ArrangementFlip {
    pub(super) target: usize,
    pub(super) source: ArrangementSource,
    pub(super) attenuation: u8,
    pub(super) link: usize,
    pub(super) entering: bool,
}

#[derive(Clone)]
pub(super) struct ArrangementLink {
    pub(super) target: usize,
    pub(super) side: bool,
    pub(super) silent: bool,
    pub(super) source: ArrangementSource,
    pub(super) attenuation: u8,
    pub(super) group: usize,
    pub(super) active: Vec<bool>,
}

#[derive(Clone)]
pub(super) struct MovementAxis {
    pub(super) token: usize,
    pub(super) stride: usize,
    pub(super) depart: Vec<Vec<ArrangementFlip>>,
    pub(super) arrive: Vec<Vec<ArrangementFlip>>,
}

#[derive(Clone)]
pub(super) struct MovementGroup {
    pub(super) axes: Vec<MovementAxis>,
}

#[derive(Clone)]
pub(super) struct ComparatorRecord {
    pub(super) component: usize,
    pub(super) stored: u8,
    pub(super) rear_override: Option<u8>,
    pub(super) far_override: Option<u8>,
    pub(super) output_is_diode: bool,
}

#[derive(Clone)]
pub(super) struct ReactiveWire {
    pub(super) wire: usize,
    pub(super) sources: Vec<(usize, u8)>,
    pub(super) recipients: Vec<usize>,
}

#[derive(Clone)]
pub(crate) struct Program {
    pub(super) comparators: Vec<ComparatorRecord>,
    pub(super) side_links: Vec<ElectricalLink>,
    pub(super) reactive: Vec<bool>,
    pub(super) reactive_wires: Vec<ReactiveWire>,
    pub(super) reactive_by_source: Vec<Vec<usize>>,
    pub(super) reactive_neighbours: Vec<Vec<usize>>,
    pub(super) pistons: Vec<super::compiler::PistonActor>,
    pub(super) tokens: Vec<super::compiler::PistonToken>,
    pub(super) piston_scenery: Vec<BlockPos>,
    pub(super) geometry: Vec<(BlockPos, Block)>,
    pub(super) component_indices: HashMap<BlockPos, usize>,
    pub(super) geometry_indices: HashMap<BlockPos, Block>,
    pub(super) projection_positions: Vec<BlockPos>,
    pub(super) token_sources: Vec<Vec<Vec<(usize, u8)>>>,
    pub(super) token_updates: Vec<Vec<[Vec<usize>; 2]>>,
    pub(super) token_watchers: Vec<Vec<Vec<usize>>>,
    pub(super) move_steps: Vec<Vec<[Vec<MoveStep>; 2]>>,
    pub(super) input_arrangements: Vec<InputArrangement>,
    pub(super) movement_groups: Vec<MovementGroup>,
    pub(super) token_movement_groups: Vec<Vec<(usize, usize)>>,
    pub(super) arrangement_links: Vec<ArrangementLink>,
    pub(super) source_watchers: Vec<Vec<usize>>,
    pub(super) home_watchers: Vec<Vec<usize>>,
    pub(super) positions: Vec<BlockPos>,
    pub(super) initial: Vec<Block>,
    pub(super) sources: Vec<usize>,
    pub(super) output_programs: Vec<[Vec<Operation>; 2]>,
    pub(super) links: Vec<ElectricalLink>,
    pub(super) incoming: Vec<Vec<usize>>,
    pub(super) watched_wires: Vec<WatchedWire>,
    pub(super) repeaters: Vec<RepeaterRecord>,
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
            let count = group
                .tokens
                .iter()
                .try_fold(1usize, |count, &token| count.checked_mul(self.tokens[token].cells.len() + 1));
            if count != Some(group.inputs.len()) || group.inputs.len() > 4096 {
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
            for step in cells.iter().flat_map(|edges| edges.iter().flatten()) {
                let watched = |node| self.watched_wires.iter().any(|wire| wire.wire == node);
                if step.node >= self.initial.len() || (step.wire && !watched(step.node)) || step.wires.iter().any(|&wire| !watched(wire)) {
                    return Err(ScratchError {
                        position: self.tokens[token].position,
                        reason: "invalid movement step reference",
                    });
                }
            }
        }
        let mut groups = Vec::new();
        let mut links = Vec::new();
        let mut grouped: Vec<(Vec<usize>, Vec<Vec<(usize, ArrangementSource, u8, bool, bool)>>)> = Vec::new();
        for input in &self.input_arrangements {
            let index = if let Some(index) = grouped.iter().position(|(tokens, _)| *tokens == input.tokens) {
                index
            } else {
                grouped.push((input.tokens.clone(), vec![Vec::new(); input.inputs.len()]));
                grouped.len() - 1
            };
            for (state, entries) in input.inputs.iter().enumerate() {
                grouped[index].1[state].extend(
                    entries
                        .iter()
                        .map(|&(source, attenuation)| (input.target, source, attenuation, input.side, input.silent)),
                );
            }
        }
        for (tokens, maps) in grouped {
            let (group, entries) = self.movement_group(&tokens, &maps, groups.len(), links.len());
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
            let (group, entries) = self.movement_group(&[token], &maps, groups.len(), links.len());
            groups.push(group);
            links.extend(entries);
        }
        self.movement_groups = groups;
        self.arrangement_links = links;
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
        Ok(())
    }

    pub(super) fn prepare_projection(&mut self) {
        self.geometry_indices = self.geometry.iter().copied().collect();
        let mut positions = self.positions.clone();
        positions.extend(self.tokens.iter().flat_map(|token| token.cells.iter().copied()));
        positions.extend(self.pistons.iter().flat_map(|piston| [piston.base, piston.home, piston.far]));
        positions.sort_by_key(|position| (position.x, position.y, position.z));
        positions.dedup();
        self.projection_positions = positions;
    }

    fn movement_group(
        &self,
        tokens: &[usize],
        maps: &[Vec<(usize, ArrangementSource, u8, bool, bool)>],
        group: usize,
        link_start: usize,
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
            .map(|&token| {
                let radix = self.tokens[token].cells.len() + 1;
                let mut depart = vec![vec![]; maps.len()];
                let mut arrive = vec![vec![]; maps.len()];
                for state in 0..maps.len() {
                    let digit = state / stride % radix;
                    if digit == 0 {
                        continue;
                    }
                    let empty = state - digit * stride;
                    for (index, &(target, source, attenuation, side, silent)) in conditions.iter().enumerate() {
                        let entry = (target, source, attenuation, side, silent);
                        if maps[state].contains(&entry) != maps[empty].contains(&entry) {
                            let flip = ArrangementFlip {
                                target,
                                source,
                                attenuation,
                                link: link_start + index,
                                entering: maps[empty].contains(&entry),
                            };
                            depart[state].push(flip);
                            arrive[state].push(ArrangementFlip {
                                entering: !flip.entering,
                                ..flip
                            });
                        }
                    }
                }
                let axis = MovementAxis {
                    token,
                    stride,
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
                | Block::RedstoneComparator { .. }
                | Block::RedstoneBlock
                | Block::Piston { .. }
                | Block::Observer { .. }
                | Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. } => {}
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
            ) {
                return Err(invalid("sampled program link source must be lever, button, wire or repeater"));
            }
            if !matches!(
                target,
                Block::RedstoneWire { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::Piston { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            ) {
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
        for programs in &output_programs {
            for operations in programs {
                for operation in operations {
                    match *operation {
                        Operation::Contribution(link) if link >= links.len() => {
                            return Err(invalid("sampled program contribution link is out of bounds"));
                        }
                        Operation::CheckWatchedWire(record) if record >= watched_wires.len() => {
                            return Err(invalid("sampled program watched-wire index is out of bounds"));
                        }
                        Operation::RefreshWire(slot) if !matches!(initial.get(slot), Some(Block::RedstoneWire { .. })) => {
                            return Err(invalid("sampled program RefreshWire target is not a wire"));
                        }
                        Operation::UpdateLamp(slot) if !matches!(initial.get(slot), Some(Block::RedstoneLamp { .. })) => {
                            return Err(invalid("sampled program UpdateLamp target is not a lamp"));
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
                        Operation::UpdateTorch(slot)
                            if !matches!(initial.get(slot), Some(Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. })) =>
                        {
                            return Err(invalid("sampled program UpdateTorch target is not a torch"));
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut incoming = vec![vec![]; initial.len()];
        for (id, link) in links.iter().enumerate() {
            incoming[link.target].push(id);
        }
        let component_indices = positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        Ok(Self {
            comparators: Vec::new(),
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
            token_sources: Vec::new(),
            token_updates: Vec::new(),
            token_watchers: Vec::new(),
            move_steps: Vec::new(),
            input_arrangements: Vec::new(),
            movement_groups: Vec::new(),
            token_movement_groups: Vec::new(),
            arrangement_links: Vec::new(),
            source_watchers: vec![vec![]; initial.len()],
            home_watchers: Vec::new(),
            positions,
            initial,
            sources,
            output_programs,
            links,
            incoming,
            watched_wires,
            repeaters,
        })
    }

    pub(super) fn start(&self) -> Runtime {
        let mut runtime = Runtime {
            program: std::sync::Arc::new(self.clone()),
            piston_state: mchprs_world::PistonState::default(),
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
                                .map_or(0, |cell| cell + 1)
                                * axis.stride
                        })
                        .sum()
                })
                .collect(),
            arrangement_contributions: vec![0; self.arrangement_links.len()],
            piston_events: std::collections::VecDeque::new(),
            queued_piston_events: vec![0; self.pistons.len()],
            motions: Vec::new(),
            motion_time: 0,
            motion_phase: false,
            landing_cursor: None,
            accepted_piston_actions: 0,
            token_movements: 0,
            motion_completions: 0,
            trace_enabled: false,
            trace_events: Vec::new(),
            blocks: self.initial.clone(),
            pending: TickScheduler::default(),
            contributions: self
                .links
                .iter()
                .map(|link| {
                    let strength: u8 = match self.initial[link.source] {
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
            watched_strengths: self
                .watched_wires
                .iter()
                .map(|record| match self.initial[record.wire] {
                    Block::RedstoneWire { wire } => wire.power,
                    _ => unreachable!(),
                })
                .collect(),
            phase: 0,
            updated_in_phase: vec![None; self.initial.len()],
            operations: 0,
            deliveries: 0,
            scheduled_repeater_ticks: 0,
            tick_operations: 0,
            failure: None,
        };
        for (index, link) in self.arrangement_links.iter().enumerate() {
            if link.active[runtime.group_arrangements[link.group]] {
                runtime.arrangement_contributions[index] = runtime.arrangement_output(link.source).saturating_sub(link.attenuation);
            }
        }
        runtime
    }
}
