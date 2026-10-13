# G3 — General graph-pass API and developer experiments

Status: proposed; requires reviewed G2. Read [the migration plan and API shapes](../GRAPH_PIPELINE_MIGRATION.md), `../AGENT_GUIDE.md`, Side B legality research and existing pass statistics.

## Goal

Developers can implement a pass, run it alone, compose an ordered pipeline, and enable/disable entries without modifying the runner or adding variants to a closed pass enum. Default compilation and experiments share validation, cancellation and reporting.

## API responsibilities

- `GraphPass` exposes a stable name, input-precondition validation and transformation of the common CompileGraph. Parameterized passes store their own parameters; use trait objects or an equally small existing mechanism, not a plugin framework.
- `PassContext` exposes monitor/options and read-only entry work. Source facts are compiled metadata; graph transformations do not require World in their signature.
- `PassEntry` owns a pass and its explicit enable state. `PassPipeline` owns the ordered entries; the default builder translates ordinary compiler options into that list.
- `PassOutcome` records actual reference remapping when a rewrite changes identities. Update graph-internal references during the rewrite and apply remaining compiler/projection remaps before validating or running another pass. Reuse the Side B mapping/disposition contract.
- A developer/test caller can construct a pipeline, insert/replace/reorder entries and change enable states before execution. Construction through the owned entry list is sufficient; do not create redundant builder methods for every operation.
- The runner knows validation/monitoring mechanics, not individual transformation names or their enable conditions. Parser, requested export and lowering are fixed compiler stages outside the reorderable transform list.
- `--optimize` affects the default transformation list. Explicit experiment pipelines can select entries independently; disabling all transformations still produces a complete unoptimized graph.
- The API is initially crate-level, usable by in-repository developers and unit tests. If external integration tests need it, expose only the validated preparation/pipeline entry points, not mutable runtime internals. No arbitrary public per-pass CLI flags are required.

## Validation and execution

1. Validate unique nonempty stable names and complete input graph before execution.
2. Check cancellation and each enabled pass's own preconditions before it mutates the graph. Preconditions concern that pass's semantic assumptions, not only ID validity.
3. Run entries in the supplied order; report enabled/skipped status, before/after counts and duration for every visited entry. Disabled entries leave graph/state references unchanged and do not run preconditions or transformation code.
4. Validate the graph after each pass in the experiment/test path and at production pipeline exit. Reuse existing structural validation. If production omits intermediate validation for measured cost, document that choice and retain per-pass preconditions and final validation.
5. Preserve initial state and imported pending work. If node IDs change, return/apply an explicit remap covering entry/projection references; do not leave a remap only in a diagnostic report. Use existing mappings where available.
6. Surface the failed pass name and error. The compiler discards an unpublished failed compilation; it need not clone the graph before every pass. Do not reuse a partially transformed artifact after failure.
7. Reordering may be invalid or produce a different optimized shape. Reject violated preconditions and compare observable behavior for accepted sequences. Structural validation alone is not an equivalence certificate.

## Scope and references

Reuse `TaskMonitor`, `PassStatistics`, `GraphCounts`, the promoted CompileGraph validator and existing errors where suitable. Make statistics describe general graph detail correctly; avoid assuming every pipeline has the old three required passes or a fixed number of optimization entries. Do not duplicate the fixed-component parser just to fit a uniform pass signature.

Primary files: `passes/mod.rs`, shared preparation, `compile_graph.rs` / `program_graph.rs`, `task_monitor.rs`, `redpiler/mod.rs` graph-statistics summaries, and selected existing pass modules. Record parser-complete and final graph baselines at explicit stage boundaries; statistics must not assume the baseline is always `passes[2]` when developers customize the list. Names/field sketches are in the main plan; every new field/enum variant needs a one-sentence explanation.

## Required evidence

- Run one meaningful existing supported transformation through this API, alone and in the default list; do not invent an unsafe rewrite solely to demonstrate dispatch. If no transformation is approved yet, use a clearly test-only pass over a valid synthetic graph and do not claim production optimization.
- An unregistered developer-defined pass executes without changes to the runner; parameterized behavior is exercised.
- Test individual enable/disable, all-disabled operation, supplied order, duplicate-name rejection, unmet precondition, cancellation between passes and a pass failure with no backend publication.
- Verify skipped entries do not call pass code and that progress/statistics match the chosen list rather than a hardcoded maximum.
- Validate an actual malformed transformation result is rejected. For a real transformation, compare a small interpreter episode before/after and preserve callback order/pending work.
- A comparator/dust graph and a piston/shared-resource graph can traverse the API even with no applicable optimization.

## Non-goals

No automatic pass-order search, general dependency solver, plugin loading, new configuration file, thread execution, physical fallback or aggressive behavioral rewrites. Do not turn the API into a catalogue requiring all possible passes to be known in advance. Stop after API review; G4 covers the runtime boundary.
