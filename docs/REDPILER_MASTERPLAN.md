# Redpiler master plan: input-driven instant netlists

Status: discovery is implemented for the connected-layer compiler; the complete
input-driven netlist and its fast-frame integration remain unfinished. The saved
FPU does not yet activate a fast logical device. This document distinguishes the
implemented slice below from the remaining design and performance targets.
The [event-netlist scope](REDPILER_EVENT_NETLIST_SCOPE.md) specifies the isolated
research pipeline, pass boundaries and implementation gates.

The implemented slice is
[`regions/greedy.rs`](../crates/core/src/redpiler/analysis/instant/regions/greedy.rs).
It follows exact activation occurrences and immediate effects, including events
inside an indivisible cell ownership group, and trims reached owners using their
native-effect dependencies. Only witnessed callback edges can join candidates;
power reads do not merge ownership. Deferred/reset effects remain boundaries.
The existing connected-region arrival and ownership certificates still decide
whether selected candidates compile. The separate `SettledFrame` admission path
has not been replaced by this discovery pass.

Lowering now selects certified multi-actor or wire-owning PullWaves independently
of a whole-plot `SettledCircuit`. Previously, overlapping Event kernels displaced
those waves whenever that separate plan was absent. A four-cell/six-wire fixture
with competing Events and no settled circuit now executes the PullWave, removes
its internal wire walks and matches native state/reset behavior for 32 ticks.
This fixes selection of already certified work; it does not certify the saved
FPU's larger native-callback boundary.

On the saved-plot census, the pass selected 9,479 of 9,906 catalog owners and
retained 186,037 witnessed immediate callback edges. The existing layer compiler
certified 3,429 regions; 4,495 were declined, predominantly for unproven signal
arrival. A 16-frame alternating-input replay passed native commands, outputs and
all six handoffs, but installed zero fast logical devices and showed no
meaningful callback reduction. This validates the discovery slice and safe
fallback, not the complete netlist or the 40k TPS objective.

## Goal and contract

Compile useful portions of ordinary plots into a compact event-driven netlist.
The compiler must work without circuit names, fixed coordinates, or a required
clock. A plot may contain several independent or connected devices. Unsupported
work stays on Graph; a failed candidate must not prevent other independent
candidates from compiling.

The requested fast-mode compatibility is intentionally narrow:

- In the experimental idealized mode the user permits native nanotick timing
  proofs to be skipped and prefers the idealized Boolean result where native
  timing changes the observed output. Report any such divergence; it is not
  native equivalence. Outside that declared timing assumption, compare output
  states and command effects against native behavior.
- Native redstone remains a reference for the FPU, but the current fixture does
  not specify an independent arithmetic oracle.
- A candidate may require inputs to remain stable within its execution frame
  and may restrict reset, editing, and handoff to a certified settled boundary.
  If an input or world edit violates that contract, do not continue a stale
  compiled transition: hand off only from a materializable state, or decline
  compilation before taking ownership.
- Input lamps and readout repeaters are observable outputs. Native neighbors,
  command blocks, and other unsupported components are boundary work, not
  silently deleted. A region may own a boundary only if it preserves the
  effects required by that boundary; otherwise shrink or decline it.
- A movable block and every piston that can move it form one exclusive physical
  ownership group. Reading a native source does not transfer ownership of it.
- Every candidate needs bounded build time and memory, cancellation, a clear
  reason for rejection, and Graph fallback. No invalid assumption may cause a
  silent output mismatch.

The user accepts **20,000–30,000 game ticks/s on the actual saved FPU plus
neighboring Divider with changing inputs**, with 40,000 as a stretch target.
COUNTER targets at least 1,000,000 game ticks/s, and adders must be correct.
A redstone tick is two game ticks. These are measured execution targets, not
compiler-admission or idle-circuit targets.

## What the evidence says

The current COUNTER logical device executes and has measured a median 5.907M
wall game ticks/s over three million-tick samples after greedy discovery was
implemented. Each sample recorded 166,667 logical evaluations and zero
callbacks. Its complete repeater bank also matched native at tested settled
boundaries across starts, stops and the 255-to-256 carry. This is a COUNTER
result, not evidence of FPU activation.

The current saved-plot oracle passed 192 alternating input frames and produced
192 command outputs during the measured interval, but installed **zero** fast
logical devices. Three server samples measured 357.48, 400.83 and 389.36 game
ticks/s, median 389.36, while delivering about 13.3 million callbacks in 1,152
measured game ticks. See [saved-plot benchmark](REDPILER_SAVED_PLOT_BENCHMARK.md).

The immediate frame rejection selects a disconnected terminal wire and finds
native ownership overlap. Excluding that wire merely exposes further overlap.
The broad interaction island merges possible reads and callbacks; it does not
prove one event-driven computation or exclusive ownership. The existing settled
equations also miss actual pulses and produce incorrect FPU settled outputs.
A model that fired every eligible piston overfired thousands of actors: an
eligible piston only acts if an actual update requests it. A shadow fed actual
Graph FIFO actions matched a small sample of output states, but Graph still did
the work and no virtual handoff was proved.

Quiet-world probing can help discover component links and possible ordering.
Those observations apply to the states and inputs actually sampled; they do not
establish saved-FPU correctness or the target throughput. See the [architecture
investigation](REDPILER_REGION_ARCHITECTURE_FAILURE.md).

An isolated two-source channel prototype actually executed 67,788 entries in one
saved-plot server sample and passed that sample's frame-output and ordered
command checks. It installed zero logical devices and reduced callbacks by only
8,154 of roughly 13.3 million (0.061%). A single 416.15 TPS sample does not
establish a speedup. Its native interpreter test and final export fix were not
rerun; the prototype remains outside the workspace. This rules out treating
that isolated channel extension alone as the route to the target.

