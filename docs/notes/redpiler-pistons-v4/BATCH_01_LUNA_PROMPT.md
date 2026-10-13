# Luna6 implementation prompt — common graph pipeline, batch 01

Implement G1, G2 and G3 of `docs/notes/redpiler-pistons-v4/GRAPH_PIPELINE_MIGRATION.md` as one reviewable batch. Work in this repository, inspect its current state, make the changes and run meaningful focused checks. Stop after completing this batch and report for human review. Do not implement G4–G7.

## Authorization and scope

The user explicitly requested several steps together. This overrides the older one-step-only and intermediate human-review instructions in `AGENT_GUIDE.md` and the G1–G3 documents for this batch only. Implement sequentially and check each intermediate result; you do not need separate permission between G1, G2 and G3. The final human review remains required before later steps.

This is a compiler integration batch. GraphBackend remains the target executor. Do not move Graph execution into Direct, merge native simulation into Graph, delete Direct/Native, change production backend selection, or change command/note/trapdoor admission policy. The earlier generalized-Direct proposal is superseded.

## Required reading

Read applicable AGENTS.md files, then:

1. `docs/notes/redpiler-pistons-v4/GRAPH_PIPELINE_MIGRATION.md` — agreed architecture, ownership and API shapes.
2. `docs/notes/redpiler-pistons-v4/steps/graph-01-shared-preparation.md`.
3. `docs/notes/redpiler-pistons-v4/steps/graph-02-common-graph-and-export.md`.
4. `docs/notes/redpiler-pistons-v4/steps/graph-03-pass-api.md`.
5. `docs/notes/redpiler-pistons-v4/AGENT_GUIDE.md`, `ARCHITECTURE.md`, and `GRAPH_JSON.md`.
6. `docs/REDSTONE_MODEL.md`, `docs/PISTON_MODEL.md`, and `docs/INTERPRETER_ARCHITECTURE.md` for the reference behavior affected by the refactor.
7. Side B legality/remapping research when touching transformations: `steps/side-b-basic-passes-research.md` and `steps/side-b-behavioral-pass-scope.md`.

Do not assume implementation status from an old report. Inspect the actual source and tests. Existing passing Graph tests establish covered histories; this batch must preserve those histories and does not establish universal runtime correctness.

## Target result

```text
World + entry state
  -> required parser stages
  -> complete general CompileGraph
  -> configurable graph-pass pipeline
  -> validation
  -> export this artifact, if requested
  -> executable lowering
  -> existing Graph execution
```

The ordinary legacy production path remains working during this batch. Its electrical graph is explicitly a legacy representation; do not feed rich links to its existing rewrites or packed encoder.

### G1: shared preparation

- Extract the geometry/electrical/callback sequence and graph assembly from backend-owned parsing into shared compiler preparation. Keep the parser stages readable and independently testable, with one budget and monitor.
- Inspect provisional `passes/rich.rs`; complete or replace it rather than creating another competing sequence.
- Make Graph compilation and standalone JSON inspection use this preparation path. Tests should use the same preparation entry point or a thin helper, not duplicate the parser.
- Keep parser errors attached to compiler stages and preserve their concrete positions/witnesses. Preserve cancellation and transactional publication.
- Keep runtime behavior unchanged. Any remaining entry-tick metadata reads belong to the compilation boundary; do not expand this into G4's runtime redesign. Report deferred boundary work explicitly.

### G2: one general compilation representation

- Promote/reuse `CircuitDraft` as the general `CompileGraph`. Rename the old electrical graph to `LegacyCompileGraph` while its consumers remain. Update affected imports/callers/enum matches together; no third graph vocabulary.
- Retain power/update/shape links, guarded contacts, route order/directions/duplicates, wire plans, comparator channels, profiles, shared resources, provenance and initial values. Keep entry time/ticks owned once and lower once.
- Preserve JSON's inspection-only relations without storing another runtime state authority. Serialization consumes the actual compilation artifact, not a second parser or a stale copy retained before transformations.
- Build complete generalized dust/contact/update detail regardless of `--optimize`. Identification-time wire elision remains only in the temporary legacy path.
- Expose structural validation needed by the pipeline; reuse existing checks. Validate before and after transformations, and at lowering.
- Keep binary/DOT compatibility where representable and existing explicit rejections otherwise. Do not truncate rich information to fit old exports or invent a silent format change.

### G3: general pass API

- Implement the minimal general interface described in the plan: a stable pass name, input-precondition check and transformation; shared context; ordered enabled/disabled entries; outcomes/remapping when actually needed.
- A developer-defined, parameterized pass must run without editing the runner or adding to a closed enum. Developers can build their own list, reorder/replace entries and run a pass alone.
- The compiler's default builder selects its entries. The runner does not hardcode individual passes, optimization enable decisions or a fixed count/order.
- Parser, mandatory validation, requested export and lowering are fixed boundaries, not arbitrary reorderable transformations. Disabling all rewrites still produces the complete graph.
- Reject duplicate/empty stable names, unmet preconditions and malformed results. Check cancellation between passes. Include the failed pass name in errors. A failed unpublished compilation can be discarded without cloning each pass input.
- Disabled entries must not call pass validation/run code or mutate data. Report skipped entries, actual before/after counts and duration. Record explicit parser-complete/final graph baselines; do not assume `passes[2]` is always the baseline.
- Apply required graph/compiler/projection reference remaps before another pass or export. Do not build a universal remapping framework if this batch's reviewed transformations preserve IDs; document that scope and avoid pretending a report-only map is applied.
- Do not port unsafe legacy optimizations merely to populate the pipeline. If no production rewrite is approved, an empty default transformation list is valid. Exercise extensibility using clearly test-only passes and describe production optimization as deferred.

