# Redpiler graph fixtures

Each schematic has two adjacent exports: `<name>.graph.json` (O0) and `<name>-o1.graph.json` (O1). Regenerate them with the ignored test `redpiler::analysis::tests::export_json::write_graph_fixture_json_variants`.

The `adder_non_instant_pistons_4bit` export world omits its wall signs because some are attached to piston bases and Redpiler rejects those supports as potentially moving. The source schematic remains unchanged; those two graph exports therefore describe the sign-free circuit copy.

The `adder-1bit/` directory contains the pinned ADDER_1BIT exports for O0 through O3. Regenerate them with `cargo test -p mchprs_core --lib write_adder_1bit_graph_json_fixtures -- --ignored`. These exports are used by Rust determinism tests and the graph viewer tests.
