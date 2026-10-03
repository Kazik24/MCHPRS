# Plan: port the MCHPRS piston fork to Minecraft 1.21.1

Written: **2026-10-03**. Status: **superseded by the corrected 1.21.5 target**.

The user corrected the target to Minecraft 1.21.5. The original plan below remains historical; current implementation, validation and remaining work are documented in [PORTING_1_21_5.md](PORTING_1_21_5.md).

This plan builds on [the repository assessment](REPOSITORY_ASSESSMENT.md). Start from `recovery/piston-1.18.2`, based on `p0.1.0` / `cb3d4e2`, including the recovered lockfile fix. The old `piston`, `cherrypick_piston`, and master branches are sources of individual changes to review.

## Outcome and scope

Deliver a server that accepts a **Minecraft 1.21.1 client**, retains the fork's piston/observer features and commands, and correctly saves, loads and migrates its supported data. The target protocol is **767**, verified in [the pinned version data](https://github.com/PrismarineJS/minecraft-data/blob/23303d2350177f2084f94cd38e5d7c8e54dfc8d3/data/pc/1.21.1/version.json).

The work sequence is:

1. Preserve the compiled 1.18.2 baseline and record its behavior.
2. Complete the 1.21.1 port without introducing unexplained simulation changes.
3. Subsequently bring this fork's piston implementation into **100% agreement with the target reference piston behavior**.

The memory-cell discrepancy belongs to this implementation. It remains visible during the port and is resolved in the reference-compliance stage. Its current expected trace still needs verification against real/reference Minecraft.

### Implementation decisions

- Keep World/storage, redstone, and Redpiler in their current `core` modules for this port. Crate extraction and major compiler refactoring are separate work.
- Preserve the typed block/property model, piston action hooks, moving-piston entities, observer updates, zero-delay queues and nano/pico advancement.
- Use 1.21.1 numeric IDs in the existing numeric block/item storage model, with explicit legacy conversion. This fits the current engine; adding a multi-version runtime abstraction is unnecessary for this target.
- Preserve the baseline plot width and vertical geometry initially: 256 blocks wide, minimum Y of 0, 256 blocks high, and 16 sections. Advertise a consistent custom dimension. Changing geometry would also change save shape and fixtures.
- Preserve the existing connection/authentication and proxy behavior to the extent supported by the target protocol. Inventory these paths before implementation; a new authentication system or proxy deployment is outside this port.
- Preserve existing supported gameplay. New vanilla gameplay systems are separate features. Decode valid client data sufficiently to avoid breaking the supported creative/redstone workflow.
- Prefer small commits that compile and have focused validation. Adapt selected upstream changes to the baseline instead of merging its entire architectural refactor.

## Reference material

