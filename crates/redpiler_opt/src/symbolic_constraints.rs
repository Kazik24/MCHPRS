//! Conservative ready-state reduction: binary consequences prove constants, never guess memory.
use super::*;

impl Diagram {
    /// Prove fixed inputs from unit/binary consequences of the required conditions.
    /// `None` proves inconsistency; an empty vector means no further constants were proven.
    pub fn forced_inputs(&mut self, conditions: &[Formula]) -> Result<Option<Vec<(InputId, bool)>>, SymbolicError> {
        for &condition in conditions {
            self.validate(condition)?;
        }
        if self.structural {
            return Err(SymbolicError::RequiresCanonicalDiagram);
        }
        if conditions.contains(&Formula::FALSE) {
            return Ok(None);
        }
        let inputs: Vec<_> = self.inputs.iter().copied().collect();
        let ordinals: HashMap<_, _> = inputs.iter().enumerate().map(|(index, &input)| (input, index)).collect();
        let count = inputs.len().checked_mul(2).ok_or(SymbolicError::BudgetExceeded)?;
        let mut edges = vec![Vec::new(); count];
        // A false path forbids its prefix assignments. Longer clauses remain admission checks.
        for &condition in conditions {
            if condition == Formula::TRUE {
                continue;
            }
            let first = self.ordered_decision(condition);
            for (value, child) in [(false, first.low), (true, first.high)] {
                self.charge()?;
                let a = ordinals[&first.variable] * 2 + usize::from(value);
                if child == Formula::FALSE {
                    edges[a].push(a ^ 1);
                } else if child != Formula::TRUE {
                    let second = self.ordered_decision(child);
                    for (value, child) in [(false, second.low), (true, second.high)] {
                        if child == Formula::FALSE {
                            let b = ordinals[&second.variable] * 2 + usize::from(value);
                            edges[a].push(b ^ 1);
                            edges[b].push(a ^ 1);
                        }
                    }
                }
            }
        }
        for outgoing in &mut edges {
            outgoing.sort_unstable();
            outgoing.dedup();
        }
        let mut visited = vec![usize::MAX; count];
        let mut fixed = vec![None; inputs.len()];
        let mut pending = Vec::new();
        for start in 0..count {
            pending.clear();
            pending.push(start);
            while let Some(node) = pending.pop() {
                if visited[node] == start {
                    continue;
                }
                self.charge()?;
                visited[node] = start;
                if node == start ^ 1 {
                    let value = start & 1 == 0;
                    let previous = fixed[start / 2].replace(value);
                    if previous.is_some_and(|previous| previous != value) {
                        return Ok(None);
                    }
                    break;
                }
                pending.extend(&edges[node]);
            }
        }
        Ok(Some(
            inputs
                .into_iter()
                .zip(fixed)
                .filter_map(|(input, value)| value.map(|value| (input, value)))
                .collect(),
        ))
    }

    /// Substitute proven input constants while preserving root order and duplicate roots.
    /// The caller must retain the original conditions that justify these bindings.
    pub fn bind_roots(&mut self, roots: &[Formula], bindings: &[(InputId, bool)]) -> Result<Vec<Formula>, SymbolicError> {
        let mut assignments = HashMap::new();
        for &(input, value) in bindings {
            if !self.inputs.contains(&input) || assignments.insert(input, value).is_some_and(|previous| previous != value) {
                return Err(SymbolicError::InvalidInput);
            }
        }
        for &root in roots {
            self.validate(root)?;
        }
        let original_count = self.nodes.len();
        let mut live = vec![false; original_count];
        let mut pending = roots.to_vec();
        while let Some(value) = pending.pop() {
            if std::mem::replace(&mut live[value.0 as usize], true) || value.constant().is_some() {
                continue;
            }
            self.charge()?;
            match self.nodes[value.0 as usize] {
                Node::Decision(node) => pending.extend([node.low, node.high]),
                Node::Selection(node) => pending.extend([node.condition, node.yes, node.no]),
            }
        }
        let mut translated = vec![Formula::FALSE; original_count];
        translated[1] = Formula::TRUE;
        for index in 2..original_count {
            if !live[index] {
                continue;
            }
            self.charge()?;
            translated[index] = match self.nodes[index] {
                Node::Decision(node) => {
                    let low = translated[node.low.0 as usize];
                    let high = translated[node.high.0 as usize];
                    match assignments.get(&node.variable) {
                        Some(false) => low,
                        Some(true) => high,
                        None => self.decision(node.variable, low, high)?,
                    }
                }
                Node::Selection(node) => self.select(
                    translated[node.condition.0 as usize],
                    translated[node.yes.0 as usize],
                    translated[node.no.0 as usize],
                )?,
            };
        }
        Ok(roots.iter().map(|root| translated[root.0 as usize]).collect())
    }
}
