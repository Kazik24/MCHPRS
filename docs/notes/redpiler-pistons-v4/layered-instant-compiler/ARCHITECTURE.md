# Architecture and algorithms

Status: design for the [layered compiler project](README.md), not current runtime behavior.
The goal is a domain compiler over gates and accepted computation uses, not a symbolic
version of block propagation. See [migration](MIGRATION.md) before introducing a new owner.

## 1. What a wave means

A wave starts at a supported boundary activation with a declared input frame. It ends at
a certified causal cut after its finite computation uses and boundary effects are determined.
The existing game clock continues independently. A layer is a causal frontier, not a game
tick, physical wire distance, or universal fixed-duration nanotick.

An actor may be sampled several times. Only an accepted use of its computation transition
sets its consumed bit for this wave. Reset and restoration are different operations. Reset
may cause another response wave while external power is held: do not require a new lever
edge, permanently consume the actor, or swallow an oscillator after its first response.

All accepted same-edge data inputs are sampled together. Do not combine arbitrary updates
merely because their game-tick number matches. For real fixtures, use their physical data
preparation followed by their qualifying activation edge. Synthetic multi-source atomic
frames must use an explicit test adapter and be labelled as a contract-specific protocol.
That adapter is not evidence that Minecraft lever operations are intrinsically atomic.

The new kernel can be described by:

```text
(boundary outputs, next small local state, boundary effects)
    = K(sampled inputs, local state, entry event)
```

Local state contains only behaviorally necessary phase/selection information. Ordinary
BUD memory is an input/output interface to its existing Graph owner. Independent arbitrary
wire strengths, visit flags, captures, headings and queue snapshots are not kernel inputs.

Derive initial internal pin values from the admitted local ready/reset contract, sampled
boundary history and required lifecycle selectors. An unsupported transient stays general;
do not introduce an independent symbolic input for every old wire to admit it. Admission
checks a local cut, not global idle. If a retained bit stores user-visible data across
completed resets, classify that memory as an ordinary boundary instead of hiding it in a
kernel phase slot. A transient shared-owner selector is not permission to absorb a BUD bank.

## 2. Two compiler passes

### Pass 1: identify-instant-boundaries

1. Index existing power/QC, callback, contact and guard dependencies once.
2. Use graph-derived local mechanism recognition to classify candidate cells. Expand from
   the present observer-reset family only when another local contract is established.
3. Close shared resources/reset ownership locally. If a shared group contains an ordinary
   memory actor or an unsupported owner transition, make it a boundary or decline that group.
4. Classify potential wire/conductor corridors and every read dependency over all admitted
   contact alternatives. Uncertain dependencies stay outside; do not use saved entry topology.
5. Collect directed interfaces: available power, delivered updates, shape, geometry and memory
   observations. A source can be an input and a receiver an output without becoming owned.
6. Propagate classification changes with a worklist until every published boundary is closed.
   A producer which was demoted to classic can introduce a new input for its neighbors.
7. Publish the complete catalog and witnesses atomically. This pass forms interfaces, not
   the final connected instant circuits. Candidate classification necessarily precedes
   knowing where instant/classic boundaries lie.

Classify both endpoints AND guard support. Include conducted power, comparator far overrides,
stored-vs-emitted dust channels, observer watches, captured Turbo reads and support geometry.
Classic-net ownership established earlier is an explicit exclusion. Never give one wire,
resource or reset clock two independent state owners.

### Pass 2: compile-instant-layers

1. Partition accepted cells into regions using instant-only data/event connections and shared
   resources. Common external fanout is a port, not a union operation. Drop no region simply
   because it has no input lever; a constant region can publish a constant, and an isolated
   cyclic region needs an admitted seed or explicit decline.
2. Extract logical pin equations and activation arcs from the local cell contracts. Collapse
   electrical corridors into these equations, retaining attenuation and relevant causal facts.
3. Find strongly connected components with Tarjan/Kosaraju. Use ordinary topological expression
   construction for acyclic parts. Cycles use the guarded finite-use procedure below.
4. Produce causal layers and a boundary-effect program. Resolve conflicting uses within a
   layer explicitly; parallel commit is allowed only for independent/commuting effects.
5. Reduce expressions from observable roots and lower a compact kernel. Layers and detailed
   witnesses are cold inspection data; runtime need not iterate the original layer graph.
