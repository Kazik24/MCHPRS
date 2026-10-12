# Redpiler graph fixtures

`adder-1bit.graph.json` exports the hash-checked `ADDER_1BIT.schem` fixture with the v1.1 inspection schema. It is a read-only `callback_draft`: the artifact includes electrical and geometry analysis, prepared callback routes, direct callback links, piston route bindings, and initial logical state. Execution trace remains unavailable (`null`).

The artifact contains 107 nodes, 205 power links, 660 sites, 3,760 guards, 36 contacts, 13 pistons, 10 shared resources, 32 wire plans, 384 wire relations, 184 callback routes, and 1,683 direct callback links. It is 2,840,962 bytes. The selected bounds are the full fixture plot, `(0, 0, 0)` through `(255, 255, 255)`.

Regenerate the artifact with:

```powershell
cargo test -p mchprs_core --release --locked redpiler::analysis::tests::export_json::write_adder_1bit_graph_json_fixture -- --ignored --exact --test-threads=1
```

For a representative power path, `site:70` is the view of wire `node:50` at `(51, 31, 41)`. `power_link:0` reads that site's stored power and connects it, with zero attenuation on the south face under its exported guard, to `node:49`'s main input; `node:49` is a repeater at `(51, 31, 40)` with delay 2. Callback links are listed in `graph.callback_links`, with their route and sequence references. Each ID resolves in the artifact's corresponding graph table.

Inspect the export with the standard-library Python CLI from the repository root:

```powershell
py -m tools.redpiler_graph docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json summary
py -m tools.redpiler_graph docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json show power_link:0
py -m tools.redpiler_graph docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json refs node:49
```

`refs` lists each typed reference path into and out of a record. The loader rejects unsupported schema versions, duplicate IDs, and dangling references before any command displays data.

For interactive graph inspection, open [`tools/redpiler-graph-viewer`](../../../../tools/redpiler-graph-viewer/) and load this JSON file. The viewer filters node/connection types, searches records, inspects selected records, and collapses graph groups; it does not execute the circuit.
