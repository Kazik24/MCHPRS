Implementation status, **2026-10-04**: [piston timing repair](PISTON_TIMING_IMPLEMENTATION.md) implements event/movement phases, ownership, typed scheduled ticks and completion updates. All five existing circuits now match captured Java 1.21.5 traces. Push reactions and slime/honey attachment graphs are explicitly deferred by the user. Historical audit findings below describe the original snapshot.

# Plan to repair piston state, short pulses and movement

Prepared: **2026-10-03**. Status: **plan only; no simulation changes made by this task**.

Implementation update, **2026-10-04**: the [focused live-testing patch](LIVE_PATCH_2026_10_04.md) implements phase 1's directional observer strong power and its complete facing/side/powered input regression. The piston event/movement engine phases and live Java circuit traces remain outstanding. The original plan preparation did not change simulation code.

This implements the findings in [the piston audit](PISTON_AUDIT.md). The repair covers delayed base-state transitions, the artificial cooldown, missing retract-and-drop behavior, overwritten push-chain blocks and observer strong-power direction. Success requires matching intermediate states and event ordering, in addition to final block positions.

The follow-up [deep Java 1.21.5 audit](PISTON_DEEP_AUDIT.md) adds verified movement reactions, ordinary tick identity, head/replacement lifecycle, observer relocation, support updates and waterlogging to the repair scope. Its 31 diagnostics have four passing controls and 27 failing expectations on the current source snapshot.

## Current repair priorities

Priority updated **2026-10-04** following the user's selection. The immediate repair focuses on these three issue documents:

| Priority | Required result | Relevant phases below |
| --- | --- | --- |
| [Lifecycle](piston-audit/LIFECYCLE.md) | Old movement cannot overwrite replacements; heads and bases retain valid ownership and removal behavior | 2–4, 6 |
| [Scheduled ticks](piston-audit/SCHEDULED_TICKS.md) | Work retains its expected block type and cannot dispatch into a replacement component | 2, 6 |
| [Completion updates](piston-audit/COMPLETION_UPDATES.md) | Restoration executes correct placement, support and state-normalization callbacks | 3, 5–6 |

Use the phase descriptions as the dependency map for those repairs. The broader movement-rule and timing findings remain a secondary backlog; they do not all need to be bundled into the first repair. Changes to event/moving-state representation are still required where they establish ownership or correct completion. Pin the reference, preserve save compatibility and run the selected acceptance checks before expanding scope.

## Baseline, target and scope

The audited simulation baseline is `cb3d4e27a67737abad990932ce94b5461368d45f`. Its five existing piston fixtures have four passes and the known memory-cell failure; [five additional regressions](piston-audit/regressions.rs) reproduce the audited defects. The memory-cell expected array is provisional and has not been recorded from a running Java server.

Since the original audit, the workspace has moved to `port/piston-1.21.5`; `server.rs` currently advertises Minecraft 1.21.5 / protocol 770, and plot saves currently use format 3 with a Minecraft data-version header. These are observed workspace facts, not a protocol-retargeting decision made by this plan. The follow-up audit now pins and inspects the exact 1.21.5 implementation and runtime block properties. Live circuit traces remain required.

Prepare reference traces and tests now. Implement and integrate the repair on a dedicated branch from the stable protocol-port baseline, preserving the existing sequence of port completion followed by simulation correction. Record the selected integration version and pin its Java reference before implementing version-dependent behavior. Do not overwrite concurrent port changes or switch the shared checkout for experiments.

Keep World, redstone and Redpiler in their current core modules. Preserve game/nano/pico advancement, direct and quasi-connected power, piston-aware WorldEdit, animation and restart behavior. Keep circuits with unsupported piston/observer components in interpreted execution, with explicit eligibility checks at compiler handoffs.

This milestone fixes the audited defects for the supported block set. Additional vanilla systems such as slime/honey attachment rules require their own reference-backed coverage before claiming complete piston compliance.

## Implementation choices

