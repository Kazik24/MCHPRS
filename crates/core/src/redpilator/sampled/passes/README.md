# Sampled compiler passes

Start at [`compiler.rs::compile_monitored_with_source_cache`](../compiler.rs).
It runs the pipeline below in order and returns the finished program.
`compile_monitored` starts the same pipeline without prior sampling artifacts.
Passes run only during compilation; the runtime and public `Compiler` API are unchanged.

Each world-discovery pass takes the same captured `Context` and phase reporter.
`Context` holds the original world, capture bounds, pending work and cancellation
handle. The passes fill in `Program`, which stores component IDs (indexes into
`positions`) and instructions. `ProbeIndexes` maps world positions back to those IDs.
The last two passes need only the program and phase reporter.
Parsing returns `ParsedCircuit` (scratch snapshot and inventory); electrical
discovery returns `ElectricalCircuit` (program and probe indexes). Repeater rear
power discovery and block-entity admission each have one helper owning their rules.

After compilation, [`Runtime::tick`](../executor.rs) runs scheduled updates,
then piston events and movement. Its tick and input entry points appear before
the component-specific mechanics. Publication back to the world is handled by
the public [`Compiler`](../../compiler.rs), whose world writes and motion-identity
reconciliation live in [`compiler/publication.rs`](../../compiler/publication.rs).

| Pass | Input | Result |
| --- | --- | --- |
| `parse` | Captured world, bounds and entry work | Validated component/piston/token inventory and isolated scratch state |
| `electrical` | Inventory and component index | Electrical links, component ports, watched wires, fixed geometry and probe indexes |
| `movement` | Program and probe indexes | Sampled departure/arrival recipients and ordered movement steps |
| `arrangements` | Program, world and probe indexes | Joint conditional inputs, movement flips and motion-dependent admission checks |
| `reactive` | Loaded state and discovered dependencies | Reactive component flags and wire/neighbor update relationships |
| `source_order` | Discovered program, isolated scratch probes and optional prior certificates | Separate rise/fall programs, validated callback-trace reuse and selected scratch-frame comparisons |
| `finalize` | Complete discovered program | Validated records and runtime/projection indexes |
| `share_output_programs` | Validated instructions | Shared identical immutable rise/fall ranges, preserving component identity |

Parsing, discovery and instruction rewrites are sequential calls in the compiler.
Add each new pass directly to that pipeline
and keep its implementation in its own module. Each rewrite checks cancellation
before and after its work and validates the resulting program.

[`mod.rs`](mod.rs) holds shared pass inputs, progress reporting, cancellation
checks and callback notification order. [`pistons.rs`](../pistons.rs) owns piston
records, payload discovery and grouping. The program and runtime use those
records directly, without depending on the compiler entry point. Parsing shares
its scratch snapshot with piston discovery instead of capturing the world twice.

[`movement.rs`](../movement.rs) owns movement-table records, `compile_movement_flips`,
its lowering helpers and its validation. This pass directory samples interpreter
behavior; the shared movement module turns those observations into executable tables.
[`program.rs`](../program.rs) owns the general program model and indexes;
[`executor.rs`](../executor.rs) owns mutable runtime initialization and execution.
Publication projection, scheduler import/export and diagnostics live in
[`publication.rs`](../publication.rs), [`scheduler.rs`](../scheduler.rs) and
[`diagnostics.rs`](../diagnostics.rs), respectively.

The [complete function and type map](../../../../../../docs/REDPILATOR_CODE_MAP.md)
lists the owners and entry points for Rust, native code and plot integration.

Finalization prepares indexes before instruction sharing, preserving the existing
compilation order. Rewrites that change component/link IDs or dependencies must
also rebuild the affected indexes before validation; instruction storage sharing
does not change them. Source-order cache entries store positions and callback
observations, so reuse lowers operations through the fresh indexes. Required
scratch-frame comparisons remain part of source ordering; full block/pending
frames are captured only when those comparisons consume them. Other repeated
transition diagnostics compare callback observations.

Existing phase timing and cancellation checks remain at the discovery boundaries.
Optimization timing has its own pass name. There are no runtime pass dispatches,
new compile flags or additional dependencies.

Native encoding keeps record fields named until serialization: component kind/state,
operation opcode/operand, command notification targets and local routing ports.
The byte layout and table IDs remain defined by the native ABI. Scheduler iterator
entries name their delay in half-ticks; `schedule_tick` accepts whole redstone ticks
and converts them at the scheduler boundary.
