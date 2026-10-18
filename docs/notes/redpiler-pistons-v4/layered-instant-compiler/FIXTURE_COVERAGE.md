# Step 01 fixture coverage

This review inventory distinguishes raw source smoke, declared native protocols, and
functional comparisons. It describes tests of the reference contract; the new boundary
compiler is not enabled and no kernels or speedups are implied.

The checkout contains **96 different `.schem` sources** under `test_data`. The recursive
`available_schematic_sources_have_bounded_native_entry_smoke` test inventories every one,
including intentional invalid imports. It runs two idle ticks, toggles the first saved
lever when present, and runs eight response ticks. This is deliberately **not** an invented
arithmetic or CPU workload. Its printed source hash identifies the loaded bytes; pinned
protocol tests additionally check their manifest hashes.

| Source group | Files | Deeper contract coverage |
| --- | ---: | --- |
| `test_data/*.schem` | 14 | Entry smoke; existing component/CPU harnesses remain separate from this contract gate |
| `instant-pistons/*.schem` | 22 | 138 declared cases, 306 case/rotation histories; the two sorter manifests have no cases in this pack |
| `instant-pistons-io/*.schem` | 22 | 140 declared cases; logical port/action mapping, final declared observations, native drain census; 203 histories including declared rotations |
| `instant-pistons-io/rejected-imports/*.schem` | 3 | Raw native smoke only; these are not Graph-admission successes |
| `graph-fixtures/*.schem` | 11 | Raw native smoke; graph export/static validation remains a separate test family |
| `piston-research/test-prefix-20261008/*.schem` | 18 | All 18 admit and match native across 27 independent-control histories; full projected future and ordered-operation comparisons use the existing TEST-family replay |
| Research manifests: CPU, Divider, FPU, RILAX memory | 4 | 39 declared histories: 36 bounded native histories plus three opt-in full CPU executions; FPU has no declared stimulus cases, so only import/observation readability is claimed |
| Revised Potados PC counter | 1 | Independent controls, stable data buses, native/Graph state and pending work, and the frozen 4096-tick count reference |
| Fresh PM1 compilation schematic | 1 | Raw native smoke; no new full-sort oracle in this batch |
| **Total** | **96** | A source appearing in an inventory is not a functional-equivalence claim |

## Stronger decoded checks

- ADDER1BIT: all eight arithmetic vectors, each with all six physical data-preparation
  orders; the known `prepared-0-1-1` history also compares ordinary Graph with native
  ordered observations and projected future.
- ADDER11BIT: all 40 pinned arithmetic response windows in the native census; this
  does not yet execute a new compiled layered kernel.
- Revised PC: all-controls-enabled output words checked against the frozen native
  reference; separate rising/falling edges for all 23 controls, plus seven independent
  controls under two stable 16-bit bus patterns, compare Graph with native.
- RILAX memory: reuse the existing data-without-update, addressed write/read, held-read,
  short-pulse and reuse tests. Memory must remain ordinary state, not instant logic.
- Primitive regressions: reuse the native chain FIFO, shared-resource OR, independent
  BUD data/update, head observer, cancellation and pulse tests rather than synthesizing
  Boolean expectations for fixtures whose declared contract is an observed episode.

The TEST-family loader supplies its existing documented extra controls and floor supports
to both worlds. Those are **derived** fixture worlds, distinct from the raw source smoke.
All five WEIRD examples also passed the bounded independent-control replay. This does not
certify them for future instant contraction. Do not turn an unsupported compiler candidate
into a pass by discarding a failing observation or silently skipping its admission error.

## Input/output interpretation

ADDER1BIT has three data inputs plus one activation control. Its three declared observation
sites observe Sum twice and Carry once. ADDER11BIT has two 11-bit data buses plus a control;
its two 11-site output groups observe the same Sum bus. The manifests do not declare an
external carry-in or carry-out for that eleven-bit fixture.

Both Sum observation bindings remain checked, including their different valid timing
windows. Sharing an arithmetic value is not permission to merge timed output ports or
remove an ordinary repeater's scheduled response.

The revised PC has 16 data controls plus seven physically separate controls. A printed sign
is evidence of the author's label, not proof of the adjoining control's connectivity. The
new boundary pass must derive separate input/output dependencies, including QC and updates,
without requiring one common trigger. It may preserve a larger shared region instead of
forcing separate read/update regions when ownership and effects require that.

