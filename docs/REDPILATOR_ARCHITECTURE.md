# Redpilator architecture

Redpilator compiles a circuit into a program, then runs that program instead of searching the world for connections on each update. Rust is the default executor. The optional LLVM backend runs a native C executor built with Clang.

Credit to MAIR for the sampled compilation algorithm that informed this rewrite, including piston arrangements and conditional wiring.

## Compilation

The plot takes a snapshot of blocks, block entities, scheduled ticks and piston state. A worker thread compiles the snapshot. The plot keeps handling packets and commands while simulation is paused. Edits are blocked during compilation and remain blocked while the compiled runtime is active.

The compiler scans components, traces wiring and finds piston groups. It probes input transitions with the interpreter in isolated scratch worlds and records the resulting updates. Scratch state is restored between probes.

The compiler stores separate instruction sequences for rising and falling signals. It also stores electrical links and connections that depend on piston positions. The instructions preserve callback order, including short pulses and piston notifications.

Before activating the result, the plot checks that its state still matches the snapshot. A failed, cancelled or stale compilation never becomes the active runtime.

Source-order probes can reuse observations from an earlier compilation. Each record stores ordered callbacks by world position and a certificate for the source's graph dependencies and the scratch chunks it read or wrote. The compiler checks those certificates against the current quiet probe basis, then lowers reusable observations into the fresh program's numeric IDs. Wiring, piston arrangements and movement validation still rebuild from the current snapshot.

Reuse is conservative: changed data anywhere in a dependency chunk can resample nearby independent sources. Bounds, loaded chunks, cursed state, ordered pending ticks and the complete piston context must match; advancing the logical clock currently prevents reuse. Unknown reads and unsafe shortcuts also cause fresh probing. A rebuilt program receives a fresh runtime initialized from the current world.

Retained sampling data has estimated limits of 64 MiB per program and 256 MiB shared across plots. Exhausting that budget declines retention without rejecting compilation. These limits do not bound total compiler or process memory. Full-program scratch frames are captured only for the source comparisons that consume them; routine probes retain callback observations.

## Execution

The program stores components, links, instructions and movement rules. Components use numeric IDs. The runtime stores current signal strengths, component states, pending ticks and piston movement.

Inputs and scheduled ticks run the stored instructions. While Redpilator is active, it advances the circuit and the plot's interpreter stays paused.

Pistons can change electrical connections by moving blocks. The compiler records movable blocks, the cells they can occupy and the connections for supported arrangements. During execution, movement selects the current arrangement and sends its recorded notifications.

## Publishing and reset

The runtime's state is separate from the plot world. Publishing copies results back to the world for players to see. `/visual rate` controls how often updates are sent, independently of simulation speed.

Publication can include all blocks and piston motion, redstone components, or just controls and visible outputs. Changing publication mode does not advance simulation.

LLVM IO publication reads only the compiled control/output component list. Effects flushing reads queued notes and command state without reconstructing piston geometry, electrical inputs or pending ticks. Both paths validate native records before publishing or acknowledging effects. Redstone/full publication and reset retain the complete snapshot path.

`/rp reset` and `/rp off` export the current state and pending work to the interpreter. This includes piston state. They continue from the current state rather than restarting the circuit.

`/rp reset` (including `/rp r` and `/rp off`) discards the plot's prepared executor and sampling cache and cancels pending compilation. `/rp clear` also discards the shared LLVM build cache. Active runtimes on other plots keep running. Internal handoffs can retain the prepared executor and sampling data; recompilation resumes the executor only when the world and compiler options still match.

## LLVM backend

The encoder writes the compiled program into versioned, little-endian tables. The native library is built on demand and reused with different circuit tables. The loader checks the ABI version before creating native state.

Each runtime owns its state buffers, command state and library handle. After native state is created, errors during command setup or tick import drop the runtime and free its resources. The encoder borrows the shared program instead of copying the circuit.

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
