# Redpiler Graph Viewer

Open `index.html` in a modern browser, then choose or drop a `mchprs.redpiler.graph` JSON export. The viewer is local and offline; it does not upload files or contact a server.

The graph displays nodes, query sites, power links, wire relations, and routes when the export contains them. Use the left panel to search record IDs or contents, filter node and connection types, inspect the selected record, and collapse/expand compound groups with Cytoscape's expand-collapse extension. **Relax** reruns the force-directed layout from the current positions. Drag to pan, scroll to zoom, and click records or connections to inspect their exported JSON.

Exports are checked for the supported major schema version, unique record IDs, and dangling typed references before display. Files larger than 64 MiB or graphs with more than 20,000 displayed elements are rejected to keep the browser responsive. Routes and traces that are `null` remain visibly unavailable; this viewer does not execute circuits or replay traces.

The browser libraries are vendored under `vendor/` so the viewer works without a build step or network access:

- Cytoscape.js 3.34.3 (MIT), `vendor/CYTOSCAPE-LICENSE.txt`
- cytoscape-expand-collapse 4.1.1 (MIT), `vendor/EXPAND-COLLAPSE-LICENSE.txt`

Example export: `../../docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json`.
