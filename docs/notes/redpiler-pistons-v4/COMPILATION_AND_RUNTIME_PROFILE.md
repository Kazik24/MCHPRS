# Redpiler compilation and runtime costs

## Current pipeline audit and the eight-second target

Source audit: 2026-10-19. The measurements further below belong to the older
`9831f7e` checkout; they are evidence of repeated work, not timings for the current
worktree or a claim that the eight-second target has been met.

The target is request-to-activation time for a specified large fixture in an
optimized server build. Include snapshot creation, preparation, passes, validation,
lowering and publication. Keep simulation benchmarks separate. Eight seconds
cannot be promised for arbitrarily large selections; first establish it on
CPU_BubbleSort, then check ANPU and the larger CHUNGUS workload.

### What the current compiler actually does

1. The plot makes an isolated activation snapshot, including pending ticks and
   piston state. Closing open barrels is staged on that snapshot.
2. `passes/rich.rs::prepare` parses geometry/contacts, electrical channels and
   ordered callback routes, then assembles one `CompileGraph` and its entry state.
   These parser stages run once, not once per optimization pass.
3. `passes/mod.rs::run_graph_pipeline` validates the graph and runs the selected
   sequence. Rewrites have mutable access and clear derived execution plans;
   builders receive an immutable graph and return a typed `PlanOutput`.
4. Required stages in `passes/validation.rs` check the whole graph after a
   rewrite and all execution plans after a read-only builder. Input validation
   remains mandatory; the duplicate exit validation has been removed.
5. `PreparedGraph::lower` performs another full validation, lowers kernels and
   immutable program tables, builds indexes and binds imported ticks.
6. `GraphBackend::from_lowered` checks supported instructions and constructs the
   executor. Successful plot-thread publication transfers pending work and closes
   containers; failures leave the live input intact.

With the current default pipeline and `-O4 --stable-instant-inputs`:

| Operation | Before | After this batch |
| --- | ---: | ---: |
| Enabled graph rewrites | 7 | 7 |
| Enabled read-only builders | 11 | 11 |
| Full structural validation | 21 | 9: entry + seven rewrites + lowering |
| Complete execution-plan validation | 21 | 20: entry + 18 outputs + lowering |
| Instant boundary inference | 5 | 4 |
| Instant layer compilation | 4 | 3 |

These counts include the concurrently added `compile-mixed-classic` builder
(one additional builder and full validation). They assume the ordinary successful path, no custom pipeline and no
`unsafe-turbo-fixed-point` feature. They count calls, not equal-cost operations.
Empty catalogs and early declines can make some calls cheap.

`program_graph/validation.rs` does more than checking IDs. It reconstructs fixed
signal plans and fixed classic plans, re-infers instant boundaries, and recompiles
instant layers to compare their certificates. `layers/kernels.rs::lower` then
rebuilds the region partition and power index and certifies corridor arrivals.
Some of this is deliberate proof checking; repeatedly rediscovering the same
unchanged facts is the simplification opportunity.

Ordered callback links now use a streaming exact comparison with receiver-bearing
route rows. It compares every original key field and multiplicity without building
a map. Differently ordered link tables retain the exact `FxHashMap` multiset
fallback. No route, callback or duplicate is removed. Guard-cycle validation also
reuses its temporary DFS stack across roots instead of allocating one per root.

### Simplify first, retaining circuit behavior and correctness checks

Items 1–3 and the immediate-lowering sharing in item 5 are implemented. Item 4
remains a proposal: no persistent validation cache or validation receipt was
introduced. The shared layer context is transient, never stored in a draft or
runtime program. Changes
retain instant admission rules and observable circuit semantics:

1. **Remove the redundant pipeline-exit validation.** Entry and every enabled
   output are already validated; disabled entries do not mutate the graph. Keep
   the final cancellation check, statistics and the lowering trust-boundary
   validation. Read-only JSON inspection also remains covered by input/output
   validation, including empty and all-disabled pipelines.
2. **Separate structure validation from execution-plan validation.** After a
   mutable rewrite, check both. After a read-only builder publishes its typed
   output, check the plans against the previously validated structure. Initially
   validate all plans after each builder: one new plan can invalidate an older
   dependent plan. The base structural scan would then run nine times rather
   than twenty-one on this O4 path: entry, seven rewrites, lowering. Keep public
   full validation for arbitrary constructed/exported drafts. This requires a
   careful split of wire execution certificates from structural wire checks,
   not simply skipping `validate_inner`.
