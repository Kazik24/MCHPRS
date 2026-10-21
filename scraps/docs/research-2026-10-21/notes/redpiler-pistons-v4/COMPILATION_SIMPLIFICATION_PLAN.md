# Compilation simplification: one owner, explicit checks, direct lowering

Status: implemented and awaiting review, 2026-10-19. The current target is **at most 10 seconds from
compile request to activation for CPU_BubbleSort** on the reference machine.
Runtime acceleration is a separate project.

## Implemented approach

The implementation keeps the existing deterministic reference algorithms. For
the two expensive instant products, the runner prepares a canonical reference
from its current valid graph before invoking a builder. Builders may read it,
but cannot replace the runner's reference. Publication compares the entire
candidate against that reference and checks affected ownership dependencies.
This shares the existing source check instead of introducing a second symbolic
compiler or weakening verification to ID-range checks.

The equivalence condition is explicit: boundary derivation does not read the
catalog/legacy instant plans that boundary publication replaces; layer derivation
does not read the layer plan being replaced. Relevant source structure and
classic ownership/catalog inputs remain immutable during the builder. Derivation
before publication therefore yields the same reference as the former derivation
after publication. A requested analysis for a different output kind cannot
authorize that output; it takes the normal validation path.

Steps 1–4 now have code: split compile timings, typed publication checks, selective
dependent validation, an owning `ValidatedGraph`, and retained corridor evidence.
Step 5 uses the shared canonical reference described above; arbitrary drafts and
the full-check test oracle still reconstruct independently. This does not claim
an independent semantic proof of the canonical compiler itself. Interpreter,
fault-injection and full-check differential tests remain necessary.

Two exploratory optimizations did not show an attributable improvement in single
CPU diagnostic runs and were removed: grouping per-cell source lookups, and
memoizing guard/evaluator emission pairs. Temporary diagnostic timers were also
removed. The retained guard scanner uses one map entry lookup and avoids DFS
scratch allocation for atomic guards, preserving the original leaf sequence.

Fresh compilation and activation results are recorded in
[the compilation report](COMPILATION_AND_RUNTIME_PROFILE.md). The measurements
below remain the previous baseline, not final results for this implementation.

The release asynchronous activation probe now passes in three fresh worlds:
9.576–9.665 s request-to-activation. Separate compiler-only pairs still vary up
to 10.965 s in the final build, so a reliable ten-second ceiling under changing
load is not established. Full measurements, provenance, correctness checks and
remaining validation limits are in the compilation report. No runtime algorithm
or supported-circuit budget was weakened to meet this probe.

## Evidence and the actual problem

The latest three alternating release pairs reduced compiler time from a median
28.323 s to 18.951 s. The new samples were 21.576, 18.951 and 18.341 s.
Those measurements exclude the plot snapshot and final publication, so they do
not yet measure the complete target. The configuration is O4, stable instant
inputs, budget multiplier 8, and identical in-memory sign normalization.

The median sample has the following **disjoint elapsed stages**:

| Stage | Time | What the timer includes |
| --- | ---: | --- |
| `identify-instant-boundaries` | 2.984 s | Builder and output validation |
| `compile-instant-layers` | 6.625 s | Builder and output validation |
| After the pass pipeline | 4.545 s | Final validation, kernel lowering, program/backend construction |
| Everything else | 4.797 s | Parsing, other passes and remaining compiler work |

The first three rows total 14.154 s, approximately 75% of compiler time.
This does **not** mean that 75% is redundant validation. The current report does
not separate these bodies from their checks or split all lowering operations.
Removing duplication must save at least 8.951 s plus the unmeasured outer work
to meet the target for this sample; the observed maximum requires more.

Sources: [measurement and prior changes](COMPILATION_AND_RUNTIME_PROFILE.md),
[`summary.json`](../../../../../target/compile-pipeline-final-paired/summary.json), and
[`2-final.json`](../../../../../target/compile-pipeline-final-paired/2-final.json).
The target artifacts are local and may be absent in another checkout.

Source inspection confirms these repeated operations:

1. `identify_instant_boundaries` builds a catalog. Its output check builds it
   again. The layer builder's output check builds the catalog again. Lowering's
   full validation builds it again: **four boundary inferences**.
2. `compile_instant_layers` builds layers and reduced programs. Its output check
   recompiles them. Lowering's validation recompiles them again: **three layer
   compilations**.
3. Layer validation returns a `LayerCompilationContext` with the partition and
   incoming-power index. Intermediate checks discard it. Final lowering already
   reuses its immediately preceding check's context; that reuse is implemented.
4. Connected layer construction certifies corridor arrivals. Kernel lowering
   calls corridor certification again for executable regions.
5. Every builder triggers checks of **all** execution plans, including unchanged
   fixed signals and fixed/mixed classic programs.

