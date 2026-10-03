# Deeper piston audit against Java 1.21.5

Audit started **2026-10-03** and completed **2026-10-04**. This follows [the original audit](PISTON_AUDIT.md) and extends [the repair plan](PISTON_FIX_PLAN.md). No shared simulation fixes were applied.

## Scope and evidence

The shared checkout advertises Minecraft **1.21.5**, protocol **770**, on `port/piston-1.21.5`, with base commit `cb3d4e27a67737abad990932ce94b5461368d45f` and uncommitted port changes. Diagnostics ran against a separate snapshot of those current sources, including the current registry mappings and compiler guard. The piston, interpreted-redstone and scheduler files were still identical to that base commit when compared.

The exact official Java 1.21.5 server and mappings were downloaded and SHA-1 verified:

| Input | SHA-1 |
| --- | --- |
| Server | `e6ec2f64e6080b9b5d9b471b291c33cc7f509733` |
| Mappings | `f4812c1d66d0098a94616b19c21829e591d0af3a` |

URLs are pinned in [the reference manifest](piston-audit/reference-manifest.json). Selected classes were inspected using CFR 0.152: piston base `ebi`, moving entity `ebl`, resolver `ebm`, head `ebj`, moving block `ebh`, observer `dtg`, server world `asb` and neighbor updater `ezh`. CFR produces incomplete local-variable reconstruction in a few methods; findings below rely on unambiguous operations, official mappings and runtime properties rather than those reconstructed variable names.

A small reflection probe also bootstrapped the **actual official Java classes** and queried their default block states. Its [source](piston-audit/java-1.21.5-block-rules.java) and measured output in [deep-evidence.json](piston-audit/deep-evidence.json) independently establish the movement reactions and block-entity flags. This is a Java runtime property check, **not a running Java world or circuit trace**. Vanilla binaries are a reference implementation; they are not described as open-source Java.

[deep-regressions.rs](piston-audit/deep-regressions.rs) contains **31 diagnostics: four passing controls and 27 failing reference/storage expectations**. Those 27 cases overlap several underlying defects; they are not 27 independent root causes. The original five regressions still fail, and the existing piston fixtures still have four passes and the memory-cell failure at index 0.

## Additional discrepancies

