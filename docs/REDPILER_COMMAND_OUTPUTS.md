# Redpiler command-block outputs

Redpiler compiles command blocks as electrical outputs. Impulse blocks activate
on rising power, repeating blocks run every game tick while powered or automatic,
and chains retain their existing direction, conditional checks and 256-block
limit. Commands execute during simulation ticks, including `/adv`, rather than
waiting for a render flush. `--io-only` retains these outputs, and optimization
does not remove or merge distinct command blocks.

Execution uses MROWW's existing `say` and `tellraw` support, selector handling,
command allowlist and output limits. Compiling a command block does not enable
other Minecraft commands. Live block entities keep their power, last execution,
success count and output. Comparator success-count reads, including reads through
one solid block, update as commands execute. Reset transfers pending callbacks
to the interpreter without replaying past output. Editing command blocks already
resets Redpiler before changing their mode or command.

Command blocks no longer prevent automatic compilation by themselves. Binary
graph export currently rejects graphs containing command-block outputs explicitly,
since the exported graph format has no command-block entity or execution support.

Error messages render block coordinates as clickable teleport links. Both
`BlockPos { x: ..., y: ..., z: ... }` and `(x, y, z)` coordinates retain their
original text. Clicking runs `/tp` to the center just above that block; ordinary
teleport permission checks still apply. Redpiler analysis diagnostics use the
same error rendering.
