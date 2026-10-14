# Step 14: Counter histories followed by FPU opcode histories

Status: not started. [Step 13](./13-divider-and-anpu.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Complete the initial graph representation's hardest stateful and arithmetic target coverage before optimization.

## Read these sources

- [BUD/reference checks](../../../../crates/core/src/redstone/piston/tests/bud_reference.rs), [counter IO checks](../../../../crates/core/src/redstone/instant_piston_tests/io.rs), and [COUNTER_BASIC manifest](../../../../test_data/instant-pistons-io/fixtures/counter_basic.json).
- [PC counter benchmark and fixture descriptor](../../../../crates/core/benches/instant.rs), and the [retained revised Potados schematic](../../../../test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem).
- [current FPU manifest](../../../../test_data/piston-research/download-manifest.json), [FPU schematic](../../../../test_data/piston-research/FPU_MROWW.schem), and [FPU protocol](../../../../test_data/piston-research/fixtures/fpu_legal.json).
- [frozen FPU reference](../../../../test_data/cpu-references/interpreter-20261009-fpu-reference.json) and [frozen PC-counter reference](../../../../test_data/cpu-references/interpreter-20261009-pc-counter-reference.json).

## Scope and implementation

Two sequential checkpoints: stateful counters, then FPU. Preserve each schematic revision and protocol identity. Large traces must be reduced to a small first-divergence episode before changing generic rules.

1. Start with independent QC data/update histories: data alone, update before data, data then update, same-strength qualifying update, and acceptance cancellation. Retained state depends on actual callbacks, not automatic resampling on every data edge.
2. Run COUNTER_BASIC active windows and stop/restart. Preserve the existing manifest's memory-bank observations separately from later bus publication; the current tests use a six-times-trigger-count pattern, not a generic increment-at-every-tick assertion.
3. Cover the minimal PC-counter mechanism and the pinned PC-counter/revised Potados episodes needed by the target set. Explain None/West/Top callback eligibility with actual reference routes. Include long idle then retrigger and reset with pending work.
4. Use the single FPU_MROWW fixture; verify its hash. Exercise every defined opcode, selected operands, held inputs, and repeated trigger/rearm histories according to the fixture protocol.
5. Compare per-tick observations, callbacks, pending graph events, obsolete-event cancellation, and stop/continuation. A fixture's observed arithmetic defects are interpreter behavior for parity purposes; report them rather than claiming IEEE or mathematical correctness.
6. Require nonzero graph execution and no native-domain fallback on every replay. Validate payload conservation in the large shared groups without inventing private copies or ignoring ownership transitions.
7. When a failure appears, isolate the first different operation and correct the common parser/query/transition. Rerun earlier affected packs and the active minimal case before repeating a long replay.
8. Publish the initial-deliverable coverage matrix with exact revisions, protocols, backend/counts, and results. Any remaining required unsupported circuit needs a concrete witness and reviewed architecture correction.

## Acceptance checks

Run minimal QC histories, COUNTER_BASIC windows, pinned PC-counter/revised Potados active protocols, and the defined FPU opcode/rearm vectors. Use existing long frozen references where they match the chosen revision/protocol and explicitly select ignored tests. Include representative mid-transition and pending-request continuation checks after large graph execution.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review counters before scaling to FPU. Then review the full first-deliverable matrix and one minimized first-divergence trace. Confirm the graph is a general representation rather than a library of recognized gates.

The requested target set has honest active graph coverage and temporal/continuation parity. Optimization begins only after the general model is reviewed.

## Leave out

No instant or BUD macro replacement, mathematical correction of downloaded circuits, native fallback, benchmark-driven semantic shortcuts, or implied coverage of unrun histories.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 15](./15-dust-optimization.md) until the user reviews this result.
