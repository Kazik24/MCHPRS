# Simplest tick rewind implementation plan

Updated 2026-10-04. Scope: interpreter-only rewind using complete in-memory snapshots and a runtime-sized vector buffer, defaulting to 100 game ticks. This replaces the earlier mutation-journal and fixed-capacity proposals. The design below is now implemented; see [usage and verification](TICK_REWIND.md).

## Approach

Before each whole interpreted game tick, copy the plot simulation into a snapshot. Keep at most the selected number of snapshots in a dynamically allocated vector used as a ring buffer. The default history depth is 100 ticks; it can be selected when enabling recording. To rewind K ticks, restore one selected snapshot and discard it and all newer snapshots. Pause ticking and resend the restored world to players.

This avoids per-block change journals, mutation hooks, incremental restoration, copy-on-write storage, compression, persistent history and Redpiler support. Full snapshots are the simplest implementation; memory and copying speed are the tradeoff.

## Commands and behavior

| Command | Behavior |
| --- | --- |
| `/rhistory on [ticks]` | Enable recording with the selected capacity, default 100 ticks; report approximate current memory and projected memory at full capacity. |
| `/rhistory off` | Report approximate memory released, disable recording and release the vector and its snapshots. |
| `/rhistory` | Report enabled state, available ticks, selected capacity and approximate retained memory. |
| `/rback` | Rewind one game tick. |
| `/rback <ticks>` | Rewind exactly that positive number of game ticks. |

- Recording defaults to off and remains local to the plot. History and the enabled flag are not persisted.
- Store the selected capacity in a runtime `max_ticks` field. A constant may supply the default of 100, but the buffer size itself is not fixed at compile time. `/rhistory on 500`, for example, starts a fresh session with capacity for 500 ticks.
- Ordinary users may request at most 1,000 ticks; `plots.admin.rewind.unlimited` granted through LuckPerms permits larger history capacities and rewind requests without bypassing other command/plot checks. Without LuckPerms, the 1,000-tick cap applies to everyone.
- Allocate vector slots when enabling; allocate snapshot data only as ticks are recorded. Enabling again starts fresh with the requested capacity. Reject zero, malformed/out-of-range counts and allocation failures without discarding an existing recording session.
- Record both automatic interpreted ticks and whole-tick `/radvance` calls, including ticks with no block changes.
- Successful rewind pauses at `/rtps 0`. Reset wall-clock timing/lag state and update sleep/render controls using existing logic. Resume explicitly with existing commands.
- Validate authorization, count, available history and execution state before changing anything. Insufficient history fails rather than silently reducing the requested count.
- Rewind affects the entire plot. Apply existing plot owner/admin mutation rules and proposed `commands.rhistory` / `commands.rback` permissions; announce a successful rewind to its occupants.
- Rewind restores all plot blocks and block entities, so it also undoes player/WorldEdit edits made after the selected snapshot. Edits already present when that snapshot was captured remain. No edit journal or per-interaction invalidation is needed.
- Player position, inventory, selections, ownership, delivered chat and sounds remain current. Forward replay can emit chat/sounds again; deterministic replay requires identical external input or none.
- There is no simulation redo. Advancing after rewind records a new future.
- While recording, reject nano/pico advancement. Reject enabling recording mid-tick until the user completes the partial tick through existing whole-tick advancement.
- Plot replacement/unloading clears history. Restart begins with recording off.

### Tick units

Use game ticks. The current `/radvance` code calls `Plot::tick()` once per count, and the interpreter completes one game-tick phase cycle. The README describes this as redstone ticks, while component redstone delays are multiplied by two when scheduled. Verify the count with a delayed-component fixture and correct the documentation without changing existing forward-step behavior.

### Redpiler exclusion

Do not change any files under `crates/core/src/redpiler/`. Do not snapshot compiled state, reset/recompile it for rewind, alter compiler preferences, or add a later Redpiler phase to this plan.

The new commands reject recording/rewinding while compiled execution is active, using the existing `is_active()` query. At the plot's existing transition into compiled execution, clear/disable interpreter history so it cannot survive a compiled interval. This is a small plot-level safety check; existing compilation behavior continues normally. It does not implement Redpiler rewind. If the deployment never runs Redpiler, this boundary is only defensive.

## Snapshot structure

Add a small `crates/core/src/plot/history.rs` module:

```text
PlotSnapshot
  chunks: Vec<ChunkData<PLOT_SECTIONS>>
  scheduler: TickScheduler<ScheduledBlockTick>
  piston_state: PistonState

TickHistory
  enabled: bool
  max_ticks: usize = 100          // runtime selection, not a fixed array
  slots: Vec<Option<PlotSnapshot>> // dynamically allocated ring slots
  next_write: usize
  len: usize                      // valid retained snapshots
  estimated_snapshot_heap_bytes: usize
```

