# Sponge schematic v2/v3 implementation

Implemented on `port/piston-1.21.5`, 2026-10-03, following [SPONGE_V3_PLAN.md](SPONGE_V3_PLAN.md).

Both **Sponge v2 and v3 imports** now use the same validated clipboard construction path. `//save` continues writing **v2**. Schematic format versions are separate from Minecraft network protocol 770 and Minecraft DataVersion 4325.

The implementation follows the [v2](https://github.com/SpongePowered/Schematic-Specification/blob/8e6be2d980d3bd794bc29df5fdca5921129fac5d/versions/schematic-2.md) and [v3](https://github.com/SpongePowered/Schematic-Specification/blob/8e6be2d980d3bd794bc29df5fdca5921129fac5d/versions/schematic-3.md) specifications at revision `8e6be2d980d3bd794bc29df5fdca5921129fac5d`.

## Behavior

- v2 accepts the existing flat root and a wrapped `Schematic` compound. v3 requires the nested compound and `Blocks` container.
- Versions, dimensions, DataVersion, palettes, data and entity envelopes are checked with field context. Dimensions use unsigned 16-bit interpretation. The block limit is 16,777,216, matching the default plot's volume; streams shorter than their minimum required entry count are rejected before allocating the block buffer.
- A complete v2 `Metadata.WEOffsetX/Y/Z` takes precedence over `Offset`. Missing legacy offsets fall back to the vector or zero; incomplete or mistyped legacy offsets fail. v3 uses its schema `Offset`, defaulting to zero. Checked negation converts displacement to the existing clipboard convention.
- Palette indices can be sparse. Duplicate/negative indices, missing references, truncated/overflowing VarInts, trailing bytes and invalid block-state strings fail. Blocks resolve by names and properties to the target registry, retaining properties outside the typed simulation model as opaque states. Unsupported namespaces and unknown blocks fail explicitly.
- v3 `Id` and `Pos` envelopes are normalized around `Data`, with the envelope ID authoritative. Supported entity parse failures are errors. Unknown block-entity types are skipped with an aggregated warning. Ordinary entities and biome data are ignored with diagnostics.
- Signs preserve legacy text, modern front/back components, color, glow and wax state. Supplied modern message lists require exactly four text components. Omitted text uses blank defaults. These richer signs use the port's versioned persistence and frozen legacy migration reader.
- Comparators, barrels, furnaces, hoppers and moving pistons are supported. Containers accept legacy `Count` or modern `count`; malformed inventory data fails. Nonempty modern **persisted NBT** item-component maps are explicitly rejected because general schematic inventory data fixing is not implemented. Protocol item-component patches stored by MCHPRS retain their internal tags across its own round trips.
- Older MCHPRS v2 files sometimes contain the redundant `sticky` piston property. Consistent flags are accepted; new exports use canonical target registry properties. This does not change piston simulation.

The command assigns a new clipboard only after successful loading, preserving the previous clipboard on failure. Console diagnostics include the file path and full error chain.

Two existing exporter/paste defects were fixed: palette VarInts now use a seven-bit mask, and replacing blocks removes old block entities before restoring clipboard entities. Undo no longer leaves stale signs behind. Exported block names and properties come from the actual registry, avoiding names such as `smooth_stone_slab[type=top][type=top,...]`.

## Verified results

`cargo test -p mchprs_core plot::worldedit::schematic --locked`: **9 passed**.

| Check | Result |
| --- | --- |
| Supplied `ADDER_GWIEZDNY_TEST.schem` | 21 × 7 × 45; 6,615 block entries; clipboard offset `(0,4,44)` |
| Seven signs | `O2`, `O1`, `TICK`, `B2`, `A2`, `B1`, `A1` at the seven positions specified in the plan |
| Paste at `(100,30,100)` | Starts at `(100,26,56)`; `O2` sign at `(120,29,92)` |
| Existing five v2 fixtures | Import and v2 export/reload preserve all supported state IDs, dimensions and offsets |
| v3 → v2 round trip | All fixture states, offsets and seven sign entities preserved |
| Spatial/VarInt coverage | Asymmetric dimensions; indices 127, 128 and 16,384; export of 300 distinct states |
| Entity handling | Legacy/modern signs, both sides, default entities, authoritative v3 IDs, moving-piston properties |
| Error handling | Missing/wrong fields, bad gzip, volume limits, offsets, malformed state strings/palettes/data and entity envelopes |
| Paste/restore/reapply | Block states and sign positions preserved; stale block entities removed |

`python tools/run_protocol_smoke.py` additionally passed against an isolated **Minecraft 1.21.5** server using an independent client: `//load`, `//paste`, `//undo`, `//redo`, `//save`, v2 reload and another paste. It verified the sign packet at `(120,29,92)` and a failed import followed by saving the retained clipboard. Reconnect and process restart also passed.

The complete workspace suite still reports the previously recorded `test_memory_cell_unaligned_nanoticks` failure at tick 0. All four piston update fixtures and the existing Chungus tests pass. Neither the failing expectation nor simulation timing was altered for this loader work.

## Limits

This imports supported block and block-entity content; it does not preserve all source metadata, ordinary entities, biomes or arbitrary modded content. A different source DataVersion does not reject supported named states, but this is not Mojang's general data fixer. Graphical vanilla-client inspection of sign appearance remains a manual check; the automated test verifies the packet contents and positions.
