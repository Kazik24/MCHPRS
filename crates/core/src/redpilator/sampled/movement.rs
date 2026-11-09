//! Movement tables, lowering and validation shared by compiler passes and executors.
use super::{
    Program, ScratchError,
    program::{ArrangementLink, ArrangementSource, valid_notification},
};
use mchprs_blocks::blocks::Block;
use smallvec::SmallVec;

#[derive(Clone)]
pub(super) struct MoveStepSpec {
    /// component notification ID for this movement callback.
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
    /// component notification ID for this movement callback.
    pub(super) node: usize,
    /// indicating this step was selected because the component watches a moved cell.
    pub(super) watcher: bool,
    /// indicating this step evaluates a watched wire.
    pub(super) wire: bool,
    /// slice into `Program::movement_wires` listing wires consulted by this step.
    pub(super) wires: std::ops::Range<usize>,
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

pub(super) const MAX_ARRANGEMENT_STATES: usize = 4096;

fn valid_states(states: &[usize], physical: usize) -> bool {
    states.len() == physical
        && states.first() == Some(&0)
        && states
            .iter()
            .copied()
            .max()
            .is_some_and(|max| max < physical && (0..=max).all(|digit| states.contains(&digit)))
}

impl Program {
    /// Build the power changes caused by moving piston-carried blocks.
    ///
    /// Inputs: This program's sampled input arrangements, token cells, token sources and ordered movement steps.
    /// A token is a movable block; each arrangement describes the inputs at one combination of token positions.
    ///
    /// # Algorithm
    ///
    /// 1. Validate component/token references, state counts and sampled callback ranges.
    /// 2. Merge input maps that use the same tokens and equivalent-position mappings.
    /// 3. Compare each occupied arrangement with that token absent. Store the changed electrical links as
    ///    departure flips, then reverse those changes for arrival flips.
    /// 4. Build per-token, per-cell ranges so execution can apply those flips without searching the world.
    ///
    /// Returns an error for invalid discovery data or tables whose indexes/counters exceed runtime limits.
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
            if !group.token_states.is_empty()
                && (group.token_states.len() != group.tokens.len()
                    || group
                        .token_states
                        .iter()
                        .zip(&group.tokens)
                        .any(|(states, &token)| !valid_states(states, self.tokens[token].cells.len() + 1)))
            {
                return Err(invalid("invalid electrical token states"));
            }
            let count = group.tokens.iter().enumerate().try_fold(1usize, |count, (axis, &token)| {
                count.checked_mul(group.radix(axis, self.tokens[token].cells.len() + 1))
            });
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
                    if !valid_notification(&self.initial, step.node)
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
            let states: Vec<_> = input
                .tokens
                .iter()
                .enumerate()
                .map(|(axis, &token)| {
                    (0..=self.tokens[token].cells.len())
                        .map(|digit| input.digit(axis, digit))
                        .collect::<Vec<_>>()
                })
                .collect();
            let index = if let Some(index) = grouped
                .iter()
                .position(|(tokens, mapping, _)| *tokens == input.tokens && *mapping == states)
            {
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
        self.input_groups_by_target = vec![SmallVec::new(); self.initial.len()];
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
                    self.movement_transitions.push(MovementTransition {
                        group,
                        delta,
                        depart,
                        arrive,
                        states: axis_record.depart.len(),
                    });
                }
                cells.push(start..self.movement_transitions.len());
            }
            self.movement_cells.push(cells);
        }
        self.validate_input_bound()
    }

    /// Lower one set of token arrangements into movement axes and links.
    ///
    /// Inputs: Token IDs, equivalent-state mappings, per-state input maps, group/link offsets and the shared flip
    /// buffer.
    ///
    /// Deduplicate connections, encode each token as a digit with its own stride, and compare each
    /// occupied state with its absent-token state to emit inverse departure/arrival ranges.
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
            .enumerate()
            .map(|(index, &token)| {
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

    /// Store sampled movement callbacks in flat runtime tables.
    ///
    /// Inputs: Nested token/cell lists containing separate departure and arrival step sequences.
    ///
    /// Append each step and its watched-wire IDs to shared buffers, replacing nested vectors with ranges
    /// into those buffers.
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

    /// Check that compiled movement groups, links and transitions agree.
    ///
    /// Inputs: This program's sealed movement axes, flip ranges, token associations and arrangement masks.
    ///
    /// Validate state dimensions and every referenced index, then cross-check each per-cell transition
    /// against its group's stride and stored arrival/departure ranges.
    pub(super) fn validate_movement(&self) -> Result<(), ScratchError> {
        let invalid = |reason| ScratchError { position: None, reason };
        let nodes = self.initial.len();
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
                let records = self
                    .movement_transitions
                    .get(range.clone())
                    .ok_or_else(|| invalid("invalid movement plan range"))?;
                let associations = &self.token_movement_groups[token];
                if records.len() != associations.len() {
                    return Err(invalid("invalid movement plan length"));
                }
                for (record, &(group, axis)) in records.iter().zip(associations) {
                    let axis = self
                        .movement_groups
                        .get(group)
                        .and_then(|g| g.axes.get(axis))
                        .ok_or_else(|| invalid("invalid movement plan axis"))?;
                    let depart = record
                        .depart
                        .checked_add(record.states)
                        .and_then(|end| self.movement_ranges.get(record.depart..end));
                    let arrive = record
                        .arrive
                        .checked_add(record.states)
                        .and_then(|end| self.movement_ranges.get(record.arrive..end));
                    if axis.token != token
                        || record.group != group
                        || axis.states[cell + 1].checked_mul(axis.stride) != Some(record.delta)
                        || depart != Some(axis.depart.as_slice())
                        || arrive != Some(axis.arrive.as_slice())
                    {
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
        Ok(())
    }

    /// Check sampled movement callbacks against the program's components.
    ///
    /// Inputs: This program's movement-step/wire buffers and a component-indexed watched-wire mask.
    ///
    /// Walk token/cell edge ranges, reject missing slices or invalid notifications, and require every wire
    /// callback and referenced wire to be represented by the mask.
    pub(super) fn validate_move_steps(&self, watched: &[bool]) -> Result<(), ScratchError> {
        let invalid = |reason| ScratchError { position: None, reason };
        let nodes = self.initial.len();
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
                    if !valid_notification(&self.initial, step.node)
                        || (step.wire && !watched.get(step.node).copied().unwrap_or(false))
                        || wires.iter().any(|&wire| wire >= nodes || !watched[wire])
                    {
                        return Err(invalid("invalid sealed movement step reference"));
                    }
                }
            }
        }
        Ok(())
    }
}
