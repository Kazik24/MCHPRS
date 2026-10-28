# Native interpreter and event research

This investigation concerns the native `PlotWorld` interpreter, not the Graph
backend or the unfinished FPU/Divider logical executor. The corrected saved
plot is `target/build-audit-tps/prod/p-1,-1-FPU27-2026-10-27`, SHA-256
`261e7c67467d788023b0b809f439cf390e09d0e449ab103fcc5c75be75bcf319`.
The workspace default is `PLOT_SCALE=5`; two game ticks are one redstone tick.
There is no independent arithmetic oracle for this plot.

## Ownership and behavior

`Plot::tick` selects either the live interpreter or Redpiler. In the interpreter,
`redstone::update` delivers component callbacks, and Wire Turbo runs an ordered
breadth-first walk with per-walk captured block views and live electrical reads.
Piston callbacks sample power and queue requests; event execution rechecks live
power. The plot dispatcher drains typed scheduled work, then FIFO piston events,
then moves the captured motion identities. Observer callbacks and delayed ticks
can start further work. Blocks, entities, queued ticks and physical motion state
are authoritative; indexes and piston route certificates are derived caches.
Shared payloads are represented by the actual moving or stationary block, not
independent logical copies. Save/load, partial stepping and compiler reset must
preserve pending work and motion, not just visible output.

Graph instead owns lowered components and pending work while compiled; reset
materializes the state and future for native continuation. The new event-netlist
passes are disabled; their candidates and shadow evaluations are not execution
or acceleration. The standalone Counter fast executor is a separate workload.
See [interpreter architecture](INTERPRETER_ARCHITECTURE.md),
[piston model](PISTON_MODEL.md), and
[event-netlist results](REDPILER_EVENT_NETLIST_RESULTS_2026-10-28.md).

## Correctness and diagnostics

The event research reinforces that steady power is insufficient to infer a
piston request: a source-qualified callback can enter a wire walk and later
reach a piston. It also distinguishes read dependencies from shared-payload
ownership and requires reset and boundary effects. Those are requirements for
compiled ownership, not proof of a native interpreter bug. Do not replace
captured views wholesale with live reads: that changes walk semantics.

The old Wire Turbo comment claimed non-wire callbacks never change block state;
lamp, hopper and trapdoor handlers can do so, and the note block explicitly
rereads live state. The comment is corrected. Whether a particular repeated
callback should see a captured or live state needs a focused fixture and an
independent behavior reference before changing execution.

Existing native tests cover BUD sampling, repeated motion, observer feedback,
shared OR payload and reset starvation, and an event/resource reference
contract. Useful additions are: two source identities entering a common wire
route with identical final power; multiple callbacks to a mutable receiver in
one captured walk; shared payload contention in both orders; repeated pulses
with observer, payload and input-restoration resets; and independent and
connected devices through reset and native continuation. Compare ordered
callbacks, block/entity state, scheduler and piston identities as well as
outputs. Current saved-plot comparisons use the interpreter as the oracle and
therefore cannot independently prove its arithmetic or vanilla behavior.

The changed-source-only notification experiment in the research failed: 140 of
408 stopped FPU observations differed. The prototype that appeared to achieve
4,800 TPS froze the Divider and is invalid as a baseline. Neither suggests a
native optimization. No circuit defect is established by a compiler decline.

## Saved-plot check

On revision `13b50706abb6e751b1916a496caf1cc1a58f0d9f` with the existing
uncommitted workspace changes, the ignored release test below passed with 16
alternating input frames and all six handoff phases. It compares lamps,
repeaters, comparators, command entities and output each game tick; physical
blocks/entities and pending scheduler/piston work are compared at handoff and
after 16 native continuation ticks. The warmup emitted eight command outputs;
the run refired commands, including the `div by zero` and `NaN` witnesses.
This is Graph versus native consistency, not an independent arithmetic proof.
The single Graph timing printed by this test ran during concurrent builds and
is not a performance baseline.

```powershell
$env:MCHPRS_SAVED_PLOT = 'F:\rustrepos\MCHPRS\target\build-audit-tps\prod\p-1,-1-FPU27-2026-10-27'
$env:MCHPRS_PROFILE_FRAMES = '16'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
cargo test -p mchprs_core --locked --release --lib redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_changing_inputs_refire_commands_with_six_handoffs -- --ignored --exact --test-threads=1 --nocapture
```

The same release test executable also passed all 12
`redstone::instant_piston_tests::io::` tests, including the adder, Counter,
shared-payload and reset fixtures. These are correctness checks, not timings.

## Native timing pilot

The opt-in `saved_plot_native_profile` test loads a fresh saved world per
sample, applies the command witness and runs 24 warmup game ticks, then changes
the 35 FPU inputs and outside lever controls every six game ticks. A fixed seed
produced 1,669 input and 21 outside-lever changes in each 96-frame sample.
Each sample saw 68 distinct readout words, a nonzero readout, and a changed
outside wire, lamp or repeater. Warmup emitted commands; the measured random
interval emitted none. Outside controls are found by a native scan: their order
has not been proven identical to Graph's, and the activity check alone does
not prove which neighboring device changed. Do not treat this as a controlled
Graph comparison or as proof that the Divider was exercised.

Three samples of 576 game ticks, on the same uncommitted revision as above,
used a release test binary with `CARGO_PROFILE_RELEASE_LTO=false`, SHA-256
`eca2e55919f6e482fe98823a1c4d34b896b75dd0583d7be645f91f1220348212`.
The host has an AMD Ryzen 9 5950X. No compiler process was present at sample
start; other system load and CPU affinity were not controlled. Seconds for
samples 1/2/3:

| Scope | Samples | Median |
| --- | --- | --- |
| Native ticks, including live world materialization | 8.137746 / 9.493506 / 8.198623 | 8.198623 |
| Input callbacks | 0.003737 / 0.004509 / 0.003415 | 0.003737 |
| Readout and outside-activity inspection | 0.000461 / 0.000503 / 0.000404 | 0.000461 |

The median tick-only rate is about 70.3 game ticks/s for this pilot; it is not
full-server TPS. Unlike Graph, native mutation is included in tick time and
there is no separate Graph flush. Observation here is a small readout/activity
check, not client rendering or network publication. The measurements do not
include loading and warmup. No production execution code changed, so there is
no before/after performance claim. Function-level sampled hotspots, a confirmed
changing Divider workload, isolated Counter/adder/Divider/piston timings, and
full-server measurements remain to be collected. The release binary has no
debug symbols; phase timers cannot identify function-level hotspots.
The pilot also does not assert changes to all 35 input indicator lamps.

```powershell
$env:MCHPRS_SAVED_PLOT = 'F:\rustrepos\MCHPRS\target\build-audit-tps\prod\p-1,-1-FPU27-2026-10-27'
$env:MCHPRS_PROFILE_FRAMES = '96'
$env:MCHPRS_PROFILE_SAMPLES = '3'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
cargo test -p mchprs_core --locked --release --lib saved_plot_native_profile -- --ignored --nocapture --test-threads=1
```
