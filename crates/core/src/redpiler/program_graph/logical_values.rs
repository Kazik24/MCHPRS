//! Incremental Boolean evaluation for settled logical programs.
use super::{LayerExpr, LayerExprId, ReducedLayerProgram};
#[cfg(test)]
use super::{LayerInput, LayerInputId};
#[cfg(test)]
use std::cmp::Reverse;
#[cfg(test)]
use std::collections::BinaryHeap;

#[derive(Debug)]
struct DirtyFrontier {
    words: Vec<u64>,
    summaries: Vec<Vec<u64>>,
}

impl DirtyFrontier {
    fn new(expression_count: usize) -> Self {
        let words = vec![0; expression_count.div_ceil(u64::BITS as usize)];
        let mut summaries = Vec::new();
        let mut child_words = words.len();
        while child_words != 0 {
            let summary_words = child_words.div_ceil(u64::BITS as usize);
            summaries.push(vec![0; summary_words]);
            if summary_words == 1 {
                break;
            }
            child_words = summary_words;
        }
        Self { words, summaries }
    }

    fn is_empty(&self) -> bool {
        self.summaries.last().is_none_or(|top| top[0] == 0)
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.words.fill(0);
        for level in &mut self.summaries {
            level.fill(0);
        }
    }

    fn set(&mut self, index: usize) {
        let word = index / u64::BITS as usize;
        let mask = 1 << (index % u64::BITS as usize);
        let was_empty = self.words[word] == 0;
        self.words[word] |= mask;
        if was_empty {
            self.set_summary(0, word);
        }
    }

    fn pop_min(&mut self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let mut index = self.summaries.last()?.first()?.trailing_zeros() as usize;
        for level in (0..self.summaries.len() - 1).rev() {
            let word = self.summaries[level][index];
            index = index * u64::BITS as usize + word.trailing_zeros() as usize;
        }
        let word = self.words[index];
        let bit = word.trailing_zeros() as usize;
        self.words[index] &= !(1 << bit);
        if self.words[index] == 0 {
            self.clear_summary(0, index);
        }
        Some(index * u64::BITS as usize + bit)
    }

    fn set_summary(&mut self, level: usize, index: usize) {
        let word_index = index / u64::BITS as usize;
        let mask = 1 << (index % u64::BITS as usize);
        let was_empty = self.summaries[level][word_index] == 0;
        self.summaries[level][word_index] |= mask;
        if was_empty && level + 1 < self.summaries.len() {
            self.set_summary(level + 1, word_index);
        }
    }

    fn clear_summary(&mut self, level: usize, index: usize) {
        let word_index = index / u64::BITS as usize;
        let mask = 1 << (index % u64::BITS as usize);
        self.summaries[level][word_index] &= !mask;
        if self.summaries[level][word_index] == 0 && level + 1 < self.summaries.len() {
            self.clear_summary(level + 1, word_index);
        }
    }
}

/// Cached values for one immutable reduced layer program.
///
/// The program is owned by the evaluator so cached expression values cannot be
/// reused accidentally with a different expression graph.
#[derive(Debug)]
pub(crate) struct LogicalValues {
    program: ReducedLayerProgram,
    user_offsets: Box<[usize]>,
    users: Box<[usize]>,
    input_offsets: Box<[usize]>,
    input_expressions: Box<[usize]>,
    values: Vec<bool>,
    inputs: Vec<bool>,
    frontier: DirtyFrontier,
    initialized: bool,
    #[cfg(test)]
    heap_reference: bool,
    #[cfg(test)]
    heap_dirty: Vec<bool>,
    #[cfg(test)]
    heap_frontier: BinaryHeap<Reverse<usize>>,
}

