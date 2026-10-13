# Optimization 03 — certify actuator models and absorb private reset mechanisms

Status: implementation scope, not a certified piston classification. Implement one positive motif and its negative witnesses before generalizing. Instant and BUD models are separate cases of this project; one is not a fallback label for the other.

## Goal and useful output

Replace general piston/contact inference with a smaller checked actuator model, then absorb a private reset circuit when its full behavior can be represented by that model. The objective is less executed query, callback, and reset work, not merely a new `is_instant` flag.

The current CPU slice performs about 303,000 piston samples and 27,884 accepted actions in 100 ticks. Many samples may be semantically necessary. This pass reduces the cost of eligible samples and, after proof, internal reset operations; it does not discard them because the input value repeats.

Start with a stationary sticky actuator using one redstone resource in a bounded Near/Far footprint, with exclusive resource use and a completely inventoried observer/reset mechanism. Use a known TEST/reset schematic to discover a candidate. Do not select a fixture name as a rule.

## Reading and source ownership

Read `docs/PISTON_MODEL.md`, `docs/REDSTONE_MODEL.md`, the interpreter architecture, current migration report, and agent guide. Read [Optimization 01](opt-01-guard-and-update-reduction.md), [Optimization 02](opt-02-dust-contraction.md), and [the later region scope](opt-04-instant-region-contraction.md).

Inspect these current sources:

- `crates/core/src/redpiler/program_graph.rs`: PistonSpec/Routes/State/Availability, resources, contact variants, guards, pending entry work, validation, and lowering.
- `analysis/geometry.rs`, `electrical.rs`, and `callbacks.rs`: all possible contacts, independent normal/QC power plans, and ordered ingress/egress routes.
- `backend/graph.rs`: sampling, request membership, live acceptance, resource transfers, deferred publication, reset, and any current audited fixes.
- `backend/graph_fuzz_tests.rs`, `graph_adder_tests.rs`, `graph_continuation_tests.rs`, `graph_test_family_tests.rs`, and general Graph tests.
- `crates/core/src/redstone/piston.rs` and the reference-contract recorder/fixtures as the independent operational oracle.
- `scraps/docs/merge-domains/test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md` as a separated data/update counterexample.

The old instant architecture and response cache are examples to study, not code to merge. Construct and execute specialized instructions inside the current Graph pipeline.

## Definitions to enforce

An **instant actuator** has a proved same-action-phase response for a named trigger/action boundary. It can still have delayed head/base/resource restoration and rearming. Instant does not mean that every internal operation, timer, or observation disappears.

A **BUD storage actor** retains state between qualifying delivered updates while power data can change independently. A QC-only input is not sufficient to classify it as a storage cell; a QC-instant receiver can use the same separated interface and belong to a reset mechanism.

Neither actor becomes a pure Boolean level function solely because its saved pose looks stable. A callback must sample current power, a request must enter the appropriate FIFO position, and acceptance must recheck its live conditions. Capturing a value at request time cannot replace acceptance-time power.

Shared/drop/recapture payload behavior is legal in the general graph. The first specialization can decline shared ownership, but cannot reject the circuit or revive the discarded 'every sticky piston always owns its block' assumption.

## Algorithm 1 — build the semantic motif closure

Starting at the actuator, collect:

1. All normal and QC source/query branches over every contact alternative.
2. Every base/head update caller, including duplicate, captured, Shape, and prospective-head routes.
3. The resource's allowed contacts and all actors capable of changing its location/enabled state.
4. Reset observers, their watches and output routes, intervening wires/components, support dependencies, and ordinary/deferred work they can generate.
5. All readers/watchers of the candidate's internal geometry, power, and component values.

Follow typed dependencies, not geometric proximity alone. Compute a stable set of original IDs for the candidate and classify all crossing reads/deliveries as boundary operations.

First reject the *specialization* if the closure includes an unexplained writer, external observer of hidden intermediate cells, analog reset behavior outside the selected input domain, host command, or unbounded causal feedback. Keep the general candidate unchanged.

Exclusive resource use means no other actor can transfer that token, not that it never detaches or stays at Far after RetractWithoutPull. Either include such states in the model or prove them unreachable under every admitted history.

