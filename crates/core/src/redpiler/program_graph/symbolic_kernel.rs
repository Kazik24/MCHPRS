//! Immutable AOT phase programs; proof construction and state ownership stay elsewhere.
use super::*;
use mchprs_redpiler_opt::Plan;
use mchprs_world::{PistonAction, TickPriority};

#[derive(Debug, Clone, PartialEq, Eq)]
/// One compiled recurrence family admitted by a falling control edge.
pub(crate) struct SymbolicKernel {
    /// This identifies the graph-derived falling edge that starts the recurrence.
    pub(crate) trigger: NodeId,
    /// These typed bindings freeze the external and retained-memory inputs at admission.
    pub(crate) inputs: Box<[InstantInput]>,
    /// These local codebooks decode stored wire shapes into the frozen input vector.
    pub(crate) shapes: Box<[WireShapeMemory]>,
    /// These prebound electrical outputs stay live while private wires and future queues remain cold.
    pub(crate) hot_components: Box<[KernelComponentBinding]>,
    /// This pure Boolean circuit computes admission, phase state and ordered future roots.
    pub(crate) plan: Plan,
    /// These roots describe the complete ready state and its independent admission clauses.
    pub(crate) entry: KernelEntryLayout,
    /// This frame represents the synchronous falling-edge callback wave before the first tick.
    pub(crate) activation: KernelFrameLayout,
    /// These frames represent one proven prefix followed by a repeated cycle.
    pub(crate) phases: Box<[KernelFrameLayout]>,
    /// This is the first phase whose successor repeats for every frozen input valuation.
    pub(crate) prefix: usize,
    /// This is the universally proven recurrence period in logical ticks.
    pub(crate) period: usize,
}

