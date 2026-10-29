# MROWW external API (protocol 3)

The service requires `{"id":1,"op":"rawr","protocol":3}` and advertises `capabilities.schematic_load:true`. `load_schematic` accepts base64 gzip Sponge v3, rejects entity/block-entity payloads and creates a fresh public headless session using MROWW's native importer. Its minimum corner maps to session-local `[0,0,0]`; offsets are ignored.

`compile` selects the existing interpreter or compiled engine. `advance` forwards ordered controls and advances MROWW's own scheduler. `project` returns either the complete initial block baseline (including air) or incremental changes; air entries represent removals. `inspect` reads a named state, and `close` releases the session. No effects, sounds, animations or Paper pending ticks are part of this contract.

Limits are 1 MiB per JSON line, 675,000 bytes decoded compressed schematic, 64 blocks per axis, 30,000 cells to bound a complete baseline, 64 controls and 64 ticks per call. The existing Sponge reader bounds decompressed NBT to 256 MiB and validates dimensions and NBT structure. The strict headless import checks entity content and API dimensions before allocating the block array; normal WorldEdit imports are unchanged. Unknown names/properties, unsupported block entities, entities, moving pistons, and native compiler rejection are returned as errors. Source data version alone does not reject a schematic.

Fresh schematic sessions run native redstone initialization once after all blocks are installed, then rely on the backend's scheduler. This creates backend work without importing Paper's tick queue. Block properties returned over the API use the native registry vocabulary. The optional build-time `MROWW_API_REVISION` label is diagnostic; an unlabeled local build is not rejected solely for that reason.

Build with `cargo build -p mroww-redgear-api`. Gameplay verification has not been performed.
