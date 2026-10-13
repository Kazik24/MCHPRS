# Luna6 — efficient Graph execution and retirement of legacy backends

## Assignment and authorization

Implement this batch in the existing MCHPRS worktree. The user requests both:

1. Optimize and simplify the general Graph executor without changing its observable behavior.
2. Make Graph the sole compiled executor and remove Direct, Native, and their obsolete compilation infrastructure.

This is authorization to implement both parts as one reviewable batch. It supersedes the older one-step-only and intermediate approval restrictions in `AGENT_GUIDE.md` and the G5/G6 notes for this assignment. Complete the checkpoints below sequentially, validate each, and stop for human review at the end. Do not claim that human review has already occurred. Do not commit, deploy, or start later circuit-specialization work.

Backend retirement must follow demonstrated replacement coverage. Fix concrete blockers in the supported scope instead of merely listing them and stopping at another partial migration. If a blocker genuinely requires a new semantic design, finish independent work, record a minimal failing case and the exact blocker, and leave retirement explicitly incomplete. Never delete a working implementation just to conceal a failing comparison.

The worktree contains uncommitted implementation and benchmark changes. Start with `git status --short` and inspect the relevant diffs. Preserve existing work. Do not reset the repository, remove fixtures, or revert changes you do not own. Re-read code before editing if another agent changes it.

## Required reading

Read applicable `AGENTS.md` instructions and use the Ponytail coding guidelines if available. Then read:

- `docs/REDSTONE_MODEL.md`
- `docs/PISTON_MODEL.md`
- `docs/INTERPRETER_ARCHITECTURE.md`
- `docs/notes/redpiler-pistons-v4/AGENT_GUIDE.md`
- `docs/notes/redpiler-pistons-v4/GRAPH_PIPELINE_MIGRATION.md`
- `docs/notes/redpiler-pistons-v4/ARCHITECTURE.md`
- `docs/notes/redpiler-pistons-v4/GRAPH_JSON.md`
- `docs/notes/redpiler-pistons-v4/steps/graph-04-runtime-boundary.md`
- `docs/notes/redpiler-pistons-v4/steps/graph-05-ordinary-parity.md`
- `docs/notes/redpiler-pistons-v4/steps/graph-06-activation.md`

Some documents describe provisional names and old migration states. Inspect the actual source; the current user requirements and this assignment define the target. Use Side B research only if a transformation actually needs it. Do not implement the entire optimization research backlog.

Historical reference: read `MERGE_DOMAINS_OPTIMIZATION_REVIEW.md` in this directory. It identifies dirty-input/dependency indexing, scratch reuse, and compact serial evaluation in `794af67`, `ff46a21`, and `70f6fcd`, and explains why frozen-region caching and internal-wire suppression must not be copied into general Graph execution. Use the ideas to inform the concrete changes below; do not merge the old backend or add threading/circuit specialization to this batch. The reported historical 7–15 to 1000–1200 TPS jump has not been attributed by a matched artifact in this investigation.

## Current evidence and benchmark contract

The repaired CPU_BubbleSort schematic has a complete east-facing sticky head at `(181,14,166)` for the extended piston at `(180,14,166)`, in the research loader's coordinates. Its SHA-256 is:

`803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611`

The saved schematic retains its wall sign and block entities. Graph currently rejects the sign's possible support loss. The benchmark removes the same sign from both inputs in memory only. Different loader origins shift positions: reuse the harness and its coordinates; do not copy research coordinates into benchmark mutations.

The existing production-release benchmark is:

```powershell
cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 5
```

It runs the public Compiler, asserts `BackendKind::Graph`, and measures exactly 100 active ticks. Loading, compilation, initial stimulus, flush, RAM reads, and counter validation are outside timing. Five pairs use fresh worlds and alternate execution order. Preserve these properties.

Reported baseline, to verify on this machine before editing:

| Executor | Median | Range |
| --- | --- | --- |
| Graph | 4.396806 s | 4.245987–4.582137 s |
| Interpreter | 0.634134 s | 0.621264–0.646845 s |

Each Graph sample reports:

- 20,005 dispatched scheduled ticks;
- 2,059,909 callbacks;
- 32,505 wire walks;
- 302,784 piston samples;
- 27,884 accepted piston actions;
- 27,884 deferred publications.

