# Piston discrepancy: Java reference versus the MCHPRS fork

Audit date: **2026-10-03**. Audited engine: **`cb3d4e27a67737abad990932ce94b5461368d45f`** (`p0.1.0`). The current working branch is `port/piston-1.21.1`; its piston, interpreted-redstone and scheduler files were still identical to that baseline when checked. Experiments ran in a separate detached checkout, using the recovered lockfile. The ongoing port and shared simulation source were not modified.

## Finding

An additional [deep audit against exact Java 1.21.5](PISTON_DEEP_AUDIT.md) checks the current port snapshot, adds 31 diagnostics and identifies movement-rule, tick-identity, lifecycle and support-update discrepancies. The version/snapshot statements below describe the original audit run.

**The fork's piston state machine postpones logical extension until movement completes, suppresses further piston decisions with a three-game-tick scheduled lock, and lacks Java's short-pulse retract-and-drop event.** These are connected defects in its movement/event model, rather than a protocol or client-animation problem.

The delayed extended-state write directly explains the existing memory-cell assertion failure at test index 0. Restoring that write makes index 0 pass, but the test then fails at index 1. A separate one-game-tick pulse reproducer confirms that the fork pulls the sticky payload back when Java's event rules require it to remain at the pushed destination. Consequently, uncommenting the extended-state write is **not a complete fix**.

The memory-cell test still contains `//todo, get the data from real minecraft`. Its entire expected trace must not be presented as a measured Java trace. This audit proves source-level Java divergences and reproduces the Rust symptoms; it does not claim a live Java run of that schematic.

## Java references and version control

| Reference | Pinned input | Use and limitation |
| --- | --- | --- |
| Official Java 1.18.2 implementation | Server SHA-1 `c8f83c5655308435b3dcf03c06d9fe8740a77469`; official server mappings | Exact version of the original fork; inspected the piston base, moving entity, structure resolver, observer and server block-event processing |
| Official Java 1.21.1 implementation | Server SHA-1 `59353fb40c36d304f2035d51e7d6e6baa98dc05c`; official server mappings | Cross-check of the target port: the relevant extended-state and retract/drop distinctions remain |
| MinecraftForge `1.18.x` | `9c5d527d71553d297d6ad684211929f66f596ae1` | Open-source piston patches retain distinct event IDs 0, 1 and 2, with pre/post movement hooks; cancelled/modded events can change behavior |
| Glowstone | `e9aeaa46330326f0d3917dce8f9bc4cdd1e434bd` | Independent open-source Java implementation; writes the base's extension bit during movement and resolves push lists, but its simplified movement cannot establish exact vanilla short-pulse timing |

Official binaries and mappings were downloaded through Mojang's version manifest, and both downloads for each version were checked against their published SHA-1. Selected classes were inspected with [CFR 0.152](https://www.benf.org/other/cfr/). The downloaded/decompiled Mojang implementation is a reference, **not a claim that vanilla Minecraft is open source**. No decompiled Java files or server binaries are included in this repository.

