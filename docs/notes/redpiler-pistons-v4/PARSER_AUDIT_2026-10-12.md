# Steps 01-05: deeper parser audit

Audit date: 2026-10-12. Status: findings resolved in follow-up review; supported static parser gate approved. Execution parity is not established. The original failing results below are retained as historical evidence; current results are in the follow-up section.

## Scope and changes

Reviewed the reference comparison harness, typed graph validation/lowering, geometry admission, electrical channels, callback construction, connected interpreter procedures, and parser callers/tests. Added 15 active regression/property tests; no production behavior fixes were made by this audit. Concurrent Step 05, export, and native piston changes were preserved. In particular, the concurrent head/resource restoration and guard-validation fixes are reflected in the final tested checkout, not claimed as audit-authored changes.

New test locations:

- `crates/core/src/redpiler/analysis/tests/deep_parser_audit.rs`: nine parser/query/admission tests.
- `crates/core/src/redpiler/program_graph.rs`, `deep_audit_*`: four validation counterexamples.
- `crates/core/src/redstone/instant_piston_tests/reference_contract.rs`, `deep_audit_*`: two comparator false-negative counterexamples.

The parser test module is registered in `analysis/tests.rs`. The guard/channel evaluator is test-only and uses native primitive source facts; it is not a new simulation backend. Callback projection lowering deliberately does not claim a complete executable program: electrical query plans still live separately from `CircuitDraft`.

## Must fix 1: comparison normalization loses observed state

Location: `reference_contract.rs::semantic_boundaries`.

Counterexample: three records share tick, phase, protocol stage, and action. A middle record contains an ordered piston sample. Only that record has a changed shared-resource enabled state, or a different pending completion deadline; the following record restores the original value. Comparing against an episode without that middle discrepancy incorrectly returns `Ok(())`.

The normalizer merges all three into one boundary and overwrites its resource/deadline state with the final snapshot. The ordered sample remains, but the state associated with it disappears. This defeats the harness's stated resource and pending-work comparison contract.

Tests: `deep_audit_comparison_preserves_resource_state_at_callback_boundary` and `deep_audit_comparison_preserves_deadline_at_callback_boundary`.

Smallest fix: preserve semantically distinct snapshots at observed boundaries. Discard only justified unobservable stutter/progress visits; sharing tick/phase/action is not enough to merge state. Keep the existing progress-visit acceptance test passing. Do not remove resource/deadline comparisons or weaken these new tests to bless the false negative.

## Must fix 2: inert sign callbacks reject target fixtures

Location: `callbacks.rs::Builder::branches`, `SiteSource::Constant` branch.

Counterexample: a floor lever beside a supported oak sign. The sign has no neighbor-update handler, but the parser rejects its ordinary Update delivery because `attachment_support` is present. Update-handler eligibility and support/Shape work are different.

Frozen real fixtures fail for the same reason:

| Fixture | Notifying source | Rejected target | Actual block |
| --- | --- | --- | --- |
| OR_1 | (40,31,41) | (40,32,40) | Oak standing sign, rotation 8 |
| ADDER_1BIT | (59,32,41) | (60,31,41) | Oak wall sign facing East |
| BUD_PistonUpdate | (41,33,40) | (41,34,41) | Oak standing sign, rotation 8 |

Tests: `inert_supported_sign_does_not_require_an_update_receiver` and the three `*_completes_callback_projection_and_preserves_entry` tests.

Smallest fix: determine required handling by delivery kind. Ordinary updates to inert attachments need no component receiver. For Shape, use the existing geometry support proof to retain safe fixed attachments as inert context, or represent the required shape handler explicitly. Do not strip fixture labels or ignore genuinely changing support. The three fixture tests must proceed through callback/state validation, not merely geometry inventory.

## Must fix 3: component bindings can alias specialized owners

Locations: `program_graph.rs::validate_sites_and_wires` and `validate_receiver`.

Counterexample: change a synthetic site's source to `Component(piston_node)` and route receivers/edges to that same node. Validation accepts it because references are in range and source/receiver IDs match. The supplied node actually owns specialized piston state; its callbacks require base/head semantics. A Contact owner can likewise be referenced through the generic component vocabulary.

Test: `deep_audit_component_sites_cannot_alias_specialized_owners` (the first failing witness is the piston owner).

Smallest fix: validate semantic kinds as well as bounds. Generic component site/receiver bindings should point to admitted electrical/wire/observer nodes; pistons and contacts must use their specialized bindings. Test both wrong kinds and retain valid specialized routes.

## Should fix 4: redstone payload admission ignores stray entities

Location: `geometry.rs::classify_material`.

Counterexample: an imported/raw redstone block carries a stray comparator entity. The inert-cube branch checks actual entity absence, but the redstone-block branch bypasses that check. Native transport retains the entity; the geometry resource has only material and contact state, so it cannot preserve it.

