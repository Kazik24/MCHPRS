# Plot subtree cleanup

Scope: all 71 files under `crates/core/src/plot/`. Each file was read and reviewed for small, behavior-preserving DRY/readability improvements. The edits are comments that explain long iterator pipelines, one named intermediate in the WorldEdit history counter, and a local variable rename. No data formats, serialized layouts, command ordering, validation, or runtime behavior were changed.

## File coverage

| File | Review result |
| --- | --- |
| `benchmark.rs` | Reviewed; no worthwhile cleanup without changing the small benchmark harness. |
| `command_block/mod.rs` | Reviewed; command-block execution and limits are already separated into focused helpers. |
| `command_block/tests.rs` | Added a note explaining the row-major chunk reconstruction in the restart fixture. |
| `commands/declaration.rs` | Reviewed; declaration helpers already centralize node construction and parsing. |
| `commands/mod.rs` | Reviewed; kept command-node construction order and validation untouched. |
| `commands/plot.rs` | Reviewed; no safe duplication removal across the distinct plot command cases. |
| `commands/redpiler.rs` | Explained the completion pipeline that filters used flags before building matches. |
| `containers.rs` | Reviewed; small shared container operations are already factored. |
| `data.rs` | Reviewed; save/load wrapper is small and preserves the serialized type. |
| `database/mod.rs` | Reviewed; no local edit because membership and claim flows depend on transactional ordering. |
| `diagnostics.rs` | Reviewed; report fields are already grouped by purpose. |
| `git/diff.rs` | Explained that the pipeline keeps changed section indices in matching fingerprint order. |
| `git/mod.rs` | Explained expired-session and chunk-index pipelines; named the expired-session stage. |
| `git/repository.rs` | Explained the ten-row recovery page and next-page probe. |
| `git/repository/database.rs` | Reviewed; query and row-to-model mapping are already concise. |
| `git/resources.rs` | Reviewed; bounded NBT parser branches are explicit and stateful. |
| `git/snapshot.rs` | Reviewed; normalization and fingerprint stages are distinct. |
| `git/tests.rs` | Explained that the concurrent quota test counts successful opens. |
| `git/visuals.rs` | Explained the viewer-distance filter before marker reconciliation. |
| `help.rs` | Reviewed; static help content needs no code abstraction. |
| `history.rs` | Reviewed; history capture, rewind, and player notification paths are separated. |
| `history/budget.rs` | Reviewed; reservation and release logic is already localized. |
| `history/codec.rs` | Reviewed; encoding and restoration preserve their explicit order. |
| `history/tests.rs` | Explained the deterministic y/z/x state walk used by comparisons. |
| `memory_budget.rs` | Reviewed; shared budget synchronization is already isolated. |
| `mod.rs` | Explained sidebar access filtering, entity cleanup, and saved chunk index mapping. |
| `monitor.rs` | Explained the tick-rate calculation over the newest sample window. |
| `neighbors.rs` | Explained filtering requests to coordinates owned by the requested plot. |
| `neighbors/tests.rs` | Reviewed; fixtures already make grouping, expiry, and cache behavior clear. |
| `packet_handlers.rs` | Explained prefix filtering before the bounded tab-completion result cap. |
| `picking.rs` | Reviewed; slot choice and reach calculations are small and direct. |
| `player_model.rs` | Reviewed; packet construction is already a narrow responsibility. |
| `player_model/tests.rs` | Reviewed; protocol-facing expectations are explicit. |
| `preview.rs` | Explained removal-first marker budgeting and chunk-scoped removal after unload. |
| `redpiler_compilation/mod.rs` | Explained why open barrels are extracted from the staged compile snapshot. |
| `redpiler_compilation/tests.rs` | Reviewed; async compilation scenarios remain intentionally explicit. |
| `redstone_tools/compass.rs` | Explained ray projection and lateral-distance scoring. |
| `redstone_tools/completion.rs` | Explained that find completion combines block names with the page flag. |
| `redstone_tools/items.rs` | Reviewed; inventory-slot search order is protocol/gameplay behavior. |
| `redstone_tools/mod.rs` | Reviewed; tool dispatch is already split into focused modules. |
| `redstone_tools/search.rs` | Reviewed; search phases and limits are named and bounded. |
| `redstone_tools/selection.rs` | Reviewed; coordinate selection rules are already direct. |
| `redstone_tools/tests.rs` | Reviewed; shared fixtures and command cases are explicit. |
| `redstone_tools/waveform.rs` | Explained permission-filtered actions, tick/signal limits, and separate probe replacement preparation. |
| `redstone_tools/wire.rs` | Explained view-distance filtering and the clamped eye-to-support fallback distance. |
| `redstone_tools/wire/routing_tests.rs` | Explained reversed-route fixtures, support sampling, and mask-selected obstacles. |
| `redstone_tools/wire/routing.rs` | Reviewed; search and route validation are performance-sensitive and deterministic. |
| `redstone_tools/wire/session_tests.rs` | Explained that fixture support placements precede dust placements. |
| `scoreboard.rs` | Explained hiding only the repository-status row without read permission. |
| `screen_updates.rs` | Reviewed; presentation tracking already separates simulation writes from visible updates. |
| `sign_tests.rs` | Reviewed; packet-order and sign-state assertions are purpose-specific. |
| `tests/client_sync.rs` | Reviewed; packet-sequence assertions intentionally encode client-visible ordering. |
| `tests/client_test_utils.rs` | Reviewed; test packet decoding helpers are already shared. |
| `tests/piston.rs` | Explained row-major reconstruction and order-preserving motion removal expectations. |
| `tests/redpiler_notes.rs` | Reviewed; note parsing expectations are already narrow. |
| `visuals.rs` | Reviewed; presentation policy is already separated from world storage. |
| `world.rs` | Explained restoring repeating command-block ticks after entities load. |
| `world/interpreter.rs` | Reviewed; tick and piston work use explicit state transitions. |
| `world/interpreter/cache.rs` | Reviewed; cache invalidation and index updates are performance-sensitive. |
| `world/scratch.rs` | Reviewed; scratch-world state is already grouped in one private type. |
| `worldedit/execute.rs` | Reviewed; flip/rotate traversal is similar but layout-sensitive; see proposal below. |
| `worldedit/mod.rs` | Named the clipboard iterator stage in retained undo/redo accounting. |
| `worldedit/safety.rs` | Reviewed; geometry and work limits remain explicit before mutation. |
| `worldedit/schematic_paths.rs` | Explained sorting ranked candidates before applying the completion cap. |
| `worldedit/schematic/mod.rs` | Reviewed; versioned Sponge parsing and writing must preserve format details. |
| `worldedit/schematic/validation.rs` | Reviewed; bounded preflight parsing is already broken into focused helpers. |
| `worldedit/stack.rs` | Reviewed; source snapshot and destination validation semantics are explicit. |
| `worldedit/tests/schematic.rs` | Reviewed; focused malformed-input and version cases. |
| `worldedit/tests/schematic/regression.rs` | Explained pairing block names with sparse Sponge palette indices. |
| `worldedit/tests/stack.rs` | Reviewed; snapshot comparisons directly assert undo/redo behavior. |
| `worldedit/tests/update.rs` | Reviewed; packet and update-order expectations are explicit. |

