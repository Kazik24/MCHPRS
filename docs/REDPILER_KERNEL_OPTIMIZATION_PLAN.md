# Redpiler kernel optimization implementation plan

## Goal and evidence

Reduce time inside `Compiler::tick()` while preserving ordered redstone behavior. The supplied ZCore12 profile attributes 53.6% of kernel samples to piston events, 39.2% to motion completion, and 7.3% to scheduled ticks. `set_token_pos -> run_move_steps` accounts for 65.1% inclusively. Arrangement processing accounts for 24.9% of the kernel, pre-callback wire refresh for 12.4%, and `token_at` has 5.4% self samples. These percentages overlap where stated in the profile and are not additive speedup estimates.

Evidence: `target/zcore-kernel-profile-20261101/report.txt`. Uninstrumented baseline: 2,641–2,932 TPS, median 2,786 TPS across three 4,096-tick runs. The profile contains no publication work. Publication cadence and compilation caching remain separate projects in `REDPILER_OPTIMIZATION_SCOPE.md`.

## Shared methodology

Implement each experiment as an isolated change. Keep a baseline executable built from the same source revision, compiler version, optimization settings, target CPU settings, and feature set. Record source revisions and build commands with each result. Do not compare instrumented throughput against uninstrumented throughput.

Use the same loaded circuit, input sequence, initial scheduled ticks, piston state, and warmup. Run at least seven paired baseline/candidate trials with alternating order. Record total elapsed time, ticks, median TPS, spread, and retained memory. Use the existing 4,096-tick workload for continuity and a longer run to reduce short-run noise. Exclude compilation, publication, and reference checking from kernel timing. Also measure integrated execution separately.

Workloads must include ZCore12, a wire-heavy circuit, an analog comparator circuit, opposed/two-token piston groups, and circuits with observers, repeaters, command blocks, and noteblocks. Include a small circuit to expose overhead added to common cases. Repeat a noisy result before accepting a performance conclusion.

For correctness, compare baseline and candidate at logical tick boundaries: projected blocks, comparator outputs, ordered pending ticks, pending piston events, active motion projection, command emissions, and notes. Sort only collections whose order is semantically irrelevant. Compare against native execution on supported reference cases, including intermediate screen observations. Do not rely solely on the final screen. Run focused tests plus the existing relevant movement, admission, runtime-index, publication, and reference tests. Exercise both publication feature settings where relevant. Known unrelated divergences must be recorded rather than attributed to a candidate automatically.

Instrumentation builds should count calls, work sizes, cache hits/misses, and skipped calculations. Collect counters per runtime and print outside timed loops. Keep counters out of the final measured build. If hardware counters are available, collect cycles, retired instructions, branch misses, and cache misses; otherwise describe locality improvements as hypotheses.

Accept a candidate when correctness passes and the speedup exceeds observed measurement noise without an unexplained material regression on another workload. Record actual memory and compile-time costs. Avoid imposing a universal percentage threshold before measuring baseline variance.

## 1. Flatten movement metadata and improve layout

### Existing path

`run_move_steps` follows `token_movement_groups[token] -> (group, axis) -> movement_groups[group].axes[axis] -> arrive/depart[state] -> movement_flips`. It computes `(cell + 1) * stride` for each affected group. Flips then read source output and mutate contribution/input state. The latter work remains necessary even if lookup overhead is removed.

### Data model

Prepare immutable records during finalization:

```rust
struct TokenMovementPlan {
    cells: Vec<CellMovementPlan>,
}
struct CellMovementPlan {
    groups: Vec<GroupTransition>,
    steps: [Range<usize>; 2], // departure, arrival
}
struct GroupTransition {
    group: usize,
    delta: usize,
    departure_ranges: Range<usize>, // slice in a flat range table
    arrival_ranges: Range<usize>,
}
// Each range-table element is a Range<usize> into movement_flips.
```

This is a conceptual layout. First implement a flat group-record buffer plus per-token/per-cell ranges, avoiding one allocation per record. Copying state-range tables per cell may waste memory: share each axis's tables and duplicate only the small group/delta/reference record. Keep `usize` initially; consider narrower indexes only after measuring record sizes and validating checked conversions. Do not pack records with unaligned fields.

### Algorithm

1. After movement-flip construction, walk tokens and legal cells in existing order.
2. For each existing `(group, axis)` association, copy the group ID, precompute `delta = (cell + 1) * stride` with checked arithmetic, and store references to the axis's flat range tables.
3. Copy the existing departure/arrival step ranges into the cell plan.
4. At runtime, fetch the cell plan. For each group record, read `before`, compute `after = before +/- delta`, update `group_arrangements[group]`, and select `state = after` for arrival or `before` for departure.
5. Select the existing flip range for that state and execute flips in precisely the existing order. Continue pre-refresh, ordered callbacks, remaining switched nodes, and changed-wire watchers in their current sequence.

