# Redpiler development plan

**Status:** active work beyond the v0.6.0 release. The release is the production
baseline; its supported optimization tiers and admission rules are documented in
[Redpiler optimization levels](REDPILER_OPTIMIZATION_LEVELS.md). This plan describes
work in progress and does not expand release guarantees. Source, tests, and release
notes decide what is shipped. For implementation structure, see
[Redpiler architecture](REDPILER_ARCHITECTURE.md).

## Accepted execution contract

An admitted logical executor preserves externally observable values at their required
publication ticks, including signal strength wherever an outside consumer depends on it,
normal controls, persistent state, and consequential update/sample/write/publication events.
Internal glitches, intermediate geometry, piston poses, and reset propagation may
disappear when they do not change those results. An update is distinct from a value
change: BUD memory can sample unchanged power after a qualifying update, while a changed
level alone need not cause a write.

An instant actor may fire once before its proven reset and rearm for a later wave. Keep
reset-scoped fired state; do not turn feedback into unrestricted reevaluation or infer
registers from cycles. For ordinary BUD operations, preserve the meaningful sample,
acceptance-time recheck, commit, and delayed publication cuts. Fuse cuts only where the
dependency proof shows that no intervening event can affect behavior.

Graph remains the authoritative game clock, phase dispatcher, and scheduler. Logical
events share its relevant FIFO and deadlines; combine events only when their effects
commute. Give every actor, resource, route, and callback exactly one owner. On decline,
the complete unsupported region stays on Graph. On native handoff, transfer current
observable state and pending work exactly once; never replay an already delivered effect
or resume from a stale physical shadow.

Do not enumerate input combinations, cache whole input/state cases, add fixture-specific
names or coordinates, or introduce a second world simulator. Unknown behavior gets a
bounded decline with a source/owner witness and remains on the existing executor.

## Work sequence

1. **Prove a small memory/event slice.** Use Rilax and small cell fixtures to establish
   ordinary BUD sample, same-level update, acceptance, write, latched-read, and delayed
   availability behavior against native execution. Keep preparation, activation, reset,
   and observation protocols identical between executors. Prove cells and a small
   Rilax slice before generalizing to larger circuits.
2. **Build the graph-derived compiler.** Reuse prepared graph dependencies to form typed
   power, state, update, clock, and effect equations. Propagate constants, remove dead
   roots, classify feedback only after simplification, and retain strength only where
   consumers require it. An unresolved live cycle declines; a cycle is not by itself
   persistent state. Keep bounded work and useful decline witnesses.
3. **Replace internal physical work at proven cuts.** Lower retained state and logical
   clock/events into the existing Graph runtime. Replace internal callbacks, reset work,
   and piston actions only when each consequence has a logical equivalent. Keep boundary
   callbacks and native continuation working while the circuit runs.
4. **Expand by evidence.** Apply the same contracts to BubbleSort and ANPU; preserve their
   RAM/screen values, consequential BUD transactions, controls, and interpreter
   continuation. Treat FPU arithmetic and output publication as separate proof goals.
   A computed result or fixture census alone does not establish native output parity.
5. **Measure and simplify.** Validate arithmetic independently of Redpiler, then profile
   complete release execution against the same fixture, input sequence, and publication
   window. Report compile/adoption, runtime, handoff, retained state, and removed physical
   work separately. Optimize only measured costs; a kernel benchmark or plan size is not
   whole-device TPS evidence. Remove superseded code only after correctness, fallback,
   handoff, and performance gates pass.

Benchmark CPU work separately from full plot simulation, visual updates, and network/server
throughput. State whether compilation, world loading, activation, cache warm-up, reset, and
handoff are included; show both active-tick and complete-episode cost when setup can shift
work outside the measured interval. Confirm output validity at declared publication ticks
independently from timing. Compare the same build mode, fixture, stimuli, and observation
window, and identify whether caches start cold or warm.

## Completion evidence

For each supported family, tests must cover changing controls, meaningful update events,
strength-sensitive outputs, required deadlines, and continuation into the interpreter.
Compare rejected as well as accepted actions where they affect writes. Verify ownership
is exclusive and no callback, queued tick, or publication is duplicated or lost. Keep
unsupported families explicit; coverage of one memory reader or a subset of pistons is
not whole-device compilation. Retain all established O0–O5 behavior and the release O6
contract as regressions.

The dated [instant circuit acceleration plan](../scraps/docs/research-2026-10-21/notes/redpiler-pistons-v4/INSTANT_CIRCUIT_ACCELERATION_PLAN.md)
and [layered compiler notes](../scraps/docs/research-2026-10-21/notes/redpiler-pistons-v4/layered-instant-compiler/README.md)
contain historical design detail and research evidence. Use this page, the architecture
document, and the optimization-level reference for current direction and shipped behavior;
do not copy old status tables or benchmark values forward as current results.
