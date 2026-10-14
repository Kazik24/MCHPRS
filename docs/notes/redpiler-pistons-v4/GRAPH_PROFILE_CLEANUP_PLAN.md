# Graph profiling inventory and cleanup plan

Status: approved implementation scope and agent prompt. Documentation only; no profiling code has been removed. Source snapshot: 2026-10-14; the four files with temporary opt-08 census work are already modified in the worktree.

## User authorization and agent assignment

The user selected **A/B and K/L only**. Remove C through J, including all `graph-profile` timing buckets and research APIs. The user also requested that human-readable debugging use trace logging enabled by a compilation flag, with a small formatter for readable ticks/phases. Detailed profiling follows the external-profiler approach linked below.

Implement the cleanup and trace replacement described here, validate it, and stop for review. Preserve unrelated worktree edits, schematic data, supported execution behavior, and correctness coverage. Do not commit. Do not start any guard, callback, dust, or piston optimization as part of this assignment.

The document retains the full inventory so the agent can remove producers and consumers together. Historical wording describing optional retention is superseded by the decisions in the next table: no part of C through J is retained.

## Direction

Keep the uninstrumented runtime benchmark and existing basic execution counts. Temporary manual measurements can answer a specific research question; they do not become permanent timing buckets. Use an external profiler for detailed CPU attribution. Delete completed research instrumentation instead of turning each investigation into a permanent runtime API.