RAM agrees after 100 ticks. An earlier full Graph replay sorted to `0..15`; do not repeat full sorting or use its instrumented time as a performance baseline. The user requested short measurements.

Retained table estimates: about 411 MiB after callbacks and 324 MiB after lowering. The large tables include deliveries (133 MiB), guards (102.7 MiB), and callback links (64 MiB). Estimates are not RSS or peak allocation measurements. Unbounded test recording explained one 12.45 GiB execution spike. The older ~42 GiB preparation report remains unexplained. Do not reproduce it or describe it as a proven current representation cost.

Production release omits `cfg(test)` text traces. Their removal cannot explain a new production speedup. Do not promise 1000x: this batch removes avoidable engine costs, not circuit-level work.

## Architecture and semantics that must remain

```text
World + stationary entry + ordinary pending work
  -> shared required parser stages
  -> complete general CompileGraph
  -> configurable graph passes
  -> validation and inspection/export
  -> lowered immutable CircuitProgram + mutable CircuitState
  -> Graph execution
```

- Keep one general compilation representation. No replacement legacy graph, parallel parser, packed fallback, hidden world snapshot, or native executor inside Graph.
- Keep the configurable GraphPass API: parameterized passes, caller-selected order, enabled/disabled entries, validation, cancellation, and reporting. Required construction/lowering indices must work with optimization disabled.
- Execution reads graph references and graph state. World access is restricted to compilation, presentation/materialization, and the existing narrow host capability for commands. Shared pure redstone helpers are allowed; physical redstone/piston/dust execution is not.
- Abstract motion away. Retain only necessary logical piston state, connection availability, resource selection, revision/cancellation data, and scheduled graph events. Add no motion/progress model.
- Preserve distinct normal/QC inputs, samples, requests, acceptance attempts, accepted actions, and settled values.
- Preserve callback order, directions, duplicates, nesting, live/captured operands, scheduler phases, tick priorities/deadlines, and stale imported work.
- A shared resource has one authoritative state. Derived indices must not become a second mutable ownership model.
- Observers can make intermediate changes observable. Do not replace dust with a fixed point, coalesce callbacks, suppress repeated piston samples, or reorder operations in this batch.
- Do not cache live guards across a tick or route: nested callbacks can change their operands. A captured operand remains tied to its own capture.
- Keep cancellation and preparation errors transactional. Failures must not mutate the input world, lose pending work, activate a partial compiler, or publish success statistics.
- Retain command and note support, including synchronous command results and correct note sound eligibility. Hoppers and trapdoors remain explicitly unsupported. Reject broken heads, moving entry, transported observers/electrical components, support loss, and other already established unsupported cases with structured witnesses.
- Do not add new unsupported categories to make retirement pass. An admitted ordinary circuit may contain comparator feedback; comparator presence or a cycle does not choose a native simulator or justify rejection.

## Checkpoint 1 — capture the baseline and dependency map

1. Verify the short release benchmark discovers and executes its intended workload; capture five paired samples before changes. Use the existing output support if available rather than a new measurement framework.
2. Inspect all affected callers with `rg`, including tests, exports, host commands, scoreboard, benchmark helpers, and read-only analysis.
3. Confirm the global searches below still exist. Record their callers and which relations are immutable.
4. Write down the retained supported component/lifecycle cases and the explicit unsupported policy. Use existing G5 cases; do not expand to full FPU or a broad new fuzz campaign.

Important files:

- `crates/core/src/redpiler/backend/graph.rs`
- `crates/core/src/redpiler/program_graph.rs`
- `crates/core/src/redpiler/passes/rich.rs`
- `crates/core/src/redpiler/passes/mod.rs`
- `crates/core/src/redpiler/mod.rs`
- `crates/core/src/redpiler/backend/mod.rs`
- `crates/core/src/redpiler/backend/queue.rs`
- `crates/core/benches/cpus.rs` and `benches/support/cpus.rs`

## Checkpoint 2 — replace global discovery with compiled bindings

These are confirmed structural costs in the reviewed source; use function names because line numbers change:

| Function | Current operation | Required direction |
| --- | --- | --- |
| `resource_for_lane` | Searches all resources for matching near/far contacts | Resolve the lane's resource binding once during lowering/construction |
| `resource_contact_available` | Scans every resource and every piston for one contact | Visit only resources and piston bases/heads related to that contact |
| `resource_at_contact` | Scans all resource states | Use the contact's compiled candidates and their authoritative states |
| `comparator_input` | Searches all comparator plans by node | Direct node-to-plan binding |
| `command_plan` / note plan access, if still linear | Searches component plans by node | Reuse the same straightforward indexing approach where applicable |