Use the vector as a ring: overwrite the oldest slot once full, rather than moving all snapshots with `Vec::remove(0)` every tick. Physical slot order can wrap; chronological order is defined by `next_write` and `len`. Allocate empty slots with a fallible reservation before committing enablement. On disable, replace the vector with an empty vector and reset ring/accounting fields; `clear()` alone would retain its allocation.

Reuse existing representations and operations:

- [Chunk storage](../crates/core/src/world/storage.rs): `Chunk::save()` produces section/palette data and cloned block entities, incorporating buffered block writes. `Chunk::load()` reconstructs storage.
- [Scheduler](../crates/core/src/redpiler/backend/queue.rs): clone the scheduler directly to retain the cursor, remaining delays, expected block types, priority and FIFO ordering. No scheduler code changes are required.
- [Piston state](../crates/world/src/lib.rs): clone the entire `PistonState`, including logical time, phase, events, motions, identities, progress, movement work and carried block entities.

Using chunk `save/load` means in-memory conversion only: no serialization, disk writes or save-format changes. Keep plot coordinates, players, packet senders, communication channels, timing measurements and history itself outside snapshots.

Full snapshots also capture in-place mutable entity changes, including command-block timestamps and success counts, without instrumenting setters.

## Capture algorithm

Integrate capture immediately before whole interpreted ticking in the existing [plot tick path](../crates/core/src/plot/mod.rs), used by automatic ticking and [manual advancement](../crates/core/src/plot/commands.rs). Use one recording boundary, not separate recording in both callers.

1. If recording is disabled, run the existing tick without snapshot allocations.
2. Check that execution is at a whole-tick boundary.
3. If the buffer is full, take/drop the snapshot in `next_write` and subtract its estimated heap memory before creating another.
4. Collect chunk `save()` results; clone the scheduler and piston state.
5. Insert the snapshot in `next_write`, add its estimated heap memory, advance the cursor modulo `max_ticks`, and update `len`. Then run the existing tick unchanged.

Verify that repeated `Chunk::save()` capture preserves normal buffered packet delivery and section block counts. Its current flush copies pending values into storage without consuming the outgoing change tracker; use a dedicated helper only if verification reveals a problem.

## Restore algorithm

For H available pre-tick snapshots and a requested rewind K, select chronological index `H - K`, translated into the vector's ring slot. Its physical index is `(next_write + max_ticks - K) % max_ticks`, using checked arithmetic or an equivalent overflow-safe calculation. After 10 recorded ticks, rewinding 3 restores the snapshot captured before tick 8: the state after tick 7, including any edits made before that snapshot.

1. Validate permission, positive K, history depth, interpreter mode and whole-tick boundary.
2. Pause at TPS zero and reset timing backlog.
3. Take the selected snapshot and discard all newer snapshots. Subtract the removed snapshots' estimated memory, reduce `len` by K, and set `next_write` to the selected slot. Retain older snapshots for further step-back commands; newly recorded ticks fill the discarded future's slots.
4. Consume the snapshot's chunk data through `Chunk::load()` with the existing plot chunk coordinates. Replace only chunks, scheduler and piston state.
5. Do not use `PlotWorld::from_chunks`, ordinary entity setters, neighbor updates or placement callbacks: these can register new motion identities or schedule new work.
6. Discard unsent command-block messages from the abandoned future. Verify the normal command boundary drains staged messages; delivered messages remain delivered.
7. Clear current occupants' WorldEdit undo/redo buffers, with a notice, to prevent replaying edits from the abandoned future. Inspect actual buffer ownership; keep selections and clipboards.
8. Resend every currently visible plot chunk to each player through `encode_packet_for_client`, using the paused plot's rendering mode. Refresh even with `/wsr 0`.
9. Report the number of restored game ticks and remaining depth.

Reconstructed chunks have fresh packet trackers. Preserve connections and packet senders. Do not replay historical piston action packets. Full chunk refresh is simpler than finding changed blocks/chunks and covers restored entities and heightmaps. Verify client convergence when a piston animation was in progress.

## Approximate memory reporting

Add a small estimation helper; exact allocator tracking is outside scope. Estimate allocated vector capacity and recursively owned snapshot data, including chunk section buffers/palettes, entity maps and owned payloads, scheduler queue capacities, piston vectors/events and carried entities. Use allocated capacities where available. Include snapshot structs through the slot-vector allocation, rather than counting their inline fields twice. Do not use serialized byte size as if it were actual memory usage.

Store an estimated heap-byte count with each snapshot and update the history total when inserting, evicting or discarding it. Reporting then needs no full history scan. For owned data whose exact allocation is inconvenient to inspect, use documented approximations. Allocator overhead and process-wide memory are not included, so label every value approximate and specific to this history buffer.

When enabling, build one temporary representative snapshot to estimate the current plot's per-snapshot cost, then drop it. This does not advance simulation or add an available rewind tick. Report the vector's initial memory separately from the projected full-buffer use:

