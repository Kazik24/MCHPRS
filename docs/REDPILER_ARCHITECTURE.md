# Redpiler architecture

The player-facing Redpiler uses the physical Graph backend. A separate private
reference experiment lives in `redpiler/sampled.rs` and `redpiler/sampled/`; it is
not selectable on player plots. The former `event_netlist` executor and compiler
have been removed. Historical results from that experiment are retained below
as evidence, not as current implementation or acceptance status.

## Current execution paths

The active `sampled/adder_tests.rs` manifest acceptance test currently fails at
world admission. Its presence is a required future execution boundary, not
evidence of ADDER1BIT compilation or runtime support. Current verification and
the exact failure are recorded in the progress document.

Graph compiles selected regions into `CircuitProgram` and `CircuitState`, then
runs them in `GraphBackend`:

```text
World selection -> graph discovery -> ordered plans -> GraphBackend
```

The graph and its existing optimizations remain the player-facing path. The
sampled reference experiment uses a private compiler, validated immutable `Program`,
and mutable `Runtime`. The compiler probes an isolated `ScratchWorld`; the
runtime executes ordered instructions and its own scheduler without Graph or
an interpreter. It currently admits an empty selection or one floor/wall lever,
one or more dust wires and one lamp, with supported stationary geometry,
decorative signs and settled input changes. Native sampling and runtime
self-validation can decline geometries whose behavior is not represented.

The sampled implementation is split by responsibility:

- `sampled/compiler.rs`: world discovery, native sampling, self-check and lowering.
- `sampled/program.rs`: immutable records and structural validation.
- `sampled/executor.rs`: runtime state, ordered instructions and scheduler.
- `sampled/tests.rs`: reference acceptance and mechanism tests.
- `scratch.rs`: captured isolated interpreter worlds, entry-state validation,
  boundary detection and native observation/electrical-discovery helpers.

The existing Graph backend, analysis and pass directories remain shared
infrastructure. Scratch probes clone the selected region, reject pending entry
work, and report reads beyond capture. Compilation does not mutate the host.

## Sampled behavior and limits

Rise and fall use separate ordered programs. `Contribution` recalculates one
attenuated link from its source's live output; `RefreshWire` stores the wire's
current incoming strength. Neither operation dispatches a receiver by itself.
For each lamp, the compiler examines all six neighboring cells. It collects
possible directional weak-power sources, expands eligible dust through the
shared scratch wire extractor, and checks one neighboring layer for strong
sources through a solid conductor. Solid conduction does not recurse. Repeated
source paths keep their minimum attenuation. Captured-world reads are checked
before source positions are resolved to deterministic component indexes; the
admitted chain now gets its lever-to-lamp contribution from discovery. This
does not widen executable admission to multiple sources.
Explicit `UpdateLamp` always delivers. Watcher delivery uses `UpdateOnce`,
which suppresses repeated watcher callbacks in an input phase; a relevant
contribution change invalidates the marker. Production compilation currently
installs no world-derived watcher records.

Runtime drains due work in the current scheduler bucket, advances the ring
once, then drains the new current bucket through the same lamp tick dispatch.
The native `schedule_tick(..., 2, ...)` lamp-off delay remains four game ticks.
The compiler's sampled-runtime self-check and comparisons of component state
and pending ticks remain enabled.

The acceptance gate compares initially-off and initially-on loaded entry,
including 12 alternating transitions, the 32-bucket wrap, host preservation,
settled pending work and tick-4 lamp shutoff. Boundary tests reject tight
captures and an excluded electrical source with positioned errors.

Compile-time inventory records empty piston actors and their shared contact
geometry. It does not enable piston execution. The unmet piston gate is native
comparison of extension, retraction and rearm, including accepted actions and
pending work. World-derived watcher discovery, receiver ports, observers,
resource movement, BUD batching, interacting circuits and player-facing
activation also remain outside the sampled runtime.

## Historical event-engine evidence

The retired event engine used `EventProgram`/`EventState` and temporary
`LoweredCircuit` adapters. Its final recorded suite result was 45 passed,
1 failed and 1 ignored. ADDER1BIT `prepared-0-1-0` differed at the declared sum
output at tick 3 (actual 0, expected 1); COUNTER_BASIC remained ignored after a
recorded piston divergence at `(56,36,44)`. Selected static-piston and
one-resource BUD comparisons passed through those adapters. These results are
historical and are not run by current test commands.

Those useful scenarios remain future reference acceptance coverage, using the
existing fixture manifests and their declared outputs and observation windows:

- Static piston and event-torch comparisons against native execution.
- BUD resource store/hold/sample/resample, movement callback order and
  quasi-power during trigger departure.
- COUNTER_BASIC's declared output window, currently awaiting piston execution.
- ADDER1BIT cases, including `prepared-0-1-0`, at the manifest's sum boundary.
- ADDER11BIT, CPU_BUBBLESORT, FPU_27 and TEST fixtures after required
  components and interactions are represented.

Do not treat old executor results as reference coverage. Preserve the fixture inputs,
expected outputs and boundaries when each scenario is migrated.

## Verification

Current verification on 30 October 2026:

- `cargo test -p mchprs_core redpiler::sampled --lib`: 19 passed, 0 failed,
  0 ignored (1,003 filtered out).
- `cargo test -p mchprs_core redpiler::scratch --lib`: 15 passed, 0 failed,
  0 ignored (1,007 filtered out).
- `cargo check -p mchprs_core --tests` and `git diff --check`: passed.
