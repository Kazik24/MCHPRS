# Plan: add Sponge schematic v3 loading

Written: **2026-10-03**. Status: **implemented and tested on the 1.21.5 port branch**.

See [SPONGE_V3_IMPLEMENTATION.md](SPONGE_V3_IMPLEMENTATION.md) for completed checks, fixes and supported-content limits. The original plan below records the earlier 1.21.1 working-tree state; the corrected target is 1.21.5 / DataVersion 4325.

## Outcome and scope

Make `//load rf\ADDER_GWIEZDNY_TEST.schem` populate a usable clipboard, including its seven signs, and paste it at the correct position. Continue loading the existing Sponge v2 fixtures and files produced by `//save`.

This work adds **v3 imports**. Keep the default exporter on v2; v3 export can follow as a separate change. Biomes, ordinary entities, arbitrary modded blocks, and general Minecraft data-version conversion are outside this change. Describe the feature as importing supported blocks and block entities from Sponge v3, rather than claiming lossless support for every v3 schematic.

Coordinate shared block-entity changes with [the Minecraft 1.21.1 port plan](PORTING_1_21_1.md), especially phases 5 and 7. The working tree currently declares Minecraft `1.21.1` and data version `3955`; recheck this when implementation begins. The earlier analysis ran before those port edits. Do not overwrite ongoing protocol or generated-registry work.

## Confirmed failure and fixtures

| Property | Existing v2 fixture | New v3 fixture |
| --- | --- | --- |
| File | `test_data/UpdateTesterExtendNonInst.schem` | `test_data/ADDER_GWIEZDNY_TEST.schem` |
| Schematic version | 2 | 3 |
| Minecraft data version | 2975 | 3953 |
| Dimensions, X/Y/Z | 7 / 3 / 7 | 21 / 7 / 45 |
| Block entries | 147 | 6,615 |
| Block entities | None | Seven signs |

The new fixture is valid gzip/NBT. All 6,615 palette references resolve, and the analysis found 44 supported palette state names. Its SHA-256 is `1ca12fe1214dff068d7eff82a50ade332b805dc48628a1eadb5fd6a33c682245`; the copy in `schems/rf` was identical.

The current `load_schematic` reads `Width` directly from the NBT root and fails with `Expected Value::Short but got None`. After adapting the field locations in memory, the existing loader loaded all blocks but produced zero block entities: the sign parser requires legacy `Text1`–`Text4` fields and ignores parse failures.

Revalidate these observations against the active working tree before changing code. A schematic format version, Minecraft data version, and network protocol version are separate values.

## References and field mapping