The exact official download URLs and hashes are in [reference-manifest.json](piston-audit/reference-manifest.json). The open-source comparisons are [Forge's piston patch](https://github.com/MinecraftForge/MinecraftForge/blob/9c5d527d71553d297d6ad684211929f66f596ae1/patches/minecraft/net/minecraft/world/level/block/piston/PistonBaseBlock.java.patch) and [Glowstone's piston implementation](https://github.com/GlowstoneMC/Glowstone/blob/e9aeaa46330326f0d3917dce8f9bc4cdd1e434bd/src/main/java/net/glowstone/block/blocktype/BlockPiston.java).

In the official 1.18.2 mappings, `PistonBaseBlock` is `coo`, `PistonMovingBlockEntity` is `cor`, `PistonStructureResolver` is `cos`, `ObserverBlock` is `cil`, and `ServerLevel` is `adw`. The corresponding 1.21.1 piston classes are `dsv`, `dsy` and `dsz`. This records which binary classes were actually inspected, rather than relying on an unversioned source mirror.

## How the current fork works

1. A redstone neighbor update calls `update_piston_state`. It evaluates direct power excluding the facing side, then quasi-connectivity around the block above the piston.
2. When desired extension differs from `piston.extended`, it queues a position with zero delay, provided **no scheduled tick at that position exists anywhere in the scheduler**.
3. `piston_tick` reads the current power again and chooses extension or retraction. The queue contains a position, not a captured piston event type.
4. Extension replaces the adjacent head/payload block with `MovingPiston`. Its entity stores the original adjacent block as `block_state`, schedules completion after one redstone tick and schedules a base decision after three game ticks.
5. Completion recreates an extended base and a piston head, then writes the stored payload one more block forward. Only here does normal extension set the base's `extended` flag.
6. Retraction waits for a stationary head, removes a pullable sticky payload from two blocks ahead, puts its moving entity at the head position and later restores the payload there. The base remains extended until this completion.
7. A custom neighbor-update routine notifies the head, optionally the pushed position and optionally the base.

In this fork, `schedule_tick(..., 1, ...)` means **two game ticks**; `schedule_half_tick(..., 3, ...)` means **three game ticks**. Each `tick_interpreted()` call advances one game-tick queue position, after draining work already due. Confusing redstone ticks with game ticks would obscure the discrepancy.

Relevant files: [piston state machine](../crates/core/src/redstone/piston.rs), [interpreted dispatch and fixtures](../crates/core/src/redstone/mod.rs), [scheduler](../crates/core/src/redpiler/backend/queue.rs), [plot advancement](../crates/core/src/plot/mod.rs) and [action definitions](../crates/core/src/world/mod.rs).

## Defect 1: extension state is written at the wrong phase

In `schedule_extend`, the ordinary base-state write is commented out around lines 204–207. `moving_piston_tick` writes an extended base only after the moving-head completion is dispatched. The resulting world can contain a moving extension while its base still claims `extended=false`.

The official Java extension event marks the base extended after successful movement setup, within that event. Completion of the moving block entity is separate. Both inspected Java versions agree on this distinction. Glowstone independently writes its base extension bit during its movement path, although it uses a different movement model.

The minimal reproducer starts a powered east-facing sticky piston and executes its queued extension with `picotick_advance(1)`. The fork produces:

```text
base: Piston { extended: false }
head: MovingPiston
scheduled head completion: +2 game ticks
scheduled base decision: +3 game ticks
```

The new regression expecting an extended base fails. In a counterfactual experiment that restores the commented write, that regression passes and the memory-cell test's first failing index moves from 0 to 1. All four existing update fixtures still pass. This establishes the cause of the first assertion failure without treating the counterfactual as a complete or correctly ordered Java port.

## Defect 2: scheduled completion is treated as a piston cooldown

Both extension and retraction call:

```rust
world.schedule_half_tick(piston_pos, 3, TickPriority::Normal);
```

`update_piston_state` then refuses to queue a decision while `pending_tick_at(piston_pos)` is true. The presence of that future base tick blocks a new power-off decision, including after the head has already finished extending.

The focused reproducer lets extension complete, removes power and calls the neighbor-update handler. Its pending queue contains only the old base decision **one game tick in the future**; it does not contain the zero-delay retraction decision.

Java's piston events and moving-entity ticks have distinct roles. An event queue can retain the requested action and validate it when executed; a moving entity's future work is not a three-game-tick veto on subsequent piston events. Removing the fork's lock alone would still encounter its `MovingPiston` early-return paths and cannot implement correct early retraction by itself.

## Defect 3: short-pulse retract-and-drop is missing

Java distinguishes extension event **0**, ordinary retraction **1** and retract-without-pulling **2**. Its retraction decision examines a matching extending moving entity two blocks ahead, its progress/last-tick time and the server tick phase. The event is queued with that distinction; ordinary sticky pulling is not used for the drop case. These paths exist in the inspected [1.18.2 binary](https://piston-data.mojang.com/v1/objects/c8f83c5655308435b3dcf03c06d9fe8740a77469/server.jar) and [1.21.1 binary](https://piston-data.mojang.com/v1/objects/59353fb40c36d304f2035d51e7d6e6baa98dc05c/server.jar), with names established by their official mappings.

The fork instead:

- Stores the moving payload at the head position until completion, rather than representing each moving destination separately.
- Does not advance the moving entity's `progress` during this piston path or track the last tick needed by the Java decision.
- Defers power-off handling until its artificial lock expires.
- Uses the normal sticky pull whenever the eventual payload is a cube without a block entity.
- Defines action 2 as `PistonAction::Cancel`, but never uses it in the piston engine. Sending a client packet with ID 2 alone would not repair the server's state.

In the minimal one-game-tick pulse test, a sticky piston begins pushing stone, loses power and is allowed to settle. The fork ends retracted with **stone at the former head position and air at the pushed destination**. The source-derived Java short-pulse expectation is the opposite: the payload remains at the pushed destination. The regression fails on the current implementation.

This confirms a movement-state discrepancy independently of the disputed expected memory-cell trace.

## Observed memory-cell evolution

The fixture removes a lit redstone torch at `(100, 30, 100)`. Its monitored piston is a downward-facing sticky piston at `(97, 30, 107)`, initially unextended, with a redstone block at `(97, 29, 107)`.

| Test index | Current base extended | Current piston power | Head position | Two blocks forward |
| --- | --- | --- | --- | --- |
| 0 | false | false | Moving piston | Air |
| 1 | true | false | Piston head | Redstone block |
| 2 | true | false | Moving piston | Air |
| 3 | true | false | Moving piston | Air |
| 4 | false | true | Redstone block | Air |
| 5 | false | true | Moving piston | Air |
| 6 | false | true | Moving piston | Air |
| 7–9 | true | true | Piston head | Redstone block |

Index 0 is the observation after the **first** `tick_interpreted()` call, not a Java server tick-number assertion. The current base sequence is `F T T T F F F T T T`. Restoring the immediate extension write produces `T T T T F T T T T T`. The fixture's provisional expected sequence is `T F F F F F F F F F`.

At index 4, the fork has pulled the redstone block back and the circuit supplies power again; it subsequently reextends. This records the whole circuit's observed evolution. It does not isolate the returned block as the sole cause of renewed power: the other pistons, observers and quasi-connected wiring participate too.

The first mismatch is proven to be delayed logical extension. Later behavior is consistent with the confirmed cooldown and missing-drop defects, but the exact intended ten-step Java sequence still requires a version-matched schematic run and event trace. The array in the test cannot supply that evidence by itself.

## Additional independently reproduced defects

| Defect | Rust result | Reference expectation and scope |
| --- | --- | --- |
| Push destination is overwritten without resolving the block chain | Pushing stone into a gold block's position leaves stone there and air beyond it; the gold block disappears | Java resolves the push list and preserves both movable blocks. Glowstone also resolves a push list with a limit of 12. This is data loss, beyond merely lacking long-chain support |
| Observer strong power ignores direction | `get_strong_power` returns 15 toward North for a powered Down-facing observer | Java observer direct power follows its directional signal method. The fork's weak-power path checks direction, but its strong-power arm does not. This can produce incorrect piston input through conducting blocks |

The observer defect is at `crates/core/src/redstone/mod.rs:100`. The chain defect follows from `schedule_extend` considering only the adjacent block and `moving_piston_tick` writing its payload into the next position without checking/moving that position's occupant. Neither additional reproducer establishes that it is the cause of this particular memory-cell assertion.

## What a complete correction requires

The sequenced implementation plan, file ownership and acceptance gates are in [the piston repair plan](PISTON_FIX_PLAN.md).

The recommended repair is to align event handling and movement state with the Java reference, then verify each phase independently:

1. Capture typed piston events, including direction and extend/retract/drop intent, separately from ordinary scheduled ticks. Deduplicate the relevant event rather than every future tick at the position.
2. Apply the base's logical transition during the successful event. For retraction, model the moving source at the base and finalize the old head consistently.
3. Resolve the full movement before mutating the world; retain each moved block's state at its moving destination. Never overwrite an unresolved occupant.
4. Track movement progress and timing so a short-pulse drop decision is made when requested, not reconstructed after a forced delay.
5. Implement the ordinary-retract and drop paths, with the correct update order. Review neighbor/observer notifications against the reference instead of assuming the current custom order is equivalent.
6. Correct directional observer strong power and test direct versus quasi-connected piston inputs separately.
7. Run the new regressions, existing fixtures and captured Java traces. Keep the provisional memory-cell expectation visible until it is verified.

These steps are implementation recommendations inferred from the comparison, not a claim that the diagnostic one-line experiment meets them. No shared piston-engine fix was applied in this task.

## Reproduction and validation limits

[regressions.rs](piston-audit/regressions.rs) contains five focused failing regression cases. They operate on an empty `PlotWorld`, so the three core state/pulse failures and the two additional failures do not depend on the memory-cell schematic. [evidence.json](piston-audit/evidence.json) records the measured outcomes and trace sequences.

Run from the repository root, on a separate baseline checkout:

```powershell
$sourceRepo = (Get-Location).Path
$auditRoot = Join-Path $env:TEMP ('mchprs-piston-audit-' + [guid]::NewGuid().ToString('N'))
git worktree add --detach $auditRoot cb3d4e27a67737abad990932ce94b5461368d45f
if ($LASTEXITCODE -ne 0) { throw 'Unable to create audit checkout' }
Copy-Item -LiteralPath (Join-Path $sourceRepo 'Cargo.lock') -Destination (Join-Path $auditRoot 'Cargo.lock')
Copy-Item -LiteralPath (Join-Path $sourceRepo 'docs/piston-audit/regressions.rs') -Destination (Join-Path $auditRoot 'crates/core/src/redstone/piston_regression_audit.rs')
Add-Content -LiteralPath (Join-Path $auditRoot 'crates/core/src/redstone/mod.rs') -Value "`n#[cfg(test)]`nmod piston_regression_audit;"
Push-Location -LiteralPath $auditRoot
cargo test -p mchprs_core --lib --offline redstone::tests -- --nocapture
cargo test -p mchprs_core --lib --offline piston_regression_audit -- --nocapture
Pop-Location
```

The copied modern lockfile lets Cargo resolve the baseline with the recovered compatible `time` dependency. It may be rewritten within the detached checkout. Offline execution requires the dependencies to be cached. The current port's uncommitted source is not copied into that checkout.

Measured results: **existing fixtures: 4 pass / 1 fail at index 0; new regressions: 0 pass / 5 fail**. Restoring only the early extended-state write yields **existing fixtures: 4 pass / 1 fail at index 1; new regressions: 1 pass / 4 fail**. New regressions are intentionally failing expectations for a future correction, not green verification of the current engine.

The full workspace suite, release build, production deployment, live Java schematic execution and complete piston compliance were not validated in this audit. No expectations in the existing fixture were changed, and no 1.21.5 retargeting was performed.
