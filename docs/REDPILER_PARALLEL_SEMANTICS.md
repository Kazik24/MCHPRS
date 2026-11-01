# Parallel Redpiler execution semantics v1

Status: normative specification for the proposed `/rp compile --parallel` runtime. No executor implementation is implied by this document. The serial runtime keeps its existing behavior.

## 1. Time, identity, and observable behavior

A parallel **update round** is a synchronous state transition with no elapsed simulated time. It is distinct from native `nanotick_advance`, whose phase snapshot is a count of operations selected using live queues; see [REDSTONE_MODEL.md](REDSTONE_MODEL.md#152-pico-and-nano-stepping).

Logical game ticks, scheduler half-tick delays, block-type bindings, tick priorities, piston landing deadlines, and publication options retain their current units. A round never advances a scheduler cursor or motion clock. Wall-clock scheduling, Rayon worker count, task size, and shard assignment have no semantic meaning.

Component, token, contact-group, contribution, and cell IDs are immutable for the compiled program. Canonical component order is the program's component ID order. Every pending event has a monotonically allocated event ID. Initial scheduled events receive IDs in existing scheduler export order. External controls receive IDs in the order accepted by the plot thread. Generated event IDs are allocated at commit using the effect order below, never by worker races.

Given the same compiled program, entry state, and accepted external controls, all worker counts must produce identical committed states and emitted event sequences. Parallel mode deliberately permits differences from serial/native execution for circuits sensitive to within-round ordering.

## 2. Complete state and event categories

Committed state `S` comprises component blocks, comparator and command state, all fixed/conditional contribution slots and input aggregates, token occupancy/positions, joint arrangements, piston flags/motions, logical clocks, scheduled queues, pending piston requests, immediate event queues, dirty state, queued output, and resource accounting. Pending queues are part of state, not derived from block values.

Events have a target, category, stable event ID, and typed payload:

| Category | Meaning | Coalescing |
| --- | --- | --- |
| `Reevaluate` | Re-read current inputs after an ordinary delivered update | At most one pending marker per target within one causal settling episode |
| `Notify` | A qualifying notification whose occurrence/direction/shape cause matters, including observer callbacks | Retain each occurrence and its payload |
| `Scheduled` | One typed scheduled entry with deadline and priority | Retain each entry; producer-specific deduplication still applies |
| `PistonRequest` | Extend, retract, or early retract request with actor identity | Existing per-actor/action deduplication |
| `MotionComplete` | A due motion identity/part, validated when executed | Retain distinct identities; ignore stale identities safely |
| `Control` | Accepted external lever/button operation | Retain each accepted operation |

The preparation pass must classify notification handlers explicitly. Default an unclassified callback to `Notify`; do not assume callbacks are redundant just because the target block or aggregate input is unchanged. Directional exclusions and qualification predicates remain those of the current component logic. For example, an observer's qualifying callback is an event even when the watched block value compares equal.

State assignments are not events. Notes, command emissions, scheduled entries, and accepted mechanical requests are events/effects and cannot be discarded when equal state writes are combined.

## 3. Atomic round transition

For an eligible frontier `F`, a successful round computes `S_next = commit(S, evaluate(S, F))`:

1. Select at most one event per target component. Selection is deterministic, as defined below. Selected entries remain logically reserved until successful commit.
2. All evaluators read exactly `S`, with selected scheduled entries considered removed for pending-count queries. Unselected entries remain pending. Evaluators may use private scratch values for one handler; they cannot observe another evaluator's writes.
3. A handler emits proposed state writes, contribution replacements, schedules, notifications, outputs, or piston proposals. It never recursively calls another component handler. A read requiring an emitted write to become visible becomes a subsequent round/event dependency.
4. Merge effect streams by `(producer component ID, selected event ID, handler-local effect ordinal)`. The local ordinal follows that handler's specified scalar operation order. This order assigns IDs and resolves explicitly permitted per-producer sequences; it does not expose partially committed inputs to other evaluators.
5. Validate mechanical claims, single-writer invariants, arithmetic, queue capacities, output policy, and work budgets before making changes visible.
6. Commit all admitted writes, input aggregates, queues, counters, and outputs atomically at the semantic level. Parallel disjoint commits are permitted only after validation and before any next-round readers.
7. Capture final source-state transitions and explicit delivered notifications into next-round queues. Clear consumed reevaluation markers before enqueueing new ones, so self-notification can cause another round.

If validation fails, commit none of that round. Retain its reserved events and the preceding committed state, record a deterministic structured error, and stop. There are no host writes, sounds, command messages, or partial contribution updates from the failed round.

At most one selected event per component prevents duplicate same-component handlers from both evaluating against the same old state. Distinct events on that component are processed in successive rounds, with their multiplicity and local order intact.

### Selecting immediate events

Within an immediate settling episode, choose each component's oldest event ID across its `Notify` and coalesced `Reevaluate` markers. A reevaluation marker keeps the first triggering ID; subsequent triggers merge into it. Select the oldest available event for every component concurrently. Other events remain queued.

Multiple causes may be represented by one reevaluation only if the handler is classified as level-only. A callback that schedules a pulse or reacts to delivery rather than input level must remain a typed notification.

There is no phase-wide `updated_in_phase` suppression in this model. Such suppression could prevent a required reevaluation in a later round. Deduplication applies to pending reevaluation markers; after a marker is consumed, a subsequent delivered update can queue it again.

## 4. Input and wire semantics

Each contribution slot has one logical producer and one target owner. Evaluation writes replacements, not deltas to a shared aggregate. If one handler writes the same slot repeatedly, retain its final value for state commit; retain explicit non-state effects separately. Different producers writing one slot is a preparation error or runtime invariant failure, not a last-writer choice.

At commit, replace each changed slot once. For a target/channel, update the active count and analog histogram from old slot values to final new slot values, then expose the complete aggregate to the next round. Distinct source contributions remain distinct even when strengths are equal. Check count underflow/overflow.

A joint arrangement has one transition owner. All token changes admitted in a mechanical round determine its final arrangement before conditional contributions are recomputed. Do not independently add/subtract arrangement deltas against stale shared state. Each conditional contribution has one final replacement for that committed arrangement.

For every changed source output, update affected contribution values from its committed new output and the committed arrangement. A newly active conditional link uses the committed source output, including a source change in the same transaction. These edge calculations run as commit preparation from proposed final source/geometry values; they do not invoke component handlers. Geometry-dependent source/link metadata must be complete at admission.

When wire inputs change or an explicit wire notification occurs, queue a wire event. A wire event reads the previous barrier's aggregate and proposes its new power. The next commit updates wire output contributions, and downstream readers run in later rounds. This creates causal wire-propagation rounds without assigning artificial elapsed time.

Continue using existing attenuation, analog maximum, diode, override, and component equations. Never reduce analog strengths to booleans merely to simplify parallel reductions. Dependency changes can require a delivered notification even if aggregate strength stays equal; preserve notifications required by the compiled plan.

## 5. Pulse and event visibility

The v1 model samples **committed transitions**. It does not reconstruct transient values that existed only in a serial interleaving or evaluator scratch state.

| Situation | Defined result |
| --- | --- |
| A source rises in one round and falls in a later round | Both source transitions exist; notifications/outputs generated by those transitions remain queued even if the source later changes again |
| Two different sources change simultaneously and a receiver's aggregate remains powered | The receiver reads the final powered aggregate; no artificial off/on pulse is generated |
| One handler proposes multiple state values for itself in one round | Only the final state becomes visible; explicit output or schedule effects emitted by the handler remain events |
| Two scheduled events target the same component | Execute in successive rounds, so the second reads the first committed result |
| Two qualifying observer callbacks target the same observer | Retain both callback events; its existing powered/pending guards determine whether either schedules a pulse |
| A receiver notification arrives after its triggering source has fallen again | Level-sensitive logic reads current committed levels; a typed notification retains its occurrence/direction, not a fictitious old power level |

Noteblocks generate a note for each accepted committed rising transition that satisfies playback eligibility. Capture instrument, note, and eligibility at that transition. Do not use publication cadence as a per-component deduplication window. Copper-bulb latches, observer pulses, and command-block scheduling retain their component-specific rules applied to successive committed states.

Scheduling requests emitted in one round commit in canonical effect order. Apply the same request predicates as the existing component: a deduplicating producer sees earlier accepted requests from its own effect stream in a private scheduling overlay. A generic scheduled queue must not deduplicate distinct entries that were already accepted. Existing entries retain their block-type binding and deadline when sources change.

## 6. Driver, deadlines, and priority

One successful game-tick call follows the existing sampled runtime sequence:

```text
increment motion_time once
drain current scheduler bucket with electrical settling
advance scheduler cursor by one half-tick bucket exactly once
drain the new current scheduler bucket with electrical settling
drain piston requests with electrical settling
process eligible motion completions with electrical settling
finish the game tick and reset its semantic operation budget
```

This describes the current sampled executor's ring behavior; it does not redefine native time units. Preserve existing delay conversion (`game delay -> 2 * half-tick delay`) and the current accepted delay range. In the current code `NanoTick` aliases `Normal`; it is not an extra priority class. Delay zero remains valid only under that existing rule.

### Scheduled drain

1. Finish pending immediate settling first.
2. Select the highest-priority nonempty queue at the current cursor: `Highest`, `Higher`, `High`, `Normal`.
3. From that bucket's current entries, select the first entry per component. All selected entries execute as one frontier. The same component's remaining entries keep FIFO order and remain queued. Different components deliberately share the snapshot.
4. Validate expected block types and pending-count accounting using the existing stale-entry rules. A stale typed entry is consumed without calling the component handler and is charged scheduled-dispatch work.
5. Commit, then settle all generated immediate notifications to quiescence.
6. Reinspect all priorities at the same cursor. A newly queued entry can therefore precede older lower-priority entries, but cannot preempt an already running round or its causal electrical settling.
7. Finish this drain only when the current cursor's scheduled queues and immediate queues are empty. Future buckets do not prevent quiescence at the current time.

An entry committed while draining uses the cursor current at its scheduling point. Positive delay cannot run early. Zero delay runs during this drain if it is still the scheduled phase. Zero-delay entries generated during piston/motion phases remain in the current bucket for the next game-tick call's first drain; the driver does not jump backward to a completed phase.

Piston requests generated during either scheduled drain wait for the piston phase. Piston requests generated during motion completion wait for the next game tick's piston phase. Immediate electrical notifications settle in their originating phase. There is no global idle resampling of pistons or other components.

## 7. Piston requests and motion arbitration

Power samples and electrical callbacks use committed state. Retain the current request kinds, per-actor/action deduplication, base/head constraints, token-domain restrictions, early-retract conditions, and landing-time formulas. Requests are revalidated on execution; queued activation alone does not guarantee movement.

### Request round

1. For each actor, consider its oldest pending request. Among actors in one mechanical contact group, nominate the actor with the smallest `(actor component ID, request event ID)` key for this round. Leave other requests pending for subsequent rounds.
2. Evaluate nominated requests against the round snapshot. An invalid power/mechanical request is consumed and its dedup bit cleared. It does not automatically retry; a later qualifying delivered update may create a new request.
3. A valid proposal declares all cells, tokens, heads, bases, motion identities, and joint-arrangement state it reads or writes. Omitted dependencies are an invariant failure.
4. The coordinator visits valid proposals in the same canonical actor/event order. Admit a proposal only if its write set overlaps neither another accepted proposal's read set nor write set, and its read set overlaps no accepted write set. Nonconflicting read/read overlap is allowed.
5. A valid proposal blocked only by another accepted proposal remains pending, with the same event ID, until the next request round. No part of it commits. Its conditions will be reevaluated from the next committed state.
6. Commit accepted transactions together, consume accepted/invalid requests, and clear their request bits. Update occupancy and motion state before subsequent electrical rounds. Settle electrical notifications before selecting the next request frontier.

This conservative rule makes conflicting mechanics deterministic while allowing independent groups to execute concurrently. The stable winner does not repeatedly win the same consumed request. Recurrent newly generated work remains subject to the operation/convergence limits.

Departure removes a token's electrical contribution and occupies its destination with motion state according to existing piston rules. A moving payload emits no carried-block power. Arrival becomes a separate mechanical commit at its existing logical deadline. Requests that interrupt motions declare the affected identities and cells in their proposal sets and perform the existing ownership transfer atomically.

### Motion-completion phase

Snapshot the eligible motion identities on entering this phase, following existing phase/progress eligibility; newly created identities are not added to that snapshot. Order nominations by `(landing deadline, motion creation ID)`. Each completion is one transaction for the motion's remaining parts. Preserve token-before-piston effect generation for extension and piston-before-token effect generation for retraction, although both parts become visible at the same commit. Each completion revalidates its identity and remaining parts before execution. Canceled/replaced parts are stale work and produce no movement.

Choose nonconflicting completion transactions with the same read/write-set rule. Conflicting valid completions remain in this phase's snapshot for the next round, after electrical settling. New piston requests wait for the next piston phase. Increment progress/clock only at the existing game-tick progression point, never because another completion round was required.

Token departure/arrival, geometry notifications, and source reevaluations remain distinct typed effects. An opposed/two-token interaction must be represented as one atomic contact-group transaction or rejected at admission until supported; arbitrary concurrent writes cannot stand in for its mechanics.

## 8. Failure, limits, controls, and handoff

The current semantic operation budget is 1,000,000 per game-tick execution window. Preserve it as the v1 budget, applying it to scheduled dispatches, component/notification dispatches, compiled contribution operations, piston proposals, and motion operations corresponding to guarded work in the serial executor. Administrative bookkeeping, worker tasks, empty loops, and shard routing do not consume semantic operations. Publish a fixed charge table in implementation tests before exposing the mode; optimizing task granularity must not alter charges.

Every successful nonempty round consumes at least one charged dispatch. Thus the operation budget also bounds settling rounds. There is no separate preexisting round-limit constant to preserve. Queue capacities and delay validation retain their current rules; additional typed-event buffers require checked bounds derived from admitted program/work capacities, with structured exhaustion errors. Never drop work on overflow.

Preflight a whole round's charges and effects. If it exceeds remaining budget or capacity, fail before committing it. Report the first offending effect in canonical effect order (or first dispatch when dispatch charges alone exceed the budget). The failure state and position must agree across worker counts. A failed round is retained for diagnosis; it is not partially committed to imitate a serial failure prefix.

Accept external controls only between complete driver calls, in plot-thread order. Process each control's immediate electrical settling before accepting the next; resulting scheduled work and piston requests enter their normal queues. Do not advance simulation time while settling a control. This preserves separate rapid control operations even if publication coalesces their visuals.

Successful publication observes a committed, electrically settled boundary. Reset/handoff occurs after completing the current driver/control call, exports scheduled work and coherent mechanical state, and joins workers before host mutation. Failed execution cannot hand off pending immediate rounds to a native interpreter that has no representation for them. In that case retain compiler ownership and report the export failure; diagnostic snapshot export is allowed, silent continuation is not.

## 9. Required reference examples

The one-worker round interpreter must cover these before Rayon is introduced:

1. Same-round removal of one strength-15 input and addition of another keeps the receiver at 15, with no synthetic pulse.
2. Source rise and fall committed in successive rounds retain both transition-caused notifications; level-sensitive receivers still sample current state when dispatched.
3. Two observer notifications retain multiplicity and run in separate rounds for that observer, subject to its pending guard.
4. Duplicate scheduled entries targeting one component remain separate and see successive committed states.
5. A high-priority entry created by settling executes before the next older low-priority batch; it does not interrupt the current round.
6. Delay-zero scheduled work created in piston/motion phases waits until the next scheduled drain; future deadlines do not move forward during settling.
7. Same-group piston requests resolve in canonical actor order, with postponed requests revalidated and invalid requests consumed.
8. Independent piston groups commit concurrently; read/write-overlapping groups produce the same result for every worker count.
9. Replaced motion identities never complete stale payloads; joint-token transactions preserve unique occupancy.
10. A self-sustaining immediate feedback loop fails at a deterministic atomic round boundary. No output or contribution from its rejected round escapes.
11. A note emitted in two committed rising transitions survives infrequent publication twice; command output respects one shared deterministic quota.
12. Reset/export succeeds after normal settling and refuses an unrepresentable failed frontier.

These examples establish the model's behavior. Native comparisons apply where ordering does not affect outcomes; deterministic differences on ordering-sensitive cases must be recorded as parallel-mode semantics.