| Area | Measured current behavior | Java 1.21.5 reference / consequence |
| --- | --- | --- |
| Immovable blocks | Bedrock, obsidian, crying obsidian and respawn anchors are pushed; obsidian can also be pulled | `PistonBaseBlock.isPushable` rejects the explicit immovable states and unbreakable hardness. Shape alone cannot determine movability |
| Pull-only distinction | A sticky piston pulls white glazed terracotta | The runtime state reports `PUSH_ONLY`; Java rejects pulling it |
| Movable partial blocks | An iron trapdoor in front of the piston disappears | Its runtime reaction is `NORMAL`, with no block entity. It must move despite its partial shape |
| Block entities and unsupported states | A chest represented as `Unknown` is pushed; an oak sign disappears and the piston extends | Java reports a block entity on both. A normal-reaction sign blocks movement; deleting it also risks losing its text. Unknown registry states need real movement metadata or explicit conservative rejection |
| Blocked push chains | Stone overwrites bedrock two blocks ahead | The resolver rejects the complete move before changing anything. The earlier gold/stone overwrite is not restricted to movable destinations |
| Push limit | A line of 13 stones allows extension | Java resolves a total movement list with a maximum of 12. The fork does not resolve the line at all |
| Slime and honey | Their neighboring gold block stays behind | Java includes attached side branches and counts them against the shared movement limit; slime and honey do not adhere to one another |
| Plot and height bounds | Stone at x=255 or y=255 vanishes when a piston attempts to push it outside storage | Height/border checks belong in movement resolution. Isolated plot boundaries are a fork-specific requirement: either reject crossing or implement coordinated ownership. Java's ordinary chunk boundaries are not plot borders |
| Replacement during motion | Replacing the base with gold before completion results in the old piston being recreated | Java completes a moving entity at its own position, guarded by a moving-block check. It does not blindly rebuild the neighboring base |
| Head lifetime | Removing a base leaves its head; breaking a stationary head leaves the extended base | Java checks backing type, facing and extension, removes unsupported heads, and removes a matching base when its head is broken |
| Head identity | A North-facing ordinary head in front of an East-facing sticky piston marks that base extended | The fork accepts any `PistonHead` as a recovery shortcut. Java blocks that movement and validates the matching backing piston |
| Moving destinations | The pushed destination remains air with no moving entity; source-head entity stores the payload | Java creates a moving entity for each payload destination with `source=false`, plus a separate source head entity |
| Retraction geometry | The base stays a stationary extended piston; only the head position is moving | Java installs a retracting source entity at the base, storing the unextended base state. A pulled payload has a separate moving entity |
| Tick identity | An old observer tick at the head position completes the replacement moving head after one game tick | Java ordinary ticks retain their expected block type. `ServerLevel.tickBlock` checks that type before dispatch. The fork's queue stores only `BlockPos` and dispatches the current occupant |
| Progress | After one game tick the moving entity still has progress 0; ordinary completion occurs from a fixed scheduled callback | Java advances current and previous progress by 0.5 in moving-entity ticks and completes on an invocation that starts at progress 1. Creation phase affects elapsed game-tick numbering |
| Progress representation | Encoding 0.5 produces 127, decoding to 0.49803922 | The byte field cannot represent the exact half boundary. This is a latent predicate error: current short-pulse code does not yet read the field |
| Observer relocation | A moved powered observer remains powered after eight game ticks, even with its original pulse-off tick queued | Java's observer placement path resets a powered state without a pending observer tick at its new position; the old position's tick cannot follow the block implicitly |
| Waterlogging | Moved waterlogged oak stairs remain waterlogged | Java's ordinary moving-entity completion explicitly clears `WATERLOGGED` on the restored state |
| Support and shape updates | A torch above the original stone remains above the final horizontal piston head | Java shape/support updates remove it. The fork's piston helper only calls redstone updates, while its cube shortcut also treats the head as general support |

All measured Rust cases are independent of the memory-cell schematic. The Java expectations above are derived from the pinned implementation and, for the block classification rows, checked against its runtime state properties. Full phase-by-phase circuit equivalence still needs a live Java trace.

## Why the movement predicate must change

The fork uses `!has_block_entity || !is_cube` to permit extension, then `!has_block_entity && is_cube` to transport a payload. Sticky pulling uses the second expression too. These predicates produce opposite errors: some immovable cubes move, while some movable partial blocks disappear.

The official runtime probe returned:

| State | Reaction | Block entity | Other Java checks |
| --- | --- | --- | --- |
| Stone, iron trapdoor, observer | `NORMAL` | false | Generally movable; resolve surroundings and bounds |
| Bedrock | `NORMAL` | false | Unbreakable hardness rejects it |
| Obsidian, crying obsidian, respawn anchor | `NORMAL` | false | Explicit rejection in piston-base code |
| White glazed terracotta | `PUSH_ONLY` | false | May be pushed, cannot be pulled |
| Chest, oak sign, barrel | `NORMAL` | true | Block-entity rejection prevents movement |
| Comparator | `DESTROY` | true | May be broken by an extending piston; block entities do not universally mean “block extension” |
| Torch, repeater, redstone wire | `DESTROY` | false | Destruction path and ordered notifications |
| Piston head | `BLOCK` | false | Blocks movement; backing/support rules are separate |

This corrects an easy overgeneralization: Java does **not** reject every block entity before considering its reaction. A comparator can be destroyed, while a sign blocks the piston. Neither `is_cube()` nor the fork's `has_block_entity()` is sufficient. In particular, the fork marks piston heads and extended bases as “having a block entity” for its own control purposes, although the Java head runtime reports false.

## Event, tick and notification differences

The exact 1.21.5 source retains the earlier audit's base-state transition, short-pulse event 2 and event power-validation behavior. Thus the delayed extension, three-game-tick lock and missing retract-without-pulling path remain defects against the current target, not merely historical 1.18.2 issues.

There are now two distinct identity requirements:

