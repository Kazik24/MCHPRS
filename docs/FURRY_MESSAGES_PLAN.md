# Furry message style plan

Give MCHPRS server-authored feedback a cute furry voice while preserving what
each operation does. Use paws, floof, sniffing, an occasional rawr, and light
emoticons such as `:3`, `^^`, and `>w<`. Keep failures understandable and command
syntax, numerical results, units, coordinates, permissions, and paths exact.

This is a presentation-only proposal. The work consists of locating existing
messages, proposing their wording, and later replacing selected feedback
producers. It adds no commands, item behavior, placement rules, recipients,
ownership, persistence, or authored-text features.

The [message inventory](FURRY_MESSAGE_INVENTORY.md) lists send sites and upstream
text producers. The [wording proposal](FURRY_MESSAGE_WORDING.md) supplies current
and proposed replies. The [RedstoneTools command plan](REDSTONETOOLS_COMMAND_PORT_PLAN.md)
owns that separate command work; this document only specifies its message tone.

## Tone

- Build the voice into the phrasing: fetch clipboard contents, sniff out matches,
  follow tick trails, and pounce to coordinates. State the result or failure
  clearly; avoid a plain message followed by unrelated furry flavor.
- Vary verbs, imagery, sentence shape, and rhythm. Use emoticons occasionally,
  without a default ending or repeated form of address.
- Use one deterministic phrase per notice. Avoid randomness, automatic text
  transformations, and a universal prefix or suffix.
- Keep routine replies short. Useful instructions take priority over jokes.
- Keep storage failures, crash reports, and other technical diagnostics factual.
- Preserve authored player chat, `/say` and `/tellraw` content, sign lines,
  item names, URLs, and debug payloads.
- Preserve configurable permission checks. Do not replace permission-node
  requirements with an invented role requirement.

## Existing message paths

| Source | Role | Style work |
| --- | --- | --- |
| [player.rs](../crates/core/src/player.rs) | Red error, yellow system, light-purple WorldEdit, and raw JSON senders. | Add a typed notice entry point alongside existing transports. |
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

Use a concrete enum with named payloads, such as
`CommandNotice::PermissionDenied`, `CommandNotice::SelectionRequired`,
`CommandNotice::InvalidArgument`, `CommandNotice::SearchCompleted`, and
`CommandNotice::HistoryEnabled`. Store structured values such as coordinates,
counts, requested ticks, and required permissions in the corresponding variants.

Put phrase selection and color assignment in a small `command_feedback` module.
Render one deterministic component per notice and pass it to the existing
sender. Keep error messages red, system messages yellow, and WorldEdit replies
light purple where they already use those colors.

Do not pass an already decorated string into a renderer and infer its category.
Keep external diagnostic text separate from the server-authored wrapper. Shared
raw senders must remain usable for authored components and other existing text.
This style work needs no serializer, interaction-policy, or protocol changes.

## Proposed wording

| Situation | Proposed reply |
| --- | --- |
| Selection required | “One pawprint's missing from your selection. Set both positions with //pos1 and //pos2.” |
| Permission denied | “This command trick is outside your permissions.” |
| Search success | “Sniffed out 12 matches. Page 1/2.” |
| No results | “No matches—the sniff patrol came back empty-pawed >w<” |
| Clipboard empty | “Can't paste from an empty clipboard—fetch a selection with //copy first.” |
| History enabled | “Recording up to 100 game ticks of pawprints. Estimated size: 2 MiB.” |
| Plot claimed | “Plot 2,3 is your den now. Awoo!” |
| Schematic storage failure | “Could not save schematic: permission denied” |

The in-progress RedstoneTools `/container` handler accepts power 0 through 15
and lowercase a through f. Its wording should describe that actual range. This
handler changed during the audit, so check its validation against the inventory
snapshot before implementing replacements. Message style does not change it.

## Implementation sequence

1. Finish reviewing the inventory and wording proposal against the working tree.
2. Add the minimal deterministic notice renderer with explicit colors and values.
3. Convert one small family, such as selection or history feedback, preserving
   state changes and recipients.
4. Convert the remaining routine feedback in command-family batches. Preserve
   technical diagnostics, help syntax, and authored text.
5. Check documentation and help examples for consistency with the final wording.

## Verification

Renderer checks should cover category, color, important values, and the chosen
phrase. Command checks should verify state changes independently of exact
sentences. Confirm one notice per existing feedback event and no change to
authored content, recipient routing, permissions, or failure behavior.

Use existing protocol and packet paths. Retain links, hover labels, and actions
already owned by other features; changing their visible labels must preserve
their actual actions. No new interactive behavior is required for message style.
