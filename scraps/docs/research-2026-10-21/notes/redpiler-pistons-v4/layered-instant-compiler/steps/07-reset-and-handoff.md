# Step 07: reset continuations, recurring waves and interpreter handoff

Status: planned; requires reviewed runtime replacement. Stop after lifecycle review.

## Goal

Replace reset circuitry with small derived continuations while preserving the shared clock,
future observations and subsequent execution. Keep meaningful phase/owner state, not the
cross-product of physical component states or a learned global reset period.

## Implementation

1. Derive per-reset owner wake contracts from step 02. Reuse existing reset NodeIds,
   schedule deduplication and phase priorities. Do not invent a universal reset duration.
2. Route admitted owned reset wakeups into compact continuation entries. Unowned reset
   observers and local exact reset-wire specializations remain normal Graph fallback.
3. Preserve independently valid actor/resource publications where they affect boundary
   behavior. A stale actor witness cannot automatically suppress a valid shared payload.
4. Reset may change available power, restore geometry, emit boundary effects or create a
   new computation wave under held inputs. Refresh consumption only at the proven wave
   cut. Stable input does not mean an oscillator can be replaced by permanent idle.
5. Keep independent clocks independent. Do not enumerate their combined phase period or
   require a complete-device recurrence to admit a region.
6. Handle imported work explicitly. Preserve unrelated typed/stale/duplicate/priority work.
   An unsupported imported internal tick declines before activation instead of disappearing.
7. Supply a cold materialization recipe from the current logical state and boundary frame.
   Replay no generic interpreter inside timed inference. Rebuild internal dust/poses only
   for projection or a supported handoff, with boundary source bindings still authoritative.
8. If immediate handoff is unsafe, use pending/completed lifecycle state and a certified
   local safe-cut predicate with a justified wait bound. Advance the ordinary global clock;
   never tick only the region, fast-forward the plot, or wait for global idle/phase alignment.
9. Materialize once and transfer remaining scheduled work once. Invalidate stale kernel
   continuations by revision so they cannot later restore a retired kernel's state.
10. Define input changes while busy: admit a supported entry/continuation, or leave/deoptimize
    at a valid cut. The stable-input promise does not authorize silently losing an input
    operation while the kernel waits to hand off.

For an explicit protocol violation with no safe fallback, produce a specific diagnostic.
Do not run an already unsupported partial continuation or corrupt the outside world to
preserve the appearance of successful compilation.

## Tests

- Cut before activation, during accepted computation, before/after reset deadlines and after
  settling. Compare ordinary memory, boundary signals and pending future work after handoff.
- Continue both native and handed-back execution for several ticks; include a new activation.
- Shared payload capture/drop/recapture, two actors with independent publication validity,
  interrupted extension and blocked computation.
- Repeated held-input responses and staggered reset clocks without global period inference.
- Repeated reset/handoff requests, stale continuation version, revision wrapping and cancellation.
- All 48 pinned adder arithmetic histories with cycle-by-cycle output windows and local memory.
- Counter must preserve its BUD memory owner, qualified updates and future count sequence.

## Gate

ADDER1BIT and ADDER11BIT run complete computation/reset cycles through real kernels, with
correct boundary outputs and native continuation. Imported internal work and protocol limits
have tested diagnostics. No raw-wire state is retained as hot memory merely for display.
Measure reconstruction cost separately; it is not free and may matter to real server TPS.