6. Validate ownership, output coverage, future work and reconstruction before publishing.
   A region either has a complete program or a structured decline. Partial helper plans
   must not masquerade as full region compilation.

Both passes use the existing `PlanBuilder`/`PlanOutput` mechanism. Their working helpers
live under `analysis/instant/`; their immutable/runtime types live under `program_graph/`.
All graph rewrites run first. There is no rewrite between these two builders.

## 3. Logical cells and pin semantics

The initial value vocabulary is buffer, inverted buffer, all/AND and any/OR. These are
normal forms of derived predicates, not schematic patterns with hand-authored truth tables.

Power and activation are separate. An inverter is not a spontaneous event source whenever
its input is absent. It inverts a value under an explicit qualifying activation. A QC input
can change the predicate used by the next sample without delivering that sample.

Polarity is declared per pin. For example, disappearing positive contributors combine as
AND in a falling-event decoder although positive electrical power combines as maximum.
A shared payload can produce OR-like disappearance. Infer both from source/payload rules;
do not label every sticky piston a buffer or every electrical join an OR event gate.

For each physical cell/group, retain:

- Source actors and every conserved resource/contact/reset identity.
- Input value predicates, activation sources and output decoder/polarity.
- Readiness, request and acceptance conditions, including blocking and inhibition.
- One computation-use identity per actor; grouped OR output does not consume every owner.
- Causal dependencies and any required shared-owner priority.
- Necessary boundary effects and reset/wake obligations.
- A finite local proof witness tied to graph IDs/guards, not the fixture's name.

Use shared-resource groups as the unit of arbitration. Separate Boolean outputs can still
be extracted from such a group, but its payload cannot be duplicated across independent
gate instances. A logical OR result alone does not describe which actor owns the block
for the next reset or sample.

V1 may admit Boolean power ports with a known nonzero amplitude, not necessarily strength
15. Attenuation can make a physical path electrically absent, and external analog consumers
can distinguish strengths. Derive nonzero reachability correctly; if an output requires
unimplemented variable analog behavior, retain its adapter/general region rather than
turning all positive strengths into 15.

## 4. Guarded layer construction

The following is the semantic algorithm. Concrete debug evaluation and symbolic compilation
use the same cell contracts, but native/ordinary Graph remains the independent oracle.

```text
sample the declared input frame and local cut-state bindings
seed candidate activations from that frame's boundary events
consumed[use] = false

while a guarded frontier remains:
    derive samples/requests using this frontier's input observations
    resolve queue deduplication and relevant causal precedence
    revalidate acceptances in that order against preceding logical writes
    accepted = requested AND still_valid AND NOT consumed[use]
    consumed[use] = consumed[use] OR accepted
    derive changed output pins, shared-owner state and boundary effects
    enqueue activations caused by those accepted/observable changes
    append the guarded layer and discard proven ineffective work

retain outputs, necessary local state, reset obligations and boundary effects
```

An unsuccessful sample/request never burns a use. Example: an AND sampled when only A has
arrived may become eligible when B arrives. Distinguish that from an accepted computation
which a feedback edge tries to invoke again. Duplicate `(actor, action)` requests keep
Graph's existing deduplication meaning; sampling and acceptance are not interchangeable.

### Causal order and conflicts

Derive ordering from source event witnesses and established cell-level dependencies.
Do not use DFS order, NodeId order, physical cable length, or `distance + 1` as a universal
nanotick model. Buffers can be value identities while still contributing causal precedence.

Two uses can commit in parallel only when neither writes a resource/state/read dependency
of the other and their boundary effects commute under the selected contract. Otherwise
retain an ordered block with acceptance revalidation. An unknown order is a decline witness,
not permission to choose whichever order makes an adder test pass.

Sample facts and acceptance facts can be different. A queued use may be invalidated by an
earlier accepted owner/inhibit change. Derive that conditional cancellation at cell level;
do not restore the generic Turbo executor to obtain it.

An internal pin's eventual Boolean function is not automatically available at an earlier
sample boundary. Preserve whether its generating use has arrived. Otherwise an inhibit
which arrives late would incorrectly suppress an already accepted event. Atomic external
sampling does not make all internal arrivals simultaneous.

### Cycles

