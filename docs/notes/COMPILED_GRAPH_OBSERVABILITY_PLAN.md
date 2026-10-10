# Compiled graph observability: implementation plan for Luna 6

Status: implementation handoff only, 2026-10-10.
Baseline inspected: `redpiler/piston-1.21.5`, HEAD `7fc44e3`, with existing uncommitted investigation changes. Recheck the checkout before implementing; the working tree is the effective baseline.

## Objective

Provide an optional debug capture and a small presentation layer that links schematic wires and compiled connections to their signal/state at a selected moment. A user must be able to select a receiving repeater, follow its input upstream, and see why Direct delivered that strength.

The first useful result is an explanation of the early input at counter repeater `(15,9,64)`, including its source comparator and the shared payload group's actuator states. The observability change does not need to fix counter equivalence.

Use unoptimized Direct compilation for the first implementation. Keep repeater memory as ordinary graph nodes and internal instant execution expression-based.

## Scope and constraints

- Add an explicit `--trace` compiler option, default off. Reject its combination with `--optimize`, both through parsing and through direct `CompilerOptions` callers. Report unsupported executors clearly; this first version supports Direct.
- Capture initial state, ordered changes, and actual signal/activation deliveries. Export a versioned JSON document.
- Add a local HTML viewer with a schematic-position view, a connection view, time selection, and selection details. Use native HTML/SVG/JavaScript; no framework, package installation, server, or build pipeline.
- Reuse the existing graph IDs, positions, aliases, expression machinery, JSON support, and graph exporter.
- Keep diagnostics separate from electrical state. Trace collection must not schedule work, dirty production expressions, add timing boundaries, call native redstone functions, or change admission and ownership.
- Disabled tracing allocates no extra diagnostic buffer or wire plans and performs no extra expression evaluation. A cheap optional-recorder check at hooks is acceptable.
- Preserve every existing uncommitted investigation change. Do not clean up or rewrite the counter instrumentation as part of this work.

Out of scope: optimized tracing, a native executor trace framework, automatic interpreter comparisons, an in-game overlay, waveform-video generation, remote streaming, arbitrary Boolean DAG animation, and a counter fix.

## Read before editing

Read the relevant model/architecture documents and the current callers of each changed function. The following are starting points, not exhaustive hook coverage:

| Existing location | Role in this change |
| --- | --- |
| `crates/core/src/redpiler/mod.rs` | Compiler options, parser, compiler lifecycle, public capture/export entry point |
| `crates/core/src/plot/commands.rs` | Compile flags, command completion, trace export command, user messages |
| `crates/core/src/redpiler/backend/mod.rs` | Dispatch and unsupported executor behavior |
| `crates/core/src/redpiler/backend/direct/compile.rs` | Node IDs/aliases, initial strengths, imported pending work |
| `crates/core/src/redpiler/backend/direct/mod.rs` | `set_node`, connection delivery, input buckets, instant publication, tick phases, DOT export |
| `crates/core/src/redpiler/backend/direct/update.rs`, `tick.rs` | State changes, locking, scheduling and execution that bypass the central setter |
| `crates/core/src/redpiler/backend/direct/instant.rs` | Response/committed state, activation queues, output terms, shared geometry, bindings |
| `crates/core/src/redpiler/backend/direct/instant/logical.rs`, `instant/timing.rs` | Frozen expression snapshots and boundary transitions |
| `crates/core/src/redpiler/instant/logic.rs`, `outputs.rs`, `program.rs`, `boundary.rs` | Wire provenance, expression terms, arena compaction, ownership |
| `crates/core/src/redpiler/backend/tick_scheduler.rs` or the actual scheduler module found by `rg` | Accepted scheduling, imported deadlines and priority order |
| Existing compiler-option, command-completion, Direct and research tests | Update callers and add focused checks |

Put the recorder/schema in one small module following the existing module structure. Do not put export file I/O in electrical update loops. Locate the scheduler's actual path before editing; do not assume the suggested filename exists.

## Wire/state contract: implement this first

Every displayed wire has a stable identity and a real block position. Graph connections and physical dust segments are different objects; the viewer must say which is selected.

