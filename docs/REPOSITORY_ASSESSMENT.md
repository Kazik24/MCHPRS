> Target update: Minecraft **1.21.5**, protocol 770, DataVersion 4325. This assessment records the earlier branch investigation; see [the implemented port](PORTING_1_21_5.md) for current code and validation.

# MCHPRS fork assessment and development plan

Assessment date: **2026-10-03**. Branches and remote references below describe the snapshot inspected on that date; commit hashes are the reproducible reference points. Commit dates in the appendices are author dates and do not necessarily describe integration order.

## 1. Agreed objective and sequence

1. Recover and compile the original **Minecraft 1.18.2 piston fork**, preserving the work by Kazik24 and Lord225.
2. Port that implementation to **Minecraft 1.21.1**, using upstream changes to identify affected subsystems and version-specific open-source implementations to verify the protocol.
3. After the port, bring this fork's piston implementation into **100% agreement with the original/reference piston behavior**.

The existing piston discrepancy is a limitation of **this fork's implementation**, not an inherent limitation of Minecraft pistons. Keep it visible during the port and address it in the subsequent compliance stage. Compilation and protocol compatibility do not establish simulation compliance.

## 2. Current working state

| Item | State |
| --- | --- |
| Working directory | `F:\rustrepos\MCHPRS` |
| Active branch | `recovery/piston-1.18.2` |
| Base commit | `cb3d4e2`, tag `p0.1.0`, authored 2024-05-31 |
| Minecraft version | `1.18.2` |
| Protocol version | `758` |
| Minecraft data version | `2975` |
| Plot save format version | `1` |
| Toolchain used | Rust/Cargo `1.98.1`, Windows MSVC |
| Built executable | `target/debug/mchprs.exe` |
| Implementation of the 1.21.1 port | Pending |

### Changes made during recovery

- Fetched `origin` and `upstream` to inspect the fork and current upstream references.
- Created `recovery/piston-1.18.2` from the original tag, initially using isolated checkouts for validation.
- With explicit user authorization, discarded the two uncommitted edits in `crates/network/src/packets/clientbound.rs` and `crates/network/src/packets/mod.rs` and switched the current directory to the recovery branch.
- Updated `Cargo.lock`: `time 0.3.20` failed to compile on the installed Rust version; `time 0.3.36` compiled successfully. Cargo also updated required dependencies, including Serde and its macro dependencies, and changed the lockfile format from version 3 to 4.
- Built the server and checked startup/status using a temporary configuration and temporary world on localhost.
- Saved the initial plain-text assessment in `repository-audit.txt`; this Markdown file consolidates the findings and adds the project map and detailed handoff plan.
- Removed the temporary Git worktrees after validation.

No simulation source or test expectations were changed. Existing branches, the existing stash, and the working directory's runtime configuration/world files were retained. The lockfile and assessment documents remain **uncommitted**.

## 3. Validation and its limits

| Check | Result | Interpretation |
| --- | --- | --- |
| `cargo build --locked` | Pass | Development executable built in the current working directory |
| `cargo check --workspace --all-targets --locked --offline` | Pass | Recovered baseline libraries, executable, tests, and benches type-check |
| Core piston update tests | Four pass | Existing instant/non-instant extension and update fixtures pass |
| `test_memory_cell_unaligned_nanoticks` | Fails at tick 0 | Expected `extended=true`; actual `false`; reproduced on both the original baseline and committed `piston_cherrypick_cd` |
| `test_zero_tick`, `test_reset_queue` | Both pass | Existing scheduler tests run successfully |
| Isolated startup/status request | Pass | Server starts and reports Minecraft `1.18.2`, protocol `758` |
| `git diff --check` during recovery | Pass | No whitespace errors in the tracked recovery diff |

The scheduler reset test currently schedules entries without asserting reset behavior, so its passing result provides limited coverage. The memory-cell fixture also contains a TODO to obtain expected data from real Minecraft; it must be checked against a reference trace before its expectations are treated as authoritative.

The full workspace test suite, release build, real-client login, gameplay compatibility, and complete piston compliance were **not** validated. Compiler warnings include overlapping block/item mappings and dead code; a successful build does not resolve these issues.

Reproduction commands from the repository root:

```powershell
cargo build --locked
cargo check --workspace --all-targets --locked --offline
cargo test -p mchprs_core --lib --locked redstone::tests -- --nocapture
cargo test -p mchprs_core --lib --locked redpiler::backend::queue::tests
```

The piston-test command is expected to fail on the existing memory-cell test. Do not disable it or change its expected result merely to make the suite green.

## 4. Project structure at the selected baseline

This map describes `p0.1.0` and the current recovery branch. Later upstream branches use a different crate layout.

```text
MCHPRS/
  Cargo.toml, Cargo.lock             Application/workspace manifests
  src/main.rs                       Logging and server startup
  crates/
    core/src/
      server.rs                     Server loop, login/status, version constants
      config.rs                     Configuration loading and defaults
      player.rs                     Player state, inventory and outgoing updates
      interaction.rs                Placement, breaking and block interaction
      chat.rs                       Baseline chat/text components
      permissions/, profile.rs      Permissions and player profile handling
      plot/
        mod.rs                      Plot threads, PlotWorld and interpreted ticks
        data.rs, database.rs        Plot generation/data and ownership database
        commands.rs                 Server/plot commands and tick advancement
        packet_handlers.rs          Play-state packet handling
        monitor.rs, scoreboard.rs   Timing monitor and scoreboard
        worldedit/                  Editing, clipboard and schematic handling
      world/
        mod.rs                      World trait, block/piston actions, iteration
        storage.rs                  Chunk/section storage and change tracking
      redstone/
        mod.rs                      Interpreted updates, observers and test fixtures
        piston.rs                   Piston movement, scheduling and recovery
        repeater.rs, comparator.rs   Redstone component behavior
        wire/, noteblock.rs         Wire propagation and note blocks
      redpiler/
        mod.rs, compile_graph.rs    Compiler orchestration and graph representation
        passes/                     Graph identification and optimization
        backend/queue.rs            Shared scheduling and zero-delay queues
        backend/direct/             Compiled simulation backend
      tests/                        Core simulation tests
    blocks/src/                     Block/item IDs, properties and block entities
    network/src/                    TCP connections, framing, codecs and packet dispatch
    world/src/lib.rs                Shared TickPriority and TickEntry types
    save_data/src/                  Plot/player persistence and format migration
    proc_macros/src/                Block/item definition macros
    redpiler_graph/src/             Exported graph representation
    utils/src/                      Shared helper macros/utilities
  test_data/                        Five piston/observer schematic fixtures
  crates/core/benches/               Storage/CHUNGUS benchmarks and plot fixture
  docs/                             Documentation and assessment
  .github/workflows/                Build, test and Docker workflows
  world/, schems/, logs/             Runtime data
  Config.toml                       Runtime server configuration
```

The baseline `mchprs_world` crate contains shared tick types; the **World trait and chunk storage are still in `mchprs_core`**. Redstone and Redpiler are also modules inside `core`. This distinction is central to understanding the failed integration.

The root manifest explicitly lists `proc_macros` and `redpiler_graph` as workspace members; the in-tree path dependencies are also covered by the workspace checks. `.gitignore` contains `world/`, which can hide the tracked `crates/world` directory from ordinary ignore-aware file searches. Use `git ls-files crates/world` or an explicit read when inspecting it.

### Execution and data flow

`src/main.rs` starts `MinecraftServer`. The network crate decodes incoming packets; the server handles status/login and routes players to plot threads. A plot owns `PlotWorld`, processes player interactions, advances interpreted redstone or Redpiler, and sends chunk/block/entity updates. Save-data code persists chunk palettes, block entities, and pending ticks.

Important cross-subsystem boundaries are block-state IDs, moving-piston `block_state`, `World` actions, tick ordering, and packet encoding. A port can compile while those boundaries disagree.

## 5. Core branches and their changes

