# G2 — General CompileGraph and artifact export

Status: proposed; requires reviewed G1. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md).

## Goal

CompileGraph is the complete pre-optimization circuit representation. Required parsing, optional rewrites, validation, export and lowering are visible stages of the compiler pipeline.

## Scope

1. Promote `CircuitDraft` rather than forcing rich semantics into today's main/side `StableGraph`. Rename the old representation to `LegacyCompileGraph` while legacy consumers remain. Update callers/types together; do not introduce a third graph vocabulary.
2. Keep descriptions and initial values owned once. Entry clock/ticks are staged separately and lowered once. Preserve all typed power/update/shape links, contact alternatives, guards, ordered routes, wire plans, comparator channels, resources and provenance.
3. Put identification/geometry, electrical discovery and callback routes in the required front-end phase of the pipeline. Do not turn `identify_nodes` into one giant parser. Make dependencies and validated stage output explicit. Graph transformation API is a separate G3 deliverable.
4. Build the same complete graph with optimization enabled or disabled. The old identification-time wire elision stays confined to the temporary legacy path until a reviewed replacement exists. General passes must not call legacy rewrites directly on rich data.
5. Retain current legacy behavior during coexistence. General rewrites remain disabled until compatible implementations are assigned; record this truthfully. G3 will make transformation selection/order extensible, rather than duplicating the old hardcoded runner.
6. Export the actual artifact after optional rewrites and before executable lowering. Adapt existing JSON DTO conversion and preserve version/stage/capability/provenance contracts. The standalone inspection command uses this same pipeline.
7. Keep legacy binary/DOT support for representable artifacts; report unsupported rich exports explicitly. Do not erase callback/contact information to fit an old format, or write an empty electrical graph.
8. Validate before and after rewrites, then lower dense IDs/indexed input ranges once. Executable capability admission remains distinct from successful parser inspection.

## References

`compile_graph.rs`, `program_graph.rs`, all `passes/`, `analysis/graph.rs`, `analysis/export_json.rs`, `docs/notes/redpiler-pistons-v4/GRAPH_JSON.md`, `plot/commands.rs`, `redpiler/mod.rs`, graph/export tests, Side B legality research.

## Required evidence

- With and without `--optimize`, pre-rewrite general graphs retain dust and ordered routes for an ordinary comparator circuit and a shared-payload fixture.
- Export reflects a real applied local rewrite where one is already supported; otherwise verify unchanged graph identity and truthful skipped-pass reporting. Do not invent a rewrite solely for this test.
- Deserialize exports and check full typed references/route order against the in-memory artifact. Existing viewer/export tests remain compatible or receive a deliberate schema migration.
- Strength-15 attenuation, duplicate callbacks, guards, contact selection and pending initial work survive pipeline/lowering.
- Legacy ordinary compilation and existing explicit export rejections remain covered.

## Non-goals

No broad runtime activation, packed lowering of rich relations, physical fallback, or aggressive dust elimination. Stop with one coherent compilation representation and reviewable exporters.