1. **Ordinary runtime wire:** show the actual Direct node's `output_power` (0–15), position and timestamp. Associate node aliases with their positions. Capture runtime state rather than reading a potentially stale display block from the world.
2. **Instant internal wire:** expose its compiled electrical strength expression as a diagnostic probe. Show its evaluated strength, exact snapshot/phase, dependencies and the label `expression-derived`. This is Direct's logical electrical value, not evidence of a native wire callback or a stored native block update.
3. **Unrepresented state:** show `unavailable` with a reason. Never substitute zero, a downstream port strength, a saved schematic value, or an end-of-tick value. Represent all dust positions in the selection; explain any gaps in live probe coverage.

Disabling optimization retains ordinary dust nodes, but does not retain dust owned by instant regions: `Boundaries::executable` hides those wires independently of `options.optimize`. Merely removing the DOT export's wire filter will not fulfill this task.

An edge label shows the source contribution after attenuation. A target can have multiple contributions: its aggregate main/side input is a separate value. Do not color every dust segment along a compiled route with the consumer's final strength.

### Expression probes

- Reuse extraction of per-position `PowerTerm`s where possible, with the same geometry and live-source meanings as executable output terms. The strength is the maximum enabled source contribution after saturating attenuation.
- `WaveLogic::handoff_wires` is a useful implementation reference, not an automatic source of live truth. Its extraction can replace internal reset sources with saved constants for handoff. Audit each reuse; bind live reset/observer state where the runtime represents it, or report unavailable explicitly.
- Keep probe roots through Boolean arena compaction and bind them using the existing source, memory, committed-state and geometry conventions. Do not independently reinterpret `Variable::Actuator` as a committed pose.
- Keep probes in optional diagnostic storage/plans. Adding probes must not grow production source bindings, change actor classification, or create new geometry boundaries. In particular, do not call `watch_geometry` just to make a displayed wire available; that method can create boundaries.
- Evaluate probes against a frozen snapshot at defined evaluation/publication checkpoints. Read the snapshot without altering the production dirty caches. Diagnostic evaluation may cost extra work only when tracing is enabled.
- Record changed probe values and their snapshot identity. If a probe has not been evaluated for the selected checkpoint, expose its age or unavailable status rather than presenting an earlier value as current.
- The initial implementation must give an evaluated value for the counter's relevant route at `(15,10,65)` and its output port. Unsupported unrelated probes may be unavailable, with a coverage count in the viewer.

## Capture format and lifecycle

Use one JSON document containing metadata, topology, baseline state and ordered events. Reuse `serde`/`serde_json`; no new dependency.

### Metadata and topology

- Schema version, capture/session ID, compile flags, selected bounds and coordinate origin.
- Replay tick convention and initial logical tick offset, when available. Keep world coordinates canonical; calculate schematic-local coordinates from the recorded origin, never from an assumed fixture offset.
- Runtime node ID/type/positions; actor IDs scoped by instant region; group membership; port IDs and receiving main/side channel.
- Physical wire positions, shape/connection metadata where supported, and their runtime-node or diagnostic-probe binding.
- Compiled edges with attenuation and channel; output terms with source and geometry dependencies. Include every shared-payload member in the dependency description.
- Initial node/probe/actuator/geometry values, plus imported pending scheduler and activation work that can affect replay.

IDs need only be stable within one capture. Record the compile baseline before the first external replay action. If compilation performs earlier initialization transitions, label them as initialization or explicitly state that their history precedes capture; do not imply a complete initialization trace.

### Ordered events

Each event has a global monotonic sequence, replay tick, execution phase and object ID. Add an explicit cause ID where known; do not infer causation from adjacent timestamps alone.

| Event | Required values |
| --- | --- |
| Node/state change | Previous/new strength and relevant powered, locked or stored state |
| Connection delivery | Source/target, main/side, attenuation, previous/new delivered contribution and resulting aggregate input |
| Scheduler acceptance/execution | Node, deadline, priority; record accepted work, not a request falsely described as accepted |
| Activation enqueue/delivery | Source kind/identity, target actor, eligibility, sampled desired response when delivered |
| Actuator response/commit | Desired response and committed state separately; use explicit extended/retracted names because `fired` can mean retracted |
| Boundary request/transition | Actor, requested state, previous/new phase and deadline |
| Instant output evaluation/publication | Source strengths, term guard results, attenuation, winner(s), aggregate result and actual publication |
| Wire probe change | Position, previous/new strength, binding kind and snapshot/checkpoint |

