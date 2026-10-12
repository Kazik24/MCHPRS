# Graph backend differential fuzzing implementation plan

Status: implementation assignment for Luna6, scoped on 2026-10-12. Start with C0. Each later slice has its own review gate. This plan adds tests for the graph backend; it does not authorize implementing later piston runtime stages.

The campaign compares generated local circuits and input histories against the physical interpreter. Its first target is one piston surrounded by electrical and geometric context. Exhaustive coverage applies to a declared finite set of layouts and histories. Seeded generation then explores larger environments and longer histories. A passing campaign establishes agreement for that corpus, not universal equivalence.

## 1 Assignment and prerequisites

Read these sources before editing:

1. [Agent guide](../AGENT_GUIDE.md), [requirements](../FINDINGS.md), and [architecture](../ARCHITECTURE.md).
2. [Step 08](08-piston-transitions.md), [Step 09](09-shared-payloads.md), and [Step 10](10-activation-and-handoff.md).
3. [Redstone model](../../../REDSTONE_MODEL.md), [piston model](../../../PISTON_MODEL.md), and [interpreter architecture](../../../INTERPRETER_ARCHITECTURE.md).
4. The implementation and existing tests listed in section 2.

At the start of this assignment, the Step 08 notes describe isolated empty ordinary and single-resource sticky execution as implemented and awaiting review. Coupled instant chains remain unverified. The backend and its tests have ongoing working changes. Inspect the actual checkout; a commit title or a stale index status is insufficient evidence of support.

C0 and C1 can support the review of isolated piston execution. C5 requires reviewed shared-resource execution from Step 09. C6 requires the activation and continuation adapter from Step 10. Report unmet prerequisites without implementing them as part of this test assignment.

Implement only the assigned slice. The default first assignment is C0. After producing its code, executed checks, and report, stop for review. Do not start another slice automatically.

### Step 08 review cases to include first

Static review of the current working implementation identified these paths. Reproduce them as small native-versus-graph cases before claiming C0 parity. These are test assignments; fixes remain a separate assignment.

1. **Blocked extension:** an ordinary piston with a fixed container at Near receives normal power. The interpreter rejects the payload walk; graph sampling and acceptance must preserve the retracted pose and container. Use a container recognized by the current native `immovable_container` helper, rather than assuming vanilla immovability rules for arbitrary blocks.
2. **Dropped payload reuse:** extend a sticky redstone payload, reverse early, wait for base restoration with the resource retained at Far, then extend again. Near is empty for this second extension. The Far resource must stay enabled and must receive no transport/restoration callbacks solely because the head extends. Repeat with an inert payload and an observing consumer.
3. **Indirect external dust:** use a partial selection with outside dust feeding an inside consumer through a solid conductor, and another case feeding a piston through QC. Give the dust saved sides that differ from regulated sides. The current rejection checks only adjacency to component nodes; require an explicit unsupported-query rejection or correct compiled emission for every actual power dependency.
4. **Interrupted head removal:** extend an empty ordinary piston for one tick, then retract with a stationary observer at Far watching Near. The interpreter finishes the unavailable head and notifies its removal while the base is still extended, then begins base retraction and notifies Near again. Compare both callbacks, their order, and the transient base receiver eligibility.
5. **Normal sticky retraction completion:** hold extension until ready, retract normally, and observe the third restoration visit at Base and Near. Native creation order restores Base before the pulled resource at Near. Compare source-specific callback order, resource availability during the Base callback, and callback multiplicity. The graph currently enables the resource first and also delivers a separate Near head-restoration route.

Passing the previously reported 43 graph tests does not cover these histories. Keep confirmed divergences visible; do not reclassify supported cases as expected rejections or suppress callback records.

### Code style and scope

- Use small helpers named for fixture construction, input delivery, observation, comparison, and reduction.
- Keep the replay function focused on applying one action and comparing its resulting boundary.
- Name geometry, input values, eligibility, and deadlines explicitly. Avoid dense conditions and long iterator chains.
- Reuse existing Rust types, queues, recorder hooks, `rand`, and `serde_json`. Add no dependency, general simulator framework, or instruction VM.
- Test adapters may read the native world and graph descriptions. Graph execution must retain its existing prohibition on spatial world reads and native simulation calls.
- Keep new instrumentation behind `cfg(test)`. It must not change execution order, schedule work, or normalize component state.
- Preserve ongoing changes. Narrowly expose reusable test helpers rather than copying large existing test modules or rewriting their fixtures.
- Backend defects found by this campaign become minimized regressions and a separate implementation assignment. Do not weaken assertions or expand admission to make this assignment green.

