# Guard optimization implementation plan

Status: scoped, not implemented or benchmarked. Source inspection: 2026-10-14. These scopes supplement the existing pass projects; they do not replace opt-01, Turbo indexing, or callback pruning.

## What to implement

| Project | Removes | Implementation priority |
| --- | --- | --- |
| [Opt-10: compile common guard patterns](steps/opt-10-compiled-guard-patterns.md) | Recursive dispatch and repeated reads within one guard evaluation | First: census, then a small specialization with immediate runtime coverage |
| [Opt-08: state-domain reduction](steps/opt-08-state-domain-guards.md) | Contradictory, redundant, or exhaustive tests of the same state field | Second, if the census finds frequently evaluated candidates |
| [Opt-09: Boolean factoring](steps/opt-09-boolean-guard-factoring.md) | Repeated subexpressions and redundant branches | Third; use measured patterns and a bounded rewrite algorithm |

Numbering describes logical graph-processing order, not implementation order. Opt-10 works on the current O1 graph without opt-08 or opt-09. It is the best first experiment because it can accelerate an existing expression without needing that expression to become simpler.

Finish and measure the separately scoped [receiver-less callback pruning](steps/opt-07-pruning.md) independently. Keep its effect distinguishable from guard optimizations. No piston or instant-region specialization is included here.

## Evidence and its limits

The existing [callback research](CALLBACK_RESEARCH.md) reports, for one O1 CPU_BubbleSort episode, 11,704,471 guard-root evaluations and 29,255,884 recursive guard operations. Of those operations, 12,737,473 were State, 6,971,543 All, 3,302,174 Not, and 2,124,179 Any. These are visit counts, not self-time measurements. They precede subsequent indexing/pruning work and must be refreshed for the implementation baseline.

Opt-01 reduced stored guards substantially without a demonstrated runtime improvement. Therefore fewer stored expressions is not the acceptance criterion. Each project must show which hot evaluations it changes and measure uninstrumented execution. Do not promise a speedup from the old counts.

The current evaluator in `backend/graph.rs::guard_matches_using` recursively interprets `Guard`. Guard reads have no simulation side effects. Short-circuiting can therefore change the number and order of pure reads within one evaluation while preserving its Boolean result. This does not permit sharing results across delivered callbacks: a handler can synchronously change state or start a nested walk.

## Shared rules for implementation agents

Read the [pass agent guide](../../../crates/core/src/redpiler/passes/README.md), [redstone model](../../REDSTONE_MODEL.md), [piston model](../../PISTON_MODEL.md), and [interpreter architecture](../../INTERPRETER_ARCHITECTURE.md). Follow the guide's file organization, cancellation, testing, and reporting rules.

Each pass lives in its own descriptive `.rs` file under `crates/core/src/redpiler/passes/`. `mod.rs` registers passes; `rich.rs` coordinates preparation/lowering. Neither contains the transformation algorithms. Extract common guard traversal, exact interning, and complete reference-remapping helpers into `passes/utils/` only when a second actual consumer needs them. Preserve opt-01's existing behavior while extracting them.

The interpreter remains the independent behavioral oracle. General Graph remains the executable fallback for an optimization candidate. No Direct/Native backend, physical Motion model, runtime World geometry search, or SAT dependency is needed.

Every new struct field and enum payload needs a one-sentence explanation. The type sketches in the individual scopes describe responsibilities; reuse existing types and omit unnecessary wrappers. Keep matching, proof, application, and reporting readable as separate steps.

## Semantic boundaries

All three projects must preserve:

- Separate power, Update, and Shape behavior; QC data changing alone is not an activation.
- Delivery order, direction, sequence labels, meaningful duplicates, and effective receiver selection.
- Live versus Captured operands, including missing captures and captures from an earlier topology.
- Analog outputs, component state, resource ownership, and every piston publication phase.
- Requests, acceptance revalidation, pending ticks, priorities, deadlines, deferred publications, and reset/continue behavior.
- Synchronous command results and note-block host effects on the supported subset.

Do not infer `extended == head_present`, `enabled == ResourceAt(...)`, or a stable pose from a pending availability phase. These are separate mutable fields. A disabled resource still has an `at` value; the evaluator tests that value independently of `enabled`.

Current piston revisions are not complete guard-cache epochs: deferred publication changes availability/head/extension without necessarily incrementing the revision. No tick-wide, route-wide, or revision-keyed result cache is authorized by this plan.

