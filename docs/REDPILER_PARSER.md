# Redpiler analysis and graph preparation

Redpiler has no textual circuit-language parser. Its input is a live world:
block positions and states, block entities, and scheduled ticks. Command flags
are parsed by `CompilerOptions`; spatial analysis inventories the selected
world and prepares an electrical graph.

## Analysis and compile admission

[`analysis::analyze`](../crates/core/src/redpiler/analysis/mod.rs) reads the
selected bounds, checks resource limits and loaded chunks, and inventories
blocks, pending work, and piston geometry. This analysis is read-only: it does
not move blocks, consume scheduled ticks, or transfer world ownership.

Pistons are recorded for diagnostics. If any piston is present,
[`Compiler::compile`](../crates/core/src/redpiler/mod.rs) rejects the build
before graph preparation with `build contains pistion, pistions are not
supported`. The compile error points to a piston in the selection. The
interpreter remains responsible for piston execution.

Analysis limits include inspected cells, piston inventory, and dependency
steps. Exceeding a limit or cancelling analysis returns an error rather than a
partial compile. `AnalysisReport` and `CandidateGraph` are diagnostic results;
neither is an executable backend.

## Ordinary electrical graph

For an admitted piston-free selection, graph preparation identifies component
nodes from saved block state and searches spatial power paths. The resulting
links retain input channel and attenuation. Scheduled ticks are validated
against current block types before being transferred.

The graph pass sequence is defined in
[`passes::prepare`](../crates/core/src/redpiler/passes/mod.rs). It identifies
nodes, searches for links, clamps invalid weights, then optionally performs
guarded optimization passes and export. Selections with comparator ordering
that cannot be represented by the direct graph use the native ordinary
execution path when allowed.

Lowering converts the prepared graph into either the direct backend or the
native ordinary backend. See the [architecture](REDPILER_ARCHITECTURE.md) for
command behavior, supported options, and test locations.