Preferred data design:

- Immutable binding: `PistonId -> optional ResourceId` for the existing lane semantics.
- Immutable contact adjacency: `ContactId -> related ResourceId/PistonId` entries, with base/head roles distinguished where necessary.
- Immutable component-plan lookup: node to its relevant plan index.
- Mutable state stays in the existing component, piston, and resource state tables.

Use existing types and containers. Choose the smallest readable storage, such as arrays of indices and local ranges; do not build a general indexing framework. Do not allocate a matrix of contacts times resources/pistons. Do not add a hash lookup where a validated dense ID gives direct access.

Build these bindings once, validate them before execution, and use them with both optimized and unoptimized graphs. Prefer program-owned immutable indices when they are naturally lowering data. Do not re-read World or rediscover geometry.

Preserve the exact occupancy rules: a temporarily disabled resource may still reserve its selected contact; a transitioning base/head may block it; `vacated_head`, shared ownership, blocked paths, and stale deferred revisions matter. Do not remove these checks. Restrict their candidate set instead.

Resolve ambiguity according to the existing parser/model. Do not silently bind the wrong resource, assume one resource per piston permanently, or impose a new restriction on valid shared-resource graphs.

Add focused tests covering independent unrelated groups, overlapping/shared contacts, both piston entry poses, head/base availability, disabled resources, capture/drop/recapture, and stale deferred events as applicable. Reuse existing fixtures and helpers; target the relation whose implementation changes.

## Checkpoint 3 — remove redundant allocations and runtime tables

### Ordered routes

`deliver_route` currently copies `deliveries` with `to_vec()` on each call. Iterate by index and copy one `RouteDelivery` at a time from the immutable program before mutating the backend. Preserve order and reentrancy. Do not snapshot all guard results or receiver resolutions in advance.

Keep bounded local wire-neighborhood traversal: it is part of Turbo semantics. Inspect `wire_route_delivery` and other hot lookups for needless whole-route searches; index or bind a local candidate range only when it clearly removes redundant discovery. Never change native heading, side rules, or independent nested-walk snapshots.

### Callback links

`CircuitProgram` currently retains `links`, although execution delivers callbacks through `routes`. The reviewed callers of `program.links()` were tests and statistics. Recheck all callers.

Keep these links in CompileGraph for validation, transformations, and inspection/export. Validate their correspondence with routes before lowering. Do not carry a redundant copy into CircuitProgram unless a real execution use is found. Update statistics to measure the actual retained artifact rather than retaining an unused table for a counter. Move structural assertions to the draft/validated boundary instead of deleting their coverage.

### Other allocations

Audit hot `.clone()` calls. Distinguish cheap copies from heap allocation. For example, obtaining a bool/strength should not clone a NodeState containing a command entity. Replace demonstrated unnecessary copying with narrow reads; preserve the owned snapshot needed for reentrant behavior. Avoid an unrelated state-enum redesign.

Constant/live guard simplification is optional in this batch. Do it only if the prior work leaves it small and evidence supports it. Exact guard interning must preserve operand view and require correct ID remapping across every owner. Do not introduce broad guard caches, a symbolic engine, or a new optimization subsystem. It is acceptable to defer guard compaction explicitly.

## Checkpoint 4 — make test observation deliberate

Keep production hot paths free of text formatting and test-record allocations. They already omit `cfg(test)` recording; verify this remains true.

For test builds, avoid unconditional detailed recording for long scenarios. Use one small test-only recording control if needed. Formatting must occur only after the enabled check; clearing vectors after each tick is not sufficient. Tests requiring ordered traces must explicitly enable recording, retain meaningful structural observations, and still detect direction/order/deadline/resource perturbations.

Do not delete reference-contract instrumentation or disable tests that need it. Avoid a telemetry trait hierarchy, new CLI switches, production trace features, locks, or atomic counters. Retain the existing cheap Graph work counters needed by the benchmark and diagnostics unless a separately measured reason justifies changing them.

After checkpoints 2–4, run focused correctness tests and the same five-pair short benchmark. Ordinary work counts should remain identical for these implementation-only optimizations. Investigate any change; do not rewrite expected counters to hide it. Report memory reduction separately from timing improvement.