## Algorithm

Use a greedy, input-driven compiler with explicit boundaries and independently
testable passes. Each pass produces a bounded, inspectable artifact. A later
pass may decline a candidate, but must not silently widen its assumptions.

1. **Discover components and typed dependencies.** Reuse `CompileGraph`, its
   power links, guarded route deliveries, physical contacts and resources.
   Build indexed fans once: input reads, ordered callback/effect occurrences,
   and mutable actor/resource ownership are separate relationships. Do not join
   nodes merely because they are geometrically near or one reads the other.
   Identify real input sources and actual output/native-effect boundaries;
   terminal dust with no observable consumer is not automatically an output.

2. **Grow supported candidates from input events.** Traverse forward from
   each real source through supported instant cells and wire corridors. Merge
   candidates when they truly share mutable physical state, not just a read.
   Every possible writer of a shared payload must belong to one candidate or
   the affected candidate remains native. Stop at unsupported components and
   record their required input/effect boundaries. Independent sources and
   clocks remain independent unless an actual dependency joins them. Then
   trim candidate work that cannot influence an observable output or native
   effect, leaving excluded physical work on Graph where necessary.

3. **Derive links and event order.** Build dense component IDs and bounded
   source-to-consumer links, including attenuation and conditional geometry.
   Probe rises and falls in isolated scratch worlds using the native
   interpreter, retaining constants and actual block properties. Record
   source-qualified target occurrences and order, not merely a final power
   value. Probe relevant movable-payload arrangements and record how they
   change links. Share indexes and cache only where invalidation is explicit;
   cap fan traversal, work, memory, arrangements and scratch size.

4. **Certify the admitted domain.** A quiet probe is a hypothesis. Check its
   assumptions against live inputs, guards, receiver/capture semantics, shared
   resource ownership, pending scheduled work and native boundary effects.
   Include simultaneous or interacting sources and both transition directions
   in the admitted domain, or restrict/decline those cases. An unknown cycle,
   unsupported receiver, missing observation, budget exhaustion or conflicting
   state owner must fall back. Never convert an unproved physical state to a
   constant or treat an unobserved event as impossible.

5. **Lower and execute.** Reuse the existing logical executor and Boolean
   evaluator where their contracts fit. Where they cannot represent a proven
   source-qualified event, extend them with the smallest compact node/link
   program that can: evaluate changed sources, deliver target events in the
   certified order, retain request deduplication and first source when those
   affect results, update virtual piston/resource and delayed reset state, and
   emit required native boundary effects. Collapse internal wires and callbacks
   only when their effect on those outputs and future state is represented.
   If an external effect can change a later read, resume using fresh state or
   fall back at a safe boundary. At handoff, reconstruct the state and pending
   work required for correct native continuation under the admitted contract.

6. **Explain decisions.** For each candidate, record source, selected outputs,
   ownership and unsupported boundary. Give a short clickable block location and
   reason for a representative rejection; do not label a legal piston or a
   circuit as broken merely because its pattern was not recognized. Keep
   diagnostics bounded and distinguish candidate generation, certification,
   installation, and actual execution.

## Delivery and gates

Implement the passes in small slices, using the existing region fan,
`BoundaryCatalog`, Graph interpreter and logical executor instead of adding a
second graph framework. The current `layers::partition` already discovers some
connected cell/corridor regions; audit and reuse it where its contract fits.
The current `SettledFrame` is clock-driven, so a generic input-trigger contract
is also required for clockless candidates. Do not add a frame or clock special
case for the FPU.

1. **Discovery and ownership:** independent and connected devices, shared
   payloads, multiple sources/clocks, unsupported neighbor, dead terminal wire,
   read-only external input. Assert exactly which work is owned or native and
   that failure is local. Test budgets and cancellation.
2. **Probing and lowering:** rising/falling source changes, source ordering,
   multiple active inputs, wire attenuation, dynamic geometry, shared payload
   arrangements and unknown/over-budget fallback. Probes must not alter the
   original world. Compare predicted transitions and effects to native runs;
   do not use the same implementation as both model and oracle.
3. **Actual execution:** prove a compiled region is entered and native internal
   work is materially reduced. Compare declared settled outputs over changing
   inputs, including starts/stops and consecutive frames. Check relevant
   pending work, boundary effects, shared resources and allowed handoff. Test
   unsupported work beside an active compiled region and independent clocks.
4. **COUNTER and ADDERS:** retain the active COUNTER settled-output test,
   including 255-to-256 carry and restarts; run the million-tick benchmark and
   require at least 1M game TPS. Test one-bit and multi-bit adders with changing
   operands, carries and native fallback.
5. **Actual saved FPU:** use a scale-5 build (the workspace now defaults to
   `PLOT_SCALE=5`). Use the corrected saved FPU snapshot and
   changing/random input frames. Check 16 readout repeaters, 35 indicator lamps,
   native boundary outputs and command behavior, including commands emitted
   during measured input changes. Validate permitted settled handoff and native
   continuation. Report installed **and executed** regions, native work
   removed, compile time, all throughput samples, median, revision, executable
   and fixture hashes. Run at least three server samples; 20k–30k is the
   accepted range and 40k is the stretch target.

Do not treat a recognized actor, generated plan, reduced expression count,
shadow match, idle tick rate, or an executor installed without being entered as
success. If a slice only reduces a small fraction of the measured FPU work,
keep it out of the production path unless it is needed by a demonstrated
broader region.
