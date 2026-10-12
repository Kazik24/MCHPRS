# Step 06: Graph scheduler and fixed component callbacks

Status: implementation complete; awaiting human review. [Step 05](./05-callback-parser.md) is reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Execute ordered graph callbacks and scheduled fixed components without public piston activation.

## Read these sources

- [scheduler storage](../../../../crates/core/src/redpiler/backend/queue.rs), [backend contract](../../../../crates/core/src/redpiler/backend/mod.rs), and [plot phase driver](../../../../crates/core/src/plot/mod.rs).
- [reference callbacks](../../../../crates/core/src/redstone/mod.rs), [repeater](../../../../crates/core/src/redstone/repeater.rs), [comparator](../../../../crates/core/src/redstone/comparator.rs), and [observer checks](../../../../crates/core/src/redstone/observer_tests.rs).
- [world clock/state](../../../../crates/world/src/lib.rs), [redstone model](../../../REDSTONE_MODEL.md), and architecture sections 9 and 11.

## Scope and implementation

Introduce the internal graph backend and its callback/phase execution over prepared references. Keep it available to targeted tests; public rich-graph activation waits for Step 10.

1. Reuse TickScheduler's deadline, priority, FIFO, ring, and pending-membership behavior. Bind ticks to node and registry type. Preserve imported duplicate entries and reject stale type-bound work at dispatch.
2. Implement the clock phases explicitly: drain old current work, advance once, drain new current work, piston-action phase, due deferred-connection events, BetweenTicks. Freeze due event order before nested delivery; this phase does not advance physical movement. Until piston execution exists, tests must not claim support for nonempty piston work.
3. Implement synchronous deliver_route and typed callback dispatch. Resolve each recipient at delivery time and finish nested callbacks before continuing. Captured callbacks use their supplied block-state properties, with each handler's live reads kept explicit.
4. Execute supported controls, torches, repeaters, comparators, lamps, bulbs, and observers with graph queries. Reuse pure helpers; port World-dependent rules into named graph operations rather than calling the native executor.
5. Retain same-value comparator notifications, repeater short-pulse behavior, bulb powered-versus-lit state, and directional diode outputs. Observer sampling follows watched-face callbacks, pending suppression, and existing powered/None recovery behavior.
6. Separate dirty presentation from simulation. If a fixed component needs command execution before Step 13, implement the synchronous graph-owned command path then; otherwise mark that execution unavailable explicitly. Never execute commands later during flush.
7. Keep legacy direct/backend callers working. Add kind/state and scheduler validation; do not make an imported pending flag a second authority.

## Acceptance checks

Compare native and graph fixed-component episodes with reordered priorities, same-deadline FIFO, due-at-entry ticks, stale block types, pending suppression, same-value comparator updates, repeater short pulses, and bulb toggles. Check observer activation and off delays of two game ticks, correct watched face, and nonqualifying callbacks. Show at least one operation-level trace, not just a final lamp.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show a graph-only call path from input to route to scheduled transition, and a clock trace proving that advance occurs exactly once. Confirm that flush does not change any semantic field.

Fixed-component callbacks and scheduled transitions match the oracle in the admitted tests; unsupported live operations cannot report success.

## Implementation evidence

The internal graph backend compiles the prepared geometry, electrical queries, and callback routes. Delivery resolves recipients against current graph state and finishes nested routes synchronously. Its clock drains current work, advances the existing scheduler exactly once, drains the next bucket, then enters empty piston-action and deferred-connection phases.

The operation trace asserted by the lever/lamp episode is: lever state change -> ordered route delivery -> lamp state change. The same episode matches native lamp state. Observer, comparator, repeater, and bulb episodes also compare graph results with the native interpreter. Flush tests confirm that presentation projection leaves component state, scheduler contents, pending membership, and clock state unchanged.

Compiled boundary sites retain the interpreter's sampled source strength, including block-entity-backed outputs. Imported duplicate ticks retain their scheduler order; stale ticks at uncompiled air positions remain pending until dispatch, while live non-air ticks without a graph node fail compilation. Registry-type mismatches are skipped at dispatch.

Shape deliveries do not enter electrical update handlers; observers sample only their separately routed Update callbacks. Captured repeater power determines its stored output strength. Bulb-to-comparator callback order and count match native execution, with the compiled route as the sole notification path.

Unsupported command blocks, note blocks, trapdoors, dust, and piston transitions return explicit errors. Commands are not deferred to flush.

Verified with `cargo test -p mchprs_core --locked redpiler::backend::graph::tests` and `cargo test -p mchprs_core --release --locked redpiler::backend::graph::tests`: 13 passed in each profile. `git diff --check` and direct Rust 2024 formatting checks for changed Rust files passed. `cargo fmt --all -- --check` still reports formatting drift in unrelated baseline files restored during this change.

Remaining boundary: piston actions/deferred events are empty phases, dust traversal and command execution are unsupported, and public rich-graph activation remains Step 10 work.

## Leave out

No native simulation adapter, generalized dust traversal, piston transition stubs, parallel scheduler, or public activation of unfinished behavior.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 07](./07-generalized-dust.md) until the user reviews this result.