## Longer iterator pipelines

The pipelines below have more than three iterator stages when terminal collection or reduction is counted. Adjacent comments explain their filtering, ordering, or budgeting purpose; `worldedit/mod.rs` splits the history count into a named `clipboards` stage.

| File | Pipeline purpose |
| --- | --- |
| `command_block/tests.rs` | Reconstruct serialized row-major chunk order. |
| `commands/redpiler.rs` | Suggest matching flags that are not already present. |
| `git/diff.rs` | Pair fingerprints, preserve indices, and collect differing sections. |
| `git/mod.rs` | Expire inaccessible sessions, restore snapshot chunk coordinates, and cap matching completions. |
| `git/repository.rs` | Render the visible recovery page while detecting a next page. |
| `git/tests.rs` | Count successful concurrent opens under the shared quota. |
| `git/visuals.rs` | Restrict marker overlays to a viewer's range. |
| `history/tests.rs` | Compare chunk state in deterministic y/z/x order. |
| `mod.rs` | Filter sidebar readers, enumerate remaining entity IDs, and restore chunks by saved index. |
| `monitor.rs` | Average a bounded recent sample window. |
| `neighbors.rs` | Reject out-of-plot chunk coordinates before reading a save. |
| `packet_handlers.rs` | Prefix-filter names before applying the tab-completion cap. |
| `preview.rs` | Spend the reconciliation budget on removals first; remove only markers in an unloaded chunk. |
| `redpiler_compilation/mod.rs` | Close open barrels in the isolated snapshot before compiling. |
| `redstone_tools/compass.rs` | Project a candidate onto a ray and measure lateral distance. |
| `redstone_tools/completion.rs` | Combine block-name suggestions and the page flag. |
| `redstone_tools/waveform.rs` | Apply action permissions and configured tick/signal caps; prepare probe removals separately. |
| `redstone_tools/wire.rs` | Restrict markers to visible chunks and bound fallback ray reach. |
| `redstone_tools/wire/routing_tests.rs` | Exercise routes in both directions, sample first support material, and derive obstacle sets from masks. |
| `redstone_tools/wire/session_tests.rs` | Preserve support-before-dust fixture placement order. |
| `scoreboard.rs` | Hide only the repository status row for viewers without read access. |
| `tests/piston.rs` | Rebuild saved chunk coordinates and preserve movement order in assertions. |
| `world.rs` | Requeue restored repeating command blocks after entity loading. |
| `worldedit/mod.rs` | Sum buffer entries across every retained undo clipboard through a named stage. |
| `worldedit/schematic_paths.rs` | Sort the full ranked completion set before taking its cap. |
| `worldedit/tests/schematic/regression.rs` | Pair names and sparse palette indices during fixture construction. |