Capture relevant output-term explanation changes even if the final published strength is unchanged. Otherwise a permanently powered route could hide a geometry change. Record delivered activations even when they leave the committed pose unchanged.

Audit all scheduling and state-mutation paths: some update functions use the scheduler or mutate state directly. A hook only in `DirectBackend::set_node` or `schedule_tick` is insufficient. In `set_node`, retain the existing rule that all input strengths are published before consumer callbacks.

Align phases with the actual Direct order: imported/current-deadline work, advancing the ordinary scheduler, callbacks and intervening instant evaluations, owned-clock evaluation, end of tick, and flush/input actions outside a tick. Do not sort same-tick records after capture.

Start with a fixed cap, for example 100,000 records. On overflow stop recording, retain the valid prefix and expose the last captured sequence/tick plus `truncated: true`. The viewer must not offer later moments as fully captured. Keep the ordinary runtime running normally. Set the captable in an internal test; do not add a user-configurable tuning subsystem.

## Commands and export

Proposed workflow:

```text
/rp compile --trace --export-dot
# activate inputs and run the episode
/rp trace export
# open tools/redpiler_trace_viewer.html and select the exported JSON
```

- Wire up parser, option validation, command help/completion, backend dispatch and programmatic compiler access. Ordinary callers using `..Default::default()` keep tracing off; update exhaustive initializers.
- Export directly through a reusable capture API so the existing research harness can save the same document without a player or command.
- Use the project's existing generated-artifact location/conventions and a fresh capture filename. Return the path to the player; handle serialization/write errors and avoid overwriting an earlier capture.
- Export snapshots without consuming active recording, allowing another export after further ticks. Require export before reset/recompile; communicate that reset discards the current capture. Do not add capture history storage.
- Update the existing DOT exporter to include retained wire nodes and meaningful virtual-port provenance in trace mode. JSON is the viewer's source of truth; importing DOT or requiring Graphviz is unnecessary.
- Do not implement this solely using `tracing::debug!`: release builds use `release_max_level_info`, which removes debug logging.

## Minimal presentation layer

Commit one local HTML viewer under `tools/`, plus brief usage in `tools/README.md`. It loads an exported JSON file through a file input. Do not embed trace data into executable scripts or use `innerHTML` for imported labels; use DOM text nodes. Validate schema version and references before rendering, and display useful errors.

Required controls and views:

- A tick selector and previous/next event controls. Tick selection displays the final captured state for that tick; event selection allows inspection of intermediate same-tick values.
- A schematic-position SVG slice using the recorded X/Z positions and a Y-layer selector. Wire intensity reflects 0–15 strength, with numeric state available on selection; unknown/stale values are visibly distinct. Allow selecting positions by coordinates so a large counter remains usable.
- A small connection view around the selected object showing incoming/outgoing compiled edges, strengths, attenuation and main/side labels. Do not attempt a whole-counter force-directed layout.
- A details panel with canonical/local coordinates, node/probe/port IDs, value kind, snapshot age and recent changes. For repeaters include input, output, locking and pending work; for actors include desired response, committed pose, phase and group members.
- Selecting an edge shows its actual recorded contribution. Selecting an instant port shows the source terms and geometry conditions at that moment, including member-specific state.
- Show capture completeness and probe coverage. Explain that ordinary nodes are measured Direct state and internal probes are expression-derived. The viewer displays what Direct did, including incorrect signals; it does not certify interpreter equivalence.

Keep colors supplementary: include numeric labels/details and keyboard-operable controls. Reconstruct state from baseline plus events without mutating the imported document. Cache the current replay state if needed; add no general animation engine.

## Suggested implementation sequence

1. Inventory actual wire/state representations and current callers; define the schema and identity mapping. Record known probe coverage limits.
2. Implement optional recorder, baseline, ordinary node/connection events and accepted scheduler work. Add the programmatic capture API and a small ordinary-circuit check.
3. Add live internal wire probes, activation/commit/boundary records, and instant output explanations. Check the existing pose-retention case before adding the viewer.
4. Add command/export plumbing and the local viewer. Connect physical positions to captured IDs; verify intermediate event replay and alias handling.
5. Capture the existing six-tick counter route and inspect it in the viewer. Finish focused checks and concise documentation. Keep the counter fault and diagnostic changes separate.

