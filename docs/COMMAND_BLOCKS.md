# Basic command blocks

Placed command blocks and Sponge v2/v3 schematic command-block entities retain their commands, names, power/automatic flags, conditional state, output tracking and execution metadata. Creative players can edit them through the Minecraft command-block screen; edits require `commands.commandblock.edit`, the usual plot interaction permission and a nearby target.

The executable subset is `/say` and `/tellraw`, with or without a leading slash. See [`CHAT_COMMANDS.md`](CHAT_COMMANDS.md) for formatting and selectors. Unsupported commands are retained and return zero success with an explanation in `LastOutput` when output tracking is enabled. They do not execute through the unrestricted player-command dispatcher.

Impulse blocks run once after a rising redstone edge, with a one-game-tick delay. Queued work survives short pulses. Repeating blocks execute once per game tick while powered or automatic, including the final queued execution after power disappears. Chain blocks follow facing, require power or automatic mode, and stop after 256 links. Conditional execution checks the previous command block's success; scheduled activations retain the condition observed when queued. Command blocks provide comparator output from their success count.

Success count is **1 for a valid supported dispatch and 0 for failure**, rather than Minecraft's exact recipient count. Command minecarts, other commands, scoreboard evaluation and additional selector types are outside this subset. Plots containing command blocks use the interpreter.

Plot save format is **5**, storing the appended command-block entity variant and per-plot piston-animation preference. Format 4 has an explicit reader; conversion validates and preserves the original in a backup. Formats 0/1/3 remain supported through existing readers, and format 2 remains unsupported. Player saves retain version 3.

The supplied [`potados_27072024.schem`](../test_data/potados_27072024.schem) is Sponge v2, DataVersion 3700, size 187 × 242 × 111. All 425 command blocks round-trip with their commands and metadata intact: 27 `/tellraw`, two `say`, 392 `/scoreboard`, one `/summon` and three `/setblock`. All 29 supported commands execute in isolated activation checks. The 396 remaining commands are preserved and inactive, so this does not implement the schematic's scoreboard-based features.

The fixture also exposed ambiguous sign-text conversion. The importer uses source DataVersion to convert legacy JSON fields before reading modern NBT components, retaining numeric text and literal quotes through export/reload.

Validation includes activation, repeated power, short pulses, repeating shutdown, conditional chains, loop limits, replaced blocks, automatic restart, saved metadata, v2/v3 paste/activation and the complete supplied fixture. Independent clients check placement/editing, global output, save/load/paste, undo/redo, and process restart.
