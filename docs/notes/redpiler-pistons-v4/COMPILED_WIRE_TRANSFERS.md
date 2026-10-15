# Compiled wire transfer expressions

This is an exact partial evaluation of connecting dust calculations, implemented by the existing `specialize-wire-queries` pass at O2/O3. It is a smaller milestone than contracting a whole instant region. Reset clocks and actor/resource state keep their existing owners. The earlier private two-wire transcript remains available under its stronger certificate.

## Why this slice

The prepared ADDER1BIT graph has 32 wires, but none meets the old private two-wire output grammar. Most have nearby payload/head contacts and wires at several heights. A network cannot be replaced by its final electrical fixed point: the previous negative control reverses the first two accepted piston requests for prepared case 0-1-1 and changes its result. Compiling a pure power expression avoids this event-order problem without pretending that reset provenance proves a whole circuit atomic.

## Representation and algorithm

`WireQueryFacts.transfer` optionally contains separate `seed` and `turbo` expressions. Each expression has two 24-bit canonical-neighbor masks:

- `direct`: read these potential wire strengths without a geometry test.
- `conditional`: read these diagonal strengths only when the original step-up/down condition holds.

The compiler examines every neighbor's fixed class or every contact alternative. It omits a possible strength contribution only when no admitted variant/profile/receiver can represent a wire. Fixed geometry folds conditions to true or false; changing contact geometry retains the original condition. Planning costs O(24 + inspected contact variants) per wire. There is no stored power value, graph-size search during execution, private event queue, new backend or new dependency.

Seed and Turbo rules are deliberately separate. A seed includes direct top/bottom wire strengths; Turbo reads horizontal direct wires. Seed ascent requires `!above.solid && !horizontal.transparent`; Turbo additionally requires `horizontal.solid`. Both descend when `!horizontal.solid`. Captured Turbo geometry and wire strengths come from that walk's snapshots; strong conducted power remains live. Pure maxima can reorder reads because no notification or state mutation executes inside a power query.

The correctness argument is compositional: an omitted strength is identically zero or has a false step condition; retained conditional terms use the same geometry; the maximum and saturating attenuation therefore agree. With equal results at each pure query, the unchanged event machine writes the same values and takes the same subsequent transitions, including nested walks.

The original walk still determines heading, queue layers, intermediate writes, every outside callback and duplicate, nested execution, requests, acceptance and deferred publication. Thus this milestone removes repeated strength/property inspection, **not** wire states, reset transitions or semantic callbacks. Whole instant-region contraction remains unfinished.

## Validation and tests

Graph validation independently re-infers each complete certificate and rejects stale/corrupt masks. Later passes invalidate facts by default. Existing preserving passes keep the same geometry/occupants. JSON exports include the additive expressions; Rust, Python and HTML consumers reject malformed masks and still accept older exports without them.

- 6,656 finite local cases compare selected slots with the separate native seed/Turbo equations, including rotations, known/unknown geometry and every basis input slot.
- Runtime strength tests compare actual optimized queries with metadata-free queries for every four-bit direct/adjacent input pair.
- Captured/live tests change resource selection and live strengths after capture; conducted sources remain live while captured wire geometry/strength remains old.
- Explicit regressions protect the different vertical/ascent rules and possible wire contact profiles/typed receivers.
- ADDER1BIT checks all 32 wire plans over 256 varied component/resource snapshots and all four headings (32,768 captured comparisons plus 8,192 seed comparisons), including changing live state after capture.
- Existing arithmetic, cancellation, nested walks, shared-resource and interpreter continuation suites remain required.

These checks establish expression equivalence and exercised continuation histories. They do not prove every arithmetic/control history or authorize dropping boundary callbacks.

## Final release measurement

**Draft implemented and awaiting review; no substantial speedup established.** Both produced test binaries passed the complete Redpiler filter: 309 passed, 7 ignored, in debug and release. The final core test/bench and server checks passed, as did focused formatting, diff checks, Python loader tests (10) and viewer tests (18). ADDER1BIT's O2/O3 JSON fixtures were regenerated; O0/O1 exports and source schematics are unchanged.