## 2 Source map and proposed file layout

Paths below are relative to the repository root.

| Source | Reuse or inspect |
| --- | --- |
| `crates/core/src/redpiler/backend/graph.rs` | Internal `GraphBackend`, input operations, callback delivery, scheduler, piston actions, and deferred publication. |
| `crates/core/src/redpiler/backend/graph_tests.rs` | Existing isolated-piston setup, power queries, cancellation, early reversal, restored contacts, and native comparison helpers. |
| `crates/core/src/redpiler/analysis/tests/redstone_fuzz.rs` | Deterministic `StdRng` seeding and campaign reporting conventions. Its public compiler route can select native execution. |
| `crates/core/src/redstone/instant_piston_tests.rs` | Test-only `capture_at`, callback/event recording, and notification source metadata. |
| `crates/core/src/redstone/instant_piston_tests/reference_contract.rs` | Existing observable projection, deadline derivation, ordered comparisons, and comparison self-checks. |
| `crates/core/src/redstone/piston/trace.rs` | Native sample and acceptance instrumentation. |
| `crates/core/src/redpiler/analysis/geometry.rs` | Supported material, footprint, support, and entry rejection witnesses. |
| `crates/core/src/redpiler/program_graph.rs` | Authoritative state, immutable IDs, contacts, profiles, and resources. |
| `crates/core/src/plot/mod.rs` | Native tick and phase boundaries, scheduler snapshots, and stepping semantics. |

Start with `crates/core/src/redpiler/backend/graph_fuzz_tests.rs`, registered inside `graph.rs`:

```rust
#[cfg(test)]
#[path = "graph_fuzz_tests.rs"]
mod fuzz_tests;
```

Expected test discovery prefix: `redpiler::backend::graph::fuzz_tests::`.

Keep fixture generation and replay in that module initially. Split a reducer or saved-case reader only when its assigned slice becomes substantial. The file layout is a suggestion; test registration, clear responsibilities, and reuse are requirements.

The existing reference-contract types and functions are mostly private and tied to schematic manifests. Read them before choosing reuse. Extract or expose only the small comparison/projection operations needed by generated cases, through test-only visibility. Retain the existing tests unchanged in meaning. Do not expose test machinery through the production API.

## 3 Execution contract and outcome classification

Every positive case must:

1. Construct one explicit initial snapshot and input script.
2. Load two independent worlds from that snapshot, including entities and ordinary pending ticks.
3. Compile the graph copy through `GraphBackend::compile` directly.
4. Execute the reference with the interpreter and the compiled side with graph operations.
5. Compare matching semantic boundaries throughout the script.

Do not use `Compiler` as the execution selector for this campaign before Step 10 integration. Never run interpreted ticks on the graph-side world during replay. Native execution is allowed for the reference and explicitly named entry preparation only.

Record distinct outcomes:

| Outcome | Required treatment |
| --- | --- |
| Admitted and executed | Compare every declared boundary; count actual graph execution. |
| Expected unsupported case | Assert a specific diagnostic witness and unchanged input snapshot; count separately. |
| Unexpected rejection | Fail a case declared supported by the generator. |
| Observable divergence | Fail at the first differing boundary and preserve the complete case. |
| Invalid generated fixture | Fail the generator validation; keep it separate from backend divergence. |
| Replay or preparation budget exhausted | Report an incomplete case; it is never a parity pass. |

Determine a case's expected support from the generator's declared constraints and reference mechanics before invoking compilation. A compiler error cannot label itself an expected rejection. Use the structured parser error where the backend currently wraps preparation errors as strings.

An unsupported backend component must be a failure in a positive corpus. Do not catch it and advance only the reference, omit that component, or claim admission as execution coverage.

## 4 The initial piston fixture

### Physical layout

Use a loaded chunk with the piston near its center, for example `(8, 20, 8)`. Start from the small `PlotWorld` construction already used by graph tests. Use explicit selection bounds appropriate to those loaded chunks; the full plot corners may include chunks absent from this fixture.

