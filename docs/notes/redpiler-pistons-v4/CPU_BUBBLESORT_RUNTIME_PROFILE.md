# CPU_BubbleSort runtime profile

Captured 2026-10-14 from the current uncommitted worktree, with O3 and the wire-transfer-expression draft enabled. This report supersedes the earlier mixed-tier capture for locating current runtime costs. It does not establish an optimization speedup.

This capture predates the [compiled-hop optimization](COMPILED_WIRE_HOPS.md). Its percentages describe that earlier executable; the follow-up document reports the before/after benchmark. A post-hop sampled capture has not been run.

## Main finding

The expensive work is repeatedly constructing views of compiled sites and neighborhoods, not mainly calculating redstone strength. Turbo wire execution accounts for **69.65%** of Graph tick samples, including its nested callbacks. Capturing sites for walks accounts for **29.41%**, and calculating raw wire sides accounts for **22.71%**. Those last two call-stack subtrees do not overlap in this capture: together they account for **52.12%** of tick samples.

The graph is **not rebuilt during execution**. `CircuitProgram` already contains site identities, canonical wire neighborhoods, routes, and guarded contact alternatives. The runtime constructs temporary walk observations and slot mappings, and reconstructs block views to answer shape and power queries. Calling this work "rebuilding wire subtrees" would confuse a profiler call-stack subtree with a graph transformation.

Some observations must remain distinct: a walk can capture a contact before a nested callback changes it, while a nested walk sees the new state. That requirement does not justify repeatedly discovering immutable relationships or resolving unused neighbors.

## Capture and correctness

- Native Windows, AMD Ryzen 9 5950X, 16 cores / 32 logical processors.
- Samply 0.13.1; 1,000 Hz sampling, benchmark main thread.
- Rust 1.98.1, LLVM 22.1.8, `x86_64-pc-windows-msvc`.
- Cargo `profiling` profile: optimized release inheritance, debug symbols retained, no stripping.
- No `cfg(test)` recorder, `graph-trace`, custom runtime timers, or `unsafe-turbo-fixed-point` feature.
- Source HEAD: `10a146ba1517136d18b78a75d63582ec221231a8`; the benchmark JSON records the worktree diff and status. HEAD alone does not identify this source.
- Executable SHA-256: `d0aad7b0c824cb8c9333a68c8015c9ad93def2076b82353dedb8b21c18953a27`. The matching executable and PDB were preserved together.

The existing benchmark ran five fresh 100-active-tick Graph/interpreter pairs. Both inputs used the same approved, temporary sign removal; the schematic on disk was unchanged. All pairs passed the harness's reference comparisons, including RAM, and its reset/continuation validation. Each Graph episode retained these counters:

| Operation | Count per 100 ticks |
|---|---:|
| Scheduled ticks | 20,005 |
| Callbacks | 1,760,070 |
| Wire walks | 32,505 |
| Dust-path power samples elided | 0 |
| Piston samples | 302,784 |
| Accepted piston actions | 27,884 |
| Deferred publications | 27,884 |

Recording covers the entire process, including preparation and interpreter execution. Analysis selects only stacks beneath `GraphBackend::tick_with_world`, `tick_with_host`, or `tick_phases`, including inlined frames. There are **6,169 weighted tick samples** out of 135,422 main-thread sample records. This includes the five measured episodes and one additional first-tick reset/handoff check.

At the nominal 1 ms interval, tick samples represent approximately 6.169 CPU seconds. The five measured Graph windows sum to 6.174201 wall seconds, a useful scope sanity check. These sampled runs are not a replacement for paired, unprofiled speed measurements.

All 11,722 sampled application addresses were resolved using the matching PDB, including inline frames. Initial presymbolication produced generic `fun_...` names; that fallback sidecar was moved aside before explicit PDB symbolication. Some private system-module symbols remain unresolved. Numeric and folded-stack reports use the resolved application frames.

## Where execution time goes

The phase partition is disjoint and sums to 100%. It identifies which phase starts the work, including everything called inside that phase.

| Phase | Samples | Share of tick CPU |
|---|---:|---:|
| Piston action draining | 3,582 | 58.06% |
| Deferred connection publication | 2,266 | 36.73% |
| Scheduled work | 321 | 5.20% |

A piston phase can invoke Turbo, which invokes site resolution and guards. Consequently, the following inclusive operation shares **overlap** and must not be added together or added to the phase table.

| Operation, including descendants | Samples | Share of tick CPU |
|---|---:|---:|
| Turbo wire execution | 4,297 | 69.65% |
| Site / block / property resolution | 2,340 | 37.93% |
| Wire neighborhood identification | 2,235 | 36.23% |
| Wire shape handler | 2,156 | 34.95% |
| Walk site capture | 1,814 | 29.41% |
| Power sampling | 1,811 | 29.36% |
| Raw wire side calculation | 1,401 | 22.71% |
| Piston sampling | 1,371 | 22.22% |
| Hash-table / hash machinery | 1,243 | 20.15% |
| Guard evaluation | 1,133 | 18.37% |
| Wire power queries (`seed_power`, `turbo_wire_power`) | 504 | 8.17% |
| Turbo callback admission | 359 | 5.82% |

The wire-power row includes its source queries, not just max/subtraction arithmetic. Raw side calculation also runs during electrical emission queries, outside shape callbacks. Its intersection with Turbo is 8.07%; the union of Turbo and raw side calculation is therefore 84.29%, not 92.36%.

