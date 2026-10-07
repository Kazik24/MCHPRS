# Instant piston fixtures and validation

The physical specification is in [the piston model](../PISTON_MODEL.md).
Compiled admission and execution are in [the parser](../REDPILER_PARSER.md) and
[the compiled model](../REDPILER_MODEL.md). Fixture names identify evidence; the
compiler derives behavior from geometry and block state rather than filenames.

## Packs and provenance

| Pack | Role |
| --- | --- |
| `test_data/instant-pistons` | Original physical gate/reset/nanotick fixtures and versioned protocols. |
| `test_data/instant-pistons-io` | Separate lever-input, repeater-output revision, including independent BUD update inputs. |
| `test_data/piston-repair` | Frozen Java edge-case, adder, oscillator, and observer-feedback traces. |
| `test_data/piston-research` | Larger FPU, divider, memory-bank, and CPU protocols with capture sidecars and summaries. |

For each versioned fixture, read `fixtures/<id>.json`: schematic SHA-256,
dimensions, paste origin/offset, port coordinates, ordered actions, rotations,
readiness checks, observation decoder, and step/time limits belong to that exact
binary. Input and update ports may be independent. Preserve action order and
same-value samples; a truth table alone cannot represent a BUD episode.

Coordinates are selection-local unless a manifest explicitly says otherwise.
`paste_anchor = origin + loader_offset`; rotations and translation change the
absolute cells being observed. Separate pack revisions and Java captures at
different origins are not interchangeable expectations.

Frozen Java data provides an independent comparison for the declared projection.
MCHPRS captures characterize this implementation. Research extraction probes
may bypass executable admission for analysis; their output is not evidence that
a circuit compiles or is correct under an arbitrary input protocol.

## Physical regression tests

```sh
cargo test -p mchprs_core --lib --locked redstone::piston::tests::
cargo test -p mchprs_core --lib --locked redstone::instant_piston_tests::
```

These checks include queued-event revalidation, pulse/drop behavior, movement
identities, head/shape notifications, data-versus-update BUD sampling, held
inputs, reset, and agreement between game/nano/pico stepping at completed tick
boundaries. Read the individual test for the exact observation window.

## Inspections and fresh captures

Inspection decodes schematics without simulation:

```sh
python tools/inspect_instant_pistons.py --write
python tools/inspect_instant_pistons.py --check
python tools/inspect_instant_pistons.py --pack-dir test_data/instant-pistons-io --write
python tools/inspect_instant_pistons.py --pack-dir test_data/instant-pistons-io --check
python tools/instant_piston_io_protocols.py --check
```

Capture tools refuse existing output directories/files. Use fresh destinations.
MCHPRS pack capture also updates that pack's versioned `source-baseline.json`
provenance sidecar; review this change separately from the new trace files.
The following default-pack commands are equivalent for the I/O pack after adding
`--pack-dir test_data/instant-pistons-io` to each capture/analyzer invocation:

```sh
python tools/capture_instant_pistons.py --output-dir target/instant-mchprs-new
python tools/analyze_instant_pistons.py --ingest target/instant-mchprs-new
python tools/capture_instant_pistons_java.py --server-jar <server.jar> --output-dir target/instant-java-new
python tools/analyze_instant_pistons.py --ingest target/instant-java-new
python tools/analyze_instant_pistons.py --write
python tools/analyze_instant_pistons.py --check
python tools/validate_instant_pistons.py
```

Java capture requires the exact server JAR checked by the tool, Java, and RCON.
Lever input uses a real offline protocol client and UseItemOn packets while
ticking is frozen; it verifies the resulting lever state. The helper requires
Node and `minecraft-protocol@1.62.0` under ignored `tools/node_modules`; for
example `npm install --prefix tools --no-save --package-lock=false
minecraft-protocol@1.62.0`. Pass `--node <node-executable>` when the default
Windows path does not apply. The capture records server/helper hashes, origin,
stimuli, and termination limits.

After regenerating the I/O pack's inspection and analysis artifacts, run:

```sh
python tools/validate_instant_io.py
```

Both validators accept `--recapture-dir <fresh-mchprs-directory>` to compare a
new episode to the existing complete trace. Validators need ignored inspections,
traces, and trace indices; they do not generate those prerequisites. Historical
source fingerprints in captures describe those captures, not current sources.

The original pack's frozen-Java projection test is an explicit opt-in:

```sh
cargo test -p mchprs_core --lib --locked redstone::instant_piston_tests::frozen_java_port_waveforms_match_current_binaries_and_protocols -- --ignored --exact
```

The I/O validator deliberately retains two `xor_simple` projection differences.
Its origin-aligned diagnostic evidence contains one match and one mismatch.
Successful validation means hashes, coverage, and this classification agree;
it does not turn the mismatch into conformance.

## Compiled checks

```sh
cargo test -p mchprs_core --lib --locked redpiler::analysis::tests::
```

This covers expression extraction, conditional occupancy/conduction, sequential
sampling, output ports, budget/admission failures, ordinary graph consumers, and
reset/handoff. Optimization and I/O-only flags change the observed representation
and must not silently waive boundary behavior.

One ignored test records an unresolved complete-reset waveform discrepancy:

```sh
cargo test -p mchprs_core --lib --locked redpiler::analysis::tests::outputs::xor_complete_reset_waveform_matches_interpreted_consumers -- --ignored --exact
```

The documented mismatch is at the reset waveform around tick 8. This is a
diagnostic check with a known failure, not a required passing baseline. Consult
the current test body if its status changes.

## Larger research captures

```sh
python tools/capture_piston_research.py --fixture rilax_memory_bank_bud --output target/rilax-new.json
python tools/summarize_piston_research.py --check
```

Use the manifest's case IDs with `--case` to select an episode. Other supported
fixtures are `fpu_legal`, `fpu_divider`, and `cpu_bubblesort`. `--probe-logic` is a
read-only FPU expression probe which bypasses admission and requires a fresh
output file. Download manifests and sidecars retain original binaries,
legalization history, and capture identities. Do not rename a derived fixture
into an original reference or alter a source binary to make it pass.
