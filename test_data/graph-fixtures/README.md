# Redpiler graph fixtures

Each schematic has two adjacent exports: `<name>.graph.json` (O0) and `<name>-o1.graph.json` (O1). Regenerate them with the ignored test `redpiler::analysis::tests::export_json::write_graph_fixture_json_variants`.

The `adder_non_instant_pistons_4bit` export world omits its wall signs because some are attached to piston bases and Redpiler rejects those supports as potentially moving. The source schematic remains unchanged; those two graph exports therefore describe the sign-free circuit copy.