## Checkpoint 5 — activate one Graph pipeline and close replacement coverage

Change public ordinary compilation to use the same required graph preparation, pass pipeline, validation, lowering, and executor as piston compilation. Complete the replacement checks before deleting the legacy implementations.

Required cases, using existing tests and small additions where gaps remain:

- ordinary feedforward components, lever/button/pressure plate, torch/repeater delays;
- analog comparator compare/subtract, rear/side inputs and live overrides, same-value notifications, shared-input reconvergence and feedback;
- dust rise/decay, branches, slopes and loops, regulated emissions, direct/conducted lamp power, nested Turbo callbacks;
- observer intermediate changes, bulbs, retained command/note behavior and host effects;
- imported matching/stale/duplicate ticks, type replacement, priority/deadline ties and pending ordinary work;
- public compile/use/tick/flush/reset, hidden state with `io_only`, and reset-to-interpreter continuation;
- admitted TEST/adder/shared-resource fixtures already exercised by Graph;
- deliberate Hopper/trapdoor/support-loss/moving-entry/malformed-head errors and input preservation.

Assert Graph identity in public execution tests. Compare independently stepped interpreter episodes, not legacy-selected public compilation. Outputs alone are insufficient for targeted ordering/continuation tests. Preserve first-divergence diagnostics.

All accepted compiler modes must still work: `--optimize` on/off, applicable `io_only` behavior, entry work, cancellation, developer-defined pass pipelines, read-only analysis, and inspection/export.

Admission must no longer treat a stationary piston as unsupported merely because the old compiler lacked it. Remove or adapt temporary `PistonRuntimeUnavailable` routing logic at all consumers; preserve real entry errors.

Keep `compile_with_graph_pipeline` small: prepare, run passes, lower/construct, publish through well-named existing helpers. Do not add a second compiler coordinator or move parsing back into GraphBackend.

Execution errors must remain visible and latched according to the existing policy. Do not silently resume with corrupted state, fall back to World simulation, or report repeated failure as successful fast execution. Do not redesign the server error API unnecessarily.

## Checkpoint 6 — delete legacy code and migrate all consumers

Once checkpoint 5 passes for the retained scope:

1. Remove `backend/direct/`, `backend/native.rs`, Direct/Native variants, constructor branches, fallback selection, and related backend-reporting branches. Retain the one-variant `Runtime::Graph(Box<GraphBackend>)` if it avoids host churn; do not create another wrapper layer to hide it.
2. Remove `prepare_legacy_backend`, `requires_native_propagation`, legacy comparator/cycle heuristics, and old parser/pass entry points.
3. Remove `LegacyCompileGraph`, `CompileNode`, `CompileLink`, legacy link types/errors, and legacy-only transformations after migrating real consumers. Do not port old power-only rewrites onto rich links without proof.
4. Graph currently imports `NodeType`, `NodeState`, and `identify_block` from legacy-looking modules. Preserve their useful behavior: move reusable component types and identification into an appropriately named existing/new small module before deleting their former owners. A compatibility re-export is justified only by real current callers, not hypothetical future users.
5. `analysis/graph.rs::prepare_candidate_graph` currently builds a legacy graph, and plot commands use it. Make it consume shared general preparation; update its summary and tests to report the actual graph. Do not retain the old compiler just for an inspection command.
6. Update `plot/commands.rs`, `plot/scoreboard.rs`, runtime callers, help text, compilation statistics, benchmarks, and all enum matches. Search the root `src/` server consumers too; the `mroww` package is at the repository root.
7. Preserve the existing JSON inspection schema and post-pipeline artifact semantics. Runtime-only indices should not create an unnecessary schema change or a new parsing path.
8. Retire legacy binary/DOT coupling honestly. Do not serialize rich semantics into an incomplete old binary. If a legacy export cannot represent the unified graph, retain its flag's explicit prepublication error, clearly describe it in help/docs, and test input preservation. This authorizes removal of the legacy-only export implementation, not silently replacing the requested format with JSON. Reuse existing JSON for supported inspection. Do not add a binary compatibility subsystem or viewer project.
9. Migrate meaningful legacy regression scenarios to Graph/interpreter tests before deleting backend-internal tests. Implementation-specific packed-layout tests can disappear with their implementation; list them separately. Do not delete a behavioral test because Graph fails it.
10. Remove obsolete helpers, transitional docs claims, unused imports and dependencies only after confirming all workspace consumers. Keep useful scheduler/pure helpers and the interpreter: removing NativeBackend does not mean removing interpreted execution.

