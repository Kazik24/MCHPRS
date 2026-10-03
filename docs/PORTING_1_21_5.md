# MCHPRS piston port to Minecraft 1.21.5

Target corrected from 1.21.1 to **1.21.5** at the user's request. Implemented on `port/piston-1.21.5`, based on recovered `p0.1.0` / `cb3d4e2` and the recovery lockfile fixes. Date: 2026-10-03.

The branch now builds and serves independently decoded Java **1.21.5** clients: **protocol 770**, **Minecraft DataVersion 4325**. The original [1.21.1 plan](PORTING_1_21_1.md) is historical; [the repository assessment](REPOSITORY_ASSESSMENT.md) retains the branch analysis. Original branch heads remain intact. Current implementation changes are in the working tree.

## Completed work

| Area | Implementation |
| --- | --- |
| Baseline | Full suite recorded before changes; known piston memory-cell discrepancy kept visible |
| Versioned data | Pinned inputs, exact official 1.21.5 registries, generator and reproducible extraction instructions |
| Blocks/items | 20,342 legacy block-state mappings into 27,914 target states; full target item registry; semantic legacy migration |
| Codec primitives | Bounded framing, compression, integers, strings, anonymous NBT and component parsing |
| Login/configuration | Login acknowledgement, feature flags, known-pack negotiation, registry data, 556 resolved vanilla tags, finish acknowledgement, then play |
| Play packets | Target IDs/layouts for join, chunks, movement, entity/player info, inventory, equipment, commands, text, scores and interactions |
| 1.21.5 differences | Implicit paletted-array lengths, untrusted creative component payloads, sea level, registry changes and updated entity layouts |
| Items/text/signs | Retained structured component patches and removals, nested containers, block-state tags, anonymous text components, both sign sides |
| Pistons | Correct block registry IDs for actions, official moving-piston entity IDs, state/property NBT, interpreter fallback for piston/observer plots |
| Persistence | Frozen legacy plot readers, remapped IDs, explicit version headers, validated data, preserved backups and atomic replacement |
| Schematics | v2/v3 imports, v2 export, offsets, rich signs, malformed-input checks, real fixture and command integration tests |
| Validation | Debug/release builds, format/check, regression suite and independent two-client integration including restart |

The implementation deliberately preserves the plot geometry (min Y 0, height 256, 256 × 256 plots) and the existing piston scheduler. Master and other fork branches informed individual changes; this does not merge their entire simulation/compiler histories.

## Structure and maintenance

- `mc_data/1.21.5`: checked-in protocol/block/item inputs, exact registry snapshots and provenance.
- `tools/generate_mc_data.py`: generated Rust mappings and configuration registry bytes.
- `tools/import_official_registries.py`: verifies Mojang's artifact, extracts client registry data and imports builtin registry reports.
- `crates/blocks`: target IDs around the legacy typed block model, canonical registry names, item catalog and block-entity conversion.
- `crates/network`: target packet formats, connection state transitions, text NBT, trusted/untrusted item-component codecs and codec tests.
- `crates/core`: login orchestration, inventory and interaction adaptation, piston integration, chunk encoding, WorldEdit and regression tests.
- `crates/save_data`: frozen legacy layouts, migration, validation, backups and atomic writes.
- `tools/protocol_smoke.js` / `run_protocol_smoke.py`: independently decoded clients in temporary worlds.

See [tools instructions](../tools/README.md), [data provenance](../mc_data/README.md), and [Sponge implementation details](SPONGE_V3_IMPLEMENTATION.md).

Prismarine's target `loginPacket` dataset actually routes to 1.21.3. It was replaced with registry content from the exact Mojang 1.21.5 server artifact. MCProtocolLib's matching revision was used to resolve the intangible-projectile and chicken-variant component discrepancies. This port does not depend on Glowstone's older protocol target.

Generated item mappings use explicit legacy IDs; legacy data starts at ID 1, while the target includes air at ID 0. Item round-trip coverage found and fixed that indexing issue, the legacy terracotta range and jungle/acacia sign-item inversion. Duplicate block aliases were removed so decoding retains the intended note-block instruments.

## Persistence compatibility

