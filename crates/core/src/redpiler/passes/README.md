# Redpiler graph passes

The compiler keeps the parsed circuit and its physical state owners in the Graph
backend. The current pass pipeline prepares electrical geometry, callbacks,
wire queries and shapes, then lowers the graph to the physical runtime.

## Active passes

`passes/mod.rs` defines the ordered defaults. They prune inert work, simplify
generic state guards, compile classic redstone and fixed signals, and prepare
wire callbacks, query facts, shapes, and captures. O5 can coalesce equivalent
delayed updates. `--optimize` selects the normal optimized pipeline.

The test-only `experimental_graph_probe_pipeline` is a small comparison
pipeline for graph probing. It is not selected by production options.

## Pass rules

- A rewrite changes the graph and clears derived execution plans.
- A builder reads the graph and publishes a typed execution plan.
- Validation must reconstruct any plan from the source graph before lowering.
- Physical state, scheduled actions and wire traversal remain owned by Graph.
- Unsupported or unproven cases keep the general physical execution path.

Instant and settled logical execution plans, stable-input options, and their
specialized pass implementations were removed. Their historical research notes
are not part of the current compiler pipeline.
