# ZCore throughput recovery experiments — 2026-11-03

## Target and priorities

Recover at least 2,500–2,600 Rust Redpiler kernel TPS on the established ZCore workload while preserving the current ordered simulation semantics. Precompute geometry-dependent decisions, destinations, and state transitions. Execution should use dense IDs and direct array/table reads. Memory is secondary to throughput: record its cost, but do not trade repeated searches for smaller tables without measured justification.

This is an experiment scope, not a claim that the target has been recovered. No engine change is included in this document.

## What changed

| Evidence | Finding | Limit |
| --- | --- | --- |
| [Retained paired runs](redpiler-regressions/2026-11-02/evidence/zcore-kernel-pairs.json) | 5,000 active Rust ticks: 2,391.0 / 2,566.5 / 2,502.0 TPS; median 2,502.0. Same fixture hash and 413 preparation ticks as today's harness. | Historical machine conditions/build must be reproduced before using this as a paired baseline. |
| [Rollback record](REDPILER_NATIVE_BACKEND_EXPERIMENT.md#2026-11-02--stopped-experiment-and-checkpoint-decision) and its dated tail paragraphs | Clean `143f1940` reached 2,716.4 TPS in one run and matched all 5,000 interpreter screens. | Wider notification/wire-side regressions remained; rollback alone is not the solution. |
| `git show 0b3d0972` | After `30c87fcd`, adds sampled wire-side shape programs and calls `notify_shape` before ordinary movement callbacks. Previously that method handled observers only. | The added behavior repairs real correctness defects. |
| [Controlled shape-fix comparison](redpiler-regressions/2026-11-02/piston-wire-performance.md) | Rust ZCore kernel time rises 3.833 → 6.685 s for 5,000 ticks (+74.4%). Active counter +59.1%; LLVM ZCore +19.2%. | Development profile with opt-level 1, affinity `0xff`, four Rayon threads. Do not compare these times directly with release/LTO timings. |
| Recent WPR capture, `target/zcore-cpu.etl` | In the 35–62 s steady window, PID 39984 has 26,852 sampled stacks. `set_token_pos` is ~74.4% inclusive (~27.9% self); `notify_shape` ~31.6% inclusive (~22.3% self); hashing beneath it ~6.9%. | 2,562,493 events dropped across the trace. Shares are approximate; they locate candidates, not guaranteed speedups. Optimized/inlined movement work is attributed to `set_token_pos`. |

The WPR run executed 30,000 ticks in 32.105 s (934.4 TPS); its 20.776 s admission and reset were outside kernel timing. The 30,000-tick average is not interchangeable with the historical 5,000-tick prefix. Keep both windows in subsequent trials.

A fresh unprofiled 5,000-tick screening run on 2026-11-03 measured **6.5134 s / 767.65 TPS**, with WPR inactive. Preparation was 413 ticks; admission 24.252 s and reset 464.0 ms were outside timing. Final screen SHA-256 was `563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b`, matching the historical 5,000-tick screen. Result, log and provenance are under `target/zcore-scope-baseline-20261103-133904-6b545535*`. The existing profiling binary SHA-256 is `E47E8A28044D3A51E39EB7ECFE30B01D300B2047FE20AC3929B652EBA5662005`; source HEAD was `431b236b`. No fresh build was performed for this screening run, so binary/source identity must be established for the paired experiments. One run confirms low throughput in this window; it is not a controlled comparison or full-state correctness proof.

The earlier dynamic shape/world-traversal implementation and its fixed-target/geometry-plan trials were rolled back. Their ~398 → 784 → 1,520 TPS sequence is historical, not a description of today's static shape-map implementation. See the final dated paragraphs of [the native backend notes](REDPILER_NATIVE_BACKEND_EXPERIMENT.md).

## Existing static work to reuse

- `shapes.rs` already samples local geometry into `Local<[u8; 256]>`: the state key includes local token arrangement and stored wire sides. Runtime `Local::select` borrows a map; it does not copy 256 bytes per notification.
- `movement_cells`, `movement_transitions`, `movement_ranges`, `movement_flips`, `move_steps`, and `movement_steps` already flatten movement instructions. Extend those plans instead of introducing another movement planner.
- Projection-cell IDs, component IDs, token cell tables, input aggregates, and operation-budget guards already exist.
- `notify_shape(cell)` nevertheless converts the ID back to `BlockPos` and repeatedly hashes that position for each step and callback. Shape/routing selectors repeatedly search token cell positions. These are avoidable runtime decisions.

## Ordered experiments

### 1. Address shape programs directly by projection cell

Replace `HashMap<BlockPos, Vec<ShapeStep>>` in `Program` with a cell-indexed range table and flat ordered step/callback buffers. A runtime notification reads its range once and uses integer indexes, including after callbacks which require a fresh borrow of the runtime.

Represent three cases explicitly: no compiled plan (retain the manual-program observer fallback), a compiled empty plan (return without fallback), and a populated plan. Conflating the first two changes behavior. Preserve step order, duplicate callbacks, budget checks, and failure propagation. Retain map/axis evaluation unchanged in this first experiment so its cost can be measured separately.

Scope: `shapes.rs` sampling, `program.rs` defaults/projection preparation/validation, metadata accounting in `mod.rs`, `executor.rs::notify_shape`, `llvm/shapes.rs` encoding, and shape/publication/manual-program tests. LLVM can keep its existing serialized table format; the exporter must produce identical ordered records. If encoding changes, update the native decoder/version and malformed-table checks too.

Measure: position hashes during shape replay (target zero), step/callback counts (unchanged), runtime allocations, kernel time, compile time and table bytes. Hash removal alone has a theoretical ceiling of about 1.08× using this trace; it cannot recover the full target.

### 2. Maintain direct token digits and shape selector indices

Keep each token's current legal digit (`0 = absent`, otherwise legal-cell index + 1), updating it at the same boundary as `token_positions`. Internal movement already knows the token/cell; carry that ID into the write instead of searching coordinates. Retain existing validation for external/test inputs.

First substitute digit reads for `cells.iter().position(...)` in shape/routing evaluation. Then, as a separately measured increment, compile inverted dependency lists `(token -> affected shape selector, radix stride)` and maintain each selector's mixed-radix index on token departure/arrival. A shape step becomes `map_ids[current_selector]` followed by `maps[map_id][stored_sides]` and the existing ordered callbacks. Initial selectors are computed once from initial token state; token changes update affected selectors before callbacks observe the new geometry.

Do not freeze all shape results at notification entry: earlier callbacks may change live state. Read each step's current selector and stored sides at its original point in the callback sequence. Keep physical token digits distinct from compressed electrical-axis states. Keep invalid overlapping arrangements as invalid table entries. Initialization, reset/snapshot import and diagnostic runtimes must initialize/reconstruct digits and selectors consistently. `llvm/movement.rs` also assigns `runtime.token_positions` during synchronization; it must not leave derived Rust state stale. Intern identical selector-axis layouts during compilation if it reduces duplicate dependency updates.

Scope: Runtime state/preparation, `set_token_pos` and all token mutation callers, `routing::Local` consumers, shape compiler dependencies, and LLVM snapshot synchronization. Reuse immutable Program maps through the existing Arc at execution; no per-tick map cloning or new shared mutable state.

Measure: token-cell comparisons and selector-axis visits removed; dependency writes introduced per movement; selector reads; callback counts and kernel TPS. A large dependency fan-out can outweigh saved selector work, so keep the direct-digit and maintained-selector trials distinct.

### 3. Replace global input scans with compiled per-target link ranges

Implementation audit: normal wire/comparator runtime initialization already assigns histogram channels, and movement's wire reads take that constant-time path. Several nonanalog reads occur only in diagnostic trace code. Consequently this fallback index is deferred until counters show production calls; it should not be assumed to explain ZCore's movement samples. The initial implementation trials prioritize direct shape addressing, physical token digits and maintained shape selectors instead.

`input_strength` scans every `arrangement_link` for each nonanalog input read, filtering for the target and primary side. Compile each target's relevant link IDs once and read only their live contributions. Reuse existing `incoming` for fixed links, optionally preparing effective incoming IDs after link finalization. Leave the analog histogram path unchanged. Preserve original ordering and contribution values.

This removes a concrete repeated whole-program search. Movement can read the same wire's strength before and after callbacks; each read must still see intervening contribution changes. An indexed read avoids the global scan without introducing invalidation or caching. Count nonanalog calls, total links scanned, relevant links read and repeated reads first; if the histogram path dominates, this experiment has limited reach. The fallback can inline into `set_token_pos`, so its time may appear within that function's ~27.9% self samples. This attribution is a hypothesis until measured.

Scope: Program per-target primary arrangement-link ranges, finalization/validation and memory diagnostics, `executor.rs::input_strength`, input/movement/runtime-index tests. Preserve LLVM's representation unless a shared source field changes; check exporter consumers before removing old fields. No wire input-strength cache: that combined cache/early-exit trial was previously reverted after negative/noisy ZCore results. See [the recorded trials](REDPILER_KERNEL_OPTIMIZATION_PLAN.md#experiment-record-wire-input-strength-cache).

Measure: link entries visited per read and per token movement, kernel time and additional index bytes. This isolated experiment precedes any broader movement dispatch specialization.

### 4. Specialize the measured residual movement work

`MoveStepSpec`/movement records already store direct node IDs, wire/watcher flags and watched-wire ranges; flips already name contribution slots and targets. Do not introduce a second recipe format to encode them again. Profile after experiments 1–3 and specialize one remaining hot instruction family. Count flips, effective contribution changes, shape selections, changed/unchanged sides, generic updates by target kind and executed callbacks in a separate diagnostic build; print counters outside timed loops.

Candidates are compiled handler kinds for immutable target classes, comparing packed sides before constructing/unpacking an unchanged wire, and state-specific direct shape-map/callback ranges. Read dynamic source strength, stored wire sides, scheduled state and piston motion at their original point of execution. Preserve the phase order: routing/contribution changes, shape transformations and resulting callbacks, then ordinary movement callbacks and remaining wire notifications. A proven identity map may avoid transformation arithmetic, but must preserve invalid-state errors and operation-budget behavior.

Do not deduplicate callbacks or skip one merely because power/contribution is unchanged. Existing `set_arrangement_contribution` already returns on equal values; adding that check again cannot save work. Remove an instruction only if all observable effects, validation and budget accounting are preserved. Duplicate destinations remain distinct when their sequence matters.

Where profiling supports it, use compiled per-token reader/link ranges and reusable scratch buffers in `routed_values` instead of rebuilding reader/update Vecs on every movement. Direct occupancy is secondary: `token_at` has ~1.7% self samples. Preserve first-candidate precedence for ambiguous cells and maintain occupancy on every departure, arrival and import.

Scope depends on the measured family: `passes/movement.rs`, `shapes.rs`, Program finalization/validation, `executor.rs::run_move_steps`, dispatchers or routing APIs, metadata accounting, and relevant LLVM encoding/import. Preserve backend equivalence and ABI validation if serialized records change.

Measure dispatches/searches/allocations eliminated, work per token movement, kernel time and added static metadata. A lower work count caused by lost notifications is a failure.

## Comparison and correctness gates

1. Preserve named baseline/candidate executables with source revision/hash, executable hash, Rust version, profile/features, affinity, thread settings and fixture hash. Verify the selected native DLL when testing LLVM. Serialize builds, benchmarks and profilers; throughput trials run with WPR stopped.
2. Use `bench_llvm_zcore 5000 1 --rust-only` for the historical prefix, 413 preparation ticks and the same Run input. Run at least seven alternating baseline/candidate pairs once a candidate passes screening. Also compare a longer 30,000-tick window to expose activity-window effects. Keep output JSON/logs under unique names because the harness overwrites its default output.
3. Compare interpreter/Rust screens every tick with `bench_cpus zcore 5000 1 integrated` and full publication, including the previously problematic tick 1,520. Final-screen equality is only a screening check. Compare baseline/candidate projected blocks, wire sides, comparator outputs, ordered pending ticks/events, active motions, command/note effects and reset continuation at tick boundaries on supported workloads.
4. Preserve the current known failure baseline: RedstoneDB Four In A Row differs from the interpreter; frozen `cpus` replay has tick-10 differences for ANPU/BubbleSort, rejects PM1's moving state and lacks ZCore's reference JSON. Investigate/record those separately; never regenerate references or disable assertions to accept a speedup. Client suggestion-count failure is unrelated to kernel work.
5. Focused gates include `piston_payload_landing_updates_wire_shape_in_interpreter_order`, `empty_piston_shape_observers_follow_interpreter_fifo`, `landing_callbacks_preserve_both_motion_orders`, `compressed_electrical_axes_preserve_physical_token_positions`, torch/reactive FIFO, opposed/shared-token motion, interrupted movement, and shape publication/reset tests in all modes. Run Redpiler tests and longer Rust/LLVM comparisons for any shared-plan/import change. Validate both full-publication feature settings where host state is touched.
6. After ZCore, check active counter, 11-bit adder with full publication, ANPU, a wire/analog circuit and a small logic control. Record compile time, retained Program/Runtime bytes, peak process memory and reset/publication time separately from kernel TPS. Memory growth is acceptable when it produces a repeatable TPS gain and stays within available memory; preserve allocation/admission error handling.

## Decision rule

The goal is at least 2,500–2,600 TPS on the comparable 5,000-tick workload, with valid output and no new correctness regression. Use paired kernel time, spread and per-event work counts to decide what to retain. From the current 767.65 TPS screening run, that requires roughly 3.26–3.39× throughput, or a 69–70% reduction in time. Removing all `notify_shape` work alone would still be insufficient, and dropping its required semantics is invalid. Address movement execution as a whole.

Compilation caching, slower publication, map compression, parallel execution, global state cross-products and broad executor rewrites are outside these experiments. Compilation/reset are excluded from the measured regression; parallelism and compact maps add separate design questions before the direct indexing opportunities are exhausted.

## Implementation and rough trials — 2026-11-03

Retained direct cell-indexed shape ranges and cached physical token digits. Shape callbacks use flat step indices; shape/routing selectors read token digits without coordinate searches. LLVM snapshot imports validate token membership and refresh digits before routing. Missing and compiled-empty shape plans remain distinct. Sampling replaces its generated flat plans when repeated.

Also implemented shared, eagerly maintained shape selector indices as a separate variant, then removed that experiment: two samples did not demonstrate an advantage over direct indexing plus digits. The original mixed-radix map selection remains, using direct digit reads. The per-target input scan experiment is deferred because normal wire/comparator reads already use histograms.

Retained [raw trial results and binary hashes](redpiler-regressions/2026-11-03/zcore-index-trials.json). Each process runs 413 preparation ticks followed by 5,000 active ticks using the optimized `profiling` profile. CPU affinity is `0xff`, Rayon threads 4, WPR stopped, and processes serialized with compilation/reset outside kernel timing. First round orders baseline → shape → digits → selectors, second reverses the order, third repeats baseline → digits. These are rough screening samples, not seven-pair acceptance trials.

| Variant | Round 1 TPS | Round 2 TPS | Round 3 TPS | Decision |
| --- | ---: | ---: | ---: | --- |
| Saved baseline | 1,405.0 | 1,231.0 | 1,121.6 | Reference |
| Direct shape indexing | 1,847.2 | 1,323.7 | — | Included in retained change |
| Direct shape indexing + physical digits | 1,791.9 | 1,730.1 | 1,483.1 | Retained |
| Above + eagerly maintained selectors | 1,206.3 | 1,808.5 | — | Removed; no demonstrated benefit |

Retained versus baseline paired throughput increases are approximately 27.5%, 40.5%, and 32.2% (median paired improvement 32.2%). Median kernel time falls from 4.062 s to 2.890 s; the paired percentage and ratio of medians are different statistics. All final screen hashes matched the historical 5,000-tick hash. Variability is substantial, so these measurements do not establish sustained production TPS or recover the 2,500–2,600 target. Initial unpinned samples were even noisier and are excluded from this comparison.

Before removing the selector prototype, 11 focused shape/publication tests passed, and the Redpiler suite produced 194 passed, 18 ignored, and the same existing Four In A Row failure at the same wire/action/tick. The prototype's dedicated test was removed with that prototype. On retained source, the Redpiler suite produced **193 passed, 18 ignored, and that same one existing failure**. The separately rebuilt `bench_cpus zcore 5000 1 integrated`, with `MCHPRS_BENCH_PUBLISH_ALL=1`, passed all 5,000 per-tick interpreter screen comparisons after 413 preparation ticks. This checks observed screens, not every internal queue or state. Logs, JSON and saved executables are under `target/zcore-recovery-trials/`; the replay result is `retained-per-tick.json`.