## Preserve validation without building a theorem prover

The existing `guard_implies` and `guard_implies_site_profile` checks in `program_graph.rs` recognize syntactic witnesses. A logically equivalent expression can lose the witness used to validate a callback receiver. Structural validation must remain sound and enabled.

For opt-08/09, use this initial integration strategy:

1. Keep existing guard records intact while building interned candidate expressions. Keep contact-variant guards as stable witnesses in this batch.
2. Choose a rewritten root separately for each owner. Power roots and callback roots can use different equivalent representations of the same old expression.
3. For each callback replacement, use the existing implication rules against the unchanged contact witnesses, or the required Captured profile predicate. Extract the existing check into a pure shared helper if necessary; do not weaken it.
4. If a candidate loses its witness, retain that owner's original expression. Other owners can still use the candidate. A false candidate may retain its witness in an `All(false, witness)` form if that is both valid and cheaper.
5. After choosing roots, compact only unreachable guard records using a complete remap. Preserve guard zero as `Always`. Do not compact nodes, routes, contacts, or resources.

This deliberately trades some optimization coverage for a small, reviewable implementation. Report witness-related declines. If they dominate hot candidates, propose a separate bounded implication-checker project with measurements; do not silently expand this batch into a solver framework.

Callback links must use the selected guard of their corresponding delivery occurrence. Update the exact occurrence multiset, including duplicate sequence labels; do not identify an occurrence using sequence alone. Guard consumers include contacts, power links, callback links, deliveries, comparator far overrides, and wire relations. Search the current source for additional consumers before writing a remapper.

## Pipeline and metadata

The eventual logical order is:

```text
required preparation
  -> opt-01 normalization/reduction
  -> state-domain reduction, when selected
  -> Boolean factoring, when selected
  -> receiver-less pruning, when selected
  -> Turbo callback indexing
  -> existing dust specialization
  -> compiled guard patterns, when selected
  -> validation and lowering
```

This is a proposal, not authorization to change the default list before review. Use custom pipelines to compare passes alone and in combination. Register approved passes in the existing optimization-tier lists; do not add a dedicated command-line flag per pass. The runner requires unique names, so do not register opt-01 twice as a cleanup shortcut.

Any guard-ID rewrite must invalidate metadata that embeds guards or depends on their representation. Route-changing passes invalidate Turbo offsets. Compiled guard patterns are derived metadata and must be rebuilt or cleared after any unproven mutation. Follow the runner's current invalidation mechanism; do not introduce a second metadata framework in this work.

## Reviewable batches

1. **Census:** count actual hot root patterns, pattern misses, and expected avoided recursive operations under `graph-profile`. No permanent root-ID trace buffers and no normal-build instrumentation.
2. **One complete pass:** implement the smallest measured useful family, its checker, unit/property tests, composition tests, and a five-pair runtime comparison. Stop for review.
3. **Next family:** proceed according to measured coverage. Do not implement a broad rule catalog whose CPU candidates all miss.

The individual documents define the milestones within each pass. A slowdown or zero useful coverage is a valid result: retain general execution and report it.

## Shared validation and benchmark gate

Discover the actual tests, run focused tests in debug/release, and run the broader Redpiler and reference-contract suites for these cross-cutting changes. Add reproducible generated guard tests and real interpreter comparisons using the existing fixture loader and observation contract. Compare pending future work and intermediate callbacks, not just final RAM. Existing unrelated failures must be reported separately, not edited away.

Use the current release CPU harness:

```text
cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 5
```

Compare the same baseline with and without one pass, with identical pruning/index/dust settings. Use five fresh paired samples, alternate order, and keep preparation, start stimulus, flush, and verification outside timing. Preserve the pinned schematic and disclose the existing identical sign removal in memory for both executors. Use the existing crate-private custom-pipeline test support; if the external bench needs selection support, make the smallest reviewed change rather than a new benchmark harness or per-pass product flag.

Separately profile root/leaf operations, specialized hits/misses, delivery rows, and state reads. Report ordinary work counts, matched observations, median/range, retained bytes, and proof/candidate coverage. Instrumented inclusive timings are not self-time. No speedup is established by this plan or by a smaller table alone.

Out of scope: cross-callback memoization, global reachable-state exploration, BDD construction, new solver dependencies, event deduplication, callback coalescing, and piston/region specialization.