impl SymbolicKernel {
    /// Bind electrical scalar slots once, without scanning graph nodes during a kernel tick.
    pub(crate) fn component_bindings(graph: &CompileGraph) -> Box<[KernelComponentBinding]> {
        let mut scalar = 0;
        let mut bindings = Vec::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            if matches!(node.state, ComponentState::Electrical(_)) {
                bindings.push(KernelComponentBinding {
                    node: NodeId::new(index as u32),
                    scalar,
                });
            }
            scalar += if matches!(node.kind, CircuitNodeKind::Wire(_)) { 14 } else { 6 };
        }
        bindings.into_boxed_slice()
    }

    /// Validate every lowered input, phase root and graph-owned event reference before publication.
    pub(crate) fn validate(&self, graph: &CompileGraph) -> bool {
        let root_count = self.plan.roots().len();
        let scalar_count = graph
            .nodes
            .iter()
            .map(|node| if matches!(node.kind, CircuitNodeKind::Wire(_)) { 14 } else { 6 })
            .sum::<usize>()
            + graph.pistons.len() * 7
            + graph.resources.iter().map(|resource| resource.contacts.len() + 1).sum::<usize>();
        if !matches!(
            graph.nodes.get(self.trigger.index()).map(|node| &node.kind),
            Some(CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever))
        ) || self.period == 0
            || self.prefix.checked_add(self.period) != Some(self.phases.len())
            || self.inputs.len() != self.plan.inputs().len()
            || self.hot_components != Self::component_bindings(graph)
            || !self.plan.inputs().iter().copied().eq(0..self.inputs.len())
            || self.entry.state.len() != scalar_count
            || self.entry.revision_deltas.len() != graph.pistons.len() + graph.resources.len()
            || self.entry.revision_deltas.iter().any(|range| !valid_bits(range, root_count))
            || self.activation.state.len() != scalar_count
            || !valid_range(&self.entry.state, root_count)
            || !valid_range(&self.entry.conditions, root_count)
            || !valid_frame(&self.activation, root_count, scalar_count, graph)
            || self.phases.iter().any(|phase| !valid_frame(phase, root_count, scalar_count, graph))
            || self.shapes.iter().any(|shape| {
                !matches!(graph.nodes.get(shape.node.index()).map(|node| &node.kind), Some(CircuitNodeKind::Wire(_)))
                    || shape.values.is_empty()
                    || shape.values.len() > 81
            })
        {
            return false;
        }
        self.inputs.iter().all(|input| match *input {
            InstantInput::Powered(node) => graph
                .nodes
                .get(node.index())
                .is_some_and(|node| matches!(node.kind, CircuitNodeKind::Electrical(_))),
            InstantInput::Locked(node) => graph.nodes.get(node.index()).is_some_and(|node| {
                matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Repeater { .. })
                )
            }),
            InstantInput::SignalBit { node, bit } => {
                bit < 4
                    && graph.nodes.get(node.index()).is_some_and(|node| {
                        matches!(
                            node.kind,
                            CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Comparator { .. })
                        )
                    })
            }
            InstantInput::Extended(actor) => actor.index() < graph.pistons.len(),
            InstantInput::ResourceBit { resource, bit } => bit < usize::BITS as u8 && resource.index() < graph.resources.len(),
            InstantInput::ShapeBit { memory, bit } => bit < usize::BITS as u8 && (memory as usize) < self.shapes.len(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KernelComponentBinding {
    /// This is the original electrical state owner updated without graph propagation.
    pub(crate) node: NodeId,
    /// This locates its six scalar bits relative to each phase's cold state roots.
    pub(crate) scalar: usize,
}

fn valid_range(range: &Range<usize>, root_count: usize) -> bool {
    range.start <= range.end && range.end <= root_count
}

fn valid_bits(range: &Range<usize>, root_count: usize) -> bool {
    valid_range(range, root_count) && range.len() <= u64::BITS as usize
}

fn valid_frame(frame: &KernelFrameLayout, root_count: usize, scalar_count: usize, graph: &CompileGraph) -> bool {
    frame.state.len() == scalar_count
        && frame.pending.len() == graph.nodes.len()
        && frame.revision_deltas.len() == graph.pistons.len() + graph.resources.len()
        && valid_range(&frame.state, root_count)
        && frame.revision_deltas.iter().all(|range| valid_bits(range, root_count))
        && valid_range(&frame.pending, root_count)
        && frame.ticks.iter().all(|event| {
            graph.nodes.get(event.node.index()).is_some_and(|node| node.raw_state.is_some())
                && event.enabled < root_count
                && event.delay < crate::redpiler::backend::NUM_QUEUES
                && (event.delay != 0 || event.priority == TickPriority::NanoTick)
        })
        && frame
            .requests
            .iter()
            .all(|event| event.actor.index() < graph.pistons.len() && event.enabled < root_count)
        && frame.publications.iter().all(|event| {
            event.actor.index() < graph.pistons.len()
                && event.enabled < root_count
                && event.delay < crate::redpiler::backend::NUM_QUEUES as u64
                && valid_bits(&event.actor_revision_age, root_count)
                && event.resource.as_ref().is_none_or(|payload| {
                    payload.enabled < root_count
                        && payload.resource.index() < graph.resources.len()
                        && graph.resources[payload.resource.index()].contacts.contains(&payload.contact)
                        && valid_bits(&payload.revision_age, root_count)
                })
        })
        && frame
            .effects
            .iter()
            .all(|effect| effect.actor.index() < graph.pistons.len() && effect.enabled < root_count && effect.extended < root_count)
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Root ranges used to admit a kernel against its complete symbolic ready state.
pub(crate) struct KernelEntryLayout {
    /// These roots encode every component, shape, actor and resource scalar.
    pub(crate) state: Range<usize>,
    /// These roots must all be true for the prepared entry equations to hold.
    pub(crate) conditions: Range<usize>,
    /// These counts define the symbolic revision baseline before activation changes any owner.
    pub(crate) revision_deltas: Box<[Range<usize>]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Root layout for one exact state/future boundary and its ordered presentation effects.
pub(crate) struct KernelFrameLayout {
    /// These roots encode every component, shape, actor and resource scalar for cold restoration.
    pub(crate) state: Range<usize>,
    /// These little-endian root ranges count revision writes for actors, then resources.
    pub(crate) revision_deltas: Box<[Range<usize>]>,
    /// These roots encode scheduler membership by node independently of display state.
    pub(crate) pending: Range<usize>,
    /// These rows preserve conditional scheduled work in deadline and priority order.
    pub(crate) ticks: Box<[KernelTickRoot]>,
    /// These rows preserve conditional FIFO piston requests.
    pub(crate) requests: Box<[KernelRequestRoot]>,
    /// These rows preserve guarded deferred connection publications and captured witnesses.
    pub(crate) publications: Box<[KernelPublicationRoot]>,
    /// These rows preserve accepted piston presentation effects in causal order.
    pub(crate) effects: Box<[KernelEffectRoot]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One scheduled event selected by a Boolean plan root.
pub(crate) struct KernelTickRoot {
    /// This root selects whether the scheduled event exists for the frozen inputs.
    pub(crate) enabled: usize,
    /// This identifies the event's compiled receiver.
    pub(crate) node: NodeId,
    /// This is the event's half-tick deadline relative to the frame boundary.
    pub(crate) delay: usize,
    /// This preserves scheduler ordering within the deadline.
    pub(crate) priority: TickPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One queued piston request selected by a Boolean plan root.
pub(crate) struct KernelRequestRoot {
    /// This root selects whether the request exists for the frozen inputs.
    pub(crate) enabled: usize,
    /// This identifies the requesting piston actor.
    pub(crate) actor: PistonId,
    /// This preserves the requested logical piston action.
    pub(crate) action: PistonAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One deferred connection publication and its independent captured resource witness.
pub(crate) struct KernelPublicationRoot {
    /// This root selects whether the publication remains pending at this frame boundary.
    pub(crate) enabled: usize,
    /// This identifies the actor whose revision witness was captured at acceptance.
    pub(crate) actor: PistonId,
    /// These bits encode the actor revision age relative to the live runtime origin.
    pub(crate) actor_revision_age: Range<usize>,
    /// This retains the optional independently guarded payload publication.
    pub(crate) resource: Option<KernelPayloadRoot>,
    /// This is the logical deadline relative to the frame boundary.
    pub(crate) delay: u64,
    /// This distinguishes extension publication from retraction publication.
    pub(crate) extending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A payload transferred by a pending publication, selected by its own guard.
pub(crate) struct KernelPayloadRoot {
    /// This root selects whether the payload witness is present.
    pub(crate) enabled: usize,
    /// This identifies the conserved shared payload.
    pub(crate) resource: ResourceId,
    /// These bits encode its revision age relative to the live runtime origin.
    pub(crate) revision_age: Range<usize>,
    /// This preserves the contact captured when the payload was transferred.
    pub(crate) contact: ContactId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One ordered piston presentation effect selected by Boolean roots.
pub(crate) struct KernelEffectRoot {
    /// This root selects whether the presentation effect is emitted.
    pub(crate) enabled: usize,
    /// This root selects the extension bit captured with the presentation action.
    pub(crate) extended: usize,
    /// This identifies the actor that owns the block action.
    pub(crate) actor: PistonId,
    /// This preserves the exact accepted action kind.
    pub(crate) action: PistonAction,
}
