//! Bounded Boolean reduction for AOT symbolic execution, without enumerating input values.
//!
//! Canonical mode gives equivalent Boolean functions the same identity for proofs.
//! Structural mode shares expressions without expanding their decision trees during tracing.
//! Both use four-bit signals and lower to the existing allocation-free Boolean Plan.
use crate::{Circuit, InputId, Limits, Op};
use std::collections::{BTreeSet, HashMap};

#[path = "symbolic_constraints.rs"]
mod constraints;

#[path = "symbolic_propagation.rs"]
mod propagation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A Boolean function owned by the Diagram that constructed it.
pub struct Formula(u32);

impl Formula {
    pub const FALSE: Self = Self(0);
    pub const TRUE: Self = Self(1);

    pub fn constant(self) -> Option<bool> {
        match self {
            Self::FALSE => Some(false),
            Self::TRUE => Some(true),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A four-bit redstone strength; bit zero is the least significant bit.
pub struct Signal {
    /// These Boolean functions describe one strength for every declared input valuation.
    bits: [Formula; 4],
}

impl Signal {
    pub fn literal(value: u8) -> Result<Self, SymbolicError> {
        if value > 15 {
            return Err(SymbolicError::InvalidSignal);
        }
        Ok(Self {
            bits: std::array::from_fn(|bit| if value & (1 << bit) == 0 { Formula::FALSE } else { Formula::TRUE }),
        })
    }

    pub fn from_bits(bits: [Formula; 4]) -> Self {
        Self { bits }
    }

    pub fn bits(self) -> [Formula; 4] {
        self.bits
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolicError {
    /// Construction exhausted its work or node budget; the caller must decline compilation.
    BudgetExceeded,
    /// An operation referenced a formula outside its owning diagram.
    InvalidFormula,
    /// A strength literal was outside the redstone domain zero through fifteen.
    InvalidSignal,
    /// The reserved terminal variable identifier cannot name an input.
    InvalidInput,
    /// This proof requires canonical decision nodes rather than structural expressions.
    RequiresCanonicalDiagram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Decision {
    /// This orders input tests; child decisions always test later input identifiers.
    variable: InputId,
    /// This is the selected function when the tested input is false.
    low: Formula,
    /// This is the selected function when the tested input is true.
    high: Formula,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Selection {
    /// This is the Boolean condition being tested.
    condition: Formula,
    /// This is the function selected when the condition is true.
    yes: Formula,
    /// This is the function selected when the condition is false.
    no: Formula,
}

#[derive(Debug, Clone, Copy)]
enum Node {
    /// This is an ordered input decision, including the two terminal placeholders.
    Decision(Decision),
    /// This retains a conditional expression without expanding its Boolean decision tree.
    Selection(Selection),
}

pub struct Diagram {
    /// These shared nodes follow their children; the first two entries are terminals.
    nodes: Vec<Node>,
    /// This interns input/decision nodes; canonical mode also identifies equivalent functions.
    unique: HashMap<Decision, Formula>,
    /// This caches canonical applications or interns structural conditional nodes.
    selections: HashMap<Selection, Formula>,
    /// These are all declared inputs, including declarations later removed by reduction.
    inputs: BTreeSet<InputId>,
    /// This bounds compiler work and retained decision nodes without limiting operand width.
    limits: Limits,
    /// This counts charged construction steps across the whole compilation.
    work: usize,
    /// This keeps traced expressions structural instead of requiring canonical Boolean functions.
    structural: bool,
}

impl Diagram {
    pub fn new(limits: Limits) -> Result<Self, SymbolicError> {
        Self::with_representation(limits, false)
    }

    /// Build a shared expression DAG; equality proves identical structure, not all equivalent functions.
    pub fn new_structural(limits: Limits) -> Result<Self, SymbolicError> {
        Self::with_representation(limits, true)
    }

    fn with_representation(limits: Limits, structural: bool) -> Result<Self, SymbolicError> {
        if limits.max_nodes < 2 {
            return Err(SymbolicError::BudgetExceeded);
        }
        let terminal = Decision {
            variable: InputId::MAX,
            low: Formula::FALSE,
            high: Formula::TRUE,
        };
        Ok(Self {
            nodes: vec![Node::Decision(terminal); 2],
            unique: HashMap::new(),
            selections: HashMap::new(),
            inputs: BTreeSet::new(),
            limits,
            work: 0,
            structural,
        })
    }

    pub fn input(&mut self, variable: InputId) -> Result<Formula, SymbolicError> {
        if variable == InputId::MAX {
            return Err(SymbolicError::InvalidInput);
        }
        let value = self.decision(variable, Formula::FALSE, Formula::TRUE)?;
        self.inputs.insert(variable);
        Ok(value)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn work(&self) -> usize {
        self.work
    }

    pub fn not(&mut self, value: Formula) -> Result<Formula, SymbolicError> {
        self.validate(value)?;
        if self.structural
            && let Node::Selection(Selection {
                condition,
                yes: Formula::FALSE,
                no: Formula::TRUE,
            }) = self.nodes[value.0 as usize]
        {
            return Ok(condition);
        }
        self.select(value, Formula::FALSE, Formula::TRUE)
    }

    pub fn and(&mut self, left: Formula, right: Formula) -> Result<Formula, SymbolicError> {
        self.validate(left)?;
        self.validate(right)?;
        if left == Formula::FALSE || right == Formula::FALSE {
            return Ok(Formula::FALSE);
        }
        if left == Formula::TRUE || left == right {
            return Ok(right);
        }
        if right == Formula::TRUE {
            return Ok(left);
        }
        if self.structural && self.complements(left, right) {
            return Ok(Formula::FALSE);
        }
        if self.structural {
            if self.negated(right).is_some_and(|value| self.structurally_implies(left, value))
                || self.negated(left).is_some_and(|value| self.structurally_implies(right, value))
            {
                return Ok(Formula::FALSE);
            }
            if self.structurally_implies(left, right) {
                return Ok(left);
            }
            if self.structurally_implies(right, left) {
                return Ok(right);
            }
        }
        let (left, right) = self.commutative_operands(left, right);
        self.select(left, right, Formula::FALSE)
    }

    pub fn or(&mut self, left: Formula, right: Formula) -> Result<Formula, SymbolicError> {
        self.validate(left)?;
        self.validate(right)?;
        if left == Formula::TRUE || right == Formula::TRUE {
            return Ok(Formula::TRUE);
        }
        if left == Formula::FALSE || left == right {
            return Ok(right);
        }
        if right == Formula::FALSE {
            return Ok(left);
        }
        if self.structural && self.complements(left, right) {
            return Ok(Formula::TRUE);
        }
        if self.structural {
            if self.negated(right).is_some_and(|value| self.structurally_implies(value, left))
                || self.negated(left).is_some_and(|value| self.structurally_implies(value, right))
            {
                return Ok(Formula::TRUE);
            }
            if self.structurally_implies(left, right) {
                return Ok(right);
            }
            if self.structurally_implies(right, left) {
                return Ok(left);
            }
        }
        let (left, right) = self.commutative_operands(left, right);
        self.select(left, Formula::TRUE, right)
    }

    pub fn xor(&mut self, left: Formula, right: Formula) -> Result<Formula, SymbolicError> {
        self.validate(left)?;
        self.validate(right)?;
        if left == right {
            return Ok(Formula::FALSE);
        }
        if left == Formula::FALSE {
            return Ok(right);
        }
        if right == Formula::FALSE {
            return Ok(left);
        }
        if self.structural && self.complements(left, right) {
            return Ok(Formula::TRUE);
        }
        let (left, right) = self.commutative_operands(left, right);
        let inverse = self.not(right)?;
        self.select(left, inverse, right)
    }

    /// Return the exact number of selected occurrences as little-endian bits, without a valuation table.
    pub fn count(&mut self, values: &[Formula]) -> Result<Vec<Formula>, SymbolicError> {
        let mut bits = Vec::new();
        for &value in values {
            self.increment(&mut bits, value)?;
        }
        Ok(bits)
    }

    /// Add one selected occurrence to an exact, unbounded-width unsigned bit vector.
    fn increment(&mut self, bits: &mut Vec<Formula>, value: Formula) -> Result<(), SymbolicError> {
        self.charge()?;
        self.validate(value)?;
        for &bit in bits.iter() {
            self.validate(bit)?;
        }
        let mut carry = value;
        for bit in bits.iter_mut() {
            if carry == Formula::FALSE {
                break;
            }
            let previous = *bit;
            *bit = self.xor(previous, carry)?;
            carry = self.and(previous, carry)?;
        }
        if carry != Formula::FALSE {
            bits.push(carry);
        }
        while bits.last() == Some(&Formula::FALSE) {
            bits.pop();
        }
        Ok(())
    }

    pub fn select(&mut self, condition: Formula, yes: Formula, no: Formula) -> Result<Formula, SymbolicError> {
        for value in [condition, yes, no] {
            self.validate(value)?;
        }
        let root = Selection { condition, yes, no };
        if self.structural {
            return self.structural_selection(root);
        }
        // Memo entries are disposable; clear between ITE calls without discarding canonical function identities.
        if self.selections.len() >= self.limits.max_nodes / 2 {
            self.selections.clear();
        }
        // An explicit stack avoids recursion proportional to a circuit's input count.
        let mut pending = vec![root];
        while let Some(&selection) = pending.last() {
            self.charge()?;
            if self.known(selection).is_some() {
                pending.pop();
                continue;
            }
            let variable = [selection.condition, selection.yes, selection.no]
                .into_iter()
                .map(|value| self.ordered_decision(value).variable)
                .min()
                .expect("three operands");
            let low = self.cofactor(selection, variable, false);
            let high = self.cofactor(selection, variable, true);
            let Some(low_value) = self.known(low) else {
                pending.push(low);
                continue;
            };
            let Some(high_value) = self.known(high) else {
                pending.push(high);
                continue;
            };
            let result = self.decision(variable, low_value, high_value)?;
            if self.selections.len() >= self.limits.max_nodes {
                return Err(SymbolicError::BudgetExceeded);
            }
            self.selections.insert(selection, result);
            pending.pop();
        }
        self.known(root).ok_or(SymbolicError::InvalidFormula)
    }

    pub fn signal_select(&mut self, condition: Formula, yes: Signal, no: Signal) -> Result<Signal, SymbolicError> {
        let mut bits = [Formula::FALSE; 4];
        for (index, bit) in bits.iter_mut().enumerate() {
            *bit = self.select(condition, yes.bits[index], no.bits[index])?;
        }
        Ok(Signal { bits })
    }

    pub fn powered(&mut self, signal: Signal) -> Result<Formula, SymbolicError> {
        signal.bits.into_iter().try_fold(Formula::FALSE, |result, bit| self.or(result, bit))
    }

    pub fn signal_equal(&mut self, left: Signal, right: Signal) -> Result<Formula, SymbolicError> {
        let mut equal = Formula::TRUE;
        for (a, b) in left.bits.into_iter().zip(right.bits) {
            let differs = self.xor(a, b)?;
            let same = self.not(differs)?;
            equal = self.and(equal, same)?;
        }
        Ok(equal)
    }

    pub fn maximum(&mut self, left: Signal, right: Signal) -> Result<Signal, SymbolicError> {
        if left == right || left == Signal::literal(0)? || right == Signal::literal(0)? {
            for bit in left.bits.into_iter().chain(right.bits) {
                self.validate(bit)?;
            }
            return Ok(if left == Signal::literal(0)? { right } else { left });
        }
        let mut borrow = Formula::FALSE;
        for (a, b) in left.bits.into_iter().zip(right.bits) {
            let not_a = self.not(a)?;
            let either = self.or(b, borrow)?;
            let take = self.and(not_a, either)?;
            let both = self.and(b, borrow)?;
            borrow = self.or(take, both)?;
        }
        self.signal_select(borrow, right, left)
    }

    pub fn attenuate(&mut self, signal: Signal, amount: u8) -> Result<Signal, SymbolicError> {
        if amount == 0 {
            for value in signal.bits {
                self.validate(value)?;
            }
            return Ok(signal);
        }
        if amount >= 15 {
            for value in signal.bits {
                self.validate(value)?;
            }
            return Signal::literal(0);
        }
        let mut borrow = Formula::FALSE;
        let mut bits = [Formula::FALSE; 4];
        for (index, bit) in bits.iter_mut().enumerate() {
            let value = signal.bits[index];
            let difference = self.xor(value, borrow)?;
            let inverse = self.not(value)?;
            if amount & (1 << index) == 0 {
                *bit = difference;
                borrow = self.and(inverse, borrow)?;
            } else {
                *bit = self.not(difference)?;
                borrow = self.or(inverse, borrow)?;
            }
        }
        self.signal_select(borrow, Signal::literal(0)?, Signal { bits })
    }

    /// Lower to the existing pure IR; root order and duplicates remain observable.
    pub fn circuit(&self, roots: &[Formula]) -> Result<Circuit, SymbolicError> {
        self.lower_circuit(roots, true)
    }

    /// Lower only reachable input bindings, retaining their original identifiers and root order.
    pub fn circuit_live_inputs(&self, roots: &[Formula]) -> Result<Circuit, SymbolicError> {
        self.lower_circuit(roots, false)
    }

    fn lower_circuit(&self, roots: &[Formula], retain_declared_inputs: bool) -> Result<Circuit, SymbolicError> {
        for &root in roots {
            self.validate(root)?;
        }
        let mut live = vec![false; self.nodes.len()];
        let mut live_inputs = BTreeSet::new();
        let mut pending = roots.to_vec();
        while let Some(value) = pending.pop() {
            let index = value.0 as usize;
            if std::mem::replace(&mut live[index], true) || index < 2 {
                continue;
            }
            match self.nodes[index] {
                Node::Decision(node) => {
                    live_inputs.insert(node.variable);
                    pending.extend([node.low, node.high]);
                }
                Node::Selection(node) => pending.extend([node.condition, node.yes, node.no]),
            }
        }
        let inputs: Vec<_> = if retain_declared_inputs {
            self.inputs.iter().copied().collect()
        } else {
            live_inputs.into_iter().collect()
        };
        let mut ops: Vec<_> = inputs.iter().copied().map(Op::Input).collect();
        let variables: HashMap<_, _> = inputs.iter().enumerate().map(|(net, &input)| (input, net)).collect();
        let mut translated = vec![usize::MAX; self.nodes.len()];
        translated[0] = ops.len();
        translated[1] = ops.len() + 1;
        ops.extend([Op::Const(false), Op::Const(true)]);
        // Proof scratch must not survive indirectly through Plan's original-net layout map.
        for (index, node) in self.nodes.iter().enumerate().skip(2).filter(|(index, _)| live[*index]) {
            translated[index] = ops.len();
            let (condition, yes, no) = match node {
                Node::Decision(node) => (
                    variables[&node.variable],
                    translated[node.high.0 as usize],
                    translated[node.low.0 as usize],
                ),
                Node::Selection(node) => (
                    translated[node.condition.0 as usize],
                    translated[node.yes.0 as usize],
                    translated[node.no.0 as usize],
                ),
            };
            ops.push(Op::Select(condition, yes, no));
        }
        Ok(Circuit::new(inputs, ops, roots.iter().map(|root| translated[root.0 as usize]).collect()))
    }

    fn known(&self, selection: Selection) -> Option<Formula> {
        match selection {
            Selection {
                condition: Formula::TRUE,
                yes,
                ..
            } => Some(yes),
            Selection {
                condition: Formula::FALSE,
                no,
                ..
            } => Some(no),
            Selection { yes, no, .. } if yes == no => Some(yes),
            Selection {
                condition,
                yes: Formula::TRUE,
                no: Formula::FALSE,
            } => Some(condition),
            _ => self.selections.get(&selection).copied(),
        }
    }

    fn cofactor(&self, selection: Selection, variable: InputId, high: bool) -> Selection {
        let branch = |value: Formula| {
            let node = self.ordered_decision(value);
            if node.variable == variable {
                if high { node.high } else { node.low }
            } else {
                value
            }
        };
        Selection {
            condition: branch(selection.condition),
            yes: branch(selection.yes),
            no: branch(selection.no),
        }
    }

    fn decision(&mut self, variable: InputId, low: Formula, high: Formula) -> Result<Formula, SymbolicError> {
        if low == high {
            return Ok(low);
        }
        let node = Decision { variable, low, high };
        if let Some(&value) = self.unique.get(&node) {
            return Ok(value);
        }
        self.charge()?;
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(SymbolicError::BudgetExceeded);
        }
        let value = Formula(u32::try_from(self.nodes.len()).map_err(|_| SymbolicError::BudgetExceeded)?);
        self.nodes.push(Node::Decision(node));
        self.unique.insert(node, value);
        Ok(value)
    }

    fn structural_selection(&mut self, selection: Selection) -> Result<Formula, SymbolicError> {
        if let Some(value) = self.known(selection) {
            return Ok(value);
        }
        self.charge()?;
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(SymbolicError::BudgetExceeded);
        }
        let value = Formula(u32::try_from(self.nodes.len()).map_err(|_| SymbolicError::BudgetExceeded)?);
        self.nodes.push(Node::Selection(selection));
        self.selections.insert(selection, value);
        Ok(value)
    }

    fn ordered_decision(&self, value: Formula) -> Decision {
        match self.nodes[value.0 as usize] {
            Node::Decision(node) => node,
            Node::Selection(_) => unreachable!("ordered-decision algorithms require a canonical diagram"),
        }
    }

    fn complements(&self, left: Formula, right: Formula) -> bool {
        self.negated(left) == Some(right) || self.negated(right) == Some(left)
    }

    fn negated(&self, value: Formula) -> Option<Formula> {
        match self.nodes[value.0 as usize] {
            Node::Selection(Selection {
                condition,
                yes: Formula::FALSE,
                no: Formula::TRUE,
            }) => Some(condition),
            _ => None,
        }
    }

    fn conjunction(&self, value: Formula) -> Option<(Formula, Formula)> {
        match self.nodes[value.0 as usize] {
            Node::Selection(Selection {
                condition,
                yes,
                no: Formula::FALSE,
            }) => Some((condition, yes)),
            _ => None,
        }
    }

    fn disjunction(&self, value: Formula) -> Option<(Formula, Formula)> {
        match self.nodes[value.0 as usize] {
            Node::Selection(Selection {
                condition,
                yes: Formula::TRUE,
                no,
            }) => Some((condition, no)),
            _ => None,
        }
    }

    // Bounded Boolean structure proves some dead paths without constructing a canonical relation.
    fn structurally_implies(&self, left: Formula, right: Formula) -> bool {
        self.implies_with_budget(left, right, &mut 64)
    }

    fn implies_with_budget(&self, left: Formula, right: Formula, remaining: &mut usize) -> bool {
        if *remaining == 0 {
            return false;
        }
        *remaining -= 1;
        if left == right || left == Formula::FALSE || right == Formula::TRUE {
            return true;
        }
        if let (Some(a), Some(b)) = (self.negated(left), self.negated(right)) {
            return self.implies_with_budget(b, a, remaining);
        }
        if let Some((a, b)) = self.conjunction(right)
            && self.implies_with_budget(left, a, remaining)
            && self.implies_with_budget(left, b, remaining)
        {
            return true;
        }
        if let Some((a, b)) = self.disjunction(left)
            && self.implies_with_budget(a, right, remaining)
            && self.implies_with_budget(b, right, remaining)
        {
            return true;
        }
        if let Some((a, b)) = self.conjunction(left)
            && (self.implies_with_budget(a, right, remaining) || self.implies_with_budget(b, right, remaining))
        {
            return true;
        }
        if let Some((a, b)) = self.disjunction(right)
            && (self.implies_with_budget(left, a, remaining) || self.implies_with_budget(left, b, remaining))
        {
            return true;
        }
        false
    }

    fn commutative_operands(&self, left: Formula, right: Formula) -> (Formula, Formula) {
        if self.structural && left.0 > right.0 {
            (right, left)
        } else {
            (left, right)
        }
    }

    fn validate(&self, value: Formula) -> Result<(), SymbolicError> {
        if (value.0 as usize) < self.nodes.len() {
            Ok(())
        } else {
            Err(SymbolicError::InvalidFormula)
        }
    }

    fn charge(&mut self) -> Result<(), SymbolicError> {
        if self.work >= self.limits.max_work {
            return Err(SymbolicError::BudgetExceeded);
        }
        self.work += 1;
        Ok(())
    }
}

#[cfg(test)]
#[path = "symbolic_tests.rs"]
mod tests;