Define:

```text
base = fixture origin
near = base + piston facing
far  = base + 2 * piston facing
up   = world Top
```

Place candidate environment blocks within a small active region, initially radius two around the actor and its contacts. Keep a loaded halo for power queries, the 24-site dust neighborhood, support checks, and notifications. Verify that the actual chosen probe/controller positions and their required neighborhoods fit the loaded selection. Increase the loaded fixture only for a demonstrated missing site.

Protect base, near, far, controls, support pads, and observation ports from arbitrary replacement in the positive generator. Axis occupancy belongs to the declared actor/payload template or a separately named negative case.

Provide two independently addressable controls:

- A normal-power source beside the base.
- A QC source positioned using world-up geometry.

Use existing lever setup and notification helpers where possible. Give controls stable supports independent of moving base/head/resource contacts. Verify their normal and QC query effects with the interpreter for each piston facing. An intended QC-only source must leave the normal channel unchanged when its level changes. Reposition a control when geometry creates another path; do not assume isolation from the diagram.

World-up stays fixed when the piston faces up or down. Rebuilding a fixture from its geometry rules is required. Rotating every coordinate and treating QC as piston-relative is incorrect. Even horizontal rotation does not justify equal callback traces: native absolute direction order can affect causality.

### Initial actor matrix

| Actor form | Payload at retracted entry | Payload at extended entry |
| --- | --- | --- |
| Sticky empty | None | None; matching head at near |
| Sticky redstone block | At near | At far; matching head at near |
| Sticky white wool | At near | At far; matching head at near |
| Ordinary empty | None | None; matching head at near |

Cross four forms with six facings and two entry poses: 48 actor templates. White wool is the first supported inert payload. Add required concrete and quartz variants in C3; do not assume arbitrary cube transport is admitted.

Entry must be stationary, BetweenTicks, with no active motion, queued piston events, or movement frontier. Ordinary scheduled ticks are added in C3. An extended but currently unpowered entry is a valid saved BUD state. It must stay unchanged until the reference receives work that samples it.

Build the declared head and resource state directly. Preserve raw saved component properties and dust strength/sides. Do not notify every block or warm up until it settles unless that preparation is explicitly part of a case, completed before the snapshot is cloned, and checked for stationary admission.

### Environment probes

Publish a deterministic six-site probe table for each facing before C1. Select sites from these roles:

- Normal-power/conduction context around the base.
- QC context above the base.
- A side of the prospective head.
- A side of the far resource contact.
- Dust step/ascent/descent context around a declared observation wire.

Derive candidates from the actual native query neighborhoods. Remove coordinate aliases, protected cells, and unsupported support arrangements. Keep world-up explicit. If six independent legal sites cannot express those roles for a facing, report the smaller table and correct the campaign count. Do not silently omit orientations or claim six-site coverage with fewer sites.

For C1, the palette is air, stone, glass, redstone block, and one declared dust state. Dust must have stable support and a recorded side pattern. Its initial power is zero. The air choice means no override at that probe. Fixed pads are part of the case descriptor and must not overwrite another probe or a protected contact.

In the positive subset, reference mechanics must not permit successful unsupported transport or component destruction. Support-sensitive and out-of-footprint arrangements belong in the separate rejection corpus. Legal blocked extension remains positive behavior.

## 5 Input actions and fixed protocols

Start with a small Rust action enum rather than a general interpreter for test instructions. Proposed responsibilities are:

```rust
enum Action {
    SetControl {
        input: Input,             // Normal or Quasi control.
        powered: bool,
        delivery: InputDelivery, // NormalUse or Silent.
    },
    Notify {
        target: NotificationTarget, // Base or Near site initially.
        direction: Option<BlockFace>,
    },
    Tick { count: u32 },
}
```

`NormalUse` follows the existing control's real notification procedure. Setting the same value is a no-op if that control's normal implementation is a no-op. `Silent` changes only the registered control's electrical property: the native side writes the corresponding control state without notifications; the graph side writes authoritative component state without firing routes. This is a test input operation, not a geometry edit or a piston-state assignment.

`Notify` delivers the native callback to the block currently at the selected site. The graph side resolves that compiled site's current receiver and invokes the matching callback family. A near site that currently holds air, a resource, an unavailable head, or a ready head must follow its actual receiver rules. Never call `sample_piston` unconditionally for every near-site notification.

