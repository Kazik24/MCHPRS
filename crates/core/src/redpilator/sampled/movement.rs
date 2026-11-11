//! Movement tables, lowering and validation shared by compiler passes and executors.
use super::{
    Program, ScratchError,
    program::{ArrangementLink, ArrangementSource, valid_notification},
};
use mchprs_blocks::blocks::Block;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ArrangementFlip {
    /// component index into `Program::positions` whose input changes.
    pub(super) target: u32,
    /// component or token whose movement changes the input.
    source: u32,
    /// signal attenuation for this source-to-target connection.
    pub(super) attenuation: u8,
    /// index into `Program::arrangement_links` for the connection being changed.
    pub(super) link: u32,
    /// indicating the source is entering the target's input footprint (`false` means leaving).
    pub(super) entering: bool,
    /// indicating this is a side input rather than the primary input.
    pub(super) side: bool,
    /// Attenuated immutable token output, or zero for component sources; uses the spare byte.
    pub(super) token_contribution: u8,
}

impl ArrangementFlip {
    fn new(target: usize, source: ArrangementSource, attenuation: u8, link: usize, entering: bool, side: bool) -> Result<Self, ScratchError> {
        let overflow = || ScratchError {
            position: None,
            reason: "movement flip index overflow",
        };
        let (source, tag) = match source {
            ArrangementSource::Component(index) => (index, 0),
            ArrangementSource::Token(index) => (index, 0x8000_0000),
        };
        if source >= 0x8000_0000 {
            return Err(overflow());
        }
        Ok(Self {
            target: u32::try_from(target).map_err(|_| overflow())?,
            source: source as u32 | tag,
            attenuation,
            link: u32::try_from(link).map_err(|_| overflow())?,
            entering,
            side,
            token_contribution: 0,
        })
    }

