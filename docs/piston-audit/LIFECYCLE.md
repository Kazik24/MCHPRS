Current status, **2026-10-04**: the reported repairs are implemented and covered by normal regressions. See [implementation](../PISTON_TIMING_IMPLEMENTATION.md) and [review](../PISTON_REVIEW.md). The findings below preserve the original audit snapshot; statements that no fix is applied refer to that snapshot.

# Piston lifecycle: replaced bases, orphan heads and invalid recovery

Documented **2026-10-04**. **Primary repair priority**, as selected by the user. Status: reproduced in Rust; compared with the exact Java 1.21.5 source; no fix applied.

An unfinished piston movement can overwrite a newer block at the base. Ordinary removal also leaves inconsistent head/base pairs, and the recovery branches accept heads that belong to a different piston. These are lifecycle defects: the implementation does not establish which transition owns the blocks it changes.

Related issues: [scheduled tick identity](SCHEDULED_TICKS.md) and [completion callbacks](COMPLETION_UPDATES.md). The [deep audit](../PISTON_DEEP_AUDIT.md) records the inspected snapshot, hashes and verification limits.

## Reproduced behavior

The diagnostic setup uses an empty plot. Let the East-facing sticky base be **B = (40,30,40)**, its head position **H = (41,30,40)** and pushed destination **D = (42,30,40)**. Stone starts at H; a redstone block below B supplies power. Execute the extension request with one pico operation. `settle()` then advances eight interpreted game ticks.

| Trigger | Current result | Required behavior |
| --- | --- | --- |
| Replace B with gold after extension starts, before completion | Completion writes an extended sticky piston over the gold | Preserve the replacement; old motion must not recreate its base |
| Let extension settle, then destroy B using the ordinary interaction path | H remains a stationary piston head | Remove the unsupported head through the appropriate survival/shape update |
| Let extension settle, then destroy H using the ordinary interaction path | B remains an extended piston, with air at H | A broken head must remove its matching backing base under the reference removal rules |
| Put a North-facing ordinary head at H before powering an East-facing sticky base | B becomes extended despite the mismatched head | Do not adopt that head or fabricate a successful extension |
| Start ordinary retraction after a settled extension | B remains a stationary extended piston; it has no moving entity | Java creates the retracting source at B, storing the unextended base state |

Measured replacement trace:

```text
After extension request: B = unextended sticky piston; H = moving piston
Edit before completion:  B = gold block
After completion:       B = extended sticky piston
```

The gold replacement reproducer deliberately uses the storage API without neighbor callbacks. It isolates the completion routine's unconditional write. The two removal cases use `interaction::destroy`, so they also exercise the ordinary removal/update path. Neither is a captured Java world trace.

## Root causes in this fork

Relevant source:

- [piston.rs](../../crates/core/src/redstone/piston.rs): `moving_piston_tick`, `schedule_extend`, `schedule_retract`.
- [redstone/mod.rs](../../crates/core/src/redstone/mod.rs): `update` handling `PistonHead`.
- [interaction.rs](../../crates/core/src/interaction.rs): `destroy`, `is_valid_position`, `change`.
- [block_entities.rs](../../crates/blocks/src/block_entities.rs): `MovingPistonEntity`.

`moving_piston_tick` computes a base by stepping backward from the moving head. Both its extending and retracting branches then write a reconstructed piston into that position. It does not first establish that the base still exists, has matching properties or belongs to that movement. Thus the callback can overwrite a newer ordinary block or newer piston transition.

The moving entity has facing, extending, progress, source and carried state, but no identifier tying deferred work to a particular movement. A missing entity produces a fabricated default entity, and the routine continues making writes. The replacement test proves the unchecked base write; missing-entity corruption and replacement by a newer motion are additional cases that require regression coverage before implementing recovery.

The `PistonHead` redstone update merely finds a piston behind the head and updates its power decision. It does not check sticky type, matching facing or extension, and it does not remove an unsupported head. `is_valid_position` has no piston-head survival branch, so ordinary structural updates do not repair this omission.

`schedule_extend` accepts any `PistonHead` immediately in front and sets the base extended. `schedule_retract` accepts any head there too; when it finds air instead, it can manufacture a head and return. These recovery paths use geometric adjacency as proof of ownership. A recovery write is not justified by adjacency alone.