3. **Make exact callback correspondence cheap.** For links in parser route order,
   compare the expected receiver-bearing deliveries with links in a streaming,
   allocation-free exact comparison. Match source, target, callback reference,
   guard, kind and direction, and require equal lengths. Preserve the current
   multiset checker as the fallback for valid differently ordered link tables.
   This changes no route, delivery multiplicity or acceptance condition.
4. **Validate proof dependencies once per unchanged version.** After the simpler
   split is established, identify each certificate's real dependencies. A later
   unrelated wire-index builder should not need to reconstruct instant layers.
   Use compiler-local ownership/version evidence, not a persistent unchecked
   `already_validated` flag or a hash used as proof of equality. Rewrites and
   replacement of dependencies must invalidate affected certificates. Keep an
   independent full validator for malformed/custom drafts and differential tests.
5. **Share discovery products within compilation.** The final full validation
   reconstructs and checks layers, then returns their region partition and power
   index in `LayerCompilationContext`. Kernel lowering consumes these immediately
   against the unchanged draft instead of discovering them again. The context is
   dropped before program-table construction. Corridor certification still runs;
   no proof check is skipped. Ordinary standalone kernel tests rebuild their own
   context and compare kernels with the reused-context result.

Do not merge unrelated passes into one opaque function, reduce proof budgets to
make compilation look faster, disable validators in release, or assume fixed
builder order when the developer API allows custom ordering. No new synthesis
algorithm or runtime behavior change is needed for the first three items.

Required checks: malformed pass output still rejects; empty, disabled, cancelled
and custom pipelines retain their behavior; out-of-order callback-link tables
remain accepted; missing/extra/mismatched and duplicate links still reject where
appropriate; corrupted certificates and stale dependent plans still reject.
Compare prepared graph/export, lowered program, initial state and imported work
before/after. Then run existing adder, counter and CPU differential episodes.

For timing, use the existing release harness with the same fixture hash, flags,
budget and sign normalization on both revisions. Record each paired compile
sample, not just the median; separate snapshot, parser, pass bodies, validation,
lowering and activation. Use the existing timing/report infrastructure or
temporary local measurements, not a new profiling framework. An eight-second
result needs a fresh measurement; the old breakdown does not establish one.

### Correctness argument for the implemented changes

The runner establishes valid structure at entry. A rewrite discards execution
plans and checks the complete result. A builder receives only an immutable draft;
its publisher can change only execution-plan fields. Checking every plan after
publication therefore preserves valid structure and plans by induction, including
custom pass orderings and replacement of earlier plan dependencies. No mutation
occurs between the last output check and pipeline exit. Empty or disabled-only
pipelines retain the validated input. Lowering still performs a complete check.

For callbacks, equality of the complete expected edge sequence implies equality
of the old key multiset, including duplicate counts. Any sequence mismatch uses
the unchanged multiset checker. Both ordered and permuted valid tables therefore
remain accepted; missing, extra, mistargeted and misdirected edges still reject.
Receiverless deliveries require no edge, as before. The DFS work stack empties
before the next root, so reusing its allocation changes no traversal or cycle test.

The layer context comes from reconstruction against the same immutable draft
that immediately enters kernel lowering. Neither input nor index is mutated in
between. It is freed before ordinary program tables are constructed. Independent
context reconstruction remains available in tests; ADDER1BIT and ADDER11BIT
compare those kernels with reused-context lowering at O0/O2/O4.

Final verification: `cargo test -p mchprs_core --lib redpiler:: --locked --
--test-threads=1` passed 512 tests, with 35 explicitly ignored. This includes all
eight new validation regressions, the adder graph/program/state comparison, stale
dependent-plan rejection, arbitrary callback-link permutations, exact duplicate
keys and malformed edge multiplicity. The final release CPU benchmark built.
Long ignored research captures and a new full CPU sort were not run.

### Final paired release measurement