Use the official [Sponge v3 specification](https://github.com/SpongePowered/Schematic-Specification/blob/master/versions/schematic-3.md) and [v2 specification](https://github.com/SpongePowered/Schematic-Specification/blob/master/versions/schematic-2.md). Record the specification revision consulted during implementation.

Paths below are relative to the actual NBT root:

| Input | Existing v2 layout | v3 layout |
| --- | --- | --- |
| Schema | Root compound itself | Root's `Schematic` compound |
| Version and data version | `Version`, `DataVersion` | `Schematic.Version`, `.DataVersion` |
| Dimensions | `Width`, `Height`, `Length` | `Schematic.Width`, `.Height`, `.Length` |
| Palette | `Palette` | `Schematic.Blocks.Palette` |
| Encoded blocks | `BlockData` | `Schematic.Blocks.Data` |
| Block entities | `BlockEntities` | `Schematic.Blocks.BlockEntities` |
| Block-entity payload | Fields beside `Id` and `Pos` | Nested `Data` beside `Id` and `Pos` |
| Paste displacement | Existing `Metadata.WEOffsetX/Y/Z` convention | `Schematic.Offset` |

The NBT root name is separate from a child named `Schematic`. Do not dispatch using the root name or extension. For v3, absent `Offset` means zero displacement and absent `BlockEntities` means an empty list. Metadata is optional.

## Implementation sequence

### 1. Capture compatibility expectations

**Files:** `crates/core/src/plot/worldedit/schematic.rs`, `test_data/*.schem`.

- Add focused loader tests alongside the schematic module, using `include_bytes!` for fixtures so tests do not depend on the current directory.
- Record dimensions, offsets, semantic block states, and block entities for existing v2 files. Compare names/properties rather than historic numeric block IDs as the protocol port changes those IDs.
- Add the failing v3 regression test. Keep `ADDER_GWIEZDNY_TEST.schem` unchanged and include it in the eventual implementation commit; it is currently untracked.
- Capture existing redstone fixture results, including the separately documented memory-cell discrepancy. Do not change simulation expectations to accommodate the loader.

**Complete when:** the new failure and the existing v2 behavior are reproducible in focused tests.

### 2. Separate schema selection from clipboard construction

**File:** `crates/core/src/plot/worldedit/schematic.rs`.

- Retain the existing gzip/NBT reader and public `load_schematic(impl Read)` interface.
- Select the schema compound, read a typed `Version`, and explicitly dispatch versions 2 and 3. Report unsupported versions, missing versions, and wrong tag types with their NBT paths.
- Require the nested schema for v3. Continue accepting the existing flat v2 layout; accepting a wrapped v2 schema is a small compatibility extension if covered by a test.
- Introduce a private normalized view, with dimensions, clipboard offset, palette, encoded block bytes, entity list, format version, and source data version. Borrow NBT maps/slices where practical rather than cloning the entire document into a fake v2 file.
- Give each version a small adapter and share block decoding and clipboard assembly.
- Read and report `DataVersion`, but resolve supported blocks by names/properties. A different source data version alone must not reject the supplied fixture or old v2 fixtures. Do not imply that this performs Mojang data fixing.
- Return a clear unsupported-content error for a v3 document without a block container. Report ignored nonempty biome/entity sections as aggregated warnings.

**Complete when:** both formats reach the same clipboard construction path, and format errors identify the actual field.

### 3. Preserve offsets and validate block decoding

**Files:** `schematic.rs`; inspect `worldedit/mod.rs::paste_clipboard` without changing its coordinate convention.

The existing paste path uses `paste_position - clipboard_offset`. Therefore convert a v3 displacement `[dx, dy, dz]` to clipboard offset `[-dx, -dy, -dz]`, using checked negation. The fixture's `[0, -4, -44]` becomes `(0, 4, 44)`. A paste at `(100, 30, 100)` starts at `(100, 26, 56)`.

- Preserve the current v2 `WEOffset*` convention. When those fields are absent, support the v2 `Offset` vector or zero; reject a partially present or mistyped legacy offset rather than silently guessing. Lock precedence down in tests: complete legacy offsets take priority for v2; v3 always uses its schema `Offset`.
- Require three integers in a provided offset. Do not use WorldEdit's metadata `Origin` as the relative paste displacement.
- Decode dimensions as unsigned 16-bit values, then widen; the current direct signed-short-to-`u32` cast is unsafe for large values. Reject zero dimensions.
- Compute volume with checked arithmetic. Before allocating the block buffer, reject a byte stream shorter than the minimum one byte per required block. Use checked allocation sizing and a documented schematic-volume bound consistent with the server's intended clipboard limits.
- Replace unchecked byte indexing and palette `unwrap()` calls with errors. Decode at most five bytes per varint, reject overflow in the fifth byte and unterminated values, and reject missing palette references.
- Require exactly the expected block count with no trailing encoded entries. Validate nonnegative, unique palette indices; allow sparse indices and multiple names mapping to the same internal state.
- Keep the existing coordinate traversal. Do not reuse network chunk packing for schematic palette indices.
- Make palette parsing consume the entire state string and accept valid property names/values, including underscores. Unsupported namespaces/names must produce an explicit error instead of becoming air. Preserve intentional property projections already supported by the block model, documenting any lossy cases; do not broaden gameplay semantics here.

**Complete when:** both formats produce correct block positions and malformed input returns an error without a panic.

### 4. Adapt block-entity envelopes and sign payloads

**Files:** `schematic.rs`, `crates/blocks/src/block_entities.rs`; coordinate with the port's sign/container work.

- Read `Id` and `Pos` from the v3 envelope. Pass a normalized payload to the existing entity parser, making the envelope ID authoritative instead of relying on an optional duplicate `Data.id`.
- Accept an omitted `Data` compound as an empty payload; use supported entity defaults where the implementation defines them. Report unsupported defaults or malformed supported payloads explicitly.
- Require exactly three position integers and validate local positions against schematic dimensions. Apply paste displacement only during paste, to both blocks and block entities.
- Keep the flat v2 entity path compatible. Distinguish unknown entity types from failed parsing of supported types. Skip unknown types with an aggregated warning; fail loading when supported entity data is malformed instead of silently dropping it.
- Extend sign reading to accept legacy `Text1`–`Text4` and modern `front_text.messages`, preserving each text-component string. Require four string messages when supplied and use blank rows for legitimately absent text. Schema version alone does not determine which sign payload is present.
- With today's `SignBlockEntity { rows }`, import the front side. Explicitly report loss of meaningful back text, color, glow, or wax state. If the port adds a richer sign model first, use it and test both sides; do not change persisted sign types independently of its migration work.
- Preserve supported comparator, container, and moving-piston payload handling through the envelope adapter. Check modern inventory payloads against the shared port reader; if unsupported, return a clear error rather than importing an empty inventory. Keep general inventory migration and moving-piston fidelity fixes tracked in the port plan.

For the supplied fixture, retain signs at `(20,3,36)`, `(20,3,40)`, `(2,5,44)`, `(3,6,36)`, `(3,6,38)`, `(3,6,40)`, and `(3,6,42)`, with first-row labels `O2`, `O1`, `TICK`, `B2`, `A2`, `B1`, and `A1`, respectively. The remaining rows are blank.

**Complete when:** the fixture clipboard contains seven readable signs and supported entity failures no longer disappear silently.

### 5. Integrate diagnostics and verify existing save behavior

**Files:** `schematic.rs`, `worldedit/execute.rs`; documentation describing schematic support.

- Add load context to errors: file path at the command boundary, format version and NBT field within the loader, and entity ID/local position for entity failures.
- The command already logs the underlying error in a second console line. Improve its context rather than assuming the reason is never logged.
- Aggregate successful-import warnings by unsupported or projected content type, avoiding one log per block. Keep clipboard assignment after a successful load so an error preserves the previous clipboard.
- Keep `//save` writing v2 and test semantic save/load compatibility with both imported formats. This preserves supported clipboard content, not source metadata or ignored content.
- Exercise a palette index above 127. The current writer masks with `0xff` before shifting by seven; if the round-trip test exposes its existing varint defect, fix that mask to `0x7f` as a narrow prerequisite for reliable existing export.
- Document v2/v3 import support, v2 export, sign projection, and content limitations. Add a link to this plan from the port documentation when the implementation is integrated.

**Complete when:** command errors are actionable and the existing exporter produces files the shared loader can read correctly.

## Validation and acceptance

| Check | Required result |
| --- | --- |
| Existing v2 fixtures | Same dimensions, offsets, and supported semantic states |
| Real v3 fixture | 21 x 7 x 45; 6,615 entries; clipboard offset `(0,4,44)`; seven signs with the labels above |
| Offset integration | At paste `(100,30,100)`, start `(100,26,56)`; `O2` sign at `(120,29,92)` |
| Spatial order | Asymmetric synthetic schematic places unique states at their intended X/Y/Z coordinates |
| Property fidelity | Fixture piston facing/extension/type, wire connections/power, repeater settings, slab placement, and sign rotation survive import |
| Optional fields | Missing v3 metadata, offset, and entity list load with the defined defaults |
| Entity variants | Legacy and modern signs; v3 payload without duplicate `id`; supported comparator/container/piston envelopes |
| Multi-byte IDs | Palette indices 127, 128, and a larger sparse index decode correctly; export/import crosses 127 correctly |
| Malformed input | Wrong/missing version or tag type, bad gzip/NBT, zero/oversized dimensions, bad offset/position length, truncated/overflowing varint, unknown palette ID, duplicate palette index, and extra block data return errors |
| Unsupported content | Explicit error or aggregated warning according to the policies above; no silent air/inventory/sign replacement |
| Clipboard atomicity | Failed load retains the player's previous clipboard |
| Save compatibility | Imported supported content survives v2 export and reload, compared semantically |
| Existing simulation tests | No new redstone/piston failures; report the known memory-cell result separately |

Run the focused new tests first, then the existing relevant regression tests and the checks required by the active port branch. Suggested commands from the repository root:

```powershell
cargo test -p mchprs_core plot::worldedit::schematic
cargo test -p mchprs_blocks
cargo test -p mchprs_core redstone::tests::test_updates_
cargo test -p mchprs_core redstone::tests::test_memory_cell_unaligned_nanoticks
cargo check --workspace
cargo fmt --all -- --check
```

The memory-cell command reports the known discrepancy; it is not permission to hide or alter that test. Check actual module names when implementation begins. Do not reformat unrelated in-progress port files to satisfy formatting checks.

Finally, use an isolated server run directory and execute `//load`, `//paste`, `//undo`, `//redo`, `//save`, and reload the saved file. Verify signs visually and compare circuit state before advancing ticks. The loader change does not promise new redstone simulation behavior.

## Proposed commit order

1. Fixture regression tests and schema adapters for v2/v3.
2. Checked block decoding, offset handling, and malformed-input coverage.
3. Block-entity envelope normalization and modern sign import.
4. Command diagnostics, exporter round-trip fixes if needed, integration tests, and support documentation.

Each commit should compile. Keep changes to shared sign/item representations coordinated with the Minecraft port and its persistence compatibility checks.
