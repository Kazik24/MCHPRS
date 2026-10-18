# CPU_BubbleSort runtime profile

Captured 2026-10-14 from the current uncommitted worktree, with O3 and the wire-transfer-expression draft enabled. This report supersedes the earlier mixed-tier capture for locating current runtime costs. It does not establish an optimization speedup.

The initial capture below predates the [compiled-hop optimization](COMPILED_WIRE_HOPS.md).
Its percentages describe that earlier executable. The final section contains a fresh
2026-10-18 full-sort capture with all retained inference optimizations enabled.

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

## 2026-10-18: current full sort with all inference flags

The current Rust inference path still spends most of its CPU on ordered wire
propagation and rebuilding wire geometry. The union of Turbo execution and raw
wire-side calculation is **81.01%** of inference samples, counting their overlap
once. Recursive guard evaluation is the hottest individual application function.

### Build, flags and measurement boundary

Source baseline is `d917e5684ed0ede5d1c21061234b4ffc3a8912a7`
(`Remove LLVM compilation`). The benchmark now accepts O4 and
`--stable-instant-inputs` alongside `--classic-redstone`. The measured settings
are equivalent to `-O4 --stable-instant-inputs --classic-redstone`; `--optimize`
selects the same retained passes with those explicit opt-ins. O4 currently adds
no synthesis passes beyond O3. All 15 registered rewrites/builders are enabled
by this configuration. The harness uses preparation budget multiplier 8.
`--io-only` affects flush and `--update` affects reset, outside inference.

Built the existing `cpu_bubblesort` benchmark with `--profile profiling`, which
inherits release optimization and preserves PDB information. No test recorder,
graph tracing, C/LLVM backend or unsafe fixed-point feature is enabled. Other
concurrent source edits were test-only and do not enter this benchmark binary.
No competing build or benchmark ran during capture.

Pinned executable `cpu_bubblesort-68fb9ace92060bc2.exe`, SHA256
`af0365f639088a75eb69f25b8f5a3545c77bb091cc4f51c5a32fd6f318d1ff7d`.
Samply recorded at 1,000 Hz, main thread only. All 7,812 sampled application
addresses resolved through the matching PDB, including inline frames. Filtering
to `GraphBackend::tick_with_world`, `tick_with_host` or `tick_phases` retains
**80,540 weighted samples**, approximately 80.54 CPU seconds, out of 106,411
main-thread samples. Inference wall time was **80.509886 s**. This agreement
checks the attribution scope; it is a sampled run, not an unprofiled speed claim.

The complete **10,036-tick** active/settling sort matched frozen interpreter RAM
at every tick and ended with sorted RAM. The harness also completed reset and
32 interpreted continuation ticks. This single-run protocol does not establish
a separate complete-state differential continuation proof. World projection
took 8.278880 s and graph preparation 16.444361 s, both excluded from the
inference sample denominator, as are activation, RAM probes and handoff.

Executed work: 196,243,779 callbacks; 3,882,011 wire walks; 35,541,685 piston
samples; 3,295,083 accepted actions; 2,344,242 scheduled ticks. There were zero
classic-net invocations and zero classic receiver operations on this workload.
Enabling the classic option therefore supplied no executed direct-net shortcut
in this sort. The optional pass remains useful only where it admits a region.

### Where current inference CPU goes

These rows are **inclusive**, with descendants, and overlap. Do not add them.

| Operation | Tick CPU share |
| --- | ---: |
| Turbo wire execution, including nested callbacks | 64.37% |
| Wire shape handler | 36.49% |
| Power sampling and dependency evaluation | 33.89% |
| Site/block/property resolution | 28.03% |
| Raw wire-side calculation | 26.47% |
| Piston sampling, including power queries | 26.33% |
| Wire neighborhood identification | 24.17% |
| Guard evaluation | 19.22% |
| Walk site capture | 18.92% |
| Seed/Turbo wire-power queries | 9.80% |
| Turbo callback admission | 8.27% |
| Hash machinery | 6.60% |
| Scheduler access/scheduling helpers | 0.70% |

Raw wire-side calculation intersects Turbo in 9.82% of all samples. Their
union is `(51,841 + 21,316 - 7,911) / 80,540 = 81.01%`.

