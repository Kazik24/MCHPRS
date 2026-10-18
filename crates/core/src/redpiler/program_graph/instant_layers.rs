//! Cold, inspectable result of graph-derived instant-region and causal-layer analysis.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Stable ID for one connected component of instant cells and wire corridors.
pub(crate) struct LayerRegionId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Stable ID for one topologically earlier Boolean expression.
pub(crate) struct LayerExprId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Stable ID for one accepted-use candidate occurrence in the causal program.
pub(crate) struct LayerUseId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Stable ID for one typed boundary input binding in a region.
pub(crate) struct LayerInputId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Complete cold analysis result; declined components never appear as compiled regions.
pub(crate) struct LayeredInstantPlan {
    /// These connected owner components have complete finite causal-layer artifacts.
    pub(crate) regions: Box<[LayeredRegion]>,
    /// These whole owner components were not compiled, with graph-derived failure witnesses.
    pub(crate) declines: Box<[LayerDecline]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One connected set of cells/corridors with its retained interface and finite-use layers.
pub(crate) struct LayeredRegion {
    /// This is the deterministic ID assigned by the minimum original owner ID.
    pub(crate) id: LayerRegionId,
    /// These catalog cell indices own the actors and payload state in this region.
    pub(crate) cells: Box<[u32]>,
    /// These catalog corridor indices carry only derived electrical state in this region.
    pub(crate) corridors: Box<[u32]>,
    /// This is the region's stationary-entry predicate at the original request-sample cut.
    pub(crate) stationary_entry: LayerExprId,
    /// These catalog input rows remain outside dependencies or event entry points.
    pub(crate) inputs: Box<[LayerInput]>,
    /// These catalog output rows remain externally observable effects or values.
    pub(crate) outputs: Box<[LayerOutput]>,
    /// These predicates expose the selected token's source vacancy after this finite wave.
    pub(crate) resource_outputs: Box<[LayerResourceOutput]>,
    /// These original callbacks remain ordinary Graph lifecycle work outside this wave.
    pub(crate) cold_lifecycle_occurrences: Box<[LayerLifecycleOccurrence]>,
    /// These topologically ordered shared Boolean operations are the layer guard roots.
    pub(crate) expressions: Box<[LayerExpr]>,
    /// These map each local cell equation pin to its region-wide Boolean expression.
    pub(crate) pin_values: Box<[LayerPinValue]>,
    /// These preserve callback-time requests and acceptance at the live FIFO prefix.
    pub(crate) layers: Box<[CausalLayer]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A boundary input used at a specific semantic point in request or acceptance processing.
pub(crate) struct LayerInput {
    /// This identifies the exact event, electrical predicate or state read being bound.
    pub(crate) source: LayerInputSource,
    /// This distinguishes sampled values from callback events and live acceptance reads.
    pub(crate) role: LayerInputRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Source predicate attached to one region-local Boolean input.
pub(crate) enum LayerInputSource {
    /// This samples an outside directional source above a four-bit threshold, without reading owned wires.
    SourceAbove {
        /// This is the original outside source site.
        site: SiteId,
        /// This preserves the original electrical channel and direction.
        port: PowerPort,
        /// This is the exclusive threshold after accumulating path attenuation.
        threshold: u8,
    },
    /// This is an original callback occurrence delivered through the referenced input port.
    BoundaryEvent {
        /// This indexes the original catalog input port that admits the callback.
        port: u32,
        /// This identifies the original route and delivery ordinal of the event.
        occurrence: RouteOccurrence,
    },
    /// This is positive strength after the original link guard and attenuation are applied.
    PowerContribution {
        /// This indexes the original catalog input port that supplies the contribution.
        port: u32,
        /// This retains the source, direction, guard and attenuation of the original link.
        link: PowerLink,
    },
    /// This reads the exact recognized reset observer's positive output on an original power link.
    LocalResetOutput {
        /// This is the recognized observer whose output remains owned by the existing scheduler.
        observer: NodeId,
        /// This is its exact direct or static-conductor input link from the source graph.
        link: PowerLink,
    },
    /// This reads the exact live or captured graph state predicate at the declared phase.
    State(StateTest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Semantic point at which the layer program reads one original boundary binding.
pub(crate) enum LayerInputRole {
    /// This samples power, guards or geometry when the original callback chooses a request.
    RequestSample,
    /// This records one qualifying Update event that can enqueue a request.
    ActivationEvent,
    /// This re-reads power, readiness, pose or payload validity at the FIFO acceptance prefix.
    AcceptanceRevalidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// An original catalog output retained as an externally visible value or callback.
pub(crate) struct LayerOutput {
    /// This indexes the original boundary catalog row without collapsing duplicate occurrences.
    pub(crate) port: u32,
    /// This enables callback dispatch or gives the Boolean value of a Power/GuardState output.
    pub(crate) emitted_when: LayerExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A finite token-selection result whose source contact can be observed by another cell.
pub(crate) struct LayerResourceOutput {
    /// This identifies the cell contract that owns the single conserved token.
    pub(crate) cell: u32,
    /// This is the original resource whose source contact may become vacant.
    pub(crate) resource: ResourceId,
    /// This is the resource's common Far contact before the admitted pull wave.
    pub(crate) source: ContactId,
    /// This is the accepting actor's Near destination for the conditional transfer.
    pub(crate) destination: ContactId,
    /// This is true exactly when the FIFO action is accepted while the token is at Far and enabled.
    pub(crate) transfer_when: LayerExprId,
    /// This is true when the token is already absent or this wave transfers it from Far.
    pub(crate) source_absent_after: LayerExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// An original callback row retained for normal scheduler/lifecycle handling outside this wave.
pub(crate) struct LayerLifecycleOccurrence {
    /// This is the cell actor whose accepted action or restoration emitted the callback.
    pub(crate) actor: PistonId,
    /// This is the original route row, including its exact delivery ordinal.
    pub(crate) occurrence: RouteOccurrence,
    /// This identifies the action phase and why the row remains cold Graph work.
    pub(crate) phase: LayerLifecyclePhase,
    /// This classifies the callback without turning it into a new layer activation.
    pub(crate) reason: LayerLifecycleReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Original phase in which one retained callback row is dispatched.
pub(crate) enum LayerLifecyclePhase {
    /// This is the first synchronous callback route, emitted after base availability changes.
    RetractionBaseChanged,
    /// This is the conditional payload-source callback route after a selected transfer.
    RetractionFarChanged,
    /// This is the final synchronous callback route, emitted after the Near contact changes.
    RetractionNearChanged,
    /// This is the actor's deferred base restoration route.
    DeferredBaseRestoration,
    /// This is the actor's deferred head restoration route.
    DeferredHeadRestoration,
    /// This is the private resource's independent deferred restoration route.
    DeferredResourceRestoration,
    /// This is an observer's later watched-base reset lifecycle route.
    ResetObserverLifecycle,
    /// This is the reset observer's original output route after its accepted pull.
    ResetOutput,
    /// This is the piston actor's later far-contact shape route.
    ResetFarShapeChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Why this original callback remains outside the finite pull-computation wave.
pub(crate) enum LayerLifecycleReason {
    /// This cell's own Update or recognized non-wire Shape cannot start another finite use.
    InertCellCallback,
    /// This observer notification is a preserved reset wake handled by the ordinary scheduler.
    ResetObserverUpdate {
        /// This is the existing reset observer that receives the original callback.
        observer: NodeId,
    },
    /// This callback is retained for ordinary Graph execution after the finite layer cut.
    OrdinaryGraphLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A local cell equation pin mapped into the region-wide Boolean arena.
pub(crate) struct LayerPinValue {
    /// This is the original cell-contract index containing the pin.
    pub(crate) cell: u32,
    /// This is the typed pin ID from that cell's topological equation table.
    pub(crate) pin: CellPinId,
    /// This is the shared Boolean expression for the pin at FIFO acceptance revalidation.
    pub(crate) value: LayerExprId,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Minimal hash-consable Boolean operation used for request and acceptance guards.
pub(crate) enum LayerExpr {
    /// This is a Boolean literal independent of region inputs.
    Constant(bool),
    /// This reads one typed boundary binding at its declared semantic phase.
    Input(LayerInputId),
    /// This negates one earlier expression.
    Not(LayerExprId),
    /// This requires every listed earlier expression.
    All(Box<[LayerExprId]>),
    /// This accepts at least one listed earlier expression.
    Any(Box<[LayerExprId]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One FIFO frontier of requests with a proven source order.
pub(crate) struct CausalLayer {
    /// This states whether FIFO order is semantically required within the layer.
    pub(crate) order: LayerOrder,
    /// These occurrences retain the original request and acceptance roots in execution order.
    pub(crate) uses: Box<[LayerUse]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Commit discipline justified by the original Graph request queue.
pub(crate) enum LayerOrder {
    /// This list follows an established Graph request FIFO order and revalidates each prefix.
    Fifo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One candidate request sampled at its cause and accepted against the later live prefix.
pub(crate) struct LayerUse {
    /// This identifies the unique cell computation use, shared across its candidate occurrences.
    pub(crate) use_id: LayerUseId,
    /// This is the original actor whose accepted action changes local owner state.
    pub(crate) actor: PistonId,
    /// This is the request predicate captured when its qualifying callback was delivered.
    pub(crate) requested: LayerExprId,
    /// This rechecks current power, readiness, destination and single-use state at acceptance.
    pub(crate) accepted: LayerExprId,
    /// This is the use-consumed predicate immediately before this queue occurrence.
    pub(crate) consumed_before: LayerExprId,
    /// This is the use-consumed predicate after this occurrence's conditional acceptance.
    pub(crate) consumed_after: LayerExprId,
    /// These original callback events explain why the request was sampled.
    pub(crate) cause: UseCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Original callback-time event occurrences that supplied one request sample.
pub(crate) struct UseCause {
    /// These boundary Update deliveries are OR-sampled by this actor's input callback.
    pub(crate) boundary_occurrences: Box<[RouteOccurrence]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A declined connected component; it is not an executable or successful partial region.
pub(crate) struct LayerDecline {
    /// These cells form the complete owner component that could not be compiled.
    pub(crate) cells: Box<[u32]>,
    /// These corridors belong to the same declined component.
    pub(crate) corridors: Box<[u32]>,
    /// This original route or actor identifies the first unsupported semantic obligation.
    pub(crate) witness: LayerWitness,
    /// This names the unsupported causal or electrical fact.
    pub(crate) reason: LayerDeclineReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Small graph reference that points reviewers to the declined semantic boundary.
pub(crate) enum LayerWitness {
    /// This is the cell computation use whose order or source could not be established.
    Actor(PistonId),
    /// This is the original callback occurrence whose capture or delivery semantics are unsupported.
    Callback(RouteOccurrence),
    /// This is the original power link whose value/arrival equation is unsupported.
    Power(PowerLink),
    /// This actor's callback-time or acceptance read depends on a still-internal wire value.
    PowerAtCut {
        /// This is the actor whose local equation reads the unproven contribution.
        actor: PistonId,
        /// This is the original directional link carrying that contribution.
        link: PowerLink,
        /// This is the exact sample or acceptance cut at which the value is required.
        phase: LayerInputRole,
    },
    /// This is the wire whose finite guarded signal equation could not be derived.
    Wire(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Structured reason why a whole connected owner component remains general Graph execution.
pub(crate) enum LayerDeclineReason {
    /// This electrical dependency has no proven value at the relevant request/acceptance cut.
    SignalArrivalUnproven,
    /// This callback's capture/receiver alternatives cannot be represented safely.
    CallbackSemanticsUnsupported,
    /// Competing shared-owner requests lack a source-derived FIFO precedence.
    SharedOwnerOrderUnknown,
    /// This external observation needs an unsupported analog or geometry adapter.
    BoundaryEffectUnsupported,
}