## Important sources and connected callers

Under `crates/core/src/` inspect:

- `redpiler/mod.rs`: compilation, publication, entry work, errors and statistics.
- `redpiler/passes/mod.rs`, `passes/rich.rs`, existing pass modules and their tests.
- `redpiler/compile_graph.rs` and `program_graph.rs`: both representations, validation, IDs, initial state and lowering.
- `redpiler/analysis/{geometry,electrical,callbacks,graph,export_json}.rs` and connected tests.
- `redpiler/backend/{mod,graph,native,queue}.rs` and `backend/direct/`: existing execution and legacy dependencies.
- Graph regression, fixture, fuzz and continuation tests; pinned schematic loader/reference-contract helpers.
- `redpiler/task_monitor.rs`, `plot/commands.rs`, and any export/viewer/report consumers affected by data or type changes.

Use `rg` to enumerate callers and renamed type references before editing. Read connected code, not only the functions named here. Inspect `git status` and preserve concurrent user/agent changes. Do not revert unrelated edits, modify fixtures/manifests to hide failures, or commit unless explicitly asked.

## Readability and limits

- Reuse the existing parser, scheduler, validation, DTOs and fixtures. No new dependency, VM, plugin loader, configuration language, pass-order solver, worker pool or speculative caching.
- Use named intermediate variables and small semantic operations. Separate parsing, transformation, serialization, lowering and publication; do not create a giant orchestration function with all internals inlined.
- Give every new struct field and enum variant a short one-sentence explanation. Keep names descriptive; no step-number test folders or artificial regression names.
- Preserve power versus callback semantics, normal versus QC input, Update versus Shape, live versus captured reads, event ordering, pending work and continuation. Do not deduplicate ordered notifications as electrical edges.
- No physical Motion/progress representation, runtime spatial discovery, interpreted dust, native fallback or reachable `todo!()` handler in an admitted graph.
- Add a `shortcut: <limit>, <when to upgrade>` comment for a genuine implementation limit. An incomplete requirement is not satisfied by a silent no-op.

## Verification required for this batch

Use existing meaningful tests and add targeted checks for the new integration/API. Discover exact tests before running them; do not report a zero-test command as success.

1. Shared preparation/export agreement for real compiled input: IDs, guards, route order/duplicates, initial values and resource selections, not just counts. Reuse a pinned sticky/shared fixture and an ordinary comparator/dust case.
2. Complete rich graph with optimization enabled/disabled; export matches the artifact passed through the pipeline. For a real rewrite, export must reflect its actual result. Otherwise report the empty production rewrite list truthfully.
3. API tests for a developer-defined parameterized pass, supplied order, enable/disable, all-disabled operation, duplicate/empty names, precondition rejection, cancellation, malformed output and pass failure without backend publication. At least one test actually changes a valid synthetic artifact; observer-only probes do not test output validation.
4. Preserve typed links at boundary strengths, directional duplicate callbacks, contact selection and pending entry references through lowering. Test remapping if a real rewrite changes IDs.
5. Cancellation and geometry/pass failure preserve input world/entities/ticks and do not publish an active backend or misleading success. Reuse existing read-counting/cancellation helpers.
6. Existing focused Graph execution/continuation episodes still pass. Ordinary legacy compilation and export rejections remain covered; tests claiming Graph execution must explicitly demand Graph.
7. Run `cargo check -p mchprs_core --tests --locked`, focused formatting and `git diff --check`. Run relevant release-locked suites and debug runs for new nontrivial integration/API checks. Use descriptive filters discovered from this checkout.

An appropriate discovery command is `cargo test -p mchprs_core --lib --release --locked redpiler:: -- --list`; select the relevant suites from its actual output. Do not automatically run full CPU captures, broad fuzz campaigns or the whole workspace. Expand verification when a concrete changed dependency or unresolved failure justifies it.

## Finish and report

Update the G1–G3 status notes to implemented/awaiting human review only for genuinely completed requirements. Do not mark G4–G7 started. Explain any current interface sketches you adjusted and the concrete reason.

Final report must include:

1. What changed and which production consumers now use shared preparation, artifact export and the general API.
2. A short list of remaining legacy paths and deliberately deferred runtime/optimization work.
3. Exact test discovery/run commands, discovered/executed counts and outcomes; distinguish skips and failures.
4. One useful artifact/trace example proving ordered links and initial state survived, plus the extensibility test demonstrating a custom pass.
5. Known risks and the proposed next batch, without implementing it.

Stop for human review after this batch. End with one or two lines stating what you did not check and the material remaining risk.
