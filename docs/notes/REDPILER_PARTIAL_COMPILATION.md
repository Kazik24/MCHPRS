# Redpiler execution generalization: project scope

Consolidated scope, 2026-10-08: committed BUD presentation, generalized
instant notification delivery, and partial native/compiled execution. This
document defines their common contract, delivery order, and acceptance gates.
BUD presentation is implemented; general notification execution and native
boundaries remain proposed. Current changes do not extend admission or claim
PM1 replay equivalence. The [execution model](../REDPILER_MODEL.md),
[physical piston model](../PISTON_MODEL.md),
[ANPU investigation](../tests/ANPU_PISTON_DOMAINS.md), and
[boundary design](../tests/ANPU_PHYSICAL_DOMAIN_MODEL.md) provide context. Current
source and runnable regressions take precedence over historical descriptions.

## Objective

Compile the parts of a circuit whose abstraction preserves observable behavior;
retain native execution wherever that proof is missing. Support independently
controlled notification generators and expose committed supported BUD state.
PM1 and ANPU are integration workloads, not recognition rules. Original
schematics, protocols, and frozen expectations remain unchanged.

The common rule is **power determines the desired response; qualified
notification delivery determines when stored state may change**. Apply it with
**one execution owner per component, one ordered timeline, and an explicit
contract at every ownership boundary**. Presentation reads committed state;
it never creates a sample. Compilation chooses an implementation of behavior,
with internal effects collapsed only under a verified observation contract.

## Current implementation and actual gaps

[Preparation](../../crates/core/src/redpiler/instant/program.rs) unconditionally
extracts ideal logic, and the active
[runtime](../../crates/core/src/redpiler/backend/direct/instant.rs) uses cached
logical decisions. Default compilation and `--assume-instant` already share
execution semantics; the flag relaxes construction proofs. The
[counter regression](../../crates/core/src/redpiler/analysis/tests/ideal.rs)
checks the same atomic six-step commits, hold, and restart under all four
optimization/assumption combinations. Preserve this contract.

Some reference documents still describe older wave and sequential runtime
paths. The retained sequential files are extraction/dependency helpers, not
an active alternative executor. Extend the current logical sampling path;
do not revive a second physical simulator based on those descriptions.

The relevant gaps are concrete:

- [Clock recognition](../../crates/core/src/redpiler/instant/clocked.rs) runs before
  independent sampling and rejects multiple clock-shaped candidates. A failed
  specialization currently prevents a general notification interpretation.
- [Independent sampling](../../crates/core/src/redpiler/instant/sampling.rs) already
  represents multiple generators and recipients, but restricts dust writers,
  rejects stored-state generator control, and does not establish every valid
  observer/dust route.
- Runtime delivery flags collapse repeated deliveries within an evaluation;
  coordinate-sorted writer groups do not establish callback order. Ordinary
  graph propagation also suppresses equal-strength emissions and groups fanout
  by node type. These are correctness gaps, not generator-count limits.
- [Flush](../../crates/core/src/redpiler/backend/direct/mod.rs) now publishes
  committed BUD poses independently of observer bookkeeping. Manual `/adv`
  flushes compiled state and delivers block changes after nonzero advancement.
- Native and Direct execution have separate phase drivers and scheduler
  ownership. There is no mixed callback router or operation-preserving handoff.

