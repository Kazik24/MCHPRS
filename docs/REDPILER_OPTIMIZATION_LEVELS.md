# Redpiler optimization levels

`-O0` through `-O6` remain accepted. `-O`, `-o`, and `--optimize` select O6;
no optimization flag selects O0. Higher levels include the lower-level graph
rewrites and execution plans.

| Tier | Active additions |
| --- | --- |
| O0 | Graph preparation and physical Graph execution. |
| O1 | Guard/update cleanup, inert callback pruning, callback indexes, static dust paths. |
| O2 | Compiled signal queries, wire specialization, fixed classic execution and local classic nets. |
| O3 | Wire shape and capture plans. |
| O4 | No additional production pass. |
| O5 | Delayed-update coalescing. |
| O6 | No additional production pass. |

The previous instant-region, transport-kernel, stable-logic, and settled-device
compiler paths have been removed. Execution uses the physical Graph runtime,
including its piston actions, callbacks, and scheduled ticks. `--stable-instant-inputs`
is no longer a compiler option.

Historical measurements and pass inventories remain in the [archived optimization
guide](../scraps/docs/research-2026-10-21/REDPILER_OPTIMIZATION_LEVELS_SNAPSHOT.md).
They describe older revisions and are not current behavior.
