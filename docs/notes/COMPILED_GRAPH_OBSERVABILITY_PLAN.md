# Optional graph presentation correlations: plan for Luna 6

Status: historical proposal. This predates removal of the instant piston
compiler; references to instant extraction and execution are not current
implementation targets. The ordinary graph observability ideas remain notes,
not an implementation handoff.

Inspected baseline: `redpiler/piston-1.21.5`, HEAD `7fc44e3`, with uncommitted investigation work. Recheck the checkout and preserve all existing changes; other investigation work may continue while this plan is implemented.

## Objective and first deliverable

Add an optional correlation list describing **which compiled edge corresponds to which physical wire positions**. The presentation layer joins this list to captured runtime signals: selecting an edge highlights its wires; selecting a wire finds its compiled connections and available state.

Keep correlations independent of electrical execution and the renderer. A future parser debugger should be able to use the same data to explain how physical wiring became graph edges.

The first complete deliverable is an optional presentation sidecar, optional ordered signal capture, JSON export, and a small local viewer linking positions, edges and recorded values. Repeater memory stays ordinary graph nodes; instant execution stays expression-based. This work exposes the counter's fault and does not need to fix equivalence.

## Presentation and optimization policy

Presentation is optional **data**, not a property of the electrical model. Use `Option<PresentationMap>` or an equivalent house structure. `None` means correlations were not retained; an empty retained map is different.

Resolve retention once at compile entry and thread the decision through preparation/lowering. First-version behavior:

| Mode | Presentation correlations | Optimization |
| --- | --- | --- |
| Default, no `--optimize` | Retained | Off |
| `--optimize` | Absent | On, subject to existing executor rules |
| `--trace`, no `--optimize` | Retained, with signal recording | Off |
| `--trace --optimize` | Reject clearly | No compilation |

Invariant: **optimization requires presentation to be absent**. `--optimize` requests optimization and removal of presentation. Never optimize with partial or misleading correlations still attached.

Prepare for a future separate retention flag through this single policy decision; do not add an unused flag now. The future flag can allow unoptimized compilation with presentation disabled. An explicit future request to retain presentation must conflict clearly with optimization. Avoid scattered `!options.optimize` conditions throughout extractors and renderers.

Validate this for programmatic callers as well as command parsing. Static correlation retention does not imply recording events. Absent presentation allocates no correlation/path buffers or reverse indexes. Preserve the packed `ForwardLink` layout and electrical data structures; a cheap optional-recorder check at runtime hooks is acceptable.

## Read and reach before editing

Read the relevant architecture/model references and find all callers of changed functions. Starting points:

| Location | Responsibility |
| --- | --- |
| `crates/core/src/redpiler/mod.rs` | Retention policy, `--trace`, lifecycle and export access |
| `crates/core/src/redpiler/compile_graph.rs` | Compile identities; keep presentation in a sidecar |
| `crates/core/src/redpiler/passes/mod.rs` | Optional presentation through preparation |
| `crates/core/src/redpiler/passes/input_search.rs` | Ordinary physical route/edge provenance at discovery |
| `crates/core/src/redpiler/passes/identify_nodes.rs`, `clamp_weights.rs` | Virtual output edges and removed edges |
| `crates/core/src/redpiler/instant/logic.rs`, `outputs.rs`, `program.rs`, `boundary.rs` | Instant route/term provenance, ownership and arena compaction |
| `crates/core/src/redpiler/backend/direct/compile.rs` | Compile-to-runtime node/link remapping |
| `crates/core/src/redpiler/backend/direct/mod.rs` | Strengths, connection delivery, tick phases and DOT |
| Direct `update.rs`, `tick.rs`, `instant.rs`, `instant/logical.rs`, `instant/timing.rs` | Mutation paths, locks, activation, commitment, geometry and publication |
| `crates/core/src/redpiler/backend/mod.rs` | Dispatch and unsupported executor behavior |
| `crates/core/src/plot/commands.rs` | Parsing/completion, export command and errors |
| Existing compiler, command and Direct tests; research regressions | Callers and focused checks |

Prefer one small presentation/trace module following house conventions, without a generic instrumentation framework. Locate the actual scheduler module before adding scheduling hooks.

## Correlation contract

Use stable identities within one compile/capture. Endpoints alone cannot distinguish parallel edges with equal channel and attenuation. A practical runtime identity is `(source_node, outgoing_link_index)` with a capture-local exported edge ID assigned during lowering.

Each record needs:

