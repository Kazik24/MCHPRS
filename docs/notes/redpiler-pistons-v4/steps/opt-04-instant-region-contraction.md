# Optimization 04 — contract certified regions into compact logic and transitions

Status: implementation scope, not implemented. This is the highest-potential and highest-risk project in this set. Begin with one small certified region; do not replace a full adder/CPU from its schematic name or final truth table.

## Goal and prerequisites

Replace many internal wires, reset mechanisms, actuator queries, and callbacks with a small logic DAG or finite transition model executed inside Graph. Recompute only pure results affected by changed inputs while retaining every meaningful activation, event boundary, and outgoing observation.

Prerequisites are the usable proof/rewrite helpers in [Optimization 01](opt-01-guard-and-update-reduction.md) and a certified family from [Optimization 03](opt-03-piston-specialization.md). [Optimization 02](opt-02-dust-contraction.md) may reduce supporting paths but is not mandatory for a region whose own proof accounts for those wires.

Do not require universal whole-CPU equivalence before proving one motif. Conversely, passing one motif does not certify nearby shared-resource, analog, or feedback variants. Unknown regions remain on general Graph execution.

## Source references

Read the three reference model documents, current general architecture/migration report, and agent guide. Inspect:

- `crates/core/src/redpiler/program_graph.rs`: typed connectivity, model bindings, mutable ownership, validation, lowering, and position/projection indexes.
- `passes/mod.rs` and the existing optimized/unoptimized pipeline tests.
- `analysis/electrical.rs` and `callbacks.rs`: the distinction between power reads and activation.
- `backend/graph.rs`: action FIFO, live validation, deferred restoration, captured/nested callbacks, flush, inspect, and reset.
- Graph TEST/adder/shared-resource/continuation fixtures and the reference-contract comparison harness.
- `docs/notes/redpiler-pistons-v4/MERGE_DOMAINS_OPTIMIZATION_REVIEW.md`: historical dirty-input/shared-expression ideas and their boundaries.
- `steps/12-adder-optimizer-analysis.md`: why a timed sum shortcut needs more than matching arithmetic outputs.

Historical logical programs can inform algorithms. Do not import their executor, native ownership hooks, assembly parser, or assumed input protocol.

## Candidate selection and complete boundary

Build a semantic dependency graph containing power queries, callback routes, observer watches, guards/sites, resource owners/readers, support changes, and pending/projection obligations. Use connected closure to find small regions; SCC analysis identifies causal feedback, but a cycle is neither automatic failure nor proof of a valid memory boundary.

The first family should be a small acyclic dependency motif built from certified private-resource actors, implementing a buffer/inverter or small Boolean combination. No host commands, unaccounted analog readers, external observers of removed internal cells, external capture through the region, or outside resource owners in the first slice.

For every cut edge, name:

- the incoming strength/directional channel and input-domain decoder;
- the incoming callback/control label, supplied view, and ordering;
- the outgoing strengths, ordered callbacks, shape effects, sounds/block actions, and phase;
- the work and state that can remain pending at a host reset/inspection boundary;
- the resource/contact states the outside world can read or change.

All outside effects count, even when they do not connect electrically. Adding a disconnected active circuit cannot change correctness by changing which unrelated queued action lies between two internal actions.

## Replacement classes

### A. Pure value network with a retained activation/publication plan

The *value computation* can be stateless when its result is a function of declared inputs. That does not imply the whole region is a zero-delay gate. Keep the phase and queued acceptance/publication required to expose the computed value.

Use a compact typed Boolean DAG for certified binary channels, or existing strength links for max/attenuation relations. Proposed pure operations are Literal, Input/Threshold, Not, And, and Or. Add only operations required by the first demonstrated family. Do not disguise a pure gate as a repeater, overload Guard with arbitrary execution, or build a general bytecode VM.

Boolean replacement must preserve its signal decoder: a physical falling-event OR-like mechanism may expose active-low output or inverted pulse timing. Derive the equation from its accepted transitions, not the schematic's AND/OR label. Unknown strength is 0..15; no Boolean assumption follows from a value observed once.

### B. Small transition model

Use explicit finite state when storage, reset/rearm, cancellation, or shared-token history affects the next response. A BUD keeps a sampled value; an instant actuator can keep busy/restoration state. Do not replace either with current-input combinational logic when history matters.

A table or named specialized handler can encode the first family's states and boundary operations. Keep timed continuations on the existing Graph scheduler/action queue. A future shared-resource family needs one authoritative token/location state, not copied cells in every gate.

### C. Retained general region

If the state/event model is unbounded, a required observation has no reconstruction, or proof is unknown, leave the candidate general. Do not route it to Native or reject a circuit admitted by the general parser.

## Algorithm 1 — partial evaluation and logic reduction