Compared the pinned normal built-in-reset baseline (`target/builtin-reset-bench/cpus.exe`, hash checked against its successful prior report) with the new normal release executable (`target/wire-transfer-bench/cpus.exe`). Both use O3, the existing harness and no unsafe fixed-point/debug recording features. Five pairs per device alternate which binary runs first. Setup, compilation, publication, reads and validation remain outside the runtime timer; the generic adder protocol includes its existing 64 control toggles. CPU uses the same approved sign-free in-memory copy for both executors.

Every sample matched the interpreter and its reset continuation checks. Cross-binary comparisons also matched observations, chat, checkpoints, pending cut state/future and complete semantic work counts. No callbacks, wire walks, scheduled ticks, samples or actions were eliminated by this expression lowering.

Times are seconds. Paired change is the median of `(new / baseline - 1)` across pairs; positive means slower. It can differ from the ratio of aggregate medians.

| Device | Ticks | Compiled wire plans | Baseline median [range] | Draft median [range] | Median paired change |
|---|---:|---:|---:|---:|---:|
| adder1bit | 512 | 32 | 0.009163 [0.009009, 0.010741] | 0.009265 [0.008887, 0.010641] | -0.93% |
| adder11bit | 512 | 295 | 0.074525 [0.073860, 0.080290] | 0.075142 [0.068309, 0.077207] | -5.44% |
| divider | 512 | 5,128 | 0.478370 [0.463200, 0.485668] | 0.468704 [0.449482, 0.470977] | -2.02% |
| cpu | 100 | 41,354 | 1.169071 [1.157415, 1.179181] | 1.161098 [1.142478, 1.190115] | +0.32% |
| counter | 1800 | 837 | 0.993345 [0.977763, 1.018575] | 0.985379 [0.963395, 1.004939] | -1.28% |
| anpu | 400 | 34,781 | 1.056053 [0.983475, 1.078371] | 1.026169 [0.971838, 1.045140] | -1.03% |

All ranges overlap. CPU medians differ by -0.68%, while the paired median is +0.32% with mixed pairs: **no demonstrated CPU acceleration**. Divider/Counter/ANPU have small measured improvements, insufficient evidence of a substantial general gain. ADDER1BIT/11BIT have slightly higher aggregate medians with mixed paired results. Do not describe these results as a large speedup.

ADDER1BIT expressions contain 42 direct and four conditional input terms in each query mode across 32 wires. This reduces strength/geometry inspection, but preserves six non-dust source queries and the expensive walk/callback machinery. The draft adds 20 bytes per wire plan, approximately 0.79 MiB for CPU; this is a table-size difference, not peak RSS. Such local algebra does not remove the dominant event workload. A whole-region optimization must additionally absorb discovery/capture and internal delivery work while proving its boundary transcript.

CPU work remains: 100 ticks, 20,005 scheduled ticks, 1,760,070 callbacks, 32,505 wire walks, 302,784 piston samples and 27,884 actions/publications. All 30 old/new pairs matched their independent interpreter episodes and cross-binary protected observations.

Artifacts: `target/wire-transfer-bench/{device}-{baseline,transfer}-{1..5}.{json,log}`, compact `summary.json`, and full test/check logs in that directory. The local runner only invokes the existing benchmark and compares its JSON; it adds no runtime timers or instrumentation. Build commands were `cargo test -p mchprs_core --lib --locked --no-run`, the equivalent release build, and `cargo bench -p mchprs_core --bench cpus --locked --no-run`; produced test executables ran `redpiler:: --test-threads=1`. Each benchmark invocation used `--graph-short --all-devices --cpu DEVICE --graph-tier 3 --iterations 1 --output REPORT`.

Not checked here: a complete CPU sort, full FPU, the full workspace, broad new fuzz campaigns or fresh sampled attribution. The requested follow-up is sampled CPU profiling on the current source, rather than interpreting accepted-plan counts as a performance result. Whole instant-region contraction is unfinished.