The historical six-piston minimal PC schematic is absent; its preserved report is a design
regression requirement, not fresh execution evidence. See the Step 01 notes for its path.

## Execution-context percentages

Compilation now reports structural execution ownership through
`CompileStatistics.execution_coverage` and its existing `summary_lines()` output. Thus
the ordinary compile/diagnostics UI gets the same summary without a separate renderer.
`Compiler::graph_execution_coverage()` provides a cold, current snapshot after execution.
No extra timers, event counters or scans are added to ticking.

The categories are disjoint **component-node ownership**, excluding geometry-only contact
nodes: whole instant kernels, active compiled classic wire regions, remaining general
Graph nodes and native fallback. They sum to 100% for a nonempty circuit. A separate
wire-only percentage is useful because classic compilation owns wires while ordinary
receivers continue in Graph. Deoptimized classic wires return to general Graph ownership.

The new layered compiler still owns **zero executing kernel nodes**. Recognized actors,
private piston occupancy checks, reset-transfer entries and instruction routes are reported
separately as candidates or partial shortcuts; they must not be counted as eliminated
instant circuitry. Both payload and repeater observation windows still apply.

Compiled execution is Graph-only, so its native fallback share is **0%**. A standalone
interpreter oracle run is **100% native reference execution**, not evidence of a fallback
inside Graph. Post-reset interpreter continuation is outside the benchmark's compiled
execution window.

The short benchmark JSON includes `coverage_at_compile`, `coverage_after_window` and
`execution_context_summary`, plus the existing classic invocation/receiver work counters.
Coverage is a structural snapshot, **not CPU time or the fraction of callbacks saved**.
Nested work counts cannot be added to obtain a CPU-time percentage. The snapshot after
the timed window can differ from compilation when a classic net has deoptimized.
`timed_tick_contexts` separately counts Graph/native tick calls outside the timers. An
explicit reset-pending episode can contain both contexts; it is labelled as a handoff,
not as native fallback or a wholly interpreted reference run. Setup ticks and the final
32-tick interpreter continuation are excluded from this window breakdown.

The fresh O3 census covers all 22 IO fixtures. This census uses the default
`classic_redstone: false`; it does not imply that the optional classic compiler cannot
cover these circuits. Every fixture currently reports 0% whole instant kernels,
100% general Graph component ownership and 0% native fallback. Partial shortcuts overlap
that Graph ownership:

| Circuit | Component nodes | Wires | Stationary candidate actors | Private occupancy checks |
| --- | ---: | ---: | ---: | ---: |
| ADDER1BIT | 71 | 32 | 13 / 13 (100%) | 4 / 13 (30.8%) |
| ADDER11BIT | 690 | 295 | 142 / 142 (100%) | 43 / 142 (30.3%) |
| Revised PC counter, after 128 ticks | 3,388 | 1,890 | 307 / 500 (61.4%) | 163 / 500 (32.6%) |

The counter's excluded actors include ordinary/BUD or unsupported shared groups;
candidate recognition is not execution replacement. The real classic deoptimization
regression separately compiles a four-wire line: classic ownership is initially 4 / 4
wires (100%), then 0 / 4 after deoptimization, with all six components back in Graph.
The native reference remains a separate run throughout.

## Run record

Completed so far:

- Latest integrated adder/contract/report run: **44 passed, 1 ignored**. Includes all
  nine stationary cell/effect tests, all three coverage tests, the 22-fixture coverage
  census, and existing ordering, arithmetic, continuation and shared-ownership checks.
- The PC 128-tick Graph/native/frozen comparison passed again with the ownership report.
- `cargo check -p mchprs_core --tests --benches --locked`: **passed**, including the
  additive benchmark JSON fields and explicit Graph/native tick-context breakdown.
- `cargo test -p mchprs_core --lib pc_counter_contract:: --locked -- --include-ignored --nocapture --test-threads=1`: **4 passed**. Includes the full native 4096-tick frozen sequence, the 128-tick Graph/native/frozen comparison, 736 independent-control ticks and 672 stable-bus/control ticks.
- `available_schematic_sources_have_bounded_native_entry_smoke`: **passed**, all 96 sources.
- Core-pack native census: **passed**, all 306 declared case/rotation histories.
- IO-pack native census: **passed**, all 203 declared case/rotation histories, with
  input/action mapping and final observation retention. Both packs preserve action kinds
  and ordered drains; neither found a repeated ordinary retraction within one checked drain.
- TEST-family independent-control replay: **passed**, all 18 fixtures, 27 histories and
  864 native/Graph tick pairs; includes the five WEIRD examples.
