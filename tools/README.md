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
