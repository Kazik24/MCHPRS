# Redpilator architecture

Redpilator compiles a circuit into a program, then runs that program instead of searching the world for connections on each update. Rust is the default executor. The optional LLVM backend runs a native C executor built with Clang.

## Reading and changing the code

Follow the data in this order:

```text
plot snapshot → ScratchWorld probes → ordered compiler passes → shared Program
                                                               ↓
                                             Rust Runtime or LlvmRuntime
                                                               ↓
                                         publication → plot world / interpreter
```

`Compiler` owns the active executors and exposes the plot-facing API. Its implementation
is divided by operation: `compiler/compilation.rs` validates and activates programs or
cached executors, `compiler/execution.rs` dispatches ticks and controls,
`compiler/publication.rs` publishes state, and `compiler/lifecycle.rs` exports pending
work before reset or suspension. `compiler/diagnostics.rs` owns caller-facing errors
and statistics. These modules implement the same `Compiler`; they add no runtime layer.

`ScratchEntry` captures scheduled ticks and piston state. `ScratchWorld` owns the isolated
interpreter snapshot, probe workers and restoration; `scratch/discovery.rs` discovers
electrical sources and wire watchers. Discovery records connections; scratch probes
establish their observable callback order.

The sampled compiler already has an explicit sequence of passes, documented in the
[pass guide](../crates/core/src/redpilator/sampled/passes/README.md). Keep orchestration
there and put discovery or lowering rules in the matching pass. `Program` owns immutable
tables and indexes; `Runtime` owns mutable simulation state. Input slots, component IDs,
token IDs and projection-cell IDs refer to different arrays and must stay distinct.

Rust and LLVM implement the same program contract. Preserve symmetry by following each
behavior across its representations, rather than introducing a common dispatch trait:

| Concern | Rust implementation | Native boundary / execution |
| --- | --- | --- |
| Ticks and controls | `sampled/executor.rs`, `sampled/scheduler.rs` | `sampled/llvm/mod.rs`, native executor |
| Motion and connectivity | Executor piston/movement helpers and `sampled/routing.rs` | `sampled/llvm/movement.rs`, `sampled/llvm/routing.rs`, native executor |
| Commands and effects | `sampled/commands.rs` | `sampled/llvm/command.rs`, `sampled/llvm/effects.rs` |
| Visible state and handoff | `sampled/publication.rs`, `compiler/publication.rs` | `sampled/llvm/publication.rs` |

The Rust runtime remains the publication projection when LLVM executes; it does not run
a second simulation. Preserve validation at native boundaries, cancellation checks before
activation, callback ordering, and export-before-reset error handling. Keep execution
buffers and their allocation order intact when reorganizing hot code.

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

Each native batch retains the internal scheduler, event and movement phases of every tick. Command clocks remain live per tick. If command-effect storage needs to grow, execution returns before that tick, Rust grows and rebinds the buffer, and the batch resumes without repeating completed ticks. Failures retain the successful prefix. Native movement flips use 16-byte records, validated precomputed token contributions and proven fixed-state range selectors, matching the Rust replay; component contributions remain live. Both executors can replay a validated concatenation of fixed movement ranges when the whole range fits the operation budget. Conditional transitions and partial-failure cases retain scalar replay. Current circuit modules use native semantic version 39; old generic DLL overrides must be removed or replaced with a matching generated module.

Generated modules access immutable auxiliary tables, scheduler capacity, reactive/lazy-wire decisions and movement dimensions directly as compile-time constants. The generic diagnostic reference retains its validated pointer cache; temporary allocation-size views and live instances initialize that cache before use.

Pistons can change electrical connections by moving blocks. The compiler records movable blocks, the cells they can occupy and the connections for supported arrangements. During execution, movement selects the current arrangement and sends its recorded notifications.

A bounded Rust connectivity-cache prototype was tested separately from table compaction. Three paired PM1 comparisons did not establish a repeatable throughput gain, so it is not part of the runtime. The standalone [experiment patch](../scraps/experiments/pm1-connectivity-cache.patch) is retained for further investigation.

## Publishing and reset

The runtime's state is separate from the plot world. Publishing copies results back to the world for players to see. `/visual rate` controls how often updates are sent, independently of simulation speed.

Publication can include all blocks and piston motion, redstone components, or just controls and visible outputs. Changing publication mode does not advance simulation.

Visual deadlines retain their cadence across completed native batches and skip expired frames. IO publication gathers only control/output states into reusable buffers; piston state is not read. The network writer already coalesces block updates, encodes, compresses and writes them on its own thread. The default fast-render limit is 10 Hz; a lower configured limit still applies.

Command output uses a separate ordered queue and continues draining while simulation is paused. Live delivery accepts up to 1,024 messages per one-second window, with 4,096 queued messages and 64 KiB per message as hard bounds. Execution no longer rejects an otherwise valid message because 64 messages ran earlier in the second. Host queue changes refresh native accounting without acknowledging or discarding unpublished effects. Queue exhaustion remains an explicit execution error.

