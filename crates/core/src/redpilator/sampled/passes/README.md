# Sampled compiler passes

`compiler.rs::compile_monitored_with_source_cache` runs the ordered pipeline.
`compile_monitored` starts it without prior sampling artifacts. Passes run only
during compilation; the runtime and public `Compiler` API are unchanged.

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

Parsing, discovery and instruction rewrites are sequential calls in
`compiler.rs::compile_monitored_with_source_cache`. Add each new pass directly to that pipeline
and keep its implementation in its own module. Each rewrite checks cancellation
before and after its work and validates the resulting program.

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