- Compute SCCs first; do not solve an entire large adder by a global path enumeration.
- For monotone activation SCCs, use a worklist/least-fixed-point argument: accepted uses
  accumulate and cannot invalidate earlier results. An inversion on a dynamic activation
  dependency needs proof; a negated stable data predicate is a different case.
- For order-sensitive SCCs, unroll only the accepted cell uses and their local owner bits
  with guarded causal blocks. No moving-cell simulation or wire snapshot state is allowed.
- A cell may have candidate occurrences in multiple layers with mutually compatible guards.
  Once consumed under a valuation, later occurrences have `NOT consumed` guards.
- With N computation uses, a concrete admitted wave has at most N accepted uses and at most
  N layers containing a new accepted computation. This is NOT a claim that every generated
  candidate list, effect list or symbolic DAG has size O(N).
- Each new internal event must have a finite accepted-use/effect provenance. Failed samples
  must not generate an endless fresh worklist without a changed input/event. Reset/wake
  transitions terminate the current wave and use the global scheduler.
- Budget expressions, candidate occurrences, effect roots and bytes. If conditional ordering
  still explodes, decline that SCC or revisit its contract; never silently truncate layers.

## 5. Representation shapes

These are target responsibilities and field shapes, not a request to add all types in step
01. Existing graph IDs and enums are reused. `CellId`, `UseId`, `InputId`, `OutputId` and
`ExprId` are compact typed indices introduced only where they prevent index confusion.
Module-private discovery details can be smaller. Every actual field needs a short comment.

```rust
struct BoundaryCatalog {
    cells: Vec<CellContract>,       // These are graph-derived local behavior contracts, including grouped payload owners.
    inputs: Vec<InputBinding>,     // These are outside value/event dependencies, independently classified.
    outputs: Vec<OutputBinding>,   // These are all outside power/update/geometry observations.
    owners: Vec<Option<CellId>>,    // This assigns each original node to at most one candidate cell group.
}

struct CellContract {
    actors: Box<[PistonId]>,         // These actors contribute distinct computation uses to this local group.
    resources: Box<[ResourceId]>,    // These identities stay conserved across the group's possible contacts.
    resets: Box<[NodeId]>,          // These original nodes own reset clocks whose behavior the contract summarizes.
    equations: Box<[GateEquation]>, // These local pin equations derive values and qualifying activation predicates.
    uses: Box<[UseContract]>,       // These rules distinguish sampling, accepted use and relevant owner changes.
    witnesses: Box<[RouteOccurrence]>, // These source occurrences justify notification and causal dependencies.
}

struct GateEquation {
    output: u32,                   // This is the local output-pin index defined by the equation.
    kind: GateKind,                // This is the derived normal form, not a fixture-specific circuit name.
    inputs: Box<[u32]>,            // These are indices in the contract's validated local pin table.
}

enum GateKind {
    Buffer,                       // This preserves one pin's value.
    Invert,                       // This negates one pin's value without creating activation by absence.
    All,                          // This requires all listed value/event predicates.
    Any,                          // This requires at least one listed value/event predicate.
}

struct UseContract {
    actor: PistonId,               // This identifies the physical actor consumed by this computation use.
    action: PistonAction,          // This distinguishes the admitted computation action from reset/restoration.
    sample: u32,                  // This is the local predicate authorizing a request at the sample boundary.
    accept: u32,                  // This is the local predicate rechecked after earlier logical owner writes.
}

struct LayeredRegionDraft {
    cells: Box<[CellId]>,          // These cell groups form one instant-only connected circuit.
    inputs: Box<[InputId]>,        // These catalog bindings seed this region's sampled frame.
    outputs: Box<[OutputId]>,      // These catalog bindings are the region's protected observations.
    expressions: Vec<BoolOp>,      // These shared Boolean expressions depend on inputs and small local state.
    layers: Vec<NanoLayer>,        // These cold causal frontiers explain how computation uses were derived.
    effects: Vec<GuardedEffect>,   // These ordered effects retain necessary outward and future consequences.
}

struct NanoLayer {
    uses: Box<[LayerUse]>,         // These occurrences preserve relevant order; they are not all parallel commits.
}

struct LayerUse {
    use_id: UseId,                 // This refers to one admitted actor computation use.
    requested: ExprId,             // This is the conditional request after qualifying samples and deduplication.
    accepted: ExprId,              // This includes live revalidation and the not-consumed condition.
    cause: UseCause,               // This identifies the original boundary event or earlier causal occurrence.
}

enum UseCause {
    Boundary(InputId),             // This is the event input whose binding retains the original source occurrence.
    Predecessor { layer: u32, occurrence: u32 }, // These locate an earlier causal use occurrence in the layer draft.
    Reset(NodeId),                 // This is the existing reset-clock owner starting a new admitted wave.
}
```

