# RedstoneTools command adaptation plan

This document compares the commands in Lord225's RedstoneToolsRF with MCHPRS and
proposes an incremental port. Two command families already exist here with
different behavior. The first implementation should reuse those operations and
add the missing commands without importing Bukkit or WorldEdit abstractions.
This is analysis and a proposed plan; no commands have been implemented by this
document.

The reference is RedstoneToolsRF commit
`3bcb69f3d6018f875f01cf20f3eda81550ff0dba`. Links below pin that revision.
The MCHPRS comparison uses the current workspace, based on commit `507e058`.

## Command inventory

| Command | Reference behavior | MCHPRS status |
| --- | --- | --- |
| `//rstack`, `//rs` | Stack with explicit spacing, optional selection expansion, and diagonal directions. | Partial implementation. |
| `//find <mask>` | Search the selection and display matching block locations. `-p <page>` retrieves saved results. | Missing; `//count` only reports a count. |
| `//signsearch <regex>`, `//ss` | Search sign lines and display highlighted matches. `-p <page>` retrieves saved results. | Missing. |
| `/autowire`, `/aw` | Toggle automatic wire placement above newly placed blocks. | Missing. |
| `/container <type> <power>` | Give a container filled to produce the requested comparator signal. | Partial implementation. |
| `/slab [type]` | Give a special top slab, or convert the held slab when no type is supplied. | Missing command; some slab placement support exists. |
| `/cursel` | Toggle the selection sidebar showing dimensions and volume. | Missing; WorldEdit CUI messages already exist. |

The source registers these seven command families. `/cursel` is absent from the
README's command list. Its implementation is in
[WorldEditHelper.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/WorldEditHelper.kt).

## Existing commands that need compatibility work

### Redstone stacking

The reference accepts direction and flags in flexible positions, with the first
two numbers interpreted as count and spacing. Omitted values mean count 1,
spacing 2, and look direction. Negative count reverses the spacing direction;
spacing can also be negative. `-e` expands the selection and `-w` includes air.
Direction handling delegates to WorldEdit and adds vertical diagonal components.
[RStack.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/RStack.kt)

MCHPRS currently expects count, spacing, then direction. Numbers are unsigned,
`-a` includes air, and direction handling is more limited. Basic overlapping
copies, ignoring air by default, undo, and selection expansion already exist in
[execute.rs](../crates/core/src/plot/worldedit/execute.rs) and
[worldedit/mod.rs](../crates/core/src/plot/worldedit/mod.rs).

Proposed change: introduce a concrete `RStackRequest` with validated count,
signed spacing, direction, air policy, and selection policy. Preserve existing
invocations and `-a` as a compatibility alias while adding `-w` and flexible
argument order. Reject unknown directions and overflowing coordinates before
editing. Prepare all destination bounds and undo snapshots before placing copies.

The reference README describes an overlap restriction for `-w`, but the command
implementation contains no explicit overlap check. Resolve that behavior with a
fixture before claiming exact parity. Also test source copying when successive
destinations overlap; our implementation captures a clipboard once, whereas the
reference delegates repeated copying to WorldEdit.

### Comparator containers

The reference accepts chest, barrel, hopper, and furnace. Power accepts decimal
0 through 15 and lowercase hexadecimal `a` through `f`. Container names accept
prefixes. Items have a power label, lore, and a hidden enchantment.
[Container.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/Container.kt),
[input types](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/RedstoneTools.kt)

MCHPRS supports barrel, hopper, and furnace. The command accepts decimal 1
through 15 and replaces the selected hotbar slot. The item constructor already
handles zero, but the command rejects it. The reference adds items to inventory
instead. These differences are visible in
[commands.rs](../crates/core/src/plot/commands.rs),
[items.rs](../crates/blocks/src/items.rs), and
[block_entities.rs](../crates/blocks/src/block_entities.rs).

Proposed change: use a validated `SignalStrength` newtype for 0 through 15;
accept only explicit, unambiguous aliases at the input boundary. Prepare the
generated item and an inventory insertion before mutating either. Test every
signal strength against the actual comparator calculation for each container.

Chest support is a separate change involving block entities, menus, placement,
and save compatibility. Do not add only a command token. Preserve existing enum
representations and provide save mappings wherever representations change.

## Missing command behavior

### Block and sign search

`//find` saves results per player, displays the first page immediately, and
clears old results when nothing matches. Both searches display seven results per
page, with clickable teleport locations and page completion.
[Find.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/Find.kt),
[pagination](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/RedstoneTools.kt)

Sign search uses RE2J, examines each legacy `Text1` through `Text4` line
individually, and highlights the first regex match on each matching line. It
does not support matches spanning multiple lines.
[SignSearch.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/SignSearch.kt)

Proposed implementation:

- Store bounded, transient results on the player, recording plot coordinates
  and a stable ordering of block coordinates.
- Keep block and sign results distinct with a `SearchKind` enum. Share only the
  result listing and pagination that the commands actually have in common.