Expand `Tick { count }` into single game-tick steps and compare after each one. An eight-tick wait has eight observable boundaries, not just one final assertion.

Establish eight fixed short protocol IDs in C0:

| Protocol | Operations and purpose |
| --- | --- |
| Idle | Eight ticks without external callbacks; preserve stationary disagreement and saved state. |
| Pulse one | Normal power on, one tick, off, then eight ticks. |
| Pulse two | The same sequence with two ticks before off. |
| Pulse three | The same sequence with three ticks before off. |
| Held | Power on for eight ticks, off for eight ticks. |
| Cancel before acceptance | Power on with normal delivery, silently remove power before the next action phase, then tick. |
| QC data then sample | Silently change QC, wait, explicitly notify base, then reverse QC and sample again. Verify the actual generated callback path. |
| Duplicate and alternating samples | Repeated same-value base callbacks, then high/low changes and callbacks before draining actions; check coalescing, FIFO, and cancellation. |

Specify the exact actions for each protocol in code and print them in failures. A protocol name alone is insufficient replay data. Keep supplied directions explicit; C1 expands direction-bearing and directionless callback cases as additional rows with separately reported counts.

Eight protocols across 48 templates yield 384 baseline episodes. Static environment sources can mask controls or legally block movement. Such cases may remain quiescent, but campaign reports must demonstrate accepted extension and retraction, pending restoration, cancellation, and same-value samples across the appropriate unmasked baseline cases. Agreement between two idle runs is insufficient evidence of piston execution.

## 6 Observation and comparison

### Boundaries

Compare before the first action, after each input/callback action, and after every completed game tick. Map native and graph phase names into the same logical scheduled/action/publication phases where phase observations are exposed.

C0 needs complete input and game-tick boundaries. Add synchronous operation observations where the existing recorder can capture them. Partial-phase stepping becomes an explicit C3 capability only when the backend offers a faithful test seam. Do not simulate a graph partial phase by draining a queue while leaving its clock and phase at an unrelated boundary.

Cancellation before acceptance can be tested between external actions without inventing a partial-phase API. Nested callbacks and individual publication order require observation hooks at their actual execution sites.

### Canonical observations

Reuse the reference contract's rules for these fields:

| Field | Required comparison |
| --- | --- |
| Clock and boundary | Logical tick, phase where observable, action ordinal, and boundary ordinal. |
| Electrical state | Named normal/QC/consumer strengths, wire strength and sides, powered/locked/lit state, and comparator output. |
| Piston state | Pose, base/head callback eligibility, and observable transient contact availability. |
| Resources | One token per declared resource, selected contact, availability, and provenance needed for pending publication. |
| Ordered operations | Samples, requests, acceptance attempts, accepted/cancelled actions, component callbacks, and wire writes. |
| Ordinary pending work | Position, expected registry type, remaining deadline, priority, duplicate membership, and FIFO order. |
| Delayed publication | Effective target contacts, deadlines, publication order, and replacement/cancellation dependencies. |

Identify graph nodes and resources by fixture roles and source positions rather than raw dense IDs. Normalize unrelated native motion identities while preserving identity equality and replacement relationships that affect future dispatch. Revision counter values need not match between implementations; their validity relationships and observable cancellation effects must match.

The oracle may inspect native motion metadata to derive resource availability and publication deadlines. That logic belongs in test projection. It must not become a graph executor or make the graph recreate physical progress.

If one graph event bundles base, head, and resource publication, project its actual execution order. Do not expand it into the order the interpreter should have used. If the current representation cannot expose a distinguishing boundary, report the missing comparison capability or concrete abstraction defect.

Compare callbacks that have a semantic receiver, retaining literal directions, supplied properties, duplicates, and nested order. Normalize receiverless native calls only through a documented no-handler rule. Never drop same-value callbacks to eligible components merely because they leave strength unchanged. Add minimal `cfg(test)` Shape instrumentation if required; lack of an existing Shape recorder cannot justify a claim of exact Shape parity.

### Permitted normalization

Ignore physical interpolation visits only when they publish no semantic change and cause no observable callback or change to the pending future. Do not collapse distinct snapshots because they share a tick and phase. Reuse the existing semantic-boundary preservation regressions.

