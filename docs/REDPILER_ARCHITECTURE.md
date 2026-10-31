# Redpiler architecture

Redpiler compiles admitted regions into the sampled runtime in
`crates/core/src/redpiler/sampled/`. It uses the same public compiler facade
for plots and headless sessions. The native interpreter remains responsible
for any circuit or entry state the compiler declines.

## Compile and handoff

Compilation reads an activation snapshot and probes an isolated `ScratchWorld`.
It does not mutate the live world. Plot compilation runs on a worker and
activates only if the source world still matches that snapshot. Cancellation,
unsupported components, unsupported block entities, partial ticks, and active
piston motion leave native state in place.

An active compiler owns its immutable program and mutable runtime state. It
accepts controls only for compiled inputs, publishes supported block values on
flush, and owns imported scheduled ticks until reset. Reset exports the runtime
state, pending ticks, queued piston actions, and supported motion records before
returning ownership to the interpreter. If export fails, the compiler remains
active and the caller receives the error; native simulation does not resume
from a partial export.

Plot and headless entry points use the same `Compiler` facade. Headless sessions
surface compile, tick, flush, and reset errors directly. `/rp` and `/redpiler`
offer compile and reset commands with only the implemented `--io-only` (`-i`)
and `--update` (`-u`) options.

## Supported behavior

The compiler samples native transitions in scratch worlds, lowers the accepted
mechanisms into a compact program, and checks runtime transitions against its
sampled model. The maintained acceptance suite includes both adder fixtures,
COUNTER, and focused TEST timelines, plus scratch boundary, wire discovery,
repeater, piston, scheduler, and runtime checks.

Admission is explicit. Components and entry states outside the modeled subset
return a positioned compile error. Pressure plates, command blocks, copper
bulbs, active piston motion, and partial piston phases are not compiled. Their
native world state and queued work remain available to the interpreter.

`--io-only` limits publication to visible outputs and controls. `--update`
runs native neighbor updates after reset. Compile statistics report component
count, imported pending ticks, and compile duration; no optimization-tier or
compiled-work counters are exposed.

## Verification

Run the sampled mechanism and reference suite with:

```sh
cargo test -p mchprs_core --lib redpiler::sampled:: -- --test-threads=1
```

The scheduler, plot lifecycle, piston, headless, history, and copper-bulb
regressions live beside the native modules they exercise. Workspace and all
target checks are run with:

```sh
cargo check --workspace --all-targets --locked
cargo check -p mchprs_core --all-targets --locked
```

Historical measurements and experiments remain under `scraps/` and
`test_data/`; they describe only their recorded revision and protocol.