Validate every generated reference and state bound during compilation. Preserve runtime failure handling for invalid transitions. Remove old redundant metadata only after all callers, validation, tests, and memory diagnostics use the new representation.

Start with an array of compact records because the loop consumes their fields together. Do not introduce structure-of-arrays storage until counters show a loop that consumes only selected fields. Preclassify constant source output where valid, but never substitute a constant for a component whose output can change during callbacks.

### Validation and measurement

Assert old and new plan expansion produce identical ordered flips for every legal group state, cell, and direction. Test empty ranges and joint token arrangements. Measure movement lookup time, instructions per transition, compile time, metadata bytes, and total kernel TPS. Distinguish lookup savings from contribution-update costs.

### Files

`sampled/program.rs`, movement-flip preparation and validation, `sampled/executor.rs`, compiler finalization, runtime-index/movement tests, and memory diagnostics in `sampled.rs`.

## 2. Maintain direct cell occupancy

### Data model

Add `occupied_token: Vec<Option<NonZeroUsize>>` to `Runtime`, parallel to `projection_positions`. Store token ID plus one, using the existing compact-index convention. Keep `token_positions` initially for existing callers and diagnostics. Added storage is one optional index per projection cell, not per world block.

### Algorithm

1. During runtime preparation, initialize all cells empty and insert each initially present token using the existing projection index. Validate candidate membership and occupancy invariants.
2. For departure in `set_token_pos`, validate the old cell, clear occupancy, update `token_positions`, mark dirty, and then execute departure steps. Callbacks must observe the token already absent, matching current behavior.
3. For arrival, validate destination, set occupancy and `token_positions`, mark dirty, and then execute arrival steps. Callbacks must observe the token already present.
4. Implement `token_at(cell)` as a direct indexed read and decode.
5. Route every token-position mutation, including preparation and motion completion, through the same maintenance rule. Audit all assignments rather than updating only the common path.

The current lookup returns the first matching candidate. Before replacing it, establish whether multiple tokens can occupy one cell in any admitted state. If they can, a single-slot table changes semantics: retain a candidate-scan fallback for ambiguous cells or use a representation that preserves candidate precedence. Reject new states only when existing admission rules already reject them.

### Validation and measurement

In diagnostic builds compare direct lookup with the original scan after every arrival/departure and during projection. Cover opposed pistons, interrupted motion, empty tokens, departure-before-arrival, and failure paths. Count candidate lengths and query/update ratios first. Measure the added table's initialization and memory costs against lookup savings.

### Files

`sampled/executor.rs`, runtime construction in `sampled/program.rs`, projection/movement tests, and memory diagnostics.

## 3. Cache wire input strength and suppress redundant refresh work

### Existing behavior

`InputState` already maintains active counts and analog histograms. Analog maximum lookup is cheap. Non-analog `input_strength` can scan incoming and arrangement links. `run_move_steps` can refresh a wire before callbacks and revisit it through `mark_changed_wire` or remaining switched-node processing. A second refresh is redundant only if no relevant contribution changed between visits.

### Instrument before implementing

Count `input_strength` calls by target type, incoming-link visits, arrangement-link visits, repeated reads without contribution changes, refresh calls with unchanged power, and callbacks that occur between repeated reads. Count histogram updates separately. `InputState::change` already skips `old == new`; another identical check inside that function adds no benefit.

### Data model and algorithm

Prefer explicit invalidation over TTL or a hash map:

```rust
struct CachedStrength {
    value: u8,
    valid: bool,
}
// Runtime: one cache entry per eligible wire/component.
```

1. Initialize cache entries invalid after runtime input construction.
2. In every effective primary-contribution mutation, compare old/new values. If they differ, invalidate the target cache. This includes fixed and arrangement contributions, initialization/rebuild paths, and any direct input mutation discovered by the audit.
3. On a cached strength read, return `value` when valid; otherwise compute through the original algorithm, save the result, and mark valid.
4. Keep cache lookup behind a mutable helper for runtime hot paths; retain an uncached immutable calculation for diagnostics/reference comparison. Do not add interior mutability merely to preserve an incidental helper signature.
5. In `refresh_wire_power`, return before constructing/writing a block when its current power equals the requested strength, after verifying the original equal-strength path has no required side effects.
6. Preserve `mark_changed_wire`, watcher delivery, operation budgets, and callback order. An unchanged numeric strength does not authorize skipping a callback.

