Current status, **2026-10-04**: the reported repairs are implemented and covered by normal regressions. See [implementation](../PISTON_TIMING_IMPLEMENTATION.md) and [review](../PISTON_REVIEW.md). The findings below preserve the original audit snapshot; statements that no fix is applied refer to that snapshot.

# Piston completion: restored blocks miss placement and support updates

Documented **2026-10-04**. **Primary repair priority**, as selected by the user. Status: three focused Rust failures reproduced; compared with exact Java 1.21.5 completion and block callbacks; no fix applied.

A piston payload cannot be restored correctly by copying its stored state and notifying nearby redstone alone. Completion must account for the destination's neighbors, placement callbacks, support rules and reference-specific state normalization. The current shortcut leaves moved observers powered, retains waterlogging and leaves a torch on an invalid support.

Related issues: [lifecycle ownership](LIFECYCLE.md) and [scheduled tick identity](SCHEDULED_TICKS.md). The [deep audit](../PISTON_DEEP_AUDIT.md) and [deep-evidence.json](deep-evidence.json) retain provenance and measured results.

## Reproduced cases

Use an empty plot, an East-facing sticky base **B = (40,30,40)**, head position **H = (41,30,40)** and pushed destination **D = (42,30,40)**. Power comes from below B. Execute the extension request, then advance eight interpreted game ticks so ordinary motion has settled.

| Case | Initial state / stimulus | Measured current result | Java-derived requirement |
| --- | --- | --- | --- |
| Powered observer relocation | Powered Down-facing observer at H; its pulse-off tick is pending at H | Observer at D remains `powered=true` | The old tick belongs to the old position/type. Placement at D must apply the observer's reset/notification rules; it must not remain powered indefinitely in this setup |
| Waterlogged payload | Oak stairs at H with `waterlogged=true` | Oak stairs at D still have `waterlogged=true` | Ordinary moving-entity completion clears waterlogging on the restored state |
| Support loss | Stone at H, lit torch immediately above H | Torch remains above the final horizontal head | Structural updates must remove the torch when the old support becomes a head that does not support it |

Measured output:

```text
MOVED_OBSERVER destination = Observer { facing: Down, powered: true }
WATERLOGGED destination properties include waterlogged = true
SUPPORT_REMOVED torch = RedstoneTorch { lit: true }
```

These cases use minimal worlds, not the memory-cell schematic. Their Rust outcomes are measured. The Java expectations come from pinned source inspection; there is no live Java circuit recording of these three setups yet.

## How completion currently runs

Relevant source:

- [piston.rs](../../crates/core/src/redstone/piston.rs): `moving_piston_tick`, `destroy_moved_block`, `on_piston_state_change`, `update_neighbors`.
- [plot/mod.rs](../../crates/core/src/plot/mod.rs): `World::set_block_raw` implementation.
- [redstone/mod.rs](../../crates/core/src/redstone/mod.rs): observer `update`/`tick` and notification dispatch.
- [interaction.rs](../../crates/core/src/interaction.rs): placement, structural `change`, survival checks and ordinary destruction.
- [blocks/mod.rs](../../crates/blocks/src/blocks/mod.rs): cube/support classification.

On extension completion, `moving_piston_tick` reconstructs the base and head, reads the carried state, and writes an eligible payload directly into D. `PlotWorld::set_block_raw` changes storage; it does not automatically execute Java-style block placement or shape callbacks.

The completion routine then calls `on_piston_state_change`. That helper updates H itself, its neighbors, optionally D's neighbors and optionally B's neighbors. These are calls to `redstone::update`, rather than the structural `interaction::change` path. Its comment that the pushed block was already updated by a head update does not imply that all the pushed block's placement behavior ran.

For the Down-facing observer, H's notification reaches D with a side that is not the observer's watched face. The observer `update` branch with `Some(dir)` therefore does not schedule its pulse-off work. Its old scheduled tick at H does not relocate automatically to D; it can instead dispatch into the moving head, as documented in [SCHEDULED_TICKS.md](SCHEDULED_TICKS.md).

For stairs, the stored state is copied unchanged, including `waterlogged=true`. For the torch, redstone updates do not run the required survival check. The fork also treats a piston head as a cube in support classification, so merely calling the existing cube-based support test would still be insufficient.

## Java 1.21.5 comparison

The [reference manifest](reference-manifest.json) pins the official implementation. Inspected classes include `PistonMovingBlockEntity` (`ebl`), `ObserverBlock` (`dtg`), `PistonHeadBlock` (`ebj`), `PistonBaseBlock` (`ebi`) and `NeighborUpdater` (`ezh`).

