//! Pure Boolean circuits and allocation-free execution plans.
//!
//! Every root is observable, including state-write roots with no display output.
//! This representation has no physical timing or world-update semantics.

use std::collections::{HashMap, VecDeque};
use std::fmt;

pub type NetId = usize;
pub type InputId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Input(InputId),
    Const(bool),
    Buffer(NetId),
    Not(NetId),
    And(NetId, NetId),
    Or(NetId, NetId),
    Xor(NetId, NetId),
    /// Condition, true branch, false branch.
    Select(NetId, NetId, NetId),
}

impl Op {
    fn operands(self) -> impl Iterator<Item = NetId> {
        let operands = match self {
            Self::Input(_) | Self::Const(_) => [None, None, None],
            Self::Buffer(a) | Self::Not(a) => [Some(a), None, None],
            Self::And(a, b) | Self::Or(a, b) | Self::Xor(a, b) => [Some(a), Some(b), None],
            Self::Select(c, a, b) => [Some(c), Some(a), Some(b)],
        };
        operands.into_iter().flatten()
    }

    fn remap(self, mut slot: impl FnMut(NetId) -> NetId) -> Self {
        match self {
            Self::Buffer(a) => Self::Buffer(slot(a)),
            Self::Not(a) => Self::Not(slot(a)),
            Self::And(a, b) => Self::And(slot(a), slot(b)),
            Self::Or(a, b) => Self::Or(slot(a), slot(b)),
            Self::Xor(a, b) => Self::Xor(slot(a), slot(b)),
            Self::Select(c, a, b) => Self::Select(slot(c), slot(a), slot(b)),
            op => op,
        }
    }
}

/// Nets are indices into `ops`; input values follow the declared input-ID order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Circuit {
    inputs: Vec<InputId>,
    ops: Vec<Op>,
    roots: Vec<NetId>,
}

impl Circuit {
    pub fn new(inputs: Vec<InputId>, ops: Vec<Op>, roots: Vec<NetId>) -> Self {
        Self { inputs, ops, roots }
    }

    pub fn inputs(&self) -> &[InputId] {
        &self.inputs
    }
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }
    pub fn roots(&self) -> &[NetId] {
        &self.roots
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    DuplicateInput(InputId),
    UndeclaredInput { net: NetId, input: InputId },
    InvalidOperand { net: NetId, operand: NetId },
    InvalidRoot { root: usize, net: NetId },
    Cycle,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateInput(id) => write!(f, "duplicate input ID {id}"),
            Self::UndeclaredInput { net, input } => {
                write!(f, "net {net} uses undeclared input ID {input}")
            }
            Self::InvalidOperand { net, operand } => {
                write!(f, "net {net} uses invalid operand {operand}")
            }
            Self::InvalidRoot { root, net } => write!(f, "root {root} uses invalid net {net}"),
            Self::Cycle => f.write_str("circuit contains a cycle"),
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizeError {
    InvalidCircuit(ValidationError),
    Cancelled,
}

impl fmt::Display for OptimizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCircuit(error) => error.fmt(f),
            Self::Cancelled => f.write_str("optimization cancelled"),
        }
    }
}

impl std::error::Error for OptimizeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidCircuit(error) => Some(error),
            Self::Cancelled => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationError {
    InputCount { expected: usize, actual: usize },
    ScratchTooSmall { expected: usize, actual: usize },
    RootsTooSmall { expected: usize, actual: usize },
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputCount { expected, actual } => {
                write!(f, "expected {expected} inputs, got {actual}")
            }
            Self::ScratchTooSmall { expected, actual } => {
                write!(f, "scratch needs {expected} slots, got {actual}")
            }
            Self::RootsTooSmall { expected, actual } => {
                write!(f, "root output needs {expected} slots, got {actual}")
            }
        }
    }
}

impl std::error::Error for EvaluationError {}

/// Optional optimizer limits. Reference validation and construction precede them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum optimizer visits across rewriting, pruning and layout.
    pub max_work: usize,
    /// Maximum distinct nodes during structural sharing, before pruning.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_work: 16_777_216,
            max_nodes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    inputs: Vec<InputId>,
    ops: Vec<Op>,
    roots: Vec<NetId>,
    original_to_slot: Vec<Option<NetId>>,
    // Compiled input ordinals avoid hashing input identities during evaluation.
    input_ordinals: Vec<usize>,
    used_fallback: bool,
}

