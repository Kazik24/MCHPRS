# Findings and agreed requirements

Status: research and agreed design input, 2026-10-11. Runtime parity and performance have not been freshly measured for this proposal.

## User decisions

1. Preserve observable interpreter behavior: timing, delivered updates, acceptance cancellation, outputs, and continuation. Internal operations may later disappear when equivalence is established.
2. Shared payload ownership, temporary detachment, drop, and recapture are allowed. The earlier example assumption that every piston always retains its own block is withdrawn.
3. Compile only stationary piston entry. A moving piston is a valid compilation error. Initially also require BetweenTicks, no active motion work, and no queued piston events; preserve ordinary scheduled ticks.
4. Empty ordinary pistons are update generators, subject to their real geometry and ordered transition behavior. Single-block pushes by ordinary pistons are outside the initial admission subset.
5. Transport redstone blocks and explicitly supported entity-free inert cubes, including required wool, concrete, and quartz states. Reject transported observers, electrical components, containers, and pistons.
6. Stationary observers may watch any changing cell, including dust, bases, heads, and payload destinations.
7. General execution handles arbitrary legal control histories. A protocol-dependent optimization needs checked or enforced preconditions.
8. Start circuit coverage with TEST examples, then ADDER1BIT and ADDER_11BITS, Divider, counters, and FPU. ANPU supplies noninstant examples early and a full replay checkpoint later.
9. The initial executable representation is entirely graph-native. A physical/native domain or World adapter used for simulation does not satisfy the first deliverable.
10. Dust is generalized in the graph. A later optimization must be able to eliminate safe dust and preserve the speed of the existing ordinary path.
11. Human-readable structure, named intermediate values, debuggable stages, and Ponytail simplicity are requirements. Luna6 implementation agents work one reviewed step at a time.
12. Later clarification: abstract physical movement away. The runtime graph/state must be simple: selected connections, compact piston state, shared resources, and necessary pending graph events. No Motion objects, progress fields, carried-block simulation, or physical movement loop.

## Required reference reading

- [Redstone model](../../REDSTONE_MODEL.md): electrical queries, callback procedures, scheduler, seed/Turbo dust, observers, support, persistence.
- [Piston model](../../PISTON_MODEL.md): samples, requests, acceptance, transport, shared state, instant causality.
- [Interpreter architecture](../../INTERPRETER_ARCHITECTURE.md): actual ownership and execution paths.
- [Failed attempts summary](../../../scraps/docs/REDPILER_PISTON_ATTEMPTS_SUMMARY.md).
- [Regression analysis](../../../scraps/docs/REDPILER_REGRESSION_TESTS.md).
- [Parser generalization analysis](../../../scraps/docs/REDPILER_PARSER_GENERALIZATION.md).

The last three files are local archived evidence under ignored scraps. The essential lessons are restated here so implementation does not depend on those archives being present in another checkout.

## Four operations that must stay distinct

| Operation | Meaning | Incorrect shortcut |
| --- | --- | --- |
| Power availability | Current normal/QC query result | Automatically activate a QC piston |
| Sample | Delivered callback evaluates current power | Deliver only when the strength changes |
| Request | An action is queued | Immediately commit the requested pose |
| Acceptance | Live power and geometry permit the popped event | Use request-time power forever |

Settlement is another observation: motion completion and reset may produce further updates. Same final blocks with different queues or phases can have different futures.

## Geometry and movement findings

These describe the reference interpreter. They are evidence for extracting graph connections and event timing, not instructions to copy its physical motion data structures. The graph preserves their relevant consequences through contact selection, availability, and queued events; interpolation is absent from execution.

- Moving payloads emit and conduct none of their carried stationary material's power.
- Extension installs moving payload/head geometry before the base's extended flag is written. Retraction makes the base itself temporarily unavailable to ordinary piston updates.
- Source/head interruption and transported-payload interruption restore different occupants.
- Uninterrupted motion restores on its third movement visit: creation at 0, then 0.5, then 1, then restoration. Reaching 1 does not itself restore the payload.
- Motions have exact identities. A replacement at the same position must not be advanced by an older movement snapshot.
- Motion completion order and callbacks matter; motion cannot be one bank-wide final-state assignment.
- A piston can affect conduction, dust ascent/descent, stored sides, support, and observer delivery without its payload being an electrical source.
- A fixed Far cell of an empty ordinary piston is context. It is not an owned payload merely because it lies on the facing axis.

## Fresh stationary fixture inventory

The previous investigation reused [the schematic inspector](../../../tools/inspect_instant_pistons.py). These counts describe imported stationary geometry only. They do not prove all reachable push lines or support conditions.