If analog reads dominate, an additional strength cache may cost more than the existing histogram lookup. Restrict eligibility or abandon the cache based on measurements. As an alternative experiment, preindex each target's arrangement links so the uncached fallback stops scanning the entire arrangement-link array; measure this independently from cache reuse.

Do not use only the current phase ID as a cache key: contributions can change repeatedly within one phase. A revision design is also valid, but every mutation must advance the revision, and wraparound must invalidate caches. Boolean invalidation avoids that extra state.

### Validation and measurement

Compare every cached result against the original calculation in diagnostic runs. Cover equal maxima from competing sources, attenuation changes that preserve zero/nonzero status, analog strength changes, conditional-link activation, source changes during movement callbacks, and repeated reads within one phase. Report hit ratio, scans avoided, cache bytes, and total TPS. Reuse existing exhaustive `InputState` tests and add targeted cache invalidation tests.

### Files

`sampled/executor.rs`, runtime construction, input/runtime-index tests, and potentially finalized per-target arrangement indexes in `sampled/program.rs`.

## 4. Reuse movement-plan selection at runtime

### Scope

Existing `arrive[state]` and `depart[state]` tables already cache transition selection. Method 1 should normally deliver the desired reuse by making those tables cheaper to reach. Do not add a hash cache over an existing indexed lookup.

If measurements still show repeated plan selection as material, use a per-group-axis last-selection record:

```rust
struct LastTransition {
    state: usize,
    arriving: bool,
    flips: Range<usize>,
    valid: bool,
}
```

Store one record per immutable group-axis association; cell-specific deltas still come from the cell plan. If transition-range selection has additional cell dependence after refactoring, include the cell in the key. Match state and direction before reuse; misses read the immutable table and replace the record. Update arrangement state on every transition, including hits. Cache only the ordered range, never signal strengths, callback effects, pending work, or resulting world state.

Benchmark against direct indexing. A key comparison and extra memory access may be slower than reading the existing table. Keep this only if the profile demonstrates a benefit after method 1; otherwise omit it entirely.

## 5. Batch independent contribution arithmetic and evaluate SIMD

### Preconditions

SIMD is a later experiment. The current loop combines source gathers, branches, scattered input updates, histogram maintenance, and ordered callbacks. Those operations are poor SIMD targets. First count flip-run lengths and determine whether enough independent arithmetic exists to amortize batch setup.

### Data model

During compilation, partition ordered flips into contiguous batches whose source reads can safely occur together. Describe batches with a range plus parallel arrays for source descriptors, attenuation, entering flags, destination contribution indexes, target indexes, and side flags. Constant token outputs can be stored directly. Retain the original scalar records for the reference implementation until validation completes.

### Algorithm

1. Prove the batching boundary: reading any batch source early must yield the same value as reading it at its original flip position. If contribution updates or other side effects can change a later source read, terminate the batch before that dependency.
2. Gather current source strengths into a small reusable scratch buffer without allocating in the hot loop.
3. Calculate entering contribution values using unsigned saturating subtraction; departing values are zero. Start with scalar code over contiguous arrays and inspect whether the compiler vectorizes it.
4. Replay contribution writes, input updates, phase invalidations, switched-node insertion, and operation accounting in original order. SIMD must not reorder callbacks or observable mutation.
5. For sufficiently long proven-independent batches, compare scalar and architecture-specific SIMD implementations. Process the tail scalarly. Keep a portable fallback and gate CPU-specific instructions using the project's deployment/build policy or runtime feature detection.

Select SIMD width and thresholds from measured batch sizes. Explicit x86 intrinsics require narrowly scoped unsafe code with documented bounds, alignment/load rules, and CPU-feature preconditions. Do not introduce a new SIMD dependency for this experiment. Do not depend on nightly portable SIMD unless the project deliberately adopts nightly tooling.

### Validation and measurement

Compare calculated values for all source strengths and attenuations in 0–15, both directions, all tail lengths, and empty batches. Compare full ordered execution traces against the scalar implementation. Measure gather, arithmetic, replay, and total kernel costs separately. SIMD is successful only when total runtime improves after gather and replay overhead. Record CPU architecture and enabled features with results.

## 6. Small independent experiment: defer diagnostic position loads

Change the operation-budget helper to accept a component index at callers that currently evaluate `program.positions[node]` solely for error reporting. Check existing failure and budget first, then load the position only when constructing a new limit error. Preserve the operation limit, increment points, failure position, and failure propagation. Keep a position-taking path where callers naturally have a position rather than a component.