impl Plan {
    pub fn inputs(&self) -> &[InputId] {
        &self.inputs
    }
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }
    pub fn roots(&self) -> &[NetId] {
        &self.roots
    }
    /// Equivalent retained values have a slot; pruned values have `None`.
    pub fn original_to_slot(&self) -> &[Option<NetId>] {
        &self.original_to_slot
    }
    pub fn scratch_len(&self) -> usize {
        self.ops.len()
    }
    pub fn used_fallback(&self) -> bool {
        self.used_fallback
    }

    /// Writes scratch and ordered root outputs without allocating.
    /// Input count must match exactly; scratch/root buffers may have extra space.
    /// Invalid buffer sizes return before any buffer is modified.
    pub fn evaluate(
        &self,
        input_values: &[bool],
        scratch: &mut [bool],
        root_values: &mut [bool],
    ) -> Result<(), EvaluationError> {
        if input_values.len() != self.inputs.len() {
            return Err(EvaluationError::InputCount {
                expected: self.inputs.len(),
                actual: input_values.len(),
            });
        }
        if scratch.len() < self.ops.len() {
            return Err(EvaluationError::ScratchTooSmall {
                expected: self.ops.len(),
                actual: scratch.len(),
            });
        }
        if root_values.len() < self.roots.len() {
            return Err(EvaluationError::RootsTooSmall {
                expected: self.roots.len(),
                actual: root_values.len(),
            });
        }
        for (slot, &op) in self.ops.iter().enumerate() {
            scratch[slot] = match op {
                Op::Input(_) => input_values[self.input_ordinals[slot]],
                Op::Const(value) => value,
                Op::Buffer(a) => scratch[a],
                Op::Not(a) => !scratch[a],
                Op::And(a, b) => scratch[a] & scratch[b],
                Op::Or(a, b) => scratch[a] | scratch[b],
                Op::Xor(a, b) => scratch[a] ^ scratch[b],
                Op::Select(c, a, b) => {
                    if scratch[c] {
                        scratch[a]
                    } else {
                        scratch[b]
                    }
                }
            };
        }
        for (output, &slot) in root_values.iter_mut().zip(&self.roots) {
            *output = scratch[slot];
        }
        Ok(())
    }
}

/// Validates and topologically lays out every net, including unused nets.
pub fn reference(circuit: &Circuit) -> Result<Plan, ValidationError> {
    let mut inputs = HashMap::new();
    for (ordinal, &id) in circuit.inputs.iter().enumerate() {
        if inputs.insert(id, ordinal).is_some() {
            return Err(ValidationError::DuplicateInput(id));
        }
    }
    let count = circuit.ops.len();
    let mut incoming = vec![0usize; count];
    let mut fanout = vec![Vec::new(); count];
    for (net, &op) in circuit.ops.iter().enumerate() {
        if let Op::Input(input) = op {
            if !inputs.contains_key(&input) {
                return Err(ValidationError::UndeclaredInput { net, input });
            }
        }
        for operand in op.operands() {
            if operand >= count {
                return Err(ValidationError::InvalidOperand { net, operand });
            }
            incoming[net] += 1;
            fanout[operand].push(net);
        }
    }
    for (root, &net) in circuit.roots.iter().enumerate() {
        if net >= count {
            return Err(ValidationError::InvalidRoot { root, net });
        }
    }
    let mut ready: VecDeque<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(net, &count)| (count == 0).then_some(net))
        .collect();
    let mut ops = Vec::with_capacity(count);
    let mut input_ordinals = Vec::with_capacity(count);
    let mut original_to_slot = vec![None; count];
    while let Some(net) = ready.pop_front() {
        original_to_slot[net] = Some(ops.len());
        let op = circuit.ops[net].remap(|operand| original_to_slot[operand].unwrap());
        input_ordinals.push(match op {
            Op::Input(id) => inputs[&id],
            _ => 0,
        });
        ops.push(op);
        for &target in &fanout[net] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                ready.push_back(target);
            }
        }
    }
    if ops.len() != count {
        return Err(ValidationError::Cycle);
    }
    let roots = circuit
        .roots
        .iter()
        .map(|&net| original_to_slot[net].unwrap())
        .collect();
    Ok(Plan {
        inputs: circuit.inputs.clone(),
        ops,
        roots,
        original_to_slot,
        input_ordinals,
        used_fallback: false,
    })
}