Search the entire workspace for `DirectBackend`, `NativeBackend`, `LegacyCompileGraph`, `Runtime::Direct`, `Runtime::Native`, `BackendKind::Direct`, `BackendKind::Native`, `prepare_legacy_backend`, `requires_native_propagation`, `ordinary_sources`, and other deleted APIs. Explain any remaining historical documentation references. No executable compiled path may retain those backends.

Do not chase unrelated parser refactors, broad formatting changes, a generic bytecode VM, threading, instant-group/BUD specialization, algebraic circuit reduction, or new dependencies. The general pass API remains available for future explicit optimizations.

## Code quality requirements

- Use the smallest complete change. Prefer deletion and existing helpers over infrastructure.
- Keep immutable topology/program, authoritative mutable state, and transient walk/work data distinct.
- Every added struct/enum field gets a short sentence explaining what it represents and why it exists. Typed IDs should convey semantic ownership.
- Name functions for real operations. Break complex work into named intermediate values and readable steps. Do not turn one function into a chain of trivial wrappers.
- Validate references and structural assumptions once at trusted construction boundaries. Preserve live semantic eligibility/cancellation checks. Do not remove Rust bounds checks through `unsafe` or unchecked access.
- No whole-circuit discovery during a local power/contact query. Iterate actual local links/candidates; local neighbor visits required by semantics are allowed.
- Avoid temporary Vec allocation in frequent route delivery. Avoid cloning heap-bearing state to read scalar fields.
- Store positions as provenance/boundary metadata; do not use them to rediscover runtime topology.
- Keep fixture hashes, stored CPU sign, block entities, interpreter references, and benchmark workload unchanged during this batch. No fixture-name behavior branches.
- No arbitrary speedup target. Report measured improvement and any regression honestly.

## Verification

Verify test discovery first. Use normal descriptive test names in existing relevant test modules. Never count a zero-test filtered run as verification.

After focused checks for each change, run the integrated redpiler suites in debug and release:

```powershell
cargo test -p mchprs_core --lib redpiler:: --locked -- --list
cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1
cargo check -p mchprs_core --tests --locked
cargo check -p mroww --locked
cargo fmt -p mchprs_core -- --check
git diff --check
```

Run discovered reference-contract, Turbo, and relevant plot/host lifecycle tests when touched; they may not match `redpiler::`. Discover the existing CPU short ignored test and run its actual matching filter with `--ignored --nocapture`. Keep its 100-tick scope. Run benchmark compilation checks for affected targets; do not execute unrelated long benches.

Finally repeat:

```powershell
cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 5
```

Compare before/after medians and ranges, RAM, backend identity, and work counters. Compilation time is not the performance objective. Record available retained-memory estimates using the same definitions; do not label them peak RSS.

Tests must cover the new bindings against shared-resource and head-blocking counterexamples, order-preserving nested routes, recording enabled/disabled, ordinary public Graph selection, rejected-case input preservation, and reset continuation. Prefer a few meaningful cases to implementation-mirroring assertions.

## Completion report

Update G5/G6 and the migration report to reflect actual final state, including explicit export policy. Correct obsolete table entries claiming CPU execution is unverified where the existing sign-free replay is now verified, while keeping its scope clear.

Provide a concise report with:

1. Changes made and their semantic owners.
2. The global searches/allocations eliminated and the remaining meaningful runtime checks.
3. Removed files/types/APIs and migrated consumers, with an explanation of retained shared code.
4. Exact discovered/executed test counts and commands; ignored tests and zero-match attempts listed honestly.
5. Before/after release timings, ranges, RAM agreement, work counts, and retained-memory estimates.
6. G5/G6 completion status with concrete evidence. Mark incomplete if a production legacy path or a retained supported capability still depends on it.
7. Remaining blockers, untested histories, and explicitly unsupported cases.

Stop for human review after the batch. End with one or two lines stating what was not checked and the material risk. Do not claim full CPU-history equivalence, a measured 1000x improvement, a solved 42 GiB report, or completed retirement from a short passing output test alone.
