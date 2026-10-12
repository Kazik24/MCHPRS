# Redpiler Graph Viewer

Open `index.html` in a modern browser, then choose or drop a `mchprs.redpiler.graph` JSON export. The viewer is local and offline; it does not upload files or contact a server.

Small circuits open as a **Circuit overview**. The 1-bit adder displays all 107 circuit nodes and 205 power links, plus the 45 source/conductor sites used by those links. Larger exports start at a bounded neighborhood. Each view contains at most 400 records and 800 connections; a footer warning reports truncation. All records remain searchable.

- **Inputs** jumps to levers, buttons, and pressure plates (up to 20 shortcuts; search finds the rest).
- Click a component or edge to highlight its immediate connections while preserving the layout. The right inspector lists incoming/outgoing connections, clickable references in both directions, and exact record JSON. Selecting a shared resource highlights its contact nodes together.
- Search an ID, component kind, or record contents and click a result to center it. Exact ID matches come first. Following a hidden connection enables its connection filter; following a node reveals its component kind and clears a Y filter if needed.
- **Selected neighborhood** shows 1–3 hops around the selection; double-click a component to open that view. **Circuit overview** returns to the whole circuit within the size limits.
- **Connections** arranges components by their links; **Signal flow** arranges directed layers. **World X/Z by Y layer** preserves X/Z positions in separate bands for each Y, with small offsets for records sharing one block. Choose **Y** to inspect one world layer. These positions describe the export, not a simulated circuit.
- Drag nodes to arrange them, drag the background to pan, and scroll to zoom. **Center selection** zooms into its neighborhood; **Fit view** returns to the overview; **Relayout** recomputes the chosen layout. Clicking the background or pressing Escape clears highlighting.
- Power links appear by default. Enable **wire relation** for wire geometry dependencies or **route delivery** when exported. Labels appear on highlighted edges; **Edge labels** shows all labels. Component filter colors provide the legend.
- Choose an exported **Snapshot** to highlight supported state guards: green enabled, faded disabled, dashed unknown. **Hide disabled connections** preserves unknown connections. The inspector includes the selected component/piston/resource state. Piston/resource state tests and Boolean compositions are supported; site queries and captured operands stay unknown. This preview does not evaluate signal propagation or execute the circuit.

Exports are checked for the supported major schema version, unique record IDs, and dangling typed references before display. Files larger than 64 MiB are rejected. Larger graphs load without laying out every record. Routes and traces that are `null` remain visibly unavailable; this viewer does not execute circuits or replay traces.

The browser library is vendored under `vendor/` so the viewer works without a build step or network access:

- Cytoscape.js 3.34.3 (MIT), `vendor/CYTOSCAPE-LICENSE.txt`

Example export: `../../docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json`.

Run the standalone regression checks with `node --test tools/redpiler-graph-viewer/viewer.test.cjs` from the repository root.

To inspect the adder: load the example export, click a lever under **Inputs**, and follow an outgoing link in the right panel. Search `node:49` to inspect a repeater fed by `node:50`; follow `power_link:0`, then its guard reference. Select `snapshot:initial` to see guard status. Search `resource:1` to highlight its three contacts, then follow a contact reference to inspect its alternatives. Try **World X/Z by Y layer** with **Y = 33**, then **Fit view**. Routes and trace remain explicitly unavailable for this electrical draft.
