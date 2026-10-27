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
    pub(crate) program: KernelProgram,
    /// These sorted actor IDs delimit the kernel's exclusive computation ownership.
    pub(crate) actors: Box<[PistonId]>,
    /// These original wire levels validate a settled entry without creating walk snapshots.
    pub(crate) wires: Box<[(NodeId, u8)]>,
    /// These sorted routes keep reset notifications and replace owned computation callbacks.
    pub(crate) routes: Box<[KernelRoute]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KernelProgram {
    /// A composed pull wave accelerates only requests Graph actually pops in FIFO order.
    PullWave {
        program: ReducedLayerProgram,
        stationary: LayerExprId,
        pulls: Box<[KernelPull]>,
    },
    /// Independent cell entries consume only the already-popped request; wires remain live.
    Events(Box<[InstantEventEntry]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantEventEntry {
    /// This actor owns the original action and all mutable pose/publication state.
    pub(crate) actor: PistonId,
    /// These live state reads and pure gates check resources after ready/unpowered entry checks.
    pub(crate) pins: Box<[CellPin]>,
    /// This is the source cell's remaining validity root after the runtime entry checks.
    pub(crate) valid: CellPinId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelPull {
    /// This original actor owns all pose, token and deferred-publication state.
    pub(crate) actor: PistonId,
    /// This root records whether the original callback requested this pull.
    pub(crate) requested: LayerExprId,
    /// This root records live revalidation after the preceding conditional pulls.
    pub(crate) accepted: LayerExprId,
    /// This states whether Graph already queued one of several boundary sources, or an exact
    /// internal callback cut will enqueue the request with its original source.
    pub(crate) origin: KernelPullOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KernelPullOrigin {
    /// These are alternative external callback routes that can supply this OR-sampled request.
    Boundary(Box<[RouteOccurrence]>),
    /// This callback source and its exact enqueue cuts recreate one internal request.
    Internal {
        cause: RouteOccurrence,
        enqueue_triggers: Box<[RouteOccurrence]>,
    },
}

impl KernelPullOrigin {
    pub(crate) fn is_boundary(&self) -> bool {
        matches!(self, Self::Boundary(_))
    }

    pub(crate) fn source_matches(&self, source: Option<RouteId>) -> bool {
        let Some(source) = source else { return false };
        match self {
            Self::Boundary(alternatives) => alternatives.iter().any(|event| event.route == source),
            Self::Internal { cause, .. } => cause.route == source,
        }
    }

    pub(crate) fn internal_cause(&self) -> Option<RouteOccurrence> {
        match self {
            Self::Boundary(_) => None,
            Self::Internal { cause, .. } => Some(*cause),
        }
    }

    pub(crate) fn enqueue_triggers(&self) -> &[RouteOccurrence] {
        match self {
            Self::Boundary(_) => &[],
            Self::Internal { enqueue_triggers, .. } => enqueue_triggers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelRouteStep {
    /// This keeps effects at their original callback cut, including duplicate route rows.
    pub(crate) ordinal: u32,
    pub(crate) kind: KernelRouteStepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KernelRouteStepKind {
    /// This certified source callback replaces only its private corridor's synchronous walk.
    ClearWires(Box<[NodeId]>),
    /// This is one internal request whose original callback source is its pull's internal cause.
    EnqueuePull(usize),
    /// This original row retains native callback guard, receiver and route occurrence behavior.
    Deliver(RouteDelivery),
    /// This callback can change later reads; Graph resumes the whole original route here.
    NativeHandoff(RouteDelivery),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelRoute {
    /// This is the original synchronous computation route being replaced.
    pub(crate) route: RouteId,
    /// These ordered operations preserve clear, request and callback effects at their source cut.
    pub(crate) steps: Box<[KernelRouteStep]>,
}
