# MROWW

**M**inecraft **R**edstone **o** **W**ysokiej **W**ydajności

Based on [MCHPRS project](https://github.com/MCHPR/MCHPRS)

By Kazik24 and Lord225.

---
A creative redstone server for Minecraft Java 1.21.5. Each 512x512 plot runs on its own thread. There is a physical redstone/piston interpreter and a rewritten Redpilator with an optional LLVM backend, plus tools for building and debugging circuits.

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
- `/small` makes you half-size to fit inside builds. `/small wolf|fox|cat|ocelot|baby|bat|enderdragon` picks the animal other players see; baby is smaller again. `/small off` restores normal size when there is room. Your choice is saved.
- `/kiss` kisses the nearest other player on your plot. `/bark`, `/mrow`, `/meow`, `/woof` and `/rawr` announce furry emotes to everyone on the server, as does `/kiss`. `/mrow` and `/meow` play a cat sound, and `/woof` plays a wolf sound. These three share a burst of 10 uses per player and refill one use per second.

Simulation:

- `/tps 20` runs normally, `/tps 0` pauses, `/tps unlimited` runs as fast as possible. `/tps timings` shows timing data.
- Pause, then `/adv 1` advances one game tick. `/adv nano 1` and `/adv pico 1` step interpreter operations inside a tick. Two game ticks make one redstone tick.
- The interpreter handles supported dust, torches, repeaters, comparators, containers, lamps, copper bulbs, observers, BUD updates, pistons, moving blocks and instant piston circuits. Exact behavior is in the [redstone](docs/REDSTONE_MODEL.md) and [piston](docs/PISTON_MODEL.md) models.
- `/visual pistons auto|on|off` controls piston animation. `/visual rate <0-1000>` changes client update frequency. `/visual displayonly on|off` limits simulation updates to lamps and copper bulbs. `/visual publish all|redstone|io|auto` selects compiled publication.
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
- `/wire` gives a wire pen. Hold a carrot on a stick, right-click a support or dust, aim, then right-click to build the preview. It places supports and dust together. F cycles horizontal, two vertical planes and free aim; sneak + F changes the bend. `/wire free` and `/wire plane` select aiming directly. Sneak + right-click cancels the route; `/wire off` disables the pen.

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

Enable `waveform_enabled = true` in the config. Capture uses the interpreter: run `/rp reset` before recording if Redpilator is active.

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

## Redpilator

Use `/redpilator` or its short alias `/rp`.

Credit to MAIR for the sampled compilation algorithm that informed this Redpilator rewrite, including piston arrangements and conditional wiring.

Redpilator discovers the circuit once and runs a compact program instead of repeatedly searching the world for the same connections. MROWW's rewritten compiler also models supported piston movement and the electrical connections it changes.

The compiler takes an isolated plot snapshot, including scheduled work. It finds components, wires, inputs, piston groups and the cells their payloads can occupy. It then probes transitions in scratch worlds using the interpreter, recording electrical dependencies, ordered rise/fall updates, movement notifications and conditional connections for different payload arrangements.

Those results become indexed components, links, movement tables and schedules. The runtime processes input changes and scheduled events using that program, then publishes results at the visual update rate.

Rust is the default executor. The optional LLVM backend generates a module for each circuit: the runtime and program tables compile together with Clang/LLVM at `-O3`, and the validated tables become `static const` data visible to optimization. The Windows DLL or Linux shared library owns those tables; each instance has independent mutable state. The host submits ticks and controls and reads requested publications/effects. Performance depends on the circuit and publication settings.

For fresh LLVM compilation, stop the circuit and let scheduled component work and piston movement finish. Powered components and stationary extended pistons are supported. Starting the compiled circuit afterward preserves normal scheduling and movement.

The current PM1 performance baseline is [pm1_redpilator.schem](test_data/pm1_redpilator.schem). Its [protocol](test_data/cpu-references/pm1_redpilator_protocol.json) presses Start once with no preparation ticks; the [captured interpreter outputs](test_data/cpu-references/pm1_redpilator_reference.json) and [measured results](test_data/cpu-references/pm1_redpilator_results.json) are retained. Kernel TPS and server publication/client-send costs are measured separately.

```text
/tps 0
/rp compile
/adv 1
/tps unlimited
/rp reset
```

While Redpilator is active, plot edits are blocked in every publication mode. Levers and buttons still work. Run `/rp reset` before editing blocks, containers, signs, or using WorldEdit to change the world.

- `/rp compile --llvm` selects the native backend. Install Clang with C23 `#embed` support and LLD on the server's PATH, or set `MCHPRS_LLVM_CLANG` and `MCHPRS_LLVM_LINKER` to their executable paths. The runtime and generated circuit tables build on demand. `MCHPRS_LLVM_WORK_DIR` changes the work directory (default `./work/llvm`); `MCHPRS_LLVM_DLL` selects an already built module only when its embedded program identity and ABI match. Old generic DLLs cannot run this path; unset the override to compile the correct module automatically.
- `--relaxed` skips wire-shape replay and its indirect callbacks. With LLVM, circuits whose wires have no outgoing electrical links also compute wire power on reads instead of eagerly refreshing it during movement. Watched-wire strength tracking and piston-head/observer callbacks remain enabled. Relaxed mode can change transient wire behavior; the default retains strict replay.
- `/visual publish all|redstone|io|auto` saves the plot's publication mode and applies it to active compiled execution. `all` includes piston geometry and motion, `redstone` shows redstone components, and `io` shows controls and visible outputs. `auto` uses `all` through the configured fast render TPS threshold (200 by default) and `io` above it or at unlimited TPS. Standard builds default to `redstone`. Switching to `all` catches up piston geometry. `/rp publish` is an alias.
- `--publish-all`, `--publish-redstone`, and `--publish-io` set the mode for the compiled run. `/rp reset` and `/rp off` restore the saved choice. `/visual publish` shows the current mode.
- `/rp reset` exports state and pending work back to the interpreter.
- `/rp reset` (aliases `/rp r` and `/rp off`) stops execution, cancels pending compilation and discards this plot's prepared executor and sampling cache. The next compile rebuilds the circuit from the exported interpreter state.
- `/rp clear` does the same and also clears the process-shared owned LLVM artifact cache, forcing the next native module build; other plots' active executors keep running. A busy native build is reported immediately. Locked artifacts may remain until later cleanup, and a custom `MCHPRS_LLVM_DLL` is never deleted.

Admission depends on component types, piston arrangements and entry state. Accepted circuits can still expose differences from the interpreter. LLVM is optional and experimental. See the [compiler passes](crates/core/src/redpilator/sampled/passes/README.md) for implementation details.

## Testing

Testing includes 200+ Redpilator test cases and 140+ interpreter redstone test cases, alongside world, save/load, plot and client tests. We use small mechanism tests, real schematics, seeded fuzzing and whole-computer replays.

- Interpreter tests cover electrical rules, wire propagation and shape, observer/BUD ordering, piston movement, scheduling, cache invalidation and partial stepping.
- Redpilator tests cover discovery, unsupported circuits, source ordering, conditional wiring, piston arrangements, Rust/LLVM agreement, publication modes and reset. Handoff tests continue in the interpreter to check that state and pending work survive.
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

- **PM1 SORT (active program):** Interpreter: **160 TPS**; LLVM backend **1,019 TPS**.
- **ZCore12B (fully instant):** Interpreter **119 TPS**; Rust backend **1,810 TPS**; LLVM backend **3,126 TPS**. Compilation: Rust **28.6 s**, LLVM **28.0 s**.
- **ANPU Pong (piston used):** Interpreter **1,016 TPS**; Rust backend **16,501 TPS**; LLVM backend **26,525 TPS**. Compilation: Rust **16.6 s**, LLVM **16.8 s**.
- **CHUNGUS Mandelbrot (fully non-instant):** Interpreter **~1,884 TPS**; Rust backend **16,468 TPS**; LLVM backend **27,673 TPS**. Compilation: Rust **95.9 s**, LLVM **98.0 s**.
- **FPU: (fully instant, combinational)** Interpreter **200 TPS**; Rust backend **2,443 TPS**; LLVM backend **3,834 TPS**. Compilation: Rust **7.4 s**, LLVM **7.5s**.
- **PC_COUNTER (fully instant counter)** Interpreter **680 TPS**; Rust backend **13,706 TPS**; LLVM backend **21,856 TPS**. Compilation: Rust **1.1 s**, LLVM **1.1 s**.

Recorded on a Ryzen 9 5950X, Windows x64, with optimized builds. Compiled TPS excludes publication and server overhead, which can reduce throughput; short runs do not establish sustained performance.

An [older PM1 interpreter benchmark](test_data/cpu-references/pm1-sort-correction-results.json) measured **192 TPS**, or **62.68 s** for **12,051 ticks** including startup. Its measurement interval differs from the LLVM program-only result above.

To run checks:

```sh
cargo test -p mchprs_core --lib redstone:: --locked
cargo test -p mchprs_core --lib redpilator:: --locked
cargo test -p mchprs_core --lib normal_redstone_matches_interpreter --locked -- --nocapture
cargo test -p mchprs_core --lib redstonedb_levers_match_interpreter --locked -- --nocapture
cargo test -p mchprs_core --test cpu_references --release --locked frozen_reference -- --ignored --test-threads=1
cargo bench -p mchprs_core --bench cpus -- --iterations 3
cargo run -p mchprs_core --profile bench --locked --example bench_chungus --features redpilator-full-publication -- 128 1 --paired
cargo run -p mchprs_core --profile bench --locked --example bench_pc_counter
```

Long replays are run explicitly with `--ignored`. Keep frozen expectations when investigating failures.

## Server setup

[config.rs](crates/core/src/config.rs) lists all settings/defaults: plot speed, view distance, neighbor snapshots, render throttling, WorldEdit limits and history/Git/waveform budgets. `schemati = true` uses player UUID folders for schematics. `schematic_save_prefix` adds a folder or filename prefix to saves.

Velocity modern forwarding uses `[velocity]` with `secret_file` pointing to the forwarding secret. Keep the backend behind the proxy. Authenticated forwarding supports SignedVelocity decisions for chat/commands. `native_chat = false` silences backend public chat and join/leave notices; an external broadcaster must supply public chat.

`redbox = true` enables the optional RedBox Velocity bridge for positions, teleports, gamemodes and proxy chat. The proxy then owns travel commands and moderation. LuckPerms supports MySQL/PostgreSQL with `source = "database"`, or effective proxy permissions with `source = "redbox"` (requires RedBox). Rank metadata and PlotSquared compatibility are supported. See [permissions setup](docs/MCHPRS_PERMISSIONS.md).

`/serverinfo`, `/serverinfo plots [page]`, `/serverinfo plot [x z]` and `/serverinfo settings` report running plots, timings, resources and compilation. They require explicit admin permission. `/whitelist add|remove <name>` manages access. Commands and operations log at INFO; file logs rotate between two 16 MiB files in `logs/`. `MCHPRS_LOG` selects filters; release builds omit debug/trace events.

Headless tools can use Rust's `headless::Session` for loading, inputs, stepping, compilation and snapshots. `cargo run -p mroww-redgear-api --release` starts the [protocol 4 API](docs/REDGEAR_API_V4.md) at `127.0.0.1:8765` over JSON-lines TCP on IPv4 loopback. It supports Rust, LLVM and interpreter sessions, compilation progress, timed power inputs and block projections. Incremental recompilation is currently advertised as unsupported.

More references: [docs](docs/README.md). License: [MIT](LICENSE).
