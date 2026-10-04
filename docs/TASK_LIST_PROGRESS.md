# Current task list

Scope: [`tasks.txt`](tasks.txt), updated 4 October 2026. Earlier work is recorded separately in [`TASK_PROGRESS.md`](TASK_PROGRESS.md).

1. **Complete:** high-TPS static piston rendering and configurable visual send-rate cap. `/piston_anim [auto|on|off]` and its `/bisdon_anim` alias override automatic rendering per plot; the preference survives restart. See [`HIGH_TPS_RENDERING.md`](HIGH_TPS_RENDERING.md). Boundary/rate tests, snapshot and simulation equivalence tests, and independent protocol/restart checks passed.
2. **Complete:** Minecraft 1.21.5 middle-click packets, item selection/swapping, reach validation and Ctrl-middle-click entity data. See [`BLOCK_PICKING.md`](BLOCK_PICKING.md). Unit tests and independent protocol/restart checks passed.
3. **Complete:** basic `/tellraw` and `/say`, selectors and supported text formatting, including redstone command-block editing, execution and schematic import/export. See [`CHAT_COMMANDS.md`](CHAT_COMMANDS.md) and [`COMMAND_BLOCKS.md`](COMMAND_BLOCKS.md). The supplied `potados_27072024.schem` retains all 425 command blocks through export/reload; all 29 supported commands execute. The other 396 commands remain preserved and inactive.

Validation: **116 unit tests passed**, with no failures or ignored tests. Debug and release builds, formatting and diff checks passed. Independent Minecraft 1.21.5 clients verified picking, cross-plot chat, animation overrides, command-block placement/editing/activation, WorldEdit save/load/paste and undo/redo. Reconnect and process restart retained command text, item components, piston state and animation preferences. A separate load check confirmed unsupported plot saves remain intact.

Plot saves now use format 5, with a frozen format-4 migration reader that validates and backs up the original. Player save format remains 3. The command subset deliberately excludes scoreboard execution and exact recipient-count success values; graphical vanilla-client acceptance remains the operational follow-up recorded in [`PORTING_1_21_5.md`](PORTING_1_21_5.md).