| Branch/ref | Tip | Main changes and relationship | Validation in this assessment |
| --- | --- | --- | --- |
| `p0.1.0` | `cb3d4e2` | Original 1.18.2 piston feature baseline; selected starting point | Recovered build/check pass; piston tests 4 pass/1 fail |
| `recovery/piston-1.18.2` | `cb3d4e2` | New branch at the original tag, plus uncommitted lockfile compatibility fix and documentation | Build/check/startup pass |
| `master`, `origin/master` | `76b9b8c` | Fork's September 2024 upstream snapshot; 1.20.4 protocol, crate extraction, forwarding, persistence and test updates | All-target check passes |
| `merge_piston` | `56ad793` | Large September 2024 integration attempt; mixes upstream World extraction with retained core redstone/Redpiler modules | All-target check fails in integration tests |
| `cherrypick_piston` | `26f1028` | Selective 1.20.4 upgrade with upstream merge parents; retains the original core architecture | History/source inspection; no standalone build run |
| `piston` | `a0a7efb` | Continues `cherrypick_piston`; pose/skin metadata fix and Docker Compose cleanup | Local tip not separately checked |
| `origin/piston` | `dd01a49` | Five later commits: entity spawn/attack fix, schematic traversal, target ID fix and formatting | All-target check passes |
| `piston_cherrypick_cd` | `c1d2544` | Sibling of the remote piston continuation; selective chat, login plugin and metadata updates plus traversal/cleanup | Committed tree checks pass; piston tests 4 pass/1 fail |
| `observer` | `83a463d` | Observer implementation and update handling; merged into the original piston lineage | Ancestor of `p0.1.0`; no separate build |
| `instant-piston` | `21b39da` | Instant piston scheduling and tick/update refactoring | Ancestor of `p0.1.0`; local tip two commits behind remote |
| `origin/instant-piston` | `07a5376` | Adds interpreted zero-delay scheduling and logging/tick refactoring | Ancestor of `p0.1.0` |
| `update-piston` | `fc2e2f2` | Restricts piston updates to necessary neighbors, with timing and animation work | Ancestor of `p0.1.0`; local tip one commit behind remote |
| `origin/update-piston` | `2726145` | Adds instant/non-instant piston-observer interaction tests | Ancestor of `p0.1.0` |
| `piston-auto-instant` | `471a9a7` | Separate automatic-extension experiment; removes observer tick/update handling | One unique commit outside `p0.1.0`; retain for later review |
| `remove`, `origin/piston-instant` | `f4c3833` | Earlier instant-piston work and a moving-piston entity adjustment | Ancestor of `p0.1.0`; no separate build |
| `upstream/master` | `d492e43` | Current upstream snapshot; still declares 1.20.4; major compiler/tooling/data changes since fork master | Source/history inspection; not built |
| `upstream/mc-1.18.2` | `970b3c8` | Upstream 1.18.2 continuation ending before the 1.20.4 protocol integration | Historical reference |
| `upstream/dev/stack/mc-1.21.8` | `4660d75` | Work-in-progress 1.21.8 protocol/item/palette/sign changes, protocol `772` | Source/history inspection; not built |

No 1.21.1 implementation was found in the named fork branches inspected. Other upstream experiments/releases are recorded in the full reference inventory in the appendices; they were not individually audited.

### Original feature work to preserve

The original fork contains 116 commits absent from fork master after their shared ancestor, including merges. Representative contributions are:

| Area | Representative commits | Behavior/work to retain |
| --- | --- | --- |
| Piston blocks/entities | `4cdf7d3`, `14b4134`, `a3144db` | Piston/head/moving-piston representation and conversion |
| Scheduling | `ff1adb5`, `1011d46`, `3d33438`, `94619ce` | Shared scheduler, zero-delay updates, moving-block timing |
| Piston update behavior | `7be984d`, `fc2e2f2`, `13352eb`, `c50b561` | Neighbor update selection, BUD/instant behavior, state recovery |
| Observers | `83a463d`, `e0368fe`, `8f77198` | Observer transitions and propagation ordering |
| Animation/actions | `4cc96da`, `b154b67` | Piston block actions and animation handling |
| Fine-grained advancement | `f587ba6`, `9c1b491`, `e00db85` | Nano/pico tick advancement and corrected update order |
| WorldEdit | `cc99e57`, `6461ff8`, `293b020` | Piston rotation/flip, NBT errors and completion |
| Debugging/experimental behavior | `a981a0b`, `ffba8c1`, `e213d66`, `f8685ba` | Debug stick, power display, cursed behavior, instant repeaters |
| Memory/storage | `fb053a6`, `05a37ce` | Smaller block entities and packed positions |
| Fixtures | `2726145`, `5982f3e`, `cb3d4e2` | Piston/observer update and memory-cell tests |

These are historical features, not a claim that every feature matches vanilla reference behavior.

### Large merge attempt: `merge_piston`

Seven commits follow `p0.1.0`: `bae6a55`, `048b42e`, `025eaad`, `3b5c860`, `c2efba6`, `8e8a53a`, and `56ad793`. They combine a large imported snapshot with formatting, block-macro fixes and `BlockEntity::from_nbt` API adaptation.

`bae6a55`, despite its merge-oriented subject, has **one parent**, `cb3d4e2`. Git therefore does not record the imported upstream snapshot as a merged parent. This complicates later ancestry-based integration.

Its libraries and executable target passed checking, but `tests/components.rs` imports `mchprs_redstone` and `mchprs_redpiler`, which are not available as crates on that branch. The all-target failure is concrete evidence of incomplete architectural integration. Do not describe this branch as having a validated full build/test suite.

### Selective integration: `cherrypick_piston`

This branch integrates the upstream 1.18.2 cleanup and 1.20.4 changes incrementally. It includes real merge parents at `2b4d877`, `036bb6d`, and `4544bc0`, the last reaching upstream `adb426a`.

Changes cover block/item IDs, configuration/login, world height, text/chat, signs, equipment, scoreboards, plot save format 2, acknowledgements, player list, compiler flushes, and plot selection. Fork-specific follow-ups include F3+N handling (`4b6127c`), piston block-state IDs (`fdc07c0`), block-action encoding (`b641fcc`), and the observer draft (`26f1028`). It keeps World/redstone/Redpiler inside core.

Across `p0.1.0` and the later piston lineage, the compared piston engine, redstone tests, scheduler and core tests are unchanged; the redstone subsystem difference is the empty-container comparator fix. Most version-update work is in networking, block/item tables and integration code.

### Later piston continuations

`piston` adds `3054b51` (pose metadata/skin parts) and `a0a7efb` (Docker Compose cleanup). Its remote adds:

- `4abac26`: player entity spawn and attack handling.
- `2de260e`, `48222f1`: formatting.
- `6cc6959`: schematic subdirectory traversal.
- `dd01a49`: target block-state ID correction.

`piston_cherrypick_cd` diverges from the local `piston` tip with:

- `9f52a9a`: remove the chat-message sender argument.
- `e49a8de`: directory traversal.
- `42905bf`: login plugin request/response packets.
- `9629658`: pose metadata type and metadata on join.
- `c1d2544`: remove a redundant derive during cleanup.

Reflog inspection shows additional world-extraction attempts and resets in October 2024. Those abandoned attempts explain part of the integration history but are not the selected baseline.

The discarded working edits removed `PackedPos`, changed positional packet APIs, removed `CBlockAction` and `CEntityStatus`, introduced player-property changes and imported `bitvec` without declaring it. The immediate checked build errors were missing `bitvec` and missing `PackedPos`; other removed APIs still had callers. The committed branch itself passed checking.

## 6. Changes in master to use as references

See [the current-server versus newest-master comparison](CURRENT_VS_MASTER.md) for the detailed behavior, compatibility and tooling differences at `d492e43`, including upstream save format 3 and the absence of a version-1 plot converter.

The original fork and fork master share ancestor `1f33340` (authored 2024-04-05). Fork master has **70 subsequent commits**; current upstream has **159 additional commits** beyond that snapshot. The appendices provide complete commit lists.

| Area | Fork-master commits | Port relevance/recommendation |
| --- | --- | --- |
| Protocol/data foundation | `166b922`, `2e92c7c`, `709923f`, `678dae1` | Use as a map of changes from 1.18.2: block/item IDs, configuration/login, registries, packet fields and height. Replace version-specific values with verified 1.21.1 values |
| Commands/text | `c30c8bb`, `9b62b41`, `9efcdb2`, `404aeca` | Adapt command/chat handling and component serialization |
| Scoreboards/equipment/signs | `f69a4fe`, `4d649aa`, `47bb8ad` | Review formats and use exact target codecs |
| Interactions/player presentation | `7eff4fd`, `48d688b`, `83e3803`, `c83a79d` | Preserve acknowledgements, player list/profile properties and metadata correctness |
| Persistence | `dd37ceb`, `93d8149`, `feae35e` | Adapt explicit migration, error handling and graceful-shutdown saving |
| WorldEdit | `b5d751c`, `bce8883`, `0364b17`, `680a8f0` | Carry relevant count/selection/Sponge v3/block-entity fixes |
| Simulation/compiler | `68213ab`, `15a93a6`, `68d8c59`, `a02d76f`, `21b62fc`, `ae72401` | Review empty containers, trapdoors, flush/pending ticks, coalescing and wire outputs against fork semantics |
| Crate architecture | `21c694f`, `75f3718` | World extraction and redstone/Redpiler extraction; treat as a separate refactor, not a prerequisite for the protocol port |
| Proxy/network integration | `9116f7a`, `df8dbe7`, `9d12cef` | Legacy forwarding removal, plugin packets and Velocity forwarding; decide supported integration explicitly |
| Infrastructure/tests | `0fd5d75`, `62d2963`, `ab76784`, `012df2d`, `76b9b8c` | Dependencies, Docker/CI and backend/component tests; adapt tests to the retained architecture |

