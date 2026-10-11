# Step 07: Ordered dust evaluation over graph references

Status: not started. [Step 06](./06-scheduler-and-components.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Execute seed and Turbo wire behavior using the graph's compiled neighborhoods and independent local walk memory.

## Read these sources

- [wire rules and pure helpers](../../../../crates/core/src/redstone/wire/mod.rs), [Turbo implementation](../../../../crates/core/src/redstone/wire/turbo.rs), and [Turbo tests](../../../../crates/core/src/redstone/wire/turbo_tests.rs).
- [power queries](../../../../crates/core/src/redstone/power.rs), [redstone model](../../../REDSTONE_MODEL.md), and architecture sections 8-10.
- [notification routes](../../../../crates/core/src/redpiler/analysis/tests/research/notification_routes.rs).

## Scope and implementation

Implement the graph wire executor. The parser already supplies every required site, guarded contributor, and route. No runtime position lookup or native Turbo call is permitted.

1. Implement evaluate_seed separately from evaluate_turbo_wire. Keep their vertical contributor and ascent conditions explicit; do not replace both with equilibrium propagation.
2. Reuse TURBO_ORDER, on_neighbor_changed_from, and regulate_sides where they are pure. Use each WirePlan's 24 site references and preserve distinct air/solid identities that influence visitation and heading.
3. Capture site properties lazily at first discovery. Track heading, x/z biases, visited state, layer, and three ordered queues in one local WireWalk. Update captured wire strength when the reference algorithm does, without replacing captured geometry.
4. Distinguish captured emitter/block properties from live strong-input neighborhoods and live comparator/entity outputs. Name the operand view in query helpers and trace it.
5. Dispatch captured callbacks in the native traversal order. Each nested wire walk owns a separate frame; release state borrows before recursive callbacks. Do not share a scratch map that lets nested walks overwrite outer observations.
6. Preserve stored dust sides and strength at entry, including stale states, until an actual rule changes them. Contact selection changes invalidate necessary derived queries but do not automatically normalize wires.
7. Fail preparation if required neighborhood/query sites were omitted. Do not synthesize air on a missing reference or discover neighbors spatially at runtime.

## Acceptance checks

Compare first-seed versus later-Turbo episodes, unchanged-power notifications when the algorithm delivers them, heading-sensitive recipient order, captured properties after a nested contact selection change, live strong contributors, nested independent frames, and stale imported wires left idle. Include ascent/descent and side-regulation variants; register and execute existing relevant Turbo checks.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one readable walk trace with discovery, heading, layer, power write, and callback delivery. Explain why a naive fixed-point dust solver produces a different trace.

Graph dust is executable with the reference's ordered, supplied/live semantics and no native or spatial runtime dependency.

## Leave out

No wire elimination, final-strength-only propagation, whole-circuit snapshots, permanent walk caches, or synthetic updates after every state write.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 08](./08-piston-transitions.md) until the user reviews this result.