Search every caller before editing. Add a targeted limit-boundary check that verifies the same failing component and count. Measure this patch separately before combining it with movement-layout changes. Source-attributed samples suggest an opportunity but do not establish how much of the load cost survives optimization.

## Implementation order and deliverables

1. Preserve the baseline, add diagnostic counters, and establish intermediate-state comparison for the kernel workload.
2. Measure the deferred diagnostic-load experiment.
3. Implement and measure flattened movement metadata.
4. Implement and measure direct occupancy after validating uniqueness/precedence.
5. Use counters to choose between strength caching, per-target arrangement indexes, or no additional cache.
6. Evaluate extra movement-selection caching only if direct tables remain costly.
7. Attempt SIMD only when independent batch lengths and arithmetic cost justify it.
8. Combine individually successful changes, rerun correctness and integrated performance, and record interactions between them.

Each retained change should include its algorithm, actual data sizes, correctness results, paired benchmark results, and remaining limitations. Remove unsuccessful experiments and temporary instrumentation from the production path. No whole-move memoization is planned: its key would need to cover signals, scheduled work, motion state, and callback suppression, and no useful reuse rate has been demonstrated.

## Experiment record: flattened movement metadata (2026-11-01)

**Disposition:** retained experimentally at the user's request; ZCore showed no convincing speedup and metadata grew substantially. The FPU timings below are invalidated by profiler trace activity during the runs. The candidate and measurement artifacts are retained under `target/kernel-plan-trials/` (ignored build output; not a durable archive).

The experiment built per-token/per-cell ranges into a flat transition buffer. Each transition stored the group, precomputed cell delta, offsets into duplicated flat departure/arrival range tables, and state count. The runtime selected the state-indexed flip range and preserved the existing flip and callback order. Compilation validated each flattened transition against the original group/axis ranges. The original group tables remained allocated for validation and initialization; movement callback step ranges were not flattened in this candidate. A corrupted-transition validation test was added experimentally and passed.

Build: `cargo build -p mchprs_core --profile profiling --locked --features redpiler-full-publication --example bench_cpus`; rustc 1.98.1; revision `f7c511314e0408814808bf296bc7debbbc4ae9f7` with the existing dirty benchmark patch. Seven alternating baseline/candidate ZCore trials each measured 4,096 continuous kernel ticks, excluding native execution, compilation and publication. Baseline TPS: 2795.1, 2797.9, 2684.7, 2676.8, 2767.3, 2504.2, 2402.0 (median 2684.7; range 2402.0–2797.9). Candidate TPS: 2530.3, 2690.3, 2616.1, 2815.0, 3017.9, 2478.5, 2714.4 (median 2690.3; range 2478.5–3017.9). Median per-pair change was -1.0%; individual paired changes ranged from -9.5% to +13.0%. Median compile times were 13.29 s baseline and 13.87 s candidate. This spread does not establish a speedup or its cause.

For the ZCore fixture, opt-in capacity diagnostics measured 6,937,880 bytes and 95,734 nonempty vector allocations in the original movement groups and token associations, plus 7,687,952 bytes and 13,172 allocations for the candidate's flattened buffers. The latter had 62,852 transition records of 40 bytes and 200,700 duplicated ranges of 16 bytes (capacities 65,536 and 262,144 respectively). Counts exclude allocator overhead and are not process-wide retained memory. The old and new tables were both retained in this experiment.

The sampled-runtime suite reported 92 passing and 6 ignored tests; the targeted corrupted-plan validation test passed. Every ZCore run matched the native final reset, but intermediate projected blocks, ordered pending work, callbacks and motions were not compared. A candidate CPU profile could not be collected: Windows Performance Recorder failed with `0xc5585011` when enabling the system-performance profiling policy. Thus extra metadata traffic and locality remain unverified hypotheses, not established causes. Hardware counters and integrated performance were not collected.