### Restored state and callbacks

Normal moving-entity completion checks its current moving block, derives the restored state from destination neighbors, and takes the reference's corresponding placement/update path. A restored state that cannot survive may require temporary restoration followed by destruction/shape processing so the correct callbacks execute.

In the non-air normal-completion branch, Java explicitly clears `WATERLOGGED` when the restored state has that property. The interrupting `finalTick` path is separate. Do not clear every carried state's waterlogging in a generic helper without checking both paths against the reference.

`ObserverBlock.onPlace` resets a powered observer when the destination has no pending observer tick, and notifies its output. The exact condition matters: a pre-existing tick at the destination must be treated as the reference treats it. Always forcing every moved observer off, or blindly moving its old tick to the destination, would substitute a new rule for Java's callback.

Structural shape updates and support geometry determine whether the torch survives. A horizontal piston head is not interchangeable with an ordinary full supporting cube.

### Notification order

Java separates shape updates, placement/removal callbacks, ordinary neighbor notifications and observer-triggering changes. Its default neighbor order is **West, East, Down, Up, North, South**. The fork's shared face iteration is **Up, Down, North, South, East, West**, and its piston helper adds a custom head/pushed/base sequence with skipped faces.

The three failures prove missing completion behavior. They do not isolate neighbor iteration order as the cause of the memory-cell failure. Capture exact order and phase before replacing the sequence, and keep piston-specific changes local instead of changing all component loops globally.

## Repair contract

1. Finalize only the moving entity that still owns its position; apply [lifecycle checks](LIFECYCLE.md) before completion writes and callbacks.
2. Derive the restored state using the destination's current neighbors and the correct normal/interrupted completion path.
3. Apply ordinary-completion waterlogging normalization where Java applies it.
4. Execute the restored block's required placement behavior, including the observer's conditional reset and output notification.
5. Send structural updates to positions whose support or shape changed. Use suitable support geometry; a general `is_cube()` check is not sufficient for piston heads.
6. Preserve the reference order of writes, shape updates, placement/removal callbacks, neighbor notifications and observer reactions. Avoid duplicate callbacks through multiple helper layers.
7. Keep ordinary tick identity intact: old work must not follow a moved block or dispatch into its replacement accidentally.
8. Verify interruption, restart and editing without replaying completion callbacks twice or omitting them.

Calling `place_in_world` everywhere is not, by itself, the repair. That helper has its own update sequence and side effects. Specify the required completion operations and order, then adapt reusable helpers only where their behavior matches the reference.

## Acceptance cases

The following existing diagnostics fail on the recorded snapshot:

| Test in [deep-regressions.rs](deep-regressions.rs) | What must pass |
| --- | --- |
| `moved_powered_observer_does_not_stay_powered_forever` | The moved observer settles unpowered in the recorded setup |
| `moved_waterlogged_stairs_are_not_still_waterlogged` | The restored stairs remain stairs, with `waterlogged=false` |
| `moving_support_breaks_torch_above_old_payload` | The unsupported torch becomes air |

Additional required checks:

- Observer destinations with and without pending observer work; observe output pulses and notified positions, not only the final power flag.
- Normal completion versus early `finalTick`/drop behavior, including waterlogging and unsupported carried states.
- Six directions, relevant attachment faces, sticky retraction and source-head/base completion.
- A supported neighboring block remains intact while an unsupported one is removed; no blanket clearing of all attachments.
- Completion repeated, interrupted by replacement, and resumed after save/load: callbacks occur only as required for the correct motion.
- Equivalent game/nano/pico advancement produces the same states and notification order.

These additional cases are proposed acceptance coverage, not newly measured results. Keep the original four passing piston-update fixtures as regression controls; validate the provisional memory-cell expectation using a live Java trace.

## Reproduction and evidence limits

Use the isolated-checkout setup in [the deep audit](../PISTON_DEEP_AUDIT.md#reproduction-and-limits), register `piston_deep_audit`, then run:

```powershell
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::moved_powered_observer_does_not_stay_powered_forever -- --nocapture
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::moved_waterlogged_stairs_are_not_still_waterlogged -- --nocapture
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::moving_support_breaks_torch_above_old_payload -- --nocapture
```

This document adds no simulation changes or new test executions. Live Java notification traces, client presentation, save/restart equivalence and complete support geometry remain implementation checks.
