//! Bounded symbolic values and ordered state/effect IR for instant-region tracing.

use super::*;
use std::collections::HashMap;

/// An index into a program's topologically ordered expression array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ExprId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ValueKind {
    /// This value is a two-state control condition.
    Boolean,
    /// This value is a redstone signal in the inclusive range zero through fifteen.
    Signal,
}

/// One evaluated Boolean or four-bit signal value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolicValue {
    /// This is a two-state control or power predicate.
    Boolean(bool),
    /// This is a redstone signal in the inclusive range zero through fifteen.
    Signal(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Expr {
    /// This is a declared region input in its boundary order.
    Input { port: u16, kind: ValueKind },
    /// This is a persistent boundary state read in its slot order.
    State { slot: u16, kind: ValueKind },
    /// This is a Boolean literal.
    Boolean(bool),
    /// This is a four-bit signal literal.
    Signal(u8),
    /// This negates one Boolean expression.
    Not(ExprId),
    /// This is the conjunction of two Boolean expressions.
    And(ExprId, ExprId),
    /// This is the disjunction of two Boolean expressions.
    Or(ExprId, ExprId),
    /// This is the exclusive-or of two Boolean expressions.
    Xor(ExprId, ExprId),
    /// This selects the true or false value while retaining the selected value's kind.
    Select {
        condition: ExprId,
        yes: ExprId,
        no: ExprId,
        kind: ValueKind,
    },
    /// This tests whether a four-bit redstone signal is nonzero.
    Powered(ExprId),
    /// This is the strongest of two signal values.
    MaxSignal(ExprId, ExprId),
    /// This subtracts fixed attenuation with a floor at zero.
    Attenuate { signal: ExprId, amount: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SymbolicLimits {
    /// This bounds interned expression nodes before the caller declines the candidate.
    pub(crate) max_expressions: usize,
    /// This bounds retained ordered effects before the caller declines the candidate.
    pub(crate) max_effects: usize,
}

impl Default for SymbolicLimits {
    fn default() -> Self {
        Self {
            max_expressions: 65_536,
            max_effects: 16_384,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolicError {
    /// The candidate exceeded an explicit construction budget.
    BudgetExceeded,
    /// An operation received an expression of the wrong value kind.
    TypeMismatch,
    /// A signal literal exceeded the modeled four-bit domain.
    InvalidSignal,
    /// An input or state slot was not declared before it was read.
    InvalidPort,
    /// The caller did not provide every declared input and state value.
    MissingValue,
    /// The reusable output buffer cannot hold every expression value.
    ScratchTooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolicDecline {
    /// The requested graph input has no owning instant region.
    TargetOutsideRegion,
    /// An internal feedback link has not been symbolically traced yet.
    InternalPowerLink,
    /// A guarded link needs state/site predicates outside this first operation set.
    GuardedPowerLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicPowerQuery {
    /// This preserves the graph-derived boundary ports used by the query.
    pub(crate) boundary: KernelBoundary,
    /// This is the compiled power aggregation and nonzero test.
    pub(crate) program: SymbolicProgram,
    /// This identifies the graph target whose Main/Quasi power is queried.
    pub(crate) target: NodeId,
    /// This is the Boolean root returned by the evaluator.
    pub(crate) powered: ExprId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardedWrite {
    /// This identifies the persistent state slot being updated.
    pub(crate) slot: u16,
    /// This is the branch condition under which the assignment occurs.
    pub(crate) guard: ExprId,
    /// This is the sequentially merged value after this assignment.
    pub(crate) value: ExprId,
    /// This is the source-order position of the assignment.
    pub(crate) sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectKind {
    /// This delivers a retained callback or outside signal observation.
    Boundary,
    /// This records a required scheduled phase obligation.
    Schedule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicEffect {
    /// This is the path condition under which the effect occurs.
    pub(crate) guard: ExprId,
    /// This classifies delivery versus retained scheduled work.
    pub(crate) kind: EffectKind,
    /// This identifies the outside port receiving the effect.
    pub(crate) destination: KernelOutput,
    /// These are symbolic values delivered with this occurrence.
    pub(crate) operands: Box<[ExprId]>,
    /// This is the causal order; equal effects at different positions stay distinct.
    pub(crate) sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicProgram {
    /// These are deterministic, hash-consed Boolean and signal expressions.
    pub(crate) expressions: Box<[Expr]>,
    /// These are sequential state assignments in source order.
    pub(crate) writes: Box<[GuardedWrite]>,
    /// These are observable or scheduled effect occurrences in causal order.
    pub(crate) effects: Box<[SymbolicEffect]>,
    /// These roots are evaluated during ordinary ticking.
    pub(crate) hot_roots: Box<[ExprId]>,
    /// These roots are evaluated only for projection or handoff.
    pub(crate) cold_roots: Box<[ExprId]>,
}

impl ExprId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

impl SymbolicProgram {
    /// Evaluate the topologically ordered DAG into caller-owned scratch without allocating.
    pub(crate) fn evaluate(&self, inputs: &[SymbolicValue], state: &[SymbolicValue], scratch: &mut [SymbolicValue]) -> Result<(), SymbolicError> {
        if scratch.len() < self.expressions.len() {
            return Err(SymbolicError::ScratchTooSmall);
        }
        for (index, expression) in self.expressions.iter().enumerate() {
            let value = match *expression {
                Expr::Input { port, kind } => *inputs
                    .get(usize::from(port))
                    .filter(|value| value.valid_for(kind))
                    .ok_or(SymbolicError::MissingValue)?,
                Expr::State { slot, kind } => *state
                    .get(usize::from(slot))
                    .filter(|value| value.valid_for(kind))
                    .ok_or(SymbolicError::MissingValue)?,
                Expr::Boolean(value) => SymbolicValue::Boolean(value),
                Expr::Signal(value) if value <= 15 => SymbolicValue::Signal(value),
                Expr::Signal(_) => return Err(SymbolicError::InvalidSignal),
                Expr::Not(value) => SymbolicValue::Boolean(!scratch[value.0 as usize].boolean()?),
                Expr::And(left, right) => SymbolicValue::Boolean(scratch[left.0 as usize].boolean()? && scratch[right.0 as usize].boolean()?),
                Expr::Or(left, right) => SymbolicValue::Boolean(scratch[left.0 as usize].boolean()? || scratch[right.0 as usize].boolean()?),
                Expr::Xor(left, right) => SymbolicValue::Boolean(scratch[left.0 as usize].boolean()? ^ scratch[right.0 as usize].boolean()?),
                Expr::Select { condition, yes, no, kind } => {
                    let selected = if scratch[condition.0 as usize].boolean()? { yes } else { no };
                    let value = scratch[selected.0 as usize];
                    if value.kind() != kind {
                        return Err(SymbolicError::TypeMismatch);
                    }
                    value
                }
                Expr::Powered(signal) => SymbolicValue::Boolean(scratch[signal.0 as usize].signal()? != 0),
                Expr::MaxSignal(left, right) => SymbolicValue::Signal(scratch[left.0 as usize].signal()?.max(scratch[right.0 as usize].signal()?)),
                Expr::Attenuate { signal, amount } => SymbolicValue::Signal(scratch[signal.0 as usize].signal()?.saturating_sub(amount).min(15)),
            };
            scratch[index] = value;
        }
        Ok(())
    }
}

impl SymbolicValue {
    fn valid_for(self, kind: ValueKind) -> bool {
        match (self, kind) {
            (Self::Boolean(_), ValueKind::Boolean) => true,
            (Self::Signal(value), ValueKind::Signal) => value <= 15,
            _ => false,
        }
    }

    fn kind(self) -> ValueKind {
        match self {
            Self::Boolean(_) => ValueKind::Boolean,
            Self::Signal(_) => ValueKind::Signal,
        }
    }

    fn boolean(self) -> Result<bool, SymbolicError> {
        match self {
            Self::Boolean(value) => Ok(value),
            Self::Signal(_) => Err(SymbolicError::TypeMismatch),
        }
    }

    fn signal(self) -> Result<u8, SymbolicError> {
        match self {
            Self::Signal(value) => Ok(value),
            Self::Boolean(_) => Err(SymbolicError::TypeMismatch),
        }
    }
}

pub(crate) struct SymbolicBuilder {
    boundary: KernelBoundary,
    limits: SymbolicLimits,
    expressions: Vec<Expr>,
    interned: HashMap<Expr, ExprId>,
    state_kinds: Vec<ValueKind>,
    current_state: HashMap<u16, ExprId>,
    writes: Vec<GuardedWrite>,
    effects: Vec<SymbolicEffect>,
    hot_roots: Vec<ExprId>,
    cold_roots: Vec<ExprId>,
}

impl SymbolicBuilder {
    pub(crate) fn new(boundary: KernelBoundary, state_kinds: Vec<ValueKind>, limits: SymbolicLimits) -> Result<Self, SymbolicError> {
        if state_kinds.len() != boundary.state.len() {
            return Err(SymbolicError::InvalidPort);
        }
        Ok(Self {
            boundary,
            limits,
            expressions: Vec::new(),
            interned: HashMap::new(),
            state_kinds,
            current_state: HashMap::new(),
            writes: Vec::new(),
            effects: Vec::new(),
            hot_roots: Vec::new(),
            cold_roots: Vec::new(),
        })
    }

    pub(crate) fn input(&mut self, port: usize, kind: ValueKind) -> Result<ExprId, SymbolicError> {
        if port >= self.boundary.inputs.len() || port > u16::MAX as usize {
            return Err(SymbolicError::InvalidPort);
        }
        self.intern(Expr::Input { port: port as u16, kind })
    }

    pub(crate) fn state(&mut self, slot: usize) -> Result<ExprId, SymbolicError> {
        let kind = *self.state_kinds.get(slot).ok_or(SymbolicError::InvalidPort)?;
        let id = u16::try_from(slot).map_err(|_| SymbolicError::InvalidPort)?;
        if let Some(&current) = self.current_state.get(&id) {
            return Ok(current);
        }
        let expression = self.intern(Expr::State { slot: id, kind })?;
        self.current_state.insert(id, expression);
        Ok(expression)
    }

    pub(crate) fn boolean(&mut self, value: bool) -> Result<ExprId, SymbolicError> {
        self.intern(Expr::Boolean(value))
    }

    pub(crate) fn signal(&mut self, value: u8) -> Result<ExprId, SymbolicError> {
        if value > 15 {
            return Err(SymbolicError::InvalidSignal);
        }
        self.intern(Expr::Signal(value))
    }

    pub(crate) fn not(&mut self, value: ExprId) -> Result<ExprId, SymbolicError> {
        self.require(value, ValueKind::Boolean)?;
        self.intern(Expr::Not(value))
    }

    pub(crate) fn and(&mut self, left: ExprId, right: ExprId) -> Result<ExprId, SymbolicError> {
        self.boolean_pair(left, right)?;
        self.intern(Expr::And(left, right))
    }

    pub(crate) fn or(&mut self, left: ExprId, right: ExprId) -> Result<ExprId, SymbolicError> {
        self.boolean_pair(left, right)?;
        self.intern(Expr::Or(left, right))
    }

    pub(crate) fn xor(&mut self, left: ExprId, right: ExprId) -> Result<ExprId, SymbolicError> {
        self.boolean_pair(left, right)?;
        self.intern(Expr::Xor(left, right))
    }

    pub(crate) fn max_signal(&mut self, left: ExprId, right: ExprId) -> Result<ExprId, SymbolicError> {
        self.require(left, ValueKind::Signal)?;
        self.require(right, ValueKind::Signal)?;
        self.intern(Expr::MaxSignal(left, right))
    }

    pub(crate) fn attenuate(&mut self, signal: ExprId, amount: u8) -> Result<ExprId, SymbolicError> {
        self.require(signal, ValueKind::Signal)?;
        self.intern(Expr::Attenuate { signal, amount })
    }

    pub(crate) fn select(&mut self, condition: ExprId, yes: ExprId, no: ExprId) -> Result<ExprId, SymbolicError> {
        self.require(condition, ValueKind::Boolean)?;
        let kind = self.kind(yes)?;
        if self.kind(no)? != kind {
            return Err(SymbolicError::TypeMismatch);
        }
        self.intern(Expr::Select { condition, yes, no, kind })
    }

    pub(crate) fn powered(&mut self, signal: ExprId) -> Result<ExprId, SymbolicError> {
        self.require(signal, ValueKind::Signal)?;
        self.intern(Expr::Powered(signal))
    }

    /// Apply a guarded assignment after the current symbolic value, preserving sequential semantics.
    pub(crate) fn assign(&mut self, slot: usize, guard: ExprId, value: ExprId) -> Result<(), SymbolicError> {
        self.require(guard, ValueKind::Boolean)?;
        let id = u16::try_from(slot).map_err(|_| SymbolicError::InvalidPort)?;
        let kind = *self.state_kinds.get(slot).ok_or(SymbolicError::InvalidPort)?;
        if self.kind(value)? != kind {
            return Err(SymbolicError::TypeMismatch);
        }
        let old = self.state(slot)?;
        let merged = self.select(guard, value, old)?;
        self.current_state.insert(id, merged);
        let sequence = u32::try_from(self.writes.len()).map_err(|_| SymbolicError::BudgetExceeded)?;
        self.writes.push(GuardedWrite {
            slot: id,
            guard,
            value: merged,
            sequence,
        });
        Ok(())
    }

    /// Append an occurrence without deduplicating equal destination and operand tuples.
    pub(crate) fn effect(&mut self, guard: ExprId, kind: EffectKind, destination: KernelOutput, operands: Vec<ExprId>) -> Result<(), SymbolicError> {
        self.require(guard, ValueKind::Boolean)?;
        if self.effects.len() >= self.limits.max_effects {
            return Err(SymbolicError::BudgetExceeded);
        }
        if !self.boundary.outputs.contains(&destination) {
            return Err(SymbolicError::InvalidPort);
        }
        for operand in &operands {
            self.expression(*operand)?;
        }
        let sequence = u32::try_from(self.effects.len()).map_err(|_| SymbolicError::BudgetExceeded)?;
        self.effects.push(SymbolicEffect {
            guard,
            kind,
            destination,
            operands: operands.into_boxed_slice(),
            sequence,
        });
        Ok(())
    }

    pub(crate) fn hot_root(&mut self, expression: ExprId) -> Result<(), SymbolicError> {
        self.expression(expression)?;
        self.hot_roots.push(expression);
        Ok(())
    }

    pub(crate) fn cold_root(&mut self, expression: ExprId) -> Result<(), SymbolicError> {
        self.expression(expression)?;
        self.cold_roots.push(expression);
        Ok(())
    }

    pub(crate) fn finish(self) -> SymbolicProgram {
        SymbolicProgram {
            expressions: self.expressions.into_boxed_slice(),
            writes: self.writes.into_boxed_slice(),
            effects: self.effects.into_boxed_slice(),
            hot_roots: self.hot_roots.into_boxed_slice(),
            cold_roots: self.cold_roots.into_boxed_slice(),
        }
    }

    fn boolean_pair(&self, left: ExprId, right: ExprId) -> Result<(), SymbolicError> {
        self.require(left, ValueKind::Boolean)?;
        self.require(right, ValueKind::Boolean)
    }

    fn require(&self, expression: ExprId, expected: ValueKind) -> Result<(), SymbolicError> {
        if self.kind(expression)? == expected {
            Ok(())
        } else {
            Err(SymbolicError::TypeMismatch)
        }
    }

    fn kind(&self, expression: ExprId) -> Result<ValueKind, SymbolicError> {
        Ok(match self.expression(expression)? {
            Expr::Input { kind, .. } | Expr::State { kind, .. } | Expr::Select { kind, .. } => *kind,
            Expr::Boolean(_) | Expr::Not(_) | Expr::And(_, _) | Expr::Or(_, _) | Expr::Xor(_, _) | Expr::Powered(_) => ValueKind::Boolean,
            Expr::Signal(_) | Expr::MaxSignal(_, _) | Expr::Attenuate { .. } => ValueKind::Signal,
        })
    }

    fn expression(&self, id: ExprId) -> Result<&Expr, SymbolicError> {
        self.expressions.get(id.0 as usize).ok_or(SymbolicError::InvalidPort)
    }

    fn intern(&mut self, expression: Expr) -> Result<ExprId, SymbolicError> {
        if let Some(&id) = self.interned.get(&expression) {
            return Ok(id);
        }
        if self.expressions.len() >= self.limits.max_expressions || self.expressions.len() >= u32::MAX as usize {
            return Err(SymbolicError::BudgetExceeded);
        }
        let id = ExprId(self.expressions.len() as u32);
        self.expressions.push(expression);
        self.interned.insert(expression, id);
        Ok(id)
    }
}

/// Trace one component's Main/Quasi input query from its graph-derived external power ports.
///
/// This produces a read-only symbolic operation, not an activation kernel. Internal feedback
/// and guarded transfers decline until their owner state and ordered callback semantics are traced.
pub(crate) fn compile_power_query(
    graph: &CompileGraph,
    region: &InstantRegion,
    target: NodeId,
    limits: SymbolicLimits,
) -> Result<Result<SymbolicPowerQuery, SymbolicDecline>, SymbolicError> {
    // shortcut: trace only Always-guarded boundary links; upgrade when owner state and feedback are traced.
    if !region.actors.iter().any(|actor| graph.pistons[actor.index()].node == target)
        && !region.resets.contains(&target)
        && !region.wires.contains(&target)
    {
        return Ok(Err(SymbolicDecline::TargetOutsideRegion));
    }
    let boundary = KernelBoundary::from_region(region);
    let query_boundary = KernelBoundary {
        inputs: boundary.inputs.clone(),
        ..KernelBoundary::default()
    };
    let mut builder = SymbolicBuilder::new(query_boundary, vec![], limits)?;
    let mut strength = builder.signal(0)?;
    for (index, link) in graph.power_links.iter().enumerate() {
        let PowerTarget::Input {
            node,
            port: InputPort::Main | InputPort::Quasi,
        } = link.to
        else {
            continue;
        };
        if node != target {
            continue;
        }
        let Some(input) = boundary.inputs.iter().position(|input| *input == KernelInput::Power(index as u32)) else {
            return Ok(Err(SymbolicDecline::InternalPowerLink));
        };
        if graph.guards[link.guard.index()] != Guard::Always {
            return Ok(Err(SymbolicDecline::GuardedPowerLink));
        }
        let source = builder.input(input, ValueKind::Signal)?;
        let transferred = builder.attenuate(source, link.attenuation)?;
        strength = builder.max_signal(strength, transferred)?;
    }
    let powered = builder.powered(strength)?;
    builder.hot_root(powered)?;
    Ok(Ok(SymbolicPowerQuery {
        boundary,
        program: builder.finish(),
        target,
        powered,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_use_latest_symbolic_value_and_effects_keep_multiplicity() {
        let boundary = KernelBoundary {
            inputs: vec![KernelInput::Power(0)],
            state: vec![KernelStateSlot::Reset(NodeId::new(0))],
            outputs: vec![KernelOutput::Power(4)],
            ..KernelBoundary::default()
        };
        let mut builder = SymbolicBuilder::new(boundary, vec![ValueKind::Boolean], SymbolicLimits::default()).unwrap();
        let input = builder.input(0, ValueKind::Boolean).unwrap();
        let old = builder.state(0).unwrap();
        let yes = builder.boolean(true).unwrap();
        builder.assign(0, input, yes).unwrap();
        let after_first = builder.state(0).unwrap();
        let no = builder.boolean(false).unwrap();
        builder.assign(0, old, no).unwrap();
        let after_second = builder.state(0).unwrap();
        assert_ne!(old, after_first);
        assert_ne!(after_first, after_second);

        let destination = KernelOutput::Power(4);
        builder.effect(input, EffectKind::Boundary, destination, vec![after_second]).unwrap();
        builder.effect(input, EffectKind::Boundary, destination, vec![after_second]).unwrap();
        let program = builder.finish();
        assert_eq!(program.writes.iter().map(|write| write.sequence).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(program.effects.len(), 2);
        assert_eq!(program.effects[0].sequence, 0);
        assert_eq!(program.effects[1].sequence, 1);
    }

    #[test]
    fn signal_values_reject_boolean_operations_and_out_of_domain_constants() {
        let boundary = KernelBoundary {
            inputs: vec![KernelInput::Power(0)],
            ..KernelBoundary::default()
        };
        let mut builder = SymbolicBuilder::new(boundary, vec![], SymbolicLimits::default()).unwrap();
        let signal = builder.signal(15).unwrap();
        assert_eq!(builder.not(signal), Err(SymbolicError::TypeMismatch));
        assert_eq!(builder.signal(16), Err(SymbolicError::InvalidSignal));
        let attenuated = builder.attenuate(signal, 1).unwrap();
        assert_eq!(attenuated, builder.intern(Expr::Attenuate { signal, amount: 1 }).unwrap());
    }

    #[test]
    fn program_executes_symbolic_values_in_reusable_storage() {
        let boundary = KernelBoundary {
            inputs: vec![KernelInput::Power(0), KernelInput::Power(1)],
            ..KernelBoundary::default()
        };
        let mut builder = SymbolicBuilder::new(boundary, vec![], SymbolicLimits::default()).unwrap();
        let left = builder.input(0, ValueKind::Boolean).unwrap();
        let right = builder.input(1, ValueKind::Boolean).unwrap();
        let sum = builder.xor(left, right).unwrap();
        let carry = builder.and(left, right).unwrap();
        builder.hot_root(sum).unwrap();
        builder.hot_root(carry).unwrap();
        let program = builder.finish();
        let mut scratch = vec![SymbolicValue::Boolean(false); program.expressions.len()];
        for a in [false, true] {
            for b in [false, true] {
                program
                    .evaluate(&[SymbolicValue::Boolean(a), SymbolicValue::Boolean(b)], &[], &mut scratch)
                    .unwrap();
                assert_eq!(scratch[sum.0 as usize], SymbolicValue::Boolean(a ^ b));
                assert_eq!(scratch[carry.0 as usize], SymbolicValue::Boolean(a & b));
            }
        }
        assert_eq!(
            program.evaluate(&[SymbolicValue::Boolean(false)], &[], &mut scratch),
            Err(SymbolicError::MissingValue)
        );
        assert_eq!(
            program.evaluate(&[SymbolicValue::Boolean(false); 2], &[], &mut scratch[..1]),
            Err(SymbolicError::ScratchTooSmall)
        );
    }

    #[test]
    fn powered_tests_the_signal_domain_boundary() {
        let boundary = KernelBoundary {
            inputs: vec![KernelInput::Power(0)],
            ..KernelBoundary::default()
        };
        let mut builder = SymbolicBuilder::new(boundary, vec![], SymbolicLimits::default()).unwrap();
        let input = builder.input(0, ValueKind::Signal).unwrap();
        let powered = builder.powered(input).unwrap();
        builder.hot_root(powered).unwrap();
        let program = builder.finish();
        let mut scratch = vec![SymbolicValue::Boolean(false); program.expressions.len()];

        for (signal, expected) in [(0, false), (1, true), (15, true)] {
            program.evaluate(&[SymbolicValue::Signal(signal)], &[], &mut scratch).unwrap();
            assert_eq!(scratch[powered.index()], SymbolicValue::Boolean(expected));
        }
    }
}
