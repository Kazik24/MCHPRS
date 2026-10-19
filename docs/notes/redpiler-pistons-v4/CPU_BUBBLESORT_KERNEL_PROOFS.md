# BubbleSort: full-sort baseline and kernel proof cuts

## Concrete compiler limitations, not a noninstant counterexample

The investigation found **no counterexample showing noninstant CPU computation**.
The CPU's computational networks remain expected positive cases under the agreed
stable, synchronous external-input contract. Memory and scheduled interfaces
remain explicit. A declined certificate is not evidence of noninstant behavior.
The diagnostic below runs with `classic_redstone: false`; the historical timing
runs later in this document retain their original flags for reproducibility.

All positions in this section are loaded-world coordinates, including the
fixture loading offset `(2,8,2)`. Exact graph witnesses are saved in
`target/layered-instant-devices/cpu-kernel-proof-cuts.json`.

### Ordinary payload output to dust

Piston 0 at **(176,9,227)** has its recognized own reset. Its redstone payload
at **(176,9,229)** powers the wire at **(176,9,230)**. This is an ordinary
instant output. `single_cell_outputs_supported` in `analysis/instant/layers.rs`
allows a power output only when it matches `local_reset_output_link`.
Consequently, this normal output fails before symbolic output construction.
The existing output compiler has the same narrow capability. Exposing the wire
as a boundary also exposes profile/neighborhood observations the adapter cannot
represent. This is missing output lowering, not a failed timing proof.

### Direct two-instant chain with multiple entry routes

Pistons 739 and 740 at **(213,10,218)** and **(216,10,218)** form a direct
chain: the first payload at **(215,10,218)** supplies the second piston.
Both have recognized own resets. There are four outside activation routes:

| Source piston and position | Route | Phase |
|---|---:|---|
| 727 at (212,10,216) | 100059 | far_changed |
| 727 at (212,10,216) | 100062 | resource_restored_far |
| 738 at (210,10,218) | 100147 | far_changed |
| 738 at (210,10,218) | 100150 | resource_restored_far |

`layers/composition/connected.rs` collects outside activation route IDs and
rejects unless `entries.len() == 1`. It mixes computation and restoration
entry possibilities. Even after separating restoration, two legitimate
computation sources remain. Requiring a common trigger would retain the
underlying limitation; the compiler needs multiple qualifying boundary events.
No wrong output or noninstant behavior was demonstrated by this rejection.

### Two actors sharing one payload

Pistons 6 at **(188,9,231)** and 19 at **(190,9,233)** share resource 7 at
**(188,9,233)**. Their pull positions are **(188,9,232)** and **(189,9,233)**.
The parser correctly represents one cell with two uses and one conserved token.
`compile_component` rejects `cell.uses.len() != 1` before attempting symbolic
ownership arbitration. This is a private-owner template restriction, not a
demonstrated conflicting execution.

The existing composition machinery already has conditional writes and FIFO
processing. Extend that store rather than duplicating the resource. For a
certified A-before-B prefix, the essential dependency is:

```text
take_A = accepted_A && present
present_after_A = present && !take_A
take_B = accepted_B && present_after_A
```

The causal prefix must come from actual execution semantics. Stable external
inputs do not justify choosing an arbitrary winner.

### Own wired observer reset feeding QC

Piston 1728 at **(180,12,160)** has its own recognized reset observer at
**(180,12,161)**. A one-wire corridor at **(180,13,162)** feeds its quasi input
through the compiled conductor relationships. The wire receives conducted
reads through cyan wool at **(180,12,162)** and **(180,13,161)**.
The corridor certificate only supports a private Far-payload producer feeding
a different cell. This input/reset corridor does not fit that template.
It therefore receives `SignalArrivalUnproven`; no nonsettling signal was found.

### Generalization required

1. Identify domains by same-phase computation and real memory/timing barriers.
   Private-cell capability cuts are diagnostic fallback boundaries, not the
   semantic definition of instant circuits. They create artificial wire and
   geometry outputs that a whole-region compiler could absorb.
2. Separate computation entry from reset/restoration events, and support multiple
   qualifying boundary entries. Keep captured requests distinct from later live
   action revalidation.
