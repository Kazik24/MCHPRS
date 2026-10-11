# Step 12: ADDER_1BIT followed by ADDER_11BITS

Status: not started. [Step 11](./11-test-family.md) must be reviewed.

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
