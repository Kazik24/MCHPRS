# Symbolic actor validation

Validation ran against the shared, uncommitted worktree at `ec08c1f99ee4493311f7314a229073473118c78b` (HEAD), after the nine test failures below were fixed. The validated tree includes concurrent edits and is not a clean commit. In particular, the current tree includes the event compiler/runtime, its tests, the export validator and test, the compact-kernel boundary regression, the piston-base sign changes, the O3/O4 graph fixture updates, and the benchmark protocol below. The benchmark harness SHA-256 was `ba4d4c6adb5ab6b5d46fb10847907de7c32ec02526d209cc281ceaba3482617c`. `git diff --check` passed.

The tested symbolic path is O4 stable-input event lowering for one `instant_observer` piston actor and its reset clock. The graph pipeline builds one symbolic actor and leaves the finite instant machine absent. The runtime test confirms at least one symbolic actor step, both accepted piston directions, reset scheduling and powered reset state, and matching callback/effect order and handoff against the ordinary Graph backend and interpreter. Separate tests cover duplicate request coalescing, stale-request revalidation after reset power rises, construction budgets, cancellation and stale plans. The power-query test checks main/quasi channels over strengths 0–15 at attenuations 0, 1 and 15, plus structured declines. The `instant_torch` lifecycle test exercises ordinary Graph/interpreter behavior; it is not evidence of symbolic actor activation.

This is an executable actor/reset event program, not whole-region contraction. Callback and Turbo propagation and deferred effects remain in the shared runtime. No input-mask enumeration is used by this actor program. This validation does not establish broad fixture coverage or a speedup.

## Focused results

Passing commands:

- `cargo test -p mchprs_core --lib symbolic_kernel_tests --locked -- --nocapture` — 5 passed. This includes the O4 `instant_observer` actor runtime and handoff, reset/pending-work parity, request revalidation and construction guards.
- `cargo test -p mchprs_core --lib redpiler::analysis::instant_regions::symbolic::tests --locked -- --nocapture` — 4 passed.
- `cargo test -p mchprs_core --lib power_boundary_excludes_internal_links_and_keeps_cross_region_links --locked -- --nocapture` — 1 passed.
- `cargo test -p mchprs_core --lib signs_ --locked -- --nocapture` — 7 passed, including piston-base signs in both poses and support-loss rejection.
- `cargo test -p mchprs_core --features graph-trace --lib graph_trace --locked -- --nocapture` — 1 passed.
- `cargo test -p mchprs_core --test cpu_references --locked` — 1 passed, 4 intentionally ignored (three 50,000-tick regressions and a capture test).
- `cargo check -p mroww --all-targets --locked` — passed, including server targets.
- `git diff --check` — passed.

The first focused run of `symbolic_event_export_preserves_program_and_rejects_malformed_references` failed because its 2×2 chunk grid was paired with full-plot bounds. The fixture now passes explicit bounds within loaded chunks, and the focused export test is included in the passing regression union reported below.

## Redpiler suites

The initial debug and release suites each reported 355 passed, 9 failed, 9 ignored. All nine initial failures were subsequently repaired. The parent’s final serialized debug run passed: 365 passed, 0 failed, 9 ignored, 512 filtered, in 101.34 seconds. Final serialized release validation passed with the same counts in 91.98 seconds:

```text
cargo test -p mchprs_core --release --lib redpiler --locked -- --test-threads=1
test result: ok. 365 passed; 0 failed; 9 ignored; 0 measured; 512 filtered out
```

The parent also reports the focused union for the nine repaired regressions plus the compact-kernel boundary regression passed 10/10.

The repaired failures were:

- `symbolic_event_export_preserves_program_and_rejects_malformed_references` — now uses explicit bounds for its loaded chunks.
- `simplify_guards::optimization_levels_select_rewrites_before_dependent_plan_builders` and `analysis::redstone_fuzz::graph_feedback_preserves_hidden_state_and_scheduler_on_reset` — brittle hardcoded pass counts were replaced by checks across configured passes and optimization modes; the O4 registry reflects the symbolic pass.
- `analysis::export_json::adder_1bit_fixture_matches_graph_json` and `analysis::export_json::stable_adder_export_names_the_contract_and_every_instruction_target`: O3/O4 snapshots were regenerated only for removal of internal-region power inputs; all other graph data remained identical. The stable-kernel export also required a validator fix: valid boundary projection-root site IDs were absent from its reference set. A new regression checks valid and missing sites.
- `analysis::deep_parser_audit::remaining_frozen_targets_report_their_first_parser_stage` — the stage sweep was updated for the new admitted target.
- `analysis::research::cpu_bubblesort_sign_on_piston_reports_transition_support_loss`: renamed to verify that the piston-base sign passes native support and geometry validation, while the intentionally small selection still rejects an external callback receiver and preserves the world. This test does not establish admission or rejection of the complete CPU schematic.
- `plot::client_sync_tests::redpiler_flag_suggestions_reach_the_chat_client` and `plot::commands::security_tests::redpiler_analysis_reports_the_unified_graph_and_compile_statistics`: updated protocol assertions to read all five optimization suggestions and wait for the actual enabled/skipped pass statistics. The socket/listener implementation was unchanged.
- The new compact-kernel boundary regression verifies site IDs in projection roots and rejects a missing site.

The parent reports that the focused union covering those nine regressions plus the new compact-kernel boundary regression passed 10/10.

## Benchmark status

The benchmark harness now admits `io/instant_observer` for the stable-input protocol. It verifies the manifest trigger starts OFF, toggles it once before the timer, flushes and verifies it remains ON, then measures 512 tick calls while holding it. Setup, publication, observations, reset, and continuation are outside the timer. The measured stable O4 window is required to report positive `symbolic_actor_steps` and zero finite `instant_transitions`.

Command:

```text
cargo bench -p mchprs_core --bench cpus -- --graph-short --all-devices --component io/instant_observer --graph-tier 4 --stable-instant-inputs --iterations 5 --label symbolic-actor-validation --output target/symbolic-actor-instant-observer.json
```

All five alternating O4, O4-stable, and interpreter trials passed the harness’s output, world checkpoint, pending-future, reset, and interpreter-handoff comparisons. Each O4-stable 512-tick window recorded 767 symbolic actor steps and 0 finite instant transitions. The O4 and interpreter work counters recorded 0 symbolic actor steps and 0 finite instant transitions.

| Executor | Median per 512 tick calls | Range |
| --- | ---: | ---: |
| O4 | 0.5971 ms | 0.5848–1.3636 ms |
| O4 stable symbolic | 0.7453 ms | 0.7350–0.7642 ms |
| Interpreter | 1.0408 ms | 1.0227–1.0598 ms |

The first O4 sample was a clear high outlier and is retained in the range. On this one fixture, stable symbolic O4 was about 25% slower than O4 without symbolic composition and about 28% faster than the interpreter. These tick-only results cover one observer-driven actor/reset fixture; they do not establish whole-region contraction, whole-server throughput, or general speedup. The structured run artifact is `target/symbolic-actor-instant-observer.json`.
