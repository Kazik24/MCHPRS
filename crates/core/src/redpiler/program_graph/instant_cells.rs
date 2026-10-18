//! Graph-derived local equations for a stationary pull-computation wave.
//!
//! These are compile-time contracts, not executable kernels or a lifecycle certificate.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CellPinId(
    /// This indexes one earlier pin in the containing cell's equation table.
    pub(crate) u32,
);

impl CellPinId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellGateKind {
    /// This forwards the value of one operand.
    Buffer,
    /// This complements one value without generating an activation event.
    Invert,
    /// This requires every operand; the empty conjunction is true.
    All,
    /// This accepts any operand; the empty disjunction is false.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellRead {
    /// This reads a guarded directional contribution, positive only above its attenuation.
    Contribution(PowerLink),
    /// This means a qualifying delivered event sampled this actor, independently of its power.
    Activation(PistonId),
    /// This reads existing local actor/resource state, never a copied wire-state input.
    State(StateTest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellPin {
    /// This preserves one typed value/event/state dependency from the prepared graph.
    Read(CellRead),
    Gate {
        /// This selects the value operation derived from the local physical rule.
        kind: CellGateKind,
        /// These earlier pins are the operation's operands in the same cell.
        inputs: Box<[CellPinId]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellUse {
    /// This actor contributes one pull-computation use under the stationary entry contract.
    pub(crate) actor: PistonId,
    /// This existing observer/reset clock provides reset provenance, not a new clock owner.
    pub(crate) reset: NodeId,
    /// This combines Main contributions before Quasi, preserving their typed source ports.
    pub(crate) powered: CellPinId,
    /// This combines activation, an available extended base and absent sampled power.
    pub(crate) requested: CellPinId,
    /// This rechecks power, pose and payload destinations against preceding accepted writes.
    pub(crate) valid: CellPinId,
    /// These original callback occurrences can qualify a sample; guards and views stay in the graph.
    pub(crate) activations: Box<[RouteOccurrence]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellResourcePin {
    /// This is the one conserved payload; shared actors do not receive separate tokens.
    pub(crate) resource: ResourceId,
    /// This is the common stationary source contact from which admitted actors pull it.
    pub(crate) source: ContactId,
    /// This derives disappearance from selected contact and electrical availability.
    pub(crate) absent: CellPinId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellContract {
    /// These graph-derived uses retain separate requests and acceptance predicates.
    pub(crate) uses: Box<[CellUse]>,
    /// These topological local equations derive values from typed reads and simple gates.
    pub(crate) pins: Box<[CellPin]>,
    /// This requires all bases extended/ready and all payloads stationary at their common Far.
    pub(crate) stationary_entry: CellPinId,
    /// These source-absence outputs can be connected to other cells without copying payloads.
    pub(crate) resources: Box<[CellResourcePin]>,
}

impl CellContract {
    /// Evaluate local equations in tests/proofs; production lowering substitutes their typed reads.
    pub(crate) fn evaluate(&self, values: &mut Vec<bool>, mut read: impl FnMut(CellRead) -> bool) -> Result<(), &'static str> {
        values.clear();
        for pin in &self.pins {
            let value = match pin {
                CellPin::Read(source) => read(*source),
                CellPin::Gate { kind, inputs } => {
                    if inputs.iter().any(|input| input.index() >= values.len()) {
                        return Err("cell pin references a missing or later operand");
                    }
                    match kind {
                        CellGateKind::Buffer if inputs.len() == 1 => values[inputs[0].index()],
                        CellGateKind::Invert if inputs.len() == 1 => !values[inputs[0].index()],
                        CellGateKind::All => inputs.iter().all(|input| values[input.index()]),
                        CellGateKind::Any => inputs.iter().any(|input| values[input.index()]),
                        _ => return Err("unary cell gate has the wrong arity"),
                    }
                }
            };
            values.push(value);
        }
        Ok(())
    }
}
