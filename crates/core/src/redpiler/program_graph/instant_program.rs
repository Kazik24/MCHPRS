//! Compact expressions and observable roots, separate from cold layer construction evidence.
use super::*;

/// Compact executable expression program; it owns no wire, walk, capture or cell-pin table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReducedLayerProgram {
    /// These are the remaining boundary and small state operands, in dense input order.
    pub(crate) inputs: Box<[LayerInput]>,
    /// These shared operations are evaluated once in their topological order.
    pub(crate) expressions: Box<[LayerExpr]>,
    /// These ordered roots retain outputs, memory writes and accepted-use reset obligations.
    pub(crate) roots: Box<[(LayerRoot, LayerExprId)]>,
}

/// Meaning of an observable expression; duplicate boundary outputs keep separate entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayerRoot {
    /// This guards admission at the declared stationary wave entry.
    StationaryEntry,
    /// This indexes the original region output, preserving callback multiplicity and order.
    Output(usize),
    /// This indexes a selected resource transfer, including its unique owner destination.
    Transfer(usize),
    /// This indexes a resource's resulting source-vacancy predicate.
    SourceAbsent(usize),
    /// This accepted computation occurrence retains its original reset/publication obligation.
    Accepted { layer: usize, occurrence: usize },
}

impl ReducedLayerProgram {
    /// Evaluate without allocation after the caller reserves one Boolean slot per operation.
    pub(crate) fn evaluate(&self, scratch: &mut [bool], mut input: impl FnMut(LayerInput) -> bool) -> Result<(), &'static str> {
        if scratch.len() < self.expressions.len() {
            return Err("layer evaluation scratch is too short");
        }
        for (index, expression) in self.expressions.iter().enumerate() {
            scratch[index] = match expression {
                LayerExpr::Constant(value) => *value,
                LayerExpr::Input(id) => input(*self.inputs.get(id.0 as usize).ok_or("layer input is missing")?),
                LayerExpr::Not(value) if (value.0 as usize) < index => !scratch[value.0 as usize],
                LayerExpr::All(values) if values.iter().all(|value| (value.0 as usize) < index) => {
                    values.iter().all(|value| scratch[value.0 as usize])
                }
                LayerExpr::Any(values) if values.iter().all(|value| (value.0 as usize) < index) => {
                    values.iter().any(|value| scratch[value.0 as usize])
                }
                _ => return Err("layer operation has a missing or forward operand"),
            };
        }
        Ok(())
    }
}