Cause payloads are IDs, not captured world snapshots. Multiple causes can retain separate
guarded occurrences; merging them must preserve their activation and ordering semantics.
Local contract pin tables
must declare whether each pin is a value, activation, state predicate or intermediate result;
do not accept an unvalidated integer reference. Use an existing typed graph read for pin
sources (`PowerTarget`, `GuardId`, `SiteId`) and source equations for internal values.

Input bindings need these explicit alternatives:

| Input | Required payload and meaning |
| --- | --- |
| Power/QC | `PowerTarget`, admitted strength domain and source witness. It samples availability, not an event. |
| Update/Shape | Original `RouteOccurrence`, kind/direction, live/captured admission and frame event index. |
| Geometry | `SiteId` plus the exact profile/property dependency, including guard support. |
| Ordinary memory | Existing node/resource read binding. No copied BUD state owner. |

Output bindings need: power `(SiteId, PowerPort, nonzero amplitude)`, callback `(target,
receiver, DeliveryKind, occurrence witness)` and geometry `(site, legal profile choices)`.
A geometry choice can lower to Boolean selector bits; validity must ensure exactly one
admitted profile, preserving receiver identity as well as material. Do not freeze it to
the entry profile. Unsupported output adapters prevent contraction of their owner.

### Minimal Boolean arena and runtime program

```rust
enum BoolOp {
    Constant(bool),                // This is a Boolean literal.
    Input(InputId),                // This is a sampled boundary input bit.
    State(u32),                    // This is an existing small local-state slot.
    Not(ExprId),                   // This negates its one earlier operand.
    And(ExprId, ExprId),           // These are the left and right earlier conjunction operands.
    Or(ExprId, ExprId),            // These are the left and right earlier disjunction operands.
    Select(ExprId, ExprId, ExprId), // These are the condition, true result and false result.
}

struct GuardedEffect {
    when: ExprId,                 // This predicate selects whether the effect occurs for the sampled frame.
    effect: KernelEffect,         // This describes one boundary/state/scheduler action in causal order.
}

enum KernelEffect {
    WriteOutput { output: OutputId, roots: Range<usize> }, // These identify the boundary shadow and its Boolean/selector root bits.
    Notify { output: OutputId },                         // This dispatches the original typed callback binding against current boundary shadows.
    ScheduleReset { node: NodeId, delay: usize, priority: TickPriority }, // These preserve the existing clock owner, deadline offset and priority.
    WriteState { slot: u32, value: ExprId },              // These update a behaviorally necessary local-state slot.
}

struct InstantProgram {
    inputs: Box<[InputBinding]>,      // These are the only boundary reads reachable from effects and live roots.
    outputs: Box<[OutputBinding]>,    // These bind observable source sites and callback targets to the ordinary Graph.
    expressions: Box<[BoolOp]>,       // These execute once in topological order with no temporary heap graph.
    roots: Box<[ExprId]>,             // These retain ordered output selector bits and effect conditions.
    effects: Box<[GuardedEffect]>,    // These are the compact boundary/scheduler actions which survived reduction.
    reset_entries: Box<[ResetEntry]>, // These bind existing clock wakeups to small continuation programs.
}

struct ResetEntry {
    node: NodeId,                    // This existing reset owner wakes the continuation at its ordinary scheduler deadline.
    program: u32,                    // This selects a validated continuation program in the same immutable program table.
}

struct InstantState {
    local: Box<[bool]>,              // These are only phase/selection bits that affect subsequent behavior.
    frame_inputs: Box<[bool]>,       // These retain sampled data only while a certified transaction needs it.
    boundary_values: Box<[u32]>,     // These typed-by-binding scalar words retain strengths, profile selectors and observable property bits without truncating IDs.
    revision: u64,                  // This invalidates stale continuations after input interruption or handoff.
}
```

