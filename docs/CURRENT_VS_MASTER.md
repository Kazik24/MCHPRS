# Current piston server compared with newest upstream master

Comparison date: **2026-10-03**. Upstream `master` was fetched from `https://github.com/MCHPR/MCHPRS.git` for this comparison. The remote tip was **`d492e433f2c7de5fd5c71f165cb60f0d912ebf04`**, committed 2026-09-28. This is a pinned snapshot; future master commits are outside this document.

The current server is the recovered **Minecraft 1.18.2 piston fork** described in [the repository assessment](REPOSITORY_ASSESSMENT.md). Newest upstream master is a **Minecraft 1.20.4 server**, with extensive compiler, data, tooling and reliability changes. It does **not** contain this fork's piston engine or its observer update implementation. Replacing the current server with master would therefore change supported circuit behavior as well as client compatibility.

## Comparison points and history

| Item | Current server source | Newest upstream master |
| --- | --- | --- |
| Branch | `recovery/piston-1.18.2` | `upstream/master` |
| Commit | `cb3d4e27a67737abad990932ce94b5461368d45f` (`p0.1.0`) | `d492e433f2c7de5fd5c71f165cb60f0d912ebf04` |
| Cargo package version | `0.4.1` | `0.5.2-dev` |
| Minecraft client version | `1.18.2` | `1.20.4` |
| Protocol version | `758` | `765` |
| Minecraft data version | `2975` | `3700` |
| MCHPRS plot save-format version | `1` | `3` |
| Plot width, source default | 256 blocks / 16 chunks (`PLOT_SCALE = 4`) | 512 blocks / 32 chunks (`PLOT_SCALE = 5`) |
| Plot height, source default | 256 blocks / 16 sections; minimum Y = 0 | 384 blocks / 24 sections; minimum Y = 0 |
| Automatic Redpiler, config default | Enabled | Disabled |
| Rust edition | 2018 | 2024 |

Sources: current `Cargo.toml`, `crates/core/src/server.rs`, `crates/core/src/plot/mod.rs`, `crates/save_data/src/plot_data.rs` and `crates/core/src/config.rs`; upstream [manifest](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/Cargo.toml), [version constants](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/world/src/lib.rs), [plot geometry](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/plot/mod.rs), [save format](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/save_data/src/plot_data.rs) and [configuration](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/config.rs).

The Cargo version, fork tag, Minecraft version, protocol number and save-format number identify different things. In particular, the fork's `p0.1.0` tag still declares Cargo version `0.4.1`, and upstream's newer package version does not imply Minecraft 1.21 support.

The branches diverged from **`1f3334097f3f882eae2d3d32acf4bfb29280f182`**. The current fork has **116 commits** absent from upstream master; upstream has **229 commits** absent from the current fork, including merges. These are diverging histories, not a 229-commit fast-forward upgrade.

There are two different masters in this checkout:

- **`master` / `origin/master`**, `76b9b8cf575ee4419eca5c2cd9dac56405a2cafe`: Kazik24's September 2024 upstream snapshot, with 70 commits after the common ancestor.
- **`upstream/master`**, `d492e43`: newest MCHPR master, with 159 additional commits after that fork-master snapshot.