Test: `redstone_payload_with_unmodeled_entity_is_rejected`.

Smallest fix: require entity-free admission for every supported payload material, including redstone blocks, or reject inconsistent block/entity metadata with a structured witness. This is malformed-input hardening, not an ordinary gameplay fixture failure. Do not broaden entity transport or add a carried-entity runtime model.

## Positive evidence

- 60 direct/one-cube directional lamp queries match native power reads (five source kinds, six faces, two paths).
- Normal/QC sampled availability matches native predicates across six piston facings and each tested neighboring/QC source location. Electrical preparation creates no callbacks.
- Each contact selects exactly one profile in the tested legal stationary poses and unavailable single-resource states, across all six facings.
- Restored head routes no longer add the native-excluded self-update.
- Missing duplicate callback edges now return an error without panic.
- Negated and optional receiver-selection guards now reject malformed routes after concurrent validation fixes.
- Existing targeted geometry, electrical, callback, graph, reference, legacy pass, direct backend, and native piston regressions remain passing.

## Commands and observed results

```powershell
cargo test -p mchprs_core --lib --locked deep_ -- --test-threads=1 --nocapture
```

Debug audit: 15 executed, 7 passed, 8 failed, none ignored. Failures represent the four findings above, including three fixtures blocked by signs. They are active tests and currently make the corresponding unit-test run fail; they were not hidden with ignore flags or assertions that expect incorrect behavior.

Existing checks, each run with `cargo test -p mchprs_core --lib --locked <filter> -- --test-threads=1 --skip deep_`:

| Filter | Passed |
| --- | ---: |
| `redpiler::analysis::tests::geometry_` | 12 |
| `redpiler::analysis::tests::electrical_` | 11 |
| `redpiler::analysis::tests::callback_routes::` | 6 |
| `redpiler::program_graph::tests::` | 6 |
| `redstone::instant_piston_tests::reference_contract::` | 4 |

Also ran the existing `redpiler::passes::` (12 passed), `redpiler::backend::direct::compile::tests::` (6 passed), and `redstone::piston::tests::` (42 passed, two existing ignored). All use the same debug/locked/lib workflow. Thus 99 existing tests executed successfully, separate from the new 15-test audit.

`cargo check -p mchprs_core --tests --locked` and `git diff --check` passed. Whole-workspace formatting check reports unrelated concurrent edits in `benches/support/cpus.rs` and native piston files; audit-owned Rust additions are formatted. Those concurrent files were not reformatted by this audit.

Release confirmation: `cargo test -p mchprs_core --lib --release --locked deep_ -- --test-threads=1 --nocapture` also executed all 15 new tests: seven passed and the same eight failed. Discovery with `cargo test -p mchprs_core --lib --locked deep_ -- --list` confirmed all 15 are registered. Earlier partial release runs are not substituted for this final audit.

## Review decision

Original decision: hold the parser gate until these findings are resolved. That decision is superseded by the follow-up review below.

## Follow-up review: fixes and wider fixture report

All four findings are resolved. Snapshot normalization preserves distinct component/resource/pending-work states, including the new pending-tick boundary regression, while same-state unobserved progress remains removable. Ordinary inert-sign callbacks are admitted; Shape validity remains checked, with changing support covered by geometry admission. Generic component bindings reject specialized owners, and all supported payload materials now require entity-free input.

Independently reran all 17 `deep_` tests in debug and release: 17 passed in each run, none ignored. Also reran callback routes (6), graph tests (10), geometry (12), electrical (11), and reference contract (7), all passing. Counts overlap because graph/reference runs include some deep regressions; they are not additive unique-test counts.

The fixture sweep reaches callback/state graph validation for 19 of 22 frozen fixtures, including OR, ADDER_1BIT, BUD_PistonUpdate, ADDER_11BITS, and COUNTER_BASIC. Three stop at geometry: AND_3 has a sign on changing piston support; NOT_1 and XOR_Simple require unsupported sandstone transport. These are explicit current subset restrictions, not claims that the latter circuits are intrinsically invalid.

Important boundary: fixture `Admitted` means geometry/electrical preparation plus callback/state projection validation. Electrical query plans are still outside `CircuitDraft`; Step 06 must carry those plans into the executable program without losing conductor policy, comparator overrides, guards, or wire context. These tests do not execute arithmetic or prove runtime parity.

`cargo check -p mchprs_core --tests --locked` and `git diff --check` passed. Focused edition-2024 rustfmt reports two nonsemantic formatting differences in the test/audit files; no source edits were made during this follow-up review.

Verdict: Step 05 and the supported static parser gate are approved; proceed to Step 06 with the query-plan integration requirement above. Broader runtime differential and continuation gates remain unchanged.

Not checked: full core suite, larger target-family preparation, graph runtime differential execution, Java/CPU captures, active compilation/reset continuation, or performance. The graph executor is not implemented at this stage; static correctness cannot certify runtime parity.