After extracting a valid region boundary and input domain:

1. Express each certified value relation using typed boundary inputs and previously certified internal functions.
2. Substitute proved constants, remove identity/inverse pairs, flatten compatible And/Or nodes, and share identical complete expressions.
3. Eliminate pure computations not used by any output, predicate, future state, or reconstruction recipe.
4. Merge repeated *computations*, not independent actuators, resource identities, pending tick handles, or output routes.
5. Compose buffers only where each removed intermediate delay/action boundary is proved unobservable and cannot affect cancellation/interleaving. A sequence is not a longer-delay buffer merely because its final waveform looks similar.

For small binary functions, enumerate their complete truth tables and compare the original/reduced pure expression. This is an arithmetic/function lemma, not proof of temporal region equivalence.

Start with local algebra and exact structural sharing. A reduced decision diagram is optional when measured function size justifies it; it is not required to implement the first regions. Do not add a Boolean/SAT dependency for a few small identities.

## Algorithm 2 — prove temporal equivalence

For the bounded first family, reuse Optimization 03's labeled transition exploration. Build the complete original/replacement state relation and explore every permitted next boundary operation, including equal-value callbacks and time advancement.

Compare the protected observation *word* for each operation: a word is an ordered list, so duplicate events and relative phases remain significant. Match future queue order, acceptance eligibility, deadlines, and reconstruction at all supported cuts.

Internal steps can disappear only under a checked no-observation argument. Consider A appending B behind an unrelated pending C. Replacing A+B with one atomic operation moves B before C. Retain a continuation at B's queue position unless a complete noninterference/observation proof permits that move. Electrical independence alone is insufficient when the order of outgoing callbacks or host actions is protected.

The candidate must account for outside writes arriving between request and acceptance or during restoration. Do not evaluate all future outputs from one request-time snapshot unless that capture is the original contract.

When a complete finite graph has been established, use partition refinement to reduce state:

1. Partition states by current protected outputs and reconstruction obligations.
2. Split a partition when a boundary label emits a different observation word, leaves different pending work, or leads to different successor partitions.
3. Repeat until no partition splits.
4. Choose one compact representative state encoding for each surviving class.

Two states with identical visible outputs but different next acceptance, pending reset, or required reconstructed cells are not equivalent. If exact projection requires retaining every old internal bit, retain it or decline that state merge; report memory savings honestly.

Exploration must have explicit state/transition/work/queue bounds. Unknown/time-out/proof budget exhaustion retains the original region. No claim of all-histories equivalence follows from testing only four operand combinations or one clock protocol.

## Algorithm 3 — incremental pure evaluation

For a certified pure DAG, compute compile-time reverse dependencies:

- input binding -> expressions that read it;
- expression -> parent expressions;
- expression -> retained output/predicate/reconstruction roots.

At execution, data changes invalidate relevant cached values. At a required read or activation, evaluate only dirty roots and necessary branches in dependency order. Use ordinary dense arrays and reusable work vectors; each pure root enters the dirty queue once.

Separate value invalidation from activation. QC data changes may invalidate a cached expression but must not sample a BUD or enqueue a piston action. A delivered callback still performs its required sampling/acceptance behavior even when the cached result is unchanged.

Every invalidation owner must be enumerated: external source outputs/strength classes, committed storage, contact selection/enabled state, actor availability/head state, and any retained capture scope. Do not cache general Live Guard results for a whole tick. Captured inputs belong to their own immutable context, not a global 'latest' cache.

Start with regions whose pure inputs come from explicitly bound boundary slots. If determining cache validity requires a large global revision framework, keep uncached evaluation until that dependency model is reviewed. No atomics or parallel evaluation in this project.

## Narrow plan and proof record sketches

```rust
enum LogicExpr {
    Literal(bool),               // A proved constant Boolean value.
    Input(u32),                  // A declared boundary binding evaluated under its certified decoder.
    Not(u32),                    // The inverse of one already defined expression value.
    And(Box<[u32]>),             // The conjunction of the listed pure expression values.
    Or(Box<[u32]>),              // The disjunction of the listed pure expression values.
}

struct RegionCertificate {
    original_nodes: Box<[NodeId]>, // The original executable identities covered by this reduction.
    original_resources: Box<[ResourceId]>, // The authoritative resource identities covered by the ownership proof.
    ingress: Box<[u32]>,          // The indexes of checked input/callback boundary descriptions.
    egress: Box<[u32]>,           // The indexes of ordered output/effect boundary descriptions.
    state_classes: u32,          // The number of distinct verified transition states retained by the model.
    projection: u32,             // The checked reconstruction plan for removed physical descriptions.
}

struct PureRegionPlan {
    expressions: Box<[LogicExpr]>, // The compact function DAG, with validated child/input references.
    roots: Box<[u32]>,            // The retained outputs and predicates that need pure evaluation.
    input_ranges: Box<[Range<u32>]>, // Each boundary input's slice in the flattened input-user list.
    input_users: Box<[u32]>,      // The flattened reverse dependencies for boundary input changes.
    parent_ranges: Box<[Range<u32>]>, // Each expression's slice in the flattened parent-user list.
    parent_users: Box<[u32]>,     // The flattened reverse dependencies for expression invalidation.
}
```

