# Side A: Graph export and offline investigation

Status: scoped for review, 2026-10-12. Implement one slice at a time; stop for review after each.

## Agent assignment

Read [AGENT_GUIDE](../AGENT_GUIDE.md), [ARCHITECTURE](../ARCHITECTURE.md), and [the format contract](../GRAPH_JSON.md). Preserve concurrent Step 05 work. Implement only the assigned slice. Use small named functions, explain every new field and enum variant, and provide meaningful registered tests. Do not introduce physical motion or a second redstone interpreter.

The export is an inspection artifact for the richer graph. Existing legacy binary/DOT exports must retain their behavior. The viewer must accurately distinguish an electrical draft, a complete program, a state snapshot, and a recorded execution trace.

## Read the connected sources

- `crates/core/src/redpiler/program_graph.rs`: typed IDs, guards, contacts, routes, immutable program, logical state.
- `crates/core/src/redpiler/analysis/geometry.rs` and `electrical.rs`: current drafts and conductor/query plans.
- `crates/core/src/redpiler/analysis/graph.rs`: existing read-only preparation boundary.
- `crates/core/src/redpiler/passes/export_graph.rs` and `crates/redpiler_graph/src/lib.rs`: legacy export; do not serialize the new graph through its smaller vocabulary.
- `crates/core/src/plot/commands.rs` and `plot/mod.rs`: command dispatch and transactional analysis.
- `crates/core/src/redstone/instant_piston_tests/reference_contract.rs`: observable trace contract.
- The Step 05 implementation when reviewed: route assembly determines what a complete program can export.

## A1: Versioned JSON export

Intermediate goal: export the current read-only electrical draft with enough provenance to investigate every link and guard externally.

1. Implement explicit export DTOs in a dedicated module, following GRAPH_JSON.md. Adapt actual graph types rather than serializing private Rust layouts. Reuse existing serde dependencies.
2. Export geometry, electrical channels, comparator query plans, wire relations, and initial logical state. Keep a missing callback section distinct from an empty complete section. A draft is never labeled executable.
3. Add `/rp analyze --export-json` to the existing analysis command. It runs reviewed read-only preparation, reports structured failures, and writes an artifact without activating or resetting compilation. Check current argument handling first; preserve existing commands and flags. Do not overload the legacy `--export` flag.
4. Use a server-owned export directory and a generated plot/revision filename. Report path and graph counts in chat. No arbitrary path supplied by players; reuse existing command permissions and host file-writing conventions. Write only after successful preparation/validation; use a temporary file and rename, avoiding partially written artifacts and overwrites.
5. The file can be retrieved from the server filesystem. Do not add an HTTP server or download service in this slice.
6. Once Step 05 provides a complete program, add its routes and assembled references through the same adapter. Do not reconstruct or approximate missing routes for export. Active-state snapshots and scheduler records wait until those runtime types exist.

Acceptance: a synthetic guarded shared resource exports with every ID resolving; duplicate directional callbacks remain ordered when available; normal/QC and both conductor dust policies remain distinct; fixed and live comparator overrides remain distinct; repeated export of the same input has the same semantic content; failed preparation creates no finished file and preserves world/ticks. Export a small fixture and explain one power path in the JSON.

Review gate: show the JSON artifact, schema version/stage, counts, and the exact command used. No browser work until A1 is reviewed.

## A2: Offline HTML viewer

Intermediate goal: open one static HTML page, load an exported JSON file, and investigate graph behavior and recorded traces.

Suggested home: `tools/redpiler-graph-viewer/`, subject to existing repository conventions. Use plain HTML/CSS/JavaScript and native SVG or canvas; no build system, remote scripts, runtime downloads, or frontend framework is required. Keep parsing, indexing, rendering, and selection separate named functions. Render imported strings as text, never HTML.

Required views:

- Summary: schema/stage/capabilities, counts, preparation diagnostics, missing sections.
- Searchable tables for nodes, sites, contacts, resources, guards, and routes. Selection follows references in both directions.
- Local graph neighborhood with typed/color-keyed connections, arrows, pan/zoom, node/edge filtering, and a legend. Default to the selected neighborhood; do not lay out the entire FPU on load.
- Position view using an X/Z projection and selectable Y layer; positions aid understanding but do not drive simulation.
- Inspector: source positions, exact ports/faces, attenuation, guard expression, selected contact alternatives, and incoming/outgoing dependencies. Route view displays ordinal, kind, direction, operand view, and duplicates.
- Logical-state preview: select an exported snapshot and evaluate supported state guards to highlight enabled connections. Captured/site operands without their required context are shown as unknown, never guessed. Manual pose previews are explicitly hypothetical and cannot certify reachability.
- Trace replay: previous/next boundary, play/pause, tick/phase filter, ordered callbacks, acceptance/cancellation, resource selection, and pending deadlines. Replay changes the displayed recorded snapshot; it does not execute the circuit.

Loading must validate schema versions, unique typed IDs, references, section availability, and configurable practical size limits before rendering. Show a readable error instead of freezing on malformed input. Use explicit user file selection; do not upload files or fetch remote graph URLs. A selected neighborhood avoids a dependency on a large graph layout engine.

Acceptance: load the A1 fixture; follow a QC input and shared resource; preserve callback duplicates in route display; reject unknown major version and dangling IDs; show missing execution as unavailable; load a representative larger fixture without rendering every edge. Supply a tiny standalone smoke-check script using repository-available tooling for parsing/indexing/guard evaluation, plus a manual browser checklist.

Review gate: provide the HTML artifact and a sample export, show screenshots or a reproducible browser walkthrough, and document how to open it locally.

## Running circuits later

True Run/Step requires the reviewed graph executor (Steps 06-09), a complete scheduler/state export, and differential tests. Scope it as a later A3: reuse the Rust executor through an appropriate browser or local process boundary after investigating the actual executor interface. Do not promise that JSON alone is runnable, introduce a JavaScript piston executor, use native interpreter fallback, or add progress/motion objects. Runtime integration is not authorized by A1/A2.

## Handoff

Report changed files, exact checks/counts, one readable exported witness, and unsupported stages. Do not modify another agent's parser or callback assembly while implementing the adapter; communicate any required interface change first.
