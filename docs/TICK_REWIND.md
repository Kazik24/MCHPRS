# Tick rewind

Tick history records complete interpreter snapshots immediately before whole game ticks. It is off by default, local to each loaded plot, and stored only in memory. Automatic ticks and `/radvance` both record history.

```text
/rtps 0
/rhistory on          # Default: keep up to 100 game ticks
# Activate or edit the circuit.
/radvance 10
/rback 5              # Restore five ticks ago and pause
/rback                # Restore one more tick
/rhistory             # Show available ticks and approximate memory
/rhistory off         # Release the buffer and report approximate memory freed
```

Use `/rhistory on 500` to start a fresh session with a different capacity. `/rhistory status` is another way to query status. Enabling reports both initial buffer memory and projected full-buffer memory based on a sample of the current plot. Snapshot data is allocated gradually as ticks run. Memory figures include owned chunk/entity/queue data and vector slots; hash-table overhead is approximate and allocator overhead is excluded. The full-buffer projection changes with the circuit and is not a memory reservation. Freeing the buffer does not guarantee an equivalent drop in process RSS.

The buffer uses a dynamic vector ring: recording past its capacity drops the oldest snapshot. Rewind consumes the selected snapshot and all newer snapshots; forward advancement then records a new future. An insufficient or invalid request leaves world state and tick rate unchanged.

History capacity and rewind counts above **1,000 game ticks** require `plots.admin.rewind.unlimited` granted through LuckPerms. Admins with that node can select larger capacities; available history and allocation/representation limits still apply. Invalid requests preserve the current recording session. The local default is now 100 ticks.

Rewind restores all blocks, block entities, scheduled tick ordering/types/delays, and piston events, identities, progress and carried entities. It also undoes edits made after the restored snapshot. It clears current occupants' WorldEdit undo/redo buffers and refreshes every visible plot chunk, including when `/wsr 0` disables periodic sends. Player positions, inventories, selections, clipboards, ownership and already delivered sounds/chat remain current. Replaying ticks can deliver command-block messages and sounds again.

Whole game ticks are the unit: two game ticks make one redstone tick. A successful rewind pauses with `/rtps 0`; resume with `/rtps 20` or another desired rate. Nano/pico stepping is rejected while recording. Finish a partially advanced tick with `/radvance 1` before enabling history.

Compiled execution is unsupported. Enabling or rewinding while compiled fails; starting compilation disables and frees interpreter history. Redpiler internals are unchanged. History is cleared when its world is unloaded and starts disabled after restart; saving persists only the restored present using the existing save format.

Permissions are `commands.rhistory` and `commands.rback`. Mutating controls also require plot ownership or the existing `plots.admin.interact.other` / `plots.admin.interact.unowned` permission, as appropriate. Rewind affects every occupant of the plot.

The repository uses LuckPerms permission nodes rather than a separate operator flag. Grant `plots.admin.rewind.unlimited` through the existing LuckPerms installation (the server itself provides no permission-management commands). It bypasses only the 1,000-tick policy, not command or plot authorization. This elevated permission requires a grant in the permission cache: without LuckPerms, the cap applies to everyone even though ordinary commands remain available.

## Implementation and verification

The implementation is in [history.rs](../crates/core/src/plot/history.rs), with the recording boundary in [plot/mod.rs](../crates/core/src/plot/mod.rs) and command handling/declarations in [commands.rs](../crates/core/src/plot/commands.rs). It reuses in-memory chunk save/load and exact scheduler/piston clones; no disk snapshots, save migration, mutation journal or compiler backend changes are needed.

Nine [focused tests](../crates/core/src/plot/history/tests.rs) cover ring eviction/wraparound and branching, empty ticks, exact scheduler/piston round trips, command-block mutable state, edit/entity restoration, packet buffering, default/custom capacity, memory accounting/freeing, invalid requests, partial-step exclusion and the ordinary-user limit/admin override.

The independent [protocol smoke test](../tools/protocol_smoke.js) exercises actual history commands, acceptance at 1,000 ticks and rejection above the cap without LuckPerms, preservation of history after rejected requests, two viewers, piston rewind/replay, full refresh with periodic sending disabled, edit restoration, automatic recording, persistence of the restored present, empty history after restart and compiled-mode exclusions. It runs in temporary worlds through [the runner](../tools/run_protocol_smoke.py). The nine focused tests, debug build, formatting and full protocol/restart smoke passed with the 1,000-tick policy; the admin override is covered by unit tests rather than a live LuckPerms database.

Initial validation on 2026-10-04 passed: all 124 workspace unit tests (including eight new history tests), benchmark smoke checks, workspace/all-target compilation, formatting, and the two-process protocol/restart smoke. All nine focused history tests also pass after adding the policy limit and admin bypass. The debug executable was rebuilt. In the original smoke plot, enabling a 1,000-tick history reported 3.07 MiB for empty ring slots and projected 765.25 MiB at full capacity; that is an estimate for that fixture, not a measured full-buffer allocation or a bound for other plots. The equivalent 10,000-tick projection is approximately 7.47 GiB, including about 30.7 MiB of ring slots. Graphical vanilla-client animation appearance was not checked.

Reproduce from the repository root:

```text
cargo test -p mchprs_core --lib --locked --offline plot::history
cargo test --workspace --all-targets --locked --offline
cargo build --locked --offline
py tools/run_protocol_smoke.py
```

Full snapshots copy the plot on every recorded tick. Larger capacities and several recording plots increase memory use; faster tick rates increase copying work. This initial implementation prioritizes simple debugging and exposes estimates through the commands. It does not promise unchanged throughput while recording or perform automatic memory-budget management.