- Edge ID, source/destination IDs and main/side channel.
- Associated physical wire positions in canonical world coordinates.
- Source/consumer positions, when represented, and instant port/term identity where applicable.
- Compiled attenuation and geometry conditions. Per-segment attenuation is optional and must come from extraction, never be guessed from the final edge weight.
- Provenance kind: ordinary electrical route, instant output term, or activation dependency. Power and activation are separate relations.
- Coverage status/reason if physical provenance is incomplete or there is no dust segment.

Support many-to-many correlations: one edge can map to several wires; one wire can map to several edges. Include fanout and alternative guarded routes. A direct source-to-consumer edge can legitimately have an empty wire list.

The list expresses associations unless a complete ordered traversal was retained. Do not render sorted coordinates as an ordered path or guess physical links between distant points. Store the list once; build the reverse wire-to-edge index in the viewer. Include node aliases in position mapping.

### Capture provenance during extraction

- Record associations while the existing electrical search discovers links/terms, preserving that search's decisions. Do not reconstruct them by spatial adjacency or another native simulation.
- `input_search::link_source` creates ordinary edges; inspect `search_wire` and retain associated dust positions only when presentation is requested.
- Map virtual output edges from `identify_nodes` to their original ports/terms. Carry term identities through arena compaction; expression IDs are not stable identities.
- For merged traversal results retain associated positions and route/guard identity, without enumerating every simple path through loops. Avoid presenting a union as one traversable path.
- After mandatory weight clamping, discard records for removed edges. During Direct lowering, remap surviving compile edges to actual runtime links. Export no dangling IDs.
- Optimization receives no sidecar; preserving provenance through all optimization rewrites is out of scope.

Instant-owned dust is hidden by `Boundaries::executable` even without `--optimize`. Simply including ordinary wire nodes in DOT is insufficient. Include instant routes' physical positions and port/term identities in the correlation list.

## Link correlations to truthful state

Runtime signals are authoritative; presentation metadata must not calculate or modify production state.

| Selection | Displayed value |
| --- | --- |
| Retained ordinary wire | Actual node `output_power`, position and event timestamp |
| Compiled edge | Delivered source contribution after attenuation, separate from aggregate target input |
| Instant output port | Evaluated/published strength, source terms and geometry explanation |
| Internal dust represented only by a route | Associated edge contributions; local wire strength unavailable unless independently represented |
| Piston/group | Desired response, committed state and geometry phase separately |

Do not color all highlighted wires with the consumer's final strength. Attenuation, multiple sources and geometry make that false. Use highlighting for correlation and signal color only for a known value; unknown is not zero. Do not read stale world display blocks as authoritative Direct state.

First-version internal-wire support is correlation plus the port/edge's actual signal. A general live per-wire expression-probe system is deferred. Existing valid live values can be exposed with their kind/snapshot, but never label an expression-derived value as native committed state. Do not automatically reuse `WaveLogic::handoff_wires` as live probes: handoff extraction can substitute saved internal/reset strengths.

The counter wire `(15,10,65)` must link to its instant port/term and source/geometry dependencies, even if Direct does not store its local dust state. Show that limitation explicitly in the viewer and implementation report.

## Minimal signal capture and export

Add `--trace`, default off, to enable a bounded recorder when presentation is retained. Static correlations remain useful/exportable without recording for future parser inspection.

Export one versioned JSON document: compile flags, selection bounds/origin, topology, correlations, baseline, ordered events and completeness. Scope actor/group IDs by instant region. World coordinates are canonical; derive local positions from the recorded origin.

Each event has a monotonic sequence, replay tick, execution phase and object ID. Capture:

- Node strength/powered/lock/stored-state changes and delivered edge contributions.
- Aggregate main/side inputs separately from contributions.
- Imported pending work and accepted scheduling/execution needed to explain repeater timing. Audit direct scheduler calls; a wrapper missing them is insufficient.
- Activation enqueue/delivery/eligibility, sampled desired response and actual commitment.
- Boundary requests/transitions and all shared-payload members.
- Port publication and source-term/guard explanation changes even when another term holds the output constant.

Preserve ordering within a tick. Capture a baseline before external replay actions and state whether earlier initialization history is outside capture. Use the same snapshot as publication without mutating production caches or adding boundaries. Do not call `watch_geometry` solely for presentation: it can create timing boundaries.

Use a fixed cap, for example 100,000 records. On overflow retain the valid prefix, mark truncation/last complete point and keep normal execution running. No configurable buffer subsystem.

Provide a reusable capture/export API for research tests and command workflow:

