# Redpiler Graph Viewer

Open `index.html` in a modern browser, then choose or drop a `mchprs.redpiler.graph` JSON export. The viewer is local and offline; it does not upload files or contact a server.

The graph initially displays the first node and its neighboring power links, wire relations, and routes when available. Search all record IDs or contents and select a result to center its neighborhood. Each view contains at most 200 nodes and 400 connections; records outside that view remain searchable. The left panel filters node and connection types, inspects records, and collapses/expands compound groups with Cytoscape's expand-collapse extension. **Relax** reruns the layout for the current view. Drag to pan, scroll to zoom, and click records or connections to inspect their exported JSON.

Exports are checked for the supported major schema version, unique record IDs, and dangling typed references before display. Files larger than 64 MiB are rejected. Larger graphs load without laying out every record. Routes and traces that are `null` remain visibly unavailable; this viewer does not execute circuits or replay traces.

The browser libraries are vendored under `vendor/` so the viewer works without a build step or network access:

- Cytoscape.js 3.34.3 (MIT), `vendor/CYTOSCAPE-LICENSE.txt`
- cytoscape-expand-collapse 4.1.1 (MIT), `vendor/EXPAND-COLLAPSE-LICENSE.txt`

Example export: `../../docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json`.

Run the standalone regression checks with `node --test tools/redpiler-graph-viewer/viewer.test.cjs` from the repository root.