Use existing/new typed IDs and local ranges in actual code rather than ambiguous integers once ownership is known. These are semantic shapes, not complete allocation/API instructions. All actual fields need a short sentence. The finite transition plan's fields must come from the first certified family's labels/state/timers, not an anticipatory framework.

Pure caches are derived, discardable values. Persistent storage and resources keep one authoritative owner. Avoid a full general-state copy plus a second 'fast' state. The first implementation may retain original state slots/projection handles while removing their costly executable operations; state-table shrinkage can follow separately.

## Projection, scheduling, and graph integration

Use GraphPass for recognition/rewrite and the single Graph executor for both general and reduced instructions. Add a narrowly named model/node kind only after the first certificate establishes the fields it needs.

Consume earlier actuator/path certificates only after their concrete preconditions have been rechecked against this graph. An old marker, pass name, or certificate hash is not enough when intervening experimental passes can change routes, ownership, or geometry. Validate the residual model's preconditions again before final lowering.

- Preserve outside identities and direction/port tags where possible.
- Every removed node/site/contact/resource needs a replacement owner or explicit non-executable projection description. Imported ticks must still bind or have a proved identical no-op disposition.
- Removed cells must reconstruct their strength, shape, block/entity state, resource position, and pending future at every supported flush/inspect/reset boundary. A final-state-only formula is insufficient while busy.
- Runtime queries from unreduced neighbors must read the replacement's correct current value/geometry. Do not leave dangling canonical wire neighborhoods or stale raw templates.
- Retain required queued continuation slots and cancellation generations; do not create an independently ticking region backend.
- Apply explicit maps across all affected draft tables, specialized plans, route/callback-edge occurrences, wire relations, and initial/pending entry bindings. Keep event sequence groups and occurrence multiplicity.
- Validate replacement ownership and entry-state correspondence before lowering. Export actual specialized records plus original-ID provenance; no lossy old binary conversion.

Do not persist a large proof exploration table solely for runtime diagnostics. Store only the residual executable plan and minimal reconstruction metadata; make proof reports compile-time artifacts.

## First implementation milestones

1. **One candidate and counterexamples:** analyze a small admitted TEST motif made from certified actors. Add an external observer, alternate update writer, analog reader, and outside token owner as declined variants. List all boundary operations.
2. **Pure lemma:** derive and simplify its value relation; prove the complete small input domain, including polarity/thresholds. No temporal rewrite yet.
3. **Timed replacement:** keep required activation/publication boundaries, certify transition/reconstruction, and execute through Graph. Compare both input orders, early reversal, held inputs, repeated triggers, and cancellation.
4. **Incremental evaluation:** bind complete dependencies, implement invalidation, and compare against uncached reduced evaluation on arbitrary short histories. Verify data-only changes do not activate storage.
5. **Composition:** connect two independently certified regions with a general component and unrelated queued work. Preserve global observable ordering. Only then expand motif size or shared-resource support.

Each milestone requires a real successful reduction and a deliberately unsafe declined candidate. Recognition without less execution does not complete the project. Do not replace ADDER_11BIT with integer addition unless a separate certificate covers all its allowed histories, timing, inputs, and continuation.

## Verification and measurement

Use original Graph, reduced Graph, and independently stepped interpreter episodes. Compare ordered boundary callbacks, source strengths, accepted/cancelled work, relative deadlines, shared-token conservation, and reset continuation. Deliberately corrupt a boundary decoder, timer, invalidation edge, and projection recipe; the checks must detect each.

Run with this pass alone and with the approved preceding passes. Retain the supplied pass API and test both valid orderings and rejected preconditions. Verify determinism and bounded compile-time proof work.

Measure the existing production short CPU protocol in five fresh pairs with setup/proof/flush/checks outside timing. Report eligible-region coverage, original/reduced executable nodes, callbacks, queries, actions/continuations, retained state/cache/projection bytes, and runtime. Larger speedup claims need representative additional active workloads, not idle tick rates.

Not included: full CPU/FPU symbolic synthesis, unchecked algebraic gates, protocol relaxation, parallelism, or a resurrected instant/native domain. Risk: function equivalence can coexist with wrong activation, event interleaving, cache invalidation, and reset state.