| Concern | Proposed design |
| --- | --- |
| Piston requests | A typed FIFO of `PistonEvent { position, expected_base_kind, action, direction }`; action distinguishes extend, retract and retract-without-pulling |
| Duplicate requests | Deduplicate the full event identity while queued. Different actions at the same position remain distinct. Remove the deduplication entry when popping so later requests can be queued |
| Ordinary scheduled ticks | Retain scheduler priority semantics while adding expected component type to interpreted tick entries. An old observer tick must not dispatch into a replacement moving piston. Future movement work must not veto a piston event |
| Runtime ownership | Add piston event, moving-entity and logical-time state to `PlotWorld`; expose explicit operations through the core `World` trait. Put shared saveable event types in a dependency-safe crate such as `mchprs_world` |
| Movement | Resolve a movement plan before writes, then install a moving block/entity at each destination. Distinguish the moving source/head from transported payloads |
| Progress | Use exact current and previous progress, preferably `f32` in the revised moving-entity representation. Keep last logical tick and movement identity in runtime metadata to avoid unnecessarily enlarging all block entities |
| Clock | Use logical game ticks, independent of wall-clock time, RTPS and world-send rate. Save enough clock/age information for restart to preserve pending behavior |
| Outgoing actions | Keep `World::block_action` as notification of an executed action. Simulation-event queueing and packet emission are separate operations; wire action 2 must retain its reference meaning |
| Persistence | Freeze old serialized readers; reserve the next unused format version for the changed entities, event queue and movement metadata. Use the port's existing atomic-write facilities |

The current `progress_to_u8(0.5)` truncates to 127; decoding yields `127 / 255`, approximately 0.498. Do not use that value in a strict half-progress predicate. The short-pulse decision must use the reference's previous/interpolated progress and tick-phase rules, rather than substituting current progress or a fixed delay.

## Phase 0 — establish the behavioral reference

**Files:** audit artifacts, regression fixtures and proposed Java/Rust trace tooling.

1. Record the completed port's source commit, lockfile, supported blocks and current test outcomes. Keep the detached 1.18.2 baseline available for comparison.
2. Pin the exact integration-version Java server and mappings, verify their hashes and inspect piston-base, moving-entity, structure-resolution, observer and server tick/event paths. Use the existing 1.18.2 reference to distinguish original defects from version differences.
3. Capture the memory-cell schematic and minimal pulse circuits on the matching Java implementation. Record initial states, placement/removal sequence and whether stimuli occur during scheduled ticks, block-event handling or between game ticks.
4. Build normalized traces containing logical tick, phase, order within the phase, semantic block state, moving state/source/direction, current/previous progress, requested event and emitted action. Normalize numeric IDs by block names and properties.
5. Port the five diagnostic regressions into the normal test suite. Keep the memory-cell expected trace provisional until the Java capture is available.

Adjust tests to check semantics. The cooldown reproducer currently inspects `TickEntry.ticks_left == 0`; after separation it must inspect a queued piston event or its observable response. The completion test must stop at the reference-defined completion phase, rather than assume the old two-call completion model is correct.

**Acceptance:** the exact target version and phase ordering are recorded; a repeatable reference trace defines the expected short-pulse and memory-cell behavior. A missing Java trace remains an outstanding acceptance requirement, not permission to invent a passing array.

## Phase 1 — correct directional observer input

**Files:** `crates/core/src/redstone/mod.rs` and component tests.

Correct the observer strong-power arm to apply the same directional output convention as the reference. Test all six observer facings and all six queried sides, including powering a conducting block and a piston through it. Verify both powered and unpowered observers.

Keep this correction in an independent commit so changes in the memory-cell input trace can be attributed to it. Preserve observer notification/tick timing until phase 5 validates ordering.

**Acceptance:** the observer regression passes, off-axis conducting blocks receive no false strong power, and existing observer/piston fixture differences have recorded explanations.

## Phase 2 — introduce piston events and explicit advancement phases

**Files:** `core/src/world/mod.rs`, `core/src/redstone/piston.rs`, `core/src/redstone/mod.rs`, `core/src/plot/mod.rs`, shared event types and scheduling tests.

1. Add typed event queueing and execution APIs. Capture action intent and direction when requested; do not replace the captured drop decision with a later generic power check.
2. Drain events in deterministic FIFO order. Match the reference's treatment of events generated during event handling and exact duplicate suppression.
3. At execution, validate the expected base block and recheck power as the reference does. Follow reference behavior for cancelled requests, changed blocks and changed facing; avoid a broad cancellation rule that changes semantics.
4. Separate ordinary scheduled-tick work, piston events and moving-entity ticks in a shared advancement engine. Determine their order from phase-0 traces and source inspection; track the specific handling-tick phase used by the drop predicate.
5. Implement game, nano and pico advancement through that same engine. A pico step executes one defined operation; a nano step processes a documented wave without accidentally consuming newly queued waves. Advance logical time at game-tick boundaries.
6. Preserve ordinary component priority ordering. Add expected block kind to interpreted ticks and reject dispatch when the position contains a different kind; keep entity incarnation checks separate. Update save conversion and compiler handoffs for the revised entry representation. `NanoTick` currently aliases `Normal`; adding a different priority alone would not create a Java-compatible event phase.

