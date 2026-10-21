//! A settled computation and its explicit connections to the physical circuit.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledCircuit {
    pub(crate) program: ReducedLayerProgram,
    pub(crate) outputs: Box<[SettledOutput]>,
    /// These observations/effects still require a boundary adapter; they are never discarded as resets.
    pub(crate) retained_effects: Box<[BoundaryPort]>,
    /// Unsupported roots retain ordinary execution instead of acquiring invented logical state.
    pub(crate) declines: Box<[String]>,
    /// The small physical timing island retained by the stable-device contract.
    pub(crate) timing: Option<SettledTimingIsland>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettledTarget {
    Power(PowerTarget),
    Register(BudMemoryRead),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettledValueKind {
    Bool,
    Strength,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledOutput {
    pub(crate) target: SettledTarget,
    pub(crate) kind: SettledValueKind,
    /// Bool has one root; strength has the fifteen shared predicates `value > 0..14`.
    pub(crate) roots: Box<[LayerExprId]>,
}

impl SettledCircuit {
    pub(crate) fn validate(&self) -> Result<(), String> {
        LogicalValues::new(&self.program)?;
        for output in &self.outputs {
            let count = match output.kind {
                SettledValueKind::Bool => 1,
                SettledValueKind::Strength => 15,
            };
            if output.roots.len() != count || output.roots.iter().any(|root| root.0 as usize >= self.program.expressions.len()) {
                return Err("settled circuit has an invalid output binding".into());
            }
        }
        Ok(())
    }
}

// shortcut: the compiler publishes this executable artifact without replacing Graph;
// activate it after clock publication and interpreter reconstruction have an adapter.
#[cfg_attr(not(test), expect(dead_code, reason = "Settled execution awaits its physical boundary adapter."))]
#[derive(Debug)]
pub(crate) struct SettledEvaluation {
    values: LogicalValues,
    outputs: Box<[SettledOutput]>,
    results: Box<[u8]>,
}

#[cfg_attr(not(test), expect(dead_code, reason = "Settled execution awaits its physical boundary adapter."))]
impl SettledEvaluation {
    pub(crate) fn new(circuit: &SettledCircuit) -> Result<Self, String> {
        circuit.validate()?;
        Ok(Self {
            values: LogicalValues::new(&circuit.program)?,
            outputs: circuit.outputs.clone(),
            results: vec![0; circuit.outputs.len()].into_boxed_slice(),
        })
    }

    pub(crate) fn evaluate(&mut self, inputs: &[bool]) -> Result<usize, &'static str> {
        let operations = self.values.evaluate(inputs)?;
        for (output, value) in self.outputs.iter().zip(self.results.iter_mut()) {
            *value = output.roots.iter().filter(|root| self.values.values()[root.0 as usize]).count() as u8;
        }
        Ok(operations)
    }

    pub(crate) fn outputs(&self) -> &[u8] {
        &self.results
    }

    #[cfg(test)]
    pub(crate) fn set_heap_reference_for_test(&mut self, enabled: bool) {
        self.values.set_heap_reference_for_test(enabled);
    }
}

#[cfg(test)]
#[path = "settled_circuit/tests.rs"]
mod tests;
