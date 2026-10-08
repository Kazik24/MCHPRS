# Redpiler partial compilation: project scope

Proposed scope, 2026-10-08. The physical hybrid is not implemented. This document
defines the work and its acceptance gates; it does not extend current admission
or claim replay equivalence. The [execution model](REDPILER_MODEL.md),
[physical piston model](PISTON_MODEL.md),
[ANPU investigation](tests/ANPU_PISTON_DOMAINS.md), and
[boundary design](tests/ANPU_PHYSICAL_DOMAIN_MODEL.md) provide the existing
semantics and evidence.

## Objective

Compile the parts of a circuit whose electrical abstraction preserves its
observable behavior; retain native execution wherever geometry or delivered
updates matter. ANPU Pong is the acceptance workload, not an architectural
special case. The original schematic, origin `(8,8,8)`, and frozen expectations
remain unchanged.

The central rule is **one execution owner per component, one ordered timeline,
and an explicit contract at every ownership boundary**. Compilation chooses an
implementation of existing behavior. It does not reinterpret ordinary movement
as instantaneous or turn data changes into memory samples.

## Architectural decisions

| Decision | Required implementation |
| --- | --- |
| Native physics stays native | Reuse `PistonState`, piston event/movement functions, support checks, and the native wire executor. Physical block/entity storage remains authoritative during movement. |
| Static graph stays static | Keep changing geometry and notification-sensitive dust inside native ownership. Read an affected compiled consumer's input through a port rather than rewrite graph topology on every movement step. |
| Ownership is spatial and behavioral | Determine owners from geometry, possible reads/writes, callbacks, and supported executor semantics. Sticky state, fixture identity, observed inactivity, and a missing instant certificate are insufficient classifiers. |
| Boundary values and events are separate | Updating a cached input is not itself a neighbor notification. Deliver qualifying callbacks with their original direction, order, and multiplicity, even when power is unchanged. |
| Scheduling has one authority | Reuse `TickScheduler<T>` with graph/native work variants and preserve deadlines, priority, FIFO order, duplicate rules, and the native current-slot semantics. |
| Fallback preserves a continuation | Before an unsupported write, materialize compiled state and transfer remaining work once. Continue the same native phase and operation; do not restart the tick. |

One sparse native ownership mask can contain disconnected mechanisms. No
per-island executors, scheduler hierarchy, generalized simulation framework,
separate BUD memory bank, or new dependency is required.

## What is admitted in the first release

Support ordinary empty-head pistons and bounded movement of inert full
conductors or redstone blocks, including sticky pulls, interrupted movement,
and the native no-pull retraction behavior. Start with a single transported
payload per actuator; broaden the admitted line length only if a general test
or the unchanged ANPU replay establishes that it is needed. This is a hybrid
admission limit, not a change to the interpreter's movement rules.

Keep motion-sensitive dust, observers, note blocks, attachment behavior, and
BUD mechanisms native. Compile fixed electrical components only where their
transitions and input/output boundary are supported. Extend the existing port
representation; do not invent a physical piston graph node.

Existing certified instant executors remain available for independent regions
under their existing declared protocols. In v1, a physical/instant coupling
must be absorbed entirely into native ownership or rejected. Retaining an
instant region requires its certificate and geometry contract to remain valid.
An explicit `--assume-instant` request must retain its documented logical
semantics; physical fallback must not silently satisfy an instant-only request.
Normal compilation selects verified graph/instant owners and retains admitted
ordinary mechanisms natively; this needs no extra partial-compilation mode.
Native ownership is explicit admission, not a catch-all for compiler errors.

Movement of another piston, a graph binding, an instant-owned component, or
unsupported entities is outside v1. A foreign base at Far is not such movement
when the actuator's empty Near leaves it untouched. Unsafe interactions remain
explicitly rejected at admission or trigger a safe runtime handoff.

## Planning a safe cut

Plan ownership before lowering and optimizing the graph:

1. Inventory actuator geometry, active motions, retained entities, pending
   events, native input reads, and update dependencies. Use existing analysis
   and payload-walk rules. Separate failure to obtain an instant certificate
   from invalid/unloaded physical context.
2. Seed native ownership with ordinary actuators and their admitted movement
   envelopes. Include all possible intermediate states, not just settled poses.
3. Close ownership over affected dust propagation, shape/support changes,
   observers, and identity-changing components. Potential wire notifications
   matter even across currently nonconducting gaps. A turbo wire walk must never
   execute graph-owned dust internally.