For another view, assigning each Turbo sample to the first matching operation beneath its outermost Turbo call gives the following **disjoint** partition. Percentages still use all tick samples as the denominator, so the rows sum to Turbo's 69.65%:

| Work within Turbo | Share of all tick CPU |
|---|---:|
| Neighborhood discovery / capture | 37.27% |
| Piston callbacks and their descendants | 12.37% |
| Walk / queue bookkeeping | 8.36% |
| Callback admission | 5.82% |
| Wire power queries | 5.77% |
| Other component callbacks | 0.06% |

The largest application leaf shares are `guard_matches_using` (9.47%), `resolve_site` (7.63%), `resolve_site_view` (5.48%), and `node_properties` (3.89%). Leaf attribution depends on inlining; it is not a promise of equal savings from changing a function.

## Source-confirmed repeated work

These are implementation observations, not measured speedups. Production code was not changed for this capture.

1. **Shape calculation resolves 24 sites but consumes only 13.** `raw_wire_sides` in `backend/graph/wire.rs` maps every `canonical_neighbors` entry through `site_block`. `redstone/wire/mod.rs::get_raw_sides_from_neighbors` only uses the upper neighbor and four horizontal/above/below triples: indices `3, 4, 20, 17, 5, 21, 18, 1, 13, 12, 0, 8, 7`. Eleven block views are unused. The remaining reads can also be conditional on the shape rules.
2. **A cached heading still reconstructs the slot array.** `identify_wire_neighbors` caches the heading in a walk, but revisiting that wire maps all 24 compiled neighbor IDs through `walk.site_slots` again. Neighbor-to-slot relationships are stable within that walk and could be retained with its heading. Nested walks must own independent captured state.
3. **An ordinary wire `Shape(Top)` computes geometry before returning unchanged.** `update_wire_shape` constructs raw sides before `on_neighbor_changed_from` applies its unconditional Top no-op. An early return can avoid that work. A removal pass must distinguish ordinary Shape deliveries from Turbo candidate rows: Turbo can use a stored Shape row to admit an ordinary update, so deleting those rows indiscriminately is incorrect. The dynamic frequency of this case was not measured.
4. **A component can reconstruct its properties twice.** `resolve_site` obtains `node_block(node)`, which computes `node_properties(node)`, then computes `node_properties(node)` again for the resolved record. Reusing the first result avoids repeated state dispatch. These are compact property values; this path does not clone full command entities.
5. **Power-target ranges still require a hash lookup.** `CircuitProgram::power_links_to` uses a `PowerTarget` map. Charging standard-library descendants to their nearest Redpiler caller attributes 3.70% to this lookup. Direct plan bindings are worth considering after the larger neighborhood costs.

## Recommended implementation order

1. Remove the definite redundant runtime work above in small changes, preserving the existing update sequence. Compare old/new results for all four headings, live and captured operands, nested walks, contact changes, and reset/continuation. Measure each change separately with the existing unprofiled harness.
2. Compile wire shape and emission queries more directly. Fold immutable profile/orientation facts and retain only genuine contact/state dependencies. Prove ordinary shape identities where possible. Initial/manual dot-versus-cross behavior and every admitted contact alternative must remain correct; moving redstone blocks can change connectivity.
3. Cache or prebind walk-local neighborhood relationships without globally caching captured state. Do not reuse one tick-wide snapshot: nested callbacks can change the relevant state in the same tick.
4. Continue certified instant-region contraction to remove internal walks, callbacks, and reset machinery while preserving boundary event order, activation, shared resources, pending work, and continuation. Recognizing an instant piston alone does not eliminate the internal work shown here.

The recent wire-transfer draft targets a query family accounting for only 8.17% inclusive here and leaves the walk/event workload unchanged. Even eliminating that entire family at unchanged surrounding cost would bound the speedup near 1.09x. Its lack of a demonstrated CPU gain is consistent with this capture. The larger opportunity is to stop reconstructing and visiting internal structure, not to keep simplifying strength arithmetic.

## Artifacts and reproduction

Ignored local artifacts are in `target/profiles/cpu-current-o3/`:

- `cpu-o3.json.gz`: standard Samply / Firefox profile.
- `cpus-b8f3588b7a903875.exe` and `.pdb`: matching optimized binary and symbols.
- `episode.json`: benchmark checks, samples, binary hash, source diff/status.
- `main-symbols.json`, `external-symbols.json`: symbolication responses.
- `tick-hotspots.json`: counts, inclusive groups, leaf/owner attribution, source locations.
- `ticks.folded`: tick-filtered stacks for flamegraph tools.
- `capture.log`, `analysis.log`: recording and analysis output.
- `cpu-o3.kernel.etl`: original ETL, approximately 3.62 GiB **on disk**, not measured runtime RAM.

The one-off Python scripts alongside these files postprocess Samply's existing profile and symbolication API. They are not a runtime instrumentation framework or a new project dependency. Build, record, and inspect instructions are in [RUNTIME_BENCHMARK_AND_PROFILING_PLAN.md](../../../RUNTIME_BENCHMARK_AND_PROFILING_PLAN.md).

Limitations: this is a sampled CPU profile of a 100-tick prefix repeated five times, not a full BubbleSort or arbitrary-history equivalence proof. It does not measure hardware cache misses, allocation counts, or peak RSS. No new speedup is claimed. Some system symbols remain private; all sampled application addresses were resolved. Rust tests were not rerun for this documentation-only profiling work.