- Validate the query before replacing the previous successful result set.
  Reject invalid page numbers and clear caches on disconnect or invalidation.
- Implement an explicit block-mask subset first. MCHPRS currently treats masks
  as patterns and compares complete block-state IDs; that is not the full
  WorldEdit mask language. A block-type search must match different orientations
  and other states of that block when no property constraint is specified.
- Use the existing Rust regex dependency, documenting any syntax differences
  from RE2J. Bound expression and result sizes. Preserve empty and zero-width
  match behavior in tests.
- Read the modern front and back sign rows from `SignBlockEntity`; display side
  and line number. Searching both sides is a proposed extension beyond the
  reference's legacy four-line representation.
- Flatten literal text with its child components, retaining parent text. The
  reference's helper drops parent content when children exist; do not reproduce
  that bug. Highlight using the regex match's actual byte range.
  [Util.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/Util.kt)

Search commands are read-only and must not reset Redpiler or create undo entries.
Pagination should operate on cached results without requiring a new selection.
Trusted teleport and page buttons need the message support described in
[the message plan](FURRY_MESSAGES_PLAN.md).

### Automatic wire placement

The reference enables autowire per player, clears the toggle on disconnect,
requires creative mode, skips nonsolid and gravity blocks, and places wire only
when the cell above is air. It calls a synthetic placement event before adding
wire.
[Autowire.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/Autowire.kt)

Proposed implementation: keep a transient player toggle and apply it only after
a successful ordinary placement. Prepare and validate the extra cell using the
same plot ownership, bounds, compiler, and placement rules. Use normal wire
neighbor updates. Do not derive success solely from
`interaction::use_item_on_block`'s cancellation boolean: an interaction can
succeed without placing a block. Introduce a small explicit placement outcome
where the actual placed position is known.

Proposed boundary behavior: if the extra wire cannot be placed, keep the player's
valid original placement and omit the optional wire. Verify this independently
of the toggle feedback. This is a convenience feature, not a second command
dispatcher or an event framework.

### Top slab items

With an argument, the reference gives the requested slab with top placement
properties. Without one, it replaces a held slab with the special item or gives
a smooth-stone slab. A separate listener attempts placement beneath an existing
top slab when the replacement would otherwise remain a top slab.
[Slab.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/Slab.kt)

Proposed implementation: validate slab IDs through generated registry data,
encode the top property using the item component machinery, and test placement
for modeled and generic slab states. MCHPRS reads `BlockStateTag` during
placement, but that alone does not prove correct slab merging or placement
beneath existing slabs. Treat the special placement behavior as a separate
tested step. Define inventory handling explicitly instead of silently discarding
an occupied slot or a stack's remaining items.

### Selection display

The reference shows inclusive dimensions and volume, colors larger selections
differently, and refreshes the sidebar every second. `/cursel` toggles visibility.
[WorldEditHelper.kt](https://github.com/Lord225/RedstoneToolsRF/blob/3bcb69f3d6018f875f01cf20f3eda81550ff0dba/src/main/kotlin/WorldEditHelper.kt)

MCHPRS already owns the Redpiler sidebar and sends WorldEdit CUI selection
updates. Proposed implementation: add a player-specific selection section or
explicit sidebar mode, with stable objective identifiers and updates triggered
by changed selection or visibility. Do not replace the scoreboard periodically.
The reference constructs the helper twice, scheduling and registering both
instances; use one owner here.

## Implementation sequence

1. Add deterministic furry command feedback and the minimal trusted result
   actions from the [message plan](FURRY_MESSAGES_PLAN.md).
2. Add `//find` with a documented mask subset, bounded results, and pagination.
3. Add `//signsearch` and `//ss`, reusing the pagination and tested text extraction.
4. Bring `//rstack` syntax into compatibility while retaining current aliases.
5. Add `/autowire` through an explicit successful-placement hook.
6. Add `/slab` item creation, then its separately tested placement behavior.
7. Improve `/container` input and inventory handling; implement chest support
   as its own feature with persistence checks.
8. Add `/cursel` without disrupting the Redpiler sidebar.

Keep upstream `redstonetools.*` permission nodes for these command families.
Apply existing plot access checks as well. Extend help, aliases, tab completion,
and command declarations with each feature rather than deferring them.

## Verification

Each implementation step needs tests for valid input, unknown tokens, omitted
defaults, permission rejection, plot bounds, and unchanged state after failure.
Search tests should check ordering, pagination, both sign sides, and actual
highlight ranges. Editing tests should verify undo and block entities with
overlapping destinations. Placement tests should cover compiler handoff and
neighbor updates. Item tests should round-trip protocol components and inventory
contents. The selection display requires a two-client test because selections
are individual while the current Redpiler sidebar is shared within a plot.

No Gradle plugin dependencies are needed for the native implementation. Exact
WorldEdit copying semantics, full mask parity, and live-client interactive text
behavior remain verification work for their corresponding implementation steps.