CPU_BubbleSort, `-O4 --stable-instant-inputs`, budget 8. The existing
`cpu_bubblesort` bench compiles the complete `world.get_corners()` selection,
then validates 100 active ticks against the frozen interpreter RAM and performs
32 native continuation ticks after reset. The saved schematic is unchanged;
both executables remove the same blocking sign only in memory.

The benchmark measures `Compiler::compile` wall time, including required parsing,
passes, validation, lowering and backend construction. World loading, plot snapshot
creation, CPU activation and visual publication are outside that compile timer.
This therefore does not establish the eight-second in-game request-to-activation
objective. No `graph-profile` feature or test tracing is enabled.

Pinned release executables were run alternately A/B, B/A, A/B on logical CPU 4
(affinity mask `0x10`), with no Cargo/rustc process running during these final
pairs. These are three local pairs, not a general hardware performance guarantee.

| Pair | Before | Final | Less compile time |
| --- | ---: | ---: | ---: |
| 1 | 29.942870 s | 21.576285 s | 27.94% |
| 2 | 28.322816 s | 18.951001 s | 33.09% |
| 3 | 28.245765 s | 18.341420 s | 35.06% |
| Median | 28.322816 s | 18.951001 s | 33.09% |

Baseline range: 28.245765–29.942870 s. Final range:
18.341420–21.576285 s. Every pair had matching active/reset/continuation
checkpoints (blocks, entities, scheduler, piston state and chat), RAM hashes,
execution counters and kernel ownership. Each individual run also passed the
existing frozen interpreter RAM checks. No inference-speed improvement is claimed.

On the final median sample, sequential pass totals were 6.625 s for
`compile-instant-layers` and 2.984 s for `identify-instant-boundaries`. Each total
includes that builder and its output validation. Another 4.545 s followed the
pipeline, including full lowering validation, kernel lowering and backend
construction. These are disjoint wall-time phases, not sampled function self-time.
They explain why this safe cleanup alone does not reach eight seconds.

The next larger target is the repeated semantic reconstruction of instant
certificates: layers are still compiled three times and boundaries inferred four
times. Removing further checks requires a sound certificate/dependency ownership
contract or a cheaper independent checker. This batch does not replace those
checks with an unchecked validity flag, weaken input assumptions, or lower
admission/proof budgets.

Artifacts: `target/compile-pipeline-final-paired/summary.json`, the six per-run
JSON/log files in that directory, and pinned executables/source snapshots under
`target/compile-pipeline-baseline` and `target/compile-pipeline-final`. An earlier
three-pair intermediate experiment is retained separately under
`target/compile-pipeline-paired`; its numbers are not mixed with this final table.
Peak RSS, a new full sort and complete server-request latency were not measured.

### Asynchronous plot compilation

The previous scoped-worker loop still blocked normal plot dispatch. It called
`Player::update` to send keep-alives, but did not run packet handlers that record
their replies. That can time out a healthy connection; a Broken pipe log alone
does not prove this caused a particular disconnection.

The current worktree moves compilation into an owned background job in
`plot/redpiler_compilation.rs`. The normal plot loop continues handling packets,
commands and players while simulation is paused. Completion is polled without
waiting. Only successful results matching the live chunk content versions,
pending ticks and piston state activate. Lighting-only changes do not invalidate
a circuit. Reset cancels cooperatively; a cancelled worker must finish before
another worker can start on the same plot. Plot shutdown cancels without joining
the worker. Compiler errors and panics cannot publish a partial executor.

Snapshot creation and final publication still execute on the plot thread.
`snapshot_ms` is logged separately so this remaining synchronous work is visible.
Async execution fixes plot responsiveness; it does not reduce compiler CPU time
or establish the eight-second target. Analysis/JSON-inspection commands have not
been converted into background jobs in this change.

The earlier captured-entry prototype is deferred from this compilation-stability
batch. Its new fixture reached a piston request with `source: None`, not a proven
captured-route activation, so the prototype has no passing regression for its
claimed family. Only its three uncommitted draft files were restored to the
previous implementation; the local patch is saved at
`target/captured-entry-prototype.patch`. The experiment would treat a delivered
boundary event as already qualified and require exact initial FIFO equality
before commitment. Captured-view semantics, acceptance revalidation and a real
captured-entry fixture still need proof. Production keeps its previous live-entry
eligibility rules in this batch.