An invalidated deferred event may be omitted from effective future work only after its inability to publish is established. Keep it in diagnostics. Preserve any queue/frontier relationship that changes whether replacement or newly created work gets a turn.

Do not compare only final stationary blocks, boolean gate outputs, or a settled truth table. Do not force settlement to avoid comparing pending state.

### Comparator calibration

C0 must include small checks that deliberately alter a copied canonical observation and require the comparison to fail for:

- Callback direction, order, or a missing duplicate.
- Strength 1 versus 15.
- A resource's contact or availability.
- A pending deadline or scheduled tick.
- A request's action or acceptance result.

These checks test the comparison function. Actual differential episodes must still compare independently executed native and graph runs.

## 7 Implementation slices and review gates

### C0 Baseline harness and independent comparison

1. Register the test module and verify discovery.
2. Implement explicit case construction, native/graph loading, actions, canonical observations, and first-difference reporting.
3. Run all 48 actor templates through all eight fixed protocols, restoring the initial snapshot for each episode.
4. Calibrate comparison failures using copied records.
5. Add observation hooks only for missing required fields, keeping them test-only.
6. Implement the five named Step 08 review cases above. Report their additional episodes separately from the 384 baseline episodes.

Gate: every declared episode executes through the graph, or remains a visible failing case with a concrete witness. Show one extension/retraction trace, one cancellation, one early reversal, and one QC data/sample separation. Report all missing observation capabilities. Do not claim completed parity while required fields remain unobserved.

### C1 Exhaustive single environment changes

For each actor template and protocol, enumerate the baseline plus one non-air override at every declared probe site. Five palette choices include four non-air overrides.

For six sites:

```text
environments = 1 + 6 * 4 = 25
episodes     = 48 * 25 * 8 = 9,600
```

Add callback-direction variants and report their additional episode count. Keep a separate named rejection suite for invalid support, unsupported transport, and footprint escape.

Gate: coverage counters reconcile the complete declared product; every unexpected rejection fails. Demonstrate a redstone payload affecting a consumer, an inert payload changing conduction context, and observer visibility at a changing contact. Observer fixtures can be named additions if they are outside the initial palette.

### C2 Exhaustive pairs and rejection witnesses

Enumerate every distinct pair of sites, with all four non-air variants at each. Include baseline and singles once when reporting the combined campaign.

For six sites:

```text
new pair environments = choose(6, 2) * 4 * 4 = 240
combined environments = 1 + 24 + 240 = 265
combined episodes     = 48 * 265 * 8 = 101,760
new pair episodes     = 48 * 240 * 8 = 92,160
```

Full Cartesian occupancy would be `5^6 = 15,625` environments and 6,000,000 episodes for this same template/protocol product. Treat that as a separately requested long campaign, with measured duration and coverage; do not call the sparse sweep a full Cartesian search.

Required negative families:

- Moved observers, electrical components, pistons, unsupported materials, and stray entities.
- Ordinary successful pushes outside the empty-actuator subset.
- Successful longer pushes and transfers outside the compiled footprint.
- Unsupported support loss, malformed/missing heads, moving entry, queued piston work, and non-BetweenTicks entry.

Keep permanently blocked extension positive when the supported model can represent it. Also include an empty ordinary actor with a stationary observer at far: fixed context is not transported payload.

Gate: first-divergence reporting works for paired interactions; each expected rejection has its intended structured witness and input-preservation assertion.

### C3 Seeded state and history campaigns

Reuse `StdRng::seed_from_u64` and the existing fuzz reporting convention. Default to 64 seeds for the ordinary test. Seed selects the complete fixture and script, not just the last input.

Extend one-piston environments with:

- Dust strengths 0, 1, and 15; dot, line, junction, and deliberately stale saved sides.
- Stable ascent/descent, transparent/opaque context, and live strong contributors.
- Floor/wall torches with legal stable support.
- Repeaters in all horizontal orientations, delays 1 through 4, and powered/locked combinations.
- Comparators in both modes, with valid entities and outputs 0 through 15.
- Observers in all six directions, including powered recovery and suppressed pending pulses.
- Bulb powered/lit combinations and supported concrete/quartz payloads.
- Imported ordinary ticks: due-at-entry work, stale type guards, duplicates, FIFO ties, and different priorities.