/// One topological rewrite sweep, structural sharing, then pruning from all roots.
/// Invalid circuits fail before cancellation or optimizer limits are examined.
/// Budget exhaustion returns the validated reference with `used_fallback = true`.
pub fn optimize(
    circuit: &Circuit,
    limits: Limits,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Plan, OptimizeError> {
    let mut reference = reference(circuit).map_err(OptimizeError::InvalidCircuit)?;
    match optimize_plan(&reference, limits, &mut cancelled)? {
        Some(plan) => Ok(plan),
        None => {
            reference.used_fallback = true;
            Ok(reference)
        }
    }
}

fn optimize_plan(
    reference: &Plan,
    limits: Limits,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Plan>, OptimizeError> {
    let mut work = 0;
    let mut step = || {
        if cancelled() {
            return Err(OptimizeError::Cancelled);
        }
        if work >= limits.max_work {
            return Ok(false);
        }
        work += 1;
        Ok(true)
    };
    if !step()? || reference.ops.len() > limits.max_work {
        return Ok(None);
    }
    let mut nodes = Vec::new();
    let mut ordinals = Vec::new();
    let mut shared = HashMap::new();
    let mut rewritten = vec![0; reference.ops.len()];
    for (slot, &op) in reference.ops.iter().enumerate() {
        if !step()? {
            return Ok(None);
        }
        let op = simplify(op.remap(|operand| rewritten[operand]), &nodes);
        rewritten[slot] = if let Op::Buffer(value) = op {
            value
        } else if let Some(&existing) = shared.get(&op) {
            existing
        } else {
            if nodes.len() >= limits.max_nodes {
                return Ok(None);
            }
            let next = nodes.len();
            nodes.push(op);
            ordinals.push(reference.input_ordinals[slot]);
            shared.insert(op, next);
            next
        };
    }
    let mut pending = Vec::new();
    for &root in &reference.roots {
        if !step()? {
            return Ok(None);
        }
        pending.push(rewritten[root]);
    }
    let mut live = vec![false; nodes.len()];
    while let Some(net) = pending.pop() {
        if !step()? {
            return Ok(None);
        }
        if !live[net] {
            live[net] = true;
            pending.extend(nodes[net].operands());
        }
    }
    let mut dense = vec![None; nodes.len()];
    let mut ops = Vec::new();
    let mut input_ordinals = Vec::new();
    for (net, &op) in nodes.iter().enumerate() {
        if !step()? {
            return Ok(None);
        }
        if live[net] {
            dense[net] = Some(ops.len());
            ops.push(op.remap(|operand| dense[operand].unwrap()));
            input_ordinals.push(ordinals[net]);
        }
    }
    let mut roots = Vec::new();
    for &root in &reference.roots {
        if !step()? {
            return Ok(None);
        }
        roots.push(dense[rewritten[root]].unwrap());
    }
    let mut original_to_slot = Vec::new();
    for &slot in &reference.original_to_slot {
        if !step()? {
            return Ok(None);
        }
        original_to_slot.push(dense[rewritten[slot.unwrap()]]);
    }
    let mut inputs = Vec::new();
    for &id in &reference.inputs {
        if !step()? {
            return Ok(None);
        }
        inputs.push(id);
    }
    if !step()? {
        return Ok(None);
    }
    Ok(Some(Plan {
        inputs,
        ops,
        roots,
        original_to_slot,
        input_ordinals,
        used_fallback: false,
    }))
}

fn constant(nodes: &[Op], net: NetId) -> Option<bool> {
    match nodes[net] {
        Op::Const(value) => Some(value),
        _ => None,
    }
}

fn opposite(nodes: &[Op], a: NetId, b: NetId) -> bool {
    matches!(nodes[a], Op::Not(net) if net == b) || matches!(nodes[b], Op::Not(net) if net == a)
}

fn simplify(op: Op, nodes: &[Op]) -> Op {
    match op {
        Op::Not(a) => match nodes[a] {
            Op::Const(value) => Op::Const(!value),
            Op::Not(value) => Op::Buffer(value),
            _ => op,
        },
        Op::And(a, b) | Op::Or(a, b) | Op::Xor(a, b) => {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            match op {
                Op::And(_, _) => {
                    if a == b {
                        return Op::Buffer(a);
                    }
                    if opposite(nodes, a, b) {
                        return Op::Const(false);
                    }
                    if let Some(value) = constant(nodes, a) {
                        return if value {
                            Op::Buffer(b)
                        } else {
                            Op::Const(false)
                        };
                    }
                    if let Some(value) = constant(nodes, b) {
                        return if value {
                            Op::Buffer(a)
                        } else {
                            Op::Const(false)
                        };
                    }
                    Op::And(a, b)
                }
                Op::Or(_, _) => {
                    if a == b {
                        return Op::Buffer(a);
                    }
                    if opposite(nodes, a, b) {
                        return Op::Const(true);
                    }
                    if let Some(value) = constant(nodes, a) {
                        return if value {
                            Op::Const(true)
                        } else {
                            Op::Buffer(b)
                        };
                    }
                    if let Some(value) = constant(nodes, b) {
                        return if value {
                            Op::Const(true)
                        } else {
                            Op::Buffer(a)
                        };
                    }
                    Op::Or(a, b)
                }
                Op::Xor(_, _) => {
                    if a == b {
                        return Op::Const(false);
                    }
                    if opposite(nodes, a, b) {
                        return Op::Const(true);
                    }
                    if let Some(value) = constant(nodes, a) {
                        return if value {
                            simplify(Op::Not(b), nodes)
                        } else {
                            Op::Buffer(b)
                        };
                    }
                    if let Some(value) = constant(nodes, b) {
                        return if value {
                            simplify(Op::Not(a), nodes)
                        } else {
                            Op::Buffer(a)
                        };
                    }
                    Op::Xor(a, b)
                }
                _ => unreachable!(),
            }
        }
        Op::Select(c, yes, no) => {
            if let Some(value) = constant(nodes, c) {
                return Op::Buffer(if value { yes } else { no });
            }
            if yes == no {
                return Op::Buffer(yes);
            }
            if let Op::Not(condition) = nodes[c] {
                return simplify(Op::Select(condition, no, yes), nodes);
            }
            match (constant(nodes, yes), constant(nodes, no)) {
                (Some(true), Some(false)) => return Op::Buffer(c),
                (Some(false), Some(true)) => return simplify(Op::Not(c), nodes),
                (Some(true), _) => return simplify(Op::Or(c, no), nodes),
                (_, Some(false)) => return simplify(Op::And(c, yes), nodes),
                _ => {}
            }
            if yes == c {
                return simplify(Op::Or(c, no), nodes);
            }
            if no == c {
                return simplify(Op::And(c, yes), nodes);
            }
            if opposite(nodes, yes, no) {
                return simplify(Op::Xor(c, no), nodes);
            }
            Op::Select(c, yes, no)
        }
        _ => op,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equivalent(circuit: &Circuit) -> Plan {
        let before = circuit.clone();
        let reference = reference(circuit).unwrap();
        let optimized = optimize(circuit, Limits::default(), || false).unwrap();
        assert!(!optimized.used_fallback());
        assert_eq!(*circuit, before);
        for plan in [&reference, &optimized] {
            for (slot, &op) in plan.ops().iter().enumerate() {
                assert!(op.operands().all(|operand| operand < slot));
            }
        }
        let mut reference_scratch = vec![false; reference.scratch_len()];
        let mut optimized_scratch = vec![false; optimized.scratch_len()];
        let mut expected = vec![false; circuit.roots.len()];
        let mut actual = expected.clone();
        for bits in 0..(1usize << circuit.inputs.len()) {
            let inputs: Vec<_> = (0..circuit.inputs.len())
                .map(|bit| bits & (1 << bit) != 0)
                .collect();
            reference
                .evaluate(&inputs, &mut reference_scratch, &mut expected)
                .unwrap();
            optimized
                .evaluate(&inputs, &mut optimized_scratch, &mut actual)
                .unwrap();
            assert_eq!(actual, expected, "assignment {bits}");
            for (original, &slot) in optimized.original_to_slot().iter().enumerate() {
                if let Some(slot) = slot {
                    let original_slot = reference.original_to_slot()[original].unwrap();
                    assert_eq!(
                        optimized_scratch[slot], reference_scratch[original_slot],
                        "net {original}, assignment {bits}"
                    );
                }
            }
        }
        optimized
    }

    #[test]
    fn rejects_invalid_inputs_operands_roots_and_all_cycles() {
        let invalid = [
            (
                Circuit::new(vec![4, 4], vec![Op::Input(4)], vec![0]),
                ValidationError::DuplicateInput(4),
            ),
            (
                Circuit::new(vec![4], vec![Op::Input(8)], vec![0]),
                ValidationError::UndeclaredInput { net: 0, input: 8 },
            ),
            (
                Circuit::new(vec![], vec![Op::Not(1)], vec![0]),
                ValidationError::InvalidOperand { net: 0, operand: 1 },
            ),
            (
                Circuit::new(
                    vec![],
                    vec![Op::Const(true), Op::Select(0, 0, usize::MAX)],
                    vec![0],
                ),
                ValidationError::InvalidOperand {
                    net: 1,
                    operand: usize::MAX,
                },
            ),
            (
                Circuit::new(vec![], vec![Op::Const(true)], vec![1]),
                ValidationError::InvalidRoot { root: 0, net: 1 },
            ),
            (
                Circuit::new(vec![], vec![], vec![0]),
                ValidationError::InvalidRoot { root: 0, net: 0 },
            ),
            (
                Circuit::new(vec![], vec![Op::Buffer(0)], vec![]),
                ValidationError::Cycle,
            ),
            (
                Circuit::new(
                    vec![],
                    vec![Op::Not(1), Op::Not(0), Op::Const(true)],
                    vec![2],
                ),
                ValidationError::Cycle,
            ),
        ];
        for (circuit, error) in invalid {
            assert_eq!(reference(&circuit), Err(error.clone()));
            let mut polled = false;
            let result = optimize(
                &circuit,
                Limits {
                    max_work: 0,
                    max_nodes: 0,
                },
                || {
                    polled = true;
                    true
                },
            );
            assert_eq!(result, Err(OptimizeError::InvalidCircuit(error)));
            assert!(!polled, "validation must precede cancellation and limits");
        }
    }

    #[test]
    fn unsorted_reference_retains_every_op_and_maps_fanout() {
        let circuit = Circuit::new(
            vec![71, 4],
            vec![
                Op::Xor(3, 4),
                Op::Const(true),
                Op::And(0, 0),
                Op::Input(4),
                Op::Input(71),
                Op::Buffer(2),
                Op::Not(1),
            ],
            vec![5, 0, 5],
        );
        let plan = reference(&circuit).unwrap();
        assert_eq!(plan.ops.len(), circuit.ops.len());
        for (original, &op) in circuit.ops.iter().enumerate() {
            let slot = plan.original_to_slot()[original].unwrap();
            assert_eq!(
                plan.ops[slot],
                op.remap(|net| plan.original_to_slot()[net].unwrap())
            );
        }
        assert_eq!(plan.roots[0], plan.roots[2]);
        let mut scratch = vec![false; plan.scratch_len()];
        let mut roots = [false; 3];
        plan.evaluate(&[true, false], &mut scratch, &mut roots)
            .unwrap();
        assert_eq!(roots, [true; 3]);
        assert!(
            !scratch[plan.original_to_slot()[6].unwrap()],
            "unused Not survives reference inspection"
        );
        let optimized = equivalent(&circuit);
        assert_eq!(optimized.original_to_slot()[6], None);
    }

    #[test]
    fn roots_preserve_order_duplicates_constants_and_state_writes() {
        let circuit = Circuit::new(
            vec![99, 3],
            vec![
                Op::Input(99),
                Op::Input(3),
                Op::Const(true),
                Op::Const(false),
                Op::Xor(0, 1), // A live state-write root with no display consumer.
                Op::Buffer(2),
                Op::Not(4), // Unobserved value must be pruned.
            ],
            vec![2, 3, 4, 0, 2, 5],
        );
        let plan = equivalent(&circuit);
        assert_eq!(plan.roots.len(), 6);
        assert_eq!(plan.roots[0], plan.roots[4]);
        assert_eq!(plan.roots[0], plan.roots[5]);
        assert!(plan.original_to_slot()[4].is_some());
        assert_eq!(plan.original_to_slot()[6], None);
        let mut scratch = vec![false; plan.scratch_len()];
        let mut roots = [false; 6];
        plan.evaluate(&[true, false], &mut scratch, &mut roots)
            .unwrap();
        assert_eq!(roots, [true, false, true, true, true, true]);
    }

    #[test]
    fn input_identity_and_shared_interior_fanout_survive() {
        let circuit = Circuit::new(
            vec![12, 9, 6],
            vec![
                Op::Input(12),
                Op::Input(9),
                Op::Input(6),
                Op::Input(12),
                Op::And(0, 1),
                Op::And(1, 3),
                Op::Or(4, 2),
                Op::Xor(5, 2),
            ],
            vec![6, 7],
        );
        let plan = equivalent(&circuit);
        assert_eq!(plan.original_to_slot()[0], plan.original_to_slot()[3]);
        assert_ne!(plan.original_to_slot()[0], plan.original_to_slot()[1]);
        assert_eq!(plan.original_to_slot()[4], plan.original_to_slot()[5]);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, Op::And(..)))
                .count(),
            1
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, Op::Input(..)))
                .count(),
            3
        );
    }

    #[test]
    fn constants_buffers_inverses_xor_and_mux_identities_fold() {
        let mut ops = vec![
            Op::Input(7),
            Op::Input(8),
            Op::Const(false),
            Op::Const(true),
            Op::Not(0),
        ];
        let identities = [
            (Op::Buffer(0), 0),
            (Op::Not(4), 0),
            (Op::And(0, 0), 0),
            (Op::And(0, 2), 2),
            (Op::And(0, 3), 0),
            (Op::And(0, 4), 2),
            (Op::Or(0, 0), 0),
            (Op::Or(0, 2), 0),
            (Op::Or(0, 3), 3),
            (Op::Or(0, 4), 3),
            (Op::Xor(0, 0), 2),
            (Op::Xor(0, 2), 0),
            (Op::Xor(0, 3), 4),
            (Op::Xor(0, 4), 3),
            (Op::Xor(4, 3), 0),
            (Op::Select(2, 0, 1), 1),
            (Op::Select(3, 0, 1), 0),
            (Op::Select(1, 0, 0), 0),
            (Op::Select(0, 3, 2), 0),
            (Op::Select(0, 2, 3), 4),
        ];
        let mut roots: Vec<_> = (0..ops.len()).collect();
        for &(op, _) in &identities {
            roots.push(ops.len());
            ops.push(op);
        }
        let plan = equivalent(&Circuit::new(vec![7, 8], ops, roots));
        for (index, &(_, expected)) in identities.iter().enumerate() {
            assert_eq!(
                plan.original_to_slot()[5 + index],
                plan.original_to_slot()[expected]
            );
        }
        assert_eq!(plan.ops.len(), 5);
        assert!(!plan.ops.iter().any(|op| matches!(op, Op::Buffer(..))));
    }

    #[test]
    fn exhaustively_checks_every_boolean_op_over_small_functions() {
        let basis = vec![
            Op::Input(41),
            Op::Input(2),
            Op::Const(false),
            Op::Const(true),
            Op::Not(0),
            Op::Not(1),
        ];
        let mut ops = basis.clone();
        for a in 0..basis.len() {
            ops.extend([Op::Buffer(a), Op::Not(a)]);
            for b in 0..basis.len() {
                ops.extend([Op::And(a, b), Op::Or(a, b), Op::Xor(a, b)]);
                for c in 0..basis.len() {
                    ops.push(Op::Select(c, a, b));
                }
            }
        }
        // All intermediates are observable so mappings are checked as well as roots.
        let roots = (0..ops.len()).collect();
        equivalent(&Circuit::new(vec![41, 2], ops, roots));
        let circuit = Circuit::new(
            vec![41, 2],
            vec![
                Op::Input(41),
                Op::Input(2),
                Op::Not(0),
                Op::Not(1),
                Op::Xor(0, 1),
                Op::Select(0, 1, 3),
                Op::Select(2, 3, 1),
                Op::Select(0, 0, 1),
                Op::Or(0, 1),
                Op::Select(0, 1, 0),
                Op::And(0, 1),
            ],
            vec![4, 5, 6, 7, 8, 9, 10],
        );
        let plan = equivalent(&circuit);
        assert_eq!(plan.original_to_slot()[5], plan.original_to_slot()[6]);
        assert_eq!(plan.original_to_slot()[7], plan.original_to_slot()[8]);
        assert_eq!(plan.original_to_slot()[9], plan.original_to_slot()[10]);
    }

    #[test]
    fn budgets_fall_back_and_cancellation_aborts_after_validation() {
        let circuit = Circuit::new(
            vec![0],
            vec![Op::Input(0), Op::Buffer(0), Op::Not(1)],
            vec![2],
        );
        let expected = reference(&circuit).unwrap();
        for limits in [
            Limits {
                max_work: 0,
                max_nodes: 99,
            },
            Limits {
                max_work: 99,
                max_nodes: 0,
            },
            Limits {
                max_work: 5,
                max_nodes: 99,
            },
        ] {
            let mut fallback = optimize(&circuit, limits, || false).unwrap();
            assert!(fallback.used_fallback());
            fallback.used_fallback = false;
            assert_eq!(fallback, expected);
        }
        assert_eq!(
            optimize(
                &circuit,
                Limits {
                    max_work: 0,
                    max_nodes: 0
                },
                || true
            ),
            Err(OptimizeError::Cancelled)
        );
        let mut polls = 0;
        assert_eq!(
            optimize(&circuit, Limits::default(), || {
                polls += 1;
                polls == 6
            }),
            Err(OptimizeError::Cancelled)
        );
        assert_eq!(polls, 6);
        equivalent(&circuit);
    }

    #[test]
    fn evaluation_checks_buffers_before_writing_and_handles_empty_cones() {
        let circuit = Circuit::new(vec![11], vec![Op::Input(11), Op::Not(0)], vec![1]);
        let plan = equivalent(&circuit);
        let mut scratch = [true; 3];
        let mut roots = [true; 2];
        assert!(matches!(
            plan.evaluate(&[], &mut scratch, &mut roots),
            Err(EvaluationError::InputCount { .. })
        ));
        assert!(matches!(
            plan.evaluate(&[false, true], &mut scratch, &mut roots),
            Err(EvaluationError::InputCount { .. })
        ));
        assert!(matches!(
            plan.evaluate(&[false], &mut scratch[..1], &mut roots),
            Err(EvaluationError::ScratchTooSmall { .. })
        ));
        assert!(matches!(
            plan.evaluate(&[false], &mut scratch, &mut []),
            Err(EvaluationError::RootsTooSmall { .. })
        ));
        assert_eq!(scratch, [true; 3]);
        assert_eq!(roots, [true; 2]);
        plan.evaluate(&[true], &mut scratch, &mut roots).unwrap();
        assert_eq!(roots, [false, true]);
        assert!(scratch[2], "extra storage is untouched");
        let empty = equivalent(&Circuit::new(vec![], vec![], vec![]));
        empty.evaluate(&[], &mut [], &mut []).unwrap();
        let pruned = equivalent(&Circuit::new(vec![11], vec![Op::Input(11)], vec![]));
        assert!(pruned.ops().is_empty());
        assert_eq!(pruned.original_to_slot(), &[None]);
        pruned.evaluate(&[false], &mut [], &mut []).unwrap();
    }
}
