# Compiled wire hops

## Scope

The current CPU profile attributes 29.41% of tick samples to walk capture and 36.23% to neighborhood identification, including capture. These overlap. The pass replaces repeated runtime addressing and full block reconstruction in those operations; it does not remove the circuit's semantic event workload.

Every wire already has 24 compiled neighbor SiteIds. A second adjacency graph is unnecessary. `compile-wire-hops`, enabled at O2 and O3, binds the wire's unique component-site root. Missing or ambiguous roots remain general. The existing canonical neighbors and ordered routes become the destinations for compiled execution.

## Representation and execution

`WirePlan::hop_root: Option<SiteId>` selects compiled execution. Validation checks that the site exists, owns this wire component, and is its unique component-site binding. The optional JSON field exposes the same binding; loaders/viewer validate it and accept exports predating the field.

The two lookup paths share the same ordered work executor:

| General execution | Compiled hops |
|---|---|
| Fresh SiteId-to-slot hash map per walk | Reusable direct SiteId-to-slot array |
| Resolve complete block, solid and transparent properties before capturing | Capture only profile, receiver and mutable properties |
| Cached heading still maps 24 neighbor IDs through a hash map | Reuse the walk's discovered 24-slot neighborhood |
| Drop scratch after the walk | Clear discovered entries and return scratch to the backend pool |

Constants retain their profile identity. Fixed components read their existing component state directly. Contacts still select the first live matching variant at discovery and capture its profile, receiver and relevant component/piston properties. Geometry and power queries continue to use their existing live/captured rules.

Dense lookup uses four bytes per program site per concurrent walk. Each nested walk owns a separate record, so a callback cannot overwrite its caller's captured operands or heading metadata. Scratch is allocated lazily, reused, and returned on both success and an execution error. Clearing visits only discovered sites, not all program sites. Observations, queues and cached slot arrays are cleared; only allocation capacity survives.

This does not introduce persistent electrical state, a World lookup, physical Motion, or a separate simulator. O0 remains a useful sparse-addressing reference path. Existing isolated-wire and private two-wire output specializations retain their faster execution where already certified.

## Equivalence argument

Relate two walks when they have the same discovered SiteIds, each with equal captured observations, visited/bias/heading/layer metadata, equal ordered queues, and equal persistent circuit state. Slot numbers need not have semantic meaning, although both implementations insert in the same order.

1. First discovery reads the same source. Direct observation constructs exactly the fields retained by general `resolve_site(...).observed()`. For contacts it evaluates guards in the same order at the same execution point.
2. Later discovery returns the same existing slot. Direct lookup changes addressing, not capture time. Clearing before reuse prevents observations from crossing invocations.
3. A cached neighborhood is immutable within its invocation: it contains references to captured slots, not copies of strengths. Subsequent wire writes still mutate the slot read by later queries.
4. Heading/bias calculations, admission guards, enqueue order, duplicate occurrences, intermediate writes, and callback handlers are unchanged. Equal operands therefore produce equal transitions.
5. Nested execution uses separate scratch and changes only the same authoritative circuit state. Returning preserves the outer observations. The shared executor then resumes in the same queue position.

Induction over discoveries, writes, deliveries and nested invocations gives the same execution transcript and future work. Arbitrary admitted analog values and changing contacts are supported; no steady-state, binary-input, or trigger/reset protocol assumption is introduced.

This is **not** a proof that callback order can be discarded. The known adder fixed-point counterexample still applies. Replacing a complete network by one transfer function remains a separate, stronger optimization.

## Verification and measurements

Implementation and tests are in `passes/compile_wire_hops.rs`, `backend/graph/tests/wire_hops.rs`, the existing native heading/nested-walk/ascent regressions, and the adder contact observation regression. Tests cover all 256 old/new analog strength transitions, 128 deterministic histories on branched cyclic dust, exact ordered traces and intermediate writes, pool reuse, genuine nested execution, every site in varied saved states, changing adder contact alternatives, cancellation, idempotence and malformed roots. Export/loader/viewer tests cover the additive reference and older files.

### Release benchmark, 2026-10-14