Keep the old movement path usable during the queue/API preparation. Remove the artificial base lock only when phases 3–4 supply coherent movement and retraction handling; do not leave an intermediate implementation that repeatedly requests extension against a moving head.

**Acceptance:** future component ticks do not suppress piston requests; duplicate/stale/opposite-action cases behave as specified; advancing by a game tick and advancing through all equivalent nano/pico operations produce the same state and event order.

## Phase 3 — resolve movement and model every moving destination

**Files:** `core/src/redstone/piston.rs`, proposed `piston/resolver.rs` and `piston/movement.rs`, `blocks/src/block_entities.rs`, plot movement metadata and tests.

1. Introduce a movement resolver returning ordered moved and destroyed blocks. Revalidate movement at execution so a queued request cannot apply a stale plan.
2. Handle linear push chains up to the reference limit, sticky pulling, world/plot bounds, immovable blocks, extended pistons, fragile blocks and block entities within the supported block set. Replace the `is_cube()`/`has_block_entity()` shortcut with explicit movement reactions. Unsupported states must not be silently overwritten or destroyed.
3. Validate the complete plan before writing. Apply block/entity changes and neighbor notifications in reference order; do not treat a generic bulk update as equivalent to Java's intermediate states.
4. On successful extension, set the base extended during the event at the reference-defined point. Store a source head state at the moving-head destination and actual payload states at their own moving destinations.
5. Advance current/previous progress in the moving-entity phase. Use a movement identity or entity incarnation to prevent old completion work from finalizing a replacement entity at the same position.
6. Finalize each moving entity into its stored state only while it still owns the corresponding moving block. Completion must not reconstruct or overwrite a base that has already entered a different transition.
7. Use the reference's `NORMAL`, `DESTROY`, `BLOCK` and `PUSH_ONLY` reactions plus hardness, special-block, block-entity and bounds checks. Include trapdoors, signs, unknown chests and glazed terracotta in coverage. Slime/honey side branches are a separate declared milestone if outside the initial supported set.

**Acceptance:** logical extension is visible before movement completes; the two-block gold/stone case preserves both payloads; blocked moves leave the circuit intact; progress and completion traces match the reference in all six directions.

## Phase 4 — implement retraction, early interruption and dropping

**Files:** piston event/movement code, action definitions and pulse tests.

1. Implement ordinary retraction and retract-without-pulling as distinct executed actions. Rename or replace `PistonAction::Cancel` with the correct semantics while retaining the numeric wire value 2.
2. Decide the requested action using the matching extending entity two blocks ahead, its direction, previous progress, last logical tick and the reference's handling-tick condition.
3. Handle retraction during extension. Finalize the old moving head as required, install the retracting source at the base and handle the moving payload without duplication or loss.
4. Ordinary sticky retraction pulls only when the reference permits it. The drop action leaves the payload at its pushed destination and never starts an ordinary sticky pull afterward.
5. Remove the three-game-tick base locks and the recovery branches that require waiting for a stationary head. Preserve controlled handling of genuinely invalid/orphaned states without fabricating successful movement.
6. Emit actions and block/entity updates in reference order; server state must be correct independently of whether packets are sent.

Test pulses before an extension event executes, immediately after it, at the half-progress boundary, after movement completes and during scheduled-tick handling. Cover zero-, one-, two-game-tick and sustained inputs, plus fast off/on cycles. Obtain each expected outcome from the corresponding reference phase.

**Acceptance:** the one-game-tick stone pulse drops its payload, sustained input followed by ordinary retraction pulls it, and retriggering neither duplicates nor loses a block. The extension and cooldown regressions also pass.

## Phase 5 — align neighbor order and verify the memory cell

**Files:** piston neighbor updates, interpreted observer/wire dispatch, direction iteration and fixture traces.

1. Replace or adapt `on_piston_state_change` using the captured notification sequence. Separate block-state/shape changes, neighbor notifications and observer-triggering changes where the reference distinguishes them.
2. Verify iteration order explicitly; the fork's shared face order must not be assumed equivalent to Java's. Keep piston-specific ordering local where possible to avoid changing unrelated component behavior.
   Include head survival/backing checks, head/base removal callbacks, support loss, moved powered-observer reset and waterlogging normalization. A direct redstone callback does not substitute for a shape update.
3. Run all four existing update fixtures and the memory-cell trace with the new engine. Compare event order, carried states and input power as well as the monitored piston flag.
4. Update a fixture expectation only when a pinned Java trace establishes that the old expectation was wrong. Preserve the original trace and an explanation of each intentional change.