Reconstruction detects stale or forged artifacts. Running the same compiler
twice is not an independent proof that its semantics are correct. Keep the
interpreter differential tests and ordering counterexamples.

## The intended shape

```text
isolated snapshot
    -> parse circuit
    -> validate input
    -> run configured rewrites and checked plan builders
    -> finish with an owned, validated graph
    -> lower its checked products
    -> attach executor
    -> publish if live entry still matches
```

The existing pass API remains the extension point. Rewrites keep mutable access
and clear plans. Builders keep immutable access and return a typed `PlanOutput`.
Validation is mandatory pipeline policy, never an optional optimization pass.

The design rule is: **build an artifact, check it against its dependencies, and
consume that checked artifact without rediscovering it**. Give each operation a
named function and a clear owner. Do not introduce a scheduler for passes, a
general cache service, or a new compiler IR for this project.

## Responsibility boundaries

| Existing location | Responsibility after cleanup |
| --- | --- |
| `passes/rich.rs` | Parser assembly and prepared input/entry ownership |
| `passes/mod.rs` | Ordered dispatch, cancellation, progress and failure attribution |
| `passes/planning.rs` | Typed plan publication and the exact fields each result replaces |
| `passes/validation.rs` | Required checks after a rewrite or a particular plan change |
| `program_graph/validation.rs` | Structural checks and plan certificate checks; full arbitrary-draft admission |
| `analysis/instant/boundaries.rs` and submodules | Cell/corridor discovery and complete boundary evidence |
| `analysis/instant/layers.rs` and submodules | Causal composition, reduction and transient analysis products |
| `program_graph/lowering.rs` | Consume validated products and construct runtime tables |
| `plot/redpiler_compilation.rs` | Snapshot, background worker, cancellation and atomic activation |

Do not put domain analysis in `passes/utils`. Reuse `utils/instant_mechanisms`
where it already owns reset discovery, and keep genuinely shared indexing there
only when multiple consumers need it. Avoid unrelated file moves.

## Step 1 — Expose the remaining costs before changing checks

Use the existing monitor/report and benchmark. Split enabled pass timing into
body and output validation while retaining the total. Split lowering into final
validation, instant kernel construction and ordinary program construction.
Record snapshot and final publication separately from worker compilation.

If persistent reporting changes would become a profiling framework, use a small
temporary diagnostic patch and retain only the results. No per-row timers, no
formatting in graph execution, no new dependency or feature flag.

Count boundary discovery, layer compilation and corridor certification in a
focused test/diagnostic run. Do not attach instrumentation to runtime state.
Use counts to distinguish duplicated work from an expensive single algorithm.

Acceptance: a CPU report explains the complete request-to-activation interval;
the current three-pair compiler-only baseline remains clearly separate.

## Step 2 — Check the changed plan and affected dependents

Replace the builder branch's unconditional all-plan validation with explicit
dispatch based on the actual `PlanOutput` variant. Keep full structural and plan
validation after arbitrary rewrites, and full validation at external draft entry.

Implementation:

1. Validate output dimensions and references **before** publication. Check vector
   lengths before `zip`, reset indices before indexing, and all typed references.
2. Derive a finite internal plan category from `PlanOutput`; a pass author does
   not declare which checks may be skipped. Keep an exhaustive central `match`.
3. Determine the affected categories from a small, explicit dependency table.
   Include transitive dependencies and cross-plan execution ownership conflicts.
4. Publish the result using the existing replacement rules, then check the new
   category and every retained affected dependent before the next pass runs.
5. Preserve current rejection of stale dependents. Do not silently change a
   caller-supplied pipeline by rebuilding or deleting them, except for existing
   replacement rules such as boundaries replacing old regions/proxies/layers.

This is a fixed set of compiler products, not a general dependency framework.
Use existing enums and straightforward matches. During migration an unaudited
category must conservatively run all plan checks. Do not infer dependencies
from pass names, requested optimization level, or the default pass order.

The source audit already establishes these dependencies; complete the audit of
called helpers before enabling selective checks for a category:

| Changed input | Known retained products which can be affected |
| --- | --- |
| Circuit structure or entry assumptions | All plans and transient analysis |
| Boundary catalog | Layers and kernel analysis; publication already removes old layers |
| Classic net ownership | Boundary corridors, fixed classic eligibility, therefore dependent layers |
| Old instant region ownership | Proxies and conflicts with the boundary catalog |
| Instant proxies | Fixed classic eligibility and route ownership |
| Fixed signal queries | Fixed classic compiled inputs |
| Wire query/callback facts | Fixed classic compiled wire and route instructions |
| Fixed classic program presence | Mixed classic eligibility |
| Dust path ownership | Classic-net analysis and ownership checks |