The [live publication benchmark](../test_data/cpu-references/llvm_live_publication_results.json) uses the actual plot update loop and one compressed TCP client. Three-run relaxed LLVM medians at the default sixteen-tick batch were 5,512/5,408 TPS for ZCore and 5,262/5,120 TPS for PM1, without publication/with IO near 10 Hz. Every run matched final circuit outputs; PM1 also matched all 1,534 ordered messages, including completion. Batches of 32 and 64 preserved outputs but did not establish a consistent overall throughput improvement, so the normal default remains sixteen. These measurements include live plot work; queued command delivery and final network drain are reported separately. Lower publication rates are supported.

LLVM IO publication reads only the compiled control/output component list. Redstone publication reads packed visual state for mutable non-piston components, including wire shapes, comparator output and note instruments. Both cache component reads until native execution or input changes. Effects flushing reads queued notes, command messages and output quotas; IO and effects-only flushing skip command entities. Circuits without notes or commands skip effect exports. These paths validate native records before publishing or acknowledging effects. Full publication and reset retain the complete snapshot path, including electrical state, pending ticks and piston geometry. Current generated modules use native semantic version 39.

`/rp reset` and `/rp off` export the current state and pending work to the interpreter. This includes piston state. They continue from the current state rather than restarting the circuit.

`/rp reset` (including `/rp r` and `/rp off`) discards the plot's prepared executor and sampling cache and cancels pending compilation. `/rp clear` also discards the shared LLVM build cache. Active runtimes on other plots keep running. Internal handoffs can retain the prepared executor and sampling data; recompilation resumes the executor only when the world and compiler options still match.

## LLVM backend

Fresh compilation requires an idle entry between complete ticks: no scheduled component ticks, piston events, active motions or unfinished movement work. Busy entry is rejected without advancing or modifying the world. Powered components, memory contents and stationary extended pistons are allowed. An empty work queue does not prove electrical equilibrium, so dormant reactive behavior remains supported. After creation, scheduling, wire history and piston movement remain dynamic. Internal continuation imports specialize their tick capacity into module identity; an unchanged prepared executor can resume its existing state.

The encoder writes the validated program into versioned, little-endian tables. The generator embeds these as aligned `static const` data with typed table views and compile-time layout assertions. C23 `#embed` keeps large table payloads compact while making the data visible to LLVM. The runtime and circuit tables compile in the same translation unit, and native component, instruction, link and auxiliary accessors resolve directly to the generated constants.

Artifacts are keyed by the complete program identity, generator/runtime sources, compiler/linker identity, flags and target. The program identity includes strict/relaxed mode and immutable command bytecode, literals and notifications. The loader checks embedded identity and ABI before creating native state, including for `MCHPRS_LLVM_DLL` overrides. Different circuits cannot share an incompatible module. Instances of the same module share only immutable data.

Each runtime owns independent state buffers, mutable command state and a library handle. Command component/chain indexes and notification tables are generated constants. Creation uses the module's embedded program; the temporary host execution blob is released after loading. Rust retains publication metadata and resizable effect buffers, advances no second inference engine, and performs no full-state synchronization on each tick/batch. Command output-window age is captured at instance creation so compilation time does not extend quotas. Setup failures release the partially initialized runtime. Cancellation reaches emission and native build tools.

This requires native semantic version 39 and Clang with C23 embedding support. Unset an old custom DLL override to use automatic compilation. `scripts/build-llvm-native.ps1 -ProgramDirectory <generated-build-directory>` can rebuild a matching module; without that option the script builds only a generic diagnostic reference.

Strength histograms retain 32-bit contribution counts and store an occupied-level mask in unused bucket zero, so finding the maximum does not scan every level. Generated table access preserves the existing update order and operation-budget accounting.

The Windows circuit build uses `-std=c23 -O3 -ffreestanding -fno-builtin -Werror --target=x86_64-pc-windows-msvc`. `engine.c` includes both C implementation files and the generated header, so optimization sees one translation unit; linking uses `lld-link /dll /noentry /nodefaultlib /machine:x64`. There is no `-march=native`, CPU tuning, LTO or profile-guided optimization in this native build. The inspected IR targets generic x86-64 with SSE2. Rust release-build settings do not supply additional optimization flags to this separate compiler invocation.

Optimized circuit IR references immutable globals directly and folds table dimensions, reactive-mode selection and deferred-wire selection. Dynamically indexed table records still require loads and dispatch: a constant array does not make an index selected by scheduling or movement constant. Creation validates IDs, ranges and table relationships, but those proofs are not automatically carried across exported calls into subsequent execution. Signal bytes provide an eight-bit type bound; they do not by themselves tell LLVM that every signal is at most fifteen. Component state also contains flags and cannot universally be treated as a Boolean.

Runtime ownership guarantees exclusive synchronous mutation, stable instance allocations and an independent clock context. However, an engine pointer alone does not express disjointness among its pointed-to buffers. The count and histogram arrays occupy separate regions established by creation; scoped contracts on those regions can supply alias information. This does not make updates to the same receiver independent, nor make scheduler queues, histories, group indexes or button releases constant within a batch. Clock callbacks must retain their real effects and per-tick timing; ordinary FFI function-pointer types do not communicate the Rust callback's limited memory access.

