//! Fold electrically equivalent token digits inside existing ordered movement groups.
use super::{Program, ScratchError, movement::MovementGroup, program::ArrangementLink};

fn fold_group(group: &mut MovementGroup, links: &mut [ArrangementLink], ids: &[usize]) -> usize {
    let count = group.axes.first().map_or(1, |axis| axis.depart.len());
    let mut rows: Vec<_> = (0..count).collect();
    let mut stride = 1;
    for axis in &mut group.axes {
        let radix = axis.states.iter().copied().max().unwrap_or(0) + 1;
        let mut representatives = Vec::new();
        let mut digits = vec![0; radix];
        for digit in 0..radix {
            digits[digit] = representatives
                .iter()
                .position(|&other| {
                    (0..rows.len()).filter(|state| state / stride % radix == 0).all(|state| {
                        ids.iter()
                            .all(|&id| links[id].active[rows[state + digit * stride]] == links[id].active[rows[state + other * stride]])
                    })
                })
                .unwrap_or_else(|| {
                    representatives.push(digit);
                    representatives.len() - 1
                });
        }
        if representatives.len() != radix {
            let mut retained = Vec::with_capacity(rows.len() / radix * representatives.len());
            for base in (0..rows.len()).step_by(stride * radix) {
                for &digit in &representatives {
                    retained.extend((0..stride).map(|offset| rows[base + digit * stride + offset]));
                }
            }
            rows = retained;
            for digit in &mut axis.states {
                *digit = digits[*digit];
            }
        }
        axis.stride = stride;
        stride *= representatives.len();
    }
    for axis in &mut group.axes {
        axis.depart = rows.iter().map(|&state| axis.depart[state].clone()).collect();
        axis.arrive = rows.iter().map(|&state| axis.arrive[state].clone()).collect();
    }
    for &id in ids {
        links[id].active = rows.iter().map(|&state| links[id].active[state]).collect();
    }
    count - rows.len()
}

impl Program {
    pub(crate) fn fold_movement_states(&mut self) -> Result<(), ScratchError> {
        if !self.relaxed_mode {
            return Err(ScratchError {
                position: None,
                reason: "movement state folding requires relaxed execution",
            });
        }
        if self.movement_folding {
            return Ok(());
        }
        let before = self.movement_transitions.len();
        let mut ids = vec![Vec::new(); self.movement_groups.len()];
        for (id, link) in self.arrangement_links.iter().enumerate() {
            ids[link.group].push(id);
        }
        let mut removed_rows = 0;
        // Preserve group/link identities: regrouping readers could change callback order.
        for (group, ids) in self.movement_groups.iter_mut().zip(ids) {
            removed_rows += fold_group(group, &mut self.arrangement_links, &ids);
        }
        self.index_movement_plans()?;
        self.validate()?;
        self.movement_folding = true;
        if std::env::var_os("MCHPRS_REDPILATOR_PHASE_TIMINGS").is_some() {
            eprintln!(
                "Redpilator movement folding: removed_rows={removed_rows} removed_transitions={}",
                before - self.movement_transitions.len()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        movement::MoveStepSpec,
        pistons::PistonToken,
        program::{ArrangementSource, InputArrangement, WatchedWire},
    };
    use super::*;
    use mchprs_blocks::{BlockPos, blocks::Block};

    fn fixture(nonzero_equivalent: bool) -> Program {
        let mut program = Program::new(
            (10..14).map(|x| BlockPos::new(x, 10, 10)).collect(),
            vec![
                Block::RedstoneBlock,
                Block::from_name("redstone_wire").unwrap(),
                Block::from_name("redstone_wire").unwrap(),
                Block::from_name("observer").unwrap(),
            ],
            vec![],
            vec![[vec![], vec![]]; 4],
            vec![],
            vec![WatchedWire { wire: 1, watchers: vec![] }, WatchedWire { wire: 2, watchers: vec![] }],
            vec![],
        )
        .unwrap();
        for cells in [
            vec![BlockPos::new(14, 10, 10), BlockPos::new(15, 10, 10)],
            vec![BlockPos::new(16, 10, 10)],
        ] {
            program.tokens.push(PistonToken {
                block: Block::Stone {},
                position: None,
                cells,
            });
        }
        let a = vec![(ArrangementSource::Component(0), 0)];
        let inputs = if nonzero_equivalent {
            vec![vec![], a.clone(), a.clone(), vec![], a.clone(), a]
        } else {
            vec![vec![], a.clone(), vec![], a.clone(), a.clone(), a]
        };
        program.input_arrangements.push(InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0, 1],
            token_states: vec![vec![0, 1, 2], vec![0, 1]],
            conditional_sources: vec![ArrangementSource::Component(0)],
            inputs,
        });
        program.set_move_steps(
            program
                .tokens
                .iter()
                .map(|token| {
                    token
                        .cells
                        .iter()
                        .map(|_| {
                            std::array::from_fn(|_| {
                                vec![
                                    MoveStepSpec {
                                        node: 2,
                                        watcher: false,
                                        wire: true,
                                        wires: vec![],
                                    },
                                    MoveStepSpec {
                                        node: 0,
                                        watcher: false,
                                        wire: false,
                                        wires: vec![2],
                                    },
                                    MoveStepSpec {
                                        node: 1,
                                        watcher: false,
                                        wire: true,
                                        wires: vec![],
                                    },
                                    MoveStepSpec {
                                        node: 3,
                                        watcher: true,
                                        wire: false,
                                        wires: vec![],
                                    },
                                ]
                            })
                        })
                        .collect()
                })
                .collect(),
        );
        program.compile_movement_flips().unwrap();
        program.prepare_projection();
        program.enable_relaxed_wire_shapes().unwrap();
        program
    }