## Java 1.21.5 comparison

The [reference manifest](reference-manifest.json) pins the server and mappings. Inspected classes are `PistonMovingBlockEntity` (`ebl`), `PistonHeadBlock` (`ebj`) and `PistonBaseBlock` (`ebi`).

Normal moving-entity completion restores the entity's stored block state **at the entity's own position** and checks that the position still contains a moving piston. The tick belongs to the moving entity itself. It does not compute an adjacent base and overwrite it as a completion side effect.

The head's backing predicate checks the expected normal/sticky piston type, `extended=true` and equal facing. `canSurvive` also permits a matching-facing moving piston behind the head during the relevant transition. Preserve that transition allowance; rejecting every moving backing block would introduce another discrepancy.

The head's `updateShape` removes it when its backing-side change makes it unsupported. `playerWillDestroy` includes the creative-player path, and `affectNeighborsAfterRemoval` handles removal effects on a matching backing base. These callbacks distinguish supported motion, ordinary breakage and invalid head/base pairs.

Retraction creates a source moving entity at the base. The state it carries is an unextended piston, while a pulled payload uses a separate moving entity at its own destination. This ownership model is needed to make interruption and completion coherent. Full slime/honey or long-chain support is not a prerequisite to document or fix the reproduced base/head lifecycle cases.

## Repair contract

1. Give each active movement an explicit identity, or an equivalent ownership mechanism. Deferred completion must refer to the entity/transition that scheduled it.
2. Finalize the carried state at the entity's position. A stale task must not recreate a removed base, overwrite a replacement or finalize a different motion.
3. Represent source heads, retracting bases and payloads separately where the Java reference does. Keep source meaning and stored states consistent.
4. Add head survival and backing validation to the structural update path. Compare kind, facing and extension, with the reference's moving-backing allowance.
5. Apply head/base removal callbacks for ordinary interactions. Batch editing must invalidate obsolete work and validate affected relationships without blindly deleting newly pasted replacement blocks.
6. Replace recovery-by-fabrication with explicit validation. A missing moving entity must not be treated as a valid air-payload retraction.
7. Keep restart, WorldEdit and compiler handoffs consistent with transition ownership. Save enough identity/state to prevent stale work from acquiring a new owner after reload.

Checking only `Block::MovingPiston` cannot distinguish two different motions at the same position. Likewise, checking only that a piston exists behind the head cannot prove it is the original owner.

## Acceptance cases

The following existing diagnostics all failed on the recorded snapshot:

| Test in [deep-regressions.rs](deep-regressions.rs) | What must pass |
| --- | --- |
| `completion_cannot_recreate_replaced_base` | Gold at B survives old completion |
| `removing_base_removes_stationary_head` | Removing B leaves air at H |
| `breaking_stationary_head_removes_base` | Ordinary breaking of H removes its matching B |
| `mismatched_head_does_not_mark_base_extended` | An unrelated head does not mark B extended |
| `normal_retraction_uses_moving_source_at_base` | Retraction has the correct source entity at B |

Add coverage for replacement by another facing/type of piston, a second motion at the same position, removal during extension/retraction, repeated completion, missing entity, all six directions and save/load between request and completion. These are proposed checks, not additional measured failures.

A lifecycle fix is accepted when it preserves replacements, removes only the relationships the reference requires, executes completion once for the correct owner and leaves ordinary piston operation intact. Compare block states and entities, not only the final `extended` flag.

## Reproduction and evidence limits

Use the isolated-checkout setup in [the deep audit](../PISTON_DEEP_AUDIT.md#reproduction-and-limits), register `piston_deep_audit`, then run:

```powershell
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::completion_cannot_recreate_replaced_base -- --nocapture
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::removing_base_removes_stationary_head -- --nocapture
cargo test -p mchprs_core --lib --locked --offline piston_deep_audit::breaking_stationary_head_removes_base -- --nocapture
```

Measured states are preserved in [deep-evidence.json](deep-evidence.json). This documentation adds no test runs or simulation changes. Live Java removal/interruption traces, forced-edit semantics and restart behavior still require verification during the repair.
