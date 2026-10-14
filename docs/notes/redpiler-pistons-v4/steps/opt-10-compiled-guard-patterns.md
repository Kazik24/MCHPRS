# Opt-10 — compile common guards into direct state tests

Status: implementation scope only. Recommended first experiment among the [new guard projects](../GUARD_OPTIMIZATION_PLAN.md). It does not depend on opt-08 or opt-09 being implemented.

## Purpose

Replace repeated recursive interpretation of a small, measured guard family with a direct pure function over existing state. For example:

```text
All(State(ResourceEnabled(r)), State(ResourceAt(r,c)))
    -> load resource r once; test enabled && at == c
```

Keep the canonical guard DAG as the source of truth for validation, inspection, and fallback. Keep `CircuitState` as the only mutable state owner. The plan is an immutable instruction, not a cached Boolean result or a second representation of current geometry.

Implement matching in `passes/compile_guard_patterns.rs`, with a name such as `compile-guard-patterns`. Runtime evaluation remains in `backend/graph.rs`; plan types, validation, and lowering belong with the existing program types. This is not a generic bytecode VM.

## Inspect and reach

- `program_graph.rs`: Guard/StateTest, resource and piston state, Draft/Program storage, validation, lowering, and typed IDs.
- `backend/graph.rs`: both guard-root wrappers, recursive evaluation, contact resolution, power queries, callback delivery, Turbo captured-site lookup, state writers, and optional profiling.
- `passes/mod.rs`, `passes/rich.rs`, `passes/utils.rs`: registration, invalidation, lowering, and table-memory reporting.
- `analysis/export_json.rs`, `tools/redpiler_graph/loader.py`, and `tools/redpiler-graph-viewer/viewer.js`: inspection of the actual prepared artifact and validation of any added exported plan table.
- Existing graph tests, seeded generators, reference contract, CPU harness, and continuation tests.

Search all callers of `guard_matches_using`. A plan installed only in ordinary route dispatch would miss power/contact/Turbo evaluations and produce misleading coverage.

## Milestone A — select the family from runtime use

Use bounded temporary measurements or compile-gated trace summaries to classify evaluated roots by complete expression shape without storing a trace of every evaluation. Report candidate visits, recursive operations, affected State kinds, and rejection reasons. Use root-local bounded matching; normal release builds must contain none of this runtime work.

First candidate families:

1. A resource equality with an optional positive enabled test, with no other owner or Site predicate. Specialize only when more than a single leaf evaluation is removed.
2. A conjunction of extension/head Boolean literals and availability membership tests for one piston, with no other owner or Site predicate. Permit same-field availability `Any` only when the whole expression is proved equivalent to a membership mask.

If these have poor hot coverage, report the actual dominant shapes before adding another variant. Do not build an interpreter for arbitrary expressions as a substitute for choosing a useful family.

## Proposed immutable representation

Keep payloads small and measure their actual `size_of` values:

```rust
enum CompiledGuard {
    /// This evaluates one resource's contact and optional positive enabled condition.
    Resource {
        /// This identifies the existing resource state to read.
        resource: ResourceId,
        /// This is the contact that the resource's stored location must equal.
        contact: ContactId,
        /// This requires enabled=true in addition to the independent contact equality.
        require_enabled: bool,
    },
    /// This evaluates a conjunction over one piston's independent fields.
    Piston {
        /// This identifies the existing piston state to read.
        piston: PistonId,
        /// This selects which of the extension/head bits must be checked.
        required_bits: u8,
        /// This gives the required values of the selected extension/head bits.
        expected_bits: u8,
        /// This selects allowed availability values using an explicit five-value mapping.
        availability_mask: u8,
    },
}

struct GuardPlanTable {
    /// This maps GuardId to plan-index-plus-one, with zero selecting ordinary evaluation.
    by_guard: Box<[u32]>,
    /// This stores immutable instructions for matched roots, sharing identical plans.
    plans: Box<[CompiledGuard]>,
}
```

Use an empty binding table when the pass is absent or has no useful candidates. A dense `u32` binding for the reported 814,416 O1 guards costs about 3.11 MiB, before payloads; measure the current size and candidate density. Do not allocate `Option<large enum>` per guard, a hash table per delivery, or a plan per callback occurrence.

Identical plans can share a payload. A validated checked conversion handles plan-index overflow; do not use an unchecked sentinel encoding. Keep the table in Draft and lower it to immutable Program; no additional per-circuit mutable arrays are needed.

For the piston instruction, define bit 0 as extension and bit 1 as head presence, and use an explicit match to assign availability bits. An unrestricted availability test uses all five bits, and zero means false. Mask/value validation must reject unknown bits and values outside `required_bits`. Never infer a relationship between head, extension, and phase.

The resource instruction must still compare `at` when disabled unless `require_enabled` makes the whole conjunction false. It cannot treat `enabled=false` as the resource having no contact. Exclude negative enabled tests and multiple accepted contacts from the first version unless the census makes them the chosen useful family instead.

## Matching algorithm

