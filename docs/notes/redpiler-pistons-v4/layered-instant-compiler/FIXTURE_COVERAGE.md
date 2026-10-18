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

## Run record

Completed so far:

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
