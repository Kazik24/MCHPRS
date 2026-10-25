//! A settled computation and its explicit connections to the physical circuit.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledCircuit {
    pub(crate) program: ReducedLayerProgram,
    pub(crate) outputs: Box<[SettledOutput]>,
    /// Optional one-shot firing/reset bindings over roots in this same program.
    pub(crate) firing: Option<LogicalFiringPlan>,
    /// Source-certified local cells whose event timing remains owned by Graph.
    pub(crate) retained_cells: Box<[CellUse]>,
    /// These observations/effects still require a boundary adapter; they are never discarded as resets.
    pub(crate) retained_effects: Box<[BoundaryPort]>,
    /// Unsupported roots retain ordinary execution instead of acquiring invented logical state.
    pub(crate) declines: Box<[String]>,
    /// Source topology used to qualify the logical clock family at activation.
    pub(crate) timing: Option<SettledTimingIsland>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettledTarget {
    Power(PowerTarget),
    Register(BudMemoryRead),
    /// This is `!fired && Main == 0 && Quasi == 0`, not a route, request, or accepted action.
    InstantPowerEligible(PistonId),
    /// A recognized two-contact payload whose stationary source occupancy is logical state.
    ResourceState {
        resource: ResourceId,
        source: ContactId,
    },
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
        LogicalValues::validate_program(&self.program)?;
        for output in &self.outputs {
            let count = match output.kind {
                SettledValueKind::Bool => 1,
                SettledValueKind::Strength => 15,
            };
            if output.roots.len() != count || output.roots.iter().any(|root| root.0 as usize >= self.program.expressions.len()) {
                return Err("settled circuit has an invalid output binding".into());
            }
        }
        self.validate_firing_binding()?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct SettledEvaluation {
    values: LogicalValues,
    outputs: Box<[SettledOutput]>,
    results: Box<[u8]>,
}

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