1. Read a validated guard DAG. Match each referenced root using a bounded pure matcher; memoize compile-time match summaries by GuardId where shared subtrees would otherwise repeat work.
2. For a resource candidate, collect only the supported equality/enabled conjunction. All resource IDs must agree, all contact requirements must agree, and no residual predicates may remain.
3. For a piston candidate, intersect the Boolean requirements and availability masks. Handle negated state leaves explicitly. Decline a mixed-owner, Site-containing, or unsupported nested expression.
4. Check whether the candidate actually removes recursive/operator work. Single-leaf roots stay general initially; their current evaluator already performs a direct field test.
5. Intern exact plan payloads and fill the binding table. Keep unmatched roots at zero. Check cancellation in traversal and table construction.
6. Validate every binding against the original canonical expression and typed owners. Publish the complete table after successful construction; partial candidates do not affect graph behavior.

Use a small explicit matcher over the supported syntax rather than SAT solving. A limit such as 32 distinct visited expression nodes per attempted root is sufficient for the first implementation; decline larger roots, record coverage, and adjust only with evidence. Bound shared matching work and temporary memory; never enumerate combinations across an entire circuit.

## Proof and validator

The checker must establish `eval(original, state) == eval(plan, state)` for the supported family. There are two manageable finite domains:

- A piston has `2 * 2 * 5 = 20` combinations of these fields. Check all 20, including transient combinations absent from the entry pose.
- A resource candidate depends on enabled and one contact equality. Check both enabled values and the equality's true/false cases. The checker must first verify that the syntax contains no other contact or state dependency. This abstraction avoids enumerating a large resource footprint.

Use the canonical expression evaluator and a separately implemented plan evaluator in tests. In production validation, independently rederive the allowed summary from canonical syntax and compare it with the stored plan; do not merely call the builder and trust its output or accept a plan because its owner exists. Validate complete binding lengths, indices, masks, supported shape, and typed resource-contact membership.

Changing any plan field, owner, binding, or canonical expression must either fail validation or produce a newly proved plan. Test deliberate corruptions. This protects custom pipelines and exported artifacts as well as the builder.

No Site predicates enter these plans, so Live/Captured resolution is unchanged. Unsupported guards use the existing evaluator with precisely its missing-capture and short-circuit behavior.

## Runtime integration

Evaluate a compiled plan at the guard-root boundary, then fall back to `guard_matches_using` for an unbound root. Both `guard_matches` and `guard_matches_for_site` must use the same root-selection helper. Keep recursive fallback calls unchanged initially: adding a binding lookup to every recursive node can make unmatched expressions slower.

A successful plan reads the referenced state directly and returns one Boolean. It does not allocate, clone component state, resolve geometry, deliver callbacks, write cache epochs, or inspect neighboring graph records. Read state at each invocation; there is no reuse across a handler call.

Preserve guard-root profiling counts. Add separate optional plan-hit/fallback and state-load counters so that bypassing the recursive evaluator is visible. Existing semantic work counters and observation traces remain unchanged. Do not keep the reference evaluator or comparison-on-every-call enabled in production.

Empty-table and unmatched-root overhead must be included in benchmarks, including a circuit with zero eligible roots. A fast successful plan is insufficient if all other guards become slower.

## Invalidation, lowering, and export

Build plans after semantic rewrites. Any later unproven guard or owner-ID mutation clears plans through the existing pass runner's metadata invalidation approach. Disabled passes do not invalidate. Known preservation requires a real unchanged-dependency argument. Do not introduce a separate automatic pass scheduler or redesign all metadata APIs in this project.

Lower validated bindings once; runtime does no pattern recognition or repeated certificate checking. Reset/recompile regenerates immutable plans from the prepared graph while preserving existing mutable state and pending-work handoff. The plan never carries a mutable snapshot through reset.

Export additive typed plan records and bindings if plans are retained in the prepared graph. Update Rust export validation, Python loading/reference checks, viewer validation/inspection, and fixtures together. Older exports without the optional table must load. Consumers must reject malformed supplied plans; they must not silently interpret a dangling binding as general execution. Execution from imported JSON is not added by this pass.

Add Draft/Program byte accounting and GraphCounts/reporting where appropriate. Plan storage is real retained memory; do not report only the reduced recursive operation count.

## Tests and review batches

1. **Matcher/checker:** all supported forms and negatives, complete finite-domain comparisons, valid/damaged plans, IDs, mask overflow, and shared/forward-referenced DAGs. Assert actual matching coverage.
2. **Root execution:** run general and planned evaluation on the same state; test both wrappers, every root consumer, all piston phases, enabled/disabled resources, and unbound mixed Site/Captured guards. Verify an unsupported child never disappears.
3. **Temporal integration:** shared-OR, independent QC/update BUD, adders with cancellation, nested Turbo and captured/live divergence, observer visibility, imported ticks, command feedback, and early/late reset cuts. Compare ordered observations and pending future work against general Graph and the interpreter.
4. **Pipeline/export:** alone/disabled, before a mutating pass, after other optimizations, deliberate stale metadata, cancellation, deterministic rebuilding, corruption tests, and older/new JSON consumers.
5. **Performance:** five paired uninstrumented CPU samples with all other passes fixed, plus a zero-hit ordinary circuit. Separately profile hit coverage, recursive calls avoided, state loads, and retained bytes. Run focused and broader debug/release checks from the shared plan.

Review after the first useful family before adding another instruction. Report median/range and whether fallback overhead offsets the gain. This pass preserves semantic callback/sample/action counts; fewer evaluator operations are the intended reduction. It does not establish that callbacks themselves are unnecessary.
