# Player-facing Redpiler analysis and instant assertions

Status: scoped, not implemented. Written 2026-10-24 against source revision
`2f5274a7bbb977290ce36cc89d625e1fe843249e` and the current worktree.
This proposal does not change compilation or execution semantics.

## Purpose

Let a player select a block or network and understand its inputs, outputs,
ownership, execution mode, timing dependencies, and available proof evidence.
Reuse the compiler's prepared graph and existing viewer-only highlights.
Keep inspection data out of the ticking hot path.

The separate `--assert-instant` proposal makes proof coverage a compilation
requirement. It must distinguish an unsupported proof from a demonstrated
noninstant behavior.

## Existing implementation to reuse

| Source | Available information or behavior |
| --- | --- |
| [Command handling](../../../crates/core/src/plot/commands.rs) | `/rp inspect` ray-traces a block. `/rp analyze` has admission, candidate-graph, and JSON-export paths. |
| [Compiler inspection](../../../crates/core/src/redpiler/mod.rs) and [Graph backend](../../../crates/core/src/redpiler/backend/graph.rs) | Position-to-node lookup and current component state; inspection currently writes debug logs rather than a player report. |
| [Graph preparation](../../../crates/core/src/redpiler/analysis/graph.rs) | A read-only candidate graph prepared through the compilation pipeline. |
| [Graph JSON export](../../../crates/core/src/redpiler/analysis/export_json.rs) | Typed references and graph tables that can support an additional analysis report. |
| [Boundary catalog](../../../crates/core/src/redpiler/program_graph/instant_boundaries.rs) | Separate input/output witnesses for power, callbacks, geometry, guards, comparators, observers, and captured wire reads. |
| [Region partitioning](../../../crates/core/src/redpiler/analysis/instant/regions.rs) | Cell/corridor ownership and connected regions. |
| [Instant region descriptions](../../../crates/core/src/redpiler/program_graph/instant_regions.rs) | Actors, wires, resources, resets, incoming/outgoing occurrences, outside readers, and observer dependencies. |
| [Layer descriptions](../../../crates/core/src/redpiler/program_graph/instant_layers.rs) | Causal layers and request/acceptance predicates where compilation succeeded. |
| [Viewer overlays](../../../crates/core/src/plot/preview.rs) | Client-only block-display markers, bounded incremental reconciliation, color changes, and cleanup helpers. |
| [Existing highlight usage](../../../crates/core/src/plot/git/visuals.rs) | An example of per-player marker ownership and chunk-unload handling; Redpiler should own its session independently of Git. |
| [Asynchronous compilation](../../../crates/core/src/plot/redpiler_compilation.rs) | Snapshot, cancellation, and version checks to follow for expensive analysis. |

Source and runnable tests remain authoritative. Existing optimization metadata
is optional: its absence at O0 must be displayed as "not analyzed", rather than
as an empty network or a failed proof.

## Define the inspected object

Do not use one connected-component calculation for every meaning of "net".

1. **Signal network:** wires and conducting paths between component ports.
   Preserve direction, attenuation, channel, and conditional connectivity;
   stop at component interfaces rather than joining through every device.
2. **Instant region:** the compiler's grouping of local computation cells and
   corridors. It can contain multiple signal networks and outside interfaces.
3. **Resource group:** the pistons and contacts that can share a movable payload.
   Shared ownership does not imply one electrical network.
4. **Memory interface:** storage, data sampling, update activation, and readout
   endpoints. Keep these roles separate.

Report the selected object type. A callback edge alone must not merge independent
signal networks. Conditional networks should distinguish "active now" from
"possible under another contact selection". Active connectivity describes a
snapshot, not all future executions.

## Proposed commands

These are proposed interfaces, not commands implemented by this change.

| Command | Result |
| --- | --- |
| `/rp inspect` | Inspect the looked-at block: component, strength/state, owner IDs, and available proof evidence. |
| `/rp inspect --net` | Summarize and highlight its signal network; provide a separate region/resource selection when the block belongs to those objects. |
| `/rp inspect --inputs` / `--outputs` | List and highlight typed boundary endpoints with coordinates and witness references. |
| `/rp inspect --why` | Explain recognition, assumptions, missing proof obligations, and concrete decline witnesses. |
| `/rp analyze` | Summarize the selection and group warnings by code and affected network/region. |
| `/rp analyze --graph` | Include prepared-graph ownership, coverage, and dependency information. |
| `/rp analyze --export-json` | Extend the existing export workflow with a versioned analysis report; avoid creating another graph format. |

Keep the first version small: player-readable inspection, a network summary,
proof explanations, and bounded highlighting. A separate `-w` compilation flag
can be considered later; analysis should already show its warnings without it.