3. Compile general corridor equations using the existing memoized signal tracer:
   attenuation, conditional contacts, fan-in and fan-out. Certify qualifying
   updates and arrival dependencies separately from settled signal values.
4. Reuse the conditional FIFO resource store for shared tokens, and lower ordinary
   power/update outputs and compact reset obligations. Reduce from these observable
   roots so internal wires and construction observations leave the executable.

The four fragments above should become positive execution regressions as these
capabilities are implemented. Deleting their rejection checks alone is not a fix.
The existing payload-reset and input-restoration contracts also need execution
support: the current compiler requires an own-observer reset before composing
them, despite the broader local admission catalog.

The final no-classic diagnostic passed: one ignored test executed successfully,
covering four catalog views. All views still certified zero executable kernels.
This report establishes concrete compiler gaps; it does not prove instancy for
every possible CPU input history.

## Follow-up implementation

The findings above describe the initial compiler. The follow-up implementation
extends entry and output handling without asserting that the complete CPU is now
an executable kernel:

- Multiple live routes can start the same symbolic program. One actual entry
  route is selected per invocation; a mixed or unmatched multi-actor FIFO retains Graph.
  Construction order between alternative entry routes is not execution order.
- Shared cells use the existing conditional resource store and source-derived
  FIFO. One token remains authoritative; no per-actor payload copies are created.
- Ordinary power outputs have symbolic roots. Geometry readers are recorded as
  `graph_outputs`, retaining the original contacts, live queries and capture times.
  This table is cold inspection evidence, not a replacement for captured values.
- A singleton retains outside handlers in their original route order. Such
  callbacks cannot invalidate a later owned pull because there is no later owned
  pull. It replaces only the already-popped action and leaves unrelated requests
  in order. Multi-actor outside feedback still needs a separate proof.
- Neighboring state is read through `BoundaryState`, bound to a matching original
  input `GuardState` port. Undeclared state reads still decline; the original
  owned-state validation is not relaxed. No outside state is copied or written.
- Input, reset and output wires that do not connect a possible payload producer
  to a Main-input consumer remain outside the wave. They are not erased or
  assumed to have settled values. The complete arrival certificate still runs
  for internal wires.
- Captured entry routes are not compiled yet. Their existing Graph execution
  remains available; their presence no longer vetoes a separate live entry.

These are conservative executable extensions, not a claim that outside wires,
reset interiors or all CPU instant networks have been optimized away.

The final no-classic CPU replay installs **2,197 executable singleton kernels**,
compared with zero in the initial audit, and executes **29 kernel invocations in
100 active ticks**. The full admitted catalog
contains 2,354 actors in symbolic regions; some multi-actor regions still lack
runtime effect certificates. The 64-cell external-wire diagnostic compiles and
lowers all 64 sampled cells. Those sampled cells are not whole-CPU coverage.
The CPU's 100 active tick comparison passes, checking ordered pending work and
command outputs each tick, full state at ticks 10 and 100, and 16 interpreter
handoff ticks. This is not a complete-sort or performance measurement.

The CPU's remaining work is substantial: 32,505 wire walks, 302,784 piston samples
and 27,884 accepted actions still occur in those 100 ticks. Installing thousands
of kernels does not imply that thousands execute in this history or that the
surrounding wire/reset interior has disappeared. No speedup is claimed here.
The separate COUNTER replay passes its saved-idle and release native histories.

Implementation verification uses five new real-layout tests in
`backend/graph/tests/instant_layer_entry/interfaces.rs`, including shared-token
mixed-entry fallback and unrelated FIFO preservation. JSON loader checks cover
retained geometry observations and matching boundary-state guard bindings.

Final verification: 499 Redpiler tests pass (32 ignored), plus the explicitly
selected CPU and COUNTER replays; 25 Python loader tests and 31 viewer tests pass.
Core test/bench compilation, focused Rust formatting and diff checks pass. The
full release suite, full CPU sort and speed benchmarks were not repeated for
these extensions. The checked-in schematic is unchanged; CPU comparisons use
the approved identical in-memory sign removal in Graph and interpreter inputs.
The final server recheck was canceled while waiting for another build's Cargo
lock; the final core test/bench checks completed successfully.

Relevant commands:

