# Step 06: execute kernels and replace owned propagation

Status: planned; requires a reviewed compact program. Public activation remains off until
reset/handoff and final acceptance are complete. Stop after runtime integration review.

## Goal

Execute the immutable expressions inside GraphBackend and replace covered propagation,
rather than running a kernel and then replaying the same wires/callbacks anyway.

## Code layout

- One focused runtime module, initially `backend/graph/instant_kernel.rs`.
- Source/entry lookup tables built during lowering, not whole-table searches in dispatch.
- Normal power/site/component/scheduler procedures remain shared. Add the smallest direct
  binding hooks to `sites.rs`, `power.rs` and existing entry dispatch.
- Focused tests in `backend/graph/tests/instant_kernel.rs`; no giant test block in graph.rs.

## Execution algorithm

1. An original qualifying boundary occurrence selects the region entry. Check its local
   readiness/protocol assumptions before suppressing any ordinary execution or mutating state.
2. Sample only live input bindings for that declared frame, including outside ordinary/BUD
   memory. Do not allocate/copy the whole program's component/resource arrays.
3. Evaluate topological Boolean expressions into reusable scratch. Keep the input frame only
   while reset continuations need it; normal kernels do not retain per-piston consumed masks.
   Sampling/request construction and accepted computation remain separate. Queue an entry
   in the existing piston-action FIFO when its accepted work belongs to that phase. Batch
   consecutive internal uses only after proving that intervening ordinary work is irrelevant;
   otherwise use a continuation cut in the same queue, or decline before taking ownership.
4. Replay the compact boundary-effect stream. `WriteOutput` updates boundary shadows;
   `Notify` calls the ordinary typed handler with its original direction/capture meaning;
   reset schedules use existing NodeIds, half-tick offsets, priorities and deduplication.
5. Outside readers resolve compiled source power/geometry through those shadows. Never let
   an outside comparator/conductor/dust query read a stale original internal wire slot.
   Cover fast Stored/Output reads and direct guard-state predicates on owned actors/resources,
   not only `resolve_site`. Lower those external observations to the same authoritative
   boundary binding; do not create a second mutable payload copy to satisfy a forgotten read.
6. A transient high followed by low is represented by a few boundary operations. The ordinary
   repeater handler schedules its response; an observer/BUD gets the qualifying notification.
7. Exclusive dispatch ownership prevents covered wire seed/Turbo handlers, piston samples,
   accepted actions and proxy routes from running in addition to the kernel. General devices
   and declined regions still use Graph's current handlers and clock.
8. If an outside callback can synchronously change/reenter the unfinished region, honor an
   admitted continuation boundary or decline before mutation. Do not queue it into another
   tick or overwrite its writes under the external-order relaxation.

No runtime World clone, Turbo evaluator, region training, truth-table lookup built on demand,
wire snapshot construction, per-layer heap allocation or loop over all program pistons.
An initial first-response test may stop at the supported cut before a reset deadline, but
that is partial experimental coverage, not a runnable public circuit.

## Validation

- One ordinary component activates a kernel without a selected lever in the owned region.
- An unrelated scheduled device executes during the same logical tick normally.
- BUD data-only changes retain memory; qualified same-value updates still reach it.
- Sub-tick repeater pulse, comparator feedback, head observer, shape direction and profile reads.
- Two regions sharing an outside source do not reenter/claim one another accidentally.
- On rejection, input state/pending work are unchanged; general execution is used exactly once.
- Accepted first-response ADDER1BIT cases compare real outputs, ordinary memory and pending
  work against native/ordinary Graph. A kernel counter must be positive and owned wire-walk
  count must be zero. Inspect general callbacks by ownership, not total count alone.

## Gate

Real kernel execution replaces internal work, with complete implemented effect variants and
direct source bindings. No unconditional trace formatting is added. Runtime counters measure
actual work; removed physical actions are not counted as secretly executed actions.
Whole-cycle correctness and public activation still require steps 07-08.
