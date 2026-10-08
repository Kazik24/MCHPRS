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

The logical executor has focused checks for dependency caching, old-bank atomic
sampling, memory hold, ambiguous feedback rejection and settled restoration:

```sh
cargo test -p mchprs_core --lib ideal
cargo test -p mchprs_core --lib instant::logical::tests
cargo bench -p mchprs_core --bench instant -- --iterations 3
```

The benchmark runs compiled Counter evaluations after initialization and warmup.
Add `--optimize` to compare ordinary graph passes or `--flush-every 1` to include
display writes. Compilation is outside the timed window. `--component
cpu_bubblesort` tests logical admission only. `--component fpu_divider` measures
warmed repeated saved-input response/reset episodes and reports episodes per
second, including stimulus handling and publication. Use `--episodes` to choose
the count; this is a bounded saved-input protocol, not an arithmetic proof.
Physical sampled throughput does not measure logical-plan performance.

The same benchmark supports native full-FPU input streams and the revised
Potados PC_COUNTER fixture. Disable random ticks, capture a baseline, then pass
that report to the candidate build with identical workload arguments:

```sh
cargo bench -p mchprs_core --bench instant -- --component fpu_legal --interpreted --workload random --episodes 8 --input-every 128 --iterations 3 --output target/native-fpu-baseline.json
cargo bench -p mchprs_core --bench instant -- --component fpu_legal --interpreted --workload random --episodes 8 --input-every 128 --iterations 3 --reference target/native-fpu-baseline.json --output target/native-fpu-candidate.json
cargo bench -p mchprs_core --bench instant -- --component pc_counter --interpreted --iterations 3 --output target/native-counter-baseline.json
cargo bench -p mchprs_core --bench instant -- --component pc_counter --interpreted --iterations 3 --reference target/native-counter-baseline.json --output target/native-counter-candidate.json
```

Native execution disables random ticks automatically. References verify fixture
and stimulus metadata, every measured output word, and final whole-world state.
Output observations and checkpoint hashing are outside timers. FPU timers include
ordered input-lever updates and interpreted ticks; its output words are raw port
observations, without an arithmetic oracle. PC_COUNTER measures 4,096 ticks after
enabling its 23 controls and one untimed activation tick, and requires decoded
pulse peaks 1 through 682 with final count 682. Use the `cpus` benchmark for PM1,
ANPU and the complete BubbleSort episode and its existing frozen assertions.
Native full-FPU episodes initialize ON for 64 ticks, warm eight vectors, then
change inputs while OFF and hold each OFF/ON phase for `--input-every` ticks.
Both phases contribute to timing and output checks. Compiled full-FPU input
streams retain their continuous-OFF protocol; these are different workloads.

### Native interpreter runtime certificates (2026-10-09)

The native interpreter learns observer-above, single-redstone-block sticky
retract/reset actors from completed physical cycles. It verifies both exact
motion identities in each direction, the restored footprint and observer-driven
reset admission. Only proven actors use the compact-address fast path, including
the six strong-power neighbors of each queried conductor. Electrical states,
notifications, accepted events and motion timing remain native. Mutable entity,
chunk and piston-state access and relevant structural edits revoke certificates.
The address set is reused across later cycles; unsupported actors use the
ordinary interpreter.

The [performance report](../../test_data/cpu-references/interpreter-20261009-performance.json)
records frozen binary/source/fixture hashes, timer scopes, all completed samples,
runtime admission counts and control diagnostics. Initial timings taken during
compiler activity are excluded. The completed runs used logical CPU 2 affinity
and Normal priority, with no own builds during timing:

| Workload | Baseline seconds | Candidate seconds | Proven actors | Power-query cache hits |
| --- | ---: | ---: | ---: | ---: |
| FPU stimulus | 5.7658 | 5.4439 | 1,640 | 2,138,380 |
| PC_COUNTER | 3.6367 | 3.4751 | 139 | 1,300,403 |
| PM1_SORT | 67.3538 | 62.1117 | 1,865 | 7,580,478 |
| ANPU Pong | 5.5633 | 4.9199 | 0 | 0 |
| CPU BubbleSort | 75.0030 | 70.1141 | 3,187 | 13,520,186 |

FPU and PC_COUNTER values are medians of two paired runs; CPU values use one
complete pair, with an additional candidate repeat retained in the report.
Remaining repeats stopped at the user's wrap-up request. These are preliminary
shared-host measurements. ANPU's difference is a noise/compiler control and
cannot be attributed to an instant-cache path it never used. Cache-hit totals
include untimed warmup. CPU timers measure their fixed active windows while all
50,000 replay ticks retain the frozen assertions.

The FPU output stayed at 65,535 throughout this protocol. Both saved controls
were investigated; the green trigger drives substantial native motion, while
the other control is locally overridden by existing sources. FPU timing measures
physical stimulus execution, without confirmed arithmetic output throughput.
The [FPU reference](../../test_data/cpu-references/interpreter-20261009-fpu-reference.json)
and [counter reference](../../test_data/cpu-references/interpreter-20261009-pc-counter-reference.json)
freeze every measured output word and final world state.

Validation: 122 redstone tests, 18 world tests and 268 plot tests passed; five
existing tests remain ignored. Native cycle tests compare complete physical
state and ordered samples/events with a cache-cleared twin, including held-zero
resets, live strong-power changes, container-cap edits, moving-entity mutation
and deletion, interrupted block replacement and multi-block reset exclusion.
For the plot suite, set `MCHPRS_CONFIG` to the root `Config.toml`; an existing
local `crates/core/Config.toml` has a WorldEdit limit below the PM1 fixture size.

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
python tools/summarize_piston_research.py --ingest target/rilax-new.json --id rilax-new
python tools/summarize_piston_research.py --check
```

Ingestion requires a fresh ID and adds a compressed trace plus versioned
provenance/summary files. `--check` checks existing ingested research evidence;
it does not inspect an un-ingested capture in `target/`.

Use the manifest's case IDs with `--case` to select an episode. Other supported
fixtures are `fpu_legal`, `fpu_divider`, and `cpu_bubblesort`. `--probe-logic` is a
read-only FPU expression probe which bypasses admission and requires a fresh
output file. Download manifests and sidecars retain original binaries,
legalization history, and capture identities. Do not rename a derived fixture
into an original reference or alter a source binary to make it pass.

The active `fpu_legal` manifest uses the author's
[fpu_fixed_compilation.schem](../../test_data/piston-research/fpu-fixed/fpu_fixed_compilation.schem)
revision, verified against the server download hash. Its trigger is selection-local
`(2,38,81)` and its loader offset is `(2,38,76)`. The previous full-FPU manifest is
retained as `fpu_legal_legacy` for historical negative admission assertions;
`fpu_divider` remains a separate smaller extraction. The fixed full FPU compiles
with default options, `--optimize`, `--assume-instant`, and both flags together.
The logical certificate admits guarded side reset observers, shared electrical
reset targets and data-coupled combinational rechecks. It continues to reject
independent storage samples and observable reset pulses without their own protocol.

```sh
cargo test -p mchprs_core --lib --locked fixed_fpu_compiles_with_and_without_optimization
```
