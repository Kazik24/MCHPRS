# Step 12: ADDER_1BIT followed by ADDER_11BITS

Status: implemented; awaiting human review.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Prove that the general parser and executor handle shared arithmetic topology and inhibition ordering.

## Read these sources

- [adder and inhibition checks](../../../../crates/core/src/redstone/instant_piston_tests.rs), [arithmetic IO windows](../../../../crates/core/src/redstone/instant_piston_tests/io.rs), and [IO manifests](../../../../test_data/instant-pistons-io/download-manifest.json).
- [ADDER_1BIT fixture](../../../../test_data/instant-pistons-io/fixtures/adder_1bit.json), [ADDER_11BITS fixture](../../../../test_data/instant-pistons-io/fixtures/adder_11bits.json), and architecture sections 7-10.

## Scope and implementation

Use ADDER_1BIT as the first arithmetic checkpoint, then the eleven-bit adder. Keep their execution general: arithmetic results are assertions, not a shortcut replacing the circuit.

1. Verify the hashes in FINDINGS and use the exact IO fixture port definitions. Start from saved state without eager dust normalization.
2. Run one-bit input/carry combinations, then selected eleven-bit carry chains, alternating patterns, and boundaries. Compare native graph behavior at every declared phase and tick as well as the mathematical output windows.
3. Preserve the existing distinction between reconnection/inhibition before pending-event validation and inhibition arriving after acceptance. Keep both as named regression episodes; late inhibition must not retroactively cancel an accepted action.
4. Exercise both operand/control orders, held inputs, repeated triggers, and rearm. The general executor cannot assume the one successful pulse/reset protocol used by an optimization candidate.
5. Track shared redstone and inert-conductor tokens, conditional dust/conduction links, delayed-event revisions/order, and ordered reset waves. Add minimal explanatory traces for the first differing operation.
6. Use representative rotations/translations with valid current and future supports to expose geometry assumptions. Preserve world-up QC rather than rotating it with the actuator axis.
7. If parser closure or a transition fails, correct the shared mechanism and rerun its minimal regression before scaling. Do not introduce an adder node, schematic detector, or family-specific truth table.

## Acceptance checks

Run ADDER_1BIT exhaustively over its defined small inputs and ADDER_11BITS over manifest vectors plus representative carry extremes. Check the existing sum/carry windows (the current one-bit tests distinguish sum ticks 3-7 from carry ticks 5-9) and full temporal parity. Continue native execution after graph reset during active and restored phases.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review ADDER_1BIT evidence before progressing to the eleven-bit replay within this step. Show shared-token conservation and the two inhibition traces, then the larger graph's active arithmetic evidence.

Both adders are admitted and execute with interpreter-equivalent timing and continuation under the tested legal histories.

## Leave out

No specialized adder execution, instant-group recognition, changed sampling windows to conceal mismatches, or silent narrowing to a single input protocol.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 13](./13-divider-and-anpu.md) until the user reviews this result.

## Implemented result

The test-only adder harness is registered from `graph.rs` in `graph_adder_tests.rs`. It loads the two frozen IO schematics without dust normalization, verifies both fixture hashes against their local manifest and the pinned download manifest, and uses the declared ports and input polarities. No runtime behavior or fixture data changed.

The final targeted release run passed all six registered tests:

```text
cargo test -p mchprs_core --release --locked redpiler::backend::graph::adder_tests:: -- --test-threads=1
6 passed; 0 failed
```

Discovery was checked with this command; it reported six tests:

```text
cargo test -p mchprs_core --release --locked redpiler::backend::graph::adder_tests:: -- --list
```

The successful group covers:

- all 8 one-bit operand/carry vectors and all 40 eleven-bit manifest vectors, with 3,640 external-action/tick boundaries compared in order. At each boundary it compares native and graph operations plus projected cells (including wire state), block entities, scheduled ticks, event queues, motions, clock, and phase;
- one-bit payload sum ticks 1–3, sum repeater ticks 3–7, and carry repeater ticks 5–9; eleven-bit payload sum ticks 1–3 and sum repeater ticks 3–7;
- reversed operand order, same-tick trigger cancellation, held input, repeated trigger and rearm; a control change after a graph-accepted action; shared resource/contact invariants after every graph boundary;
- reset at an active one-bit cut and at a stationary all-zero endpoint. The active cut hands back an interpreter future that stays matched for 256 ticks. The zero-vector endpoint recompiles and stays matched for four more ticks;
- translated 90° one-bit and 270° eleven-bit cases, including one-bit carry and eleven-bit overflow. The block rotation remains horizontal, so world-up QC is checked against the interpreter.

The eleven-bit manifest declares only the low eleven sum bits as observations. Its carry expectation is checked against the fixture operands, and overflow vectors verify the wrapped sum; there is no separate eleven-bit carry output port to compare.

The pre-acceptance inhibition checks also passed independently: the bounded graph adder episode and the native trace that changes dust connections before pending-target validation. The new post-acceptance control-change episode passed in the six-test group above.

```text
cargo test -p mchprs_core --release --locked redpiler::backend::graph::tests::adder_inhibition_matches_native_for_a_bounded_shared_resource_episode -- --exact --test-threads=1
cargo test -p mchprs_core --release --locked redstone::instant_piston_tests::adder_inhibition_changes_dust_connections_before_pending_target_validation -- --exact --test-threads=1
```

### Optimizer observations

The executable graph inventory, repeated piston slices, Step 11 comparison, and a protocol contract for a possible near-total adder reduction are recorded in [the adder optimizer analysis](./12-adder-optimizer-analysis.md). No optimization was implemented in this verification step.

The no-carry one-bit episode remained active through the 256-tick reset continuation horizon, while the all-zero vector provided a stationary reset/recompile cut. The active reset future matched the interpreter at every continued boundary; no stationary recompile is claimed for that waveform. Existing Step 11 stop/restart coverage remains the broader restart evidence.

### Review evidence

- Code: new `crates/core/src/redpiler/backend/graph_adder_tests.rs`; test registration in `graph.rs`; the shared resource/contact invariant helper is exposed to the sibling test module in `graph_tests.rs`.
- Notes: this result and inventory are saved in `12-adders.md` and [the separate optimizer analysis](./12-adder-optimizer-analysis.md); the Redpiler README status was updated.
- Fixture SHA-256: one-bit `e80092c97fc4637c313a735d38d0241b69b876f41dd2db7de571a6458369e526`; eleven-bit `335ab94a3ba7f89cd0a497a2ac606372ff2743cfcd549f4863ead310af4138b9`.
- `cargo fmt --all` and `git diff --check` completed. The release build emitted existing dead-code warnings for `passes/rich.rs`; the adder tests themselves had no failures.
- No optimization pass, broad benchmark, Step 13 work, or full Step 11 rerun was included. The next useful work is a measured guard/route sharing analysis, followed by the reviewed safe preparation pass described in [Side B](./side-b-basic-passes.md).