impl LogicalValues {
    /// Validate and index a program once, retaining storage for future evaluations.
    pub(crate) fn new(program: &ReducedLayerProgram) -> Result<Self, String> {
        let expression_count = program.expressions.len();
        let input_count = program.inputs.len();
        if program.roots.iter().any(|(_, root)| root.0 as usize >= expression_count) {
            return Err("logical program has a missing root expression".into());
        }
        let mut user_counts = vec![0usize; expression_count];
        let mut input_counts = vec![0usize; input_count];

        for (index, expression) in program.expressions.iter().enumerate() {
            match expression {
                LayerExpr::Constant(_) => {}
                LayerExpr::Input(input) => {
                    let slot = input.0 as usize;
                    let Some(count) = input_counts.get_mut(slot) else {
                        return Err(format!("logical input {slot} is out of range"));
                    };
                    *count += 1;
                }
                LayerExpr::Not(child) => add_operand_user(*child, index, &mut user_counts)?,
                LayerExpr::All(children) | LayerExpr::Any(children) => {
                    for &child in children.iter() {
                        add_operand_user(child, index, &mut user_counts)?;
                    }
                }
            }
        }

        let user_offsets = prefix_offsets(&user_counts);
        let input_offsets = prefix_offsets(&input_counts);
        let mut users = vec![0usize; user_offsets[expression_count]];
        let mut input_expressions = vec![0usize; input_offsets[input_count]];
        let mut user_cursor = user_offsets[..expression_count].to_vec();
        let mut input_cursor = input_offsets[..input_count].to_vec();

        for (index, expression) in program.expressions.iter().enumerate() {
            match expression {
                LayerExpr::Constant(_) => {}
                LayerExpr::Input(input) => {
                    let slot = input.0 as usize;
                    input_expressions[input_cursor[slot]] = index;
                    input_cursor[slot] += 1;
                }
                LayerExpr::Not(child) => {
                    let operand = child.0 as usize;
                    users[user_cursor[operand]] = index;
                    user_cursor[operand] += 1;
                }
                LayerExpr::All(children) | LayerExpr::Any(children) => {
                    for child in children.iter() {
                        let operand = child.0 as usize;
                        users[user_cursor[operand]] = index;
                        user_cursor[operand] += 1;
                    }
                }
            }
        }

        Ok(Self {
            program: program.clone(),
            user_offsets: user_offsets.into_boxed_slice(),
            users: users.into_boxed_slice(),
            input_offsets: input_offsets.into_boxed_slice(),
            input_expressions: input_expressions.into_boxed_slice(),
            values: vec![false; expression_count],
            inputs: vec![false; input_count],
            frontier: DirtyFrontier::new(expression_count),
            initialized: false,
            #[cfg(test)]
            heap_reference: false,
            #[cfg(test)]
            heap_dirty: vec![false; expression_count],
            #[cfg(test)]
            heap_frontier: BinaryHeap::new(),
        })
    }

    /// Evaluate changed inputs and only the expressions whose values can change.
    /// Returns the number of expression operations evaluated.
    pub(crate) fn evaluate(&mut self, inputs: &[bool]) -> Result<usize, &'static str> {
        if inputs.len() != self.program.inputs.len() {
            return Err("logical input count does not match the program");
        }

        if !self.initialized {
            self.inputs.copy_from_slice(inputs);
            for index in 0..self.program.expressions.len() {
                self.values[index] = self.evaluate_expression(index, inputs)?;
            }
            self.initialized = true;
            return Ok(self.program.expressions.len());
        }

        #[cfg(test)]
        if self.heap_reference {
            return self.evaluate_heap_reference(inputs);
        }

        debug_assert!(self.frontier.is_empty());
        for input in 0..self.inputs.len() {
            if self.inputs[input] == inputs[input] {
                continue;
            }
            let start = self.input_offsets[input];
            let end = self.input_offsets[input + 1];
            for offset in start..end {
                self.frontier.set(self.input_expressions[offset]);
            }
        }

