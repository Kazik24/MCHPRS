# Step 07: Ordered dust evaluation over graph references

Status: implementation complete; awaiting human review.

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
8. Derive weak and strong dust emission sides from each wire's compiled neighborhood and the pure side-regulation helpers, without writing those derived sides into stored wire state.
9. Consume imported ticks targeting admitted wires as native no-ops when due, preserving scheduler deadlines and ordering.

## Acceptance checks

Compare first-seed versus later-Turbo episodes, unchanged-power notifications when the algorithm delivers them, heading-sensitive recipient order, captured properties after a nested contact selection change, live strong contributors, nested independent frames, regulated dust outputs, imported wire ticks, and stale imported wires left idle. Include ascent/descent and side-regulation variants; register and execute existing relevant Turbo checks.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one readable walk trace with discovery, heading, layer, power write, and callback delivery. Explain why a naive fixed-point dust solver produces a different trace.

Graph dust is executable with the reference's ordered, supplied/live semantics and no native or spatial runtime dependency.

## Leave out

No wire elimination, final-strength-only propagation, whole-circuit snapshots, permanent walk caches, or synthetic updates after every state write.

## Implementation and verification

The graph backend now evaluates the initial seed separately from later Turbo visits. It uses the compiled 24-site neighborhoods for power, shape, and regulated dust-emission queries, rejects missing wire bindings during preparation, and keeps only first-discovered sites in a per-walk `FxHashMap`. Each nested invocation gets independent observations, heading/bias data, layers, and ordered queues. A wire write updates that walk's captured strength while preserving its captured geometry and sides. Imported ticks targeting admitted wires are consumed without changing the wire, matching native dispatch.

The native heading calculation is shared with Turbo. Shape-side calculation reuses the same pure side rule through compiled neighbor blocks, without translating positions or reading the world during execution. Direct block properties use the walk's captured view; strong power and live component outputs use current graph state. Idle imported wire properties remain untouched.

The rise/fall differential test compares callback order and final wire states with the interpreter. Default-dot regressions verify that a west-side lever lights an east-side lamp and a lamp through an intervening conductor while the stored wire sides stay unchanged. The nested regression updates a bulb during an outer wire walk; its native callback notifies another wire, starts a nested walk that writes a continuation wire, and then resumes the outer queue. It compares callback order and the intermediate write with native execution. A fixed-point solver would collapse intermediate states and omit their ordered callbacks; the reference instead runs one seed calculation and then dispatches a layer-ordered Turbo walk only when stored power changes.

Trace excerpt from the rise test (intermediate discoveries omitted):

```text
wire seed NodeId(1) operands=live-sites/live-strong block=15 wire=0 power 0 -> 15
wire turbo start NodeId(1)
wire discover BlockPos { x: 6, y: 20, z: 6 } receiver=Some(Component(NodeId(2))) properties=Wire(... power: 0)
wire heading NodeId(1) bias=(0,0) order=3
wire deliver BlockPos { x: 6, y: 20, z: 6 } layer=1 Update { direction: None } receiver=Component(NodeId(2))
wire query NodeId(2) layer=1 operands=captured-walk/live-strong block=15 direct=0
wire strength NodeId(2) layer=1 0 -> 14
wire deliver BlockPos { x: 7, y: 20, z: 6 } layer=1 Update { direction: None } receiver=Component(NodeId(3))
```

Discovery found 33 graph backend tests with:

```text
cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::tests:: -- --list
```

All 33 passed in both debug and release:

```text
cargo test -p mchprs_core --lib --locked redpiler::backend::graph::tests:: -- --test-threads=1
cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::tests:: -- --test-threads=1
```

The four existing Turbo checks were discovered and passed in release:

```text
cargo test -p mchprs_core --release --lib --locked redstone::wire::turbo::tests:: -- --list
cargo test -p mchprs_core --release --lib --locked redstone::wire::turbo::tests:: -- --test-threads=1
```

`cargo check -p mchprs_core --locked`, focused `rustfmt`, and `git diff --check` passed. Piston-driven contact changes remain deferred to Step 08.

## Handoff

Review this implementation before [Step 08](./08-piston-transitions.md). At that step, add an integrated captured-contact regression around a real piston-driven selection change; the current executor intentionally rejects piston transitions.