Verification for the asynchronous path: all eight `plot::redpiler_compilation`
tests passed, together with the existing barrel-close activation, pressure-plate
input and queued-piston rejection/highlight regressions (eleven focused tests).
The tests cover a deliberately blocked worker while keep-alive packets and reset
commands are dispatched, cancellation after completion, prevention of overlapping
workers, stale block/entity/tick/piston state, lighting-only changes and a worker
panic. The server check and focused formatting/diff checks passed.

An earlier wider `plot::` run had 284 passes, twelve failures and one ignored test.
The queued-piston diagnostic failure was fixed and rechecked separately; the
other failures concern packet/command expectations, local plot database setup,
sidebar layout, routing and world-edit limits. They were not audited against a
clean baseline, so this batch does not claim a passing full plot suite. Details
are in `target/async-compilation-plot-tests.log`; the final focused run is in
`target/async-compilation-final-tests.log`.

The audit originally contained no fresh timing claim. The implemented changes
and their new CPU compilation measurements are recorded in this document; the
historical profile below remains separate. No new full CPU sort was run.

---

Measured 2026-10-19 on an isolated checkout of `9831f7e`. The live checkout was undergoing
a reset API refactor and briefly failed to build; those unfinished changes are excluded.
Only benchmark reporting and diagnostic timers differ from that committed snapshot.

CHUNGUS compilation is dominated by repeatedly validating callback links. Runtime is
dominated by ordered wire execution and the reads/callbacks it causes. ANPU and BubbleSort
retain the more expensive general mixed-circuit path. Scheduler work is a small target.

## Measurement boundaries

- Windows x64, Ryzen 9 5950X, logical CPU 4, affinity `0x10`.
- Profiling Cargo profile: release optimization and fat LTO, with matching PDBs.
- O5, budget multiplier 8, `classic_redstone=false`, `stable_instant_inputs=false`.
  A separate O2 diagnostic measures the configuration behind the earlier 25-second claim.
- Uninstrumented runs provide the inference times below. Compilation, activation,
  observation/projection, reset and oracle checks are outside inference.
- Compile diagnostics time validation directly and split each pass body from its output
  validation. These are elapsed times, not sampled CPU percentages.
- Runtime diagnostics count selected function entries and randomly time approximately
  one in 1,024 calls, only inside Graph tick phases. They are temporary code in the isolated
  checkout, not a new production feature or a persistent profiling framework.
- Windows denied/timed out both requests for Samply administrator access. No fresh ETW
  stack profile or hardware cache-miss measurement was obtained. The call timers let the
  investigation proceed without administrator access.

These are individual runs, not paired speedup measurements. Other Cargo/rustc activity
was observed during parts of this session. Timers and code layout also perturb execution.
The runtime diagnostic took 16.9% longer for CHUNGUS and 17.0% longer for ANPU, but 9.2%
less time for BubbleSort. Those ratios do not isolate instrumentation overhead or establish
a performance improvement. Use broad cost rankings; do not treat short-function estimates
as precise CPU shares. Nested/recursive call times overlap and must not be added.

## Current O5 execution times

| Machine    |                    Window | Compilation | Inference | Game ticks/s | Projection                     |
| ---------- | ------------------------: | ----------: | --------: | -----------: | ------------------------------ |
| CHUNGUS    |       16,384 active ticks |    37.913 s |   5.023 s |      3,261.7 | One final flush: 6.44 ms       |
| ANPU       |        5,000 active ticks |     9.862 s |   2.527 s |      1,978.3 | One final flush: 3.78 ms       |
| BubbleSort | Complete 10,036-tick sort |    17.104 s |  76.799 s |        130.7 | Per-tick flushes total 8.315 s |

Projection protocols differ: a single final flush cannot be compared with 10,036 flushes.
These are not whole-server or network throughput numbers, and differences from older
measurements must not be attributed to O5 alone.

An additional 10,000-tick ANPU run performed the same 267,998 scheduled component ticks,
183,751 wire walks and 10,682 actions as the 5,000-tick run. Its tail was quiescent. Its
3,781 ticks/s is therefore excluded from the active-workload table.

## Compilation breakdown

The following table uses separate diagnostic runs, so their totals can differ slightly
from the uninstrumented execution table.

