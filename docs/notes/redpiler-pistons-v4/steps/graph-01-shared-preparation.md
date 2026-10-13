# G1 — Shared parser orchestration

Status: proposed; implementation stops after this gate. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md) and `../AGENT_GUIDE.md` first.

## Goal

Execution and read-only JSON inspection invoke one ordered geometry/electrical/callback preparation sequence owned by `redpiler::passes`. Backend construction no longer discovers circuit topology.

## Scope

1. Inspect every caller of `GraphBackend::compile`, `export_json::prepare`, and the three parser functions. Preserve test entry points through shared helpers rather than duplicating the preparation sequence.
2. Complete or replace the provisional `passes/rich.rs`. Keep geometry, electrical and callback parsing as separate required stages with existing budgets, cancellation, structured witnesses and normalized bounds. They share the compiler's monitoring; their input need not match later graph-pass input.
3. Move graph assembly out of the backend. Production backend construction takes validated program/state and prepared entry metadata; parser errors belong to compilation. Keep opaque public errors if internal types must remain private.
4. Make export consume the same preparation result. Inspection may export a parser-stage artifact even when execution capability checks reject it; record its actual stage. Do not start another parser merely to export a graph already prepared by compilation.
5. Record pass counts/duration/progress truthfully. Do not claim an optimization ran or dust was eliminated when it was not. Distinguish parser completion from executable lowering.
6. Keep selection, Native, existing ordinary Direct, command/note policy and circuit semantics unchanged.

## References

`passes/mod.rs`, provisional `passes/rich.rs`, `backend/graph.rs::compile`, `analysis/export_json.rs::{prepare,from_drafts}`, `program_graph.rs::CircuitDraft::lower`, `redpiler/mod.rs::Compiler::compile`, `task_monitor.rs`, JSON tests and graph test helper call sites.

## Required evidence

- Existing geometry, electrical, callback, graph validation and export tests pass through the shared path where relevant.
- Add a preparation/export agreement test retaining IDs, guarded channels, ordered duplicate deliveries and initial resource selections, not only table counts.
- Cancellation and unsupported geometry leave input blocks/entities/ticks unchanged and publish no artifact/backend.
- A real sticky-piston fixture and an ordinary dust/comparator fixture prepare identically before and after extraction; run their existing executor episodes to check a purely structural refactor.
- Verify discovery before execution. Run focused rustfmt, `git diff --check`, `cargo check -p mchprs_core --tests --locked`, and relevant release-locked tests. Record actual counts and failures.

## Non-goals

No executor merge, new optimization, Native removal, broad identifier rename, or change to runtime admission. This step is complete only when both consumers use shared orchestration, not when an unused module exists.