The requested reference is [The Rust Performance Book: Profiling](https://nnethercote.github.io/perf-book/profiling.html). It describes external CPU/heap profilers, release debug information, and frame pointers. Select an available profiler for the host; do not install tools, request profiling privileges, or alter global build settings as part of this cleanup. Symbol/debug settings belong in the profiling invocation or a narrowly scoped profile. Keep profiler runs separate from ordinary speed measurements.

## Approved retained surface

These are the user's selected boundaries. The new `graph-trace` facility is human-readable logging, separate from the removed profiling feature.

| ID | Current measurement | Decision | What is lost if removed |
| --- | --- | --- | --- |
| A | Benchmark elapsed time, paired samples, RAM verification | Keep | The trustworthy runtime comparison |
| B | Eight existing `GraphExecutionCounts` fields | Keep for this cleanup | Basic workload and regression comparisons |
| C | Coarse scheduled/action/publication phase timers | Remove | Immediate in-process phase timing; temporary manual measurements remain possible |
| D | Total internal tick timer and three nested operation timers | Remove | Overlapping internal timing buckets; benchmark elapsed time remains |
| E | Aggregate route/guard diagnostics | Remove | Work amplification and guard-kind counts |
| F | Callback role/phase/receiver/handler breakdowns | Remove | Detailed domain-specific dispatch attribution |
| G | Turbo lookup/index/scan research counters | Remove | Index research counts; correctness tests remain |
| H | Per-guard, five-context opt-08 census | Remove first after its research output is saved | Dynamic visits by individual GuardId/context |
| I | Runtime callback/guard static inventory | Remove from runtime; existing JSON export remains | The specialized research report, including empty native occurrences |
| J | Long dust-candidate sampling plumbing | Remove | The completed opt-02 length-by-query study |
| K | Compile-time counts, pass records, table-byte estimates | Keep | Preparation diagnostics and memory estimates |
| L | Opt-in test observation recorders | Keep | Ordered differential/continuation correctness evidence |

Required result: **A, B, K, L**, plus the small opt-in trace logger described below. No permanent per-guard arrays, route-role arrays, timing buckets, or research-only public getters. Delete the `graph-profile` feature completely rather than keeping an empty compatibility shell. All three profiling types and all six feature-gated Compiler/Runtime inspection methods disappear.

## Where the feature is used now

Literal search found **166 occurrences of `graph-profile` in 14 Rust/manifest files**. These are textual matches, including comments and `not(...)` branches, not 166 independent measurements. Of those, 90 are in the executor and 39 in the CPU bench.

Paths below are relative to the repository root. Line anchors describe this snapshot and will move during implementation.

| File | Literal matches | Current responsibilities |
| --- | ---: | --- |
| [`crates/core/Cargo.toml`](../../../crates/core/Cargo.toml#L7) | 1 | Declares the opt-in feature; no profiler dependency is added |
| [`crates/core/benches/cpus.rs`](../../../crates/core/benches/cpus.rs#L29) | 39 | Timer printing, snapshots/deltas, census output, inventories, profiling assertions, repeated-sample checks, JSON sections |
| [`redpiler/mod.rs`](../../../crates/core/src/redpiler/mod.rs#L109) | 13 | Three public timing/profile types, 27-field callback profile, labels/deltas, six compiler inspection methods |
| [`backend/graph.rs`](../../../crates/core/src/redpiler/backend/graph.rs#L329) | 90 | Profile state, updates/timers throughout execution, temporary guard census, inventory generation, dust sampling |
| [`backend/mod.rs`](../../../crates/core/src/redpiler/backend/mod.rs#L27) | 6 | Six forwarding methods for profiling/research access |
| [`task_monitor.rs`](../../../crates/core/src/redpiler/task_monitor.rs#L17) | 5 | Carries long dust-candidate positions from a pass to runtime construction |
| [`program_graph.rs`](../../../crates/core/src/redpiler/program_graph.rs#L399) | 2 | Profile-only `Route::native_event_count` and a test initializer |
| [`analysis/callbacks.rs`](../../../crates/core/src/redpiler/analysis/callbacks.rs#L343) | 2 | Saves the native occurrence count before profile/contact expansion |
| [`passes/contract_dust_paths.rs`](../../../crates/core/src/redpiler/passes/contract_dust_paths.rs#L58) | 2 | Records candidate positions; tests the research sampling counter |
| [`passes/index_turbo_callbacks.rs`](../../../crates/core/src/redpiler/passes/index_turbo_callbacks.rs#L129) | 1 | Supplies the profile-only Route field in a test fixture |
| [`passes/simplify_guards.rs`](../../../crates/core/src/redpiler/passes/simplify_guards.rs#L929) | 1 | Supplies the same profile-only Route field in a test fixture |
| [`backend/graph_tests.rs`](../../../crates/core/src/redpiler/backend/graph_tests.rs#L190) | 2 | Callback-counter consistency and shared-piston inventory tests |
| [`backend/graph_adder_tests.rs`](../../../crates/core/src/redpiler/backend/graph_adder_tests.rs#L28) | 1 | Shared-adder inventory test |
| [`backend/graph_test_family_tests.rs`](../../../crates/core/src/redpiler/backend/graph_test_family_tests.rs#L165) | 1 | BUD inventory test |

The declaration is not a default feature. Most data and updates are compiled out without it. However, the temporary `GuardProfileContext` enum and argument currently remain in ordinary guard-call signatures; the argument is discarded in the non-feature branch. Do not claim measured production overhead from that source-level plumbing without a benchmark.

Searches of repository Rust/Python/JavaScript consumers found the public profiling getters used by the CPU bench and the listed tests; no additional application consumer was found. They remain public Rust APIs, so out-of-repository callers have not been ruled out.

## A/B/K/L — existing facilities outside graph-profile

The following must not disappear as collateral cleanup:

- `cpus.rs` uses `Instant` around the actual measured tick window, then verifies RAM and work outside timing. This is the ordinary benchmark, independent of the feature.
- `GraphExecutionCounts` has `ticks`, `scheduled_ticks`, `callbacks`, `wire_walks`, `dust_path_power_samples_elided`, `piston_samples`, `piston_actions`, and `deferred_publications`. It is maintained without `graph-profile`. The dust-specific field can be reconsidered separately; preserve it in this initial cleanup to avoid mixing API/test changes.
- `GraphStatistics`, `GraphCounts`, `PassStatistics`, and `PreparationTableStatistics` describe compilation/pass results and retained-table estimates. Keep reporting outside the tick hot path. Table estimates are not RSS.
- `#[cfg(test)]` trace, callback-order, comparator-update, power-link inspection, and observed-operation recorders are separate correctness facilities. They already have explicit recording controls. Preserve their opt-in behavior, allocation guards, and registered differential tests.
- Scheduler state, pending membership, clock phases, dirty flags, command/note host effects, and projection data are simulation state, not instrumentation.

Do not remove every `Cell`, `Instant`, `TaskMonitor` parameter, or callback helper merely because profiling also uses that name/type. Verify remaining callers and behavior.

## C/D — timing inventory

`GraphExecutionProfile` contains seven `GraphProfileMetric { calls, elapsed_ns }` buckets. `record_profile` accumulates inclusive elapsed nanoseconds.

| Bucket | Hook | Recommendation |
| --- | --- | --- |
| `tick_phases` | `tick_with_host` around `tick_phases` | Remove: keep end-to-end bench time as the comparison |
| `scheduled_drain` | Both scheduled drains in `tick_phases` | Optional keep: coarse scheduled-work time |
| `piston_actions` | `drain_piston_actions` call in `tick_phases` | Optional keep: coarse action time |
| `deferred_connections` | `publish_deferred_pistons` call in `tick_phases` | Optional keep: coarse publication time |
| `route_delivery` | `deliver_route` wrapper | Remove: frequent/nested timing; use external attribution |
| `turbo_wire_walk` | `evaluate_wire` wrapper | Remove: frequent/nested timing |
| `piston_sample` | `sample_piston` wrapper | Remove: frequent timing, including early returns |

Remove all seven buckets, `GraphProfileMetric`, `GraphExecutionProfile`, their delta/reporting helpers, `GraphProfileBucket`, executor storage, timer hooks, and forwarding methods. Preserve the existing error paths around each drain/publication call. Collapse wrappers introduced solely to measure an inner function when doing so is a straightforward, behavior-preserving deletion; do not combine unrelated handlers or reorder work.

Coarse manual timings and external profiler results serve different purposes. Do not add inclusive buckets together or describe nested elapsed time as exclusive CPU cost. A retained optional timer is still excluded from the speed benchmark build.

## E/F/G — all 27 callback-profile fields

`GraphCallbackProfile` currently lives in `redpiler/mod.rs` and is stored as a `RefCell` in the executor. This table assigns every field to a decision group.

| Group | Fields | Hook locations |
| --- | --- | --- |
| E: aggregate route work | `route_calls`, `delivery_records_examined` | `record_route_call`, `deliver_route_inner` |
| E: aggregate guard work | `guard_roots_evaluated`, `guard_operations_by_kind`, `guard_failures` | Both guard-root wrappers and recursive evaluator |
| E: declined delivery/capture diagnostics | `missing_captured_guard_operands`, `missing_captured_delivery_operands`, `deliveries_without_effective_receiver` | Site resolution in guards; checks before dispatch |
| F: route classifications | `route_calls_by_role`, `delivery_records_by_role` | Per-route role table and per-row updates |
| F: dispatch classifications | `dispatches_by_receiver_and_kind`, `dispatches_by_role_and_kind`, `handler_invocations_by_family` | General/Turbo dispatch and component handler entry |
| F: phase matrices | `route_calls_by_phase_and_role`, `dispatches_by_phase_and_role` | Phase and owner-role classification on each relevant call |
| F: writes/scheduling/pending samples | `component_state_changes`, `tick_schedule_attempts`, `newly_scheduled_ticks`, `piston_samples_skipped_base_pending` | Multiple component handlers, scheduler insertion, piston early return |
| G: Turbo lookup study | `turbo_route_lookups`, `turbo_indexed_lookups`, `turbo_fallback_lookups`, `turbo_candidate_entries_scanned`, `turbo_target_entries_scanned`, `turbo_lookup_failures`, `turbo_first_matches`, `turbo_wire_node_evaluations` | `wire_route_delivery`, inspection callback in `first_turbo_delivery`, nested wire evaluation |

The label arrays and delta calculations for these fields also live in `mod.rs`. Role/handler helpers and `record_callback_dispatch` live in `graph.rs`. The bench snapshots these structures, asserts several counter relationships, and serializes them into `callback_research` output.

Removing G must preserve indexed candidate selection and its semantic tests. `first_turbo_delivery` currently accepts a per-row `inspect` closure for research/tests; remove or confine that observer argument to test support if no retained measurement needs it. Preserve tests of first match/miss and order independently of production profiling.

All E/F/G fields are removed, including `GraphCallbackProfile`, its label arrays/delta implementation, and the corresponding updates. Keep the independent eight-field `GraphExecutionCounts` structure. Do not replace the deleted counters with a renamed profile structure or trace one event for every formerly counted guard leaf.

## H — temporary opt-08 per-guard census

This is the highest-priority removal candidate once the active research agent has saved its results.

Current path:

```text
cpus.rs snapshots/JSON
  -> Compiler::graph_guard_visit_counts / graph_guard_visit_records_since
  -> Runtime forwarding methods
  -> GraphBackend census getters
  -> guard_visit_counts: Vec<[u64; 5]>
  -> every recursive guard evaluation
```

The five contexts are Route, Turbo, Power, Comparator, and Contact. `guard_matches` has an additional context argument, with save/restore through `guard_profile_context`. Call sites in power, comparator, contact selection, and route dispatch carry it solely for the census.

The array costs 40 bytes per stored guard before vector overhead. Using previously reported guard totals, that is approximately 31.07 MiB at O1 or 143.39 MiB at O0. The before-window snapshot clones the whole array, and the report builds strings for visited records. These are estimates from element counts, not RSS measurements, and do not explain the historical 42 GiB report.

Cleanup must remove the buffers, initialization, getters, context type/argument/save-restore, bench snapshots, formatted records, and JSON fields together. Keep the actual `guard_matches_using` Boolean evaluation and its Live/Captured semantics untouched.

## I — static research inventory embedded in runtime

`callback_research_inventory` in `graph.rs` scans routes/guards and generates role/owner tables, branch histograms, receiver summaries, duplicate-body comparisons, representative routes, and expression-size distributions. Its supporting functions are `callback_route_roles`, `profile_phase_index`, `handler_family_index`, `guard_contains`, `guard_contains_profile`, and `guard_expression_sizes`. Remove them once their research consumers are gone; verify no semantic consumer has been added since this snapshot.

Additional storage: `GraphBackend::route_roles` and feature-only `Route::native_event_count`. Callback preparation captures the original event-list length before expansion; several synthetic Route initializers supply the extra field. This is why research has spread into parser, graph types, and unrelated pass tests.

Recommend deleting this inventory from runtime and retaining the already written research report. Graph JSON export remains supported. Future static questions can be answered using the exported graph in an external analysis script when needed; writing that replacement tool is not part of this cleanup.

The exact number of empty native occurrences cannot always be recovered from sparse surviving route rows after removing `native_event_count`. Record that loss explicitly; do not invent counts or retain the field without a selected consumer. Graph export is a semantic inspection artifact, not a promise to preserve every historical research statistic.

## J — completed long dust-candidate study

Current path:

```text
contract_dust_paths records candidate positions/lengths
  -> TaskMonitor::dust_candidate_wires
  -> GraphBackend::from_lowered maps positions to node IDs
  -> dust_candidate_length_by_node
  -> seed/captured general wire-power queries increment a length histogram
  -> getter wrappers
  -> cpus.rs output and one profile-only test
```

Remove the research transport and dynamic histogram. Preserve the real dust certificates, runtime specialization, pass decline statistics, and tests of specialized/general behavior. Removing instrumentation is not permission to drop validation or change admitted path families.

Only remove `from_lowered`'s monitor parameter if it has no remaining purpose; preserve cancellation and ordinary preparation statistics wherever they still apply.

## Public forwarding and reporting surface

The six feature-gated Compiler/Runtime methods are:

1. `graph_execution_profile`
2. `graph_callback_profile`
3. `graph_guard_visit_counts`
4. `graph_guard_visit_records_since`
5. `graph_callback_inventory`
6. `dust_candidate_power_samples`

Remove all six methods, including both forwarding layers and the bench/test callers. A new runtime statistic must not automatically add another method through every layer.

Simplify bench formatting, state, duplicate-sample comparisons, and assertions with their producers. Preserve wall-clock samples, RAM checks, fixture provenance, O0/O1 selection, and work-counter verification. Do not leave unused `null` research sections or empty candidate arrays just to mimic the old schema; if report fields change, identify/version that benchmark-report change. Do not change the graph-export schema unless a graph-export field actually changes.

## Tests and documentation affected

Profile-only tests currently include:

- `callback_profile_matches_dispatched_graph_callbacks_for_an_ordered_route`
- `callback_research_inventory_covers_shared_piston_contacts_at_o0_and_o1`
- `callback_research_inventory_covers_shared_adder_at_o0_and_o1`
- `callback_research_inventory_covers_bud_at_o0_and_o1`
- `long_candidate_profile_counts_general_neighbor_power_samples`

Remove tests that only assert removed report/counter plumbing. Preserve or convert independently useful state/ordering/admission assertions. Do not delete the broader fixture and interpreter comparisons merely because their file also contains an inventory test.

Feature-related documentation appears in `CALLBACK_RESEARCH.md`, the pass `README.md`, `GUARD_OPTIMIZATION_PLAN.md`, `steps/opt-02-dust-contraction.md`, and `steps/opt-10-compiled-guard-patterns.md`. Keep historical measurements and commands labeled as historical. Update active agent instructions so they do not require recreating the removed census. New investigations may use temporary bounded manual measurements, while detailed attribution uses an external profiler.

Single test-only recorder struct would make this cleaner. Keep it separate from human-readable trace logging.

```rs
#[cfg(test)]
#[derive(Default)]
struct TestRecorder {
    /// Controls collection before any recording or formatting occurs.
    enabled: bool,
    /// Records callback targets, directions, and delivery order.
    callbacks: Vec<(BlockPos, DeliveryKind)>,
    /// Records comparator update order and multiplicity.
    comparator_updates: Vec<BlockPos>,
    /// Counts power links inspected by query-locality tests.
    power_links_examined: Cell<usize>,
    /// Records typed operations for interpreter comparisons.
    operations: Vec<GraphObservedOperation>,
}
```

Then GraphBackend contains just:

#[cfg(test)]
test_recorder: TestRecorder,

Put its definition and helpers in a focused backend/test_recorder.rs module, compiled only for tests. Give it simple enable(), clear(), and recording methods. Recording methods should check enabled before allocating; clear() should preserve whether recording is enabled.

## Human-readable trace logging

The existing debug harness has two different jobs. Separate them explicitly:

1. Human-readable diagnostics belong in ordinary trace logs, with no retained runtime log vectors.
2. Test observations belong in opt-in `#[cfg(test)]` records, so assertions remain deterministic and do not depend on a logging subscriber or message wording.

Use the existing `tracing::trace!` ecosystem. The user called this `log::trace`; that is the requested logging behavior, not a requirement to add another logging dependency. Core already depends on `tracing` and the server already configures its subscriber. Do not initialize a subscriber inside the executor or add a new logger crate.

### Compile-time switch and formatter

Add one `graph-trace` Cargo feature, off by default, and expose it through the server package if needed. Calls must compile out entirely when it is disabled: no argument evaluation, string formatting, clocks, vectors, or runtime enable Boolean.

Put the small logging helper/macro in one focused backend module, for example `backend/graph_trace.rs`. Keep the executor's call sites short. The helper emits the existing logical tick, current phase, and message through one stable logging target, such as `mchprs_core::redpiler::graph`. Include a typed node/position/route in the message or fields when that event already has it. Do not compute spatial context solely for logging.

Readable output should follow one consistent shape; exact subscriber punctuation is not a contract:

```text
TRACE graph tick=42 phase=scheduled_ticks | dispatch node=17
TRACE graph tick=42 phase=piston_actions | accept piston=3 action=Extend
TRACE graph tick=42 phase=deferred_connections | publish piston=3 revision=8
```

Use `format_args!`/normal tracing arguments instead of eagerly allocated `format!` strings. Preserve event order and the phase at the actual call site. Nested walks naturally appear in emission order; do not add a span stack, nesting counter, duration tracker, tick buffer, JSON event schema, or flush-at-tick-end mechanism.

The helper does not return simulation data, change error handling, or own mutable state. Avoid a new context struct if existing clock fields are sufficient. Explain any new struct field/enum payload with the one-sentence comments required by the pass guide.

### Important server-level logging cap

The root `Cargo.toml` currently enables `tracing/max_level_debug` and `tracing/release_max_level_info` unconditionally. A core `graph-trace` feature alone will therefore not make server trace events visible. `RUST_LOG` cannot restore events removed by these compile-time caps.

Make the documented opt-in build work while preserving normal-build logging limits. A small implementation is to move the existing caps behind a root default feature, forward `graph-trace` to core, and document a trace build with that default cap feature disabled. Inspect current root features first; do not overwrite any concurrent feature changes. Do not merely add `max_level_trace` alongside the stricter cap features and assume that overrides them.

Document and test the exact package-qualified build command plus the existing subscriber's target filter. One possible root invocation after such wiring is:

```text
$env:RUST_LOG = 'mchprs_core::redpiler::graph=trace'
cargo run -p mroww --release --no-default-features --features graph-trace
```

This is a PowerShell example; confirm the final feature names and target before documenting it as the working command. Preserve the application's ordinary default filter and log destinations.

### Migrate the current harness without weakening tests

Replace `graph_test_trace!` human diagnostics with the compile-gated trace helper. Remove the backend's textual `trace: Vec<String>` and `trace()` getter after migrating their assertion consumers. Keep `test_recording`, `clear_test_traces`/its appropriately renamed observation equivalent, and the typed callback/comparator/observed-operation records for L. They remain test-only, opt-in, and disabled before recording/allocation on long runs.

Some current tests assert behavior by parsing log strings. Convert these to typed observations or direct state/queue assertions; do not delete the tested behavior, require `graph-trace` to run ordinary tests, or capture a global logger to rebuild the old String vector.

Specifically retain evidence for:

- Scheduler dispatch order, duplicate/stale imported ticks, and exactly one clock advance per logical tick.
- Independent/nested wire walks: the outer walk starts, an inner walk starts/completes, then the outer queue resumes. Preserve the intermediate 0-to-14 wire write assertion.
- Live versus captured power samples and wire writes, including seed-only/no-op paths.
- Piston request/acceptance/cancellation and stale actor/resource publications, including reset/continue.
- Comparator and callback order, direction, multiplicity, and host effects.

Reuse `GraphObservedOperation`, existing snapshots, and typed recorder data where sufficient. Add a few test-only event variants only for assertions that lack another reliable witness; do not mirror every log message in a second event hierarchy. For example, nested-walk boundaries or scheduler dispatches can have a small typed record if the existing test genuinely needs their ordering.

Test the helper's tick/phase fields and disabled-build behavior with a small local test. Use a thread-local test subscriber if capture is needed; avoid process-global logger setup that races parallel tests. Prove lazy arguments are not evaluated with the feature disabled. The normal Redpiler suite must pass without trace logging.

## Implementation batches

1. **Preserve evidence:** coordinate with the active research work; save its existing summary/artifact before deleting temporary H/J hooks. Do not rerun long experiments or delete data to perform cleanup.
2. **Remove profiling:** delete C through J and matching API, parser, TaskMonitor, fixture-initializer, bench, and profile-only test plumbing. Delete the `graph-profile` feature and all dangling gates in active code. Keep execution logic and A/B/K/L unchanged.
3. **Replace human diagnostics:** add the small compile-gated trace helper, make the actual server trace build work, and migrate textual test assertions while retaining typed evidence. Check behavior before and after this batch separately from profiling removal.
4. **Update contracts and check:** simplify current benchmark JSON/documentation, document the trace build/filter, verify test discovery, and run the supported build/test matrix. Report remaining gates and the retained A/B/K/L facilities.

Do not add a telemetry framework, a dynamic callback sink, a generic event bus, a per-pass profiler API, or another feature per investigation. Do not redesign pass metadata or the graph architecture in the same cleanup.

## Acceptance checks

- Source search matches the approved retained groups; no active `graph-profile` feature/gates, `TEMP OPT-08 CENSUS`, orphaned public getters, research-only guard arguments, or unused Route fields remain. Historical documents may retain explicitly historical references.
- Basic execution counts, ordered reference observations, pending work, and RAM match the pre-cleanup behavior. Do not edit frozen expectations to hide a changed executor.
- Discover and run relevant tests in debug/release; run the broader Redpiler/reference-contract suites for signature changes.
- Check core tests/benches and the server normally and with the new `graph-trace` build configuration. Delete obsolete `graph-profile` commands from active instructions. Verify an actual trace-enabled server build is not silenced by static logging caps.
- Validate changed benchmark-report consumers/fixtures if any exist. Keep graph JSON loader/viewer tests if graph-export changes are actually made.
- Run the short 100-active-tick CPU replay/benchmark to verify RAM and work counts after cleanup, without logging. If reporting a speed change, use the normal five-pair comparison. Setup, compilation, flush, and correctness checks remain outside timing. Do not claim a normal-build speedup merely from removing feature-gated instrumentation.
- A future external profiling run must identify its build settings and measured window; preparation activity is outside the runtime question. No profiler installation or capture is authorized by this document alone.

Not performed for this inventory: code removal, test execution, benchmarking, external profiler setup, or a compatibility audit of third-party Rust API consumers.