4. At each candidate cut, account for complete consumer input reads and native
   notification delivery. Where the contract cannot be established, retain the
   component natively and repeat closure. Include geometry cells used as
   conductors, supports, or watched positions in the appropriate guards.
5. Preserve every graph source read by native work. Track dependencies whose
   ordering can affect a physical callback or a later scheduled operation.
   Retain identities and native emission order throughout those causal paths.
6. Lower the remaining graph and verify that its ownership, ports, and optimizer
   transformations still satisfy the planned cut before publishing it.

Closure grows monotonically within the admitted loaded context and an existing
analysis budget. Budget exhaustion, cancellation, selection escape, or invalid
entities must return concrete diagnostics, leaving the world unchanged.
Initial geometry plus runtime preflight defines the admitted movement envelope;
the planner need not solve every possible future program history.

The closure may encompass the entire circuit. That is correct native execution,
but counts as zero acceleration. Report it transparently rather than calling it
a compiled ANPU success. V1 does not repartition or recompile at runtime; an
unsafe change ends compiled ownership and hands the whole active compiler back
to the interpreter.

The current read-only probe puts 19,809 of ANPU's 34,781 dust cells inside a
candidate notification closure. That supports investigating a useful cut; it
does not establish safe graph size, admission, or a speedup.

## Boundary and optimization contract

For graph-to-native delivery, publish the boundary source's block state and
analog entity strength before native reads, then use the source's native
notification semantics. Publication is simulation state synchronization and
must work independently of `--io-only`, display flushing, and client frequency.
Locked repeater state and watched block properties also need current values
where native reads can observe them.

For native-to-graph delivery, replace an entire affected consumer channel with
its native-read aggregate. Remove ordinary links into that replaced channel to
avoid double accounting. Read comparator main/side/rear-override semantics and
repeater locking through the existing native rules; expose small shared helpers
where needed. The current `ConsumerInput` descriptor does not include repeater
locking and must be extended if locking crosses the cut. Rear override handling
must not apply a stale static override to an already resolved native input.

Refresh input caches silently and invoke the receiving component's update once
for each delivered callback. At a due component tick, refresh native-read inputs
again: geometry or QC data may have changed without a qualifying callback.
That refresh must not synthesize an earlier sample or schedule extra work.

Same-strength emissions, observer state changes, source-specific skipped
notifications, and callback directions are part of the contract. Preserve graph
and native consumer delivery together in the original synchronous order. A
native notification may reenter the graph before the source finishes notifying
its other neighbors. Do not batch all graph consumers ahead of native consumers.

Current graph fanout grouping and strength-change propagation do not meet that
contract automatically. The first vertical slice must establish an ordered
emission path, or move the affected source/path into native ownership. Release
the backend's mutable operation borrow before native callbacks reenter it;
commit source state first and dispatch effects at their original execution
point. A tick-end callback buffer changes semantics.

Protect the entire relevant causal path from rewrites that merge identities,
remove unchanged-value notifications, or change insertion order. Merely pinning
the boundary node is insufficient. Existing fast graph updates and optimizer
passes can remain where these changes are unobservable under the cut contract.
If a compiled component's state machine differs observably from native behavior,
retain it natively until that particular behavior is supported.

## Scheduling and lifecycle

Extract a shared operation driver from the existing native phase logic, with
owner dispatch for scheduled work. Keep the graph-only fast path when there is
no physical domain. A mixed run advances the clock once and preserves:

1. Native current-slot work, then cursor advancement and due scheduled work,
   including synchronous callbacks and newly scheduled current-slot work.
2. Piston events in queue order, including events enqueued during this phase.
3. An identity-checked snapshot of moving entities, with native progress and
   completion. Events created during completion wait for the next event phase.
4. Return to `BetweenTicks`. Independent certified instant deadlines retain
   their established execution points.

Import/export must preserve expected registry IDs on physical ticks and their
relative FIFO order with graph work. Do not rebind a stale base tick to a moving
block. Preserve pending lookup and deduplication behavior by owner; native tick
deduplication and compiled pending flags are not interchangeable.

Stage compilation without world mutation, then transfer queue ownership only
after success. V1 activation occurs at `BetweenTicks`; an in-flight motion at
that boundary is admitted only if its full footprint and entities pass native
ownership checks. Mid-operation activation is rejected without consuming work.