1. Piston events capture expected base kind, action and direction, and validate the relevant block/power when executed.
2. Ordinary scheduled ticks retain expected component type. Moving-entity advancement additionally needs the identity of the particular entity/transition, so replacement work cannot finalize a different motion.

Only introducing a piston event queue leaves the reproduced old-observer-tick bug intact. The repair plan must cover ordinary interpreted ticks too, including their saved representation and compiler handoffs.

Notification order also differs at source level. Java's default `NeighborUpdater.UPDATE_ORDER` is **West, East, Down, Up, North, South**; the fork's `BlockFace::values()` is **Up, Down, North, South, East, West**. Java additionally separates direct/indirect shape updates, neighbor callbacks, observer updates and movement completion. The fork's `on_piston_state_change` issues a custom head/pushed/base redstone sequence. The support-removal diagnostic establishes a concrete missing behavior; the exact memory-cell effect of the different notification order has not been isolated. Do not fix this by globally changing every face loop.

## Controls and preserved behavior

Four controls pass on the current snapshot:

- One stone extends to the expected settled destination in all six directions.
- A typed barrel blocks extension.
- Power on the facing side alone does not power the piston.
- Quasi-connected power is detected, and an explicit base update activates the piston.

These show working portions of ordinary final-state movement and input detection. They do not establish correct intermediate timing, all quasi-connectivity notifications, or complete barrel/container support. The current protocol port also has a piston/observer compiler eligibility guard; compiler omission is not reported here as a newly reproduced defect.

## Repair priorities

The user selected these three areas as the main repair priorities on **2026-10-04**. Each has a focused issue document:

1. [**Lifecycle**](piston-audit/LIFECYCLE.md): preserve replaced bases, validate head/backing relationships, remove orphan heads and make each transition own its completion work.
2. [**Scheduled ticks**](piston-audit/SCHEDULED_TICKS.md): retain the requesting block type, reject cross-type dispatch and preserve that identity through saves and compiler handoffs.
3. [**Completion updates**](piston-audit/COMPLETION_UPDATES.md): execute placement and structural callbacks, reset moved observers correctly, normalize waterlogging and remove unsupported attachments.

The other discrepancies remain recorded as a secondary backlog. Their ordering should not displace these priorities. Movement representation and event handling may still require changes where they are dependencies of the three selected repairs.

Verify the selected repairs with game/nano/pico traces, save/load in flight, editing and compiler handoffs. Keep the version-matched memory-cell run as a separate reference-validation gate.

Slime/honey remains an explicitly separate milestone if the first repair is scoped to the original supported block set; conservative behavior must still preserve unsupported circuits. No claim of complete vanilla compliance is justified by the existing four passing schematic fixtures.

## Reproduction and limits

Create a source snapshot or separate worktree with the current port changes, generated mappings, `Cargo.lock`, registry inputs and the six top-level `.schem` fixtures. Copy **only those fixtures** from `test_data`; this directory also contains unrelated nested build/workspace artifacts. Do not recursively copy it.

Copy [deep-regressions.rs](piston-audit/deep-regressions.rs) to `crates/core/src/redstone/piston_deep_audit.rs` in that isolated checkout and append:

```rust
#[cfg(test)]
mod piston_deep_audit;
```

Run `cargo test -p mchprs_core --lib --locked --offline piston_deep_audit -- --nocapture`. Expected diagnostic result on the recorded snapshot: **4 pass / 27 fail**. The test file is an audit asset, not enabled in the shared suite, because its failures are intentional. Hashes, test names and measured states are in [deep-evidence.json](piston-audit/deep-evidence.json).

The Java property probe uses the inner server JAR and libraries extracted from the verified bundler, then runs the included source with Java 21. Its obfuscated names are pinned specifically to 1.21.5 and must be regenerated for another version.

Not validated: a running Java memory-cell schematic, exact nano/pico correspondence to Java phases, player/entity collision physics, sounds/drops in a live client, save/restart equivalence, or the full workspace/release suite. The [official 1.21.5 release notes](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5) also describe piston destruction sounds; that presentation behavior is outside these simulation diagnostics. Existing memory-cell expectations remain provisional.
