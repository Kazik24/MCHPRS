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
    /// This preserves whether the original callback enqueued this candidate request.
    Requested { layer: usize, occurrence: usize },
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

#[derive(Debug, Clone, PartialEq, Eq)]
/// A reduced pull wave; ordinary Graph retains reset, presentation and handoff state.
pub(crate) struct InstantKernel {
    /// These sorted outside routes can each start a wave; mixed entry queues retain Graph.
    pub(crate) entries: Box<[RouteId]>,
    /// These Boolean operations contain boundary reads rather than internal wire reads.
    pub(crate) program: ReducedLayerProgram,
    /// This root requires the original stationary entry before any state is changed.
    pub(crate) stationary: LayerExprId,
    /// These conditional pulls retain the source-derived FIFO acceptance order.
    pub(crate) pulls: Box<[KernelPull]>,
    /// These sorted actor IDs delimit the kernel's exclusive computation ownership.
    pub(crate) actors: Box<[PistonId]>,
    /// These original wire levels validate a settled entry without creating walk snapshots.
    pub(crate) wires: Box<[(NodeId, u8)]>,
    /// These sorted routes keep reset notifications and replace owned computation callbacks.
    pub(crate) routes: Box<[KernelRoute]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KernelPull {
    /// This original actor owns all pose, token and deferred-publication state.
    pub(crate) actor: PistonId,
    /// This root records whether the original callback requested this pull.
    pub(crate) requested: LayerExprId,
    /// This root records live revalidation after the preceding conditional pulls.
    pub(crate) accepted: LayerExprId,
    /// This marks requests already present in Graph's queue when the wave starts.
    pub(crate) external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelRoute {
    /// This is the original synchronous computation route being replaced.
    pub(crate) route: RouteId,
    /// These private wires become unpowered when their certified source is removed.
    pub(crate) clear_wires: Box<[NodeId]>,
    /// These original rows retain reset scheduling and callback multiplicity.
    pub(crate) deliveries: Box<[RouteDelivery]>,
}
