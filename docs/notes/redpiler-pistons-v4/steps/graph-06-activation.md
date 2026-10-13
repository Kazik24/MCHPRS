# G6 — Activate Graph and retire legacy backends

Status: proposed; requires reviewed G5 for every cohort being activated and explicit disposition of remaining legacy capabilities. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md).

## Goal

All supported ordinary and piston circuits compile through the common graph pipeline and execute through Graph. Retire production Direct/Native after coverage and admission policy are reviewed. Keep their code until this gate is ready; do not delete an implementation merely to force tests onto Graph.

## Scope

1. Switch reviewed supported cohorts in `Compiler::compile`, rather than assigning all comparators/cycles to Native. Preparation and backend construction occur before ownership/publication changes.
2. Keep `Runtime::Graph(Box<GraphBackend>)` as the sole variant if it minimizes host changes; remove Direct/Native variants and selection once all retained supported cohorts have migrated. Update backend reporting, work counters, command help, diagnostics and tests to describe the actual executor. A one-variant enum is acceptable; do not add a new wrapper abstraction.
3. Supported circuits select Graph; unsupported circuits fail before publication. Do not select Direct/Native after Graph preparation or execution fails. During reviewed intermediate activation, remaining legacy cohorts are explicit; this step is not complete while any production selection uses them.
4. Retire Direct/Native production implementations and legacy-only graph plumbing when references are migrated. Preserve meaningful regression scenarios using the interpreter as oracle; do not delete failing tests or retain snapshots as hidden Graph simulators. Never move native redstone execution calls into Graph.
5. Before deleting legacy paths, settle command/note treatment explicitly: Graph support or a reviewed structured compilation rejection reflecting the requested future policy. Check trapdoors and every other legacy capability separately. Preserve existing export behavior or document deliberate format compatibility changes. Unresolved policy blocks retirement, not earlier refactoring/testing.
6. Ensure compiler/reset lifecycle and host effects remain transactional. Do not log execution errors and silently continue in a corrupted state; retain the project's reviewed error policy and expose defects clearly.

## References and required evidence

`redpiler/mod.rs`, `backend/mod.rs`, runtime callers in `plot/`, compiler admission/continuation tests, `analysis/tests/redstone_fuzz.rs`, Graph tests, command/export diagnostics, legacy-only references in scheduler/benchmarks/exports.

- Public compile/use/tick/flush/reset tests assert Graph for a comparator circuit, feedback circuit and supported piston fixture, in applicable optimized/unoptimized and `io_only` modes. Test every explicitly rejected legacy capability's error and input preservation.
- Failed preparation leaves world/entities/scheduler ownership and compiler activation unchanged.
- Reset then interpreted execution agrees with uninterrupted interpretation, including hidden components, comparator entities, pending ticks and piston work.
- Existing ordinary regression scenarios pass through Graph or receive their specifically reviewed rejection contract. Check that no production caller, benchmark or exporter still depends on retired backend types. No Graph execution test can pass by selecting a legacy simulator.
- Relevant server/core checks and test discovery pass; update public API consumers when reporting enums change.

## Non-goals

No new native fallback, incidental command/note policy change, or mandatory performance target. Retirement is authorized only by this step's reviewed scope/coverage gate. Report remaining legacy production cohorts explicitly; their presence means this step is incomplete.