| Fixture | Sticky bases | Ordinary bases | Shared current payload positions | Maximum sharing |
| --- | ---: | ---: | ---: | ---: |
| ANPU Pong | 2,113 | 785 | 0 | 1 |
| FPU_FULL | 8,090 | 0 | 1,463 | 3 |
| Fixed Divider | 2,055 | 0 | 489 | 2 |
| ADDER_11BITS | 142 | 0 | 33 | 2 |
| ADDER_1BIT | 13 | 0 | 3 | 2 |
| COUNTER_BASIC | 162 | 1 | 16 | 2 |

All inspected extended heads matched facing/stickiness, and expected initial sticky payload slots contained blocks. Payload references were redstone blocks or wool/concrete/quartz conductors, not observers or other electrical components. No current expected payload had dust directly above it. These reassuring facts are not future-state admission certificates.

ANPU contains 728 empty ordinary actuators with a foreign piston base at Far. All ordinary pistons in the inspected TEST snapshots were empty actuators; some WEIRD variants have a stationary observer at Far. The no-moved-observer rule must not reject an observer that remains stationary there.

| Fixture path | SHA-256 |
| --- | --- |
| `test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem` | `f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec` |
| `test_data/piston-research/fpu-full-20261009/FPU_FULL.schem` | `f893a4618f19c10bc7b1bc553a0d37e33c4fc5cd2d3521b881c1665a2b66a6e9` |
| `test_data/piston-research/fpu-piston-correct/fpu_pistion_correct.schem` | `704016b6900ec22ff0d37184de480a5c9965fd390b7bcf7c3f23d7677048c391` |
| `test_data/piston-research/divider-fixed/divider_fpu_fixed.schem` | `c6f04b0f948a3dc5b05e42e7b1a450edefed522159f421a6dd25f891b6e5297b` |
| `test_data/instant-pistons-io/ADDER_11BITS.schem` | `335ab94a3ba7f89cd0a497a2ac606372ff2743cfcd549f4863ead310af4138b9` |
| `test_data/instant-pistons-io/ADDER_1BIT.schem` | `e80092c97fc4637c313a735d38d0241b69b876f41dd2db7de571a6458369e526` |
| `test_data/instant-pistons-io/COUNTER_BASIC.schem` | `ae325e9238942ad0ebbe49671b39cda1a563ab45e7bd55c32ee5800298cf3ff4` |

Use an existing manifest's exact revision for each replay. Do not substitute FPU_FULL, corrected FPU, or a legacy Divider for each other silently. Verify hashes again during Step 01.

## Dust and observers

The physical wire algorithm is ordered. Its root seed formula differs from subsequent Turbo recalculation. Turbo captures site state lazily on first discovery, carries heading biases through all 24 neighboring sites, and can deliver callbacks with captured rather than current state. Air and inert sites have distinct traversal identities even though they have no component handler.

Observers receive qualifying direction-bearing callbacks. The interpreter does not universally compare a remembered old/new watched block ID. Shape and ordinary power delivery have different exclusions; callback equality and power equality are different questions. Observer activation/off delays are two game ticks each.

## Decisive existing regressions

- [Instant tests](../../../crates/core/src/redstone/instant_piston_tests.rs): chain FIFO causality, adder reconnection before pending validation, acceptance before late inhibition, shared OR conservation, in-flight drop and recapture.
- [IO tests](../../../crates/core/src/redstone/instant_piston_tests/io.rs): independent BUD data/update, retained storage, cancelled requests, arithmetic observation windows, stepped/full-tick agreement.
- [Piston tests](../../../crates/core/src/redstone/piston/tests.rs): support-sensitive movement, completion, identity, and replacement behavior.
- [Wire tests](../../../crates/core/src/redstone/wire/turbo_tests.rs): traversal and notification behavior.
- [CPU references](../../../crates/core/tests/cpu_references.rs): pinned interpreter state/output episodes; long replays are ignored by ordinary test runs and do not automatically compile a circuit.

## Lessons from attempts one through three

1. Cached truth tables lost notifications, cancellation, moving occupancy, and reset waveforms.
2. A second physical state machine diverged when source and compiled timelines had different rules. The new graph transition implementation therefore needs operation-level reference tests, not just final outputs.
3. Native domains preserved behavior but their broad ownership closure absorbed ordinary circuitry. Recorded Counter/FPU/Divider runs had zero executable graph nodes.
4. QC data with independently delivered updates exposed a missing activation operation. Calling every such actor memory does not supply that operation.
5. Shared block ownership cannot be represented by deterministic private copies.
6. Successful admission, two compiled models agreeing, or an old passing report does not establish interpreter-equivalent execution.
7. Tests in undeclared modules do not run. A filtered cargo command that runs zero tests is not validation.

## Research limits

Source inspection and stationary inventory support the proposed representation. No fresh long runtime captures, full CPU replays, or performance benchmarks were run for this design. Bounded contact closure under shared transfers and sufficiency of compact logical state for noninstant continuation remain the largest admission questions. The implementation must produce concrete counterexamples rather than relax guards silently.