    fn equal_runtime(actual: &super::super::Runtime, expected: &super::super::Runtime) {
        assert_eq!(actual.blocks, expected.blocks);
        assert_eq!(actual.arrangement_contributions, expected.arrangement_contributions);
        assert_eq!(actual.watched_strengths, expected.watched_strengths);
        assert_eq!(actual.pending_entries(), expected.pending_entries());
        assert_eq!(actual.piston_events, expected.piston_events);
        assert_eq!(actual.queued_piston_events, expected.queued_piston_events);
        assert_eq!(actual.updated_in_phase, expected.updated_in_phase);
        assert_eq!(actual.token_positions, expected.token_positions);
        assert_eq!(actual.token_digits, expected.token_digits);
        assert_eq!(actual.tick_operations, expected.tick_operations);
        assert_eq!(actual.operations, expected.operations);
        assert_eq!(actual.failure, expected.failure);
        for node in 0..actual.blocks.len() {
            assert_eq!(actual.input_strength(node), expected.input_strength(node));
            assert_eq!(actual.has_input(node), expected.has_input(node));
        }
    }

    #[test]
    fn fold_preserves_every_state_edge_callback_and_budget_prefix() {
        for nonzero in [false, true] {
            let baseline = fixture(nonzero);
            for first in 0..3 {
                for second in 0..2 {
                    let mut initial = baseline.clone();
                    for (token, digit) in initial.tokens.iter_mut().zip([first, second]) {
                        token.position = if digit == 0 { None } else { Some(token.cells[digit - 1]) };
                    }
                    let mut optimized = initial.clone();
                    let before = optimized.movement_transitions.len();
                    optimized.fold_movement_states().unwrap();
                    assert!(optimized.movement_folding && optimized.movement_transitions.len() < before);
                    let once = optimized.movement_transitions.len();
                    optimized.fold_movement_states().unwrap();
                    assert_eq!(optimized.movement_transitions.len(), once);
                    for token in 0..2 {
                        let destinations = if initial.tokens[token].position.is_some() {
                            vec![None]
                        } else {
                            initial.tokens[token].cells.iter().copied().map(Some).collect()
                        };
                        for position in destinations {
                            for allowance in 0..16 {
                                let mut actual = optimized.start();
                                let mut expected = initial.start();
                                actual.tick_operations = 1_000_000 - allowance;
                                expected.tick_operations = actual.tick_operations;
                                actual.set_token_pos(token, position);
                                expected.set_token_pos(token, position);
                                equal_runtime(&actual, &expected);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fold_checks_other_axes_and_every_reader_in_the_group() {
        let mut program = fixture(false);
        program.input_arrangements[0].inputs[5].clear();
        program.compile_movement_flips().unwrap();
        let before = program.movement_groups[0].axes[0].states.clone();
        program.fold_movement_states().unwrap();
        assert_eq!(program.movement_groups[0].axes[0].states, before);
        let mut program = fixture(false);
        let mut other = program.input_arrangements[0].clone();
        other.target = 2;
        other.inputs[2] = vec![(ArrangementSource::Component(0), 0)];
        program.input_arrangements.push(other);
        program.compile_movement_flips().unwrap();
        let before = program.movement_groups[0].axes[0].states.clone();
        program.fold_movement_states().unwrap();
        assert_eq!(program.movement_groups[0].axes[0].states, before);
    }

    #[test]
    fn strict_programs_are_rejected_and_relowering_clears_the_cache_marker() {
        let mut program = fixture(false);
        program.relaxed_mode = false;
        assert!(program.fold_movement_states().is_err());
        program.relaxed_mode = true;
        program.fold_movement_states().unwrap();
        program.compile_movement_flips().unwrap();
        assert!(!program.movement_folding);
        program.fold_movement_states().unwrap();
        assert!(program.movement_folding);
    }
}
