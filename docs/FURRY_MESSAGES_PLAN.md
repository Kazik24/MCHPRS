# Furry message style plan

Give MCHPRS server-authored feedback a cute furry voice while preserving what
each operation does. Use paws, floof, sniffing, an occasional rawr or woof, and light
emoticons such as `:3`, `^^`, and `>w<`. Keep failures understandable and command
syntax, numerical results, units, coordinates, permissions, and paths exact.

The style is implemented at existing feedback producers through the shared
[message catalog](../crates/core/src/messages.rs). Command behavior, permissions,
recipients, colors, and packet delivery retain their existing paths.

The [message inventory](FURRY_MESSAGE_INVENTORY.md) lists send sites and upstream
text producers. The [wording proposal](FURRY_MESSAGE_WORDING.md) supplies current
and approved replacements. The inventory's source locations are from the audit
snapshot; the catalog is the current source of wording. The
[RedstoneTools command plan](REDSTONETOOLS_COMMAND_PORT_PLAN.md)
owns that separate command work; this document only specifies its message tone.

## Tone

- Build the voice into the phrasing: fetch clipboard contents, sniff out matches,
  follow tick trails, and pounce to coordinates. State the result or failure
  clearly; avoid a plain message followed by unrelated furry flavor.
- Vary verbs, imagery, sentence shape, and rhythm. Use emoticons occasionally,
  without a default ending or repeated form of address.
- Use sympathetic addresses such as “you poor pup” sparingly for setbacks,
  and “woof” for an occasional successful fetch or command-block update.
- Use one deterministic phrase per notice. Avoid randomness, automatic text
  transformations, and a universal prefix or suffix.
- Keep routine replies short. Useful instructions take priority over jokes.
- Name the actual operation and condition. Keep terms such as tick history,
  rewind, paused, world send rate, and automatic compilation readable. A player
  should not need to translate a metaphor to understand a failure or next step.
- Keep storage failures, crash reports, and other technical diagnostics factual.
- Preserve authored player chat, `/say` and `/tellraw` content, sign lines,
  item names, URLs, and debug payloads.
- Preserve configurable permission checks. Do not replace permission-node
  requirements with an invented role requirement.

## Existing message paths

| Source | Role | Style work |
| --- | --- | --- |
| [player.rs](../crates/core/src/player.rs) | Red error, yellow system, light-purple WorldEdit, and raw JSON senders. | Use catalog entries at server-authored producers; retain shared transports. |
| [plot/commands.rs](../crates/core/src/plot/commands.rs) | Plot, simulation, teleport, and general command replies. | Convert existing literal feedback in small command families. |
| [plot/worldedit/mod.rs](../crates/core/src/plot/worldedit/mod.rs) and [execute.rs](../crates/core/src/plot/worldedit/execute.rs) | Selection checks, argument failures, operation results, and help. | Preserve syntax and counts while changing notice wording. |
| [plot/history.rs](../crates/core/src/plot/history.rs) | History status, validation, rewind, and recording failures. | Include returned messages and the direct recording-failure packet. |
| [server.rs](../crates/core/src/server.rs) | Server-thread teleport and whitelist feedback, login rejection. | Convert routine feedback; keep technical rejection reasons clear. |
| [plot/packet_handlers.rs](../crates/core/src/plot/packet_handlers.rs), [containers.rs](../crates/core/src/plot/containers.rs), and [interaction.rs](../crates/core/src/interaction.rs) | Interaction failures, permissions, command-block updates, debug reports. | Style server notices; preserve debug values and authored data. |
| [plot/mod.rs](../crates/core/src/plot/mod.rs) | Plot entry, claims, compiler notices, broadcasts, and chat delivery. | Change notice producers while retaining recipients. |
| [plot/redstone_tools](../crates/core/src/plot/redstone_tools) | New tool notices, search replies, and selection labels in the working tree. | Apply the same deterministic tone to existing feedback. |
| [chat_commands.rs](../crates/core/src/chat_commands.rs) | `/say` and `/tellraw` parsing and component sanitation. | Style parser feedback only; retain parsing and sanitation behavior. |

Raw chat and system senders also deliver authored content. A global rewrite in
those senders would cross the intended boundary. Phrase ownership belongs in
the notice producer, above serialization and packet delivery.

## Feedback model

The private `messages` module owns fixed `&str` constants and small formatting
functions with named placeholders. One `catalog!` declaration generates both;
Rust checks literal templates and interpolation during compilation. There is
no runtime lookup, replacement pass, random phrase selection, or new dependency.

```rust
player.send_error_message(messages::SELECTION_REQUIRED);
player.send_worldedit_message(&messages::selection_first(pos.x, pos.y, pos.z));
```

Edit wording in the catalog and call the corresponding entry where the notice
originates. Keep colors at existing send sites: red for errors, yellow for
system messages, and light purple for WorldEdit and tool results. Help pages
and WorldEdit help descriptions use the same catalog.

Formatting functions return a `String`; fixed phrases remain usable in const
contexts. Dynamic formatting happens when values become available. Coordinate,
duration, count, permission, and diagnostic values pass directly to Rust's
formatting machinery, so braces in an inserted name or diagnostic are literal.

External diagnostics retain their factual details inside their server-authored
wrappers. Tool errors are sent once without a second decoration. Authored chat
continues through the existing raw senders.

## Proposed wording

| Situation | Proposed reply |
| --- | --- |
| Selection required | “Select a region with //pos1 and //pos2 before putting these paws to work.” |
| Permission denied | “This command trick is outside your permissions.” |
| Search success | “Sniffed out 12 matches. Page 1/2.” |
| No results | “You poor pup, the sniff patrol found no matches >w<” |
| Inventory full | “You poor overpacked pup—your inventory is full. Free a slot for the next fetch.” |
| Item given | “Woof, your item's fetched!” |
| Clipboard empty | “Can't paste from an empty clipboard—fetch a selection with //copy first.” |
| History enabled | “Recording up to 100 game ticks of pawprints. Estimated size: 2 MiB.” |
| Plot claimed | “Plot 2,3 is your den now. Awoo!” |
| Schematic storage failure | “Could not save schematic: permission denied” |

The RedstoneTools `/container` handler accepts power 0 through 15 and lowercase
a through f. Its catalog reply describes that actual range at the command
boundary; the blocks crate's general parser keeps its own factual diagnostic.

## Coverage boundary

The catalog covers existing command, plot, WorldEdit, history, tool, server,
selection, parser-feedback, and help wording, including the direct history
failure packet. Low-level library/storage reasons, generated item names and lore,
debug dumps, administrator MOTD, command/action syntax, and Minecraft translation
keys remain owned by their existing producers. Retired entries in the inventory
are historical examples and do not introduce commands or behavior.

## Verification

The readability review covered all 232 fixed entries and 71 formatting entries,
including help pages and preserved factual diagnostics. Simulation errors now
name Redpiler being active and explain `/redpiler reset`; rewind success says
the plot was paused. History status retains its factual labels. Previously
rejected phrases were revised while retaining their review notes.

Catalog checks cover colors, system-packet position, interpolation of values,
and preserved diagnostic payloads. Command checks verify state changes independently of exact
sentences. Confirm one notice per existing feedback event and no change to
authored content, recipient routing, permissions, or failure behavior. Authored
text that matches a notice is explicitly checked through `/say` and `/tellraw`.

Use existing protocol and packet paths. Retain links, hover labels, and actions
already owned by other features; changing their visible labels must preserve
their actual actions. No new interactive behavior is required for message style.
