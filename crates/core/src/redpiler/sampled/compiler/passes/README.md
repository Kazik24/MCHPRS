# Sampled compiler passes

`compiler.rs::compile_monitored` is the ordered pipeline. Passes run only during
compilation; the runtime and public `Compiler` API are unchanged.

| Pass | Input | Result |
| --- | --- | --- |
| `parse` | Captured world, bounds and entry work | Validated component/piston/token inventory and isolated scratch state |
| `electrical` | Inventory and component index | Electrical links, component ports, watched wires, fixed geometry and probe indexes |
| `movement` | Program and probe indexes | Sampled departure/arrival recipients and ordered movement steps |
| `arrangements` | Program, world and probe indexes | Joint conditional inputs, movement flips and motion-dependent admission checks |
| `source_order` | Discovered program and isolated scratch probes | Separate rise/fall programs and existing scratch-frame comparisons |
| `reactive` | Loaded state and discovered dependencies | Reactive component flags and wire/neighbor update relationships |
| `finalize` | Complete discovered program | Validated records and runtime/projection indexes |
| `share_output_programs` | Validated instructions | Shared identical immutable rise/fall ranges, preserving component identity |

Parsing, discovery and instruction rewrites are sequential calls in
`compiler.rs::compile_monitored`. Add each new pass directly to that pipeline
and keep its implementation in its own module. Each rewrite checks cancellation
before and after its work and validates the resulting program.

Finalization prepares indexes before instruction sharing, preserving the existing
compilation order. Rewrites that change component/link IDs or dependencies must
also rebuild the affected indexes before validation; instruction storage sharing
does not change them. Scratch-frame comparisons remain part of source ordering;
their diagnostic behavior has not changed in this refactor.

Existing phase timing and cancellation checks remain at the discovery boundaries.
Optimization timing has its own pass name. There are no runtime pass dispatches,
new compile flags or additional dependencies.