Player output should show a short summary followed by a bounded list of affected
positions. Large reports belong in JSON, with explicit truncation/counts in chat.
Reuse existing coordinate actions to inspect or highlight a witness.

## Information that is straightforward to expose

| Information | Origin and qualification |
| --- | --- |
| Block identity, node ID, position, current state | Program and state tables; display strengths and pose without copying the whole component entity. |
| Main and QC power dependencies | Typed incoming power links; retain source port, attenuation, and guard. |
| Qualifying Update/Shape inputs | Original route occurrences; preserve kind, direction, ordinal, and live/captured view. |
| Outputs and outside readers | Boundary ports and region readers; distinguish signal outputs from notifications and geometry observations. |
| Reset dependencies | Recognized reset bindings and scheduled obligations; state the scope of recognition. |
| Payload ownership and possible positions | Resource/contact tables; show sharing and conditional alternatives. |
| Compilation coverage | Which actors/wires are covered by each validated optimization contract, and which still use general Graph execution. |
| Decline reasons | Existing structured reasons where present; otherwise report the precise unavailable evidence without inventing a cause. |

Static coverage counts are not runtime time percentages. A general Graph actor
can be a real instant mechanism that the optimizer has not proved. Do not label
that actor "noninstant" merely because it remains on the general executor.

## Evidence is not one Boolean flag

The current source contains several levels of evidence with different scopes:

| Existing evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Reset recognizer / region ownership | A supported reset dependency or ownership closure. | Whole-cycle instant behavior or a combinational truth table. |
| `CellContract` / `BoundaryCatalog.cells` | Local equations and effects for a stationary pull-computation wave. | A complete lifecycle certificate; the source explicitly excludes that claim. |
| `InstantKernel::Events` | An optimized validity calculation for a popped request. | Independent proof that the actor is instant for every legal history. |
| `InstantKernel::PullWave` | An executable ordered wave under its entry predicates. | Complete reset, idle, or arbitrary-history timing equivalence. |
| `InstantProxy` / `PrivateInstantCell` | The narrower route and private-occupancy optimization contract. | Universal instant behavior merely from the proxy's presence. |
| `LogicalBudOwnerPlan` | An ordinary exclusive payload-owner binding. | Functional BUD-switch behavior: the recognizer also admits Main inputs. |
| `BudMemoryRead` | An exact private storage/read QC relationship. | Independent data-versus-update behavior of the entire memory mechanism. |

The UI should expose these evidence descriptions now. A future semantic
classification must use an explicit instant/BUD definition from
[PISTON_MODEL.md](../../../docs/PISTON_MODEL.md), the accepted input protocol,
and a validator for the additional obligations. Do not promote a suggestive
type name into proof.

For BUD classification, check the supported data and notification dependencies:
which data changes can remain unsampled, which separate updates cause sampling,
and which persistent state implements memory. A QC connection by itself is not
enough. For instant classification, state the supported activation, computation,
reset, and reactivation conditions and account for shared-resource interactions.

## Proposed `--assert-instant` contract

- Apply to every piston in the compiled selection, independently of O0–O6.
- Succeed only when every piston has the required validated instant or BUD
  certificate under explicitly declared assumptions.
- Do not silently enable `--stable-instant-inputs` or stronger input protocols.
- Enforce the gate in shared preparation finalization, before lowering and
  activation. Custom or disabled pass pipelines must not bypass it.
- Reuse valid certificates; derive missing evidence through bounded, read-only
  analysis when requested. Do not force full O6 synthesis just to inspect a block.
- Report each unproven actor's ID, position, evidence, and missing obligation.
  Large failures may use a short chat summary with a complete saved report.
- An empty piston set succeeds. Cancellation or analysis-budget exhaustion is
  an incomplete analysis error, not a classification result.
- Do not exempt ordinary update-generating pistons silently. If they have no
  accepted instant/BUD proof, strict assertion rejects them too.

The exact certificate definition and validators are a prerequisite. Current
optimizer plan presence alone cannot implement this contract soundly.

## Nanotick analysis and warnings

[PISTON_MODEL.md](../../../docs/PISTON_MODEL.md) describes ordered causal work;
it does not define a fixed elapsed-time duration for each callback or wire hop.

The first useful analysis can show existing dependency witnesses and warn about:

- multiple activation paths reaching one actor;
- shared-resource arbitration and inhibition dependencies;
- QC data arriving through a different path from update delivery;
- observers or captured reads exposing intermediate geometry;
- feedback crossing memory or scheduled-component interfaces;
- stable-input assumptions and missing arrival-order evidence.

