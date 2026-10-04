# Basic chat and command-block commands

`/say <message>` broadcasts `[source] message` literally, preserving spaces. `/tellraw <target> <JSON text>` sends supported JSON text without the usual chat prefix. Messages reach players on every loaded plot.

Targets support `@a`, `@s` and player names (case insensitive). Selector options such as `@a[distance=..300]` are deliberately ignored. `@s` resolves to the executing player; a command block has no player entity and therefore selects nobody. Other selector types are rejected with an explanation.

JSON strings, objects, arrays, nested `extra`, named/hex colors, bold, italic, underline, strikethrough and obfuscation are supported. Child components preserve style inheritance and explicit `false` overrides. Unsupported content such as scoreboards, selectors, click/hover actions, keybinds and translations is omitted; a translation's explicit `fallback` text is retained. Malformed JSON produces an error without disconnecting players. Text nesting is bounded at 64 levels.

Player permissions are `commands.say` and `commands.tellraw`, following the server's existing permission configuration. Command-block execution is limited to these two commands; imported unsupported commands keep their original text.

Parser tests and independent clients check formatting, unsupported fields, selectors, literal spacing, named/self recipients, cross-plot broadcasts and malformed input.
