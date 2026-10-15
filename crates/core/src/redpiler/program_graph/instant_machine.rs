//! Bounded, exact transition programs for closed instant selections.
use super::*;
use mchprs_world::PistonAction;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct MachineState {
    /// These packed values retain every admitted component's electrical or wire state.
    pub(crate) components: Vec<u16>,
    /// These poses retain extension, head presence and logical availability for every actor.
    pub(crate) pistons: Vec<u8>,
    /// These retain each conserved payload's selected contact and enabled state.
    pub(crate) resources: Vec<(ContactId, bool)>,
    /// These relative deadlines retain scheduler order, duplicates and priorities.
    pub(crate) ticks: Vec<MachineTick>,
    /// These retain FIFO requests awaiting action acceptance.
    pub(crate) requests: Vec<(PistonId, PistonAction)>,
    /// These retain deferred publications, including stale revision witnesses.
    pub(crate) deferred: Vec<MachineDeferred>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MachineTick {
    /// This identifies the compiled receiver with its original registry type.
    pub(crate) node: NodeId,
    /// This is the remaining half-tick delay relative to the scheduler cursor.
    pub(crate) delay: u32,
    /// This retains the tick's dispatch priority.
    pub(crate) priority: TickPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MachineDeferred {
    /// This identifies the actor whose connection change will be published.
    pub(crate) piston: PistonId,
    /// This is the captured revision minus the actor's current revision, modulo 2^64.
    pub(crate) revision: u64,
    /// This retains the payload publication and its independent revision witness.
    pub(crate) resource: Option<(ResourceId, u64, ContactId)>,
    /// This is the publication deadline relative to the logical tick, modulo 2^64.
    pub(crate) delay: u64,
    /// This distinguishes extension publication from retraction publication.
    pub(crate) extending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MachineTransition {
    /// This identifies the exact normalized state after the operation.
    pub(crate) next: u32,
    /// These component writes replace internal propagation and retain final wire shapes/strengths.
    pub(crate) components: Vec<(NodeId, u16)>,
    /// These actor writes retain the final pose and add the recorded revision increment.
    pub(crate) pistons: Vec<(PistonId, u8, u64)>,
    /// These payload writes retain location/enabled state and add its revision increment.
    pub(crate) resources: Vec<(ResourceId, ContactId, bool, u64)>,
    /// These nodes were touched even if their final value equals their entry value.
    pub(crate) dirty: Vec<NodeId>,
    /// These ordered piston animations remain queued for the next world projection.
    pub(crate) effects: Vec<(PistonId, PistonAction, bool)>,
    /// This retains the number of scheduled obligations discharged by this transition.
    pub(crate) scheduled: u64,
    /// This retains accepted logical piston actions without replaying their handlers.
    pub(crate) accepted: u64,
    /// This retains logical connection publications without replaying their notifications.
    pub(crate) published: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantMachine {
    /// These lever owners define action columns 1..N; column zero advances one game tick.
    pub(crate) controls: Vec<NodeId>,
    /// These exact states retain all memory and future work needed to select a transition.
    pub(crate) states: Vec<MachineState>,
    /// These rows contain checked transitions; a missing entry retains ordinary Graph execution.
    pub(crate) transitions: Vec<Vec<Option<MachineTransition>>>,
    /// This complete-state lookup resolves hash collisions using structural equality.
    pub(crate) index: FxHashMap<MachineState, u32>,
    /// This compile-time certificate proves a compact held-input phase program.
    pub(crate) kernel: InstantKernel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Dense index into a shared semantic expression DAG.
pub(crate) struct KernelExprId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One scalar constant or a Boolean-controlled branch into the shared DAG.
pub(crate) enum KernelExpr {
    /// This is a scalar value shared by every frozen input assignment.
    Constant(u64),
    Select {
        /// This is the frozen data-control bit used to choose one child.
        control: u8,
        /// This child is selected when the control bit is false.
        low: KernelExprId,
        /// This child is selected when the control bit is true.
        high: KernelExprId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime-ready reduced execution program derived from exact canonical trajectories.
pub(crate) struct InstantKernel {
    /// This identifies the unique graph-derived falling activation control.
    pub(crate) trigger: NodeId,
    /// These are the frozen external data inputs in graph-derived order.
    pub(crate) data_controls: Box<[NodeId]>,
    /// This contains hash-consed scalar selectors shared by hot and cold roots.
    pub(crate) expressions: Box<[KernelExpr]>,
    /// This is the exact post-activation state relation for cold admission.
    pub(crate) entry: KernelStateExpr,
    /// These frames cover a finite prefix followed by one exactly repeated cycle.
    pub(crate) phases: Box<[KernelPhase]>,
    /// This is the first phase whose successor repeats within the common cycle.
    pub(crate) prefix: u16,
    /// This is the common cycle length across all admitted input assignments.
    pub(crate) period: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Scalar selectors for one exact normalized semantic state.
pub(crate) struct KernelStateExpr {
    /// These select all circuit component and wire values for cold reconstruction.
    pub(crate) components: Box<[KernelExprId]>,
    /// These select all piston poses for cold reconstruction.
    pub(crate) pistons: Box<[KernelExprId]>,
    /// These select each resource contact and enabled flag for cold reconstruction.
    pub(crate) resources: Box<[(KernelExprId, KernelExprId)]>,
    /// This selects exact ordered scheduler, request, and deferred future work.
    pub(crate) future: KernelFutureExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Expression-backed ordered future queues, evaluated only at projection or handoff.
pub(crate) struct KernelFutureExpr {
    /// This selects the active prefix length of the scheduler entries.
    pub(crate) tick_len: KernelExprId,
    /// These columns retain scheduler receiver, relative delay, and priority.
    pub(crate) ticks: Box<[KernelTickExpr]>,
    /// This selects the active prefix length of queued piston requests.
    pub(crate) request_len: KernelExprId,
    /// These columns retain FIFO request actor and action.
    pub(crate) requests: Box<[KernelRequestExpr]>,
    /// This selects the active prefix length of deferred publications.
    pub(crate) deferred_len: KernelExprId,
    /// These columns retain ordered deferred actor, revision, resource, deadline, and direction.
    pub(crate) deferred: Box<[KernelDeferredExpr]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One symbolic scheduler entry.
pub(crate) struct KernelTickExpr {
    /// This selects the compiled receiver node.
    pub(crate) node: KernelExprId,
    /// This selects the relative half-tick deadline.
    pub(crate) delay: KernelExprId,
    /// This selects the stable encoded scheduler priority.
    pub(crate) priority: KernelExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One symbolic FIFO piston request.
pub(crate) struct KernelRequestExpr {
    /// This selects the actor identifier.
    pub(crate) piston: KernelExprId,
    /// This selects the stable encoded piston action.
    pub(crate) action: KernelExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One symbolic deferred publication.
pub(crate) struct KernelDeferredExpr {
    /// This selects the actor identifier.
    pub(crate) piston: KernelExprId,
    /// This selects the revision difference from the live actor origin.
    pub(crate) revision: KernelExprId,
    /// This selects a resource identifier or the absent sentinel.
    pub(crate) resource: KernelExprId,
    /// This selects the resource revision difference or the absent sentinel.
    pub(crate) resource_revision: KernelExprId,
    /// This selects the resource contact or the absent sentinel.
    pub(crate) contact: KernelExprId,
    /// This selects the relative publication deadline.
    pub(crate) delay: KernelExprId,
    /// This selects the extension-versus-retraction flag.
    pub(crate) extending: KernelExprId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Hot transition roots plus a cold semantic state for one periodic phase.
pub(crate) struct KernelPhase {
    /// This selects only changed component values required by the boundary output cone.
    pub(crate) components: Box<[KernelComponentDelta]>,
    /// These selectors retain touched nodes for append-only flush behavior.
    pub(crate) dirty: Box<[(NodeId, KernelExprId)]>,
    /// These increments reconstruct exact actor revisions from live origins at handoff.
    pub(crate) piston_revisions: Box<[(PistonId, KernelExprId)]>,
    /// These increments reconstruct exact resource revisions from live origins at handoff.
    pub(crate) resource_revisions: Box<[(ResourceId, KernelExprId)]>,
    /// These guarded events preserve each valuation's exact ordered presentation sequence.
    pub(crate) effects: Box<[KernelEffect]>,
    /// This selects logical scheduled work discharged by the tick.
    pub(crate) scheduled: KernelExprId,
    /// This selects accepted logical piston actions.
    pub(crate) accepted: KernelExprId,
    /// This selects logical connection publications.
    pub(crate) published: KernelExprId,
    /// This is the cold exact state after the phase transition.
    pub(crate) state: KernelStateExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One hot component write selected only when that node changes in this phase.
pub(crate) struct KernelComponentDelta {
    /// This identifies the component whose boundary-visible state changes.
    pub(crate) node: NodeId,
    /// This selects whether the component changes for the frozen inputs.
    pub(crate) changed: KernelExprId,
    /// This selects the resulting packed electrical or wire state.
    pub(crate) value: KernelExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One enabled occurrence in the ordered presentation supersequence.
pub(crate) struct KernelEffect {
    /// This selects the guard for the input assignment that emitted the occurrence.
    pub(crate) enabled: KernelExprId,
    /// This identifies the actor that owns the presentation action.
    pub(crate) piston: PistonId,
    /// This retains the exact ordered piston action.
    pub(crate) action: PistonAction,
    /// This retains the captured extension flag in the native presentation payload.
    pub(crate) extended: bool,
}

impl InstantMachine {
    pub(crate) fn kernel_bytes(&self) -> usize {
        self.kernel.estimated_bytes()
    }
    pub(crate) fn estimated_bytes(&self) -> usize {
        fn state_bytes(s: &MachineState) -> usize {
            s.components.capacity() * std::mem::size_of::<u16>()
                + s.pistons.capacity()
                + s.resources.capacity() * std::mem::size_of::<(ContactId, bool)>()
                + s.ticks.capacity() * std::mem::size_of::<MachineTick>()
                + s.requests.capacity() * std::mem::size_of::<(PistonId, PistonAction)>()
                + s.deferred.capacity() * std::mem::size_of::<MachineDeferred>()
        }
        self.controls.capacity() * std::mem::size_of::<NodeId>()
            + self.states.capacity() * std::mem::size_of::<MachineState>()
            + self.states.iter().map(state_bytes).sum::<usize>()
            + self.index.capacity() * (std::mem::size_of::<(MachineState, u32)>() + 1)
            + self.index.keys().map(state_bytes).sum::<usize>()
            + self.transitions.capacity() * std::mem::size_of::<Vec<Option<MachineTransition>>>()
            + self
                .transitions
                .iter()
                .map(|row| {
                    row.capacity() * std::mem::size_of::<Option<MachineTransition>>()
                        + row
                            .iter()
                            .flatten()
                            .map(|edge| {
                                edge.components.capacity() * std::mem::size_of::<(NodeId, u16)>()
                                    + edge.pistons.capacity() * std::mem::size_of::<(PistonId, u8, u64)>()
                                    + edge.resources.capacity() * std::mem::size_of::<(ResourceId, ContactId, bool, u64)>()
                                    + edge.dirty.capacity() * std::mem::size_of::<NodeId>()
                                    + edge.effects.capacity() * std::mem::size_of::<(PistonId, PistonAction, bool)>()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self.kernel_bytes()
    }
}

impl InstantKernel {
    /// Estimate only the lowered runtime artifact, excluding its compile-time proof snapshots.
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.expressions.len() * std::mem::size_of::<KernelExpr>()
            + self.data_controls.len() * std::mem::size_of::<NodeId>()
            + self.phases.len() * std::mem::size_of::<KernelPhase>()
            + kernel_state_bytes(&self.entry)
            + self
                .phases
                .iter()
                .map(|phase| {
                    phase.components.len() * std::mem::size_of::<KernelComponentDelta>()
                        + phase.dirty.len() * std::mem::size_of::<(NodeId, KernelExprId)>()
                        + phase.piston_revisions.len() * std::mem::size_of::<(PistonId, KernelExprId)>()
                        + phase.resource_revisions.len() * std::mem::size_of::<(ResourceId, KernelExprId)>()
                        + phase.effects.len() * std::mem::size_of::<KernelEffect>()
                        + kernel_state_bytes(&phase.state)
                })
                .sum::<usize>()
    }

    /// Evaluate one scalar selector against frozen data-control bits.
    pub(crate) fn evaluate(&self, mut expression: KernelExprId, mask: u8) -> u64 {
        loop {
            match self.expressions[expression.0 as usize] {
                KernelExpr::Constant(value) => return value,
                KernelExpr::Select { control, low, high } => {
                    expression = if mask & (1 << control) == 0 { low } else { high };
                }
            }
        }
    }
}

fn kernel_state_bytes(state: &KernelStateExpr) -> usize {
    state.components.len() * std::mem::size_of::<KernelExprId>()
        + state.pistons.len() * std::mem::size_of::<KernelExprId>()
        + state.resources.len() * std::mem::size_of::<(KernelExprId, KernelExprId)>()
        + state.future.ticks.len() * std::mem::size_of::<KernelTickExpr>()
        + state.future.requests.len() * std::mem::size_of::<KernelRequestExpr>()
        + state.future.deferred.len() * std::mem::size_of::<KernelDeferredExpr>()
}