FPU follow-up (timings invalid): seven alternating pairs, each 10,000 ticks, using `FPU_27.schem` and the same profiling build settings. An aborted profiler check left a trace growing during these runs until the disk filled; its I/O could have affected throughput. The trace was removed afterwards. The existing FPU benchmark was temporarily changed to repeat its four deterministic masks every 128 ticks. Tick-only timing sums individual `tick()` durations while publishing and observing the output between ticks; it is not a continuous kernel-only measurement. Baseline tick-only TPS: 2978.4, 2904.2, 3175.4, 3254.4, 2199.0, 3041.5, 2843.5 (median 2978.4); candidate: 3047.8, 3214.9, 3297.7, 3306.3, 3438.9, 2548.1, 3179.8 (median 3214.9). Median paired change was +3.9%, range -16.2% to +56.4%. Median tick-and-publication throughput was 729.0 TPS baseline versus 764.0 candidate; median compilation was 9.57 s for both. Every run's 10,000 observed outputs matched native, and baseline and candidate output traces matched each other. This does not compare pending work, callbacks or intermediate motion state. The temporary benchmark patch and raw results are in `target/kernel-plan-trials/`.

Clean FPU repeat after removing the trace: seven alternating pairs of 10,000 ticks with the same fixture and input sequence. Baseline tick-only TPS: 2917.5, 3210.9, 3209.7, 3179.4, 3199.1, 3219.3, 3190.8 (median 3199.1). Candidate: 3490.4, 3105.9, 3389.6, 3377.0, 3334.4, 3309.2, 3243.0 (median 3334.4). Median paired change was +4.2%, range -3.3% to +19.6%. Median tick-and-publication throughput was 771.1 TPS baseline versus 802.3 candidate; median compile time was 9.11 s versus 9.18 s. All runs matched native sampled outputs, and baseline and candidate traces matched. These binaries contain only the movement experiment, not the later SmallVec changes. Raw results are `target/kernel-plan-trials/fpu-clean-*.json`.

The user elected to retain the layout experimentally. Compare complete intermediate state and investigate the metadata cost before treating it as fully validated. The separate deferred diagnostic-load and direct occupancy experiments remain unmeasured.

## Experiment record: inline small projection rows (2026-11-01)

Converted `cell_tokens`, `cell_pistons`, `cell_observers`, and `home_watchers` to `SmallVec<[usize; 2]>`, preserving insertion order and heap spill for larger rows. Existing memory diagnostics now count only spilled payloads. On ZCore the four families used 11,304,384 bytes of retained vector buffers before and 8,368,032 bytes after, saving 2,936,352 bytes; there were no spilled rows in this fixture. These figures exclude allocator overhead. The benchmark's final reset matched native; with each publication feature setting, 93 sampled-runtime tests passed and six were ignored. An added test verifies order after a row spills. Throughput impact of the SmallVec change has not been measured in paired trials.

## Experiment record: deferred diagnostic-position load (2026-11-01)

Retained at the user's request. Component-indexed operation-budget callers now defer loading the block position until the guard constructs an operation-limit error. Position-taking callers retain their existing interface; both use the same guard and operation limit. The focused limit-boundary test passed, including the operation count and reported position.

Four of seven planned alternating pairs completed before the user stopped the trials. ZCore baseline TPS: 2465.7, 2650.2, 2900.4, 2805.7; candidate: 2876.4, 3174.4, 3045.8, 2817.9. Median per-pair change: +10.8% (range +0.4% to +19.8%). FPU baseline tick-only TPS: 3492.6, 3003.8, 3453.1, 3571.8; candidate: 3077.1, 3321.9, 3047.4, 3656.5. Median per-pair change: -4.7% (range -11.9% to +10.6%). FPU measured 10,000 ticks with publication and sampling between ticks; ZCore measured 4,096 continuous kernel ticks. Every completed ZCore run matched native at final reset, and all FPU output observations matched native. These incomplete, variable measurements do not establish a reliable overall gain; complete intermediate runtime state was not compared.

Both builds used the retained movement and SmallVec changes and the same profiling settings; the source patches, executable hashes, compiler version, and raw paired results are in `target/diagnostic-load-trials/` (ignored build output). Direct occupancy was not attempted.

## Experiment record: wire input strength cache

Reverted at the user's request after two paired trials. The experiment cached wire input strengths in one byte per component, invalidated entries when effective fixed or primary arrangement contributions changed, and returned early from unchanged-power wire refreshes. Focused cache and movement-order tests passed. The two changes were measured together.

ZCore baseline/candidate TPS were 2986.4/2396.5 and 2888.6/2846.9 (-19.8% and -1.4%). FPU baseline/candidate tick-only TPS were 3188.9/2763.5 and 2815.7/3272.8 (-13.3% and +16.2%). ZCore used 4,096 continuous kernel ticks; FPU used 10,000 ticks with publication between ticks. Completed ZCore runs matched native final reset, and FPU runs matched native sampled outputs. Two pairs are insufficient for a reliable performance conclusion. Raw results and build provenance are in `target/wire-cache-trials/`; cache hit rates and full intermediate runtime state were not measured.