| Machine/tier  | Total compile | Full validations |      Validation time | Callback-link check within validation |
| ------------- | ------------: | ---------------: | -------------------: | ------------------------------------: |
| CHUNGUS O2    |      27.420 s |               14 | **21.710 s / 79.2%** |       **17.822 s / 65.0% of compile** |
| CHUNGUS O5    |      37.922 s |               21 | **30.948 s / 81.6%** |       **26.722 s / 70.5% of compile** |
| ANPU O5       |      11.520 s |               21 |  **9.734 s / 84.5%** |        **8.552 s / 74.2% of compile** |
| BubbleSort O5 |      17.101 s |               21 | **13.361 s / 78.1%** |       **10.012 s / 58.5% of compile** |

`CircuitDraft::validate_links` rebuilds a standard `HashMap` multiset keyed by callback
reference, source, receiver and delivery kind, then consumes it against route deliveries.
This verifies exact matching and duplicate multiplicity. CHUNGUS has approximately
3.48 million callback links, so repeating this operation is substantial work.

O5 validates once at pipeline entry, after each of 18 enabled rewrites/builders, once
at pipeline exit, and again at lowering: 21 times. O2 has 11 enabled entries: 14 checks.
The graph has already passed output validation before the exit check. Read-only plan
builders repeatedly trigger full structural validation even though they cannot change
the base circuit structure.

The pass totals can consequently misidentify cheap algorithms as expensive:

| CHUNGUS O5 pass           | Body/preconditions | Output validation |
| ------------------------- | -----------------: | ----------------: |
| `compile-wire-hops`       |            3.39 ms |       1,566.73 ms |
| `index-turbo-callbacks`   |           14.89 ms |       1,344.67 ms |
| `compile-instant-proxies` |            0.45 ms |       1,359.18 ms |
| `compile-fixed-classic`   |          156.00 ms |       2,155.41 ms |

In that diagnostic CHUNGUS run, geometry/electrical/callback preparation together took
2.490 seconds, pass bodies/preconditions 4.254 seconds, and validation 30.948 seconds.
Fixed-signal re-inference inside validation took 1.934 seconds. Reconstructing the fixed
program during validation took 0.459 seconds. Those repeated constructions exist, but
they are much smaller targets than the callback-link multiset.

Sources: [pipeline](../../../crates/core/src/redpiler/passes/mod.rs),
[validation](../../../crates/core/src/redpiler/program_graph/validation.rs),
[fixed-plan validation](../../../crates/core/src/redpiler/program_graph/fixed_classic.rs),
[lowering](../../../crates/core/src/redpiler/program_graph/lowering.rs).

## Runtime bottlenecks

| Machine    | Dominant work in the diagnostic                                                                                                                                                            | Small or unused work                                                                                                                                                   |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CHUNGUS    | Fixed ordered wire execution; 87.1 million fixed query entries, 71.4 million neighborhood-helper entries and 72.2 million callback-dispatch entries                                        | General wire/guard/site-resolution paths, piston handling and shape handlers have zero diagnostic entries; scheduling estimate is about 1.5% of instrumented tick time |
| ANPU       | General wire walks and their neighbor capture, power queries and site resolution: 15.2 million neighbor-helper entries, 30.0 million power-query entries and 25.3 million site resolutions | No whole-fixed execution; scheduling estimate about 0.6%; piston sampling is appreciably smaller than wire/query work                                                  |
| BubbleSort | General wire propagation, shape updates, site resolution and guard/power evaluation: 1.426 billion site resolutions, 762 million guard entries and 278.5 million power-query entries       | No whole-fixed execution; scheduling estimate about 0.3%; inactive local-net/kernel paths do no executed work                                                          |

The large-call estimates put fixed wire execution around nine tenths of CHUNGUS tick time
and general wire walks around two thirds for ANPU/BubbleSort. These are coarse inclusive
call-time estimates, not an exclusive CPU partition. Short query helpers are particularly
sensitive to timer cost. Recursive fixed callback dispatch estimates even exceed total
tick time; that is not a valid CPU percentage and is not used as one here.

BubbleSort's shape handler and raw side calculation have substantial measured call time.
Piston acceptance and deferred publication also include synchronous callbacks and wire
work, so their inclusive cost cannot be described as the cost of merely moving a token.
The evidence points to repeated dependency/property reads and propagation bookkeeping;
it does not prove a particular hardware cache or memory-bandwidth bottleneck.