Both masters target 1.20.4. The tables below distinguish changes already in the 2024 snapshot from later work. Complete historical commit lists are in [assessment appendix B](REPOSITORY_ASSESSMENT.md#b-fork-master-2024-upstream-changes-after-the-shared-ancestor) and [appendix C](REPOSITORY_ASSESSMENT.md#c-current-upstream-changes-beyond-fork-master).

### Local recovery and deployment changes

The comparison uses committed server source at `cb3d4e2`. At the start of this comparison, the working directory also contained the recovered `Cargo.lock` update (`time 0.3.20` to `0.3.36`, related dependency updates and lockfile format 4), documentation, Docker/Compose changes and an additional schematic fixture. These are local changes, not upstream additions or simulation fixes. Further uncommitted source/data-generation work appeared during the comparison and is outside this pinned baseline; this document does not audit that work or identify the exact contents of a deployed executable.

The local [production Docker build](../docker/README.md) changes plot scale to 5 inside its build, so that variant is already 512 blocks wide. The geometry table describes source defaults, not proof of a deployed binary's settings. No running production server or deployment was inspected for this comparison.

## Client protocol, login and player synchronization

Upstream replaces the 1.18.2 wire/data definitions with 1.20.4 definitions. This involves new packet IDs and fields, a login/configuration/play flow, registry data, changed text/NBT serialization and updates to chunk, entity and inventory-related packets. Changing the advertised version alone cannot reproduce these changes.

| Change in master | Practical effect | Representative commits |
| --- | --- | --- |
| 1.20.4 block/item IDs, login/configuration, registries, world height and packet encodings | A 1.20.4 client can join and receive the updated world representation | `166b922`, `2e92c7c`, `709923f`, `678dae1` |
| Command execution and text-component serialization, including colors and nested `extra` text | Fixes command and chat rendering failures during the version upgrade; text moves to a separate crate | `c30c8bb`, `9b62b41`, `9efcdb2`, `404aeca` |
| Scoreboard and equipment packet fixes; updated sign NBT | Corrects display/equipment encoding and gives signs front/back text data | `f69a4fe`, `4d649aa`, `47bb8ad` |
| Block-change acknowledgement after interaction | Synchronizes client placement/use predictions with server results | `7eff4fd` |
| Player list, forwarded profile properties, skin parts and pose metadata | Corrects tab-list and player appearance synchronization | `48d688b`, `83e3803`, `cebe4ca`, `c83a79d` |
| Player entity ID and `player_attack` damage-type fixes, added after fork master | Corrects player entities and a client crash on attack | `283cd7f`, `8285803` |
| Sound-ID off-by-one fix, added after fork master | Corrects sounds affected by the 1.20.4 registry change | `31eb350` |

Implementation references: upstream [server/login](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/server.rs), [clientbound packets](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/network/src/packets/clientbound.rs), [serverbound packets](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/network/src/packets/serverbound.rs) and [text components](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/text/src/lib.rs).

For the planned 1.21.1 port, these changes identify affected subsystems. They are not target-compatible codecs: master remains protocol 765, while the [port plan](PORTING_1_21_1.md) targets protocol 767 and structured item components.

## Configuration, proxy support and commands

| Surface | Current fork | Master behavior |
| --- | --- | --- |
| Proxy forwarding | `bungeecord` configuration and legacy forwarding path | Legacy forwarding removed (`9116f7a`); login plugin packets and Velocity modern forwarding added (`df8dbe7`, `9d12cef`). Uses a `[velocity]` table with `enabled` and `secret` |
| Automatic compilation | `auto_redpiler = true` by default | Default changed to `false` (`565564f`); an existing explicit config value is still used |
| `/rtps` | Nonnegative integer rate or `unlimited`; numeric ceiling of 100000 | Ceiling removed (`96c8bc6`); finite nonnegative fractional rates supported (`e339210`) |
| `/worldsendrate`, `/wsr` | Integer send rate | No-argument query, fractional rates from 0 to 1000 Hz; 0 disables periodic sends (`6361659`, `e339210`) |
| `/plot select`, `/p sel` | Not present | Selects the entire plot for WorldEdit (`bce8883`) |
| `/version`, executable `--version` / `-v` | Not present | Reports build/package version information (`8549127`) |
| `/radvance nano/pico` | Fine-grained interpreted advancement | Fork-specific modes absent; ordinary `/radvance <ticks>` remains |
| `/curse`, `/bless` | Experimental fork simulation controls | Command handlers absent |
| Scoreboard | Existing Redpiler state display | Fixes IO-only flag display and initial stopped state; later adds hex color support (`e6624ec`, `d04b87b`, `0dbfa12`) |

LuckPerms database integration and the general creative/redstone command set remain. Velocity support does not add standalone Mojang authentication: upstream's README documents proxy-based authentication. Changing forwarding mode requires corresponding proxy configuration.

References: upstream [command handlers](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/plot/commands.rs) and [configuration/usage documentation](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/README.md).

## Block definitions and interpreted redstone

The current fork defines supported block/item IDs and properties through handwritten tables/macros. Beyond the 2024 1.20.4 table update, master introduces a reproducible generation workflow (`5026ba4`, `3dc1dff`, `db1f62f`):

- `mc_data/blocks.json`, `mc_data/registries.json` and `mc_data/gen_info.yaml` supply data and supported-block configuration.
- `crates/block_data_gen` generates `crates/blocks/src/generated.rs` with block/item enums, mappings and properties.
- Procedural macros read registry IDs and block attributes instead of repeating hard-coded IDs. Follow-up fixes handle namespaced names.

New supported definitions/placement include stone bricks (`81b54a2`), glowstone (`af56a3e`, `e1705bf`), end portal frames with comparator output (`a423ac4`), additional wooden and polished-blackstone pressure plates (`f2a91a9`), and bamboo/cherry/mangrove signs (`f376fe9`). This expands the supported subset; it does not implement every vanilla block's gameplay.

Simulation fixes include empty-container comparator crashes (`68213ab`), default trapdoor half (`15a93a6`), interpreted repeater pulse behavior (`557b28b`), a ground torch incorrectly powering the block below (`47b4660`) and trapdoors failing to open (`37d2578`). Note-block tuning now plays a note unless blocked (`066dffb`, `bef7065`). The repeater, torch and trapdoor fixes after fork master are behavior changes to review against fork fixtures when adapting them.

References: upstream [block-generation instructions](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/docs/Adding_a_block.md), [generation configuration](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/mc_data/gen_info.yaml) and [interpreted redstone](https://github.com/MCHPR/MCHPRS/tree/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/redstone/src).

## Redpiler: correctness, optimization and performance

Master retains the Direct backend but changes its graph representation, compiler pipeline and runtime substantially. These are independent of the Minecraft protocol upgrade.

| Area | Changes already in fork master | Later changes through newest master |
| --- | --- | --- |
| World/compiler synchronization | Flush on `/radv` and block use; expose pending-tick checks (`68d8c59`, `a02d76f`) | Batched `tickn` backend API (`c98bc6d`) and additional pending-tick/state preservation |
| Graph optimization | Repeat coalescing to catch repeated patterns (`21b62fc`) | Remove inaccurate `AnalogRepeaters` (`a1f9213`); fix constant coalescing (`17b66f8`, `ff58993`); preserve inputs during orphan pruning (`28ef846`) |
| Signal analysis | Earlier unreachable-output pruning | Introduce signal-range analysis in 2025, then replace it with **signal-set analysis** (`54cfe75`) in September 2026 |
| Initial state and scheduled work | Earlier optimizer behavior | Nodes with pending ticks are marked before optimization; constant folding, coalescing and pruning preserve scheduled behavior and initial state (`0a44537`) |
| Graph and pass APIs | Crate extraction | Custom stable graph, `u32` node indices, nodes mapping to multiple blocks, pass registry/pipeline builder and analysis dependencies (`99e2bf6`, `50f9197`, `8236461`, `c950bf5`, `04f9488`) |
| Backend storage and scheduling | Existing Direct backend | Separate forward links, clear links on reset, 64-byte node alignment, faster queue iteration and boolean input lookup (`6327147`, `bd66655`, `1e59bae`, `a179ab1`, `9718694`, `6f8a38b`) |
| Compile-time work | Existing searches and passes | Stop wire searches beyond effective signal distance, cache block lookups and accelerate constant folding/orphan pruning (`c8655b0`, `03f443c`, `13e5c8e`, `7a4df4c`) |
| Display outputs | Add `--wire-dot-out` / `-d` (`ae72401`) | Add `--illegal-states-out` / `-l` and `--wire-cross-out` / `-c` (`fe21721`) |
| Diagnostics | Binary graph export | RIL dumps, `--print-after-all`, `--print-before-backend`, DOT export and standalone compiler/test tooling |

The newest signal analysis represents possible strengths as sets of values from 0 to 15, initialized with current outputs. Pending repeaters also admit 0 and 15 because a queued tick can produce a pulse after its input disappears. Sets grow until stable, including through cycles. Unreachable-output pruning removes links whose weight is at least the source's maximum possible strength. The analysis **does not track correlations between inputs**; it can include combinations that never occur together.

These changes fix specific correctness problems and target compilation/runtime costs. No performance benchmark or claim of full simulation correctness was established in this documentation task. Upstream still documents caveats for `--optimize`.

References: upstream [Redpiler documentation](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/docs/Redpiler.md), [compiler/options](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/redpiler/src/lib.rs) and [signal-set analysis](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/redpiler/src/passes/analysis/ss_set_analysis.rs).

## Features in the current fork that master does not replace

These are differences between diverging branches, rather than features removed from the fork by a completed upgrade.

| Current fork feature | State in upstream master |
| --- | --- |
| Normal/sticky piston movement, heads, moving-piston block entities, recovery and animation hooks | No equivalent piston engine or moving-piston block-entity variant. The fork's `crates/core/src/redstone/piston.rs` has no replacement in upstream's redstone crate |
| Observer transitions, scheduling and neighbor propagation | Observer block/item definitions remain, and wire connectivity recognizes observers, but there is no equivalent observer update/tick implementation |
| Shared interpreted/compiled scheduler with zero-delay work and nano/pico advancement | Upstream scheduler/backend APIs differ; the fork's advancement modes and piston-specific scheduling are absent |
| Debug stick and redstone-power display | Fork interaction behavior must be retained explicitly; it is not supplied by upgrading to master's generated data |
| Cursed/instant repeater behavior and `/curse` / `/bless` | Fork command/behavior path absent; the upstream World trait's default `is_cursed()` hook does not implement it |
| Piston-aware WorldEdit rotate/flip | No equivalent typed piston model to support the fork's transformations |
| Five piston/observer schematic fixtures and fine-grained trace tests | Absent from upstream; its component/RIL tests exercise a different supported surface |

The current fork itself has limitations: the previously recorded memory-cell test fails at tick 0, and its expected trace still needs verification against reference Minecraft. Neither branch supplies proof of complete piston compliance. Pistons/observers are not compiled graph nodes in the current fork; a graph enum placeholder is not backend support.

Sources: current piston, scheduler, interaction and test modules; upstream [redstone engine](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/redstone/src/lib.rs), [generated blocks](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/blocks/src/generated.rs), [block entities](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/blocks/src/block_entities.rs) and [node identification](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/redpiler/src/passes/frontend/identify_nodes.rs).

## WorldEdit, schematics and world export

| Change | Effect | Representative commits |
| --- | --- | --- |
| `//count` argument handling | Fixes treating the mask argument as a pattern | `b5d751c` |
| Sponge v3 loading and lowercase block-entity `id` | Accepts newer schematics and correctly handles their entities | `0364b17`, `680a8f0` |
| Generated command completion and alias/help fixes | Makes completion follow implemented WorldEdit commands | `8f4f074`, `7a9f460`, `12fc9c8` |
| Combined-flag parsing | Fixes a plot crash when parsing combined flags | `46160d4` |
| `//rstack` undo ordering | Applies undo clipboards in the correct order | `1b8d5d6` |
| `//paste -s` | Selects the pasted region | `39ae5d6` |
| Paste coordinates and selection bounds | Fixes out-of-bounds chunk indexing and the selection's second-position off-by-one error | `1a6a142`, `df0f3d1` |
| Schematic offsets | Accepts `Offset` or `Metadata.WEOffset`, handles missing offsets and prefers `WEOffset` when present | `2212f96`, `b5f17bb`, `3a04526` |
| Container NBT tolerance | Accepts `Count` and `count` and more permissive container data | `f4bec38`, `ab8503c` |
| Standalone `world_exporter` | Writes a Minecraft 1.20.4 save from supported MCHPRS world data; later parallelizes export with Rayon | `ebc330c`, `9707ab8` |

The history includes a proposed paste bounds check (`b9ff0b5`) and its revert (`64ad5bf`). The final source has the chunk-index fix; it should not be described as retaining the reverted change.

Schematic loading/pasting is now reusable through `mchprs_schematic`. The world exporter uses master's plot loader, so it does not automatically bypass the legacy-save incompatibility described below.

References: upstream [schematic crate](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/schematic/src/lib.rs), [WorldEdit execution](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/plot/worldedit/execute.rs) and [world exporter](https://github.com/MCHPR/MCHPRS/tree/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/world_exporter).

## Persistence, plot geometry and timing

Master's 2024 update raises the save format from 1 to 2 (`dd37ceb`), improves plot load/save error handling (`93d8149`) and fixes plot saving during graceful shutdown (`feae35e`). Later master raises the format to **3** because limited tick rates and world-send rates change from `u32` to `f32` (`e339210`). Chunk section storage also changes from fixed arrays to vectors.

**Master cannot directly migrate the current version-1 plots.** Its fixer returns `ConversionUnavailable` for versions 0 and 1. Its implemented migration is **version 2 to version 3**, converting the rate fields and preserving the existing chunk data and pending ticks. It does not translate 1.18.2 block-state IDs, convert fork-specific moving pistons or resize plots.

The backup loop now increments its suffix (`0098867`), resolving the hang when `.bak` and `.bak.1` both exist. The implementation renames the source to a backup and then writes the converted file; this is not an atomic replacement scheme.

Plot width doubles and height increases in master. A current scale-4 plot contains 256 chunks; master's default scale-5 plot requires 1024. The plot loader checks the chunk count against the configured source geometry. Save-format conversion alone is not a geometry conversion, and even the local scale-5 Docker variant retains the fork's other data/height differences.

Timing changes support fractional rates, adjust batching and world-send scheduling, and fix a timing-monitor reset counter underflow (`a056d7f`). World storage adds optimized region iteration and its fixes/tests (`57d7686`, `605508e`, `57c1aab`). Changed-block tracking arrays are allocated on demand (`a74fb61`), reducing storage allocated for untouched sections.

Sources: upstream [migration dispatcher and backups](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/save_data/src/plot_data/fixer.rs), [v2-to-v3 conversion](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/save_data/src/plot_data/v2_to_v3.rs), [plot loader/loop](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/core/src/plot/mod.rs) and [world storage](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/crates/world/src/storage.rs).

For the future 1.21.1 port, reserve a distinct save version and explicit legacy readers. The earlier proposal to use format 3 now overlaps an existing upstream format and must be revised before implementing migration.

## Crate structure, build tools and tests

| Area | Current layout | Master layout/change |
| --- | --- | --- |
| World API/storage | Modules in `mchprs_core`; `mchprs_world` holds tick types | World/storage extracted into `mchprs_world` (`21c694f`); networking is optional and no longer a default feature (`8734f72`) |
| Simulation/compiler | Redstone and Redpiler modules in `core` | Separate `mchprs_redstone` and `mchprs_redpiler` crates (`75f3718`) |
| Text/schematic helpers | Core modules | Reusable `mchprs_text` and `mchprs_schematic` crates |
| Block data | Handwritten definitions/macros | Data files, generator crate and generated Rust definitions |
| Compiler tooling | Graph export and server commands | RIL parser/printer and `rilc` with `compile`, `test` and `version` subcommands; selectable pass pipelines |
| Cargo/toolchain | Edition 2018 and per-crate dependencies | Edition 2024, shared workspace package/dependency fields, dependency updates, `rust-toolchain.toml` selecting `stable`, Nix development shell |
| Tests | Core unit tests and fork schematic traces | Separate interpreted/compiled component and timing integration tests plus RIL optimizer tests |
| CI | Older build/test workflows | Formatting, Clippy, workspace tests with all features/targets, and RIL tests. Stale test IDs fixed and test scope widened (`ef85327`); RIL failures correctly reported (`8957df9`) |
| Build artifacts | Older platform/action setup | Updated actions/cache, ARM64 Windows/Linux targets and compressed release artifacts |
| Docker | `docker/` Dockerfiles and production Compose variants | Root multi-stage Alpine builder / scratch runtime image, copied generated-data inputs, `/data` volume and exposed port 25565 |

RIL (Redstone Intermediate Language) makes compiler graphs readable and testable without running a server. `rilc` can compile RIL/schematic inputs and compare expected pass/backend output. Its tests complement gameplay/timing tests; they do not replace piston reference traces.

The extraction changes imports, ownership and API signatures. Existing fork integration attempts that mix core modules with tests expecting the extracted crates do not constitute a complete integration; see [the assessment](REPOSITORY_ASSESSMENT.md#large-merge-attempt-merge_piston).

Master's root Dockerfile does not carry this checkout's production Compose configuration, pinned Rust 1.98.1, Debian runtime or BuildKit cache setup. Those local deployment choices require separate adaptation if the source layout changes.

References: upstream [workspace manifest](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/Cargo.toml), [RIL documentation](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/docs/RIL.md), [CI workflow](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/.github/workflows/ci.yml) and [Dockerfile](https://github.com/MCHPR/MCHPRS/blob/d492e433f2c7de5fd5c71f165cb60f0d912ebf04/Dockerfile).

## Implications for this fork's planned port

1. Use master as a source of individually reviewed fixes and subsystem references. A direct replacement loses fork-specific behavior and does not achieve the 1.21.1 target.
2. Adapt network/data changes using exact 1.21.1 definitions. Generated 1.20.4 data cannot supply target IDs or structured component codecs.
3. Preserve piston movement, moving-piston entities, observers, zero-delay scheduling, fine-grained advancement and piston-aware WorldEdit explicitly.
4. Define a separate migration for version-1 fork plots/player data, including ID mapping, block entities and pending work. Revise the proposed format number; upstream already uses 3.
5. Review repeater/torch/trapdoor fixes and compiler pending-state fixes against baseline traces. Crate extraction and newer compiler architecture remain separate choices, consistent with the existing port plan.

## Verification and reproducibility

This comparison was checked against fetched Git history and pinned source files. It did not build or run upstream, run new simulation tests, test a real client or migrate existing worlds. Previously recorded recovery/build/test results remain in [the assessment](REPOSITORY_ASSESSMENT.md#3-validation-and-its-limits); they are not new validation of master.

To reproduce the history comparison from this checkout:

```powershell
git fetch upstream master
git ls-remote upstream refs/heads/master
git ls-remote origin refs/heads/master
git merge-base cb3d4e27a67737abad990932ce94b5461368d45f d492e433f2c7de5fd5c71f165cb60f0d912ebf04
git rev-list --left-right --count cb3d4e27a67737abad990932ce94b5461368d45f...d492e433f2c7de5fd5c71f165cb60f0d912ebf04
git log --reverse --format="%h %cs %s" cb3d4e27a67737abad990932ce94b5461368d45f..d492e433f2c7de5fd5c71f165cb60f0d912ebf04
git diff --stat cb3d4e27a67737abad990932ce94b5461368d45f d492e433f2c7de5fd5c71f165cb60f0d912ebf04
```

The left/right count is `116 229`. A two-endpoint `git diff` compares the final trees, including fork-only features; `git log current..master` lists upstream-only history. A three-dot diff instead starts at the common ancestor and would omit the fork-specific side of the comparison. `%cs` prints committer dates; author dates in the older assessment appendices can differ after rebases.
