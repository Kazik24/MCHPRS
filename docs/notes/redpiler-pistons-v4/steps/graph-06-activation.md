# G6 — Activate Graph and retire legacy backends

Status (2026-10-13): implemented for the retained supported subset and awaiting human review. Public compiled execution uses Graph only; Direct/Native and their obsolete compilation path are retired. Broader behavioral histories remain unverified. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md).

## Goal

Supported ordinary and piston circuits compile through the common graph pipeline and execute through Graph. Explicitly unsupported components and entry states fail before publication. The interpreter remains available as the reference executor.

## Scope

1. Switch reviewed supported cohorts in `Compiler::compile`, rather than assigning all comparators/cycles to Native. Preparation and backend construction occur before ownership/publication changes.
2. `Runtime::Graph(Box<GraphBackend>)` is the sole variant. Backend reporting, work counters, command help, diagnostics and tests describe Graph. A one-variant enum avoids unnecessary host changes.
3. Supported circuits select Graph; unsupported circuits fail before publication. No compiled path selects another executor after Graph preparation or execution fails.
4. Retire Direct/Native production implementations and legacy-only graph plumbing when references are migrated. Preserve meaningful regression scenarios using the interpreter as oracle; do not delete failing tests or retain snapshots as hidden Graph simulators. Never move native redstone execution calls into Graph.
5. Commands and notes remain implemented in Graph. Hoppers and trapdoors return structured rejections. Legacy binary/DOT export flags return a clear prepublication format error; JSON remains the supported inspection format.
6. Ensure compiler/reset lifecycle and host effects remain transactional. Do not log execution errors and silently continue in a corrupted state; retain the project's reviewed error policy and expose defects clearly.

## References and required evidence

`redpiler/mod.rs`, `backend/mod.rs`, runtime callers in `plot/`, compiler admission/continuation tests, `analysis/tests/redstone_fuzz.rs`, Graph tests, command/export diagnostics, legacy-only references in scheduler/benchmarks/exports.

- Public compile/use/tick/flush/reset tests assert Graph for a comparator circuit, feedback circuit and supported piston fixture, in applicable optimized/unoptimized and `io_only` modes. Test every explicitly rejected legacy capability's error and input preservation.
- Failed preparation leaves world/entities/scheduler ownership and compiler activation unchanged.
- Reset then interpreted execution agrees with uninterrupted interpretation, including hidden components, comparator entities, pending ticks and piston work.
- Existing ordinary regression scenarios pass through Graph or receive their specifically reviewed rejection contract. Check that no production caller, benchmark or exporter still depends on retired backend types. No Graph execution test can pass by selecting a legacy simulator.
- Relevant server/core checks and test discovery pass; update public API consumers when reporting enums change.

## Non-goals

No native fallback, incidental command/note policy change, or mandatory performance target. Human review still checks the implemented scope and the documented untested histories; no further executor or optimization work is included here.

## Activation and retirement evidence

Public compiler and runtime consumers now use the common Graph pipeline. Search and test discovery found no production `DirectBackend`, `NativeBackend`, `LegacyCompileGraph`, legacy pass entry point, or related backend variant. Read-only analysis uses the same prepared graph. JSON inspection remains supported; `--export` and `--export-dot` fail explicitly before activation because their format cannot preserve routes, contacts, and callback semantics. Selected Hoppers/trapdoors, moving entry, malformed piston heads, support loss, unloaded targets, and unsupported external wire-boundary queries retain structured admission errors.

`cargo test -p mchprs_core --lib redpiler:: --locked -- --list` discovered 202 redpiler tests. Debug and release each passed 198 with 4 ignored. The ignored release CPU comparison passed for 100 active ticks and matched Graph/interpreter RAM. The public release benchmark asserted Graph identity and ran five fresh pairs: Graph median 1.284687 s (1.241218–1.301638 s), interpreter median 0.649654 s (0.642933–0.695880 s). Work counts matched each run. This demonstrates a short episode only; it does not establish full CPU-history equivalence. See the migration report for exact commands, counts, table estimates, and remaining limits.
