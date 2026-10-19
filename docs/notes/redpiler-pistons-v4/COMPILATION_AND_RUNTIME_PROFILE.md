# Redpiler compilation and runtime costs

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
