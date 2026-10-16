//! Local contradiction proofs for path guards, without constructing a full truth relation.
use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy)]
struct Literal {
    /// This is the shared expression treated as a Boolean variable in the local proof.
    value: Formula,
    /// True uses the expression directly; false uses its negation.
    positive: bool,
}

impl Literal {
    fn new(value: Formula, positive: bool) -> Self {
        Self { value, positive }
    }

    fn read(self, assignments: &HashMap<Formula, bool>) -> Option<bool> {
        self.value
            .constant()
            .or_else(|| assignments.get(&self.value).copied())
            .map(|value| value == self.positive)
    }
}

impl Diagram {
    /// True proves that this guard is impossible; false means unknown, not satisfiable.
    /// `max_nodes` bounds expanded conditionals and `max_work` bounds proof operations.
    /// Unexpanded expressions stay independent variables, so cutting the cone only weakens the proof.
    pub fn prove_false(&self, value: Formula, limits: Limits) -> Result<bool, SymbolicError> {
        self.validate(value)?;
        if let Some(value) = value.constant() {
            return Ok(!value);
        }
        if !self.structural || limits.max_nodes == 0 {
            return Ok(false);
        }
        let mut remaining = limits.max_work;
        let Some(clauses) = self.local_clauses(value, limits.max_nodes, &mut remaining) else {
            return Ok(false);
        };
        let mut assignments = HashMap::from([(value, true)]);
        loop {
            let mut changed = false;
            for clause in &clauses {
                let mut satisfied = false;
                let mut unknown = None;
                let mut unknown_count = 0;
                for &literal in clause {
                    if !spend(&mut remaining) {
                        return Ok(false);
                    }
                    match literal.read(&assignments) {
                        Some(true) => {
                            satisfied = true;
                            break;
                        }
                        Some(false) => {}
                        None => {
                            unknown = Some(literal);
                            unknown_count += 1;
                        }
                    }
                }
                if satisfied {
                    continue;
                }
                if unknown_count == 0 {
                    return Ok(true);
                }
                if unknown_count == 1 {
                    let literal = unknown.expect("one unassigned literal was counted");
                    assignments.insert(literal.value, literal.positive);
                    changed = true;
                }
            }
            if !changed {
                return Ok(false);
            }
        }
    }

    fn local_clauses(&self, root: Formula, max_nodes: usize, remaining: &mut usize) -> Option<Vec<[Literal; 3]>> {
        let mut pending = VecDeque::from([root]);
        let mut seen = HashSet::new();
        let mut clauses = Vec::new();
        while clauses.len() / 4 < max_nodes {
            let Some(value) = pending.pop_front() else {
                break;
            };
            if !spend(remaining) {
                return None;
            }
            if !seen.insert(value) || value.constant().is_some() {
                continue;
            }
            let Node::Selection(node) = self.nodes[value.0 as usize] else {
                continue;
            };
            let positive = |value| Literal::new(value, true);
            let negative = |value| Literal::new(value, false);
            // These four clauses express value <=> (condition ? yes : no).
            clauses.extend([
                [negative(node.condition), negative(node.yes), positive(value)],
                [negative(node.condition), positive(node.yes), negative(value)],
                [positive(node.condition), negative(node.no), positive(value)],
                [positive(node.condition), positive(node.no), negative(value)],
            ]);
            pending.extend([node.condition, node.yes, node.no]);
        }
        Some(clauses)
    }
}

fn spend(remaining: &mut usize) -> bool {
    match remaining.checked_sub(1) {
        Some(next) => {
            *remaining = next;
            true
        }
        None => false,
    }
}