Use a world-aware compiled entry point for ticking and user actions. Route
buttons, levers, plates, and their scheduled releases by owner. Player edits and
other geometry mutations hand off before an affected compiled invariant breaks.
Native `interaction::change` and block/entity writes also require ownership
guards; guarding only `schedule_tick` does not prevent duplicate execution.
Analysis should report owner counts, ports, retained causal paths, and why a
component stayed native. Runtime handoff reports its operation and violated
invariant. Extend existing diagnostics/statistics rather than add a new
telemetry system.

Preflight each potentially mutating piston event/completion against the same
movement/support rules used by execution. Reuse the native payload traversal;
extract its operation footprint if necessary rather than implementing another
movement planner. An unsupported footprint must be detected before its first
write, including writes caused by forced completion of an existing motion.

Reset and fallback export graph state/work without synthetic callbacks, retain
physical entities, motion identities/progress, events, phase, and cursor, then
resume native execution. Invalidate/rebuild derived lookup caches while keeping
the same duplicate suppression behavior. A dequeued operation that
failed preflight remains the next unexecuted operation. Materialization must
touch only graph/instant-owned cells. Explicit reset-with-update is a separate
requested resampling action; it must not be confused with transparent handoff.
Preserve existing restrictions on nano/pico stepping while compiled. No new
save format, history format, or live migration protocol is in this project.

## Implementation gates

Each gate is an executable deliverable. Do not weaken admission to reach a later
gate before the earlier boundary behavior is established.

| Gate | Deliverable and acceptance |
| --- | --- |
| 1: shared execution | Reuse the native operation driver and mixed scheduler. Differentially verify native-only phase traces, current-slot scheduling, priority/FIFO order, deduplication, and registry-ID checks. Existing graph-only tests remain valid. |
| 2: first safe cut | Geometry-derived native mask; a compiled delayed source driving an empty ordinary actuator, plus a native source driving a compiled fixed consumer. Compare native operation/callback traces, not just final extension. Prove reentrant ordered dispatch and no duplicate work. |
| 3: changing power and memory | Admit one concrete/redstone-block payload. Check conduction/supply throughout movement, actual BUD sampling with separate QC data, unchanged-strength comparator emissions, held data without callbacks, and simultaneous ordered sources. |
| 4: lifecycle | Reset at pending event, progress 0.5, progress 1.0, and completion-generated work; continuation matches uninterrupted native execution. Exercise unsupported crossings and activation failure transactionally, including forced-completion preflight. |
| 5: ANPU | Admit from geometry/behavior; pass unchanged 50,000-tick checkpoints and ordered chat, all 14 Pong screen frames, and all 1,552 frozen physical BUD sample-tick records. Verify an active hybrid reset followed by interpreter continuation. |
| 6: release | Run existing redstone/compiler/instant/plot regressions, then three sequential release samples against the interpreter on the same machine, workload, and publication policy. Publish ownership counts, rejection/handoff reasons, correctness results, and timings. |

Use small synthetic mechanisms for boundary guarantees. Translation/rotation,
sticky/non-sticky roles, payload versus source-base identities, both crossing
directions, and optimizer on/off should be covered where they expose different
behavior. ANPU provides an integration oracle, not the general admission rule.
Test fallback with an otherwise native-supported operation so interpreter
continuation is compared, not merely a rejection message.
Update admission tests for mechanisms that acquire a verified native owner;
retain rejection coverage for unsupported cases and all frozen physical
expectations.

Prefer narrow commits along these gates. Expected source touch points are
analysis/ownership and graph boundaries; Direct input/commit/emission and queue
handling; the native update/interaction ownership router; and Plot lifecycle and
input dispatch. Add a concrete world view/coordinator only as needed to connect
those existing parts. Do not create parallel state machines in each layer.

## Definition of completion

Completion requires a correct mixed ANPU execution with a nonempty compiled
portion of its active electrical circuitry, valid reset continuation, explicit
unsupported cases, and
preserved existing compiler contracts. Admission alone, a native-only fallback,
or matching final Pong output is insufficient.

Report active-window TPS for ticks 1..5,000 separately from the 50,000-tick
average; show all three samples and the median. Timings must include mandatory
boundary publication, callback routing, physical movement, world mutations, and
normal dirty-state bookkeeping. State whether synchronous action encoding,
display/section flushing, packet encoding, and network delivery are included.
Match these choices in the interpreter baseline; observation and fixture loading
stay outside the execution timer. A slowdown is a measured result, not grounds
to relax semantics. Speedup and the safe fraction of ANPU remain open until gate 6.

The scope is complete as a proposal. Implementation gates 1 through 6 remain
open; the existing inspector and native replay evidence are prerequisites.