| Reference | Pinned revision | Use |
| --- | --- | --- |
| [PrismarineJS minecraft-data](https://github.com/PrismarineJS/minecraft-data/tree/23303d2350177f2084f94cd38e5d7c8e54dfc8d3) | `23303d2350177f2084f94cd38e5d7c8e54dfc8d3` | Exact 1.21.1 packet schemas, IDs and dataset routing |
| [MCProtocolLib 1.21-1](https://github.com/GeyserMC/MCProtocolLib/tree/e71d7df8c127b7196fd81dd392034d7d97acbb4e) | `e71d7df8c127b7196fd81dd392034d7d97acbb4e` | Independent codec implementation to cross-check against the exact target schema |
| Fork `master` | `76b9b8c` | Map of subsystems changed by the upstream 1.18.2 to 1.20.4 upgrade |
| Current `upstream/master` snapshot | `d492e43` | Individual data, persistence, interaction and compiler fixes |
| `origin/piston` | `dd01a49` | Fork-specific entity spawn/attack, schematic traversal and target-ID fixes |
| Upstream 1.21.8 development branch | `4660d75` | Integration ideas for components/palettes/signs; target-specific values require replacement |

Read [dataPaths.json at the pinned revision](https://github.com/PrismarineJS/minecraft-data/blob/23303d2350177f2084f94cd38e5d7c8e54dfc8d3/data/dataPaths.json): some datasets are shared with other Minecraft versions. Use the [exact 1.21.1 schema](https://github.com/PrismarineJS/minecraft-data/blob/23303d2350177f2084f94cd38e5d7c8e54dfc8d3/data/pc/1.21.1/proto.yml), including its login/configuration, per-registry data, structured item components and anonymous NBT definitions. Resolve disagreements between datasets and codecs before implementing the affected format.

Record source revisions, generation inputs and attribution with imported/generated assets. Glowstone's inspected development configuration targets 1.19, so use a version-matched implementation for packet formats; see [the assessment's source notes](REPOSITORY_ASSESSMENT.md#8-protocol-references-for-minecraft-1211).

## Dependency order

```text
0. Baseline and inventory
       |
1. Versioned data and mappings
       |
2. Codec primitives + legacy migration definitions
       |
3. Login/configuration state machine and registries
       |
4. Join, chunks and player synchronization
       |
5. Creative inventory, interactions, text and commands
       |
6. Piston wire behavior and simulation integration
       |
7. Persisted-data migration and schematic compatibility
       |
8. End-to-end release validation
       |
9. Subsequent full piston reference compliance
```

Begin migration design in phases 1–2, before changing serialized types. Until migration is validated, client milestones use fresh temporary worlds. Each phase below has a completion criterion; compilation alone is not the acceptance criterion for the completed port.

## Phase 0 — freeze the baseline and inventory the surface

**Affected files:** manifests/lockfile, current documentation, existing fixtures; initially no simulation changes.

1. Preserve the recovery lockfile fix in the implementation baseline and create a dedicated port branch, proposed name `port/piston-1.21.1`, when implementation begins.
2. Run the full existing workspace suite once and record all results before editing code. Previously observed results cover focused tests, not the entire suite.
3. Capture the four passing piston fixtures, the memory-cell failure, scheduler behavior, supported block states, fine-grained advancement and current save/load behavior.
4. Inventory every packet emitted/handled by the server, plot, player, WorldEdit and scoreboard code. Record its state, direction, target ID, field changes, call sites and validation case in a packet checklist.
5. Inventory persisted plot, player and container data; authentication/forwarding paths; and supported inventory components needed for redstone and WorldEdit.
6. Use an isolated run directory for client testing so existing worlds are not opened by partially ported code.

**Complete when:** the baseline and all known failures are reproducible, and the packet/data checklist accounts for every existing feature path. Keep failures visible; do not hide the memory-cell test to obtain a green baseline.

## Phase 1 — establish versioned data and state mappings

**Affected files:** `crates/blocks/src/blocks/mod.rs`, `blocks/props.rs`, `items.rs`, `block_entities.rs`, `crates/proc_macros/src/lib.rs`; proposed generated-data module and generator inputs.

1. Add a reproducible importer/generator or generated lookup tables for supported 1.21.1 definitions. Pin both source revision and dataset paths.
2. Keep block-state IDs, block registry IDs, item IDs, block-entity types, entity types, sounds and component types distinct. Audit each existing hard-coded value.
3. Map block states by names and properties, preserving facing, extension, stickiness, piston-head shape and observer power state. Validate all variants used by the engine, not just default IDs.
4. Preserve the fork's block/property representations and macro extensions. Resolve overlapping/incorrect mappings rather than carrying their compiler warnings into generated data.
5. Build legacy-to-target maps for chunk palettes, moving-piston carried states and items. Define explicit diagnostics for unmappable data; do not silently turn important blocks into air or substitute unrelated items.
6. Verify `MC_DATA_VERSION` from version-matched data and inventory every place it affects schematics and migration. Keep this separate from the Minecraft protocol number and MCHPRS save-format number.

**Complete when:** supported states round-trip through names/properties and target IDs, critical piston/observer variants are exhaustive, and old IDs have an explicit conversion policy. Generated output can be reproduced from checked-in inputs/instructions.

## Phase 2 — implement codec primitives and freeze legacy serialization

**Affected files:** `crates/network/src/packets/mod.rs`, `nbt_map.rs` or a focused replacement, `crates/core/src/chat.rs`, inventory/block-entity data types, `crates/save_data/src/plot_data/fixer/`.

1. Adapt target NBT/text encoding while retaining ordinary named NBT where disk/schematic formats require it. Do not use the same root-tag framing blindly in every context.
2. Replace the legacy slot representation with a target-compatible item/component representation and codecs. Preserve additions, removals and nested container contents, plus data needed by the debug stick and block entities.
3. Correctly consume component payloads in incoming creative slots. Components have type-specific formats; an unknown component cannot be skipped by assuming a generic payload length. Parse known target types or return a controlled error for unsupported input.
4. Preserve item data needed for subsequent transmission and persistence. Separate item behavior supported by the engine from the ability to decode/retain its wire data.
5. Keep `PackedPos` and the fork's internal position/action APIs stable unless a deliberate, complete call-site update is required.
6. Freeze legacy serialization types/readers before adding fields or changing enums. Old bincode data cannot safely be decoded using a newly rearranged block-entity enum or changed sign/item struct.
7. Add focused fixtures for framing, text/NBT, slots/components, empty values, nested containers and truncated/malformed payloads. Cross-check target encodings with an independent codec or recorded target-client data.

**Complete when:** all target primitive/slot formats needed by later phases have reliable fixtures and legacy data still has a defined decoder. Every packet using slots or text has an identified adaptation path.

## Phase 3 — implement the connection state machine and configuration

**Affected files:** `crates/network/src/lib.rs`, `packets/mod.rs`, `packets/serverbound.rs`, `packets/clientbound.rs`, `crates/core/src/server.rs`; proposed registry-data assets/module.

1. Replace the current transition to play on receipt of login start. Advance only at the appropriate handshake, login acknowledgement and configuration acknowledgement boundaries.
2. Define dispatch by **state and direction** so a packet ID cannot fall through to a play decoder during login/configuration. Preserve framing and compression across state transitions.
3. Adapt login start/success, profile properties, optional login plugin handling and all required target fields. Preserve the current intended authentication/forwarding mode.
4. Implement configuration handling, keepalive/custom payload behavior as applicable, known-pack negotiation, registry synchronization, enabled features/tags and completion acknowledgements.
5. Prefer sending complete required registry payloads initially rather than relying on an unverified known-pack omission optimization. Ensure the payload and IDs used later agree.
6. Validate target registry content for the custom dimension and every feature sent by the server. Account for item/component-dependent registries as well as dimensions/biomes.
7. Adapt status/version checks to the new target as the actual connection implementation becomes available; reject unsupported login versions clearly.

**Complete when:** a 1.21.1 client completes the negotiated states and reaches the play handoff. Test compression, reconnection, disconnects and invalid transitions; merely advertising protocol 767 does not complete this phase.

## Phase 4 — make world join and synchronization correct

**Affected files:** `server.rs`, `player.rs`, `plot/mod.rs`, `core/src/world/storage.rs`, clientbound packets and generated registries.

1. Adapt join/respawn, player position synchronization and teleport confirmations, abilities, game mode, view distance and chunk sequencing.
2. Keep dimension definitions consistent with storage geometry, section counts, biome palettes, heightmaps and light masks. Retain baseline geometry until a separate geometry migration is proposed.
3. Adapt chunk encoding and block entities to target wire IDs. Exercise uniform and mixed palettes, negative X/Z coordinates, section edges and plot boundaries.
4. Adapt entity spawn, metadata, equipment, player list/profile properties, skins, pose and removal. Review the fork's later spawn/attack fixes and the corresponding newer upstream fixes.
5. Review damage-type and entity registry data used by client-visible effects so player actions cannot trigger client decoding/registry failures.

**Complete when:** one client can join, fly, teleport and reconnect; two clients see each other correctly; chunks and supported block entities render consistently across plot boundaries without decode errors.

## Phase 5 — restore the creative/redstone user workflow

**Affected files:** `interaction.rs`, `player.rs`, `plot/packet_handlers.rs`, `plot/commands.rs`, `plot/scoreboard.rs`, `plot/worldedit/`, chat/text and play packets.

1. Adapt placement, breaking, use-on-block, held-item changes, animation, inventory/container actions and interaction acknowledgements.
2. Map creative items to supported target block states without losing component data needed for placement or subsequent saving.
3. Adapt chat/commands, completion, system messages, signs and scoreboards. Consume required incoming fields even when the server does not use their gameplay semantics. Preserve its established chat/authentication policy.
4. Preserve `/radvance` game/nano/pico modes, `/rtps`, debug-stick behavior, plot commands and compiler commands. Confirm responses and effects in a target client.
5. Restore WorldEdit selection, set/replace, copy/paste, rotate/flip, undo/redo, load/save and completion paths. Distinguish protocol adaptations from optional command enhancements.
6. Recalculate comparator values for supported container data and verify supported item counts and block-entity conversion.

**Complete when:** existing redstone-building and debugging workflows work in a 1.21.1 client, including hotbar/creative inventory, containers, commands, signs and representative WorldEdit operations.

## Phase 6 — integrate piston actions without changing simulation timing

**Affected files:** `plot/mod.rs`, `blocks/block_entities.rs`, piston/block-update packets; review `redstone/piston.rs` and `redpiler/` boundaries.

1. Preserve the distinction between block-state IDs sent in block updates and block registry IDs used in block actions. Verify piston action and direction encoding against target data.
2. Verify moving-piston block-entity type, carried-block state and emitted NBT. The current hard-coded type `67` is explicitly uncertain and must be replaced with a verified mapping.
3. Serialize all carried-block properties, not just its name; the baseline moving-piston NBT writer leaves properties as a TODO. Treat this as data fidelity needed for the port, without altering movement timing.
4. Preserve the ordering of simulation actions and client updates; verify extension, retraction, fast pulses and observers both in the stored world and on screen.
5. Check activation/reset/flush and pending-tick transfers when automatic Redpiler is enabled. Pistons/observers currently have no compiled graph-node implementation; do not silently run a piston circuit through a backend that drops required behavior. Make the existing intended handling explicit and verify it.
6. Re-run existing schematic traces using semantic block states. Treat changes caused by ID/schema migration separately from simulation regressions; retain the known memory-cell outcome in the port report.

**Complete when:** piston/observer circuits retain their baseline server traces and render coherent target-client movement, with no newly unexplained failures. The later full-compliance work remains outstanding.

## Phase 7 — finish migration and schematic compatibility

**Affected files:** `crates/save_data/src/plot_data.rs`, `plot_data/fixer.rs` and legacy readers, `crates/core/src/player.rs`, `blocks/block_entities.rs`, `plot/worldedit/schematic.rs`.

1. Introduce a distinct plot save-format version, with its number reserved before implementation: version 1 identifies the original baseline, version 2 is used by the later 1.20.4 fork, and newest upstream already uses version 3 for fractional tick/world-send rates. The earlier proposal to use 3 would overlap that upstream format; see [the current/master comparison](CURRENT_VS_MASTER.md#persistence-plot-geometry-and-timing). Keep Minecraft data version and MCHPRS format version separate.
2. Support migration of original 1.18.2 plot/player data as a port requirement. Add support for the later 1.20.4 fork data only through its explicit reader; report unsupported formats without opening them as baseline data.
3. Convert chunk palettes, moving-piston carried IDs, supported inventory/container items, signs and pending ticks using the frozen legacy readers and reviewed mappings.
4. Version player persistence explicitly so unversioned old item IDs cannot be mistaken for target IDs. Ensure conversion is applied once and is repeatable from a backup.
5. Fix the backup helper: it currently never increments the suffix inside its loop and can hang when `.bak` and `.bak.1` both exist. Preserve existing backups and validate repeated conversion/failed-conversion paths.
6. Write converted data through a temporary file and only replace the original after successful conversion/validation. On unsupported/unmappable data, retain the source and emit an actionable error.
7. Adapt schematic DataVersion, palette properties, sign/container NBT and moving-piston state. Round-trip the fork's supported schematic formats; include Sponge v3 only with the corresponding verified loader adaptation.
8. Test normal save/load/restart and copies containing pending ticks and moving piston entities. Keep the current plot geometry; a section-count change requires a separate explicit migration.

**Complete when:** old baseline worlds and players convert on copies without silent circuit/inventory corruption, backups are preserved, new saves round-trip, and repeated loads do not reapply migration. Report the exact supported legacy formats.

## Phase 8 — validate and deliver the port

**Affected files:** target implementation, fixtures, CI workflows, README and version/support documentation.

1. Run workspace checks/tests and release compilation. Investigate every new failure relative to phase 0. Report the known piston discrepancy separately; do not describe the whole suite as passing while it fails.
2. Complete the client matrix below on fresh and migrated test worlds. Use an actual 1.21.1 client; codec/unit fixtures alone are insufficient.
3. Verify graceful shutdown, restart, save errors and compiler handoffs. Check performance against representative existing circuits to catch accidental regressions introduced by network/data changes.
4. Update CI branch coverage and documentation for the actual supported client version, connection modes, save versions, migration behavior and remaining implementation issues.
5. Produce a reproducible release build and concise validation report identifying source/data revisions and unresolved behavior.

**Complete when:** the 1.21.1 server is usable for the existing piston/redstone workflow, migration is documented and validated, and all remaining discrepancies have a clear baseline/reference classification.

## Validation matrix

| Scenario | Evidence required |
| --- | --- |
| Status and connection states | Status/ping, login, configuration, compression, disconnect and reconnect |
| World rendering | Uniform/mixed palettes, block entities, plot boundaries, negative X/Z, teleport |
| Player synchronization | Two clients, skins, pose, equipment, game-mode changes and removal |
| Building and inventory | Creative slots, held items, placement/breaking, container contents and item data |
| Commands/text | Chat, completions, scoreboard, front/back sign text, plot and tick-advance commands |
| WorldEdit | Set/replace, copy/paste, piston rotate/flip, undo/redo, schematic round trips |
| Pistons/observers | Baseline fixture traces, action packets, intermediate moving states and visible animation |
| Compiler boundary | Interpreted behavior, automatic activation policy, reset/flush and pending ticks |
| Persistence | Original-save conversion, player/container conversion, moving states, restart and backups |
| Failure cases | Malformed/truncated codecs, unsupported state/component/format, failed migration without source loss |

Use focused tests for changed codecs, mapping and migration behavior. Do not add superficial tests that only repeat implementation constants. Run broader checks at integration milestones or when changes introduce new uncertainty.

## Suggested commit sequence

| Commit group | Deliverable |
| --- | --- |
| 1 | Recovery lockfile, baseline results and packet/persistence inventory |
| 2 | Pinned data, generated ID/state mappings and legacy mapping fixtures |
| 3 | NBT/text and structured-slot primitives; frozen legacy readers |
| 4 | Explicit state dispatch, login/configuration and registry synchronization |
| 5 | Join/chunks and geometry consistency |
| 6 | Player/entity synchronization and creative inventory |
| 7 | Interactions, acknowledgements, text, signs, commands and scoreboard |
| 8 | Piston action/entity serialization and interpreted/compiler integration |
| 9 | Plot/player migration, backup fix and schematic compatibility |
| 10 | Client/release validation, CI and supported-version documentation |

Keep the components of a group together when their API changes are inseparable. Never leave a partial replacement such as removing `PackedPos` while retaining callers. Document baseline behavior changes individually instead of hiding them in broad formatting or conflict-resolution commits.

## Upstream changes to consult selectively

| Change | Reference commits | Treatment |
| --- | --- | --- |
| 1.20.4 protocol transition | `166b922`, `2e92c7c`, `709923f`, `678dae1` | Use to locate affected files and sequencing; verify all target formats/IDs anew |
| Equipment, signs, acknowledgements, metadata | `4d649aa`, `47bb8ad`, `7eff4fd`, `c83a79d` | Adapt to exact target codecs |
| Fork piston block-action and ID work | `fdc07c0`, `b641fcc` | Preserve its integration lessons; replace 1.20.4 IDs |
| Later entity fixes | `4abac26`, `283cd7f`, `8285803` | Review entity spawn/attack fields and registries |
| Schematics | `0364b17`, `680a8f0`, `6cc6959` | Carry applicable loader/data fixes without unrelated command rewrites |
| Persistence and backups | `93d8149`, `feae35e`, `0098867` | Adapt error handling, shutdown saving and backup suffix fix |
| World/redstone/Redpiler extraction | `21c694f`, `75f3718` | Defer the refactor; preserve the coherent baseline layout |
| Newer compiler/simulation changes | `557b28b`, `a1f9213`, `0a44537` | Review separately; include only with explicit behavior validation |

## After the port: full piston reference compliance

Begin this stage after phase 8, while retaining the same protocol implementation. Establish version-matched reference traces, validate the existing memory-cell expectations, and compare event ordering/intermediate states as well as final positions. Cover all facings, normal/sticky pistons, quasi-connectivity, BUD behavior, pulse lengths, observer interactions, movement limits, chains and attachments, immovable blocks, dropping, head recovery and persistence during movement.

The completion goal is **100% agreement with the agreed target reference piston behavior**. Missing piston features and remaining mismatches remain explicit until implemented and verified. Client animation correctness and server simulation compliance are separate evidence requirements.