## Algorithm 2 — restrict the general actuator rules

Partially evaluate the existing transition rules under the certificate:

- Fold fixed obstruction/material/profile tests only when all allowed states establish them.
- Replace repeated contact-selector/material queries with direct private resource and availability predicates when equivalent.
- Reduce binary power predicates only after proving each consumed query has the declared domain. Keep separate normal and QC plans.
- Keep independent activation routes, actor/resource revisions, request deduplication/order, and deferred deadlines until a later proof removes them.
- Keep Extend, Retract, and RetractWithoutPull distinct where they are distinguishable.

The first useful runtime replacement can retain the existing PistonId, ResourceId, state tables, request queue, and general deferred publication. It specializes costly eligibility/input operations while preserving event boundaries. Existing compiled resource bindings are not a new optimization result for this pass.

Only after that replacement passes should an internal reset observer/wire chain be absorbed into a compact response/restoration/rearm plan. Preserve its initial saved state, pending ticks, repeated reset activity under held inputs, and every surviving outside effect. An external observer watching the reset cell invalidates the private-reset certificate unless its observations are reproduced explicitly.

## Algorithm 3 — verify a finite transition model

For a small private motif, construct a finite model of the relevant Graph state and labeled operations. A label identifies an input update, callback, due tick, popped request, or deferred publication; it is not simply a final truth-table input.

The state domain includes:

- actuator extension/head presence and all relevant availability states;
- resource contact/enabled state and current/stale revision relationships;
- saved reset component state and imported ordinary work;
- normal/QC input classes and supplied captured operands;
- request/deferred FIFO order, membership, action kind, phase, priority, and relative deadlines.

Derive a work/multiplicity bound from the motif and entry. Do not cap duplicate work silently. If the queue can accumulate without a proved bound, or the domain exceeds the proof budget, decline finite certification. Fresh generation identities can be renamed only after proving that the motif distinguishes current/stale equality rather than numeric identities or ordering; retain the runtime's actual revision arithmetic.

Explore reachable states from the imported entry with a BFS. For each allowed boundary label, advance reference and replacement through related microsteps, compare protected observations/future work, and enqueue their successors. A quiescent state is a state with no more immediate internal work; reaching it does not permit swallowing intervening outside callbacks or FIFO events.

Use a weak simulation relation: internal steps may disappear only if they have no protected observation and leave an equivalent boundary future. Preserve request/acceptance barriers where outside events can interleave. Enumerating all state combinations conservatively is also valid, but a saved pose alone is not an invariant.

Finite enumeration proves only the explicit complete finite model. Implementing a copied model incorrectly gives a false certificate. Reuse the general Graph's semantic operations where practical, and independently test the reduced model against the interpreter. Do not implement a second physical piston simulator or a new World snapshot executor.

## Minimal proof and model shapes

```rust
enum ActuatorClass {
    Instant, // The named accepted response has proved same-phase behavior, with explicit restoration.
    Bud,     // The actor stores its pose/value between independent qualifying update deliveries.
}

struct ActuatorCertificate {
    piston: PistonId,                 // The original actuator whose behavior is specialized.
    resource: ResourceId,             // The one existing shared-token identity used by this private motif.
    class: ActuatorClass,             // The proved behavior family, never inferred from QC presence alone.
    internal_nodes: Box<[NodeId]>,    // The reset/support nodes covered by the closed motif proof.
    ingress_routes: Box<[RouteId]>,   // The inventoried callers supplying the actor's activation boundaries.
    egress_routes: Box<[RouteId]>,    // The ordered outside deliveries that the replacement must retain.
    reconstruction: u32,             // The index of the checked reset/inspection recipe for absorbed cells.
}

enum PistonExecutionModel {
    General,                         // The current complete model used when specialization is unavailable.
    Certified {
        plan: u32,                   // The index of one immutable, checked private actuator plan.
    },
}
```

Prefer an existing per-piston model/plan binding to a separate actor manager. The certificate is compile-time evidence; execution needs only the residual plan. Every field must have a short sentence in actual code. Resource state remains authoritative in ResourceState. BUD's stored logical value should reuse or derive from the existing authoritative actor state; do not add a second independently mutable copy of `extended`.