```powershell
cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib bubblesort_layers_and_active_history_match_native --locked -- --ignored --nocapture --test-threads=1
cargo test -p mchprs_core --lib counter_layers_and_manifest_histories_match_native --locked -- --ignored --nocapture --test-threads=1
py -3 -m unittest tools.redpiler_graph.test_loader
node --test tools/redpiler-graph-viewer/viewer.test.cjs
cargo check -p mchprs_core --tests --benches --locked
```

## Full-sort baseline, 2026-10-19, before interface support

The production release benchmark completed three full sorts with O4,
`--stable-instant-inputs` and `--classic-redstone`. No tracing, test recorder or
unsafe Turbo experiment was enabled. All 10,036 RAM samples matched the frozen
interpreter trace in each run. The final sixteen words were `0..15`; the other
RAM banks matched too. Each run recorded the same 415 RAM transitions. Reset and
32 interpreted continuation ticks completed, with repeatable checkpoints across
the Graph samples. This is not an exhaustive complete-state differential proof.

| Executor | Measured window | Seconds, three samples | Median | Active TPS |
|---|---|---|---:|---:|
| O4 Graph | 10,036 tick calls | 85.522, 79.511, 91.220 | 85.522 s | 117.35 |
| Interpreter | 10,035 active tick calls | 92.929, 91.624, 86.150 | 91.624 s | 109.52 |

Compilation (about 28 s per Graph sample), activation, RAM probes, reset and
continuation are outside Graph tick timing. Display flushes were timed separately:
9.260, 8.310 and 9.814 s. The interpreter harness also runs to 50,000 ticks and
checks frozen whole-world checkpoints, ordered command output, halt timing and
per-tick RAM; its inactive tail is excluded from the active numbers above.

These are separate, non-interleaved harness runs with overlapping ranges. Graph
uses the approved in-memory sign removal; the interpreter benchmark retains that
sign. The schematic on disk is unchanged. The modest median difference is not a
controlled claim of a kernel speedup. All Graph samples installed **zero kernel
nodes** and recorded **zero kernel invocations**.

Identical Graph work in every full sort:

| Operation | Count |
|---|---:|
| Callbacks | 205,668,780 |
| Wire walks | 3,882,011 |
| Piston samples | 35,541,685 |
| Accepted piston actions | 3,295,083 |
| Scheduled ticks | 2,344,242 |

Reproduction commands:

```powershell
cargo bench -p mchprs_core --bench cpu_bubblesort --bench cpus --no-run --locked
target/release/deps/cpu_bubblesort-8fce2cc718c32b01.exe --graph-tier 4 --stable-instant-inputs --classic-redstone --iterations 3 --output target/cpu-bubblesort-full-o4-20261019.json
target/release/deps/cpus-6536be3d5ba1e0dd.exe --cpu cpu_bubblesort --iterations 3 --output target/cpu-bubblesort-full-interpreter-20261019.json
```

The Graph executable SHA-256 is
`4748ec834e062487620a6691c5930788ea426810038e56a0ac017507991ae2f4`.
It predates the candidate partition changes below; those changes cannot explain
the baseline timing. The fixture SHA-256 is
`803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611`.

## Why local instancy currently buys no CPU acceleration

After the partition changes, three more full Graph sorts passed the same RAM and
continuation checks. Tick times were 63.003, 78.611 and 72.949 seconds: median
72.949 seconds, or 137.58 TPS. The combined artifact is
`target/cpu-bubblesort-full-final-20261019.json`; its executable SHA-256 is
`3de05c5fa36d1c741d4c49b14c0893836604474ecd56cad6fdd8b5c46305829f`.
These historical runs also enabled `--classic-redstone`. They installed zero
kernels and performed exactly the same semantic work as the baseline. The timing
change is not an attributable kernel speedup: the samples were not interleaved,
and environmental or code-layout effects were not isolated.

The original layer partition grouped all 11,025 admitted cells (15,200 actors)
and 4,823 corridors into one component. The first payload-reset actor caused
`BoundaryEffectUnsupported`, rejecting that entire component. This does not
prove the CPU is noninstant. It exposes the gap between the broad local cell
contracts and the narrow executable-wave certificate.