Boundary discovery's dependency on classic nets is indirect:
`boundaries/corridors.rs::discover_corridors` excludes their wires. A search of
only `boundaries.rs` would miss it. Mixed classic validation explicitly rebuilds
against fixed classic presence. Treat these as regression requirements.

Acceptance: unrelated builders do not rebuild boundaries/layers. The existing
`builder_rechecks_existing_plans_when_a_dependency_changes` still rejects a
stale fixed classic plan. Custom orders, disabled steps and replacements retain
their behavior. Initially keep final full lowering validation unchanged.

## Step 3 — Transfer validation by ownership into lowering

Once every pipeline transition is checked, its successful result is already a
valid graph. Replace the mutable escape between pipeline completion and lowering
with a small owning result. This lets production lowering consume validation
without repeating the entire check.

An illustrative shape, not an instruction to duplicate prepared fields:

```rust
pub(crate) struct ValidatedGraph {
    prepared: PreparedGraph, // Owns the checked graph, report and imported entry work.
    layers: Option<LayerCompilationContext>, // Owns matching transient analysis for immediate lowering.
}
```

Keep these fields private. Construct the result only at a successful checked
pipeline exit. Expose `graph(&self)` for export and consuming `lower(self, ...)`
for execution. If tests/tools need the mutable draft again, consuming conversion
must discard the validation and analysis. No mutable accessor, `DerefMut`, public
constructor, serializable receipt, boolean `validated`, or reusable global token.

Keep `CircuitDraft::lower_with_monitor` as the fully checked entry point for
arbitrary drafts. Share one internal lowering implementation with the consuming
pipeline path. Both paths produce the same program/state; neither bypasses
kernel admission, runtime attachment checks or imported-tick binding.

The pipeline should retain only the latest applicable layer context. A rewrite,
boundary replacement or affected plan change discards it. End by moving the
context with its graph, not by matching pointer identities or hashing tables.

Update **both** production callers: `Compiler::prepare_graph_backend` and
`analysis/export_json.rs::prepare`. Keep serialization validation independent;
an export supplied by another process cannot carry an in-memory validation proof.

Acceptance: the optimized path performs no final reconstruction of an unchanged
plan. Arbitrary drafts still receive full checks. A compile-fail/doc example or
API test demonstrates that mutable access cannot coexist with the owned result.
No full graph clone is introduced to implement this transfer.

## Step 4 — Keep corridor evidence rather than proving it twice

Extend the existing `LayerCompilationContext` only with analysis that is already
needed twice: the checked arrivals for each region used by kernel lowering.
Separate not-yet-computed, unsupported, and successfully certified empty results;
an empty corridor is valid and must not be confused with failure.

Thread the context through connected layer construction so that its calls to
`composition::corridors::certify` retain their result. Kernel lowering reads the
same checked evidence. For a region whose compilation never needed that evidence,
compute it once when lowering actually needs it. Discard it on any relevant
change, and release it after lowering.

Keep witness types private to the analysis modules. Do not put these products in
`CircuitDraft`, JSON or `CircuitState`. A reconstructed plan from an arbitrary
draft still needs evidence checked against that draft; serialized claims cannot
be trusted merely because an in-process producer normally fills them correctly.

Acceptance: supported connected regions are not certified again during immediate
lowering. Compare kernels with the existing independent reconstruction path.
Record temporary memory; retaining every failed attempt's scratch is forbidden.

## Step 5 — Replace expensive reconstruction with certificate checking

Steps 2–4 remove repeated checks of unchanged artifacts. The first verification
of a newly built catalog/layer plan still reconstructs it. If this remains a
major measured cost, replace that reconstruction with a smaller source checker,
one artifact family at a time. This is the larger correctness task, not a flag
which trusts every built-in builder.

For the boundary catalog, check:

- Each admitted cell's actor/resource/contact ownership and reset provenance.
- Corridor membership, classic-net exclusions, conductor and callback relations.
- Every crossing power/QC, Update/Shape and geometry dependency, including absent
  or wrongly classified crossings. Verifying only listed ports is insufficient.
- Correct declined/unowned actors and completeness of the ownership partition.

For layers and reduced programs, check:

- Source occurrences, activation qualification, FIFO dependencies and inhibition.
- Shared-resource ownership, request versus acceptance conditions and reset work.
- Expression typing/references, causal dependencies, boundary root completeness
  and the equivalence of reduction, including necessary cold reconstruction data.
- Complete representation of externally observable effects, or a supported
  decline. Missing effects must not be treated as dead-code elimination.

Use existing source witnesses and graph indexes. Extend certificate data only
where an existing invariant cannot be checked from those witnesses. Check each
source table at most as often as correctness requires; a complete boundary scan
may still need to inspect the whole graph once.

