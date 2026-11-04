# MROWW

Based on this project: [MCHPRS](https://github.com/MCHPR/MCHPRS).

Minecraft Redstone o Wysokiej Wydajności. By kazik24 and lord225.

A creative redstone server for Minecraft Java 1.21.5. Each 512x512 plot runs on its own thread. There is a physical redstone/piston interpreter and a rewritten Redpiler with an optional LLVM backend, plus tools for building and debugging circuits.

This is for computational redstone. It implements a subset of Minecraft mechanics.

## Build and run

Install Rust, then run from the repo directory:

```sh
cargo build --release --locked
./target/release/mroww
```

On Windows, run `target/release/mroww.exe`. Minecraft registry data is included.

Settings are in `Config.toml`. The checked-in config is for our proxy setup. For standalone use, remove `[velocity]`, set `redbox = false` and `native_chat = true`. `MCHPRS_CONFIG` selects another config file. Missing settings get their defaults automatically.

## Features and commands

Use `/help <topic>` in game, `//help <command>` for WorldEdit arguments, and Tab completion. Commands depend on your permissions and plot access.

Plots and movement:

- `/p auto` claims a plot; `/p claim` claims the one you're in. `/p info` shows ownership.
- `/p home`, `/p visit <player> [number]`, `/p tp <x> <z>` and `/p middle` get you around. Plot coordinates use X/Z.
- `/p add <nick>` and `/p remove <nick>` manage members. `/p lock` and `/p unlock` control leaving the plot.
- Neighbor plots appear as read-only snapshots. Enter a plot to use its simulation.
- `/tp <player>` or `/tp <x> <y> <z>`, `/setwarp <name>` and `/warp [name]` handle standalone travel and shared warps. With RedBox enabled, the proxy handles travel commands.
- `/speed <0-10>` changes flying speed. `/gm creative|adventure|spectator` changes mode. Right-click a compass to jump through geometry to a safe spot.
- `/small` makes you half-size to fit inside builds. `/small wolf|fox|cat|ocelot|baby` picks the animal other players see; baby is smaller again. `/small off` restores normal size when there is room. Your choice is saved.

Simulation:

- `/tps 20` runs normally, `/tps 0` pauses, `/tps unlimited` runs as fast as possible. `/tps timings` shows timing data.
- Pause, then `/adv 1` advances one game tick. `/adv nano 1` and `/adv pico 1` step interpreter operations inside a tick. Two game ticks make one redstone tick.
- The interpreter handles supported dust, torches, repeaters, comparators, containers, lamps, copper bulbs, observers, BUD updates, pistons, moving blocks and instant piston circuits. Exact behavior is in the [redstone](docs/REDSTONE_MODEL.md) and [piston](docs/PISTON_MODEL.md) models.
- `/visual pistons auto|on|off` controls piston animation. `/visual rate <0-1000>` changes client update frequency. `/visual displayonly on|off` limits updates to lamps. Rendering settings are separate from simulation speed.
- `/rhistory on 100` records 100 interpreter ticks. `/rev 10` or `/rhistory rewind 10` goes back ten ticks. `/rhistory status` shows memory use; `/rhistory off` clears it. Rewind clears WorldEdit undo/redo. History clears on compilation, restart or an empty plot.
- `/say <text>` and `/tellraw @a {"text":"hello"}` also work in command blocks. Messages stay on the executing plot. Other saved command-block commands do not run.

Building tools:

- `//wand`, or `//pos1` and `//pos2`, selects an area. `/p select` selects the plot. `/cursel` toggles selection information; `//desel` clears it.
- WorldEdit includes `//set`, `//replace`, `//copy`, `//cut`, `//paste`, `//stack`, `//move`, `//rotate`, `//flip`, `//expand`, `//contract`, `//shift`, `//count`, `//undo` and `//redo`. Use `//help <command>` for flags and block patterns. `//up`, `//ascend` and `//descend` help with movement.
- `//update` recalculates the selection; `//update -p` updates the plot. `//invalidatecaches` clears interpreter caches.
- `//find <block>` finds blocks in the selection. `//ss <regex>` searches signs. Add `-p <page>` for more results. Search and compiler diagnostics include position links; `/hl <x> <y> <z>` highlights a position.
- `//rs [direction] [count] [spacing] [-e] [-w]` repeats a build. `-e` expands the selection and `-w` includes air. `/autostack [direction] [count] [spacing] [-e]` repeats placements/removals as you build; `/autostack off` stops it.
- `/container <chest|barrel|hopper|furnace> <0-15>` gives a container with the requested comparator output. `//replacecontainer <from> <to>` changes container types in a selection.
- `//load name.schem`, then `//paste`, imports a schematic. To export: select, `//copy`, then `//save name.schem`. Saved names become `Nick@name.schem`. Use letters, digits, `_` and `-`. Legacy `.schematic` files are supported.
- `/wire` gives a wire pen. Hold a carrot on a stick, right-click a support or dust, aim, then right-click to build the preview. It places supports and dust together, copying the starting support's passive material/color. F cycles horizontal, two vertical planes and free aim; sneak + F changes the bend. `/wire free` and `/wire plane` select aiming directly. Sneak + right-click cancels the route; `/wire off` disables the pen. `//undo` undoes a segment. It checks unsafe interactions but does not add repeaters or check signal range.

Plot Git saves versions, branches and block diffs. `/rv` is an alias for `/git`.

```text
/git commit working adder
/git branch experiment
/git checkout experiment
/git status
/git diff main
/git log
```

`/git checkout <branch|commit>` switches versions; `/git restore <ref>` restores a build. `/git rebase <branch>` reapplies changes onto another branch. `/git show <ref>` and `/git search <text>` find commits. `/git inspect` checks the block you're looking at; `/git diff inspect <x> <y> <z>` explains a difference. `/git recoveries` lists recovery points; `/git recover <id> <new-branch>` restores one. Checkout/restore replace build contents, so commit work you want to keep first. `/help git` has the full arguments.

## Waveform viewer

Enable `waveform_enabled = true` in the config. Capture uses the interpreter: disable automatic compilation with `/toggleautorp` if enabled, then `/rp reset` before recording.

```text
/waveform add clock
/waveform add output
/waveform on 100
/waveform web
```

Look at each block when adding it, or append its X/Y/Z. `/waveform addbulk data 8` lets you left-click eight bits, low to high. `/waveform list` and `/waveform remove <name>` manage probes. `/waveform clear` clears samples but keeps probes and recording; `/waveform reset confirm` removes everything. On/off also discard recorded history.

The browser has live traces, cursors, groups/buses, binary/decimal/hex and signed values, clock-edge sampling, numeric bus plots, a movable zero tick, command output, saved layouts and session files. Select rows and press G to group them, then right-click for bus/clock options. Export VCD or WaveDrom for other tools. Captures and recorded command output are public read-only data while the plot is loaded.

Run the frontend separately with Node 22.22 or newer:

```sh
cd waveform-viewer
npm ci
npm run dev
```

Open `http://localhost:8443` and load your plot coordinates. [Viewer setup](waveform-viewer/README.md) covers Docker, reverse proxying and controls.

## Redpiler

Credit to MAIR for the sampled compilation algorithm that informed this Redpiler rewrite, including piston arrangements and conditional wiring.

Redpiler discovers the circuit once and runs a compact program instead of repeatedly searching the world for the same connections. MROWW's rewritten compiler also models supported piston movement and the electrical connections it changes.

The compiler takes an isolated plot snapshot, including scheduled work. It finds components, wires, inputs, piston groups and the cells their payloads can occupy. It then probes transitions in scratch worlds using the interpreter, recording electrical dependencies, ordered rise/fall updates, movement notifications and conditional connections for different payload arrangements.

Those results become indexed components, links, movement tables and schedules. Identical output instruction sequences share storage. The runtime processes input changes and scheduled events using that program, then publishes results at the visual update rate. Independent discovery probes can use multiple threads. Execution inside one plot remains serial; plots run on separate threads.

Rust is the default executor. The optional LLVM backend runs the same program in a native C executor compiled with Clang/LLVM at `-O3`, loaded as a Windows DLL or Linux shared library. The executor is compiled and reused, with circuit-specific program tables.

```text
/tps 0
/rp compile
/adv 1
/tps unlimited
/rp reset
```

Compilation covers the current plot and runs on a worker with progress feedback. It activates only if the plot still matches its starting snapshot. Unsupported circuits or entry states are rejected with a reason and position, leaving the interpreter available. Let piston movement settle and finish partial ticks before compiling.

While Redpiler is active, plot edits are blocked in every publication mode. Levers and buttons still work. Run `/rp reset` before editing blocks, containers, signs, or using WorldEdit to change the world.

- `/rp compile --llvm` selects the native backend. Install Clang and LLD on the server's PATH, or set `MCHPRS_LLVM_CLANG` and `MCHPRS_LLVM_LINKER` to their executable paths. Bundled native sources build on demand. `MCHPRS_LLVM_WORK_DIR` changes the work directory (default `./work/llvm`); `MCHPRS_LLVM_DLL` selects an already built matching library.
- `--publish-all` shows all blocks and piston motion. `--publish-redstone` shows redstone components. `--publish-io` (or `--io-only`, `-i`) shows controls and visible outputs. Pick one. Standard builds default to redstone publication; use full publication to inspect moved payloads.
- `/rp publish all|redstone|io` changes the publication level after compilation without recompiling or advancing simulation. The newly selected level is published immediately, including current piston motion in `all` mode. Lower levels leave piston geometry at its last published state; switching back to `all` catches it up.
- `--update` (`-u`) recalculates neighbors after reset. `/rp reset` exports state and pending work back to the interpreter.
- `/rp off` is an alias for `/rp reset`. A normal reset retains the prepared executor: recompiling with the same options and unchanged exported world resumes it. Block/entity edits, tick or piston changes, and host rate-limit changes invalidate it; draining queued command messages is allowed. `--update` and staged open-barrel notifications exclude this reuse. This does not reuse connectivity across arbitrary edits.
- `/rp clear` stops execution, cancels pending compilation and discards this plot's checkpoint. It also clears the process-shared owned LLVM artifact cache, forcing the next native module build; other plots' active executors keep running. A busy native build is reported immediately. Locked artifacts may remain until later cleanup, and a custom `MCHPRS_LLVM_DLL` is never deleted.
- `/toggleautorp` toggles automatic compilation.

Admission depends on component types, piston arrangements and entry state. Accepted circuits can still expose differences from the interpreter. LLVM is optional and experimental. See the [compiler passes](crates/core/src/redpiler/sampled/passes/README.md) for implementation details.

## Testing

Testing includes 200+ Redpiler test cases and 140+ interpreter redstone test cases, alongside world, save/load, plot and client tests. We use small mechanism tests, real schematics, seeded fuzzing and whole-computer replays.

- Interpreter tests cover electrical rules, wire propagation and shape, observer/BUD ordering, piston movement, scheduling, cache invalidation and partial stepping.
- Redpiler tests cover discovery, unsupported circuits, source ordering, conditional wiring, piston arrangements, Rust/LLVM agreement, publication modes and reset. Handoff tests continue in the interpreter to check that state and pending work survive.
- RedstoneDB was used for fixture validation. The corpus contains 82 schematics, including gates, adders, memory and larger circuits. The differential test exercises lever combinations or forward/backward input toggles, then adds 32 seeded short pulses per fixture and compares components after every wait tick.
- Physical capture packs contain 22 piston fixtures and 22 IO fixtures covering gates, adders, counters and edge cases. The packs overlap; these are not 44 unique builds.
- Frozen CPU replays run for 50,000 ticks and check world checkpoints, ordered output, Pong screen frames, BubbleSort RAM and halt boundaries. Fixtures have pinned inputs and hashes so comparisons use the same initial state and stimulus.

The redstone computers tested include:

- PM1 SORT: sorting program, output and halt behavior.
- ANPU Pong: Pong execution and recorded screen frames.
- CPU BubbleSort: sorting execution, RAM changes and command output.
- ZCore12B: Sierpinski program and screen output, including Rust/LLVM comparisons.

FPU arithmetic circuits and PC_COUNTER are also tested with input/output traces. These are supporting arithmetic and counter builds, separate from the four computer fixtures. Test windows vary: frozen replays use 50,000 ticks, while backend comparisons also use shorter active runs.

Fuzzing generates seeded 8x6 flat circuits with levers, dust, torches, repeaters, comparators and lamps. Each gets 0-16 warmup ticks, then 64 input actions with waits of 0-8 ticks. Blocks and comparator outputs are compared after each action and tick, followed by reset and 12 more interpreted ticks. Failures print the seed, circuit and action prefix for replay. The default is 64 generated cases; `MCHPRS_REDSTONE_FUZZ_SEED` and `MCHPRS_REDSTONE_FUZZ_CASES` select another campaign.

Java parity checks use separate Minecraft Java 1.21.5 captures with matching geometry and inputs. They compare observed block states for piston landing, wire-shape changes and selected circuit traces. This provides an independent reference: agreement between Rust, LLVM and our interpreter alone does not prove vanilla behavior. Coverage is specific to the tested mechanisms and traces.

## Performance

Approximate throughput in game ticks per second (TPS):

- **ZCore12B (fully instant):** Interpreter **119 TPS**; Rust backend **1,810 TPS**; LLVM backend **3,126 TPS**. Compilation: Rust **28.6 s**, LLVM **28.0 s**.
- **ANPU Pong (piston used):** Interpreter **1,016 TPS**; Rust backend **16,501 TPS**; LLVM backend **26,525 TPS**. Compilation: Rust **16.6 s**, LLVM **16.8 s**.
- **CHUNGUS Mandelbrot (fully non-instant):** Interpreter **~1,884 TPS**; Rust backend **16,468 TPS**; LLVM backend **27,673 TPS**. Compilation: Rust **95.9 s**, LLVM **98.0 s**.
- **FPU: (fully instant, combinational)** Interpreter **200 TPS**; Rust backend **2,443 TPS**; LLVM backend **3,834 TPS (estimated)**. Compilation: Rust **7.4 s**, LLVM **not measured**.
- **PC_COUNTER (fully instant counter)** Interpreter **680 TPS**; Rust backend **13,706 TPS**; LLVM backend **21,856 TPS**. Compilation: Rust **1.1 s**, LLVM **1.1 s**.

Recorded on a Ryzen 9 5950X, Windows x64, with optimized builds. Results span different revisions and workloads, so they are not direct speedup comparisons. Compiled TPS excludes publication and server overhead, which can substantially reduce throughput; short runs do not establish sustained performance.

To run checks:

```sh
cargo test -p mchprs_core --lib redstone:: --locked
cargo test -p mchprs_core --lib redpiler:: --locked
cargo test -p mchprs_core --lib normal_redstone_matches_interpreter --locked -- --nocapture
cargo test -p mchprs_core --lib redstonedb_levers_match_interpreter --locked -- --nocapture
cargo test -p mchprs_core --test cpu_references --release --locked frozen_reference -- --ignored --test-threads=1
cargo bench -p mchprs_core --bench cpus -- --iterations 3
cargo run -p mchprs_core --profile bench --locked --example bench_chungus --features redpiler-full-publication -- 128 1 --paired
cargo run -p mchprs_core --profile bench --locked --example bench_pc_counter
```

Long replays are run explicitly with `--ignored`. Keep frozen expectations when investigating failures.

## Server setup

[config.rs](crates/core/src/config.rs) lists all settings/defaults: plot speed, automatic compilation, view distance, neighbor snapshots, render throttling, WorldEdit limits and history/Git/waveform budgets. `schemati = true` uses player UUID folders for schematics. `schematic_save_prefix` adds a folder or filename prefix to saves.

Velocity modern forwarding uses `[velocity]` with `secret_file` pointing to the forwarding secret. Keep the backend behind the proxy. Authenticated forwarding supports SignedVelocity decisions for chat/commands. `native_chat = false` silences backend public chat and join/leave notices; an external broadcaster must supply public chat.

`redbox = true` enables the optional RedBox Velocity bridge for positions, teleports, gamemodes and proxy chat. The proxy then owns travel commands and moderation. LuckPerms supports MySQL/PostgreSQL with `source = "database"`, or effective proxy permissions with `source = "redbox"` (requires RedBox). Rank metadata and PlotSquared compatibility are supported. See [permissions setup](docs/MCHPRS_PERMISSIONS.md).

`/serverinfo`, `/serverinfo plots [page]`, `/serverinfo plot [x z]` and `/serverinfo settings` report running plots, timings, resources and compilation. They require explicit admin permission. `/whitelist add|remove <name>` manages access. Commands and operations log at INFO; file logs rotate between two 16 MiB files in `logs/`. `MCHPRS_LOG` selects filters; release builds omit debug/trace events.

Headless tools can use Rust's `headless::Session` for loading, inputs, stepping, compilation and snapshots. `cargo run -p mroww-redgear-api --release` starts a separate JSON-lines TCP adapter at `127.0.0.1:8765`. Its [handler](crates/redgear-api/src/main.rs) defines requests; it only binds IPv4 loopback.

More references: [docs](docs/README.md). License: [MIT](LICENSE).
