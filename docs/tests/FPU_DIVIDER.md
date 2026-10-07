# FPU divider regression

The [regression](../../crates/core/src/redpiler/analysis/tests/research/fpu_divider.rs)
compares the exact [divider schematic](../../test_data/piston-research/fpu-divider/FPU_DIVIDER.schem)
under the [versioned protocol](../../test_data/piston-research/fixtures/fpu_divider.json).
It exercises the general sampled runtime, shared geometry, missing-head entry,
ordinary output consumers, and interpreter handoff. The compiler identifies these
properties from geometry; the fixture is not a special runtime instruction.

## Inputs and observations

The schematic hash, dimensions, placement, cases, and observation coordinates
are declared in the manifest and [download record](../../test_data/piston-research/fpu-divider/download-manifest.json).
The test imports through the production loader without synthesizing heads or
implicitly settling the saved circuit. Coordinates below are selection-local;
the manifest places the selection minimum at `(40,30,40)`.

| Port | Local position | Protocol |
| --- | --- | --- |
| Trigger | Lever `(1,23,44)` | ON is idle; ON to OFF triggers computation. |
| A | Ten levers `(6,23,5+4*i)`, `i=0..9` | OFF represents one; declared mantissa weight `2^-i`. |
| B | Ten levers `(6,23,3+4*i)`, `i=0..9` | Same input encoding. |
| Output | Ten repeaters `(18+6*i,5,48)`, `i=0..9` | Unpowered is one; decode in increasing X order. |

All output repeaters begin powered, decoded as reset word zero. A negative pulse
is a transient result rather than a retained numeric register. Preparing inputs,
held triggers, repeated triggers, and observation windows follow each manifest
case's ordered steps.

The saved enable-cycle case asserts the observed word set `{0,170}`, with 170
encoded as `0010101010`. Tests compare the circuit's response rather than
substituting host-language division. Complete output scale, valid arithmetic
input range, parent-FPU behavior, and independent Java conformance are not
established by this regression.

## Assertions and their limits

For each declared case, the test imports independent interpreted and compiled
worlds. It compiles without `--assume-instant`, both with default options and with
`--optimize --io-only`, using full-plot context. Assertions require active
compilation and sixteen missing-head warnings for this exact saved entry.

The saved missing heads are at local `(x,14,z)` for `x` in `{14,15}` and `z` in
`{12,16,20,24,28,32,36,40}`. They remain actual absent geometry at import. The
runtime must preserve its supported saved-state contract, including later
movement; treating every headless actor as permanently inert is not a general
rule.

The compared output is the **ordered sequence of changed words**. Consecutive
equal words collapse, and timestamps are not compared. This establishes the
declared sequence under the tested protocol; it does not certify an identical
per-game-tick waveform or arbitrary simultaneous input histories.

After reset, the test checks warning clearance and the selected head geometry,
then compares another 48 interpreter ticks using the same changed-word projection.
This covers that continuation, not every possible reset phase or whole-world
motion entity.

## Run and capture

```sh
cargo test -p mchprs_core --lib --locked divider_compiles_without_assumptions_and_preserves_triggered_output
```

Fresh characterization requires a new output file and evidence ID:

```sh
python tools/capture_piston_research.py --fixture fpu_divider --output target/divider-new.json
python tools/summarize_piston_research.py --ingest target/divider-new.json --id divider-new
python tools/summarize_piston_research.py --check
```

Ingestion adds a generated trace and versioned source/summary records; inspect
those changes before adopting evidence. Existing capture sidecars identify their
recorded sources, and older rejected admission remains historical evidence.
Neither a fresh capture nor a historical result replaces the runnable assertions.
See [instant-piston procedures](INSTANT_PISTONS.md) for shared capture rules and
[the compiled model](../REDPILER_MODEL.md) for universal runtime semantics.
