# Piston landing and carry-wire shape repair

Follow-up: the [performance comparison](piston-wire-performance.md) found substantial ZCore and arithmetic regressions in optimized development builds. The correctness fix should not be treated as performance-neutral.

Both reported regressions have the same missing shape-notification cause. The original publication fixture is valid; its assertion is retained, and it now runs alongside the solid-cap/carry-brake fixture.

## First missing operation

The interpreter's [`piston::finish`](../../../crates/core/src/redstone/piston/mod.rs) restores the payload, then calls `notify`. That delivers `shape_changed` in `SHAPE_NEIGHBORS` order before ordinary power-neighbor callbacks. For a wire below the landed block, `interaction::change` calls `wire::on_neighbor_changed` with `Bottom` (the direction from the changed block to the wire). This recalculates all connections because the block **above** the wire changed. `Up` is no longer possible under the solid cap, so regulation makes the north/south line `Side, Side`, with power still zero.

The movement sampler in [`passes/movement.rs`](../../../crates/core/src/redpiler/sampled/passes/movement.rs) only called `skipping_update_surrounding_blocks` after a token write. Those are power callbacks, not `interaction::change` shape callbacks. Its recorded component IDs also cannot describe a wire-side transition. At runtime, `finish_token_part` restored the token and ran those electrical steps; `refresh_wire_power` preserved the old sides. The old `notify_shape` handled direct observers only. Thus the first missing operation is wire shape recalculation during landing, before its resulting neighbor callbacks and before the direct observer watching the cap. Publication was faithfully exposing an already stale runtime state.

When a shape changes, `interaction::change` calls `update_wire_neighbors`. Direct watching observers receive shape callbacks; indirect observer power callbacks are excluded. Callback positions may repeat and must retain their order. A wire shape change at unchanged power must still schedule its observer.

## Vanilla Java 1.21.5 verification

Used the [official Mojang server](https://piston-data.mojang.com/v1/objects/e6ec2f64e6080b9b5d9b471b291c33cc7f509733/server.jar), SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`, SHA-256 `ae7681dadce21b6b4017d28e7eb567d86b6c100a6969994f540b9e54f812dc29`. The server was frozen, placement pulses settled for eight ticks, and an offline client sent actual `UseItemOn` packets. Each `tick step 1` waited for exactly one game-time increment.

[Captured block states](evidence/java-piston-wire-shape.json) and the [capture script with assertions](evidence/java-piston-wire-shape-capture.py) are retained. Run the script from this workspace with `py docs/redpiler-regressions/2026-11-02/evidence/java-piston-wire-shape-capture.py`; its isolated server and output directory are on E:.

| Fixture | Java result |
| --- | --- |
| Solid cap over climbing wire | Extension ticks 0–1: north `Up`; tick 2 onward: north `Side`. South stays `Side`, east/west stay `None`, power stays `0`. |
| Observers in solid-cap fixture | During extension, cap observer is powered at ticks 2–3, wire observer at ticks 4–5. During retraction, both are powered at ticks 2–3. |
| Cap removed during retraction | North returns to `Up` at tick 0, still at power `0`. |
| Original publication fixture: lone cross beside arriving redstone block | Cross through ticks 0–1; at tick 2 becomes north/south `None`, east/west `Side`. The fixture really must change sides. |

Also inspected the official jar with `javap -c -p`, using the [official server mappings](https://piston-data.mojang.com/v1/objects/f4812c1d66d0098a94616b19c21829e591d0af3a/server.txt), SHA-1 `f4812c1d66d0098a94616b19c21829e591d0af3a`. `PistonMovingBlockEntity.tick` (mapped source lines 327–360, obfuscated `ebl`) restores a surviving payload with `setBlock(..., 67)` and follows with `neighborChanged`. Flag 67 includes neighbor/client updates and piston movement, without suppressing shape updates. `RedStoneWireBlock.updateShape` (lines 168–182, `dtx`) calls `getConnectionState` for an UP neighbor change. This agrees with the measured landing behavior, independently of MCHPRS's interpreter.

## Fix and publication/reset path

- [`sampled/shapes.rs`](../../../crates/core/src/redpiler/sampled/shapes.rs) samples the relevant local token geometries using the existing bounded `compile_local` sampler. It records transformations of stored wire sides, preserving existing dot/cross choices, and ordered neighbor callbacks. Power is preserved separately. Invalid overlapping arrangements remain errors.
- [`executor.rs`](../../../crates/core/src/redpiler/sampled/executor.rs) replays those shape programs after the movement's electrical contributions have been updated and before ordinary movement callbacks. Actual shape changes mark the wire dirty and deliver their callbacks without deduplicating repetitions or suppressing them with the phase stamp. Direct observer shape callbacks retain piston shape order. Existing fallback observer handling remains for manually assembled programs.
- [`llvm/shapes.rs`](../../../crates/core/src/redpiler/sampled/llvm/shapes.rs) encodes the **same** sampled transformations, sharing identical lookup maps. [`llvm_movement.c`](../../../crates/core/src/redpiler/sampled/llvm/llvm_movement.c) validates table ranges, node kinds, axis products, map values and callbacks, then replays them in the same location/order. The native semantic version is now **20**; stale DLLs are rejected.
- Publication already uses `Compiler::publish` → LLVM `sync_publication` where applicable → `Runtime::publication_changes`. Rust dirty marking and LLVM snapshot wire-side synchronization expose the changed sides. `All` and `Redstone` publish wire shapes; `io_only` hides them during execution.
- `Compiler::try_reset` publishes the complete final projection and restores pending ticks. The strengthened publication test checks expected visibility, both backends' final wire after reset, and pending-tick order every tick and after reset. No publication/reset algorithm change was needed.

There are no runtime interpreter ticks, world-wire walks, new dependencies, configuration flags, or changed public APIs. Geometry selection remains within the existing 4096-local-state admission bound.

## Validation

Builds used `CARGO_TARGET_DIR=E:/codex-builds/mchprs-redstonedb-investigations-20261102/cargo-target`. A freshly compiled Windows x64 LLVM DLL was loaded explicitly via `MCHPRS_LLVM_DLL=E:/codex-builds/piston-wire-shape-20261102/native_engine.dll` (SHA-256 `82068710cf555c3dcf689a77db94ca87cdc8177db737105c389838e158cdbd1f`, semantic 20).

Final combined command:

```powershell
cargo test -p mchprs_core --lib -- redpiler::sampled::tests redpiler::publication_tests llvm_ observer redstone::piston redpiler::sampled::adder_tests
```

Result: **179 passed, 0 failed, 6 ignored**. This includes both named regressions, both publication geometries in all three modes, movement/landing FIFO checks, observer scheduling/feedback, instant-piston tests, malformed native-table rejection, Rust/LLVM state and scheduling parity, and six adder/reference tests. The adder run reported zero mismatches and zero runtime failures. Logs are under `E:/codex-builds/piston-wire-shape-20261102/`.

The publication tests and landing regression also passed with `--features redpiler-full-publication`: **10 passed, 0 failed**. The feature-enabled test binary was rechecked against the final rebuilt DLL after native validation ordering was tightened.

Remaining limits: Java evidence observes block states, not the internal Java callback/tick queue. It covers these two geometries, not every ALU schematic. LLVM validation here is Windows x64; Linux and the ignored long-running tests were not run. No full-CPU performance benchmark or full repository suite was run, so this does not establish that unrelated recorded fixture failures are resolved.
