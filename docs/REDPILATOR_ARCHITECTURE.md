# Redpilator architecture

Redpilator compiles a circuit into a program, then runs that program instead of searching the world for connections on each update. Rust is the default executor. The optional LLVM backend runs a native C executor built with Clang.

## Compilation

The plot takes a snapshot of blocks, block entities, scheduled ticks and piston state. A worker thread compiles the snapshot. The plot keeps handling packets and commands while simulation is paused. Edits are blocked during compilation and remain blocked while the compiled runtime is active.

The compiler scans components, traces wiring and finds piston groups. It probes input transitions with the interpreter in isolated scratch worlds and records the resulting updates. Scratch state is restored between probes.

The compiler stores separate instruction sequences for rising and falling signals. It also stores electrical links and connections that depend on piston positions. The instructions preserve callback order, including short pulses and piston notifications.

Arrangement sampling keeps shared token mappings intact while dense tables fit the existing 768 MiB estimated-table budget. On the first overflow, it compacts retained and subsequent candidates by folding token digits whose input rows match across every combination of the other axes. It retains the token axes and physical-to-electrical mappings, then rebuilds retained-table sizes and ranks. This avoids splitting shared movement groups when compaction is unnecessary. Admission estimates wire visits saved per retained byte, assuming uniform token movement frequency; it is not profile-guided. A bounded selection replaces lower-benefit tables within the budget, then restores reader discovery order before lowering. Other readers retain graph routing. The budget does not bound total compiler or process memory.

Movement lowering uses stable first-seen group and condition IDs with hash indexes for lookup. Emission still follows the stored vectors, and completed movement buffers release spare capacity.

Before activating the result, the plot checks that its state still matches the snapshot. A failed, cancelled or stale compilation never becomes the active runtime.

Source-order probes can reuse observations from an earlier compilation. Each record stores ordered callbacks by world position and a certificate for the source's graph dependencies and the scratch chunks it read or wrote. The compiler checks those certificates against the current quiet probe basis, then lowers reusable observations into the fresh program's numeric IDs. Wiring, piston arrangements and movement validation still rebuild from the current snapshot.

Reuse is conservative: changed data anywhere in a dependency chunk can resample nearby independent sources. Bounds, loaded chunks, cursed state, ordered pending ticks and the complete piston context must match; advancing the logical clock currently prevents reuse. Unknown reads and unsafe shortcuts also cause fresh probing. A rebuilt program receives a fresh runtime initialized from the current world.

Retained sampling data has estimated limits of 64 MiB per program and 256 MiB shared across plots. Exhausting that budget declines retention without rejecting compilation. These limits do not bound total compiler or process memory. Full-program scratch frames are captured only for the source comparisons that consume them; routine probes retain callback observations.

## Execution

The program stores components, links, instructions and movement rules. Components use numeric IDs. The runtime stores current signal strengths, component states, pending ticks and piston movement.

Inputs and scheduled ticks run the stored instructions. While Redpilator is active, it advances the circuit and the plot's interpreter stays paused.

Normal LLVM execution runs fixed batches of sixteen game ticks when the configured rate is at least 100 TPS (or unlimited) and the observed tick cost is at most 10 ms. The runner waits for a full batch of tick credit and publishes at opportunities after execution returns; it does not add scalar ticks to meet a visual deadline. Stopped plots, lower rates, slower observed execution and exact manual advancement use scalar stepping.

Each native batch retains the internal scheduler, event and movement phases of every tick. Command clocks remain live per tick. If command-effect storage needs to grow, execution returns before that tick, Rust grows and rebinds the buffer, and the batch resumes without repeating completed ticks. Failures retain the successful prefix. Native movement flips use 16-byte records, validated precomputed token contributions and proven fixed-state range selectors, matching the Rust replay; component contributions remain live. Both executors can replay a validated concatenation of fixed movement ranges when the whole range fits the operation budget. Conditional transitions and partial-failure cases retain scalar replay. Current circuit modules use native semantic version 38; old generic DLL overrides must be removed or replaced with a matching generated module.

Generated modules access immutable auxiliary tables, scheduler capacity, reactive/lazy-wire decisions and movement dimensions directly as compile-time constants. The generic diagnostic reference retains its validated pointer cache; temporary allocation-size views and live instances initialize that cache before use.

Pistons can change electrical connections by moving blocks. The compiler records movable blocks, the cells they can occupy and the connections for supported arrangements. During execution, movement selects the current arrangement and sends its recorded notifications.

A bounded Rust connectivity-cache prototype was tested separately from table compaction. Three paired PM1 comparisons did not establish a repeatable throughput gain, so it is not part of the runtime. The standalone [experiment patch](../scraps/experiments/pm1-connectivity-cache.patch) is retained for further investigation.

## Publishing and reset

The runtime's state is separate from the plot world. Publishing copies results back to the world for players to see. `/visual rate` controls how often updates are sent, independently of simulation speed.

Publication can include all blocks and piston motion, redstone components, or just controls and visible outputs. Changing publication mode does not advance simulation.

Visual deadlines retain their cadence across completed native batches and skip expired frames. IO publication gathers only control/output states into reusable buffers; piston state is not read. The network writer already coalesces block updates, encodes, compresses and writes them on its own thread. The default fast-render limit is 10 Hz; a lower configured limit still applies.

Command output uses a separate ordered queue and continues draining while simulation is paused. Live delivery accepts up to 1,024 messages per one-second window, with 4,096 queued messages and 64 KiB per message as hard bounds. Execution no longer rejects an otherwise valid message because 64 messages ran earlier in the second. Host queue changes refresh native accounting without acknowledging or discarding unpublished effects. Queue exhaustion remains an explicit execution error.