Generate bounded sequences up to 64 actions and 256 completed ticks per case, including repeated samples, silent data changes, held levels, rapid reversal, and repeated reuse. Do not require every episode to become quiescent; a supported oscillator can have a valid pending future.

Optional warm-up is an explicit case stage before cloning and must leave stationary admissible entry. Strict saved-state cases remain part of the corpus.

Use `MCHPRS_GRAPH_FUZZ_SEED` and `MCHPRS_GRAPH_FUZZ_CASES` as test-only controls, following the existing pattern. Reject invalid values and zero cases. Record the complete descriptor because RNG behavior can change with dependency versions.

Gate: repeat a seed range twice and obtain the same generated descriptors and verdicts for the same checkout/lockfile. Show admitted execution and temporal coverage counters, not just generated case count. Preserve the first failing cases even if the campaign continues to count additional failures.

### C4 Failure artifacts and deterministic reduction

Implement saved-case replay and reduction once the preceding generator/replay interface is stable. No new dependency is needed.

Reduce a divergence in this order:

1. Trim actions after the first divergence.
2. Remove action ranges while retaining the required initial state and same failure category.
3. Remove environment overrides and unused consumers.
4. Simplify components, strengths, delays, and waits toward simpler values.
5. Remove imported ticks one at a time, then simplify their deadlines/priorities when the failure remains.

Every candidate rebuilds both worlds from its descriptor and recompiles the graph. It must remain a valid admitted case and reproduce the same divergence category. Compilation failure, budget exhaustion, or a generator error cannot replace an observable divergence as a successful reduction.

Keep piston facing and entry pose fixed by default. Changing them creates a new test condition and can alter world-up QC and absolute callback ordering. Do not use assumed rotational equivalence as a reducer.

Gate: reduce one known injected comparison defect and one real backend divergence when available. If no real divergence exists, demonstrate deterministic reduction using a clearly labeled test-only injected observation defect. Promote real minimized failures into ordinary non-ignored regressions.

### C5 Shared resources and coupled actors

Prerequisite: reviewed Step 09 support. Extend to two actors sharing one resource, then supported three-actor sharing and cross-actor instant chains.

Exercise both action orders, overlapping controls, inhibition before/after acceptance, drop/recapture, temporary detachment, and replacement of pending publication. Test independent base/head and resource events: reversal of one actor must not cancel another valid resource publication.

Verify conserved resource count, exclusive selected contacts, and closure under all admitted histories. Fixed foreign bases/observers stay fixed context. Keep unsupported multi-resource collisions and longer pushes as explicit negative cases.

Gate: show the same initial geometry with different legal input orders producing different expected histories, and one obsolete publication correctly cancelled. Counts must show actual execution of each actor and shared token, not private copies.

### C6 Activation and continuation

Prerequisite: reviewed Step 10 integration. Add public backend selection checks and compare native execution with compile/run/reset/native continuation at stationary, queued-action, pending-publication, and supported partial-phase boundaries.

Repeat with display flush every boundary, delayed flush, and suppressed display. Compare semantic state and the future; a flush must not change execution. Never settle first to avoid reconstructing pending work.

Gate: exact pending continuation passes with preserved scheduler, contact selection, cancellation dependencies, and supported frontier order. Moving-entry support remains outside this plan unless separately authorized.

## 8 Replay data and failure reporting

Use a versioned local JSON case descriptor containing:

- Format version, campaign name, case ID, and seed or exhaustive enumeration coordinates.
- Source revision, dirty status, relevant registry version, and dependency lockfile identity when available.
- Loaded selection bounds, actor roles, facing, pose, and declared resources.
- Complete initial blocks, required block entities, and ordered ordinary pending ticks.
- Protected cells, environment probes, overrides, and named observation ports.
- Complete action script, support classification, and replay budgets.
- First divergence category, action/tick/phase/boundary, expected/actual record, and useful preceding trace context.

Store generated failures under `target/redpiler-fuzz/`, which is already ignored. Use separate filenames per case and avoid overwriting existing artifacts. Persist the original case before attempting reduction; retain both original and reduced cases. Filesystem failure must not hide the test failure: print sufficient replay data and the artifact-write error.