The final Rust payload fields should each have their own one-sentence explanation; compact
tuple examples here name all operands. `InstantState.revision` is a continuation version,
not a duplicate owner/revision of every physical payload. Meaningful payload selection may
need a compact slot; it cannot be deleted just because the current output is Boolean.

Keep materialization/cell witnesses in a separate cold table in `CircuitProgram`. The hot
program references that table only during inspection or interpreter handoff. Do not keep
`consumed`, layer frontiers or original-wire state in persistent runtime memory merely
because the construction algorithm used them.

## 6. Expression reduction and proof obligations

Use a topological Boolean DAG with hash-consing and a work/node/byte budget. Start with
constants, identical operands, complements, double negation, select identities, dead-root
elimination and local common-subexpression sharing. Do not construct a canonical global
truth relation or enumerate region input masks.

Roots are boundary values, meaningful local state, activation conditions and future effects.
Physical internal wire writes are not roots. A pin's transient value can nevertheless be
a live root if it schedules a repeater pulse or samples BUD memory. Preserve that as a
boundary effect instead of retaining its entire internal wire trace.

Proof obligations are small and separated:

1. **Cell:** the local predicate/activation/reset contract matches its admitted physical family.
2. **Boundary:** every outside observation/read is represented or prevents contraction.
3. **Causality:** parallel uses commute, or ordered acceptance captures their interaction.
4. **Single-use:** computation consumption is correct per wave, including feedback and late inputs.
5. **Reduction:** algebraic rewrites preserve all live roots and effect occurrence meaning.
6. **Lifecycle:** shared scheduling and reconstruction preserve subsequent ordinary execution.

Exhaustive small-cell tests, small generated circuit differential tests and algebraic proofs
serve these obligations. A successful arithmetic truth table is not a proof of deadlines
or memory. A successful fallback is not a proof of the compiler.

## 7. Runtime ownership, effects and reset

At lowering, create direct entry/source bindings. Each original node/resource/reset and
callback occurrence has one execution owner. Covered callbacks/wire ticks/actions must
not also reach general handlers. Generic runtime paths remain intact for unowned devices.

Evaluate a kernel from its compact sampled input vector and local state. Replay only the
necessary output/effect program. Outside Graph power/geometry queries read the boundary
shadows for contracted sources. They must not read a stale physical wire state slot.

Effects can write a boundary high value, notify a repeater, then write low and notify again.
The ordinary repeater handler schedules its response through the existing scheduler. This
models a transient pulse without generic owned-state overlays or replaying all internal
callbacks. Geometry readers require the equivalent exact profile/receiver adapter.

Synchronous outside feedback is an explicit admission issue. If an outside callback can
change an input used by the unfinished kernel, preserve it as a continuation boundary or
decline that region. Do not freeze it under the name of external same-tick reordering.

Reset continuations use existing NodeIds, deduplication, priorities and Graph phases. Derive
their delays from reset contracts; there is no universal two-tick timer. Independent reset
clocks remain independent: do not enumerate their combined period or wait for global idle.
Held inputs may generate new waves at wakeup. Reset does not mean clearing every output.

Reject unsupported imported work touching owned internals before activation, preserving
the input world. Preserve unrelated imported/stale/duplicate ticks exactly. Later support
for a compatible local cut must be an explicit extension, never an assumed empty queue.

For handoff, either materialize immediately from a proven cold reconstruction recipe or
mark the request pending and continue the ordinary global clock to a certified safe cut.
The wait needs a proven bound and must not wait for a global period. Unrelated devices
continue, and pending future work is transferred once. If no bound/recipe exists, do not
admit that kernel. No private region tick or fresh interpreter replay is hidden in inference.

## 8. Failure reporting and inspectability

Report a region/cell/port/source occurrence plus a small reason, for example `shared owner
order unknown`, `geometry output adapter missing`, `second computation use in wave`,
`analog port not admitted`, `live outside feedback`, or `expression budget exhausted`.

Export cells, boundaries, layer occurrences, requested/accepted conditions, consumption and
live roots as cold JSON. Keep deterministic ID ordering, duplicate occurrences and source
provenance. The viewer can display layers without depending on a new simulation engine.
Budget/decline statistics are compile-time data. Runtime tracing remains opt-in.