| Source | Behavior |
| --- | --- |
| Plot format 0, recovered legacy layout | Frozen reader, semantic ID conversion, send-rate default |
| Plot format 1, 1.18.2 piston layout | Frozen reader preserves tick data, moving pistons, inventories and legacy signs |
| Plot format 2, broken/other port histories | Explicit refusal; no safe reader is defined |
| Headerless plot files | Explicit refusal |
| Current plot format 3 | Magic + save version + Minecraft DataVersion 4325 + bincode payload |
| Legacy unversioned player files | Validate, remap items once, back up, write current header |
| Current player files | `MCHPLY\0` + save version 3 + DataVersion 4325 + bincode payload |
| Different Minecraft DataVersion | Refuse before deserialization |

Backups use `.bak`, then `.bak.1`, etc., preserving existing backups. Serialization/validation completes before migration replaces the original. Failed player loads refuse login instead of resetting the player's data. Tests exercise replacement of an existing file on Windows, direct packed-state conversion, signs/pistons, failed migrations and version mismatches.

This is a registry-aware migration from the recovered baseline, not a general Minecraft data fixer. Do not use another branch's format-2 world as an input and expect automatic conversion. Test copied worlds before operational rollout.

Plot and template load failures now produce a handled error, disconnect affected players and remove the unavailable plot from the active list. Healthy plots remain accessible and shutdown does not wait for a failed thread. The production format-2 spawn was archived and reset with authorization; see [recovery details](protocol-errors/2026-10-03-plot-load.md). Format-2 migration remains unsupported.

## Validation and remaining work

Commands used:

```text
cargo check --workspace --locked
cargo fmt --all -- --check
cargo build --locked
cargo build --release --locked
cargo test --workspace --locked --no-fail-fast
python tools/run_protocol_smoke.py
python tools/run_plot_load_smoke.py
```

Final workspace result: **37 passed, 1 failed**, with no ignored tests. Formatting, locked debug/release builds, generator reproducibility and the independent integration test passed.

The complete suite preserves the single known failure: `redstone::tests::test_memory_cell_unaligned_nanoticks`, tick 0. All four piston update fixtures, Chungus interpreter/compiler cases, registry round trips, codec, migration and schematic tests pass. Expected Chungus hashes are unchanged; hashing normalizes target states to their corresponding legacy IDs so registry renumbering does not alter the semantic circuit comparison.

The independent integration clients verified configuration, two-player visibility, full-height chunk palette reads, creative components/equipment, placement, prediction acknowledgements, piston extension/action events, commands, v3 load/paste/undo/redo, v2 save/reload, failed-load clipboard preservation, reconnect and saved state after a process restart. Both servers shut down through `/stop`; the real workspace world was not opened.

A graphical 1.21.5 client reported missing enchantment exclusive-set tags during registry loading. The original empty configuration tags packet was replaced with 556 vanilla tags resolved against the exact transmitted registry IDs. The smoke test now requires all seven reported exclusion sets. See [the disconnect analysis](protocol-errors/2026-10-03-enchantment-tags.md). Graphical reconnect confirmation is pending.

Remaining acceptance work is **graphical vanilla-client verification** of registry codecs, rendering, lighting, sign appearance and piston animations, plus operational proxy/forwarding validation. Independent protocol clients do not validate all vanilla registry semantics. CI runs the full suite without hiding the known simulation failure, and the separate protocol job can pass independently.

Piston/observer plots stay on the interpreter because the recovered compiler does not provide their required behavior. Detection scans palettes/direct states and can add overhead on large plots. New registry blocks can be represented and transferred, but this creative redstone server does not implement every new vanilla block's gameplay.

General nonempty modern persisted item-component maps in external schematic inventories are explicitly unsupported; see the Sponge report. Wire components and the server's own component persistence have separate coverage.

Next steps:

1. Perform graphical-client acceptance on a copied test world and verify deployment/proxy configuration.
2. Build a separate reference comparison suite for piston/observer scheduling, starting with the known memory-cell discrepancy and the recorded piston audit/fix plans.
3. Achieve full reference piston compliance without weakening existing expectations; treat the failure as an implementation defect.
4. Extend compiled piston execution only once it matches interpreter/reference behavior, and measure the interpreter-detection cost.
5. Add explicit readers for other save histories and external persisted inventory components when those formats are required.