The executor currently certifies private, stationary, own-observer-reset cells
and closed straight single-source/single-consumer cables. Reset and later
extensions remain on Graph. Shared payloads, general fan-in, captured arrival
order and effectful outside feedback still need proofs. Stable external inputs
do not remove these internal causal dependencies.

## Original candidate composition cuts, before interface support

The partition experiment separates three decisions:

1. Keep the complete cell, corridor and boundary catalog. BUD memory and ordinary
   devices retain Graph ownership.
2. Join only owners in the existing executor's necessary family, through
   computation power and synchronous computation callback dependencies. Reset
   clocks, deferred publication and geometry reads remain perimeter obligations.
3. Run the existing full layer, corridor, route and queue-ownership certificates.
   A smaller component is not permission to discard an output or callback.

Every original port remains attached to one owner. Whole shared-payload cells
are never split into competing token owners. An unsupported owner/cable stays
outside neighboring candidate waves. The runtime still declines before mutation
when entry provenance, settled cable levels or the actual FIFO do not match.

The four-view audit passed in 33.19 s after optimized preparation and the
early-rejection/partition-reuse fixes. The original catalog is still complete:
11,025 cells and 4,823 corridors. Selective composition produces 8,597 candidate
proof groups (including wiring-only groups); its largest cell group has 426
cells. **No CPU kernel is certified or installed.** This is useful decomposition,
but it does not yet remove runtime work.

| First decline reason | Cells in declined groups |
|---|---:|
| Unsupported boundary/lifecycle effects | 5,267 |
| Unproven signal arrival through wiring | 4,138 |
| Unproven shared-owner ordering | 1,313 |
| Unsupported callback semantics | 307 |

These are first-decline categories, not CPU-time shares or proof that other
conditions would pass. They sum to the 11,025 local cells, not to actor count.
The source witnesses are retained in
`target/layered-instant-devices/cpu-kernel-proof-cuts.json`.

Restricting the entire CPU catalog to observer-reset groups retains 9,182
actors in 7,800 cells; restricting further to private own-observer cells retains
6,487. Neither view certifies a kernel. The 64-cell external-wire sample also
certifies zero: each retained cell exposes a boundary the current single-cell
output compiler cannot represent. Removing unsupported reset neighbors alone
therefore does not solve the execution problem.

Two preparation costs were corrected while testing the split. A failed corridor
proof now borrows the existing partition instead of rebuilding the complete CPU
partition for every rejected group. A single-cell proof checks whether its output
families are representable before building equations and searching global input
tables. These changes do not broaden the executor certificate.

The ignored `bubblesort_kernel_proofs_report_reset_and_ownership_cuts` test also
tries observer-only/private-owner catalogs. Its external-wire probe samples at
most 64 complete cells because the single-cell compiler still searches global
port tables. That sample is diagnostic, not a production input limit or a
whole-CPU coverage claim.

## Next proof work required for meaningful runtime replacement

1. **Complete boundary values and effects.** General power outputs, live/captured
   geometry reads and one-way outside notifications must retain explicit bindings
   to authoritative state. The current singleton compiler handles only a narrow
   subset; cutting a wire into an output does not make that output irrelevant.
2. **General corridor arrival certificates.** Reuse `SignalTracer`'s memoized
   source/port/threshold equations for fan-in and conditional contacts. Prove the
   qualifying update and request order separately. Settled power equations alone
   cannot replace Turbo: transient requests can reserve a FIFO position before
   later inhibition changes.
3. **Multi-actor effects and entry composition.** Conditional shared-token writes
   and multiple alternative live entries are now supported. Captured entries,
   mixed-route multi-actor queues and synchronous outside feedback still need
   certificates. Preserve those requests on Graph until their actual ordering
   and continuation are represented; do not erase them to make a wave fit.
4. **Absorb the certified reset interior.** The present kernel still commits
   per-piston actions and runs reset/deferred propagation on Graph. A large speed
   improvement needs boundary effects and compact reset continuations, with
   reconstruction confined to projection/handoff where permitted. First-response
   microbenchmarks do not measure that remaining lifecycle work.

The sorting and decomposition results do not complete these steps. In particular,
this report makes no claim that BubbleSort now executes efficiently in kernel mode.