Newer upstream work adds generated block data, more graph/compiler changes, RIL tooling, world export, schematic offsets/bounds fixes, timing changes and persistence fixes. Particularly useful review candidates include `283cd7f`/`8285803` (player entity/attack), `557b28b` (interpreted repeaters), `a1f9213` (removing an inaccurate optimization), `1a6a142`/`df0f3d1` (editing bounds), `db1f62f` (generated IDs), and `0a44537` (pending ticks and initial state through optimization).

Each candidate must be examined for applicability. The newer graph architecture and edition/dependency updates are substantial independent work; importing them wholesale would expand the port and reintroduce integration risk.

## 7. Observations and recommendations

### Preserve simulation behavior during the protocol port

Keep the current piston engine, observers, queue ordering, advancement commands and fixtures intact while replacing wire formats and versioned data. Record baseline outcomes so a new failure can be distinguished from an existing implementation discrepancy. Review any upstream simulation fix separately and validate its effect before including it.

### Retain one coherent architecture

Start with the selected baseline layout. If crate extraction is desired later, move the World trait, storage and action hooks together, then move redstone and Redpiler with their dependencies and tests. The fork requires `schedule_half_tick`, piston/block actions, zero-delay scheduling and other hooks that a simple upstream file replacement can lose.

### Generate or validate versioned IDs

Hand-maintained mappings already produce overlapping-pattern warnings. Prefer a reproducible import or generator from pinned 1.21.1 data, while retaining the fork's typed properties. Validate round trips and defaults for pistons, heads, moving pistons, observers, wires, repeaters and supported solid blocks. Audit block-state, item, block-entity, entity, sound and metadata IDs independently.

`BlockEntity::MovingPiston` currently returns the hard-coded value `67` with an uncertainty comment. Treat that as unverified and check it against the target version's block-entity registry. Do not reuse a block-state ID as a block/entity registry ID.

### Plan save migration before running on existing worlds

The baseline stores version-specific IDs and plot format 1; later fork branches use format 2. A 1.21.1 migration must translate chunk palettes, relevant inventory data and moving-piston `block_state`, and preserve pending ticks and block-entity semantics. Determine which input save versions will be supported. Validate conversion with copied fixtures and explicit backups before using existing runtime worlds.

### Validate the interpreted/compiled boundary

Redpiler's `identify_block` does not produce piston or observer graph nodes. `redpiler_graph` has a piston enum entry marked TODO, which does not establish backend support. Validate how automatic Redpiler behaves on piston/observer circuits, including activation, reset, flush and transfer of pending ticks. Default configuration currently enables automatic Redpiler.

### Keep a visible correctness backlog

Track the memory-cell discrepancy separately from protocol work. Strengthen weak scheduler tests when changing scheduler behavior; establish authoritative reference traces before updating expected timings. Do not claim full compliance from the present small fixture set.

CI on the baseline targets `master` pushes/PRs and uses older actions. Update its branch coverage and toolchain/dependency expectations when enabling CI for the recovery/port work, while keeping the known failure explicit.

## 8. Protocol references for Minecraft 1.21.1