**Acceptance:** the memory cell agrees with the captured Java trace throughout the observation window; existing fixture changes are explained by reference evidence. Passing a guessed ten-element boolean array is insufficient.

## Phase 6 — persistence, editing, compiler handoffs and client behavior

**Files:** `save_data/src/plot_data.rs`, legacy readers, moving-entity NBT, plot save/load, WorldEdit, Redpiler activation/reset/flush and outgoing piston updates.

1. Introduce the reserved save format with explicit readers for the preceding port format and original fork inputs. Save ordered pending piston events, authoritative movement progress/history and logical timing metadata alongside ordinary component ticks.
2. Define legacy conversion before changing bincode structs/enums. Stable states can be mapped explicitly. Legacy in-flight states lacking sufficient timing/history must receive a documented conversion rule or a controlled error that preserves the source; do not invent lossless recovery.
3. Verify restart during extension, ordinary retraction and drop handling. Restored work must execute once in the correct phase and must not reinterpret old absolute tick times against a reset clock.
4. Serialize moving-piston NBT from authoritative runtime state, including carried block properties and source meaning. Preserve WorldEdit rotate/flip and schematic round trips; cancel obsolete runtime work when an edit removes/replaces its entity.
5. Gate manual and automatic Redpiler activation when the region contains unsupported piston/observer components, moving entities or pending piston events. Keep the circuit interpreted, with an actionable message for a manual compile. Do not add a pretend compiled piston node.
6. On reset/flush and plot save/shutdown, preserve every pending ordinary tick and piston event exactly once. Recheck activation after edits or schematic pastes introduce unsupported components.
7. Verify client animation, action IDs and block-state versus block-registry IDs with the chosen target client. Client rendering and server-state agreement are separate checks.

**Acceptance:** new saves round-trip in-flight motion, conversion preserves supported old data or reports its limitation safely, nano/pico commands and edits remain coherent, and compiler handoffs do not drop piston work.

## Broader commit sequence and release checks

The sequence below describes the complete earlier compliance plan. Select and regroup the required changes around the three current priorities above; it does not require an unrelated observer-power or full movement-rule repair to ship first.

| Commit group | Reviewable deliverable |
| --- | --- |
| 1 | Pinned target reference, trace harness and semantically corrected regression fixtures |
| 2 | Directional observer strong power and focused input tests |
| 3 | Typed piston events and shared advancement-phase APIs, retaining the old movement path until replacement is ready |
| 4 | Frozen legacy readers and reserved save schema before entity/runtime serialization changes |
| 5 | Movement resolver, destination entities, exact progress, base transitions and retract/drop handling as one coherent engine change |
| 6 | Reference neighbor order, memory-cell comparison and justified fixture changes |
| 7 | Save/restart, WorldEdit, compiler fallback and client integration |
| 8 | Workspace/release validation and final compatibility report |

Keep inseparable state-machine and serialization changes together. Intermediate commits may retain explicit failing regressions, but must compile and must not be described as complete fixes.

| Validation area | Required evidence |
| --- | --- |
| Five audited defects | All five semantic regressions pass, with reference-correct timing |
| Deeper audit | All supported expectations from the 31-case follow-up pass; any deferred slime/honey coverage is explicit and unsupported movement preserves data |
| Pulse timing | Phase-sensitive short/long pulses, half-progress boundaries and rapid retriggering |
| Movement | Six directions; zero/one/two/max/over-limit payloads; blocked paths, boundaries and block-count/state conservation |
| Inputs and updates | Direct power, quasi-connectivity, BUD behavior, observer direction and notification order |
| Fine-grained execution | Equivalent game/nano/pico execution produces identical normalized traces |
| Existing circuits | Four update fixtures and the memory-cell fixture compared against recorded reference behavior |
| Persistence and editing | Restart at each movement phase; migration, replacement, undo/redo and schematic round trips |
| Compiler transitions | Automatic/manual compilation, reset and flush preserve interpreted circuits and pending work |
| Client | Normal/sticky extension, retraction and drop animation with the exact target client |

Run focused tests for each changed behavior, then `cargo check --workspace --all-targets --locked`, `cargo test --workspace --all-targets --locked` and `cargo build --release --locked` at integration. Keep pre-existing unrelated failures separately classified. Check active CI coverage and add the new meaningful tests. Compare performance on representative existing circuits after correctness passes; investigate regressions without broadening this work into a compiler rewrite.

The deliverable is a corrected, version-verified piston engine with reproducible Java traces, passing targeted regressions, documented save compatibility and preserved fork workflows. Remaining unsupported mechanics must stay visible in the broader reference-compliance backlog.
