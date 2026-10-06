# Generated test artifacts

Schematic inspections, detailed instant-piston traces, projection JSONs and the
trace index are local analysis outputs excluded from Git. Fixture schematics,
protocol manifests, capture provenance and frozen CPU/Java regression baselines
remain versioned. The files in `mc_data` are required build inputs.

Regenerate schematic inspections from the checked-in `.schem` files:

```powershell
py tools/inspect_instant_pistons.py --write
```

With captures available in `test_data/instant-pistons/traces`, regenerate
`java-projections.json`, `mchprs-projections.json` and `trace-index.json`:

```powershell
py tools/analyze_instant_pistons.py --write
```

To produce captures, use new output directories. The Java capture requires the
official 1.21.5 server JAR verified by the capture tool:

```powershell
py tools/capture_instant_pistons.py --output-dir target/instant-mchprs
py tools/analyze_instant_pistons.py --ingest target/instant-mchprs
py tools/capture_instant_pistons_java.py --server-jar <server.jar> --output-dir target/instant-java
py tools/analyze_instant_pistons.py --ingest target/instant-java
py tools/analyze_instant_pistons.py --write
```

Ordinary Rust tests use the versioned fixture protocols and schematics. The
comparison against locally generated Java projections is an explicit opt-in:

```powershell
cargo test -p mchprs_core --lib redstone::instant_piston_tests::frozen_java_port_waveforms_match_current_binaries_and_protocols --locked -- --ignored --exact
```

The separate lever/repeater/BUD revision is characterized in
[INSTANT_PISTON_IO_SCHEMATICS.md](../docs/INSTANT_PISTON_IO_SCHEMATICS.md).
Its frozen fixtures live in `test_data/instant-pistons-io`; defaults above still
refer to the older pack. To reproduce the new pack, use fresh capture directories:

```powershell
py tools/inspect_instant_pistons.py --pack-dir test_data/instant-pistons-io --write
py tools/instant_piston_io_protocols.py --check
py tools/capture_instant_pistons.py --pack-dir test_data/instant-pistons-io --output-dir target/instant-io-mchprs-new
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-mchprs-new
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --server-jar <server.jar> --output-dir target/instant-io-java-new
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-java-new
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --write
py tools/document_instant_io.py --check
py tools/validate_instant_io.py
cargo test -p mchprs_core --lib redstone::instant_piston_tests::io:: --locked -- --test-threads=1
```

Java lever capture uses an isolated offline player and actual vanilla UseItemOn
packets. Install `minecraft-protocol@1.62.0` under the ignored `tools/node_modules`
directory and provide Node with `--node` if its default path differs. The helper
acknowledges teleport and client loading; accepted lever states are checked with
RCON while ticking remains frozen. Captures record the helper hash and packet
sequence. No chat messages or modified server plugins are used. The pinned JAR
hash is verified before launch.

The trace index retains two XOR projection differences at different origins.
The versioned `diagnostics` records exact-origin captures separately: one agrees
and one still differs. The validator checks that classification without treating
the remaining discrepancy as a passing conformance result.
The [validation report](../docs/INSTANT_PISTON_IO_VALIDATION.md) lists the commands
executed, isolated-checkout testing and remaining unverified protocols.