Minecraft 1.21.1 uses **protocol 767**, as recorded in [PrismarineJS's version data](https://github.com/PrismarineJS/minecraft-data/blob/master/data/pc/1.21.1/version.json).

Use [the exact 1.21.1 data directory](https://github.com/PrismarineJS/minecraft-data/tree/master/data/pc/1.21.1) and [protocol schema](https://github.com/PrismarineJS/minecraft-data/blob/master/data/pc/1.21.1/proto.yml). Check [dataPaths.json](https://github.com/PrismarineJS/minecraft-data/blob/master/data/dataPaths.json) because some registries/datasets are shared with another version. Pin the source commit before generating or importing assets.

[MCProtocolLib tag `1.21-1`](https://github.com/GeyserMC/MCProtocolLib/tree/1.21-1) is an implementation reference for codecs; cross-check it against the exact target schema rather than assuming a tag name makes every behavior an exact match. [Its release notes](https://github.com/GeyserMC/MCProtocolLib/releases/tag/1.21-1) identify its 1.21 work.

[Glowstone's development configuration](https://github.com/GlowstoneMC/Glowstone/blob/dev/gradle.properties) declares `mcVersion = 1.19` in the inspected source. It can inform architecture, but should not be the primary source for 1.21.1 packet formats.

Upstream's `mc-1.21.8` branch can suggest integration points for components, palettes and signs, but uses a different protocol version and must be checked field by field.

## 9. Next-step plan and acceptance criteria

The detailed implementation sequence, file ownership and validation gates are in [the 1.21.1 porting plan](PORTING_1_21_1.md).

### Stage A — preserve the recovered baseline

- Review and commit the lockfile compatibility fix and this handoff when desired.
- Record the tested toolchain and baseline test outcomes.
- Retain the old branches and the separate automatic-piston experiment as references.

**Status:** recovery, development build and startup/status check completed. Commit/publication was not performed.

### Stage B — prepare versioned data and compatibility boundaries

- Pin the protocol/data reference revisions.
- Inventory supported incoming/outgoing packets, registries and block/item definitions.
- Establish generated/validated target ID tables and conversion mappings from the supported save versions.
- Define how internal block states map to target wire IDs and persisted IDs, including moving piston entities.

**Acceptance:** required IDs and state mappings are reproducible, supported block states round-trip correctly, and migration fixtures preserve their circuit structure and pending updates.

### Stage C — implement status, login and configuration

- Add the configuration state missing from the 1.18.2 baseline and implement its transitions/acknowledgements.
- Adapt login, compression and packet dispatch to 1.21.1.
- Send the required registry data and handle the target known-pack exchange and configuration completion.
- Verify dimension/biome and other registry references used when joining the world.

**Acceptance:** an actual 1.21.1 client completes configuration and enters play without decoding or registry errors. Status advertising alone is insufficient.

### Stage D — implement play codecs and interactions

- Adapt anonymous NBT/text components and structured item components.
- Update chunk/palette/light encoding, entity spawn/metadata/equipment, player list, commands, chat, scoreboards and signs.
- Update creative inventory and container interactions, placement/breaking acknowledgements, block updates, piston block actions and animation data.
- Review selected upstream fixes individually without changing simulation timing as a side effect.

**Acceptance:** client join/reconnect, rendering, supported block placement/breaking, inventory, signs, commands, WorldEdit and piston animation work with a 1.21.1 client; existing fixture outcomes remain accounted for.

### Stage E — persistence and end-to-end port validation

- Validate save/load/restart and supported old-format conversion on copies.
- Verify moving piston state, observer state, pending ticks and advancement commands across persistence and compiler transitions.
- Run relevant workspace tests, release compilation and client scenarios; add meaningful codec/migration tests for changed behavior.
- Update version documentation and CI to reflect the actual supported target and known implementation discrepancy.

**Acceptance:** a usable 1.21.1 piston server with documented migration behavior and no unexplained regressions against the recovered baseline. This milestone does not assert complete piston compliance.

### Stage F — full piston reference compliance

- Agree on authoritative reference implementations/versions and record reproducible circuit traces.
- Verify/update the memory-cell fixture against those traces, then fix the implementation discrepancy.
- Cover normal and sticky pistons, all facings, direct/quasi-connectivity/BUD behavior, zero/short pulses, observer interactions, update order, movement limits, immovable blocks, block dropping, head recovery and chained interactions.
- Compare event order and intermediate states at appropriate tick granularity, rather than only final block positions.
- Verify client-visible animation separately from server simulation, plus save/load and relevant compiler transitions.

**Acceptance:** 100% agreement with the target version's agreed reference piston behavior, backed by comprehensive fixtures. Outstanding mismatches and missing piston features remain documented until resolved.

## 10. Detailed historical appendices

The following Git-generated lists preserve changes on the core branches and all fetched reference tips. They describe historical commits, not additional changes made during this assessment. Branch differences use explicitly named ranges; merged feature work appears in the original fork appendix and is not duplicated under every ancestral experimental branch.

### A. Original piston fork: shared ancestor to p0.1.0

Range: `1f33340..p0.1.0`. All commits in this range, including merge commits.

```text
090dd4f 2024-03-13 Kazik24 | piston: add todos
ed93315 2024-03-15 Kazik24 | fix compilation on 32 bit targets
9b98c0f 2024-03-15 Kazik24 | rm duplicated buffer
9e25da5 2024-03-17 Kazik24 | adding piston blocks wip..
4cdf7d3 2024-03-18 Kazik24 | add piston block states
a079501 2024-03-18 Kazik24 | piston head varian, wip..
d78e9b3 2024-03-18 Lord225 | PistonHead ?
fb64dad 2024-03-18 Kazik24 | Add piston head, moving piston placeholder
03be5e2 2024-03-19 Lord225 | pistion update
14b4134 2024-03-20 Kazik24 | Moving piston BlockEntity
333520f 2024-03-21 Kazik24 | Fix sign rotation, decrese Block size, const block id
d8203f0 2024-03-21 Lord225 | def of is_piston_powered
a3144db 2024-03-21 Lord225 | Add RedstonePistonHead conversion and update piston state
e6cead4 2024-03-21 Kazik24 | Add todos - piston update
d89e04b 2024-03-22 Lord225 | Merge branch 'piston'
2075227 2024-03-23 Kazik24 | Improve test utility
5dff962 2024-03-23 Kazik24 | Assert chungus tests after 1000 ticks
3d9cfc1 2024-03-23 Kazik24 | Tests for compiled and interpreted ticks
febbda5 2024-03-24 Kazik24 | Ultra buggy working piston
8086f5c 2024-03-24 Kazik24 | Fix piston pushing immovable blocks/redstone
7252e49 2024-03-25 Lord225 | PistonType
dbfe772 2024-03-26 Lord225 | pistion extend / retract logic
7c929b1 2024-03-26 Lord225 | refactoring extend function
5bcb18c 2024-03-26 Lord225 | remove info!()
a523be6 2024-03-26 Lord225 | retract cleanup
86f4da4 2024-03-26 Lord225 | ajusting logic flow
7e56a5f 2024-03-26 Lord225 | cleanup
6476a84 2024-03-26 Lord225 | push column
47f14df 2024-03-26 Lord225 | fixing logic
63f1565 2024-03-26 Lord225 | cleanup
63915ff 2024-03-26 Lord225 | Merge commit
0117ab5 2024-03-26 Lord225 | ,.
10cc016 2024-03-26 Lord225 | comment
9d01113 2024-03-27 Lord225 | clear a little
afbb3f8 2024-03-27 Lord225 | world.schedule_tick for pistons
35cd4f5 2024-03-27 Lord225 | observer
a592bfa 2024-03-27 Lord225 | piston optimalizations
5cac9f1 2024-03-27 Lord225 | fixing piston powered logic
2961747 2024-03-27 Lord225 | piston head is not solid
ff1adb5 2024-04-06 Kazik24 | Refactor tick scheduling
1011d46 2024-04-06 Kazik24 | Use TickScheduler for both interpreted and compiled backend
377cdec 2024-04-06 Kazik24 | Piston with 1 tick delay
a388f80 2024-04-08 Kazik24 | Refresh branch from 'master'
3406217 2024-04-08 Lord225 | instant
f4c3833 2024-04-08 Lord225 | .
da33a6f 2024-04-08 Lord225 | use block_state instead
358ef26 2024-04-08 Lord225 | Refactor piston update and tick logging in redstone piston module
9e42216 2024-04-08 Kazik24 | Increase ticks 2x
391f16e 2024-04-08 Lord225 | Merge branch 'instant-piston' o instant-piston
c05dde2 2024-04-08 Lord225 | fmt
c104406 2024-04-08 Lord225 | Refactor logging in redstone piston module
7492d16 2024-04-08 Kazik24 | add pending tick check
b097676 2024-04-08 Kazik24 | make pistons transparent
08e0463 2024-04-08 Lord225 | Refactor logging and tick handling in redstone piston module
cf6f7d7 2024-04-08 Kazik24 | set block only when there is entity
21b39da 2024-04-09 Lord225 | Refactor piston update and tick handling in redstone piston module
3d33438 2024-04-10 Kazik24 | Working zero tick scheduling for interpreted backend
5d7131a 2024-04-10 Kazik24 | Zero tick logic with block entities
07a5376 2024-04-10 Lord225 | Refactor logging and tick handling in redstone piston module
c0ed28c 2024-04-11 Lord225 | Refactor tracing configuration in Cargo.toml and piston module
e71dc74 2024-04-11 Lord225 | .
fb053a6 2024-04-12 Kazik24 | reduce size of BlockEntity
f8685ba 2024-04-13 Kazik24 | working instant repeaters!
3247507 2024-04-13 Lord225 | clean
a981a0b 2024-04-13 Kazik24 | add debug stick
490f2a6 2024-04-13 Lord225 | Refactor block entity loading and container handling
ff5e722 2024-04-13 Kazik24 | Fix piston block placing, error handling
ffba8c1 2024-04-13 Kazik24 | Show redstone power on debug stick
e213d66 2024-04-13 Lord225 | /curse & /bless
6461ff8 2024-04-14 Lord225 | proper error handeling in nbt loading
b5fae4e 2024-04-14 Lord225 | fmt
293b020 2024-04-14 Lord225 | autocomplete traversal
c9ea87b 2024-04-14 Lord225 | better tab compl
4cc96da 2024-04-14 Kazik24 | Piston animation
d8e176a 2024-04-14 Lord225 | Add disable_block_actions flag to PlotWorld struct
0fac8cf 2024-04-15 Kazik24 | PlotWorld constructor
a5d18b3 2024-04-16 Kazik24 | Optional props in blocks! and items! macros
c39ae6d 2024-04-17 Kazik24 | fix piston head parsing from MC names
041c58d 2024-04-15 Lord225 | observer
99d78de 2024-04-15 Lord225 | dfgghh
83a463d 2024-04-19 Lord225 | observers
c410bc1 2024-04-19 Lord225 | Merge branch 'observer' into piston
55c31be 2024-04-21 Lord225 | extended
673a81a 2024-04-21 Lord225 | head_block handling
686a5a4 2024-04-24 Kazik24 | fix tests
e0368fe 2024-04-24 Kazik24 | Observer on state change
6bea7f5 2024-04-25 Kazik24 | Update only necessary blocks around piston, use only raw world.set_block() in piston logic
7be984d 2024-04-25 Kazik24 | Less aggressive updates for pistons, BUD works, tests pass
b154b67 2024-04-27 Kazik24 | Fix animation, faster pending tick check
0bad8be 2024-04-27 Kazik24 | Refactor PlotWorld accessors
68dcfdf 2024-04-28 Kazik24 | Refert piston to 3 game tick delay
76d2a47 2024-04-28 Kazik24 | Make piston delay 2gt, and lock updates for 3gt
81b24b2 2024-04-29 Lord225 | Refactor redstone module and piston logic
8d21c3a 2024-04-29 Lord225 | no logs
fc2e2f2 2024-04-29 Lord225 | Refactor piston update logic to only update necessary blocks
a3b7da8 2024-04-29 Lord225 | refactor on_piston_state_change
2726145 2024-04-29 Kazik24 | Add tests for (non)instant piston observer interactions
5982f3e 2024-04-30 Kazik24 | More tests for piston updates
0ff79f6 2024-04-30 Kazik24 | Merge branch 'update-piston' into piston
f587ba6 2024-04-30 Kazik24 | Add /nadv /nanoadvance commands for nano-ticks
94619ce 2024-05-01 Kazik24 | Adjust nanotick advancing, refactor TickScheduler, fix pushed block update
05a37ce 2024-05-01 Kazik24 | Refactor, PackedPos type - will be used in future for player local chunk updates
f5b1cf6 2024-05-01 Kazik24 | Refactor other uses of write/read_position
9c1b491 2024-05-05 Kazik24 | merge /nanoadvance with /radvance, correct nanoticks advancing, add picoticks advancing
8f77198 2024-05-05 Kazik24 | Fix observer line updates
e00db85 2024-05-06 Kazik24 | Fix radv nano-ticks wrong update orders
cc99e57 2024-05-08 Kazik24 | WE piston rotate/flip
13352eb 2024-05-10 Kazik24 | Fix piston base updates in instants, all update tests pass!
7843ee5 2024-05-12 Kazik24 | Testing piston dropping blocks
133526a 2024-05-12 Kazik24 | Profiling report
f786a78 2024-05-15 Lord225 | adding missing solid blocks
bdae20c 2024-05-14 Kazik24 | Switching gamemodes with F3 + N
eef3f10 2024-05-15 Lord225 | Merge branch 'piston' of https://github.com/Kazik24/MCHPRS into piston
c50b561 2024-05-23 Lord225 | piston recovery & illigal state handling
6bd243d 2024-05-28 Kazik24 | fmt, fix test_reset_queue
cb3d4e2 2024-05-31 Kazik24 | Add test_memory_cell_unaligned_nanoticks
```

### B. Fork master: 2024 upstream changes after the shared ancestor

Range: `1f33340..master`. All commits in this range, including merge commits.

```text
e9bea82 2024-06-14 StackDoubleFlow | Create nix devshell
bd63fca 2024-06-09 Copper | Another two-letter addition
b5d751c 2024-04-25 Piet Schirm | Fixed //count incorrectly uwraping arguments The execute_count function unwraped the argument to a pattern instead of a mask fixes #115
066dffb 2024-04-23 Bram Otte | play note when adjusting noteblock pitch
bef7065 2024-04-23 Bram Otte | don't play note when tuning a noteblock if it is blocked
72d4227 2022-09-15 Olek47 | Fix issue #72
1d5817b 2022-09-25 Olek47 | Handle the issue another way
568d0d1 2024-07-22 dependabot[bot] | Bump openssl from 0.10.60 to 0.10.66
07f4c32 2024-07-30 StackDoubleFlow | Make all crates inherit the same workspace package fields
970b3c8 2024-07-30 StackDoubleFlow | redpiler: remove NodeType::is_io_block
166b922 2024-04-09 StackDoubleFlow | Update blocks and items for 1.20.4
2e92c7c 2024-06-01 StackDoubleFlow | More 1.20.4 changes
06c25f5 2024-06-09 Copper | Literally just add two letters (option->optional)
10ec94c 2024-06-17 StackDoubleFlow | Switch back to my nbt fork on github
709923f 2024-06-17 StackDoubleFlow | Increase default world height and more 1.20 fixes
678dae1 2024-06-18 StackDoubleFlow | The client is finally able to connect with 1.20.4
81b54a2 2024-06-18 StackDoubleFlow | Add placeable stone bricks and fix plot generation
c30c8bb 2024-06-18 StackDoubleFlow | Fix executing commands
9b62b41 2024-06-18 StackDoubleFlow | Fix chat color codes
9efcdb2 2024-06-18 StackDoubleFlow | Rename ChatComponentBuilder -> TextComponentBuilder
abd700a 2024-06-22 StackDoubleFlow | Add trace logging for network traffic
404aeca 2024-06-22 StackDoubleFlow | Fix TextComponent::extra not being written
f69a4fe 2024-06-22 StackDoubleFlow | Fix scoreboard packets for 1.20
dd37ceb 2024-06-22 StackDoubleFlow | Bump plot data version to 2
68213ab 2024-06-22 StackDoubleFlow | Fix crashes caused by comparators and empty containers
4d649aa 2024-06-22 StackDoubleFlow | Fix Set Equipment packet
47bb8ad 2024-06-22 StackDoubleFlow | Update sign NBT
7eff4fd 2024-06-23 StackDoubleFlow | Send AcknowledgeBlockChange packet after interaction
e6624ec 2024-06-23 StackDoubleFlow | Fix io-only flag also showing as update in scoreboard
651f016 2024-06-23 StackDoubleFlow | Fix block id for target
68d8c59 2024-06-23 StackDoubleFlow | Flush redpiler on /radv and use block
48d688b 2024-06-26 StackDoubleFlow | Show players in player list
d04b87b 2024-06-26 StackDoubleFlow | Set default redpiler scoreboard state to stopped
93d8149 2024-06-26 StackDoubleFlow | Better handle errors when failing to load or save plot
bce8883 2024-06-26 StackDoubleFlow | Add /plot select command
adb426a 2024-06-30 StackDoubleFlow | Remove old deprecated vscode rust-analyzer setting
21c694f 2024-06-30 StackDoubleFlow | Fully seperate world module from core and rip out rotted code
5d6531d 2024-06-30 StackDoubleFlow | Remove sender arg from send_chat_message
75f3718 2024-06-30 StackDoubleFlow | Split out redstone and redpiler crate
0fd5d75 2024-06-30 StackDoubleFlow | Update dependencies
9b9d054 2024-06-30 StackDoubleFlow | Fix some fields not public in network crate
da77c08 2024-06-30 StackDoubleFlow | Make the default plot scale 5
feae35e 2024-06-30 StackDoubleFlow | Fix plot not saving in graceful shutdown
62d2963 2024-06-30 StackDoubleFlow | Fix dockerfile and CI
15a93a6 2024-06-30 StackDoubleFlow | Fix default trapdoor half
181251e 2024-06-30 StackDoubleFlow | Fix compiling world crate without networking feature enabled
ae72401 2024-07-01 StackDoubleFlow | redpiler: Add wire-dot-out flag
a02d76f 2024-07-04 StackDoubleFlow | Add JITBackend::has_pending_ticks()
9116f7a 2024-07-06 StackDoubleFlow | Remove support for legacy ip forwarding
df8dbe7 2024-07-21 StackDoubleFlow | network: add login plugin request and response packets
21b62fc 2024-07-21 StackDoubleFlow | redpiler: rerun coalesce pass logic to help with repeating patterns
43019f5 2024-08-12 StackDoubleFlow | Resolve merge conflicts that slipped through the rebase
9d12cef 2024-08-18 StackDoubleFlow | Implement velocity modern forwarding support
83e3803 2024-08-18 StackDoubleFlow | Propogate player properties
cebe4ca 2024-08-18 StackDoubleFlow | Make all skin parts shown as default
c83a79d 2024-08-18 StackDoubleFlow | Fix pose metadata type and send metadata when player joins
ff473fa 2024-08-19 StackDoubleFlow | Remove unused import
0364b17 2024-08-19 StackDoubleFlow | Implement Sponge v3 schematic loading
680a8f0 2024-08-23 StackDoubleFlow | Fix lowercase block entity id tag in schematic
96c8bc6 2024-08-27 Bram Otte | Remove rtps limit
87bca1f 2024-09-03 dependabot[bot] | Bump actions/download-artifact from 3 to 4.1.7 in /.github/workflows
62748da 2024-09-03 StackDoubleFlow | ci: bump upload-artifact version for macos
ab76784 2024-09-16 StackDoubleFlow | tests: add tests for lever, trapdoor, lamp, and torches
6e2d211 2024-09-16 StackDoubleFlow | tests: include backend information when assert fails
012df2d 2024-09-17 StackDoubleFlow | tests: add repeater test
b44851b 2024-09-17 StackDoubleFlow | ci: update macos build to use upload-artifact@v4
8ed2e5e 2024-09-17 StackDoubleFlow | ci: update rust-chache to v2
a85046b 2024-09-17 StackDoubleFlow | rustfmt
dcae6f9 2024-09-18 StackDoubleFlow | Document velocity support in readme
76b9b8c 2024-09-20 StackDoubleFlow | tests: seperate test backends into individual tests
```

### C. Current upstream: changes beyond fork master

Range: `master..upstream/master`. All commits in this range, including merge commits.

```text
59ee1b8 2024-12-25 StackDoubleFlow | Document redpiler usage in README
565564f 2024-12-21 StackDoubleFlow | Turn off auto_redpiler by default in config
8f4f074 2024-12-22 StackDoubleFlow | worldedit: generate command completion from worldedit
7a9f460 2024-12-22 StackDoubleFlow | worldedit: show correct command name in help
12fc9c8 2024-12-22 StackDoubleFlow | worldedit: add completion for command aliases
c98bc6d 2024-12-25 Bram Otte | Add tickn method to redpiler JITBackend trait
f618a10 2024-12-25 Bram Otte | run cargo fmt
af56a3e 2024-07-16 Drew Smith | add glowstone placement (only broken noteblock instrument)
e1705bf 2025-01-07 StackDoubleFlow | Fix 1.20.4 item id for glowstone
2545b25 2025-01-14 Paul1365972 | Improve github action workflows
10d5953 2025-01-15 Paul1365972 | Make clippy stop complaining about noteblock pitch LUT
e63c7b1 2025-01-18 StackDoubleFlow | tests: add test for repetaer t-flip-flop
c7b0960 2025-01-19 StackDoubleFlow | tests: add test for 2t pulse gen using comparator
45d80d2 2025-01-19 StackDoubleFlow | tests: add test for 1t pulse gen using comparator
c8655b0 2025-01-25 StackDoubleFlow | redpiler: input_search: stop searching wire after distance > 15
5a014da 2025-01-25 StackDoubleFlow | tests: add tests for wire signal strength
bdeb5a9 2025-01-30 StackDoubleFlow | Move all dependencies into workspace
100a5a1 2025-01-30 StackDoubleFlow | Add missing serde feature to redpiler_graph and text crates
2305be6 2025-01-30 StackDoubleFlow | Remove unused dependencies
70cecfd 2025-01-30 StackDoubleFlow | Properly switch to using git version of hematite-nbt
931cd3b 2025-01-30 StackDoubleFlow | Update dependencies
3d240d2 2025-01-30 StackDoubleFlow | Remove use of deprecated rand functions
283cd7f 2025-04-15 StackDoubleFlow | core: fix player entity id
8285803 2025-04-15 StackDoubleFlow | network: add player_attack damage type to resolve client crash
a1f9213 2025-05-01 StackDoubleFlow | Remove inaccurate AnalogRepeaters pass
ef8cba6 2025-05-01 StackDoubleFlow | flake update
557b28b 2025-05-01 StackDoubleFlow | Fix repeater inaccuracy in non-redpiler backend
b333b2c 2025-05-01 StackDoubleFlow | nix: remove flake-utils input from rust-overlay
a2afaca 2025-08-02 StackDoubleFlow | redpiler: add ss range analysis, and rustfmt
32121af 2025-08-02 StackDoubleFlow | redpiler: add RIL dumping
17b66f8 2025-08-02 StackDoubleFlow | redpiler: fix constant coalesce removing newly added constant
bce71dd 2025-08-02 StackDoubleFlow | redpiler: print block locations in RIL
3601be3 2025-08-02 StackDoubleFlow | redpiler: fix SSRangeAnalysis not propogating
cc95682 2025-08-02 StackDoubleFlow | redpiler: remove unused includes in UnreachaableOutput pass
b7a0981 2025-08-02 StackDoubleFlow | redpiler: add --print-after-all flag to dump RIL after every pass
c1e3463 2025-08-02 StackDoubleFlow | redpiler: add --print-before-backend option
ff58993 2025-08-02 StackDoubleFlow | redpiler: fix crash in ConstantCoalesce if there are many subgraphs
059b285 2025-08-02 StackDoubleFlow | redpiler: fix issues with SSRangeAnalysis and support transient state
75de766 2025-08-02 StackDoubleFlow | redpiler: allow unknown inputs when propogating in last stage of SSRangeAnalysis
5c0afc6 2025-06-20 Piecuu | Fix spelling mistake in Redpiler.md
9718694 2025-01-21 Bram Otte | Optimize get_bool_input and get_bool_side by making sure signal strength buckets add up to 255
8870d40 2025-08-03 StackDoubleFlow | Update dependencies
618d9fd 2025-02-22 Astrid Lauenstein | Fix the clippy warnings
b70e906 2025-08-03 StackDoubleFlow | Fix more clippy warnings
46160d4 2024-08-27 Bram Otte | Fix parsing of combined flags in worldedit commands crashing the plot
7c16aaa 2025-09-09 StackDoubleFlow | redpiler: add lexer for ril
1b8d5d6 2025-09-09 StackDoubleFlow | core: fix //rstack undo clipboards being applied in reverse order
a423ac4 2025-09-11 StackDoubleFlow | blocks: add end_portal_frame block and ss override
ebc330c 2025-09-27 StackDoubleFlow | [world_exporter] create world conversion tool
5de3ec2 2025-10-04 Paul1365972 | docs: add worldsenrate command documentation
6361659 2025-10-04 Paul1365972 | core: improve worldsenrate command ergonomics
9fd7b70 2025-10-04 Paul1365972 | docs: use correct bracket notation for commands
a515f81 2025-10-04 Paul1365972 | docs: add new redpiler flags
31eb350 2025-10-04 Paul1365972 | core: fix sound id off-by-one error from 1.20.4 update
c782842 2025-02-05 Paul1365972 | Improve docker build process
5b109d5 2025-10-04 Paul1365972 | dockerfile: correct paths and add rust-toolchain
0994f4b 2025-10-04 Paul1365972 | dockerfile: advertise port
57d7686 2024-12-25 Valerio Lomanto | feat(world): implement iterators over a region
57c1aab 2024-12-25 Valerio Lomanto | tests(world): add tests for `for_each_block_optimized`
605508e 2024-12-25 Valerio Lomanto | fix(world): `for_each_block(_mut)_optimized`
f8b7b03 2025-08-10 Valerio Lomanto | fix: mark stub methods in mock world as unimplemented
6327147 2025-11-29 KonaeAkira | Store forward links separate from nodes
44f0b02 2025-11-29 KonaeAkira | Remove smallvec from dependencies
bd66655 2025-11-30 KonaeAkira | Clear forward links on reset
1e59bae 2025-12-01 KonaeAkira | Align Node to 64 bytes to fit in most cache lines
a179ab1 2025-11-28 KonaeAkira | Optimize Queues::drain_iter
5c95c71 2025-11-29 KonaeAkira | Remove noteblock pitch LUT
a7373bc 2025-12-02 KonaeAkira | Make provides_weak_power and provides_strong_power free functions
47b4660 2025-12-02 KonaeAkira | Fix ground torch providing weak power to block below (fixes #218)
7e070aa 2025-12-02 KonaeAkira | Use linear search instead of hashmap lookup for deduping
03f443c 2025-12-02 KonaeAkira | Add block lookup cache
39ae5d6 2025-08-31 Bram Otte | Add -s flag to //paste
b9ff0b5 2025-09-01 Bram Otte | Add bounds check to //paste
75bf614 2025-12-29 Bram Otte | run cargo fmt
64ad5bf 2026-03-05 Bram Otte | revert: Add bounds check to //paste
1a6a142 2026-03-05 Bram Otte | Fix get_chunk_index_for_block to resolve issues with out of bound paste
df0f3d1 2026-03-05 Bram Otte | fix off-by-one error for second_pos in execute_paste
5026ba4 2026-03-04 StackDoubleFlow | Do the refactor
3dc1dff 2026-03-04 StackDoubleFlow | Implement and fix dynamic attributes
db1f62f 2026-03-04 StackDoubleFlow | Use proc macro to read ids instead of hardcoding them
4c437bb 2026-03-05 StackDoubleFlow | Remove some unsued imports
e15ea3a 2026-03-06 StackDoubleFlow | Fix block and item name namespacing changes
4760869 2026-03-06 StackDoubleFlow | Missed a minecraft: case
d7e389b 2026-03-06 StackDoubleFlow | Add a little bit of docs for the block data generator
f4b4c39 2026-03-06 StackDoubleFlow | Attempt to fix docker build
636e987 2026-03-06 StackDoubleFlow | Prefix unused params with _ in world::test
f2a91a9 2026-03-06 StackDoubleFlow | Add all wooden pressure plate types and polished blackstone pressure plate
49ec9b8 2026-03-10 StackDoubleFlow | Implement RIL parser
f4bec38 2026-03-11 StackDoubleFlow | Make item count NBT in containers accept both `Count` and `count`
8549127 2026-03-12 StackDoubleFlow | Add /version command
2c98fca 2026-03-12 StackDoubleFlow | Update deps
cdc6f73 2026-03-16 StackDoubleFlow | nix flake update
ab8503c 2026-03-18 StackDoubleFlow | Be even more lenient with container nbt data
ab176d8 2026-03-11 StackDoubleFlow | Start working on rilc redpiler driver
c950bf5 2026-03-17 StackDoubleFlow | Implement pass pipeline builder
b099ff2 2026-03-17 StackDoubleFlow | Implement running redpiler through rilc
5beeead 2026-03-17 StackDoubleFlow | Reorganize pass directory structure
5908bc3 2026-03-17 StackDoubleFlow | Skip searching nodes without blocks in InputSearch
dee2f25 2026-03-18 StackDoubleFlow | Implement test result comparison and automatic updating
5f52ba5 2026-03-18 StackDoubleFlow | Add colors for test results
5e5c5cc 2026-03-18 StackDoubleFlow | Count number of tests passed or updated
2b1d83e 2026-03-18 StackDoubleFlow | Add tests for constant fold
04f9488 2026-03-21 StackDoubleFlow | Allow analysis passes to depend on other passes
99f041c 2026-03-25 StackDoubleFlow | Add redpiler tests to CI
48f4d7e 2026-03-25 Bram Otte | PassPipelineBuilder: insert available analysis after pass is completed
8672cd8 2026-03-21 dependabot[bot] | build(deps): bump rustls-webpki from 0.103.9 to 0.103.10
2212f96 2025-10-11 Bram Otte | Fix axiom schematics crashing the server, by also checking for Offset property for sponge version 2
b5f17bb 2025-12-29 Bram Otte | Handle the case where a schematic does not provide an Offset or WEOffset, and remove inaccurate Axiom comment also makes the project rust 2024 to make use of new `if let` syntax
1b8bb48 2025-12-29 Bram Otte | Use old style_edition to prevent needless merge conflicts
3a04526 2025-12-29 Bram Otte | Prioritize Metadata.WEOffset over offset
f844965 2026-03-25 Bram Otte | rilc: fix updated test results from replacing the wrong part of the source text when multiple tests are updated in the same file
279bd2c 2026-03-25 Bram Otte | Make index map dependency consistent with the rest
245b44c 2025-09-29 Bram Otte | Add aarch64-pc-windows-msvc target
7dce713 2025-10-03 Bram Otte | Add aarch64 linux to build.yml This uses the ubuntu-24.04-arm os since using ubuntu-latest-arm doesn't seem to resolve a runner
38a78f9 2026-03-25 StackDoubleFlow | redpiler: avoid bounds checking in TickScheduler::end_tick
6f8a38b 2026-03-26 Bram Otte | TickScheduler: replace Queues.drain_iter with drain_each to replace a flat_map with a loop and speedup iteration
b8b430f 2026-03-26 StackDoubleFlow | redpiler: clear vec instead of using drain iter
8da4759 2026-03-26 StackDoubleFlow | redpiler: remove extra queue clear in tick
1b1e2ed 2026-03-26 StackDoubleFlow | redpiler: use get_unchecked when finding node forward links in set_node
d878ea3 2026-03-27 StackDoubleFlow | rilc: add compile subcommand
52248de 2026-03-27 StackDoubleFlow | rilc: add version subcommand
6beb8da 2026-03-28 StackDoubleFlow | docs: add argument types to RIL docs
8604d33 2026-03-30 StackDoubleFlow | docs: forgot to add a couple periods in last commit
f376fe9 2026-04-03 StackDoubleFlow | Add bamboo_sign, cherry_sign, and mangrove_sign
8236461 2026-04-04 StackDoubleFlow | redpiler: allow nodes to map to multiple blocks
13e5c8e 2026-04-05 StackDoubleFlow | redpiler: significantly speed up constant fold pass on large builds
7a4df4c 2026-04-06 StackDoubleFlow | redpiler: avoid hashmap in PruneOrphans pass
b28d8b0 2026-04-06 StackDoubleFlow | Synchronize all package fields to workspace
892d95d 2026-04-06 StackDoubleFlow | Fix clippy warnings
a7aa1e2 2026-04-06 StackDoubleFlow | Add newline in comment
8440fd5 2026-04-06 StackDoubleFlow | ci: create tar.gz or zip files for build artifacts
7e5d0d1 2026-04-06 StackDoubleFlow | rustfmt
be4469b 2026-04-06 StackDoubleFlow | ci: avoid matching directories for artifacts
1772c14 2026-04-14 dependabot[bot] | build(deps): bump rand from 0.10.0 to 0.10.1
99e2bf6 2026-04-17 StackDoubleFlow | redpiler: start work on new graph implementation
d0ffffb 2026-04-17 StackDoubleFlow | redpiler: move compile_graph module into it's own dir
d2eb535 2026-04-17 StackDoubleFlow | rustfmt
50f9197 2026-04-18 StackDoubleFlow | redpiler: use u32 for node indices
86bf221 2026-04-24 dependabot[bot] | build(deps): bump rustls-webpki from 0.103.10 to 0.103.13
37d2578 2026-05-03 StackDoubleFlow | Fix trapdoors not opening
8734f72 2026-04-26 Bram Otte | mchprs_world: remove networking from default features
dcf57b4 2026-07-01 Maxwellm34 | fix: correct typo 'folowing' to 'following' in README.md
9707ab8 2026-08-06 StackDoubleFlow | Use rayon to make world exports faster
b3faf91 2026-08-07 dependabot[bot] | build(deps): bump quinn-proto from 0.11.14 to 0.11.16
28ef846 2026-08-10 Bram Otte | Retain input nodes in prune_orphans pass
0dbfa12 2026-08-14 zPippo | Add support for hex color codes in the scoreboard (#251)
a74fb61 2025-12-04 KonaeAkira | Allocate ChunkSection::changed_blocks on demand
5ef69fa 2026-08-22 CarrotRob | Fix typos in README.md
42928f7 2026-08-22 CarrotRob | Fixed README.md typo
fe21721 2026-08-25 zPippo | Add the --illegal-states-out and --wire-cross-out flags (#250)
8d0463c 2026-09-09 Quantum Branching | Removed outdated AnalogRepeater Pass documentation
ef85327 2026-09-06 Paul1365972 | blocks: fix stale block ids in tests and run the whole workspace in CI
8957df9 2026-09-16 Paul1365972 | fix(rilc): report RIL test failures correctly
0098867 2026-09-06 Paul1365972 | fix(save_data): advance the backup suffix when migration backups exist
e339210 2026-09-12 Paul1365972 | feat(plot): support fractional tick and world send rates
a056d7f 2026-09-06 Paul1365972 | fix(plot): prevent timing monitor reset counter underflow
0a44537 2026-09-06 Paul1365972 | fix(redpiler): preserve pending ticks and initial state through optimization
54cfe75 2026-09-06 Paul1365972 | feat(redpiler): replace signal ranges with signal sets
d492e43 2026-09-06 Paul1365972 | docs(redpiler): explain signal-set pruning and its limitations
```

### D. Large integration attempt: p0.1.0 to merge_piston

Range: `p0.1.0..merge_piston`. First-parent history; upstream ancestry is summarized in the other appendices.

```text
bae6a55 2024-09-25 Kazik24 | Merge freaking >70 commits from 'master' into 'piston'
048b42e 2024-09-25 Kazik24 | cargo fmt
025eaad 2024-09-25 Lord225 | fixing block macro
3b5c860 2024-09-25 Lord225 | merge cd
c2efba6 2024-09-26 Lord225 |  BlockEntity::from_nbt changed signature
8e8a53a 2024-09-26 Kazik24 | merge cd 2
56ad793 2024-09-26 Lord225 | finish freaking merge uwu
```

### E. Selective integration: p0.1.0 to cherrypick_piston

Range: `p0.1.0..cherrypick_piston`. First-parent history; upstream ancestry is summarized in the other appendices.

```text
b6d19fd 2024-06-14 StackDoubleFlow | Create nix devshell
a9b40de 2024-06-09 Copper | Another two-letter addition
562c655 2024-04-25 Piet Schirm | Fixed //count incorrectly uwraping arguments The execute_count function unwraped the argument to a pattern instead of a mask fixes #115
447189d 2024-04-23 Bram Otte | play note when adjusting noteblock pitch
d855683 2024-04-23 Bram Otte | don't play note when tuning a noteblock if it is blocked
32d01a0 2022-09-15 Olek47 | Fix issue #72
9090434 2022-09-25 Olek47 | Handle the issue another way
257b332 2024-07-22 dependabot[bot] | Bump openssl from 0.10.60 to 0.10.66
29841d8 2024-07-30 StackDoubleFlow | Make all crates inherit the same workspace package fields
94e1f42 2024-07-30 StackDoubleFlow | redpiler: remove NodeType::is_io_block
2b4d877 2024-09-28 Kazik24 | Merge commit '970b3c8' from master
13d83c0 2024-04-09 StackDoubleFlow | Update blocks and items for 1.20.4
af2053d 2024-06-01 StackDoubleFlow | More 1.20.4 changes
50d0879 2024-06-09 Copper | Literally just add two letters (option->optional)
95be50c 2024-06-17 StackDoubleFlow | Switch back to my nbt fork on github
b20a342 2024-06-17 StackDoubleFlow | Increase default world height and more 1.20 fixes
2e59fb4 2024-06-18 StackDoubleFlow | The client is finally able to connect with 1.20.4
036bb6d 2024-09-28 Lord225 | Merge commit from MASTER '678dae1' into cherrypick_piston
5774a4e 2024-06-18 StackDoubleFlow | Add placeable stone bricks and fix plot generation
0adf665 2024-06-18 StackDoubleFlow | Fix executing commands
6b53e0d 2024-06-18 StackDoubleFlow | Fix chat color codes
2d16079 2024-06-18 StackDoubleFlow | Rename ChatComponentBuilder -> TextComponentBuilder
379248d 2024-06-22 StackDoubleFlow | Add trace logging for network traffic
a0ca17b 2024-06-22 StackDoubleFlow | Fix TextComponent::extra not being written
25260a8 2024-06-22 StackDoubleFlow | Fix scoreboard packets for 1.20
be154b3 2024-06-22 StackDoubleFlow | Bump plot data version to 2
8035581 2024-06-22 StackDoubleFlow | Fix crashes caused by comparators and empty containers
5b56ac3 2024-06-22 StackDoubleFlow | Fix Set Equipment packet
aed9475 2024-06-22 StackDoubleFlow | Update sign NBT
cd3e633 2024-06-23 StackDoubleFlow | Send AcknowledgeBlockChange packet after interaction
145b3cf 2024-06-23 StackDoubleFlow | Fix io-only flag also showing as update in scoreboard
296d834 2024-06-23 StackDoubleFlow | Flush redpiler on /radv and use block
b996e47 2024-06-26 StackDoubleFlow | Show players in player list
2e805c7 2024-06-26 StackDoubleFlow | Set default redpiler scoreboard state to stopped
68d98f8 2024-06-26 StackDoubleFlow | Better handle errors when failing to load or save plot
1c92546 2024-06-26 StackDoubleFlow | Add /plot select command
b23157e 2024-06-30 StackDoubleFlow | Remove old deprecated vscode rust-analyzer setting
4544bc0 2024-09-29 Kazik24 | Merge commit 'adb426a' from master
4b6127c 2024-09-29 Kazik24 | Fix F3+N gamemode change on 1.20.4
fdc07c0 2024-09-30 Lord225 | Updating piston block ids
561fc4f 2024-09-30 Lord225 | info->debug
b641fcc 2024-09-30 Lord225 | update block action protocol
26f1028 2024-09-30 Lord225 | observer draft
```

### F. Local piston continuation

Range: `cherrypick_piston..piston`. All commits in this range, including merge commits.

```text
3054b51 2024-10-13 Lord225 | fix pose metadata & fix skin parts
a0a7efb 2024-10-14 Lord225 | docker compose cleanup
```

### G. Remote piston continuation

Range: `piston..origin/piston`. All commits in this range, including merge commits.

```text
4abac26 2025-04-02 Lord225 | fix player CSpawnEntity and Attack
2de260e 2025-04-02 Lord225 | cargo fmt
6cc6959 2025-04-02 Lord225 | laod subdirs in schematics
48222f1 2025-04-02 Lord225 | fmt
dd01a49 2025-04-02 Lord225 | fix target
```

### H. Alternate selective continuation

Range: `piston..piston_cherrypick_cd`. All commits in this range, including merge commits.

```text
9f52a9a 2024-06-30 StackDoubleFlow | Remove sender arg from send_chat_message
e49a8de 2024-10-14 Lord225 | traverse dir
42905bf 2024-07-21 StackDoubleFlow | network: add login plugin request and response packets
9629658 2024-08-18 StackDoubleFlow | Fix pose metadata type and send metadata when player joins
c1d2544 2024-10-14 Lord225 | cherrypick clean up
```

### I. Remote instant-piston commits beyond the local tip

Range: `instant-piston..origin/instant-piston`. All commits in this range, including merge commits.

```text
3d33438 2024-04-10 Kazik24 | Working zero tick scheduling for interpreted backend
07a5376 2024-04-10 Lord225 | Refactor logging and tick handling in redstone piston module
```

### J. Remote update-piston commit beyond the local tip

Range: `update-piston..origin/update-piston`. All commits in this range, including merge commits.

```text
2726145 2024-04-29 Kazik24 | Add tests for (non)instant piston observer interactions
```

### K. Separate automatic-piston experiment

Range: `p0.1.0..piston-auto-instant`. All commits in this range, including merge commits.

```text
471a9a7 2024-04-21 Lord225 | /
```

### L. Experimental branch tip changes

These summaries identify the changed files at each experimental tip. Most earlier commits are already included in Appendix A.

```text
Ref: observer
83a463d 2024-04-19 Lord225 | observers

 crates/blocks/src/blocks/mod.rs |  2 +-
 crates/core/src/redstone/mod.rs | 34 ++++++++++++----------------------
 2 files changed, 13 insertions(+), 23 deletions(-)

Ref: instant-piston
21b39da 2024-04-09 Lord225 | Refactor piston update and tick handling in redstone piston module

 crates/core/src/redstone/piston.rs | 15 +++++++++++++--
 docker-compose-prod.yml            |  1 +
 2 files changed, 14 insertions(+), 2 deletions(-)

Ref: update-piston
fc2e2f2 2024-04-29 Lord225 | Refactor piston update logic to only update necessary blocks

 crates/core/src/redstone/piston.rs | 32 ++++++++++++++++++--------------
 1 file changed, 18 insertions(+), 14 deletions(-)

Ref: remove
f4c3833 2024-04-08 Lord225 | .

 crates/blocks/src/block_entities.rs | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)

Ref: piston-auto-instant
471a9a7 2024-04-21 Lord225 | /

 crates/core/src/redstone/mod.rs    | 35 -----------------------------
 crates/core/src/redstone/piston.rs | 45 ++++++++++++++++++++++++++++++++------
 2 files changed, 38 insertions(+), 42 deletions(-)

```

### M. Complete branch and remote-reference inventory

Dates below are committer dates, which can differ from the author dates in the commit lists.

```text
cherrypick_piston | 26f1028 | 2024-09-30 | observer draft
instant-piston | 21b39da | 2024-04-09 | Refactor piston update and tick handling in redstone piston module
master | 76b9b8c | 2024-09-20 | tests: seperate test backends into individual tests
merge_piston | 56ad793 | 2024-09-26 | finish freaking merge uwu
observer | 83a463d | 2024-04-19 | observers
piston | a0a7efb | 2024-10-14 | docker compose cleanup
piston-auto-instant | 471a9a7 | 2024-04-21 | /
piston_cherrypick_cd | c1d2544 | 2024-10-14 | cherrypick clean up
recovery/piston-1.18.2 | cb3d4e2 | 2024-05-31 | Add test_memory_cell_unaligned_nanoticks
remove | f4c3833 | 2024-04-08 | .
update-piston | fc2e2f2 | 2024-04-29 | Refactor piston update logic to only update necessary blocks
origin | 76b9b8c | 2024-09-20 | tests: seperate test backends into individual tests
origin/cherrypick_piston | 26f1028 | 2024-09-30 | observer draft
origin/instant-piston | 07a5376 | 2024-04-10 | Refactor logging and tick handling in redstone piston module
origin/master | 76b9b8c | 2024-09-20 | tests: seperate test backends into individual tests
origin/merge_piston | 56ad793 | 2024-09-26 | finish freaking merge uwu
origin/observer | 83a463d | 2024-04-19 | observers
origin/piston | dd01a49 | 2025-04-02 | fix target
origin/piston-auto-instant | 471a9a7 | 2024-04-21 | /
origin/piston-instant | f4c3833 | 2024-04-08 | .
origin/piston_cherrypick_cd | c1d2544 | 2024-10-14 | cherrypick clean up
origin/update-piston | 2726145 | 2024-04-29 | Add tests for (non)instant piston observer interactions
upstream | d492e43 | 2026-09-28 | docs(redpiler): explain signal-set pruning and its limitations
upstream/bad_opt_experiment | 4d9cee5 | 2021-11-26 | Add optimized backend
upstream/colored-lamps | 6bed8d8 | 2021-02-11 | Add ColoredLamp and Randomizer
upstream/cranelift-dev | 4bcb935 | 2021-10-05 | [cranelift] Implement inlining for redstone wire updates
upstream/cursed_redstone | 464128a | 2020-06-25 | Cursed redstone
upstream/dev/stack/mc-1.21.8 | 4660d75 | 2026-09-10 | WIP 1.21.8 signs
upstream/experiment-tick-concurrency | f041de9 | 2020-06-19 | Use threadpool for ticks
upstream/master | d492e43 | 2026-09-28 | docs(redpiler): explain signal-set pruning and its limitations
upstream/mc-1.18.2 | 970b3c8 | 2024-07-30 | redpiler: remove NodeType::is_io_block
upstream/permissions-refactor | 67e5428 | 2025-03-05 | core/permissions: fix group inheritence with server context
upstream/plugins | 86b3fa2 | 2020-07-24 | broadcast_chat and cancelable events
upstream/redpiler-dev | fe21721 | 2026-08-25 | Add the --illegal-states-out and --wire-cross-out flags (#250)
upstream/redpiler_save_cg | 8e8b22c | 2024-06-23 | Save compile graph to bitcode
upstream/v0.5.1 | e374662 | 2024-09-03 | bump version to v0.5.1
upstream/v0.5.2 | 3b166ec | 2026-08-14 | v0.5.2
```
