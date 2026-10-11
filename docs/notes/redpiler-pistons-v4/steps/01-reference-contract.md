# Step 01: Reference contract and fixture harness

Status: not started. No implementation prerequisite.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Create a small independent comparison harness before changing the parser or executor.

## Read these sources

- [redstone model](../../../REDSTONE_MODEL.md), sections 4-7, 12, and 15.
- [piston model](../../../PISTON_MODEL.md), sections 2-5.
- [instant tests](../../../../crates/core/src/redstone/instant_piston_tests.rs) and [IO tests](../../../../crates/core/src/redstone/instant_piston_tests/io.rs).
- [piston trace](../../../../crates/core/src/redstone/piston/trace.rs), [analysis tests](../../../../crates/core/src/redpiler/analysis/tests.rs), and [fixture manifests](../../../../test_data/instant-pistons-io/fixtures/adder_1bit.json).

## Scope and implementation

Test support and fixture/protocol selection only. Leave production compilation and interpreter behavior unchanged. Register every added test module explicitly.

1. Reuse existing loaders, manifests, source-position conventions, trace collectors, and snapshot helpers. Pin hashes and distinguish import, optional settling, and notified input operations.
2. Define a canonical comparison record containing logical tick/phase, ordered callbacks and directions, samples/requests/accepted events, component/contact observations, projected native boundary state, and ordered pending ticks. Keep physical motion fields in the native oracle/projection checker only; future graph records compare their observable consequences rather than copying a Motion model. Translate future graph IDs through immutable provenance; do not compare raw IDs from unrelated worlds.
3. Capture small native episodes for an empty actuator, one sticky redstone/conductor actuator, independent QC/update input, shared OR, and adder acceptance cancellation. Use existing fixtures rather than new schematic formats.
4. Separate admission, actual execution, output projection, arithmetic oracle, and full-state continuation claims. A future graph run must require successful admission and active graph execution.
5. Make the comparator fail deliberately on a changed direction, event order, pending deadline, or payload observation. Restore that perturbation within the test; do not change frozen evidence.

## Acceptance checks

Run the small existing reference episodes and the harness self-checks. Verify module discovery and nonzero counts. Fresh 50,000-tick CPU captures are unnecessary here.

Verify new tests are registered and actually run. Use the existing targeted release/locked test workflow described in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one compact ordered trace and one intentional comparison failure caught by the harness. The reviewer must see which observations are authoritative and which are projections.

Independent native episode loading and meaningful trace comparison are reusable by later steps; production behavior is unchanged.

## Leave out

No new backend, parser admission, ideal arithmetic model, Java capture, or performance claim.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 02](./02-graph-and-memory.md) until the user reviews this result.
