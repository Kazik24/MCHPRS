# Step 11: Supported TEST circuits under active controls

Status: implemented; awaiting human review.

Result: `redpiler::backend::graph::test_family_tests` replays all thirteen pinned fixtures (hashes checked against `download-manifest.json`) under masks, both actor orders, staggered inputs, 1-8 tick pulses, same-tick cancellation, rearm, and long holds, plus three horizontal rotations, comparing ordered callbacks/samples/requests/acceptance and the projected interpreter future (cells, wire properties, entities, ticks, requests, motions) at every boundary. Public compile-run-reset-native-recompile cuts cover stop/restart. TEST_DEC2, TEST_OR_3, and TEST_XOR use the wall levers recorded in `output-summary.json` on their torch supports; the loader supplies a floor block under bottom-layer dust whose saved selection omitted its support (TEST_XOR). General fixes: opaque handler-free cubes such as stone are admitted resources; a retracting downward base keeps dust support (as `interaction::supports_dust`); a piston-adjacent dependent already unsupported at entry is rejected with its position; dust-walk callbacks are observed. WEIRD fixtures were excluded by request.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Demonstrate admitted graph execution of the small TEST use cases before larger arithmetic circuits.

## Read these sources

- [TEST timing research](../../../../crates/core/src/redpiler/analysis/tests/research/test_prefix_timing.rs), [research registration](../../../../crates/core/src/redpiler/analysis/tests/research.rs), and [download manifest](../../../../test_data/piston-research/test-prefix-20261008/download-manifest.json).
- [instant regression tests](../../../../crates/core/src/redstone/instant_piston_tests.rs), [IO tests](../../../../crates/core/src/redstone/instant_piston_tests/io.rs), and [notification research](../../../../crates/core/src/redpiler/analysis/tests/research/notification_routes.rs).

## Scope and implementation

Add compiled differential episodes for the thirteen non-WEIRD TEST schematics. Make only general parser/executor corrections justified by concrete traces. This is a behavior checkpoint, not a fixture-specific optimization pass.

1. Pin the local manifest/hash and cover TEST_BUD_1, TEST_BUD_2, TEST_CODER_1, TEST_DEC, TEST_DEC2, TEST_INSTANT_3, TEST_OR1, TEST_OR2, TEST_OR_3, TEST_PISTION, TEST_PISTION_2, TEST_XOR, and TEST_XOR2.
2. Reuse known controls/protocols where present. TEST_DEC2, TEST_OR_3, and TEST_XOR lack lever controls in the inspected snapshots; define explicit legitimate stimuli and observation points instead of treating idle admission as execution coverage.
3. For each case require the rich graph backend, nonzero relevant executed work, and native-versus-graph comparison through active and restored phases. A preparation result or native-only replay is insufficient.
4. Exercise meaningful held and pulsed controls, both relevant actor orders, rearm, and stop/restart. Add representative horizontal rotations and vertical/world-up QC checks using valid supports; translate coordinates without introducing circuit defects.
5. Compare callback direction/order, pending graph events, contact selection/availability, saved wire properties, and observation windows. Include stationary observer interactions where fixtures provide them; retain the earlier bulb/comparator mixed-component regressions.
6. If a TEST_WEIRD case is rejected, report the actual unsupported mechanic and position. Never reject a filename. A stationary observer at Far is context and does not itself violate the moved-observer restriction.
7. Fix a semantic cause once in the parser/query/transition helper and rerun affected earlier checks. Do not add per-circuit state machines or replace downloaded circuits with simplified stand-ins.

## Acceptance checks

Produce a coverage table with fixture hash, stimulus, actual backend, executed operation count, observed outputs, and continuation result. Run all thirteen active protocols and targeted rotations/order variants. Demonstrate a specific unsupported-case diagnostic independently of any fixture filename.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review the coverage table and one previously failing trace reduced to a small general rule. Confirm that meaningful controls were applied to the no-lever cases.

The supported TEST family actually executes through the graph, with explicit active coverage and no fixture-specific semantics.

## Leave out

No requirement to admit every WEIRD mechanic, idle-only success claims, arithmetic pattern replacement, broad benchmarks, or reference regeneration.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 12](./12-adders.md) until the user reviews this result.
