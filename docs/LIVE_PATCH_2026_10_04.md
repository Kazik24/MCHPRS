# Focused patch for live Minecraft 1.21.5 testing

The immediate scope follows the user's request to patch the reported defects and observe devices on the live server. This is an incremental patch; the full piston state-machine repair remains outstanding.

## Schematic paste crash

Production logs identify `rf/Lord225@licznik_pochodnia.schem` immediately before `//paste` panicked with `HeterogeneousList` in `PacketEncoderExt::write_nbt_blob`. A copy of that exact v2 schematic is retained as `test_data/sign_mixed_text_v2.schem` (1,682 bytes; 16 × 8 × 33; two signs). SHA-256:

```text
d3debcb545a6f4cfa1f004bf796cb1e13859c5ff30158919ad9d90daeca8c6e6
```

Its first sign mixes formatted rows (`Addery `, `by`, `Lord225`) with an empty JSON string row. Converting these to target text components produces compound and string entries in the same NBT list. The older NBT writer requires homogeneous lists and panics when the sign update packet is encoded. The real fixture reproduced the exact production panic before the fix.

The text conversion now uses Minecraft's documented list representation: mixed lists contain compound entries, and non-compound values are wrapped under the reserved empty key. Homogeneous lists retain their original types. Nested `extra` and translation `with` lists use the same conversion. Reading sign text unwraps these entries back into the internal JSON representation, preserving rows, style and translation arguments.

This format applies to sign updates, chunk block-entity data and schematic export. No rows are discarded, and the bincode save schema is unchanged. The format is documented in the [official 1.21.5 NBT changes](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5).

## Observer power and piston input

The strong-power observer arm previously returned 15 on every queried side of a powered observer. It now checks facing, matching the existing weak-power arm and the pinned official 1.21.5 `ObserverBlock` signal methods. The regression checks all six facings, six queried sides and both powered states, including conduction through stone and the resulting piston input. It failed before the one-arm correction and passes afterward.

This addresses phase 1 of [the piston repair plan](PISTON_FIX_PLAN.md). It does not change observer notification timing, piston advancement or serialized moving entities. Event separation, push-chain preservation, movement timing, short-pulse dropping and the memory-cell discrepancy still need repair; this patch does not claim complete vanilla piston compliance.

## Validation

- Exact production fixture: import, packet-producing paste, both sign sides and v2 export/reload pass.
- Nested mixed text lists retain content, style and translation arguments; homogeneous lists retain their types.
- Independent 1.21.5 clients verify the real fixture's rendered message contents, paste, undo/redo, save/reload and reconnect, alongside the existing v3 fixture, piston placement and restart checks.
- Rejected neighboring/spawn/template saves remain intact and all three isolated load-error test servers shut down cleanly.
- Workspace/all-target tests: **41 passed, 1 known failure**. All four existing piston update fixtures and Chungus cases pass. `test_memory_cell_unaligned_nanoticks` still fails at tick 0; its expectation was not changed or ignored.
- Workspace/all-target check, formatting and locked debug/release builds pass.

The patch is in the local source/builds. Applying it to the Linux production container requires rebuilding and recreating the image; production was not redeployed by this task.

## Turning live failures into regressions

For a failing device, retain its exact schematic and record the load/paste position, stimulus sequence, game/redstone tick durations, RTPS, compiler mode, expected observation and actual observation. Include the relevant server-log interval. Preserve the failing input before editing it. The sign crash above is now a fixture in both the Rust tests and the independent-client integration test; subsequent reports can follow the same route.
