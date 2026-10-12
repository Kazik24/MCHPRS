# Step 10: Transactional activation and continuation projection

Status: implemented; awaiting human review (authorized together with Step 11).

Result: builds with stationary pistons select `BackendKind::Graph` through `Compiler::compile`; other entry issues, rich-graph parser rejections, cancellation, and `--export`/`--export-dot` fail before publication with structured errors and leave world, ticks, piston state, flags, and statistics unchanged. Controls route to graph state; `flush` writes graph state only (io_only suppresses non-IO cells), and `GraphBackend::project_future` derives interpreter blocks, motions (progress from deadlines), ordinary ticks (verbatim via `World::restore_scheduled_tick`), and queued requests at reset. Evidence: `redpiler::backend::graph::fuzz_tests::continuation::*` (reset at every host boundary for 48 single-actor templates, with observer/lamp scheduled work and with display suppressed) and `redpiler::tests::failed_and_cancelled_piston_compilation_preserve_interpreter_state`.

Shortcut: reset is defined at BetweenTicks only; the host rejects nano/pico stepping while Redpiler is active, so no partial-phase frontier is persisted. Legacy `block_type: None` imported ticks are restored with the current type (inherited from Step 06).

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Publish the compact graph safely and translate its state/pending events back into an equivalent interpreter future at reset.

## Read these sources

- [compiler lifecycle](../../../../crates/core/src/redpiler/mod.rs), [analysis](../../../../crates/core/src/redpiler/analysis/mod.rs), and [backend selection](../../../../crates/core/src/redpiler/backend/mod.rs).
- [plot activation/reset/tick](../../../../crates/core/src/plot/mod.rs), [commands](../../../../crates/core/src/plot/commands.rs), and [packet/input handlers](../../../../crates/core/src/plot/packet_handlers.rs).
- [world boundary state](../../../../crates/world/src/lib.rs), [legacy export](../../../../crates/core/src/redpiler/passes/export_graph.rs), and [IO continuation tests](../../../../crates/core/src/redstone/instant_piston_tests/io.rs).
- [interpreter architecture](../../../INTERPRETER_ARCHITECTURE.md) and architecture section 12.

## Scope and implementation

Public integration, input ownership, and a projection-only continuation adapter. Physical metadata required by the interpreter can be derived at this boundary; it is not graph simulation state.

1. Prepare from an immutable snapshot; stage graph/initial state/ticks. Validate, lower, and check cancellation before publication. Transfer native scheduler ownership only after success.
2. Reject moving entry, unsupported phase/piston queues, missing contacts, and insufficient abstraction with structured errors. Failed/cancelled preparation preserves blocks, entities, clock, pending work, flags, and success statistics.
3. Select the graph backend explicitly for supported rich graphs. Report real graph/wire/piston execution counts. No error may silently select native physics or report an admission-only success.
4. Route controls into graph state; reset before geometry/configuration edits. Audit container-closing and other host actions around activation for stale reads, duplicate notifications, or host mutations bypassing ownership.
5. Make display/effect flush a pure projection. Suppressed display cannot change contact selection, sampled power, queues, deadlines, command results, or continuation.
6. Materialize graph-authored fixed component/resource/base/head state and ordinary pending ticks. Preserve untouched context entities and saved stale dust. Use the immutable parser templates and logical contact states.
7. For pending noninstant graph events, derive the physical boundary metadata needed by the interpreter from deadlines, revisions, and event order inside the adapter. The executor stores no progress, previous_progress, or physical motion objects. Normalize unrelated raw identities when comparing futures.
8. Prove reset during a pending transition, not only after settlement. Preserve the frozen deferred sequence IDs/cursor at supported partial-phase boundaries; deadlines alone cannot reconstruct consumed, cancelled, and newly inserted work. If logical state is insufficient, show the two distinguishing histories and propose a minimal logical correction. Do not settle first, delay reset, or silently weaken the contract.
9. Preserve supported semantic inspection boundaries; reset before unsupported fine-step modes. Audit old passes/options/packed links and clearly reject rich legacy export rather than altering its format or dropping edges.

## Acceptance checks

Compare uninterrupted native execution with compile-run-reset-native continuation before actions, during pending connection changes, after restoration, and with ordinary scheduled work/acceptance cancellation. Repeat with display suppressed. Test failure/cancellation for unchanged world and state, actual graph selection/counts, live controls, and explicit export rejection. Run relevant ordinary Redpiler regressions.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show the simple runtime state separately from the boundary projection. Provide one pending-transition continuation trace and one failed-compilation state comparison; prove no native simulation call exists in graph evaluation.

Activation is transactional, public execution is graph-native, and reset yields an equivalent interpreter future without adding Motion/progress to runtime or forcing settlement.

## Leave out

No moving-entry support, per-tick physical presentation state as an authority, binary format redesign, automatic native fallback, deferred reset-to-idle, or split scheduler ownership.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 11](./11-test-family.md) until the user reviews this result.