```text
History enabled: up to 1,000 game ticks.
Approximate current memory: <slot-vector allocation>.
Estimated memory at full capacity: <slot allocation + 1,000 x sample heap bytes>.
```

The projection describes the current plot and can change as blocks, entities and queues change. It is not a reservation of all future snapshot memory. Use checked arithmetic for estimated totals and projection; reject unrepresentable capacity requests before changing state.

When disabling, compute/report the current retained estimate before dropping storage:

```text
History disabled. Released approximately <retained estimate> from <available ticks> snapshots.
```

This reports storage released by the history; the allocator may retain pages, so it does not promise an equivalent reduction in process RSS. Status uses the same estimator and reports available depth versus selected capacity.

## Implementation steps

1. Add snapshot/history types and the dynamic vector ring with a runtime capacity defaulting to 100; initialize recording off.
2. Add the single pre-tick capture boundary and direct snapshot restore helper.
3. Add on/off/status, optional capacity and one/multi-tick commands, approximate memory estimates, authorization and client command declaration nodes. Validate manually indexed declaration links.
4. Add pause/timing reset, full client refresh, staged-message cleanup and WorldEdit undo/redo cleanup.
5. Add partial-step and compiled-mode exclusions, and clear history on plot replacement/unloading.
6. Run focused correctness/client checks, measure snapshot costs and document the actual command units and edit-restoration behavior.

Expected main changes: the new history module, `plot/mod.rs`, `plot/commands.rs`, focused tests and command docs. Add storage/WorldEdit helpers only where existing APIs are insufficient. No compiler-module, configuration or save-schema changes.

## Validation

- Advance K ticks, rewind K and compare complete simulation state with the original. Advance again with no external input and reproduce the same result.
- Cover one/multiple ticks, empty ticks, runtime-capacity eviction, vector wraparound, repeated rewind and branching after rewind across wrapped slots.
- Cover delayed repeaters/comparators/buttons, typed stale ticks and queue ordering.
- Cover piston extension/retraction, short pulses, progress, carried entities, motion identities and queued events.
- Cover command-block entity data, unsent future messages and entity changes without block changes.
- Confirm edits after the target snapshot are undone and player position/inventory remain current.
- Confirm invalid counts, missing permission, insufficient depth, compiled mode and partial stepping fail without changing world/TPS.
- Confirm the default is 100 ticks, custom capacities work, enabling again starts empty, off drops vector capacity as well as snapshots, and disabled ticking performs no snapshot allocations.
- Confirm invalid capacity/allocation requests preserve an existing session. Check memory estimates on enable/off/status and accounting after eviction, rewind and re-enablement, including owned entity payloads and wrapped slots.
- Confirm repeated captures preserve ordinary packets and section counts.
- Check multiple clients, restored block entities, a moving piston, transition from high-TPS rendering to paused rendering, and `/wsr 0`.
- Save/reload after rewind: the restored present survives and history is empty/disabled.

Use focused tests first, then `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked --offline`, and `cargo test --workspace --all-targets --locked --offline`. Use existing independent protocol smoke tooling in temporary worlds for client/restart verification.

## Cost and difficulty

This is simpler than the original journal design. Rough estimate: **several hours to a day for the core implementation; around 1-2 working days including focused tests**, assuming familiarity with the repository, with broader client validation afterward. The requested memory reporting adds a small estimator and bookkeeping. These are estimates, not measured delivery times.

Every snapshot copies the stored plot, even if only one component changed. Palette storage and omitted air data reduce retained memory, but capture still visits all sections. Measure both memory and recording throughput using actual circuits. The default is 100 snapshots; the user can select a smaller or larger capacity when enabling.

For scale, a raw 256 x 256 x 256 block array at four bytes/state is 64 MiB. One thousand raw snapshots would be 62.5 GiB, excluding entities and queues. These illustrate raw storage; the proposed palette-backed snapshots have different, workload-dependent costs and are allocated gradually. Several recording plots multiply total memory use. The enable-time projection makes the approximate cost visible before the buffer fills.

Keep the requested approximate estimator and per-snapshot accounting simple. Do not add compression, exact allocator instrumentation, automatic memory-budget management or incremental journals to this first implementation without measured need. If snapshot copying or memory use is unacceptable, choose a smaller runtime capacity or revisit the approach as a separate optimization task. This version prioritizes simple debugging, not maximum recording throughput.

## Analysis references

The repository analysis covered the [README](../README.md), [repository assessment](REPOSITORY_ASSESSMENT.md), [piston timing repair](PISTON_TIMING_IMPLEMENTATION.md), [scheduled-tick audit](piston-audit/SCHEDULED_TICKS.md), [piston performance](PISTON_PERFORMANCE.md), [high-TPS rendering](HIGH_TPS_RENDERING.md) and [chat commands](CHAT_COMMANDS.md), together with the code paths above. Current ticks retain expected block identity, so cloning must preserve it. Existing local command-block changes must be retained and included in snapshots.