The following attribution within Turbo is **disjoint**: each sample goes to the
first matching operation below its outermost Turbo call. Values use all tick
samples as denominator and sum to Turbo's 64.37%.

| Work within Turbo | Share of all tick CPU |
| --- | ---: |
| Neighborhood discovery/capture | 24.59% |
| Piston callbacks and descendants | 14.63% |
| Walk/queue bookkeeping | 10.08% |
| Callback admission | 8.27% |
| Wire power queries | 6.68% |
| Other component callbacks | 0.10% |

Hottest individual functions by **self** samples, excluding callees:

| Function | Self CPU share |
| --- | ---: |
| `guard_matches_using` | 11.77% |
| `resolve_site_view` | 6.50% |
| `observe_site` | 5.58% |
| `node_properties` | 4.41% |
| `identify_wire_neighbors` | 3.09% |
| `first_turbo_delivery` | 2.82% |
| `guard_matches` | 2.81% |

Self attribution depends on inlining; it is not a prediction of achievable
savings. Charging library descendants to their nearest Redpiler owner assigns
3.05% to `CircuitProgram::power_links_to`, including its map lookup.

The disjoint initiating-phase split is piston action draining **56.92%**,
deferred connection publication **38.35%**, and scheduled work **4.73%**.
The first two include all wire walks, callbacks and queries they trigger;
these numbers do not mean piston state writes or scheduler data structures
themselves consume that much CPU.

### What the source says to investigate next

1. `raw_wire_sides` still maps all 24 canonical neighbors through `site_block`.
   That resolves live views and contact guards repeatedly. Constant geometry
   and narrowly typed predicates are the first candidates for further proven
   prebinding; dynamic contacts and event-time reads must remain dynamic.
2. `identify_wire_neighbors` still captures and indexes walk-local observations
   and derives headings even with compiled hops. Compiled hops removed some
   representation overhead, but retain this event workload. Neighborhood
   discovery/capture remains the largest disjoint Turbo category.
3. `guard_matches_using` recursively interprets Not/All/Any and resolves Site
   predicates. Reducing repeated guard and site/property work addresses a
   measured hot path. A tick-wide cached value would not preserve nested
   callback observations without a separate validity proof.
4. Certified direct-net operations can remove propagation where admission
   succeeds. This CPU still runs almost 200 million callbacks. The current
   classic option executes no direct-net operations here; broader useful
   coverage requires preserving delivered values, transient samples and order.

Strength-query arithmetic is a smaller target: even the entire seed/Turbo
query subtree is only 9.80%, including its source reads. Scheduler helpers are
also small. This capture does not measure cache misses, allocation counts or
peak RSS, and does not establish the ranking for ANPU or every circuit.

### Artifacts and reproduction

Ignored local artifacts are under `target/profiles/current-all-flags-20261018/`:
the pinned EXE/PDB, `metadata.json`, `source.diff`, `episode.json`,
`cpu-o3.json.gz`, `main-symbols.json`, `external-symbols.json`,
`tick-hotspots.json`, `ticks.folded`, and recording/symbolication/analysis logs.
The reused one-off Python analysis checks its disjoint partitions and minimum
sample count. The metadata retains both artifact hashes and capture-time status.

```powershell
cargo build -p mchprs_core --bench cpu_bubblesort --profile profiling --locked --offline
samply record --save-only --no-open --rate 1000 --main-thread-only --output target/profiles/current-all-flags-20261018/cpu-o3.json.gz -- target/profiles/current-all-flags-20261018/cpu_bubblesort-68fb9ace92060bc2.exe --graph-tier 4 --classic-redstone --stable-instant-inputs --iterations 1 --output target/profiles/current-all-flags-20261018/episode.json
samply load --no-open --symbol-dir target/profiles/current-all-flags-20261018 target/profiles/current-all-flags-20261018/cpu-o3.json.gz
```

Pin the newly built EXE/PDB together before recording; a future build can change
their filenames and hashes. Focused rustfmt and diff checks pass. The full Rust
test suite was not rerun for this benchmark-option/documentation change; the
full-sort RAM oracle ran successfully during the capture.
