# Redpiler architecture

**Redpiler pays the cost of understanding a circuit once, then runs the result
many times.** That's where the speed comes from.

Imagine a lever connected to a lamp through a long dust wire. The interpreter
follows the blocks and their update rules when something changes. Redpiler
works out the connection during compilation, so execution can use stored links
and instructions.

There are three main pieces:

| Piece | Job |
| --- | --- |
| Compiler | Discover connections and learn update order. |
| Program | Store what the compiler learned. |
| Runtime | Run that program and keep the circuit's current state. |

## 1. Learn the circuit

Compilation starts with a **snapshot of the plot**, including scheduled ticks
and piston state. A background worker does the expensive work. Before accepting
its result, the plot checks that the captured entry state still matches.

First, the compiler finds components and traces wiring. Then it runs small
experiments in isolated copies of the world, called **scratch worlds**.

These experiments use the existing interpreter: change an input, observe the
updates, restore the scratch world, repeat. This is what *sampled compilation*
means here. It discovers behavior through targeted probes.

The compiler records both **who gets updated** and **in what order**. Turning a
signal on and turning it off can need different instruction sequences.

## 2. Deal with pistons

A lever changes a signal. A piston can change the wiring itself.

For example, moving a solid block can connect or disconnect a power path. The
compiler discovers piston groups, movable blocks and the cells those blocks can
occupy. It records connections for supported arrangements, plus the notifications
caused by movement.

At runtime, movement changes which connections are active. This is why the
program needs more than a fixed graph of wires.

## 3. Run the program

The **program** stores components, links, ordered instructions and movement
rules. Components get numeric IDs, so the executor can work with indexed data.

The **runtime** stores the changing values: signal strengths, component states,
block positions and pending events.

When an input changes or a scheduled tick becomes due, the runtime executes the
affected instructions. Delays stay in a scheduler. Update order is preserved
because a brief pulse or an earlier piston notification can change what happens
next, even when the final power levels look identical.

There are two executors:

- **Rust:** the default.
- **LLVM:** an optional native C executor built with Clang/LLVM. It runs the same
  sampled program using circuit-specific tables. The native library is reused.
  This backend is experimental and has additional limits on what it accepts.

## 4. Show results in Minecraft

The runtime advances the circuit; **publication** copies results back to the
plot for players to see. Simulation speed and visual update rate are separate.

Publication can show all blocks and piston motion, redstone components, or just
controls and visible outputs. Switching modes changes what is shown without
advancing the simulation.

Block edits are blocked from the start of compilation until an explicit reset.
Levers and buttons work once compilation finishes. Changing the build would
invalidate the compiled connections.

## Compile ownership and handoff

**Only one simulator advances the circuit at a time.** After compilation, that's
the compiled runtime.

`/rp reset` writes the current state and pending work back to the interpreter,
including piston state. The circuit continues from there. The command does not
rewind it to the moment it was compiled.

A normal reset can keep the prepared executor for reuse. Matching options and
an unchanged exported world allow it to resume; edits or simulation changes
invalidate that reuse. `/rp clear` discards the plot checkpoint and clears the
shared owned LLVM artifact cache.

## Code map

| Responsibility | Entry point |
| --- | --- |
| Snapshot, worker lifecycle, progress and activation | [Plot compilation](../crates/core/src/plot/redpiler_compilation/mod.rs) |
| Entry validation, backend selection and state handoff | [Compiler](../crates/core/src/redpiler/compiler.rs) |
| Allocation checks and Linux memory limits | [Memory guards](../crates/core/src/redpiler/memory.rs) and [Linux reader](../crates/core/src/redpiler/memory/linux.rs) |
| Native runtime initialization, controls and pending ticks | [LLVM runtime](../crates/core/src/redpiler/sampled/llvm/mod.rs) |
| Binary program encoding, alignment and validation | [LLVM encoding](../crates/core/src/redpiler/sampled/llvm/encoding.rs) |
| Native exports, ABI checks and library handle ownership | [LLVM library](../crates/core/src/redpiler/sampled/llvm/library.rs) |

The runtime owns its native state and library. Initialization uses the same
cleanup path as normal shutdown, including when command binding or tick import
fails. Encoding borrows the shared program rather than copying the circuit.

## Where the shortcut stops

Redpiler accepts a subset of circuits the interpreter can run. Unsupported
components, piston arrangements, unfinished movement or dependencies outside
the captured region can prevent compilation.

Successful compilation also doesn't guarantee perfect agreement with the
interpreter or vanilla Minecraft. Tests check specific mechanisms and circuits.

Memory checks are advisory: compilation and execution still share the server
process. Hard isolation from OOM kills or native faults requires worker processes.

For the next layer of detail:

- [Commands and setup](../README.md#redpiler).
- [Compiler passes](../crates/core/src/redpiler/sampled/passes/README.md).
- [Program data](../crates/core/src/redpiler/sampled/program.rs) and
  [Rust executor](../crates/core/src/redpiler/sampled/executor.rs).
- [Redstone rules](REDSTONE_MODEL.md) and [piston rules](PISTON_MODEL.md).

Credit to MAIR for the sampled compilation algorithm that informed this rewrite,
including piston arrangements and conditional wiring.