The exact runtime plan must be chosen from the first successful certificate. It must name its dynamic reads, accepted-action behavior, remaining timers, rearm operation, and projection rule. Do not introduce a general transition bytecode VM before a motif requires one.

## BUD-specific conditions and tests

A BUD replacement is permitted only if the candidate closure proves persistent stored behavior, independent data/update input paths, and a complete activation contract. It must:

- Hold state when only data changes.
- Deliver and sample qualifying updates in their actual order, including repeated same-value updates.
- Retain request/acceptance cancellation when data changes before the queued action executes.
- Preserve unavailable/transitional phases and resource location; never commit the bit eagerly at callback receipt.
- Produce the same power, outgoing updates, and reset reconstruction.

Use the minimal six-piston QC counter to reject the old mistake of calling an instant/QC response receiver a memory cell. Its old admission report is a counterexample, not a frozen full-output oracle. Run an independent interpreter episode and compare the named receiver callbacks and actuator actions before extending to the full counter.

## Proof helpers required

1. **Motif/boundary checker:** explicit dependency closure and private-reset ownership checks.
2. **Local invariant checker:** proves fixed obstruction/material facts and source domains over all represented alternatives.
3. **Bounded transition checker:** explores the small complete labeled state model and returns either a certificate or a concrete first divergence/unsupported dimension.
4. **Reconstruction checker:** compares current blocks/entities and the projected pending future at each supported reset cut.

Use bounded ordinary data structures and named functions. No SMT solver, global circuit model checker, truth-table learner, or schematic classifier is required. Unknown proof results preserve general execution; proof-work limits do not alter the circuit's admitted behavior.

## Integration and milestones

1. **Recognize only:** identify one instant motif and one BUD/storage candidate; list every boundary and unsupported alternative. Add minimally modified negative witnesses. No execution rewrite yet.
2. **Residual actor:** specialize the instant motif's local eligibility operations, retaining general queue/publication boundaries. Demonstrate a reduction in evaluated queries/guards, not just a new type name.
3. **Private reset absorption:** remove at least one internal reset operation only after transition and reconstruction checks cover it. Update validators, graph sites/links/routes, runtime dispatch, and projection together.
4. **BUD slice:** implement one separately certified sampled actor. Prove data-only hold and acceptance-time cancellation; do not reuse the instant rule as a memory heuristic.
5. **Review:** show one positive real schematic, one declined variant for each rule, all preserved boundary observations, and measured execution work/time.

Preserve original IDs in the first slice where possible. If absorption removes IDs, remap every owner in CompileGraph, pending entry bindings, wire neighborhoods, contacts, routes, and specialized plans. Physical cells still needed for flush/inspect/reset get non-executable reconstruction descriptions. Do not weaken receiver validation to accept the replacement.

Later passes must not invalidate a certified model silently. Recheck its concrete ownership/boundary/model preconditions during transformed-output validation and before lowering. A pass that changes those dependencies must update and reverify the plan or leave the candidate untouched; a stale certificate is a malformed transformation, not permission to execute optimistically.

Export the actual specialized model and original-ID provenance with an intentional schema update only if needed. Missing optimizer UI cannot block the semantic tests; absent provenance must not be disguised as an unchanged original node.

## Required verification

Test held inputs, repeated cycles, both input orders, multiple updates in one tick, early reversal, inhibition before/after acceptance, pending reset, blocked movement, drop/recapture where admitted, and both entry poses. Add external observer/writer/analog consumer variants that invalidate the first certificate.

Run general Graph, specialized Graph, and interpreter comparisons with independent reference recording. Check normal/QC sampling separately from requests/accepted actions. Compare future pending deadlines and reset continuation, not only settled output bits.

Run descriptive discovered tests in debug/release with `--locked`, affected parser/validator/executor suites, core/server checks, formatting, and diff checks. Benchmark the same 100-tick production workload and small targeted motifs with preparation outside timing; report actual specialization coverage and work reduction. Unsupported motifs remain executable through general Graph.

Not included: motion/progress state, global instant recognition, shared-resource specialization in the first family, relaxed input protocols, or whole-region arithmetic replacement. Risk: an apparently combinational actor can retain important pending/reset state or activate only through an independent callback.
