# Redpiler Graph Viewer

Open `index.html` in a modern browser, then choose or drop a `mchprs.redpiler.graph` JSON export. The viewer is local and offline; it does not upload files or contact a server.

Small circuits open as a **Circuit overview**. The 1-bit adder displays all 107 circuit nodes and 205 power links, plus the 45 source/conductor sites used by those links. Larger exports start at a bounded neighborhood. Each view contains at most 400 records and 800 connections; a footer warning reports truncation. All records remain searchable.

Exports with `graph.instant_programs` open in **Compiled plans** by default. Choose **Circuit (physical)** to inspect the original nodes and links. In the compiled view, mapped transport wires collapse into one net record; uncovered wires and other physical state owners remain visible. Typed drive inputs, predicates, possible outputs, CPU piston predicates, and the per-net plans are inspectable. Select a compiled net and expand its operands to see the full typed tables and ordered transcript operations. Expansion is lazy because large circuits can export many instructions.

Edges marked **Original graph witness** preserve physical connectivity through a collapsed net. They are mapping evidence, not claims that the compiled plan has the same dependency or event order. Typed input/predicate/output edges come from the exported program IR. A transition marked eligible only has a static eligibility certificate: runtime-learned transcripts are not included in the export. The viewer does not claim that the original graph has been wholly removed or execute any compiled program.

- **Inputs** jumps to levers, buttons, and pressure plates (up to 20 shortcuts; search finds the rest).
- Click a component or edge to highlight its immediate connections while preserving the layout. The right inspector lists incoming/outgoing connections, clickable references in both directions, and exact record JSON. Large “Referenced by” lists page in batches of 100, so every result remains reachable without building a graph-wide reverse-reference index. Selecting a shared resource highlights its contact nodes together.
- Search an ID, component kind, or record contents and click a result to center it. Exact ID matches come first. Following a hidden connection enables its connection filter; following a node reveals its component kind and clears a Y filter if needed.
- **Selected neighborhood** shows 1–3 hops around the selection; double-click a component to open that view. **Circuit overview** returns to the whole circuit within the size limits.
- **Connections** arranges components by their links; **Signal flow** arranges directed layers. **World X/Z by Y layer** preserves X/Z positions in separate bands for each Y, with small offsets for records sharing one block. Choose **Y** to inspect one world layer. These positions describe the export, not a simulated circuit.
- Drag nodes to arrange them, drag the background to pan, and scroll to zoom. **Center selection** zooms into its neighborhood; **Fit view** returns to the overview; **Relayout** recomputes the chosen layout. Clicking the background or pressing Escape clears highlighting.
- Power links appear by default to keep large callback graphs within the overview limit. Enable **callback link** to see update and shape arrows from the owning component (including pistons) to the receiving component; searching a `callback_link:*` record reveals its edge directly. Enable **wire relation** for wire geometry dependencies or **route delivery** for deliveries without a compiled receiver. Route records remain searchable. Labels appear on highlighted edges; **Edge labels** shows all labels. Component filter colors provide the legend.
- Choose an exported **Snapshot** to highlight supported state guards: green enabled, faded disabled, dashed unknown. **Hide disabled connections** preserves unknown connections. The inspector includes the selected component/piston/resource state. Piston/resource state tests and Boolean compositions are supported; site queries and captured operands stay unknown. This preview does not evaluate signal propagation or execute the circuit.

Exports are checked for the supported major schema version, unique record IDs, dangling typed references, dust-path ownership, and compiled local ranges/opcodes before display. Older exports without the additive compiled-program tables remain readable in the physical view. Files up to 2 GiB are accepted; files over 32 MiB are parsed incrementally from the browser file stream, without creating one giant source string. The parsed graph still needs browser memory, so available memory can limit very large files. Compiled view avoids retaining a second physical adjacency index until that view is selected. Larger graphs load without laying out every record. Routes and traces that are `null` remain visibly unavailable; this viewer does not execute circuits or replay traces.

The browser library is vendored under `vendor/` so the viewer works without a build step or network access:

- Cytoscape.js 3.34.3 (MIT), `vendor/CYTOSCAPE-LICENSE.txt`

Example export: `../../test_data/graph-fixtures/adder-1bit/adder-1bit.graph.json`.

The complete current FPU example is `../../target/graph-viewer-current/fpu-o5-stable.graph.json`
(619.41 MiB, O5 + stable inputs). It was checked in Edge with 70,458 nodes and
850,792 links: load and compiled overview took about 19.4 seconds, and expanded
stable expressions and operands remained inspectable. Search `compiled_net:25`
for the checked 21-wire stable program. Use **Selected neighborhood**, one hop,
and zoom/pan to read its connections; the compiled input/output/predicate
filters can hide the physical mapping witnesses.

Run the standalone regression checks with `node --test tools/redpiler-graph-viewer/viewer.test.cjs` and `py -3 -m unittest tools.redpiler_graph.test_loader` from the repository root.

Experimental instant-boundary exports add searchable `instant_cell:*`, `instant_corridor:*`,
`instant_port:*` and `instant_decline:*` records. Ports distinguish input/output bindings
and link back to original power, callback, guard and geometry witnesses. Their count includes
connections between candidate owners, not just outside data bits. A catalog describes
potential compilation coverage; it does not mean the circuit executes as an instant kernel.
Fixture review exports are written to `target/layered-instant-boundaries/` by the Rust tests.

To inspect the adder: load the example export, click a lever under **Inputs**, and follow an outgoing link in the right panel. Search `node:49` to inspect a repeater fed by `node:50`; follow `power_link:0`, then its guard reference. Select `snapshot:initial` to see guard status. Search `resource:1` to highlight its three contacts, then follow a contact reference to inspect its alternatives. Try **World X/Z by Y layer** with **Y = 33**, then **Fit view**. Routes and trace remain explicitly unavailable for this electrical draft.