Keep reconstruction as a test oracle while migrating. Mutation tests must delete,
add and alter witnesses, including valid IDs with wrong semantics, and compare
accept/reject results. A checker must not approve a forged plan merely because
its references are in range. If an independent cheaper check cannot yet be
specified convincingly, keep the reconstruction for that family.

Do not replace `regions::perimeters` with filtering the old catalog's ports
without proof. It deliberately rebinds owners and rescans: composition can expose
reads/effects that were internal under a different ownership arrangement.

Acceptance: new plans have one construction plus a source check, rather than a
second complete compilation. No admission, queue-order or reset contract changes.

## Step 6 — Optimize a single expensive algorithm only if still necessary

If the complete result still exceeds 10 s, use the split profile to select the
next operation. Two concrete source candidates deserve investigation:

1. `cells::CellIndex::build` and layer preparation independently build incoming
   power indexes. Share an index within the unchanged planning interval only if
   its measured cost justifies extending the existing context. Avoid copying
   `PowerLink` tables again or caching across rewrites.
2. `composition/corridors.rs::certify` loops over corridors, candidate cells and
   their wires, issuing symbolic queries. Use reverse indexes to narrow producer
   and consumer candidates, retaining a conservative candidate superset and all
   original guard/contact/conductor checks. Do not choose a producer by the first
   matching row or weaken the certificate to achieve a faster compile.

These are source-based candidates, not measured dominant functions. Do not start
by rewriting the union-find partition, introducing parallel passes, shrinking
proof budgets, or imposing new limits on accepted circuits.

## Verification and completion criteria

Every step is independently reviewable. Run focused tests after each change;
reserve expensive fixture sweeps and release builds for a coherent checkpoint.

Correctness gates:

1. All `PlanOutput` variants: malformed length, dangling reference, invalid index
   and replacement of a dependency. No panic or silently truncated publication.
2. All meaningful producer/dependent builder pairs in both orders, plus disabled,
   empty, cancelled and rewrite-between-builders pipelines. Independent plans
   remain independent; stale executable owners still reject.
3. Fault injection into boundaries/layers: omitted crossings, duplicate or wrong
   occurrences, wrong shared owner, incorrect expression/root and lost reset.
4. Exact graph/export agreement and lowered program/state agreement with the
   full-check path. Lowering must not change enabled regions or kernel ownership.
5. ADDER1BIT/ADDER11BIT, Divider, Counter/PC counter, ANPU and CPU episodes compare
   output **and** state, resources, imported/pending work and reset continuation.
   Retain existing ordering and captured/live regressions.
6. Background compilation remains responsive: reset/cancel, stale entry, worker
   failure and live edits cannot publish an executor for a different snapshot.

Performance gates:

- Use the existing release CPU harness and frozen fixture. Preserve tier, stable
  input assumption, budgets, sign normalization and exact execution coverage.
- Pin before/after binaries. Run at least five alternating pairs without other
  builds, report all samples and their maximum as well as the median.
- Measure request-to-activation on the asynchronous plot path separately; a fast
  harness worker time alone does not satisfy the 10 s requirement.
- CPU_BubbleSort must stay within 10 s in the defined reference runs. Report cold
  and warm results separately. Queueing under other load must be visible; this is
  not a hard real-time guarantee for arbitrary selections or machine load.
- Check ANPU and CHUNGUS for scaling, peak memory and cancellation responsiveness.
  No runtime semantic counter reduction or missing optimization is accepted as a
  compilation speedup. Runtime timings are a regression check, not the objective.

## What to remove, replace and reuse

Remove repeated **calls**, not the correctness contracts: unrelated all-plan
checks, duplicate reconstruction after validated ownership transfer, duplicate
corridor analysis, and discarded/rebuilt transient indexes where measured.

Replace monolithic validation dispatch with explicit changed-product checks and
an owning successful pipeline result. Keep full arbitrary-draft admission and
the existing interpreter/reference harnesses. Do not add a second compiler path.

Reuse `PlanOutput`, `Pass::Rewrite`/`Build`, `TaskMonitor`,
`LayerCompilationContext`, the existing partition/power/corridor algorithms,
streaming callback-link validation and the asynchronous snapshot boundary.

Comments explain invariant preservation and ownership. Each new field or enum
payload gets one sentence describing its meaning. Keep tests under the existing
`passes/validation/tests.rs`, analysis tests and `backend/graph/tests` directories.
Required policy belongs in `passes/validation.rs`; actual optional optimization
passes remain individual files in `passes`.

The preceding research used the 18.951 s compiler-only median as its baseline.
Implementation and subsequent results are described at the start of this file
and in the compilation report. The dependency table is a conservative permission
list: unaudited indirect dependencies retain complete plan checks.