The [recorded PM1 investigation](../tests/CPU_REFERENCES.md#fresh-pm1-compilation-save-2026-10-08)
found 51 candidate generators in one region. Normal analysis exhausted the
piston budget; maximum analysis reached the single-generator rejection. An
independent classifier probe failed at selection-local `(9,51,0)` because it
had not proved a notification source. These are historical diagnostics, not
proof that the circuit is invalid or results for a future download. A subsequent
hash-matching download and native perturbation identify a real base-to-head
route from local `(8,50,0)` to `(9,50,0)`. A queued target retraction is cancelled
when power returns before event acceptance. Discovery now records that route
and its head eligibility, but instant execution lacks the required ordered
movement/reset timing certificate. See the
[route evidence](../tests/CPU_REFERENCES.md#pm1-base-to-head-route-and-cancelled-request-2026-10-08).

## Common execution contract

Keep power dependencies and notification routes separate. A desired piston
response is a cached expression of current strengths, committed stored state,
and admitted geometry. A notification carries its source, target, callback
kind/direction, eligibility, multiplicity, and execution order. Refreshing a
power cache does not deliver that notification.

Delivery may queue a request. Native execution can recheck power before
accepting the event; notification, queued request, and accepted commit are
different observations. An instant executor may collapse that episode only
after proving that no admitted consumer observes its intermediate effects.
Head callbacks qualify only while the corresponding head exists at delivery.

Committed stored state can control generators without becoming combinational
feedback: expressions read the latest committed state at the start of each
delivery; a certified atomic transaction reads its declared old-bank snapshot.
Pure expression dependencies must remain acyclic. A same-step notification
loop requires a proved finite episode or an existing observable delay; an
unproved loop is rejected or assigned to a supported native owner.

An atomic old-bank transaction requires a sampling certificate. One writer,
one connected region, or one piston facing is insufficient to prove it.
Otherwise preserve individual delivery/acceptance order, including repeated
and unchanged-value callbacks. The shared-clock adapter remains a specialization
selected only after its control, recipients, sample timing, stop/restart, and
old-bank contract are established.

Choose ownership from possible observable reads, writes, timing, and callbacks.
A certified instant BUD owns its committed logical bit and virtual geometry;
a native BUD owns actual piston state, events, movement, and physical geometry.
The hybrid creates no additional memory bank. Two executors never write the
same owned component. Failed specialization can try a general supported path;
invalid geometry, entities, context, or pending work cannot be bypassed.

Separate three operations throughout the implementation:

| Operation | Required timing and effect |
| --- | --- |
| Simulation commit and notification | At the established execution point; update logical/native state and deliver observable effects in order. |
| Boundary state synchronization | Before native reads/callbacks, independent of clients, display cadence, and suppression flags. |
| Client presentation | At the existing display cadence; project committed state and reuse ordinary packet batching. It cannot drive simulation. |

## Delivery packages and order

| Package | Smallest complete result | Dependency |
| --- | --- | --- |
| P: committed BUD display | Show both poses for currently admitted independent cells and shared-clock banks, including paused `/adv`. | Existing logical state; can ship independently after the live-world-read audit. |
| N: general notification execution | Admit separately proved generators/routes/writers and preserve their ordered deliveries through cached logic and optimization. | Common event/commit contract and small interpreter comparisons. |
| H: partial compilation | Compile a verified static electrical cut around supported native mechanisms. | The same ordering contract plus the six native-boundary gates below; it need not await PM1 admission. |

First trace the missing PM1 route and native delivery/acceptance ordering in
small mechanisms. Ship P without broadening admission. Develop N and H against
the same delivery rules and scheduler primitives, but require each to pass its
own integration oracle. PM1 success does not prove a native boundary; ANPU
native replay does not prove an instant sampling certificate.

The proposed 1–2 developer days applies only to P for already admitted cells,
including validation. It remains provisional until live-world readers are
checked. N and H need discovery and the first ordered slice before a credible
effort estimate; neither is a small relaxation of the generator-count guard.

## P: committed BUD presentation

Implemented in the current runtime, with memory and client packet regressions.
The following describes the contract retained for further changes.

Use recognized memory cells and the committed bit to project base, near, and
far positions through the runtime's existing geometry/`observed_block` rules.
An extended cell has its stationary head at near and payload at far; a
retracted cell has its payload at near and air at far. Preserve direction,
sticky/head properties, and the original payload material.

Track the last published bit separately from observer bookkeeping. Initialize
it from the admitted saved pose and write only cells whose committed pose
changed. An initial linear scan is acceptable; add a changed-cell queue only
if measured bank size justifies it. The new presentation step must not consume
notification tracking or create extra samples, semantic outputs, or pending
work. Existing flush evaluation and output delivery remain separate operations.

Publish after logical evaluation using ordinary block writes and existing
packet collection. Do not invoke physical redstone updates or `materialize()`:
that routine also performs interpreter handoff. Extend manual advancement to
flush compiled state and deliver block changes after its tick batch. Reuse the
existing normal-tick and interaction flush paths.

`--io-only` prevents internal BUD presentation writes; `--optimize` alone does
not. Screen-only continues its existing packet suppression/storage-overlay
behavior: update storage so disabling it and chunk snapshots see the last
published pose. A chunk snapshot between commit and the next display flush
sees that previous published pose. No new chunk resend protocol is needed.
Native-owned movement continues through its existing physical presentation.

Publishing geometry requires canonical reads where display frequency could
affect simulation. Notes beneath owned memory now capture obstruction from
committed geometry at the power-rise event, before any later pose changes or
flush. Ordinary notes preserve their live-world fallback. Deferred sound
delivery therefore retains eligibility across multiple intervening commits,
including when `--io-only` suppresses geometry writes.

Acceptance covers both memory mechanisms and poses, data-only changes without
notifications, repeated/no-change flushes, stop/recompile, chunk reload, paused
manual advancement, optimization, `--io-only`, and screen-only. Compare
frequent/deferred flushing for committed bits, observer events, semantic
outputs including note eligibility, and remaining scheduled work. Include
admitted active-clock entry: first evaluation can establish a sample deadline.
Packet tests must establish delivery of the three positions, not merely updated
storage.

Exclude smooth motion, internal dust/reset animation, new commands/dependencies,
and new circuit admission. Update the current execution description to name
visible supported BUDs while other hidden internal components retain saved
appearance. Do not describe native in-flight geometry as a Boolean projection.

## N: generalized instant notification execution

Discover qualifying routes from supported base/head changes, observers, and
dust independently of power paths. Reuse topology, update-port, region,
sampling, observer, and Boolean extraction helpers. Trace every rejected route
to its actual writer and callback semantics; ambiguous ordering remains a
diagnostic until proved. Do not special-case coordinates, schematic identities,
or presumed arithmetic functions.

Extend the existing prepared sampling descriptors and runtime deliveries to
retain repeated callbacks, multiple valid writers, recipients, head eligibility,
and actual source order. Source identity alone is not a transaction boundary.
Drive delivery at the source operation, not by comparing only final strengths
at tick end. Reuse cached expressions and `TickScheduler`; retain observer
deadlines and any timing visible downstream instead of implementing another
movement engine.

General preparation precedes optional shared-clock specialization. Failure to
prove one clock can fall through to a separately proved notification program;
it cannot erase ownership or construction checks. Allow generator controls to
read committed memory while retaining acyclicity checks for pure expressions
and rejection of unproved immediate update loops.

Protect every upstream path whose identity or order can affect sampling from
optimizer rewrites that merge deliveries, discard same-value notifications,
or reorder callbacks. Boundary-node pinning alone is insufficient. Preserve
geometry, payload, entities, attachments, selection context, pending work,
budgets, cancellation, and construction validation. Default and
`--assume-instant` continue to use the same execution semantics; the flag only
changes construction-proof requirements, never notification qualification.

| Gate | Acceptance |
| --- | --- |
| N1: delivery evidence | Trace the missing route at local `(9,51,0)` and representative burst generators; distinguish callbacks, requests, accepted events, and commits. Produce small ordered interpreter traces before changing admission. |
| N2: generic execution | Two separately controlled generators in one region; multiple recipients and writers; unchanged data/sample deliveries; head callbacks; stored-state control; stop/restart and delayed feedback. Compare ordered semantic observations, not only final bits. |
| N3: preserved contracts | Counters, instant gates, memory, flush, observers, reset/handoff, option combinations, and transactional rejection pass. Keep shared-clock extra-writer rejection as a specialization check while testing any separately admitted generic route. |
| N4: PM1 | Re-download and hash the current schematic, run the eight flag/budget attempts, then replay and measure every admitted configuration as specified below. Rejection with a precise cause remains a reported blocker, not completion of general execution. |

## H: native ownership and partial compilation

The following boundary design and six gates retain the original partial
compilation scope. ANPU uses its unchanged schematic, origin `(8,8,8)`, frozen
checkpoints, command output, screen frames, and physical sample records.

### Architectural decisions

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

### What is admitted in the first release

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
`--assume-instant` still only relaxes construction proofs for an instant owner;
it must not substitute physical fallback for a missing logical sampling
contract. Admitted native mechanisms retain their native semantics with either
flag, and a common admitted configuration must keep the same observation trace.
Normal compilation selects verified graph/instant owners and retains admitted
ordinary mechanisms natively; this needs no extra partial-compilation mode.
Native ownership is explicit admission, not a catch-all for compiler errors.

Movement of another piston, a graph binding, an instant-owned component, or
unsupported entities is outside v1. A foreign base at Far is not such movement
when the actuator's empty Near leaves it untouched. Unsafe interactions remain
explicitly rejected at admission or trigger a safe runtime handoff.

### Planning a safe cut

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

### Boundary and optimization contract

For graph-to-native delivery, publish the boundary source's block state and
analog entity strength before native reads, then use the source's native
notification semantics. Publication is simulation state synchronization and
must work independently of `--io-only`, display flushing, and client frequency.
Locked repeater state and watched block properties also need current values
where native reads can observe them.

Mandatory publication must also preserve the client delta policy: native reads
need current simulation state, but that must not expose suppressed internal
visuals. Check `--io-only` and screen-only separately, including chunk reload;
screen-only snapshots retain their existing authoritative-storage semantics.

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

### Scheduling and lifecycle

Extract a shared operation driver from the existing native phase logic, with
owner dispatch for scheduled work. Keep the graph-only fast path when there is
no physical domain. A mixed run advances the clock once and preserves:

1. Native current-slot work, then cursor advancement and due scheduled work,
   including synchronous callbacks and newly scheduled current-slot work.
2. Native random copper ticks at their existing point, with the same input
   randomness when comparing executions.
3. Piston events in queue order, including events enqueued during this phase.
4. An identity-checked snapshot of moving entities, with native progress and
   completion. Events created during completion wait for the next event phase.
5. Return to `BetweenTicks`. Independent certified instant deadlines retain
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

### Implementation gates

Each H gate is an executable deliverable. Do not weaken admission to reach a
later gate before the earlier boundary behavior is established. These gates
share delivery invariants with N but do not require PM1 to compile first.

| Gate | Deliverable and acceptance |
| --- | --- |
| H1: shared execution | Reuse the native operation driver and mixed scheduler. Differentially verify native-only phase traces, current-slot scheduling, priority/FIFO order, deduplication, and registry-ID checks. Existing graph-only tests remain valid. |
| H2: first safe cut | Geometry-derived native mask; a compiled delayed source driving an empty ordinary actuator, plus a native source driving a compiled fixed consumer. Compare native operation/callback traces, not just final extension. Prove reentrant ordered dispatch, no duplicate work, and simulation publication independent of display suppression/cadence. |
| H3: changing power and memory | Admit one concrete/redstone-block payload. Check conduction/supply throughout movement, actual BUD sampling with separate QC data, unchanged-strength comparator emissions, held data without callbacks, and simultaneous ordered sources. |
| H4: lifecycle | Reset at pending event, progress 0.5, progress 1.0, and completion-generated work; continuation matches uninterrupted native execution. Exercise unsupported crossings and activation failure transactionally, including forced-completion preflight. |
| H5: ANPU | Admit from geometry/behavior; pass unchanged 50,000-tick checkpoints and ordered chat, all 14 Pong screen frames, and all 1,552 frozen physical BUD sample-tick records. Verify an active hybrid reset followed by interpreter continuation. |
| H6: release | Run existing redstone/compiler/instant/plot regressions, then three sequential release samples against the interpreter on the same machine, workload, and publication policy. Publish ownership counts, rejection/handoff reasons, correctness results, and timings. |

Use small synthetic mechanisms for boundary guarantees. Translation/rotation,
sticky/non-sticky roles, payload versus source-base identities, both crossing
directions, and optimizer on/off should be covered where they expose different
behavior. ANPU provides an integration oracle, not the general admission rule.
Test fallback with an otherwise native-supported operation so interpreter
continuation is compared, not merely a rejection message.
Update admission tests for mechanisms that acquire a verified native owner;
retain rejection coverage for unsupported cases and all frozen physical
expectations.

Prefer narrow commits along these gates. Add a concrete world view/coordinator
only as needed to connect the existing parts. Do not create parallel state
machines in each layer.

## Change surface

Trace these callers before implementation; use existing interfaces and fixtures.
No new command, dependency, save format, or binary export is in scope.

| Surface | Existing code and required reach |
| --- | --- |
| Recognition and admission | `analysis/{ports,topology,families,mod}.rs`; `instant/{program,regions,clocked,sampling,observer,logic,boundary,outputs}.rs`; shared geometry and payload checks. |
| Runtime and graph semantics | `backend/direct/{instant,compile,mod,update,tick}.rs`, `backend/direct/instant/logical.rs`, `backend/queue.rs`, and the causal paths through `passes/`. Preserve command, note, observer, and scheduled effects. |
| Native ownership | `World`, `redstone::update`, piston preflight/events/movement, `interaction::change`, native/turbo dust, and scheduler lookup/deduplication. |
| Plot and client lifecycle | Activation, normal ticking, manual `/adv`, lever/button/plate dispatch, reset, edit barriers, world storage, screen overlays, chunk snapshots, and outbound packet collection. |
| Regression and research | Analysis memory/ideal/observer/region/output/admission tests; physical piston tests; plot/client and network packet tests; existing PM1/ANPU research and CPU replay/benchmark helpers. |
| Documentation and artifacts | Execution model, parser, optimizer, architecture, client sync where changed, and test reproduction docs. Keep original schematics and frozen reference files; write new diagnostics to separate output paths. |

Historical rejection strings are not contracts once a separately proved owner
is admitted. Keep tests for genuinely unsafe cases, update only the applicable
admission expectations, and never change frozen behavioral expectations to
make a replay pass. Refresh stale ignored admission matrices before citing
them as evidence for the current source.

## Integration evidence and performance

After the small N comparisons pass, download the latest PM1 schematic again
from its established source. Record SHA-256, source revision/time, selection
dimensions/offset, paste origin, start/stop positions, and relevant saved-state
differences without overwriting prior artifacts. Retry default, `-O`,
`--assume-instant`, and both flags at normal and maximum existing analysis
budgets. Report compile-call time separately from loading, preparation, and
builds; every failure includes selection-local coordinates where applicable
and the exact failed resource, ownership, or semantic proof. Do not turn a
resource limit into a claim that the circuit is invalid.

For each admitted configuration, run the existing 50,000-tick start episode
and 100-tick manual-stop tail against the interpreter from the same saved
state. Compare complete ordered, tick-stamped command output and halt behavior,
plus declared memory/boundary observations. Preserve older frozen fixtures as
separate unchanged regressions. A changed fresh save needs its own separately
recorded interpreter comparison; it cannot inherit or replace an earlier
save's frozen expectations.

Account for representation differences explicitly. Compare native physical
records exactly where native ownership is retained. For virtual instant
geometry, declare the committed projection and observation times before the
run; inspect disagreements in phase, head, payload, or ownership instead of
masking all piston cells or normalizing output to fit expectations.

Measure three sequential release samples per admitted benchmark configuration
and matching interpreter baseline on the same machine and saved workload.
Report each sample and median, compilation time, total 50,000-tick time, the
declared active window, ownership/graph counts, and outstanding limitations.
Reuse the fixture's established active window where valid; establish and
report one from the fresh interpreter trace before timing a changed save.
Compilation, loading, preparation, and reference observation stay outside
execution timers. Always include mandatory boundary publication and state
bookkeeping; state explicitly whether display flush, action/packet encoding,
and network delivery are timed. Match those choices in the baseline. Display
may coalesce short-lived states at high TPS without coalescing simulation
notifications.

## Definition of completion

P requires correct visible committed poses and delivery, with unchanged
simulation under different flushing and suppression policies. N completion
requires the small independent-generator comparisons, preserved existing
semantics, and admitted PM1 compilation/replay/performance evidence. Report
precise blockers if that gate cannot pass. An unproved route must remain
rejected; a blocker report is an incomplete integration gate, not an excuse
to bypass its checks.

H completion requires a correct mixed ANPU execution with a nonempty compiled
portion of its active electrical circuitry, valid reset continuation, explicit
unsupported cases, and
preserved existing compiler contracts. Admission alone, a native-only fallback,
or matching final Pong output is insufficient.

For ANPU, report active-window TPS for ticks 1..5,000 separately from the
50,000-tick average; show all three samples and the median. Timings must include
mandatory boundary publication, callback routing, physical movement, world mutations, and
normal dirty-state bookkeeping. State whether synchronous action encoding,
display/section flushing, packet encoding, and network delivery are included.
Match these choices in the interpreter baseline; observation and fixture loading
stay outside the execution timer. A slowdown is a measured result, not grounds
to relax semantics. Speedup and the safe fraction of ANPU remain open until H6.

The consolidated scope is complete as a proposal. P, N1 through N4, and H1
through H6 remain implementation/validation work. Start with notification-route
evidence and P's world-read audit; the first H slice then establishes whether
the ordered boundary leaves a useful compiled cut.