The Windows target also defaults to relaxed aliasing: the compiler's expanded invocation contains `-relaxed-aliasing`, and the inspected baseline IR has no type-based alias metadata. This matches [Clang's documented MSVC-target default](https://clang.llvm.org/docs/UsersManual.html#strict-aliasing). An isolated `-fstrict-aliasing` build emits that metadata, but enabling it for production requires checking all buffer layouts, typed views and FFI accesses against the stronger contract; passing a finite trace alone is not that proof.

IO publication narrows export work, not the state the simulator must preserve. Unpublished components can influence later outputs, and reset still exports complete state and pending work. The optimizer therefore cannot discard their updates merely because the next publication requests IO. Likewise, operation limits, movement-transition checks and command-capacity retries protect observable failure behavior and successful prefixes.

Current IR, isolated contract/code-size experiments and measured results are recorded in [the LLVM contract investigation](../test_data/cpu-references/llvm_contract_investigation.json). Code-size reductions and optimization remarks are evidence for an experiment, not throughput results.

The first November 12 contract/code-size phase retained no additional native change. A rolled batch loop reduced machine code from 110,096 to 47,744 bytes, but its initial gain reversed to -0.58% over five confirmation pairs. Explicit count/histogram alias metadata gave no repeatable gain. Strict aliasing measured approximately +2% in three ZCore pairs but needs a complete contract audit. Specializing solely on a zero static command count looked faster, but a direct test found that the existing ABI permits binding an empty command engine: the specialization incorrectly accepted a missing clock and skipped observable callbacks. Normal Rust callers avoid that case; the exported boundary still needs its fallback.

The semantic-38 live relaxed runs used three alternating samples, a compressed TCP client and a 10 Hz publication cap. Median simulation-loop TPS, excluding compilation, final state verification and delivery/writer drain, was:

| Circuit / publication | Batch 16 | Batch 32 | Batch 64 |
| --- | ---: | ---: | ---: |
| ZCore / disabled | 5,512 | 5,475 | 5,441 |
| ZCore / IO | 5,408 | 5,461 | 5,407 |
| PM1 / disabled | 5,262 | 5,290 | 4,755 |
| PM1 / IO | 5,120 | 5,286 | 4,606 |

All variants matched final outputs; every PM1 run matched the reference RAM/lamps and 1,534 ordered messages including `shut`. IO publication ran at roughly 9.7–9.95 Hz. These measurements support retaining the production batch default of sixteen; they do not establish that a larger batch improves throughput generally. Lower configured publication cadence remains supported.

Boolean-only input channels keep active counts and contribution bytes while omitting strength histogram writes. Wires and comparators retain analog input accounting. Eligible relaxed circuits use precomputed dependency masks for bounded, ordered movement callback plans; conflicting shared layouts and plans over 64 steps retain the ordered fallback.

The relaxed replay graph can omit updates to unwatched wires with immutable sources and no electrical, routed, reactive or side-input consumers, subject to a bounded movement-group lookup. Reads and full snapshots reconstruct their state. Generated filtered ranges sit beside movement cases and fixed rules to avoid dependent prefix lookups. Original operation costs and record order remain intact: before a range can fail partway, state is materialized and canonical records are replayed. Oversized ranges retain the existing format.

Logical work counts include skipped records to preserve operation limits and successful prefixes; they are not CPU instruction counts. IO publication does not by itself prove that unpublished state is unnecessary for later execution or reset.

## Limits

Unsupported components, piston arrangements, unfinished movement or dependencies outside the captured region can prevent compilation. The LLVM backend has additional restrictions.

Tests cover specific mechanisms and circuits. Passing compilation does not prove that every circuit will match the interpreter or vanilla Minecraft.

Compilation and execution share the server process. A native fault or an OS OOM kill can still terminate the server.

## Source files

| File | Purpose |
| --- | --- |
| [Plot compilation](../crates/core/src/plot/redpilator_compilation/mod.rs) | Snapshot, worker thread, progress and activation |
| [Compiler](../crates/core/src/redpilator/compiler.rs) | Executor ownership, status and public API |
| [Compilation](../crates/core/src/redpilator/compiler/compilation.rs) | Entry checks, cache restoration, program building and backend activation |
| [Execution dispatch](../crates/core/src/redpilator/compiler/execution.rs) | Ticks, batches and external controls |
| [Lifecycle](../crates/core/src/redpilator/compiler/lifecycle.rs) | Export, reset and suspend |
| [Compiler diagnostics](../crates/core/src/redpilator/compiler/diagnostics.rs) | Errors, recovery hints and compile statistics |
| [Scratch world](../crates/core/src/redpilator/scratch.rs) | Isolated interpreter snapshots, bounded probing and restoration |
| [Electrical discovery](../crates/core/src/redpilator/scratch/discovery.rs) | Sources, wire ports and watchers |
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