```text
/rp compile --trace --export-dot
# run the episode
/rp trace export
# open tools/redpiler_trace_viewer.html and select the exported JSON
```

Allow static export without `--trace` if presentation exists; label it as having no recorded waveform. Follow existing artifact conventions, use fresh filenames, handle errors and return the path. Export does not consume active recording. Require export before reset/recompile; do not add capture history storage.

Include retained wire nodes and virtual-port provenance in DOT when presentation exists. JSON drives the viewer; no Graphviz dependency. Debug logs alone are insufficient because release builds strip debug logging.

## Minimal presentation layer

One local HTML/SVG/JavaScript file under `tools/`, with usage in `tools/README.md`; no dependencies or server.

- JSON file input with schema/reference validation and safe text-node labels.
- Tick-end selection and previous/next event controls for intermediate same-tick changes.
- Schematic X/Z slice with Y-layer selection and coordinate lookup.
- Edge selection highlights correlated wires; wire selection lists correlated edges and available node state.
- A small incoming/outgoing connection view with contributions, attenuation, channels and aggregate input clearly labeled.
- Details for port terms, guard conditions, all payload owners, activations and committed pose.
- Explicit unavailable/derived/measured state, coverage and truncation. Numeric values and keyboard-operable controls supplement color.

Defer a whole-counter force-directed layout, animation framework, browser parser and interpreter comparison UI. The future parser debugger consumes the same correlation contract.

## Implementation sequence and acceptance checks

1. Central retention policy and optional sidecar; assert optimization has no presentation; update callers/exhaustive options.
2. Ordinary and instant extraction correlations plus lowering remaps; test before UI work.
3. Optional capture, export API/command and ordered signal/state records.
4. Viewer joining physical wires to edges/state, with honest local-state limits.
5. Inspect the counter route and report coverage and remaining fault.

Focused checks for the implementing agent:

- Default compilation retains presentation, optimized compilation has `None`, and optimized trace fails through commands and direct API. Exercise unoptimized no-presentation at the internal policy boundary, ready for a future flag.
- Parallel edges remain distinct; fanout maps a wire to several edges; direct connections allow no wires; clamped edges leave no dangling records.
- A lever/dust/repeater circuit exports known positions, attenuation, main/side contributions and ordered delays. Compare full waveforms/pending work with tracing on/off, not just final output.
- A guarded instant route exports physical associations and every shared-payload owner. Metadata/tracing must preserve classification, production source bindings, boundaries and deadlines.
- Pose-retention regression shows desired response and commitment separately, including deliveries that leave pose unchanged.
- Overflow, repeated export, reset, absent presentation, unsupported executor and malformed viewer data report clearly.
- A tiny JSON fixture checks reverse selection, parallel edges, same-tick replay and unavailable local wire state. Use a small assert-based check following existing tools without installing a JS framework, then manually inspect a real capture.

Existing focused diagnostic commands:

```text
cargo test -p mchprs_core unnotified_qc_falling_edge_preserves_committed_piston_pose -- --ignored --nocapture
cargo test -p mchprs_core pc_counter_terminal_activation_handoff -- --ignored --nocapture
cargo test -p mchprs_core pc_counter_memory_input_publication_route -- --ignored --nocapture
```

Route diagnostic fixture: original revised `TEST_POTADOS_PC_COUNTER.schem`, SHA-256 `641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7`, 24 × 19 × 75. Schematic-local positions: repeater `(15,9,64)`, wire `(15,10,65)`, comparator `(16,10,66)`, payload `(16,10,65)`, owners `(18,10,65)` and `(16,12,65)`. Earlier actor numbers 340/435 are not stable identities to hard-code.

The measurement fixture is 24 × 19 × 76, SHA-256 `0749f1b8d5e52e13f62323d42aa194b5d1440f48fcc679aaf687eaa67d5e078b`; do not mix its local coordinates with the original fixture.

## Definition of done

- Optional correlations connect physical wire positions to surviving runtime edges and instant terms.
- Optimization requires and guarantees absent presentation.
- Static correlation export works independently of recording and can support a future parser debugger.
- Viewer shows actual recorded values for represented objects and honest unavailable local internal-dust state.
- Counter input can be followed to source and group geometry without ad hoc coordinate logs.
- Tracing preserves circuit behavior; implementation report names changed files, usage, checks, coverage limits and remaining counter failure.

Skipped: general live internal-wire probes, optimized provenance, parser debugger UI, broad workspace checks and CHUNGUS replay.
Risk: correlation alone does not establish local stored/native wire strength. Keep that distinction visible; observability does not fix the counter's replay mismatch.
