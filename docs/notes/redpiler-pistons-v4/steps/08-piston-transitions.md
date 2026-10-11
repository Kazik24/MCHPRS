# Step 08: Piston connection switching and timed graph events

Status: not started. [Step 07](./07-generalized-dust.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Execute isolated piston nodes as compact stateful switches with ordered updates and necessary delayed graph events.

## Read these sources

- [piston reference](../../../../crates/core/src/redstone/piston.rs), [instant helpers](../../../../crates/core/src/redstone/piston/instant.rs), and [piston tests](../../../../crates/core/src/redstone/piston/tests.rs).
- [instant regressions](../../../../crates/core/src/redstone/instant_piston_tests.rs), [QC/BUD tests](../../../../crates/core/src/redstone/piston/tests/bud_reference.rs), and [piston model](../../../PISTON_MODEL.md).
- [architecture](../ARCHITECTURE.md), especially compact state/events, guards, routes, and scheduling.

## Scope and implementation

Implement isolated empty ordinary and sticky single-resource actors over compiled contacts. The runtime has no Motion type, progress, carried block, physical entity loop, or World simulation adapter. Shared execution is Step 09.

1. Separate current normal/QC availability, delivered sampling, action enqueue, and acceptance. Deduplicate equal actor/action requests and retain other actions/FIFO. Recheck live inputs and selected contacts when accepting.
2. Use the proposed pose bit, availability enum, and revision as the starting piston state. Derive base/head receiver eligibility and connection facts. Preserve the transient PublishingHeadRemoval view: nested notifications see the removed head while the base remains extended/sampleable. Add no independent physical fields.
3. Implement sample_piston, accept_action, extend_connections, retract_connections, release_resource, and apply_deferred_event as named steps. Select compiled connections, set resource availability, and deliver the actual routes in order.
4. During noninstant transitions, disable the channels that the reference makes unavailable and enqueue their restoration/update events. Establish tick/phase deadlines against the oracle. Skip interpolation work; do not run 0-to-0.5-to-1 progress steps.
5. Represent early release/drop and interruptible pending restoration through logical events and resource contact selection. Retain initiating actor/contact provenance to distinguish extension from retraction. Derive early release from pending timing/phase, not a renamed progress counter. Test another completion notifying the actor at its resource's deadline before that resource publishes. A captured head callback still follows supplied/live forwarding rules.
6. Use revisions to discard obsolete events after reversal or replacement. Freeze due sequence IDs and a cursor before nested publication; new work cannot inherit a replaced event's turn. Preserve the frontier at supported inspection boundaries. Piston and resource revisions invalidate their own work independently.
7. Drain newly appended actions in the action phase; actions produced by deferred publication wait for the next action phase. Retain distinct base/head/resource publication order when observable.
8. Keep legal blocked extension as ordinary behavior. Preparation must already have ruled out successful unsupported pushes. Empty ordinary actors change base/head connection behavior without acquiring fixed Far context.

## Acceptance checks

Compare native and graph held noninstant transitions, early reversal/drop, acceptance cancellation, same-value QC updates, instant chained actions, unavailable redstone channels, and completion callback order. Include empty ordinary actuators with fixed foreign context at Far. Require graph-only execution and verify that no progress/entity state exists in the backend. Compare observable traces, not physical internal step counts.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one small piston state table, its selected connections, and three traces: cancelled request, early release, and delayed restoration. Explain why each stored field is necessary; delete derivable fields.

Isolated actors produce equivalent observable power/update/timing behavior through compact graph state and events. No physical motion simulator is introduced.

## Leave out

No Motion/progress implementation, private payload copy, ideal gate shortcut, giant transition instruction VM, shared execution claim, or forced settlement on stop.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 09](./09-shared-payloads.md) until the user reviews this result.