Production work counters remain the useful scale check:

| Machine/window            | Scheduled component ticks | Counted callbacks | General wire walks | Piston samples / actions |
| ------------------------- | ------------------------: | ----------------: | -----------------: | -----------------------: |
| CHUNGUS / 16,384 ticks    |                 1,379,485 |        76,553,353 |                  0 |                    0 / 0 |
| ANPU / 5,000 ticks        |                   267,998 |        16,324,304 |            183,751 |         327,585 / 10,682 |
| BubbleSort / 10,036 ticks |                 2,344,242 |       199,211,687 |          3,882,011 |   35,541,685 / 3,295,083 |

Zero general walks on CHUNGUS means that fixed wire steps replace them, not that
wire propagation disappeared. Scheduling and individual redstone arithmetic operations
are poor first optimization targets beside this volume of repeated propagation/lookup work.

## Simplifications that can preserve behavior

1. **Make exact callback-link validation cheaper first.** The smallest candidate is the
   project's existing `FxHashMap` for compiler-owned typed keys, retaining all key/count
   checks. A larger candidate is an exact ordered comparison for compiler-generated links,
   retaining multiset validation when valid input is differently ordered. Missing, extra,
   mismatched and repeated edges must still reject correctly. Benchmark either change;
   no new speedup for these candidates is claimed here.
2. **Delete the duplicate pipeline-exit full validation.** Entry and every enabled output
   have already been checked. Keep cancellation, pass-specific preconditions, mutation
   output checks, and full validation at lowering. Inspect the graph-inspection caller too.
3. **Validate new plan output without rechecking unchanged base structure.** `PlanBuilder`
   receives an immutable graph. Separate structural and plan checks, audit plan dependencies
   and keep final validation. Do not introduce an unchecked persistent validation cache or
   treat every builder as independent merely because it is read-only.
4. **Reuse exact execution in mixed circuits.** ANPU/BubbleSort need fixed-region lowering,
   capture/property reuse and direct queries while preserving live/captured distinctions,
   contact changes, ordering and reset. The existing whole-fixed executor provides a model.
   This is a larger implementation task, not a proven one-line speedup.

Do not replace Turbo/FIFO propagation with final-strength settling as a shortcut. The
existing [ordering counterexamples](DUST_FIXED_POINT_INVESTIGATION.md) show that meaningful
intermediate samples and piston request order can change outputs. Similarly, a ready-to-use
pass with a near-zero body is not free if full validation is charged after it.

No simulation algorithm, compiler admission rule or validation rule was changed in the
main checkout. Only the two existing benchmark reports now expose existing compile timers,
and `classic_baseline` accepts O2-O5. Diagnostic code stays under the ignored profiling
worktree. The timing differences above are findings, not optimization speedup claims.

## Evidence and checks

All uninstrumented and runtime-diagnostic CHUNGUS/ANPU runs passed complete native endpoint
state, exact future and 32-tick interpreter continuation checks. Both full BubbleSort runs
matched every frozen RAM sample and ended with the sorted range; reset and 32 interpreted
continuation ticks completed. That harness does not independently establish complete
continuation-state equality against an interpreter run from the beginning.

Diagnostic work counters match the corresponding uninstrumented runs exactly on all three
machines. Additional 64-tick compile-diagnostic runs passed their existing oracles. Profiling
builds and benchmark whitespace checks passed. No new full workspace/test-suite run was
needed for the reporting-only changes; these runs do not validate later live reset edits.

Local evidence is under `target/profiles/current-o5-20261019/`:

- [Uninstrumented measurements](../../../target/profiles/current-o5-20261019/unprofiled-summary.json).
- [O5 validation breakdown](../../../target/profiles/current-o5-20261019/diagnostic/validation-summary.json)
  and [CHUNGUS O2 breakdown](../../../target/profiles/current-o5-20261019/diagnostic/chungus-o2-validation-summary.json).
- [Raw runtime diagnostic estimates](../../../target/profiles/current-o5-20261019/runtime-summary.json).
- Per-machine JSON/log files, pinned executables/PDBs and the isolated source/instrumentation.

These ignored artifacts may be absent in another checkout; the principal conclusions
and measurements are retained above.