        self.inputs.copy_from_slice(inputs);
        self.propagate_frontier()
    }

    /// Update only the listed dense input slots, validating the entire batch before mutation.
    /// This is for runtime event cuts where only one or a few logical state bits changed.
    #[cfg(test)]
    pub(crate) fn evaluate_changes(&mut self, changes: &[(LayerInputId, bool)]) -> Result<usize, &'static str> {
        if !self.initialized {
            return Err("logical input changes require an initial evaluation");
        }
        for &(input, _) in changes {
            if input.0 as usize >= self.inputs.len() {
                return Err("logical input change is out of range");
            }
        }
        #[cfg(test)]
        if self.heap_reference {
            let mut inputs = self.inputs.clone();
            for &(input, value) in changes {
                inputs[input.0 as usize] = value;
            }
            return self.evaluate_heap_reference(&inputs);
        }

        debug_assert!(self.frontier.is_empty());
        for &(input, value) in changes {
            let slot = input.0 as usize;
            if self.inputs[slot] == value {
                continue;
            }
            self.inputs[slot] = value;
            let start = self.input_offsets[slot];
            let end = self.input_offsets[slot + 1];
            for offset in start..end {
                self.frontier.set(self.input_expressions[offset]);
            }
        }
        self.propagate_frontier()
    }

    fn propagate_frontier(&mut self) -> Result<usize, &'static str> {
        let mut evaluated = 0;
        while let Some(index) = self.frontier.pop_min() {
            evaluated += 1;
            let value = self.evaluate_expression(index, &self.inputs)?;
            if value == self.values[index] {
                continue;
            }
            self.values[index] = value;
            let start = self.user_offsets[index];
            let end = self.user_offsets[index + 1];
            for offset in start..end {
                self.frontier.set(self.users[offset]);
            }
        }

        Ok(evaluated)
    }

    /// Current topologically ordered expression values.
    pub(crate) fn values(&self) -> &[bool] {
        &self.values
    }

    #[cfg(test)]
    pub(crate) fn set_heap_reference_for_test(&mut self, enabled: bool) {
        self.heap_reference = enabled;
        self.frontier.clear();
        self.heap_frontier.clear();
        self.heap_dirty.fill(false);
    }

    #[cfg(test)]
    fn evaluate_heap_reference(&mut self, inputs: &[bool]) -> Result<usize, &'static str> {
        self.heap_frontier.clear();
        for input in 0..self.inputs.len() {
            if self.inputs[input] == inputs[input] {
                continue;
            }
            let start = self.input_offsets[input];
            let end = self.input_offsets[input + 1];
            for offset in start..end {
                self.schedule_heap(self.input_expressions[offset]);
            }
        }

        let mut evaluated = 0;
        while let Some(Reverse(index)) = self.heap_frontier.pop() {
            self.heap_dirty[index] = false;
            evaluated += 1;
            let value = self.evaluate_expression(index, inputs)?;
            if value == self.values[index] {
                continue;
            }
            self.values[index] = value;
            let start = self.user_offsets[index];
            let end = self.user_offsets[index + 1];
            for offset in start..end {
                self.schedule_heap(self.users[offset]);
            }
        }
        self.inputs.copy_from_slice(inputs);
        Ok(evaluated)
    }

    #[cfg(test)]
    fn schedule_heap(&mut self, index: usize) {
        if !self.heap_dirty[index] {
            self.heap_dirty[index] = true;
            self.heap_frontier.push(Reverse(index));
        }
    }

    fn evaluate_expression(&self, index: usize, inputs: &[bool]) -> Result<bool, &'static str> {
        let value = match &self.program.expressions[index] {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(input) => *inputs.get(input.0 as usize).ok_or("logical input is missing")?,
            LayerExpr::Not(child) => !*self.values.get(child.0 as usize).ok_or("logical operand is missing")?,
            LayerExpr::All(children) => {
                let mut all = true;
                for child in children.iter() {
                    if !*self.values.get(child.0 as usize).ok_or("logical operand is missing")? {
                        all = false;
                        break;
                    }
                }
                all
            }
            LayerExpr::Any(children) => {
                let mut any = false;
                for child in children.iter() {
                    if *self.values.get(child.0 as usize).ok_or("logical operand is missing")? {
                        any = true;
                        break;
                    }
                }
                any
            }
        };
        Ok(value)
    }
}

fn add_operand_user(operand: LayerExprId, user: usize, user_counts: &mut [usize]) -> Result<(), String> {
    let operand_index = operand.0 as usize;
    if operand_index >= user || operand_index >= user_counts.len() {
        return Err(format!("logical expression {user} has a missing or forward operand {operand_index}"));
    }
    user_counts[operand_index] += 1;
    Ok(())
}