Add an ignored saved-case replay test selected with `MCHPRS_GRAPH_FUZZ_REPLAY`. Validate descriptor version, sizes, coordinates, registry compatibility, and action limits before allocating a world. A source revision difference should be reported rather than preventing replay against a fix. Record whether a fresh native oracle differs from the original oracle summary; do not silently rewrite frozen expectations.

Successful cases need small aggregate counters. Failure diagnostics should include the complete initial descriptor and script, with concise expected/actual differences. Keep only the last useful trace window in the human report; do not truncate semantic observations used to decide parity.

Reducers and debug trace collection have explicit budgets. A synchronous callback or walk that fails to return must be reported as a stuck replay using the test runner's process timeout. A between-ticks counter cannot stop a stuck synchronous operation. Initially run suspect long cases individually; add test-only work limits only where a real hanging case demonstrates the need. Do not build a worker-process framework for C0.

## 9 Test tiers and proposed names

| Tier | Contents |
| --- | --- |
| Ordinary tests | C0 baseline cases, comparator calibration, minimized regressions, small rejection checks, and the C3 default seed count when implemented. |
| Explicit exhaustive tests | Full C1 and C2 products, marked ignored with a descriptive reason until explicitly selected. |
| Explicit replay | One saved descriptor, selected by its required environment variable. |
| Long campaign | Larger seed ranges or separately requested Cartesian products, with a measured run budget. |

Proposed test names are `isolated_piston_baselines_match_interpreter`, `comparison_detects_observable_differences`, `exhaustive_single_environment_matches_interpreter`, `exhaustive_environment_pairs_match_interpreter`, `unsupported_piston_cases_preserve_entry`, `seeded_piston_histories_match_interpreter`, and `replay_saved_case`. These are planned names, not claims of existing executable tests.

Ordinary tests must not silently switch to fewer seeds because compilation failed or a case ran slowly. Select explicit campaigns with filters and ignored-test flags; keep the chosen corpus and counts visible.

## 10 Commands and acceptance report

After implementing and registering a slice, verify discovery before execution:

```powershell
cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::fuzz_tests:: -- --list
cargo test -p mchprs_core --lib --locked redpiler::backend::graph::fuzz_tests:: -- --test-threads=1
cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::fuzz_tests:: -- --test-threads=1
```

Select a planned exhaustive test explicitly after it exists:

```powershell
cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::fuzz_tests::exhaustive_single_environment_matches_interpreter -- --ignored --exact --test-threads=1
```

For C3, use process-scoped test controls in PowerShell and restore the session afterward:

```powershell
$previousGraphFuzzSeed = $env:MCHPRS_GRAPH_FUZZ_SEED
$previousGraphFuzzCases = $env:MCHPRS_GRAPH_FUZZ_CASES
$env:MCHPRS_GRAPH_FUZZ_SEED = "0"
$env:MCHPRS_GRAPH_FUZZ_CASES = "1000"
try {
    cargo test -p mchprs_core --release --lib --locked redpiler::backend::graph::fuzz_tests::seeded_piston_histories_match_interpreter -- --exact --test-threads=1
} finally {
    $env:MCHPRS_GRAPH_FUZZ_SEED = $previousGraphFuzzSeed
    $env:MCHPRS_GRAPH_FUZZ_CASES = $previousGraphFuzzCases
}
```

Use the analogous replay variable only for the saved-case test. No production config changes are needed.

Run the connected graph backend, parser, reference-contract, and native piston regressions when helpers, observation hooks, or projection rules change. Run focused formatting and `git diff --check`. Broaden testing for a concrete integration risk; do not repeatedly run expensive unrelated CPU captures.

The handoff must include:

1. Assigned slice, implemented behavior, affected files, and intentional limits.
2. Discovered/executed test counts, ignored counts, and exact commands/results.
3. Candidate, admitted/executed, expected-rejected, unexpected-rejected, divergent, and incomplete episode counts. Reconcile them with the declared product.
4. Coverage by actor form, facing, pose, material, probe, protocol, and relevant timing/acceptance event.
5. One readable operation-level trace and any first-divergence witness.
6. Original/reduced artifact paths, exact replay procedure, and new permanent regressions.
7. Missing capabilities or runtime defects for separate review, plus the next proposed slice.

End with what was skipped or not checked and the remaining material risk. Never report all piston circuits correct from this bounded corpus. Human review approves the next slice.
