# Step 13: Divider and noninstant ANPU checkpoints

Status: not started. [Step 12](./12-adders.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Validate a harder instant arithmetic fixture and full noninstant logic, including host-visible component behavior.

## Read these sources

- [Divider manifest](../../../../test_data/piston-research/divider-fixed/download-manifest.json), [Divider protocol](../../../../test_data/piston-research/fixtures/fpu_divider.json), and [research test registration](../../../../crates/core/src/redpiler/analysis/tests/research.rs).
- [CPU reference tests](../../../../crates/core/tests/cpu_references.rs), [CPU reference harness](../../../../crates/core/benches/support/cpus.rs), and [ANPU schematic](../../../../test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem).
- [command-block rules](../../../../crates/core/src/redstone/command_block.rs), [plot host](../../../../crates/core/src/plot/mod.rs), and architecture sections 7 and 11-12.

## Scope and implementation

Two ordered reviewable checkpoints: fixed Divider, then ANPU. Small noninstant mechanisms were already covered in Step 08; ANPU verifies their composition. If the work is too large, propose a split at the checkpoint rather than mixing incomplete behavior.

1. Pin the corrected Divider revision and the matching explicit trigger/operand/rearm protocol. Do not silently substitute a legacy schematic or assume an arithmetic oracle that the recorded circuit does not implement.
2. Replay held inputs, pulsed triggers, rearm, and selected changed operands against the interpreter. The recorded fixed Divider's observed 170 is reference behavior for its specified episode, not a general division law.
3. For ANPU, certify the many empty ordinary actuators independently of fixed Far context. The inspected 728 Far foreign-base cases must remain stationary context, not stolen payloads.
4. Run noninstant hold/release episodes and compare connection disabling/restoration, requests, update order, and native-observable boundary projections. The graph does not advance physical entities. Historical extension/retraction timestamps are fixture observations, not constants to hardcode into the executor.
5. Complete any required note/screen/command behavior. Compile command chain/previous references; graph code owns power sampling, scheduling, conditions, success counts, last execution, chain limits, and notification feedback.
6. Use the existing host command allowlist/output path synchronously only for execution/result parsing. A native tick_output call that reads geometry or owns component lifecycle is not graph-native execution. A result must affect graph state before subsequent callbacks or conditional commands.
7. Ensure the public CPU harness actually compiles and selects the rich graph. Run a short active protocol first, then the full frozen ANPU replay (the existing episode is 50,000 game ticks). Document executed counts and output parity, not merely an unchanged native CPU reference.
8. Rerun relevant TEST/adder/scheduler/continuation checks when correcting a shared rule. Preserve frozen references and distinguish fidelity to the current interpreter from intended circuit arithmetic.

## Acceptance checks

Provide corrected Divider active/rearm comparisons and an ANPU noninstant connection/event trace plus full frozen replay. Include synchronous command success/failure/conditional-chain tests where ANPU uses them, note output parity, actual graph activity, and reset continuation with delayed connection events pending. Verify ignored long tests are explicitly selected and execute.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review Divider evidence before beginning the full ANPU checkpoint. At completion show the host boundary and the graph-owned feedback path, and prove Far foreign bases were not changed.

Divider and ANPU execute through the graph with active temporal parity and supported host effects. A costly full replay left unrun must be reported as unfinished coverage, not success.

## Leave out

No idealized division node, delayed command feedback, ANPU-specific piston timing, native CPU substitution, threading, or speed target.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 14](./14-counters-and-fpu.md) until the user reviews this result.
