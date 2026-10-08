# Test suites

Run commands from the repository root. Python commands below use `python`; on
Windows, `py` is equivalent when the Python launcher is installed. Test filters
are Rust module/name substrings unless `--exact` is specified.

## Ordinary checks

```sh
python tools/validate_docs.py
cargo fmt --all -- --check
cargo test --workspace --locked --no-fail-fast
```

The ordinary suite excludes tests marked `#[ignore]`. List those tests before
selecting an opt-in run; some ignored tests **capture new baselines** and others
record unresolved discrepancies. Do not run every ignored test as a conformance
suite.

```sh
cargo test -p mchprs_core --lib --locked -- --list --ignored
```

| Contract | Sources | Focused command |
| --- | --- | --- |
| Power, dust, diodes, observers, movement | [Redstone tests](../../crates/core/src/redstone/mod.rs), [piston tests](../../crates/core/src/redstone/piston/tests.rs), [wire tests](../../crates/core/src/redstone/wire/turbo_tests.rs) | `cargo test -p mchprs_core --lib --locked redstone::` |
| Copper bulbs: Java 1.21.5 electrical traces, transformations, storage, lighting | [Bulb tests](../../crates/core/src/redstone/copper_bulb/tests.rs), [Java reference](../../test_data/copper-bulbs/java-1.21.5.json) | `cargo test -p mchprs_core --lib --locked copper_bulb` |
| Physical instant protocols | [Instant tests](../../crates/core/src/redstone/instant_piston_tests.rs), [I/O tests](../../crates/core/src/redstone/instant_piston_tests/io.rs) | `cargo test -p mchprs_core --lib --locked redstone::instant_piston_tests::` |
| Recognition, admission, compiled execution, reset | [Analysis tests](../../crates/core/src/redpiler/analysis/tests.rs) and its submodules | `cargo test -p mchprs_core --lib --locked redpiler::` |
| Scheduler, storage, cache lifetime | [World tests](../../crates/core/src/world/tests.rs) | `cargo test -p mchprs_core --lib --locked world::` |
| Plot operations and synchronization | [Plot tests](../../crates/core/src/plot/mod.rs), [player tests](../../crates/core/src/player/client_sync_tests.rs) | `cargo test -p mchprs_core --lib --locked plot::` and `cargo test -p mchprs_core --lib --locked player::` |
| Protocol bytes and ordered sending | [Network tests](../../crates/network/src/outbound/tests.rs), [packet tests](../../crates/network/src/packets/tests.rs) | `cargo test -p mchprs_network --locked` |
| Registry generation | [Macro tests](../../crates/proc_macros/tests/minecraft_data.rs), [input reference](../../mc_data/README.md) | `cargo test -p mchprs_proc_macros --locked` |
| Saved data and migration | [Save tests](../../crates/save_data/src/plot_data/tests.rs) | `cargo test -p mchprs_save_data --locked` |

CI also regenerates the registry binaries and checks that their bytes match:

```sh
python tools/generate_mc_data.py
git diff --exit-code -- mc_data/1.21.5/registries.bin mc_data/1.21.5/tags.bin
```

The protocol CI job currently names `tools/run_protocol_smoke.py`,
`tools/run_plot_load_smoke.py`, and an npm package in `tools/` which are absent
from this checkout. Treat that job as incomplete configuration; the Rust network
tests above are runnable. Do not invent results for the missing scripts.

## Evidence and larger runs

The copper bulb reference was captured from the official Java 1.21.5 server
with SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`. It covers all eight
variants and all four initial `lit`/`powered` combinations, repeated levels,
zero-tick edges, direct/far comparators, and an observer. The interpreter and
compiled engine (including `-O` and `-Oi`) replay the same samples. Oxidation,
waxing, scraping, and emitted light have separate controlled tests.

To capture independently into a new file (Java 21 or newer, localhost ports
25583/25584 must be free):

```sh
python tools/capture_copper_bulbs_java.py --server-jar /path/to/server-1.21.5.jar --output /path/to/new-reference.json
```

The script uses a temporary void world and refuses to overwrite a reference.
Piston-carried bulbs work in the interpreter; compiled piston extraction rejects
them because its payload model cannot carry the bulb latch and analog output.
The legacy graph export also rejects bulbs instead of writing an incompatible
node. Bulb block light is rebuilt at visual flushes using registry attenuation;
partial-block face occlusion and cross-plot light propagation are outside this
lighting implementation.

- [Instant piston fixtures and captures](INSTANT_PISTONS.md): physical episodes,
  Java comparisons, coordinate mappings, and compiled differential checks.
- [CPU references](CPU_REFERENCES.md): complete interpreter replays, screen/RAM
  projections, compiled sampled protocols, and benchmarks.
- [FPU divider](FPU_DIVIDER.md): shared trigger and mantissa inputs, negative
  output pulses, missing-head warnings, compiled admission and reset handoff.

The schematics, fixture JSON, capture sidecars, and frozen expectations remain in
`test_data/`; this folder holds their documentation. Generated inspection,
trace, projection, and trace-index files are ignored by Git. A missing generated
file is a prerequisite failure, not an interpreter or compiler failure.

## What a passing check establishes

Physical replay compares the observations selected by its protocol, at recorded
origins, rotations, and input sequences. Whole-world checkpoints additionally
cover queued work and motion state. Compiled checks compare a declared boundary
projection; matching final arithmetic alone can miss reset pulses, consumer
ticks, stale notifications, or an incorrect continuation after reset.

For a change, select the check that would fail if its **general contract** broke.
Include both admitted and rejected inputs for extraction changes, ordered
same-value samples for BUD changes, and save/reset continuation for lifecycle
changes. Preserve existing expected outputs. Establish a new baseline only after
reviewing the changed semantics and its provenance; never regenerate a reference
to hide a failing optimization.
