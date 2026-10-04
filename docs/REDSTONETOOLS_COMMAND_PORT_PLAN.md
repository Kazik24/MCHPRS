# RedstoneTools command adaptation plan

This document records the RedstoneToolsRF comparison and the native Rust port.
The six command families below are implemented. Autowire was removed from scope
at the user's request. The original comparison and implementation sequence are
retained below for context.

The reference is RedstoneToolsRF commit
`3bcb69f3d6018f875f01cf20f3eda81550ff0dba`. Links below pin that revision.
The original MCHPRS comparison was based on commit `507e058`.

## Implemented behavior

- `//find` supports block names matching all states, partial properties such as
  `repeater[facing=north]`, numeric state IDs, comma-separated unions, and `*`.
  Other WorldEdit mask operators are rejected.
- Searches list seven results per page, ordered by `(x, y, z)`. Limits are
  16,777,216 selected blocks, 4,096 results, and 2 MiB of cached sign text.
  Truncation is reported. Invalid queries preserve prior results; empty searches
  replace them. Leaving the plot clears both caches.
- Sign search examines front and back lines separately, retaining parent and
  child literal text and highlighting the first match on each line. Lines are
  limited to 8 KiB; expressions to 1 KiB. Rust regex supports zero-width matches
  but rejects lookaround and backreferences.
- Rstack accepts flexible argument order, signed count/spacing, diagonal
  directions, and `-w`/`-a` for air. Defaults are one copy, spacing two, and look
  direction. Every destination is validated and every undo snapshot prepared
  before writing. Overlap uses one source snapshot, including with air; this
  preserves MCHPRS copying semantics rather than claiming full WorldEdit parity.
  Limits are 4,096 copies and 16,777,216 copied blocks.
- Container and slab commands insert into an empty slot. Omitted slab type
  converts a held slab while preserving its count and unrelated components.
  Special top slabs can place beneath an existing top slab when the lower cell
  is air and inside the plot.
- Containers accept chest/barrel/hopper/furnace, unambiguous prefixes, decimal
  powers 0–15, and lowercase `a`–`f`. `SignalStrength` validates powers before
  item construction. Names, lore, glint, block state and container contents use
  protocol components and survive save/load.
- Chest states, inventories, comparator output, menus, transforms and schematic
  persistence are supported. Menus expose 27 slots per block; adjacent chests
  do not merge into a 54-slot inventory. The appended save enum variant preserves
  existing furnace/barrel/hopper indices.
- `/cursel` switches only the requesting player's sidebar. It starts hidden,
  uses one stable objective, and updates when selection lines change. Hiding it
  restores Redpiler status without clearing search caches.
- `/help tools` and `//help <tool>` show concise usage. Commands and aliases are
  declared to clients, with server completion for masks, pages, slab IDs,
  container names/powers and stacking directions/flags.

Trusted result actions use the `click_event` and `hover_event` forms required by
[Minecraft 1.21.5](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5).
Only generated coordinate/page actions use this path; authored chat retains its
existing filtering policy. Feedback uses deterministic furry wording.

## Command inventory

| Command | Reference behavior | MCHPRS status |
| --- | --- | --- |
| `//rstack`, `//rs` | Stack with explicit spacing, optional selection expansion, and diagonal directions. | Implemented; see compatibility limits above. |
| `//find <mask>` | Search the selection and display matching block locations. `-p <page>` retrieves saved results. | Implemented mask subset and cached pages. |
| `//signsearch <regex>`, `//ss` | Search sign lines and display highlighted matches. `-p <page>` retrieves saved results. | Implemented on both sign sides. |
| `/container <type> <power>` | Give a container filled to produce the requested comparator signal. | Implemented; see compatibility limits above. |
| `/slab [type]` | Give a special top slab, or convert the held slab when no type is supplied. | Implemented with inventory preservation. |
| `/cursel` | Toggle the selection sidebar showing dimensions and volume. | Implemented as an individual sidebar mode. |

The reference registers seven command families; six remain in this port.
`/cursel` is absent from the
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
Trusted teleport and page buttons need explicit component support in this
command port. Their visible labels should follow
[the message style plan](FURRY_MESSAGES_PLAN.md).

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

1. Add deterministic furry command feedback following the
   [message style plan](FURRY_MESSAGES_PLAN.md), and the minimal trusted result
   actions needed by this command port.
2. Add `//find` with a documented mask subset, bounded results, and pagination.
3. Add `//signsearch` and `//ss`, reusing the pagination and tested text extraction.
4. Bring `//rstack` syntax into compatibility while retaining current aliases.
5. Add `/slab` item creation, then its separately tested placement behavior.
6. Improve `/container` input and inventory handling; implement chest support
   as its own feature with persistence checks.
7. Add `/cursel` without disrupting the Redpiler sidebar.

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

Run the native regression tests with `cargo test --workspace --locked`. Run
`py tools/run_redstone_tools_smoke.py` after `cargo build --locked` for independent
protocol decoding, two-player sidebar checks, inventory failure cases, slab
placement, chest menus and process restart. The existing protocol smoke retains
its overlapping rstack undo/redo check. Graphical vanilla-client checks remain
useful for the appearance and interaction of generated chat buttons.
