# Compiled boundary guard predicates

The experimental guard predicate compiler replaces repeated pure Guard DAG
evaluation with compact typed Boolean predicates. It reuses the CPU-kernel DNF
compiler and evaluator; state, physical effects and future work retain their
existing owners. It is another component of whole-kernel execution, not complete
physical-graph replacement.

**Production decision (2026-10-20): excluded from the default pipeline.** Five
alternating fat-LTO pairs show 8.38% more inference time. Correctness alone does
not justify enabling this candidate. The bounded predicate compiler and its
checks remain available for further experiments; its default pass wrapper is
removed.

The [fresh minimized-transition profile](INSTANT_TRANSITION_PROFILE.md) measured
general guard evaluation at 18.96% of inference samples in an overlapping
inclusive group. Ordinary delivery, replay admission and capture selection all
reach that evaluator. Generic piston dispatch alone accounted for only 0.50%.

## Representation and sampling contract

The plan has a four-byte index per original GuardId, a dense predicate-descriptor
array, contiguous typed reads and parallel positive/negative clause masks.
Trivial Always and State guards retain their direct paths. Unsupported roots
and predicates that only invoke their original guard retain raw evaluation.

The shared scalar evaluator handles literals, positive conjunctions and
disjunctions directly. The new `AllLiterals` form short-circuits a single
conjunction containing positive and negative literals; it does not gather all
operands before testing a mask. General clauses retain the existing scalar/AVX2
matcher. Pure read reordering and local Boolean minimization stay within one
original query cut.

Normal CPU-kernel compilation keeps its existing no-capture semantics. Guard
compilation uses a separate private mode: captured-site tests become context
reads. A fallback atom calls the original recursive evaluator with the same
full capture slice or single-site observation, bypassing the compiled wrapper
to avoid self-recursion. Missing capture remains false before negation. Live
contact expansion preserves first-match variant priority and no-match behavior.
No captured value, callback admission result or mutable state is cached across
calls. Delivery order and `instant_event_noop` ordering are unchanged.

Per-root limits remain 64 reads, 64 clauses, bounded recursion and 16,384 work.
The guard product additionally limits global work to 8M and stored table
capacity to 64 MiB. Its root-map bound is checked before allocation. Geometric
reservations avoid reallocating a table for every compiled root. Cancellation
aborts publication; unsupported or exhausted candidates use raw guards.
Publication and full validation independently rederive the plan. Export adds
the capability, compilation statistics and original compiled GuardIds.

## Evidence so far

Both diagnostic and three production benchmark targets compile. Four focused
checks cover live first-match truth tables, mixed-sign short-circuiting, full
and single-site captures, missing captures, identity fallbacks, stale/forged
plans and cancellation. The broad suite has 612 passes plus the expected O5
fixture difference; inspection confirms only the guard capability and statistics
were added, and the fixture recheck passes. That totals 613 passing checks with
the updated fixture. Other optimization-level exports are unchanged.

The first 128 CPU ticks pass exact graph state/future and native RAM comparisons
against raw guards. All execution counts match the preceding minimized program.
Both full-sort normal and stable-input checks pass: 10,036 ticks, all 768 native
RAM bits, sorted final words, pending/future checks and 16 native continuation
ticks. Every exported execution count matches the preceding minimized program.

A 16-tick production coverage probe confirms the attached guard tables for all
484,945 root-map entries, occupying an estimated 31,299,908 allocated bytes
including descriptors, reads and masks. Root-map entries include raw fallbacks;
this is not a claim that every guard was compiled. The reported pass body is
0.554 s and validation 0.528 s in that probe. Correctness runs were active, so
these diagnostic timings are not clean compilation or inference benchmarks.
A one-tick probe was too short to satisfy the harness's scheduled-work gate;
the 16-tick probe passes. Neither supplies a TPS result.

The frozen fat-LTO executable is
`target/guard-predicates/cpu-bubblesort-production.exe`, SHA-256
`7C0CD145D7FE33C2999AF9570DFC3D503A01DFA80D33784FCDCC635B134FA299`.
Artifacts: `target/guard-predicates/`, including build logs, focused/broad tests,
preserved/inspected fixture changes, CPU checks and source provenance.

## Production timing and rejection

| Pair | Minimized reference TPS | Guard predicates TPS |
|---|---:|---:|
| 1 | 428.70 | 400.52 |
| 2 | 437.97 | 399.66 |
| 3 | 421.87 | 404.91 |
| 4 | 431.32 | 397.98 |
| 5 | 364.50 | 332.47 |
| Median | **428.70** | **399.66** |

Median paired inference time increases **8.38%**. All five candidates are slower.
RAM hashes, checkpoints and semantic work counts match in every pair; the new
tables remove no callbacks, wire visits or replay instructions. The unchanged
reference runs faster than in the previous session, demonstrating why historical
absolute TPS cannot establish a code speedup. This session's best reference run
is 437.97 TPS; 428.70 TPS is the more representative five-run median.

The added 31.30 MB table and extra indirections are plausible costs, not isolated
causal measurements. Small predicates and captured-site fallbacks can perform
more work than the recursive evaluator. Future attempts should admit only cases
that remove work, or evaluate captured profile properties without rebuilding a
block. Eliminating effect-free callbacks and their captures remains the larger
algorithmic opportunity.

Artifacts: `target/guard-predicates/pairs.json`, `summary.json`, raw pair reports
and `correctness.json`. The O5 export fixture is restored to its pre-candidate
form. All 613 broad checks pass after excluding the rejected pass. Fresh
[FPU production timing](FPU_CURRENT_INFERENCE.md) also excludes this experiment.
Whole-circuit replacement remains unfinished.