fn prefix_offsets(counts: &[usize]) -> Vec<usize> {
    let mut offsets: Vec<usize> = Vec::with_capacity(counts.len() + 1);
    offsets.push(0);
    for &count in counts {
        offsets.push(offsets.last().copied().unwrap().saturating_add(count));
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::{LayerInputId, LayerInputRole, LayerInputSource, PowerPort, SiteId};

    fn input_program(expressions: Vec<LayerExpr>, inputs: usize) -> ReducedLayerProgram {
        let bindings = (0..inputs)
            .map(|index| LayerInput {
                source: LayerInputSource::SourceAbove {
                    site: SiteId::new(index as u32),
                    port: PowerPort::Output,
                    threshold: 14,
                },
                role: LayerInputRole::RequestSample,
            })
            .collect::<Vec<_>>();
        ReducedLayerProgram {
            inputs: bindings.into_boxed_slice(),
            expressions: expressions.into_boxed_slice(),
            roots: Box::new([]),
        }
    }

    #[test]
    fn reconvergent_inputs_are_evaluated_once_and_only_changed_values_propagate() {
        let program = input_program(
            vec![
                LayerExpr::Input(LayerInputId(0)),
                LayerExpr::Input(LayerInputId(1)),
                LayerExpr::All(vec![LayerExprId(0), LayerExprId(1)].into_boxed_slice()),
                LayerExpr::Any(vec![LayerExprId(0), LayerExprId(1)].into_boxed_slice()),
                LayerExpr::All(vec![LayerExprId(2), LayerExprId(3)].into_boxed_slice()),
            ],
            2,
        );
        let mut values = LogicalValues::new(&program).unwrap();
        assert_eq!(values.evaluate(&[false, false]).unwrap(), 5);
        assert_eq!(values.values(), &[false, false, false, false, false]);
        assert_eq!(values.evaluate(&[true, true]).unwrap(), 5);
        assert_eq!(values.values(), &[true, true, true, true, true]);
        assert_eq!(values.evaluate(&[true, true]).unwrap(), 0);
        // The reconvergent OR directly depends on the changed input, so it is
        // reevaluated and then correctly stops propagation when it stays true.
        assert_eq!(values.evaluate(&[false, true]).unwrap(), 4);
        assert_eq!(values.values(), &[false, true, false, true, false]);
    }

    #[test]
    fn changed_input_does_not_visit_an_unrelated_branch() {
        let program = input_program(
            vec![
                LayerExpr::Input(LayerInputId(0)),
                LayerExpr::Input(LayerInputId(1)),
                LayerExpr::Not(LayerExprId(0)),
                LayerExpr::Not(LayerExprId(1)),
            ],
            2,
        );
        let mut values = LogicalValues::new(&program).unwrap();
        assert_eq!(values.evaluate(&[false, false]).unwrap(), 4);
        assert_eq!(values.evaluate(&[true, false]).unwrap(), 2);
        assert_eq!(values.values(), &[true, false, false, true]);
    }

    #[test]
    fn dirty_frontier_orders_deduplicates_reschedules_and_clears_across_summary_levels() {
        let mut frontier = DirtyFrontier::new(4_097);
        assert!(frontier.is_empty());
        assert_eq!(frontier.pop_min(), None);
        for index in [4_096, 64, 4_095, 63, 4_096, 64, 0] {
            frontier.set(index);
        }
        assert_eq!(
            std::iter::from_fn(|| frontier.pop_min()).collect::<Vec<_>>(),
            vec![0, 63, 64, 4_095, 4_096]
        );
        assert!(frontier.is_empty());

        frontier.set(4_096);
        assert_eq!(frontier.pop_min(), Some(4_096));
        frontier.set(4_096);
        frontier.set(63);
        assert_eq!(frontier.pop_min(), Some(63));
        assert_eq!(frontier.pop_min(), Some(4_096));
        assert!(frontier.is_empty());

        frontier.set(4_095);
        frontier.set(64);
        frontier.clear();
        assert!(frontier.is_empty());
        assert_eq!(frontier.pop_min(), None);
    }

    #[test]
    fn many_changed_inputs_propagate_in_topological_order() {
        let mut expressions = Vec::new();
        for input in 0..32 {
            expressions.push(LayerExpr::Input(LayerInputId(input as u32)));
        }
        for input in 0..32 {
            expressions.push(LayerExpr::Not(LayerExprId(input as u32)));
        }
        for input in 0..16 {
            expressions.push(LayerExpr::All(
                vec![LayerExprId(input as u32), LayerExprId(32 + input as u32)].into_boxed_slice(),
            ));
        }
        let program = input_program(expressions, 32);
        let mut values = LogicalValues::new(&program).unwrap();
        assert_eq!(values.evaluate(&[false; 32]).unwrap(), 80);
        assert_eq!(values.evaluate(&[true; 32]).unwrap(), 80);
        assert!(values.values()[..32].iter().all(|value| *value));
        assert!(values.values()[32..64].iter().all(|value| !*value));
        assert!(values.values()[64..].iter().all(|value| !*value));
    }

    #[test]
    fn boolean_boundary_inputs_accept_thresholded_strength_values() {
        let program = input_program(vec![LayerExpr::Input(LayerInputId(0))], 1);
        let mut values = LogicalValues::new(&program).unwrap();
        assert_eq!(values.evaluate(&[false]).unwrap(), 1);
        assert_eq!(values.evaluate(&[true]).unwrap(), 1);
        assert_eq!(values.values(), &[true]);
    }

    #[test]
    fn malformed_forward_operand_is_rejected_and_bad_input_length_is_atomic() {
        let malformed = input_program(vec![LayerExpr::Not(LayerExprId(0))], 0);
        assert!(LogicalValues::new(&malformed).unwrap_err().contains("forward operand"));

        let program = input_program(vec![LayerExpr::Constant(true)], 0);
        let mut values = LogicalValues::new(&program).unwrap();
        assert_eq!(values.evaluate(&[]).unwrap(), 1);
        assert_eq!(values.evaluate(&[true]), Err("logical input count does not match the program"));
        assert_eq!(values.values(), &[true]);
    }
}