## Acceptance checks to implement/run

The user has authorized implementation of this plan only when handing it to the implementing agent. At that point run the following focused checks; this planning turn runs no tests.

1. **Ordinary signal delivery:** a lever/dust/repeater circuit with nonzero attenuation and a side/lock input. Assert initial state, old/new contribution, aggregate inputs, deadline/priority and same-tick sequence. Verify zero-to-zero attenuated contributions are not invented as strength changes.
2. **Trace invariance:** replay the same circuit and instant activation circuit with tracing enabled/disabled. Compare electrical states, committed poses, locks, pending work and output waveforms. Do not rely only on final output equality.
3. **Retained pose:** use `unnotified_qc_falling_edge_preserves_committed_piston_pose`. Show desired power falling while committed pose stays extended and no activation is delivered; show the adjacent notifying control separately. Do not assert a desired-response change alone causes a commit.
4. **Shared geometry/probes:** source strength stays fixed while a member's boundary changes. Probe/port state must follow the same group geometry semantics as production, and explain a changed guard even when another term holds the output constant.
5. **Counter route:** capture `pc_counter_memory_input_publication_route` and select repeater `(15,9,64)`, wire `(15,10,65)`, comparator `(16,10,66)`, and payload `(16,10,65)`. Both members at `(18,10,65)` and `(16,12,65)` must be discoverable. Existing actor numbers 340/435 are diagnostic references, not stable IDs to hard-code. The observed earlier run had Direct input one during ticks 1–6 while native input was zero during ticks 1–5; a trace should expose Direct's actual behavior, not manufacture matching expectations.
6. **Lifecycle/errors:** tracing defaults off; incompatible flags and unsupported executors are clear; cap overflow preserves a valid prefix; export can repeat without consuming recording; reset clears capture; bad file/version/references fail visibly.
7. **Presentation replay:** use a tiny fixture with two changes in one tick, an unknown probe and aliases. Check tick-end and per-event selection, 0–15 labels, stale/unknown handling, coordinates and JSON round trip. Follow repository conventions for a small assert-based viewer check without installing a JS test framework; manually open the viewer for the counter capture.

Recheck the fixture identity when running research diagnostics:

- Original revised counter: `test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem`, SHA-256 `641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7`, dimensions 24 × 19 × 75.
- Measurement fixture: `test_data/piston-research/pc-counter-measurement-points-20261010/PC_COUNTER_MESURMENT_POINTS.schem`, SHA-256 `0749f1b8d5e52e13f62323d42aa194b5d1440f48fcc679aaf687eaa67d5e078b`, dimensions 24 × 19 × 76. Its terminal coordinates differ; do not mix fixture-local positions.

Relevant existing commands:

```text
cargo test -p mchprs_core unnotified_qc_falling_edge_preserves_committed_piston_pose -- --ignored --nocapture
cargo test -p mchprs_core pc_counter_terminal_activation_handoff -- --ignored --nocapture
cargo test -p mchprs_core pc_counter_memory_input_publication_route -- --ignored --nocapture
```

Counter equivalence remains a separate failing regression. Broad workspace tests and CHUNGUS hash replay are outside this observability change unless another instruction requires them. Report precisely which checks ran and which did not.

## Definition of done and handoff response

- A fresh unoptimized Direct trace is exportable from both the command and the research harness.
- The viewer links physical dust positions and compiled connections to the captured signal at tick/event granularity.
- The counter's target route can be followed from repeater input to source terms and all payload owners without ad hoc console instrumentation.
- Desired responses, activation delivery, committed poses and geometry phases are separately visible.
- Measured/derived/unavailable values are truthful, and disabled tracing preserves existing behavior.
- The implementing agent reports changed files, usage, focused checks, probe-coverage limits and remaining counter failure. It does not claim that observability fixes replay equivalence.

Known risk: expression-derived internal wire values can differ from native stored wire state and callback history. Preserve that distinction in both capture and presentation; otherwise the tool could hide the exact class of fault it is meant to reveal.