    #[inline]
    pub(super) fn source(self) -> ArrangementSource {
        let index = (self.source & 0x7fff_ffff) as usize;
        if self.source & 0x8000_0000 == 0 {
            ArrangementSource::Component(index)
        } else {
            ArrangementSource::Token(index)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition_program(conditional: bool) -> Program {
        use super::super::{pistons::PistonToken, program::InputArrangement};
        use mchprs_blocks::BlockPos;
        let mut program = Program::new(
            vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10)],
            vec![Block::RedstoneBlock, Block::RedstoneLamp { lit: false }],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        for (block, cells) in [
            (Block::RedstoneBlock, vec![BlockPos::new(12, 11, 10), BlockPos::new(13, 11, 10)]),
            (Block::Stone {}, vec![BlockPos::new(14, 11, 10)]),
        ] {
            program.tokens.push(PistonToken {
                block,
                position: None,
                cells,
            });
        }
        let token = ArrangementSource::Token(0);
        let component = ArrangementSource::Component(0);
        program.input_arrangements.push(InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0, 1],
            token_states: vec![vec![0, 1, 0], vec![0, 1]],
            conditional_sources: vec![token, component],
            inputs: vec![
                vec![],
                vec![(token, 7)],
                vec![(component, 3)],
                vec![(token, if conditional { 8 } else { 7 }), (component, 3)],
            ],
        });
        program.compile_movement_flips().unwrap();
        program.validate().unwrap();
        program
    }

    fn multi_group_program(own_first: bool) -> Program {
        use super::super::program::InputArrangement;
        let mut program = transition_program(false);
        program.initial[1] = Block::from_name("redstone_wire").unwrap();
        let own = InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0],
            token_states: vec![vec![0, 1, 0]],
            conditional_sources: vec![ArrangementSource::Token(0)],
            inputs: vec![vec![], vec![(ArrangementSource::Token(0), 2)]],
        };
        if own_first {
            program.input_arrangements.insert(0, own);
        } else {
            program.input_arrangements.push(own);
        }
        program.compile_movement_flips().unwrap();
        program.validate().unwrap();
        program
    }

    fn assert_movement_state(actual: &super::super::Runtime, expected: &super::super::Runtime) {
        macro_rules! same {
            ($($field:ident),* $(,)?) => { $(assert_eq!(actual.$field, expected.$field, stringify!($field));)* };
        }
        same!(
            blocks,
            contributions,
            arrangement_contributions,
            routing_attenuations,
            comparator_outputs,
            reactive_wire_strengths,
            watched_strengths,
            pending_counts,
            pending_passive_positions,
            phase,
            updated_in_phase,
            piston_extended,
            piston_head_present,
            piston_head_moving,
            piston_base_moving,
            token_positions,
            token_digits,
            group_arrangements,
            piston_events,
            queued_piston_events,
            active_motions,
            free_motion_slots,
            piston_motion,
            cell_motion,
            motion_time,
            motion_phase,
            landing_cursor,
            movement_switched,
            switched,
            movement_changed_wires,
            changed_wires,
            notes,
            note_queued,
            dirty_cells,
            dirty,
            needs_full_publication,
            accepted_piston_actions,
            token_movements,
            motion_completions,
            operations,
            deliveries,
            scheduled_repeater_ticks,
            tick_operations,
            failure,
        );
        assert_eq!(actual.pending_entries(), expected.pending_entries());
        assert_eq!(format!("{:?}", actual.motions), format!("{:?}", expected.motions));
        assert_eq!(format!("{:?}", actual.piston_state), format!("{:?}", expected.piston_state));
        assert!(actual.command_blocks.is_empty() && expected.command_blocks.is_empty());
        assert!(actual.command_output_state.is_none() && expected.command_output_state.is_none());
        assert!(actual.command_emissions.is_empty() && expected.command_emissions.is_empty());
        // Drain copies to compare every analog count, including competing contributions at one level.
        for node in 0..actual.input_levels.len() {
            for side in 0..2 {
                let drain = |runtime: &super::super::Runtime| {
                    let mut input = runtime.input_levels[node][side];
                    let mut histograms = runtime.input_histograms.clone();
                    let mut levels = vec![];
                    while input.active != 0 {
                        let level = input.maximum(&histograms);
                        assert_ne!(level, 0);
                        levels.push(level);
                        input.change(level, 0, &mut histograms);
                    }
                    assert_eq!(input.maximum(&histograms), 0);
                    (input.analog, levels)
                };
                assert_eq!(drain(actual), drain(expected));
            }
        }
    }

    #[test]
    fn sparse_transitions_and_fixed_rows_preserve_every_applicable_ordered_sequence() {
        for conditional in [false, true] {
            let program = transition_program(conditional);
            assert!(program.movement_cells[0][1].is_empty());
            assert_eq!(program.token_movement_groups[0], vec![(0, 0)]);
            for token in 0..2 {
                let record = &program.movement_transitions[program.movement_cells[token][0].start];
                let axis = &program.movement_groups[0].axes[token];
                assert_eq!(record.fixed_state, if conditional { usize::MAX } else { record.delta });
                let radix = axis.states.iter().copied().max().unwrap() + 1;
                for row in 0..axis.depart.len() {
                    if row / axis.stride % radix != 1 {
                        continue;
                    }
                    let selected = if record.fixed_state == usize::MAX { row } else { record.fixed_state };
                    for rows in [&axis.depart, &axis.arrive] {
                        assert!(program.movement_flips[rows[row].clone()] == program.movement_flips[rows[selected].clone()]);
                    }
                }
                let mut corrupt = program.clone();
                corrupt.movement_transitions[program.movement_cells[token][0].start].fixed_state =
                    if conditional { record.delta } else { usize::MAX };
                assert_eq!(corrupt.validate().unwrap_err().reason, "invalid fixed movement state");
            }
            for cell in [0, 1] {
                let mut corrupt = program.clone();
                corrupt.movement_cells[0][cell] = if cell == 0 { 0..0 } else { program.movement_cells[0][0].clone() };
                assert_eq!(corrupt.validate().unwrap_err().reason, "invalid movement plan length");
            }
            let mut corrupt = program.clone();
            let occupied = corrupt.movement_groups[0].axes[0].depart[1].clone();
            let base = corrupt.movement_transitions[corrupt.movement_cells[0][0].start].depart;
            corrupt.movement_ranges[base] = occupied.clone();
            corrupt.movement_groups[0].axes[0].depart[0] = occupied;
            assert_eq!(corrupt.validate().unwrap_err().reason, "nonempty zero movement transition");
        }
    }

    #[test]
    fn fixed_replay_matches_original_selection_for_movements_and_every_budget_prefix() {
        for conditional in [false, true] {
            let program = transition_program(conditional);
            for first in 0..3 {
                for second in 0..2 {
                    for token in 0..2 {
                        let digits = [first, second];
                        let destinations: Vec<_> = if digits[token] == 0 {
                            program.tokens[token].cells.iter().copied().map(Some).collect()
                        } else {
                            vec![None]
                        };
                        for destination in destinations {
                            let mut initial = program.clone();
                            for (record, digit) in initial.tokens.iter_mut().zip(digits) {
                                record.position = if digit == 0 { None } else { Some(record.cells[digit - 1]) };
                            }
                            let mut reference = initial.clone();
                            for record in &mut reference.movement_transitions {
                                record.fixed_state = usize::MAX;
                            }
                            for cells in &mut reference.movement_fixed_ranges {
                                cells.fill(None);
                            }
                            for allowance in 0..=program.movement_flips.len() + 2 {
                                let mut actual = initial.start();
                                let mut expected = reference.start();
                                actual.tick_operations = 1_000_000 - allowance;
                                expected.tick_operations = actual.tick_operations;
                                actual.set_token_pos(token, destination);
                                expected.set_token_pos(token, destination);
                                assert_movement_state(&actual, &expected);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn flat_fixed_ranges_preserve_multiple_groups_empty_plans_and_every_budget_prefix() {
        let program = multi_group_program(true);
        assert_eq!(program.movement_cells[0][0].len(), 2);
        assert_eq!(program.movement_cells[1][0].len(), 1);
        assert!(program.movement_cells[0][1].is_empty());
        assert!(program.movement_fixed_ranges.iter().flatten().all(Option::is_some));
        let empty = program.movement_fixed_ranges[0][1].as_ref().unwrap();
        assert!(empty.iter().all(std::ops::Range::is_empty));
        // Three eligible cells: one multiple-group, one single-group and one empty.
        assert_eq!(program.movement_fixed_ranges.iter().map(Vec::len).sum::<usize>(), 3);
        for first in 0..3 {
            for second in 0..2 {
                let mut initial = program.clone();
                for (record, digit) in initial.tokens.iter_mut().zip([first, second]) {
                    record.position = if digit == 0 { None } else { Some(record.cells[digit - 1]) };
                }
                let mut reference = initial.clone();
                for cells in &mut reference.movement_fixed_ranges {
                    cells.fill(None);
                }
                reference.validate().unwrap();
                for token in 0..2 {
                    let destinations: Vec<_> = if initial.tokens[token].position.is_none() {
                        initial.tokens[token].cells.iter().copied().map(Some).collect()
                    } else {
                        vec![None]
                    };
                    for destination in destinations {
                        for allowance in 0..=program.movement_flips.len() + 2 {
                            let mut actual = initial.start();
                            let mut expected = reference.start();
                            actual.tick_operations = 1_000_000 - allowance;
                            expected.tick_operations = actual.tick_operations;
                            actual.set_token_pos(token, destination);
                            expected.set_token_pos(token, destination);
                            assert_movement_state(&actual, &expected);
                        }
                    }
                }
            }
        }
        let mut rebuilt = program.clone();
        rebuilt.compile_movement_flips().unwrap();
        rebuilt.validate().unwrap();
        assert_eq!(rebuilt.movement_fixed_ranges, program.movement_fixed_ranges);
        assert!(rebuilt.movement_flips == program.movement_flips);
    }

    #[test]
    fn flat_fixed_ranges_preserve_partial_failure_before_invalid_later_group() {
        let program = multi_group_program(true);
        let transitions = program.movement_cells[0][0].clone();
        let later = &program.movement_transitions[transitions.end - 1];
        let mut reference = program.clone();
        reference.movement_fixed_ranges[0][0] = None;
        for allowance in 0..=program.movement_flips.len() + 2 {
            let mut actual = program.start();
            let mut expected = reference.start();
            actual.group_arrangements[later.group] = later.states;
            expected.group_arrangements[later.group] = later.states;
            actual.tick_operations = 1_000_000 - allowance;
            expected.tick_operations = actual.tick_operations;
            actual.set_token_pos(0, Some(program.tokens[0].cells[0]));
            expected.set_token_pos(0, Some(program.tokens[0].cells[0]));
            assert_movement_state(&actual, &expected);
            assert!(actual.failure.is_some());
            assert_eq!(actual.group_arrangements[program.movement_transitions[transitions.start].group], 1);
        }
    }

    #[test]
    fn flat_fixed_ranges_reuse_single_ranges_and_bound_copies_for_aliased_cells() {
        let single = transition_program(false);
        let canonical = single.movement_ranges.iter().map(|range| range.end).max().unwrap_or(0);
        assert_eq!(single.movement_flips.len(), canonical);
        for (token, cells) in single.movement_fixed_ranges.iter().enumerate() {
            for (cell, plan) in cells.iter().enumerate() {
                let plan = plan.as_ref().unwrap();
                let records = &single.movement_transitions[single.movement_cells[token][cell].clone()];
                if let [record] = records {
                    assert_eq!(plan[0], single.movement_ranges[record.depart + record.fixed_state]);
                    assert_eq!(plan[1], single.movement_ranges[record.arrive + record.fixed_state]);
                } else {
                    assert!(records.is_empty() && plan.iter().all(std::ops::Range::is_empty));
                }
            }
        }

        let mut program = multi_group_program(true);
        program.tokens[0].cells = (0..20).map(|x| mchprs_blocks::BlockPos::new(100 + x, 11, 10)).collect();
        for input in &mut program.input_arrangements {
            let axis = input.tokens.iter().position(|&token| token == 0).unwrap();
            input.token_states[axis] = std::iter::once(0).chain(std::iter::repeat_n(1, 20)).collect();
        }
        program.compile_movement_flips().unwrap();
        program.validate().unwrap();
        let canonical = program.movement_ranges.iter().map(|range| range.end).max().unwrap_or(0);
        assert!(program.movement_flips.len() > canonical);
        assert!(program.movement_flips.len() - canonical <= canonical);
        assert!(program.movement_fixed_ranges[0].iter().any(Option::is_some));
        assert!(program.movement_fixed_ranges[0].iter().any(Option::is_none));
        assert!(
            program.movement_fixed_ranges[1][0]
                .as_ref()
                .unwrap()
                .iter()
                .all(|range| range.end <= canonical)
        );
        let plans = program.movement_fixed_ranges.clone();
        let flips = program.movement_flips.clone();
        program.compile_movement_flips().unwrap();
        program.validate().unwrap();
        assert_eq!(program.movement_fixed_ranges, plans);
        assert!(program.movement_flips == flips);
    }

    #[test]
    fn flat_fixed_range_validation_rejects_dimensions_ineligible_records_and_changed_copies() {
        let program = multi_group_program(true);
        let mut corrupt = program.clone();
        corrupt.movement_fixed_ranges.pop();
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid fixed movement plan dimensions");
        let mut corrupt = program.clone();
        corrupt.movement_fixed_ranges[0].pop();
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid fixed movement plan dimensions");
        let mut corrupt = program.clone();
        corrupt.movement_fixed_ranges[0][0].as_mut().unwrap()[0].end = usize::MAX;
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid fixed movement flip range");
        let mut corrupt = program.clone();
        let flip = corrupt.movement_fixed_ranges[0][0].as_ref().unwrap()[0].start;
        corrupt.movement_flips[flip].token_contribution ^= 1;
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid fixed movement flip sequence");
        let mut conditional = transition_program(true);
        assert!(conditional.movement_fixed_ranges[0][0].is_none());
        conditional.movement_fixed_ranges[0][0] = Some([0..0, 0..0]);
        assert_eq!(conditional.validate().unwrap_err().reason, "invalid fixed movement plan");
        let mut nonascending = multi_group_program(false);
        assert!(nonascending.movement_fixed_ranges[0][0].is_none());
        nonascending.movement_fixed_ranges[0][0] = Some([0..0, 0..0]);
        assert_eq!(nonascending.validate().unwrap_err().reason, "invalid fixed movement plan");
    }

    #[test]
    fn lowered_flips_cache_token_outputs_and_preserve_inverse_records() {
        use super::super::pistons::PistonToken;
        use super::super::program::InputArrangement;
        use mchprs_blocks::BlockPos;

        let mut program = Program::new(
            vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10)],
            vec![Block::RedstoneBlock, Block::RedstoneLamp { lit: false }],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        for (index, block) in [Block::RedstoneBlock, Block::Stone {}].into_iter().enumerate() {
            program.tokens.push(PistonToken {
                block,
                position: None,
                cells: vec![BlockPos::new(10 + index as i32, 11, 10)],
            });
        }
        program.input_arrangements.push(InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0],
            token_states: vec![vec![0, 1]],
            conditional_sources: vec![ArrangementSource::Component(0), ArrangementSource::Token(0), ArrangementSource::Token(1)],
            inputs: vec![
                vec![],
                vec![
                    (ArrangementSource::Component(0), 2),
                    (ArrangementSource::Token(0), 0),
                    (ArrangementSource::Token(0), 1),
                    (ArrangementSource::Token(0), 15),
                    (ArrangementSource::Token(1), 3),
                ],
            ],
        });
        program.compile_movement_flips().unwrap();
        program.validate().unwrap();
        let axis = &program.movement_groups[0].axes[0];
        let departing = &program.movement_flips[axis.depart[1].clone()];
        let arriving = &program.movement_flips[axis.arrive[1].clone()];
        assert_eq!(departing.len(), 5);
        assert_eq!(arriving.len(), 5);
        for (depart, arrive) in departing.iter().zip(arriving) {
            let expected = match depart.source() {
                ArrangementSource::Token(0) => 15u8.saturating_sub(depart.attenuation),
                _ => 0,
            };
            assert_eq!(depart.token_contribution, expected);
            assert_eq!(arrive.token_contribution, expected);
            assert_eq!(
                (depart.target, depart.link, depart.source(), depart.attenuation, depart.side),
                (arrive.target, arrive.link, arrive.source(), arrive.attenuation, arrive.side)
            );
            assert!(!depart.entering && arrive.entering);
        }
        let token_flip = axis.depart[1]
            .clone()
            .find(|&index| program.movement_flips[index].source() == ArrangementSource::Token(0))
            .unwrap();
        let mut corrupt = program.clone();
        corrupt.movement_flips[token_flip].token_contribution = 0;
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid movement flip token contribution");
        let link = corrupt.movement_flips[token_flip].link as usize;
        corrupt.movement_flips[token_flip].source = 0x8000_0063;
        corrupt.arrangement_links[link].source = ArrangementSource::Token(99);
        assert_eq!(corrupt.validate().unwrap_err().reason, "invalid movement flip token source");
    }

    #[test]
    fn compact_flips_preserve_ids_and_reject_overflow() {
        assert_eq!(std::mem::size_of::<ArrangementFlip>(), 16);
        for source in [
            ArrangementSource::Component(0),
            ArrangementSource::Component(0x7fff_ffff),
            ArrangementSource::Token(0),
            ArrangementSource::Token(0x7fff_ffff),
        ] {
            for entering in [false, true] {
                for side in [false, true] {
                    let flip = ArrangementFlip::new(u32::MAX as usize, source, 15, u32::MAX as usize, entering, side).unwrap();
                    assert_eq!(flip.source(), source);
                    assert_eq!(flip.target, u32::MAX);
                    assert_eq!(flip.link, u32::MAX);
                    assert_eq!((flip.attenuation, flip.entering, flip.side), (15, entering, side));
                    assert_eq!(flip.token_contribution, 0);
                }
            }
        }
        for source in [ArrangementSource::Component(0x8000_0000), ArrangementSource::Token(0x8000_0000)] {
            assert!(ArrangementFlip::new(0, source, 0, 0, false, false).is_err());
        }
        if usize::BITS > 32 {
            assert!(ArrangementFlip::new(usize::MAX, ArrangementSource::Component(0), 0, 0, false, false).is_err());
            assert!(ArrangementFlip::new(0, ArrangementSource::Token(0), 0, usize::MAX, false, false).is_err());
        }
    }
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
    /// Canonical selected row for fixed ordered flips, or usize::MAX for state-dependent selection.
    pub(super) fixed_state: usize,
}

#[derive(Clone)]
pub(super) struct MovementGroup {
    /// token axes and their movement-step ranges for one joint arrangement group.
    pub(super) axes: Vec<MovementAxis>,
}

pub(super) const MAX_ARRANGEMENT_STATES: usize = 4096;

fn empty_zero_rows(axis: &MovementAxis) -> bool {
    let radix = axis.states.iter().copied().max().unwrap_or(0) + 1;
    (0..axis.depart.len())
        .filter(|&state| state / axis.stride % radix == 0)
        .all(|state| axis.depart[state].is_empty() && axis.arrive[state].is_empty())
}

fn fixed_movement_state(axis: &MovementAxis, delta: usize, flips: &[ArrangementFlip]) -> usize {
    let radix = axis.states.iter().copied().max().unwrap_or(0) + 1;
    let digit = delta / axis.stride;
    let depart = &flips[axis.depart[delta].clone()];
    let arrive = &flips[axis.arrive[delta].clone()];
    if (0..axis.depart.len())
        .filter(|&state| state / axis.stride % radix == digit)
        .all(|state| flips[axis.depart[state].clone()] == *depart && flips[axis.arrive[state].clone()] == *arrive)
    {
        delta
    } else {
        usize::MAX
    }
}

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
        let mut group_index = HashMap::new();
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
            let key = (input.tokens.clone(), states);
            let index = *group_index.entry(key.clone()).or_insert_with(|| {
                grouped.push((key.0, key.1, vec![Vec::new(); input.inputs.len()]));
                grouped.len() - 1
            });
            for (state, entries) in input.inputs.iter().enumerate() {
                grouped[index].2[state].extend(
                    entries
                        .iter()
                        .map(|&(source, attenuation)| (input.target, source, attenuation, input.side, input.silent)),
                );
            }
        }
        drop(group_index);
        for (tokens, states, maps) in grouped {
            let (group, entries) = self.movement_group(&tokens, &states, &maps, groups.len(), links.len(), &mut flips)?;
            groups.push(group);
            links.extend(entries);
        }
        let conditional: HashSet<_> = self
            .input_arrangements
            .iter()
            .filter(|group| !group.side)
            .flat_map(|group| group.conditional_sources.iter().map(move |&source| (group.target, source)))
            .collect();
        for (token, cells) in self.token_sources.iter().enumerate() {
            let mut maps = vec![vec![]];
            maps.extend(cells.iter().map(|links| {
                links
                    .iter()
                    .filter_map(|&(target, attenuation)| {
                        let source = ArrangementSource::Token(token);
                        (!conditional.contains(&(target, source))).then_some((
                            target,
                            source,
                            attenuation,
                            false,
                            matches!(self.initial[target], Block::Piston { .. }),
                        ))
                    })
                    .collect()
            }));
            let states = vec![(0..=cells.len()).collect()];
            let (group, entries) = self.movement_group(&[token], &states, &maps, groups.len(), links.len(), &mut flips)?;
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
                    if delta == 0 {
                        if !empty_zero_rows(axis_record) {
                            return Err(ScratchError {
                                position: self.tokens[token].position,
                                reason: "nonempty zero movement transition",
                            });
                        }
                        continue;
                    }
                    let (depart, arrive) = tables[group][axis];
                    self.movement_transitions.push(MovementTransition {
                        group,
                        delta,
                        depart,
                        arrive,
                        states: axis_record.depart.len(),
                        fixed_state: fixed_movement_state(axis_record, delta, &self.movement_flips),
                    });
                }
                cells.push(start..self.movement_transitions.len());
            }
            self.movement_cells.push(cells);
        }
        self.movement_fixed_ranges.clear();
        // Use the existing admission estimate's reserved spare flip capacity.
        let mut spare_flips = self.movement_flips.len();
        for cells in &self.movement_cells {
            let mut fixed = Vec::with_capacity(cells.len());
            for range in cells {
                let records = &self.movement_transitions[range.clone()];
                if records.iter().any(|record| record.fixed_state == usize::MAX) || records.windows(2).any(|pair| pair[0].group >= pair[1].group) {
                    fixed.push(None);
                    continue;
                }
                if records.is_empty() {
                    fixed.push(Some([0..0, 0..0]));
                    continue;
                }
                if let [record] = records {
                    fixed.push(Some([
                        self.movement_ranges[record.depart + record.fixed_state].clone(),
                        self.movement_ranges[record.arrive + record.fixed_state].clone(),
                    ]));
                    continue;
                }
                let lengths: [Option<usize>; 2] = std::array::from_fn(|edge| {
                    records.iter().try_fold(0usize, |length, record| {
                        let base = if edge == 0 { record.depart } else { record.arrive };
                        length.checked_add(self.movement_ranges[base + record.fixed_state].len())
                    })
                });
                let needed = lengths[0].zip(lengths[1]).and_then(|(depart, arrive)| depart.checked_add(arrive));
                let Some(needed) = needed.filter(|&needed| needed <= spare_flips) else {
                    fixed.push(None);
                    continue;
                };
                spare_flips -= needed;
                fixed.push(Some(std::array::from_fn(|edge| {
                    let start = self.movement_flips.len();
                    for record in records {
                        let base = if edge == 0 { record.depart } else { record.arrive };
                        self.movement_flips
                            .extend_from_within(self.movement_ranges[base + record.fixed_state].clone());
                    }
                    start..self.movement_flips.len()
                })));
            }
            self.movement_fixed_ranges.push(fixed);
        }
        self.movement_flips.shrink_to_fit();
        self.movement_ranges.shrink_to_fit();
        self.movement_transitions.shrink_to_fit();
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
    ) -> Result<(MovementGroup, Vec<ArrangementLink>), ScratchError> {
        let mut conditions = Vec::new();
        let mut condition_ids = HashMap::new();
        let membership: Vec<HashSet<usize>> = maps
            .iter()
            .map(|map| {
                map.iter()
                    .map(|&entry| {
                        *condition_ids.entry(entry).or_insert_with(|| {
                            conditions.push(entry);
                            conditions.len() - 1
                        })
                    })
                    .collect()
            })
            .collect();
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
                    for (index, &(target, source, attenuation, side, _silent)) in conditions.iter().enumerate() {
                        if membership[state].contains(&index) != membership[empty].contains(&index) {
                            let mut flip =
                                ArrangementFlip::new(target, source, attenuation, link_start + index, membership[empty].contains(&index), side)?;
                            if let ArrangementSource::Token(token) = source {
                                flip.token_contribution =
                                    (u8::from(matches!(self.tokens[token].block, Block::RedstoneBlock)) * 15).saturating_sub(attenuation);
                            }
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
                Ok(axis)
            })
            .collect::<Result<Vec<_>, ScratchError>>()?;
        let links = conditions
            .iter()
            .enumerate()
            .map(|(index, &(target, source, attenuation, side, silent))| ArrangementLink {
                target,
                side,
                silent,
                source,
                attenuation,
                group,
                active: membership.iter().map(|map| map.contains(&index)).collect(),
            })
            .collect();
        Ok((MovementGroup { axes }, links))
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
                            .get(flip.link as usize)
                            .ok_or_else(|| invalid("invalid movement flip link"))?;
                        if link.group != group
                            || flip.target as usize != link.target
                            || flip.source() != link.source
                            || flip.attenuation != link.attenuation
                            || flip.side != link.side
                        {
                            return Err(invalid("movement flip does not match its link"));
                        }
                        let token_contribution = match flip.source() {
                            ArrangementSource::Component(_) => 0,
                            ArrangementSource::Token(token) => {
                                let token = self.tokens.get(token).ok_or_else(|| invalid("invalid movement flip token source"))?;
                                (u8::from(matches!(token.block, Block::RedstoneBlock)) * 15).saturating_sub(flip.attenuation)
                            }
                        };
                        if flip.token_contribution != token_contribution {
                            return Err(invalid("invalid movement flip token contribution"));
                        }
                    }
                }
            }
            group_states.push(states);
        }
        if self.movement_cells.len() != self.tokens.len() || self.token_movement_groups.len() != self.tokens.len() {
            return Err(invalid("invalid movement plan dimensions"));
        }
        if self.movement_fixed_ranges.len() != self.tokens.len() {
            return Err(invalid("invalid fixed movement plan dimensions"));
        }
        for (token, cells) in self.movement_cells.iter().enumerate() {
            if cells.len() != self.tokens[token].cells.len() {
                return Err(invalid("invalid movement plan cells"));
            }
            if self.movement_fixed_ranges[token].len() != cells.len() {
                return Err(invalid("invalid fixed movement plan dimensions"));
            }
            for (cell, range) in cells.iter().enumerate() {
                let records = self
                    .movement_transitions
                    .get(range.clone())
                    .ok_or_else(|| invalid("invalid movement plan range"))?;
                let associations = &self.token_movement_groups[token];
                let mut records = records.iter();
                for &(group, axis) in associations {
                    let axis = self
                        .movement_groups
                        .get(group)
                        .and_then(|g| g.axes.get(axis))
                        .ok_or_else(|| invalid("invalid movement plan axis"))?;
                    if axis.token != token {
                        return Err(invalid("invalid movement plan axis"));
                    }
                    let delta = axis.states[cell + 1]
                        .checked_mul(axis.stride)
                        .ok_or_else(|| invalid("invalid movement plan transition"))?;
                    if delta == 0 {
                        if !empty_zero_rows(axis) {
                            return Err(invalid("nonempty zero movement transition"));
                        }
                        continue;
                    }
                    let record = records.next().ok_or_else(|| invalid("invalid movement plan length"))?;
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
                        || record.delta != delta
                        || record.states != axis.depart.len()
                        || depart != Some(axis.depart.as_slice())
                        || arrive != Some(axis.arrive.as_slice())
                    {
                        return Err(invalid("invalid movement plan transition"));
                    }
                    if record.fixed_state != fixed_movement_state(axis, delta, &self.movement_flips) {
                        return Err(invalid("invalid fixed movement state"));
                    }
                }
                if records.next().is_some() {
                    return Err(invalid("invalid movement plan length"));
                }
                if let Some(edges) = &self.movement_fixed_ranges[token][cell] {
                    let records = &self.movement_transitions[range.clone()];
                    if records.iter().any(|record| record.fixed_state == usize::MAX) || records.windows(2).any(|pair| pair[0].group >= pair[1].group)
                    {
                        return Err(invalid("invalid fixed movement plan"));
                    }
                    for (edge, range) in edges.iter().enumerate() {
                        let Some(flips) = self.movement_flips.get(range.clone()) else {
                            return Err(invalid("invalid fixed movement flip range"));
                        };
                        let canonical = records.iter().flat_map(|record| {
                            let base = if edge == 0 { record.depart } else { record.arrive };
                            self.movement_flips[self.movement_ranges[base + record.fixed_state].clone()].iter()
                        });
                        if !flips.iter().eq(canonical) {
                            return Err(invalid("invalid fixed movement flip sequence"));
                        }
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