LLVM IO publication reads only the compiled control/output component list. Redstone publication reads packed visual state for mutable non-piston components, including wire shapes, comparator output and note instruments. Both cache component reads until native execution or input changes. Effects flushing reads queued notes, command messages and output quotas; IO and effects-only flushing skip command entities. Circuits without notes or commands skip effect exports. These paths validate native records before publishing or acknowledging effects. Full publication and reset retain the complete snapshot path, including electrical state, pending ticks and piston geometry. Current generated modules use native semantic version 38.

`/rp reset` and `/rp off` export the current state and pending work to the interpreter. This includes piston state. They continue from the current state rather than restarting the circuit.

`/rp reset` (including `/rp r` and `/rp off`) discards the plot's prepared executor and sampling cache and cancels pending compilation. `/rp clear` also discards the shared LLVM build cache. Active runtimes on other plots keep running. Internal handoffs can retain the prepared executor and sampling data; recompilation resumes the executor only when the world and compiler options still match.

## LLVM backend

Fresh compilation requires an idle entry between complete ticks: no scheduled component ticks, piston events, active motions or unfinished movement work. Busy entry is rejected without advancing or modifying the world. Powered components, memory contents and stationary extended pistons are allowed. An empty work queue does not prove electrical equilibrium, so dormant reactive behavior remains supported. After creation, scheduling, wire history and piston movement remain dynamic. Internal continuation imports specialize their tick capacity into module identity; an unchanged prepared executor can resume its existing state.

The encoder writes the validated program into versioned, little-endian tables. The generator embeds these as aligned `static const` data with typed table views and compile-time layout assertions. C23 `#embed` keeps large table payloads compact while making the data visible to LLVM. The runtime and circuit tables compile in the same translation unit, and native component, instruction, link and auxiliary accessors resolve directly to the generated constants.

Artifacts are keyed by the complete program identity, generator/runtime sources, compiler/linker identity, flags and target. The program identity includes strict/relaxed mode and immutable command bytecode, literals and notifications. The loader checks embedded identity and ABI before creating native state, including for `MCHPRS_LLVM_DLL` overrides. Different circuits cannot share an incompatible module. Instances of the same module share only immutable data.

Each runtime owns independent state buffers, mutable command state and a library handle. Command component/chain indexes and notification tables are generated constants. Creation uses the module's embedded program; the temporary host execution blob is released after loading. Rust retains publication metadata and resizable effect buffers, advances no second inference engine, and performs no full-state synchronization on each tick/batch. Command output-window age is captured at instance creation so compilation time does not extend quotas. Setup failures release the partially initialized runtime. Cancellation reaches emission and native build tools.

This requires native semantic version 38 and Clang with C23 embedding support. Unset an old custom DLL override to use automatic compilation. `scripts/build-llvm-native.ps1 -ProgramDirectory <generated-build-directory>` can rebuild a matching module; without that option the script builds only a generic diagnostic reference.

Strength histograms retain 32-bit contribution counts and store an occupied-level mask in unused bucket zero, so finding the maximum does not scan every level. Generated table access preserves the existing update order and operation-budget accounting.

## Limits

Unsupported components, piston arrangements, unfinished movement or dependencies outside the captured region can prevent compilation. The LLVM backend has additional restrictions.

Tests cover specific mechanisms and circuits. Passing compilation does not prove that every circuit will match the interpreter or vanilla Minecraft.

Compilation and execution share the server process. A native fault or an OS OOM kill can still terminate the server.

## Source files

| File | Purpose |
| --- | --- |
| [Plot compilation](../crates/core/src/plot/redpilator_compilation/mod.rs) | Snapshot, worker thread, progress and activation |
| [Compiler](../crates/core/src/redpilator/compiler.rs) | Entry checks, backend setup, publishing and reset |
| [Compiler passes](../crates/core/src/redpilator/sampled/passes/README.md) | Discovery and instruction generation |
| [Source probe cache](../crates/core/src/redpilator/sampled/source_cache.rs) | Dependency certificates, retained callback traces and memory reservations |
| [Program](../crates/core/src/redpilator/sampled/program.rs) | Compiled circuit data |
| [Movement tables](../crates/core/src/redpilator/sampled/movement.rs) | Movement records, arrangement lowering and movement validation |
| [Rust executor](../crates/core/src/redpilator/sampled/executor.rs) | Mutable state and instruction execution |
| [Runtime publication](../crates/core/src/redpilator/sampled/publication.rs) | Projected blocks, dirty cells and exported piston parts |
| [World publication](../crates/core/src/redpilator/compiler/publication.rs) | World writes, motion identity reconciliation and effect publication |
| [Scheduler](../crates/core/src/redpilator/sampled/scheduler.rs) | Bounded timing queues and host tick import/export |
| [Diagnostics](../crates/core/src/redpilator/sampled/diagnostics.rs) | Optional memory and routing reports |
| [LLVM runtime](../crates/core/src/redpilator/sampled/llvm/mod.rs) | Native state, controls and pending ticks |
| [LLVM publication](../crates/core/src/redpilator/sampled/llvm/publication.rs) | Checked IO component and queued-effect transfers |
| [LLVM encoding](../crates/core/src/redpilator/sampled/llvm/encoding.rs) | Binary tables, alignment and validation |
| [LLVM library](../crates/core/src/redpilator/sampled/llvm/library.rs) | Native exports, ABI checks and library ownership |
| [Allocation helpers](../crates/core/src/redpilator/memory.rs) | Fallible native-buffer allocation |

See the [complete function and type map](REDPILATOR_CODE_MAP.md) for code ownership,
the [README](../README.md#redpilator) for commands and setup, and the
[redstone](REDSTONE_MODEL.md) and [piston](PISTON_MODEL.md) models for interpreter behavior.