- `research_manifests_exercise_all_bounded_native_protocols`: **passed**, 36 native
  histories: five CPU, eleven Divider and twenty RILAX-memory histories. The FPU's empty
  case list is reported and its declared observation groups remain readable.
- Contract/cell and existing proxy/recognition filter: **18 passed**. An additional physical
  cell-entry test passed for all eight one-bit vectors and forty eleven-bit histories.
- Existing native instant-piston/reference-contract suite: **36 passed, 2 ignored**.
  Includes arithmetic, separate Counter memory/repeater windows, BUD activation, drops,
  shared resources, callback FIFO and pulse filtering.
- Parser/callback/electrical/geometry audit: **53 passed**, excluding the separately tested
  research family, JSON export snapshots and the unrelated redstone fuzz family.
- `cargo check -p mchprs_core --tests --locked`, focused rustfmt and `git diff --check`:
  **passed**. A transient check failure during concurrent classic-runtime edits cleared
  when that separate change supplied its new deoptimization API; those edits were preserved.

Three full CPU execution protocols
with `wait_quiet` remain in the existing opt-in `capture_bubblesort_execution` harness;
they are not approximated or relabelled as completed bounded histories. No benchmarks or
kernel speed measurements belong to this step.

The full workspace, release suite, broad fuzz campaigns and three full CPU-sort protocols
were not rerun. The current new boundary compiler/runtime is not active, so this gate
establishes reference expectations and local first-response reachability, not acceleration.

### Step 03 boundary gate, 2026-10-18

The subsequent integrated debug Redpiler suite passed **411 tests, with 18 ignored**;
the core tests/benches compilation check passed. All 22 IO catalogs plus the revised PC
catalog were exported and accepted by both external validators under their default limits.
Python loader tests passed 20/20; viewer tests passed 26/26.

Both adders retain complete actor admission (13/13 and 142/142). The PC admits 307/500
actors and explicitly keeps remaining ordinary/BUD or unsupported groups outside.
The boundary-only pipeline matches native full state and pending work across all eight
one-bit and forty eleven-bit prepared histories, including interpreter handoff. Catalog
interfaces are not executed kernels or external bit counts. See the
[Step 03 report](steps/03-boundary-pass.md) for exact ownership, port counts and artifact sizes.
No new runtime speed claim or full CPU execution follows from this gate.

### Step 04 partial layer gate, 2026-10-18

Connected-region accounting now covers all 22 IO catalogs without omitted or duplicated
cells/corridors. Both adders form one connected component, but neither has a compiled
layer actor: the request/acceptance signal-arrival proof is still missing. All 22
schematic inputs currently produce layer declines. The source witnesses and exact
counts are in the [Step 04 report](steps/04-regions-and-layers.md).

A separate normally parsed, constructed private-cell regression produces one actor in
one layer, predicts the native pull/token vacancy and post-pull state, and checks
ordinary Graph/native state and pending work through 16 ticks. An external payload
observer variant declines while its geometry boundary is unproved. This is real parser
coverage but not an executing kernel. Execution ownership remains 0% new kernels,
100% Graph and 0% native fallback; no speed measurement follows from cold analysis.

All 23 layer exports (22 schematic declines plus the positive private cell) were loaded
by both external validators under default limits. Python tests passed 22/22 and viewer
tests 28/28. The final debug Redpiler suite passed **450 tests, with 19 ignored**;
core tests/benches and server checks passed. See the Step 04 report for commands,
the corrected synthetic test fixture and verification limits.

Reproduce the main filters (add `--nocapture` when a detailed census report is wanted):

```text
cargo test -p mchprs_core --lib redpiler::backend::graph::adder_tests::instant_ --locked -- --test-threads=1
cargo test -p mchprs_core --lib pc_counter_contract:: --locked -- --include-ignored --test-threads=1
cargo test -p mchprs_core --lib all_test_fixtures_preserve_independent_control_histories --locked -- --test-threads=1
cargo test -p mchprs_core --lib available_schematic_sources_have_bounded_native_entry_smoke --locked -- --test-threads=1
cargo test -p mchprs_core --lib research_manifests_exercise_all_bounded_native_protocols --locked -- --test-threads=1
cargo test -p mchprs_core --lib redstone::instant_piston_tests:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib redpiler::analysis::tests:: --locked -- --skip research --skip export_json --skip redstone_fuzz --test-threads=1
```
