# Redpiler

Redpiler reads the selected world once, builds a directed electrical graph, and
executes that graph with the direct backend. Links carry attenuation: a source
of strength 15 through a link of weight 3 supplies strength 12.

Use `/rp compile --assume-instant` for ideal piston logic without movement delays
or reset pulses. Memory still holds its value between clock samples; ordinary
redstone components retain their timing. Independent adders and counters may
share a plot, with a separate runtime per connected instant region.
`/rp analyze --assume-instant` checks the same compilation path without activating
it. Unknown flags return an error. See the
[instant runtime contract](INSTANT_PISTON_RUNTIME.md#independent-builds-and-ideal-timing-2026-10-07)
for entry limits and state restoration.

## Reading the code

Start with [`Compiler`](../crates/core/src/redpiler/mod.rs). Compilation analyzes
live geometry, prepares the graph, and builds a fresh backend. The compiler
publishes that backend only after every step succeeds. An absent backend means
an inactive compiler; failed or cancelled compilation leaves the world alone.

Ordinary circuits use [`passes::run_passes`](../crates/core/src/redpiler/passes/mod.rs).
Piston circuits first build an
[instant program](../crates/core/src/redpiler/instant/program.rs), then use the
same graph passes with region boundaries. Boundaries keep moving payloads and
reset internals out of ordinary graph optimizations. A candidate analysis graph
is diagnostic data; it is not an executable piston program.

The passes run in this order. Their enable conditions live together in
`passes/mod.rs`; each pass module contains its graph transformation.

| Pass | When | Purpose |
| --- | --- | --- |
| Identify nodes | Always | Create components and boundary nodes; omit display wires with `--optimize`. |
| Search inputs | Always | Find electrical sources and wire distances. Stop wire searches at attenuation 15. |
| Clamp weights | Always | Remove links with attenuation 15 or greater. |
| Deduplicate links | `--optimize` | Keep the shortest link for each source, target and input channel. |
| Fold constants | `--optimize` | Replace settled constant-driven diodes and torches without losing pending transitions. |
| Prune unreachable outputs | `--optimize` | Remove comparator links that cannot carry nonzero power. |
| Coalesce constants | `--optimize` | Share equal removable constants within each connected component. |
| Coalesce logic | `--optimize` | Merge eligible nodes with the same source, type and state. |
| Prune orphans | `--optimize --io-only` | Retain input/output nodes, pending work, region boundaries and their dependencies. |
| Export graph | `--export` | Write `redpiler_graph.bc`; unsupported region and command-block exports return errors. |

## Execution and performance

The [direct backend](../crates/core/src/redpiler/backend/direct/mod.rs) lowers the
compile graph into a fixed array of packed nodes. Compilation validates strengths
and input counts before runtime code uses unchecked indices. Keep the
[node layout](../crates/core/src/redpiler/backend/direct/node.rs), packed update
links and aligned strength counters small: they are used on every update.

Each input channel has sixteen counters. Counter `s` records how many incoming
links currently supply strength `s`. When a source changes, the backend adjusts
the affected counters and calls `update::update_node`. This avoids searching the
world or scanning all incoming links during execution.

An **update** calculates whether a component needs a future tick. A **tick**
applies that change and propagates its output. `tick.rs` and `update.rs` keep
these responsibilities separate because repeater locking, pulse extension,
comparator overrides and lamp delays depend on their order. The shared
scheduler preserves half-tick deadlines, priority and FIFO order.

Instant regions evaluate compiled Boolean decisions and expose electrical
outputs to ordinary nodes. Their runtime also retains launch inputs and player
actions for physical replay when returning control to the interpreter.
With `--assume-instant`, reset instead writes current stationary logical geometry
and stored memory; it does not replay physical reset waves.
`flush` writes visible changes and processes output events. `reset` writes back
hidden strengths, materializes region geometry and returns pending ticks to the
world before dropping the backend.

## Checks

Run behavior tests from the repository root:

```sh
cargo test -p mchprs_core --lib redpiler
```

Run the existing tick benchmark from `crates/core` so its plot file is found:

```sh
cargo bench -p mchprs_core --bench chungus -- chungus-mandelbrot-tick
```
