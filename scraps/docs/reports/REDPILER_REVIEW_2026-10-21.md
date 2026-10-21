# Redpiler release and architecture review

Date: 2026-10-21. Scope: released v0.6.0 and the separate one-shot rewrite in
the working checkout. This is the saved review from the development session;
it is a dated assessment, not the current implementation reference.

The next major improvement should be **one clear model for logical values,
state and events**. COUNTER and FPU demonstrate that the value engine is fast.
BubbleSort and ANPU show that retaining physical event execution is now the
main obstacle to generalizing that speed.

The release added 1,788 lines and removed 157 across 36 files; compiler
complexity grew overall.

## What was added

- **Production FPU logical execution.** The compiler derives 68 inputs, 22,228
  shared expressions, 25 strength outputs and 15 feedback bits. Execution uses
  the existing incremental evaluator and logical clock.
- **An automatic O6 contract.** Stable inputs through reset, Boolean internal
  logic where applicable, stable observable outputs, and the approved
  shared-block pull model require no additional flag in v0.6.0.
- **Clock and control support.** The FPU clock family joins the existing
  COUNTER family. Button timers remain scheduled logically, without restoring
  internal physical propagation.
- **Stronger validation.** FPU checks cover accepted vectors, exact wire
  strengths, immediate publication and the six-game-tick cycle. BubbleSort and
  ANPU retain their full trace checks.
- **Production benchmarks.** FPU now measures compile, trigger activation,
  then execution, without warm-up. COUNTER execution time and benchmark
  overhead are reported separately.
- **A separate one-shot prototype.** The working checkout contains an executor
  for a narrowly qualified instant cell. It is excluded from the release and
  remains experimental.

## What was removed, bypassed or excluded

| Change | Actual scope |
| --- | --- |
| Physical callbacks, wire walks and piston actions | Bypassed during qualified COUNTER/FPU logical execution |
| Internal observer/reset propagation | Folded away under the accepted computation-frame contract |
| FPU `tellraw` diagnostics | Deliberately omitted in qualified O6 frames, as approved |
| Exact internal FIFO expectations | Removed from the relevant O6 comparison; event contents, timing and subsequent state remain checked |
| Slower guard-packing experiment | Excluded after its measured regression |
| Independent one-shot executor | Kept outside v0.6.0, rather than deleted |

**The physical graph itself remains allocated.** Fast execution currently sits
inside `GraphBackend`, which retains physical program/state data for activation,
projection and lifecycle handling. Runtime contraction is substantially ahead
of representation and memory contraction.

## Cleanup priorities

1. **Make release and research states unambiguous.** The working rewrite
   references an untracked `logical_firing.rs`. Commit or archive the complete
   experimental slice atomically. Record the source revision in benchmark
   results and deployment instructions. This would prevent another situation
   where the release builds but the working checkout fails.
2. **Replace stale status documentation.** `TODO.md` contains old test counts,
   old throughput figures and statements that FPU production integration is
   unfinished. Keep current status concise; move historical measurements and
   rejected approaches into research notes.
3. **Expose the executor actually selected.** “O6 compiled” should distinguish
   a logical device from ordinary Graph execution. Preserve compiler and
   activation decline reasons in existing statistics. Silent fallback makes
   performance failures difficult to diagnose.
4. **Simplify runtime ownership.** `LogicalDevice` holds separate optional
   register and frame machines. A small enum would express the valid
   alternatives directly. Avoid another collection of optional handlers as
   more modes arrive.
5. **Separate validation claims.** FPU adapter equivalence, independent
   arithmetic correctness and native continuation are separate claims. The
   accepted-vector reference shares compiler machinery with the production
   extraction. Add an independent arithmetic oracle once the build's encoding
   and operations are specified.

## Optimization priorities

| Priority | Work | Reason |
| --- | --- | --- |
| First: compilation waste | Attempt qualified FPU compilation before constructing a fallback plan that will be discarded; reuse graph indexes across clock/resource inference | FPU compilation takes approximately 25–28 seconds. The code visibly repeats work, although its exact cost needs profiling |
| First strategic runtime task | Execute a small ordinary BUD slice using logical events, then extend toward BubbleSort/ANPU | Their power queries already compile broadly; physical scheduling, updates and commits remain |
| Next: projection | Measure full node/contact scans and publish changed boundary outputs directly where justified | A small logical kernel still pays for infrastructure sized around the physical graph |
| Later: expression tuning/JIT | Optimize only demonstrated evaluator hotspots | COUNTER and FPU meet their targets. Another cache or JIT is less valuable than removing CPU event work |

## Areas approaching critical mass

| Area | Pressure | Required direction |
| --- | --- | --- |
| Execution ownership | Graph, logical devices and the one-shot prototype intercept overlapping lifecycle hooks | Establish one explicit owner for each region, state slot and retained event before promoting another executor |
| Timing specialization | Two qualified clock families work; a growing catalogue of bespoke controllers will become difficult to maintain | Lower supported timing into logical operations using the existing scheduler |
| Boundary semantics | Terminal wires, observer suppression, BUD notifications and retained effects are handled through different paths | Reuse one boundary catalogue describing value ports, strength requirements and observable events |
| Compiler plan lifetime | Pass ordering already mattered enough to erase previously constructed BUD plans | Make construction/invalidation order explicit and enforce it with validation |

The required redesign is targeted. Retain the parser, physical Graph fallback,
scheduler, Boolean evaluator and native reference tests.

The logical representation should describe shared Boolean/strength expressions,
persistent BUD state, once-per-cycle instant firing state, explicit sample,
acceptance, publication and reset events, and observable boundary outputs and
required notifications.

That distinction matters for BubbleSort: its feedback cannot generally be
replaced by repeatedly evaluating Boolean equations until stable. Instant cells
fire once until reset, and BUDs can sample on an update even when the signal
value is unchanged. Those semantics need explicit state and events. Exhaustive
input/state caching remains unsuitable.

Pause additional device-specific runtime branches, clean up ownership and
diagnostics, then prove this representation on a small Rilax-style BUD slice
before expanding to CPU memory.

This was a targeted code-and-evidence review; profiling was not rerun. General
logical handoff and library-wide equivalence remain unproven.