The existing `cpus` harness compared two pinned release binaries at O3. There were five alternating before/after pairs each for CPU_BubbleSort, ADDER1BIT and ADDER11BIT, and one smoke pair each for Divider, Counter and ANPU: **18 pairs total**. Every invocation used fresh worlds and an independent interpreter comparison. Setup, compilation, stimulus, flush, reads and continuation checks were outside the tick timer. CPU used the same approved sign-free in-memory copy in both versions; no schematic was changed.

All 18 pairs passed cross-binary checkpoint, observation, pending-cut, reset/continuation and semantic work-count comparisons. The five CPU pairs all favored compiled hops; the CPU timing ranges did not overlap. Results measure the existing short episodes, not full sorting or general server TPS.

| Device | Pairs | Before median, seconds | Compiled median, seconds | Median paired time change |
|---|---:|---:|---:|---:|
| CPU_BubbleSort | 5 | 1.200688 | 0.972016 | -18.66% |
| ADDER1BIT | 5 | 0.012160 | 0.008628 | -29.04% |
| ADDER11BIT | 5 | 0.077884 | 0.061802 | -22.29% |
| Divider | 1 | 0.516812 | 0.465561 | -9.92% |
| Counter | 1 | 1.138605 | 0.714437 | -37.25% |
| ANPU | 1 | 1.109813 | 0.766054 | -30.97% |

CPU before range: 1.172955-1.351784 s; compiled: 0.932618-1.041093 s. Individual paired changes were -18.66%, -15.62%, -18.88%, -15.38%, and -31.01%; the last baseline was a slower outlier. The ratio of CPU medians is a 19.04% time reduction, or approximately 1.24x throughput; it is a different statistic from the median paired change above. ADDER11BIT improved in all five pairs. ADDER1BIT is much smaller and noisier: its ranges overlap and one pair regressed. Single-pair rows establish correctness smoke results, not reliable speed estimates. In the Divider pair, interpreter time also fell by approximately 10%, so this sample does not establish an attributable Divider speedup.

All 41,354 CPU wires received hop bindings. Each 100-tick CPU window retained exactly 20,005 scheduled ticks, 1,760,070 callbacks, 32,505 wire walks, zero dust-path samples elided, 302,784 piston samples, 27,884 accepted actions, and 27,884 deferred publications. This gain comes from less execution overhead, not fewer circuit events.

The CPU wire-plan table estimate grows by 330,832 bytes (0.316 MiB). Each retained dense scratch index for its 522,011 sites needs approximately 1.99 MiB, plus discovered records, cached neighborhoods and queues. Allocation capacity survives in the pool; observations do not. This is a time/memory tradeoff, not a reduction in total retained memory. Peak RSS and maximum CPU nesting depth were not measured.

Artifacts are under `target/compiled-hop-bench/`: individual pair JSON/logs, `summary.json`, and the pinned executables. `run_pairs.py` only invokes the existing harness and compares its existing JSON fields; it adds no runtime instrumentation or dependency. An invocation is:

```powershell
target/compiled-hop-bench/cpus.exe --graph-short --all-devices --cpu cpu_bubblesort --graph-tier 3 --iterations 1 --label hops --output target/compiled-hop-bench/cpu-hops-1.json
```

Baseline executable SHA-256: `f62d930f60d2d163de6d6510a39d638eeaed5f7fb912b57632ab0345ec10944e`. Compiled-hop executable SHA-256: `d821805556a4e778c9105e2b8fc0b2d890a9783701142ef4e1e3fa69e58197e9`. After pinning, subsequent source changes only organized/extended tests and updated comments/documentation; the production behavior being compared was unchanged.

Final verification after the cyclic-history test and test organization:

- Full Redpiler suite in debug and release: **318 passed, 7 ignored** in each build.
- `cargo check -p mchprs_core --tests --benches --locked` and `cargo check -p mroww --locked`: passed.
- Python loader: 11 passed; viewer: 19 passed; all four ADDER1BIT JSON tiers load successfully.
- Focused Rust formatting and `git diff --check`: passed. Unrelated existing workspace formatting was preserved.

Exact broad runs were `cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1` and the same command without `--release`. Logs are `target/compiled-hop-bench/redpiler-release.log` and `redpiler-debug-final.log`; build and consumer logs are in the same directory. No commit was made.

Not checked: complete CPU sorting, broad external fuzzing, peak RSS or a fresh sampled profile of the optimized executable. Whole-network event contraction remains a separate optimization requiring boundary equivalence.