These are potential ordering sensitivities, not automatically bugs or proofs
that two executions differ. Preserve the witness and confidence of each warning.

Where a causal-layer compilation has succeeded, display its derived layer depth,
dependencies, and entry conditions. Do not infer a physical pulse width, repeater
behavior, or game-tick delay from that depth. Cycles, conditional paths, and
different activations can prevent one fixed depth from describing the network.
Exact histories would require an opt-in execution trace and are a later stage.

## Storage, performance, and display rules

- Keep one report associated with a graph/snapshot generation; IDs are local to
  that generation and must not be reused after edits or recompilation.
- Record bounds, source identity, options, assumptions, analysis completeness,
  and report schema version alongside exported IDs and positions.
- Keep signal, callback, geometry, and memory endpoints separate in saved data.
  Multiple raw witnesses may describe one player-facing port: preserve them
  without counting each occurrence as a separate data input.
- Use existing cold provenance to inspect optimized-away positions. If new
  metadata is needed, retain a compact index rather than the full compiler draft.
- An absent analysis table means "not computed"; an empty completed table means
  "computed and empty". A truncated or budget-limited report must say so.
- Expensive analysis should run on a snapshot, support cancellation, and mark
  results stale after relevant world changes. Do not recreate the synchronous
  compilation stalls this project already addressed.
- Inspection must not activate a backend, consume pending ticks, advance the
  simulation, or rewrite block state.
- Highlights are per-player overlays. Bound marker count, packet work, and
  lifetime; clean up on replacement, disconnect, chunk unload, and stale results.
  Include a legend and show when only part of a large network is highlighted.

## Implementation sequence and review gates

1. **Read-only reports:** expose existing state, ownership, boundaries, and
   evidence to chat/JSON. Test a no-piston graph, ADDER1BIT ports, shared payloads,
   and a BUD read/storage pair. Label missing proofs honestly.
2. **Network inspection and overlays:** add on-demand indexes, explicit network
   selection, input/output views, and bounded overlays. Test nearby independent
   networks, conditional contacts, optimized-away positions, and cleanup.
3. **Semantic certificates and assertion:** define and validate the required
   instant/BUD contracts, then add the flag to shared finalization. Test all
   supported levels and custom pipelines, ordinary/noninstant counterexamples,
   malformed evidence, cancellation, budgets, and input-state preservation.
4. **Ordering diagnostics:** surface existing causal witnesses first; add deeper
   analysis only for a concrete warning with a testable meaning. Test known
   ordering counterexamples and avoid reporting an unproven hazard as a failure.

Use ADDER1BIT/ADDER11BIT, Divider, COUNTER, PC COUNTER, smaller BUD fixtures,
and FPU for coverage. The user reports at least one noninstant part of FPU;
pin a concrete witness/history before using that claim as a negative oracle.
Fixtures should check real outputs, memory, and pending work where relevant.
Do not use the inspected classifier as its own correctness oracle.

## Status of the separate compiler-resource task

Implementation sequence and measurement gates are now scoped in the
[compiler-resource plan](REDPILER_COMPILATION_RESOURCE_PLAN.md).

The requested compiler time/memory/stack work remains open. Luna 6 agents
completed read-only investigation; no compiler rewrite was made in this batch.

- The 32 MiB `RUST_MIN_STACK` requirement was found in a historical FPU diagnostic
  report. No active compiler stack-size override was found in the current source.
  A current default-stack failure has not been reproduced.
- `Equations::expand` still recursively traverses raw Boolean expressions and
  clones child arrays. The proposed replacement is an explicit ordered postorder
  traversal, preserving memoization, work accounting, cancellation, and input
  interning order. Source-triggered reentry requires dedicated tests.
- Normal deferred source-equation lowering is already iterative. Some inline
  repeater and signal/guard traversal remains recursive with depth bounds.
- The old repeated layer-reconstruction profile is stale for the normal current
  pipeline: canonical preparation and retained lowering context already avoid
  that repeated work. Do not remove validation to address an obsolete cost.
- Debug-string sorting in `logical/equations.rs` is a smaller candidate for
  cached-key sorting, preserving the current deterministic order while reducing
  repeated formatting. It has not been changed or benchmarked.
- The release test executable built successfully with
  `cargo test -p mchprs_core --lib --release --locked --no-run` in the preceding
  investigation. This checks compilation; no tests were executed by that command.

Next for that task: reproduce a small-stack/deep-expression case, implement and
test iterative expansion, then compare compiler wall time and measured process
memory on pinned real-device inputs. There is no new speedup or memory-reduction
measurement to report yet.
