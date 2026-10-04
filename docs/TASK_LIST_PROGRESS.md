# Current task list

Scope: [`tasks.txt`](tasks.txt), updated 4 October 2026. Earlier work is recorded separately in [`TASK_PROGRESS.md`](TASK_PROGRESS.md).

1. **Complete:** high-TPS static piston rendering and configurable visual send-rate cap. See [`HIGH_TPS_RENDERING.md`](HIGH_TPS_RENDERING.md). Boundary/rate tests, snapshot and simulation equivalence tests, debug build and independent protocol/restart smoke passed.
2. **Complete:** Minecraft 1.21.5 middle-click packets, item selection/swapping, reach validation and Ctrl-middle-click entity data. See [`BLOCK_PICKING.md`](BLOCK_PICKING.md). Unit tests and independent protocol/restart checks passed.
3. **In progress:** basic `/tellraw` and `/say`, selectors and supported text formatting, including redstone command blocks and schematic import/export. The supplied `potados_27072024.schem` contains 425 command blocks (29 supported commands).

Player chat commands are implemented and pass parser tests and independent cross-plot protocol checks. Command-block editing/execution and persistence remain in progress.