## Larger follow-up proposals

These need separate scoped changes and tests; they were not applied during the small cleanup pass.

1. **Split the plot coordinator.** `plot/mod.rs` still combines `Plot` state, load/save setup, packet/player lifecycle, and tick orchestration. Move persistence setup into `lifecycle.rs` and the `update`/`tick` orchestration into `update.rs`, leaving the struct, constants, and stable method entry points in `mod.rs`. Callers include the server plot loop, packet handling, Git restore, and the existing client-sync, piston, history, and redpiler tests. Keep `PlotData` fields and chunk ordering identical; verify lifecycle and tick behavior with those tests.
2. **Split packet handling by responsibility.** `packet_handlers.rs` mixes movement/chat, block interaction, inventory, and settings/sign packets. Move private `Plot` handler implementations into modules by packet family while retaining the `ServerBoundPacketHandler` trait methods as dispatch entry points. Callers and protocol remain unchanged; block-action acknowledgement order and authoritative corrections are the compatibility constraints. Run `tests::client_sync`, movement tests, and `sign_tests`.
3. **Separate WorldEdit command metadata from clipboard operations.** `worldedit/mod.rs` holds command descriptors/parsing, clipboard and undo models, pattern parsing, and traversal helpers, while `execute.rs` and `stack.rs` already contain operations. Move command registration/parsing to a command module and clipboard capture/paste/undo utilities to a clipboard module. Callers are `Plot::handle_command`, `packet_handlers`, `execute.rs`, and `stack.rs`; preserve command names, aliases, permissions, traversal order, and schematic offsets. Run WorldEdit update/stack/schematic tests plus client-sync command tests. A follow-up helper could share the layout walk in `execute_flip` and `execute_rotate`, but must retain block-state transforms, offsets, and palette order exactly.
4. **Split Git runtime coordination from command handling.** `git/mod.rs` combines worker lifecycle, user commands/completions, session permissions, and overlay state alongside existing `diff`, `repository`, and `visuals` modules. Move command parsing/completion to `commands.rs` and worker/session transitions to `session.rs`, keeping `Plot::git_command` and session behavior as stable entry points. Callers include `packet_handlers`, `Plot::update_git`, Git visuals, and `git/tests.rs`; preserve permission checks, checkout locks, worker cancellation, marker ownership, and ordered commit/snapshot data. Verify with all `git::tests` and client-sync marker tests.
5. **Separate database concerns behind the shared connection.** `database/mod.rs` combines warp data, waveform settings, membership cache, claims, and schema initialization. Split membership/claim operations and per-plot settings into private modules while keeping the connection lock and public function signatures in `database/mod.rs`. Callers include plot command handling, WorldEdit membership checks, waveform setup, and plot loading. Keep SQLite schema and transaction boundaries unchanged. Verify the existing database tests plus plot claim/member command tests.
6. **Separate wire snapshot capture from route planning.** `redstone_tools/wire/routing.rs` combines bounded world capture, immutable snapshot representation, search, and route safety checks. Move capture into `snapshot.rs` and planning/validation into `planner.rs`, preserving the current `Session`/worker boundary in `wire.rs`. Callers are wire sessions and the route worker; compatibility constraints are deterministic tie-breaking, read/resource budgets, direction handling, and placement order. Run `routing_tests.rs` and `session_tests.rs`, including cancellation and budget cases.

## Verification

No Cargo command was launched from this subtree because the parent agent ran the consolidated workspace tests. That run reported no plot failures; two unrelated workspace failures remain recorded by the parent. A final core compile/test run after concurrent edits freeze is still useful, with `cargo test -p mchprs_core plot::` as the focused plot filter.
