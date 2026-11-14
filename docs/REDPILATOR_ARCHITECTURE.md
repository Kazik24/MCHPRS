# Redpilator architecture

Redpilator discovers a circuit once, then runs a compact program instead of searching the world for connections on every update. Rust is the default executor; the optional LLVM backend compiles the program and native runtime together with Clang.

Thanks MAIR for algorithm for probing the circuits.

## The algorithm

```text
Plot snapshot → interpreter probes → Program → Rust / LLVM → publication
```

1. **Capture.** The plot copies blocks, entities, scheduled ticks and piston state. Compilation runs on a worker while simulation is paused; the plot still handles packets and commands.
2. **Compile.** The compiler finds components, wiring and piston groups. It probes transitions in isolated `ScratchWorld` instances using the interpreter, then records electrical links, ordered callbacks and movement rules in a shared `Program`.
3. **Execute.** `Runtime` or `LlvmRuntime` processes controls, scheduled ticks and piston movement. Moving blocks select the connections for their current arrangement. The plot's interpreter stays paused.
4. **Publish.** Results are copied to the plot world at the visual update rate. Publication can show all blocks, redstone components, or just controls and outputs. It does not advance simulation.

The plot activates a compiled result only if its snapshot is still current. Failed or cancelled compilation leaves the live world alone. Compatible internal handoffs can reuse a prepared executor or previous probe results.

Reset exports current state and pending work back to the interpreter, so the circuit continues where it stopped. `/rp reset` also cancels compilation and discards the plot's retained compiler caches.

## Where the code lives

Paths below are relative to `crates/core/src/redpilator/`, except plot integration.

| Code | Owns |
| --- | --- |
| [Plot integration](../crates/core/src/plot/redpilator_compilation/mod.rs) | Snapshot, worker, progress and activation |
| [compiler.rs](../crates/core/src/redpilator/compiler.rs) | `Compiler`: active executors and the plot-facing API |
| [compiler/](../crates/core/src/redpilator/compiler) | Compilation, execution dispatch, publication, lifecycle and diagnostics |
| [scratch.rs](../crates/core/src/redpilator/scratch.rs) | Isolated interpreter worlds, probe jobs and restoration |
| [scratch/discovery.rs](../crates/core/src/redpilator/scratch/discovery.rs) | Electrical sources, wire ports and watchers |
| [sampled/compiler.rs](../crates/core/src/redpilator/sampled/compiler.rs) | Ordered compiler passes; see the [pass guide](../crates/core/src/redpilator/sampled/passes/README.md) |
| [sampled/program.rs](../crates/core/src/redpilator/sampled/program.rs) | Immutable circuit records, validation and indexes |
| [sampled/executor.rs](../crates/core/src/redpilator/sampled/executor.rs) | Mutable state, ticks and component updates; [pistons](../crates/core/src/redpilator/sampled/executor/pistons.rs) and [movement](../crates/core/src/redpilator/sampled/executor/movement.rs) have separate helpers |
| [sampled/publication.rs](../crates/core/src/redpilator/sampled/publication.rs) | Runtime projection; [compiler/publication.rs](../crates/core/src/redpilator/compiler/publication.rs) writes it into the world |
| [sampled/llvm/](../crates/core/src/redpilator/sampled/llvm) | Native encoding, generation, loading, execution and state export |

`Program` is shared immutable data. Each executor owns its mutable state. With LLVM active, the Rust runtime supplies the publication projection; it does not run a second simulation. Native modules embed the circuit tables and validate program identity and ABI before use.

## Making changes

Put discovery and lowering rules in the matching compiler pass. Put runtime behavior in the executor or its existing helpers. Keep world writes in publication code.

Preserve callback order, scheduled work and piston handoff. Check both Rust and LLVM when changing execution. Keep cancellation checks before activation, validate native data at the boundary, and retain compiler state if reset export fails.

Run the focused suite:

```sh
cargo test -p mchprs_core --lib redpilator --locked
```

LLVM cases need Clang and LLD. Long CPU replays remain explicit ignored tests. Compilation accepts only supported components and arrangements; acceptance alone does not prove parity with the interpreter or vanilla Minecraft.

See the [README](../README.md#redpilator) for commands and setup, and the [redstone](REDSTONE_MODEL.md) and [piston](PISTON_MODEL.md) models for interpreter behavior.
