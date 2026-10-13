# Instructions for the implementation agent

This document and one assigned step form the implementation prompt for a Luna6 agent. Read [the index](README.md), [findings](FINDINGS.md), [architecture](ARCHITECTURE.md), and the assigned step before editing. Read applicable AGENTS.md files if the checkout provides them.

## Assignment contract

Implement only the assigned step. Its prerequisites must already be reviewed; an unreviewed prerequisite is a reason to report the dependency, not implement several steps. After the step is concrete and tested, stop for human review. Do not start the next step automatically.

The design targets the repository's interpreter behavior for the admitted subset. Names in the architecture are shape sketches; adapt them to existing conventions while preserving semantic ownership. If a shape cannot express a discovered case, describe a concrete trace and propose the smallest correction. Do not substitute a different execution contract.

## Requirements applying to every step

- Runtime simulation uses graph state and compiled references. No World simulation adapter, spatial get_block/get_chunk reads, physical piston executor, native dust executor, native propagation fallback, or assumed ideal gate response.
- The World remains available at compile, host-output, and materialization boundaries. Host command output may be synchronous; its result must feed graph state at the correct boundary.
- Keep normal/QC power, callback delivery, queued requests, acceptance, and settled state separate.
- Abstract physical movement away. Use compact piston availability, selected contacts, shared resources, and necessary timed graph events. No Motion type, progress/previous_progress fields, carried-block model, mutable world-cell array, or physical movement loop belongs in execution.
- Preserve live-versus-captured operands, direction, order, duplicates, phase, and position-bound scheduled work.
- Shared payloads become shared graph resources with one authoritative contact selection. A piston does not permanently own its private copy.
- Stationary entry is required. Ordinary pending ticks and saved strengths are imported without eager normalization.
- Failure and cancellation preserve the input world, scheduler, and piston state. No partially active backend or published success statistics.
- Parser completeness, abstraction sufficiency, and resource exhaustion have explicit errors. Do not interpret out-of-footprint contacts as air.
- Generalized dust exists before any dust-elimination pass. Keep legacy ordinary backends working during migration; retire Direct/Native only at the reviewed activation gate in [the common graph migration](GRAPH_PIPELINE_MIGRATION.md). Graph remains the target executor; do not move it into Direct.
- Use the general graph-pass API for transformations. Developer-defined passes and configurable order/enable states must not require runner changes or a closed pass enum. Parser stages and outputs retain their required boundaries; accepted reordered pipelines still require behavioral verification.
- New edge families must not accidentally enter old electrical-only rewrites, cycle guards, packed link encoding, or binary export.
- An added observer or analog consumer can invalidate an optimization's observability assumptions.

## Ponytail and readability

Use the smallest complete change. Reuse current helpers, scheduler containers, block/property types, fixtures, and test harnesses. Add no dependency, option, abstraction layer, generic bytecode VM, worker pool, or cache without a demonstrated need in this step.

Before editing, identify every affected caller, enum match, test, fixture, config boundary, and export. For a bug fix, search all callers and fix the shared cause. Use rg first. Preserve concurrent user changes.

Write functions around semantic operations: read inputs, choose an action, validate an event, select contacts, deliver a route, apply a delayed connection event. Use named intermediate values for geometry, signal strength, eligibility, and phase. A few clear statements are preferable to a dense nested expression.

Do not build a large parser or executor function that mixes inventory, inference, scheduling, and publication. Do not split a few obvious statements into layers of generic wrappers. Introduce a helper when it names a real operation or is reused.

Document only the why the code cannot show. A known supported limit needs `shortcut: <limit>, <when to upgrade>` near the enforcing code. Rejected mechanics belong in structured diagnostics, not swallowed errors or fixture-name exceptions.

Start with ordinary checked indexing and readable arrays. Validate graph IDs and references at lowering. Do not copy unchecked packed-backend indexing before profiling and review justify it. Avoid parallelism and lock/atomic fields in this serial model.

Explain every retained runtime field with a concrete query, pending event, or continuation obligation. Geometry positions, block templates, and parser certificates are immutable metadata; do not consult them to rediscover the circuit during execution. Parser sophistication must not turn into a large physical runtime.

## Source navigation

| Responsibility | Existing source |
| --- | --- |
| Compiler admission/publication | [redpiler/mod.rs](../../../crates/core/src/redpiler/mod.rs), [analysis](../../../crates/core/src/redpiler/analysis/mod.rs) |
| Read-only candidate | [analysis/graph.rs](../../../crates/core/src/redpiler/analysis/graph.rs) |
| Graph vocabulary | [compile_graph.rs](../../../crates/core/src/redpiler/compile_graph.rs) |
| Passes and wire elision | [passes](../../../crates/core/src/redpiler/passes/mod.rs), [identify_nodes](../../../crates/core/src/redpiler/passes/identify_nodes.rs), [input_search](../../../crates/core/src/redpiler/passes/input_search.rs) |
| Existing fast backend | [direct](../../../crates/core/src/redpiler/backend/direct/mod.rs), [packed nodes](../../../crates/core/src/redpiler/backend/direct/node.rs) |
| Runtime selection and queues | [backend](../../../crates/core/src/redpiler/backend/mod.rs), [queue](../../../crates/core/src/redpiler/backend/queue.rs) |
| Reference power and callbacks | [power](../../../crates/core/src/redstone/power.rs), [redstone](../../../crates/core/src/redstone/mod.rs) |
| Reference dust | [wire](../../../crates/core/src/redstone/wire/mod.rs), [Turbo](../../../crates/core/src/redstone/wire/turbo.rs) |
| Reference piston behavior | [piston](../../../crates/core/src/redstone/piston.rs), [world types](../../../crates/world/src/lib.rs) |
| Support and interaction | [interaction](../../../crates/core/src/interaction.rs) |
| Activation, tick, and reset host | [plot](../../../crates/core/src/plot/mod.rs), [commands](../../../crates/core/src/plot/commands.rs), [packet handlers](../../../crates/core/src/plot/packet_handlers.rs) |
| Legacy binary graph | [redpiler_graph](../../../crates/redpiler_graph/src/lib.rs), [export pass](../../../crates/core/src/redpiler/passes/export_graph.rs) |

## Validation and review report

Every new branch, parser stage, scheduler operation, or transition needs a small meaningful check. Prefer existing fixtures and helpers. Tests must demand admitted graph execution when that is the claim; a rejection log, zero-test run, or native replay is insufficient.

Use targeted release tests with the lockfile, generally `cargo test -p mchprs_core --release --locked <actual_filter> -- --test-threads=1`. First verify discovery with `-- --list`. Proposed future test names in step files are not existing executable tests. Run relevant broader checks after integration, not the whole expensive suite for every small edit. Use the root Config.toml if fixture import limits require it; do not alter references or configs to make a failure disappear.

At the end of the assigned step, provide:

1. The implemented behavior and affected files.
2. The invariants now enforced and any intentional unsupported cases.
3. Exact commands, discovered/executed test counts, and results.
4. One short explanation of the most important trace or failure witness.
5. Remaining risks and proposed next step, without implementing it.

End with one or two lines naming what was skipped or not checked and the material risk. Human review approves the next step.
