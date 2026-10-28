# Event-netlist research results: 28 October 2026

## Status

The new event-netlist passes are disabled in production. They find and inspect
candidates but do not provide a complete executor, safe native handoff, or
fallback after activation. The corrected saved FPU and neighboring Divider do
not activate this executor. The existing Graph backend remains available; the
previously measured Counter fast path is separate from this research. No new
server binary was deployed as part of this investigation.

The experimental fast-mode contract discussed for future work compares settled
observable outputs. It permits an idealized Boolean result where native
nanotick ordering differs, but it does not permit silently freezing an
independent device or losing an effect needed for later observable behavior.
Input lamps and readout repeaters are outputs. Unsupported command diagnostics
may be excluded only under an explicit compatibility contract. No such new
public mode is enabled by this research.

## Reading compiler warnings

A connected-region warning describes a particular candidate whose optimization
was skipped and gives a short reason and clickable block coordinates when a
physical witness is available. Clicking a coordinate highlights it. Reset the
compiler, aim at a piston or wire, and run `/rp inspect` for matching local and
connected-region declines. Use `/rp inspect x y z` to inspect a recorded
coordinate, including a contact or air site that cannot be targeted by ray.
Inspection defaults to O6; pass the same flags as
the intended compilation to inspect another mode. `/rp inspect` is structural
and reports plot-wide compilation warnings, but does not test backend activation;
run `/rp analyze -O6` with the intended flags for an activation check.
`/rp analyze --graph -O6` inspects the prepared graph only.

A warning means the compiler could not apply that optimization under its
current rules; it does not by itself prove the redstone circuit is broken.
Other regions may still be accelerated. Chat output is bounded: an omitted
count is shown when a local list or warning list exceeds its limit. Scoped and
whole-plot decline messages are plot-wide, not necessarily attributable to the
targeted block. The recorded witnesses are examples, not a complete list of
every blocker or proof of the circuit's behavior.

If an instant-piston reset is not recognized, inspect the highlighted piston,
its power source and reset path. Some ordinary or deliberately held pistons
will also be unrecognized; use the circuit's intended behavior and an
interpreter comparison to decide whether anything needs repair. The corrected
saved plot and its user-provided piston fixes were the basis for the experiments
below; these measurements do not establish that the remaining circuit is faulty.

## Fixture and measurements

The corrected saved plot is `target/build-audit-tps/prod/p-1,-1-FPU27-2026-10-27`,
SHA-256 `261e7c67467d788023b0b809f439cf390e09d0e449ab103fcc5c75be75bcf319`.
The replay was built with `PLOT_SCALE=5`, the current workspace default. Game
ticks and redstone ticks differ: one redstone tick is two game ticks.

| Experiment | Result | Limit |
| --- | --- | --- |
| Saved-plot native replay | Changing inputs and six-phase handoffs passed; zero logical devices admitted. | Graph executed the work; this is not acceleration. |
| Greedy source-qualified discovery | Two potential groups: FPU 8,016 actors / 24,857 wires, Divider 2,054 actors / 5,115 wires. With 8x budget, discovery took 8.08 s and sampled peak working set was 1.18 GiB. | Discovery is not execution or ownership certification. |
| Conservative ingress cuts | 2,111 cells / 2,188 actors and 355 corridors retained in 34 rounds; largest region: 330 cells / 118 corridors. | The largest region received 64,407 of 6,171,791 callbacks (1.04%) in 288 changing-input ticks. Callback counts exclude internal wire walks. |
| Compact global power/contact shadow | 68 inputs, 4,753 expressions; 30,912 power-query and 21,592 contact comparisons matched across eight stopped/drained input words. About 114 µs per evaluation in that small sample. | Graph remained authoritative, native timing was not proved, and publication and native work were excluded from timing. |
| Candidate-local shadow under load | Around 782 µs per frame for input sampling and evaluation across 80 measured changing-input frames. | Excludes native execution and publication; not game TPS. Active contact snapshots differed from native and were not a settled-output proof. |
| Test-only FPU bulk prototype | About 1,040 game ticks/s in a limited backend test; checked stopped FPU and outside states matched in eight cases. | It starts with scheduled ticks cleared, differs at active samples, and has no safe handoff. It is not production-ready. |
| Test-only two-domain bulk prototype | Apparent 4,800 game ticks/s. | **Invalid:** the Divider domain published once while 1,059 native actors changed; it suppressed missing event ingress. Do not use this speed as evidence. |
| Changed-source-only notifications | Fewer callbacks, but 140 of 408 stopped FPU observations differed. | Failed correctness; removed rather than enabled. |

On the cleaned source, the standalone Counter O6 benchmark passed its frozen
native progression reference in all three samples. With 600 warmup ticks,
1,000,002 active ticks per sample, fat LTO and no flush, median throughput was
6,034,662 game TPS in the wall scope and 7,636,716 in the timed tick scope.
The measured window had 166,667 logical evaluations and zero Graph callbacks,
wire walks or piston actions. The protocol starts the counter, then measures a
fixed active window; this is not a full server or changing-input FPU benchmark.
The local report is `E:\codex-builds\counter-cleanup-20261028.json`.

After removing the failed runtime experiment, the release Redpiler test suite
passed 781 tests with 93 ignored, including the connected-region diagnostic
test. The cleaned-source saved-plot discovery test separately passed at 8x in
13.61 s, selecting 8,084 owners and finding 3,864 aggregate targets; it still
installed no executor. Ignored saved-plot and benchmark runs must be reported
separately.

## Root cause and next implementation

The old fast frame could evaluate a compact FPU computation for a qualified
contract, but the saved plot also has native consumers and resource ownership
that the old frame did not safely cover. The old whole-plot ownership closure
can overlap equation support because a read dependency, an event dependency,
and exclusive execution ownership are not the same relationship.

The new bulk experiment exposed a second concrete problem. A callback can
enter a compiled wire corridor and later reach a piston through the wire's
original Turbo routes. Suppressing the entering callback and merely reevaluating
steady power loses the source-qualified activation. This is why the Divider's
ten steady Boolean inputs stayed constant even while native lever and observer
callbacks changed its actors. A dirty bit is not itself an actor request.

A viable implementation needs to retain source-qualified ingress, propagate
changed wire and cell events through the region, update shared resource state,
apply the supported reset contract, and publish the values and effects needed
at native boundaries. Unknown ingress, resource ownership or effects must remain
on Graph or cause admission to decline before ownership transfers. Independent
clocks and native readers must not be absorbed merely because they are read.
An experimental deterministic event ordering may be used under the agreed
idealized timing contract, but needs settled-output validation and an explicit
handoff policy.

The bounded corridor and live-state tests exercise pieces of this model; they
do not yet implement that full worklist on the saved plot. When developing it,
measure actual executed regions, removed native work, changing-input output
correctness and server throughput. Do not count candidate recognition, a shadow
evaluation, a cleared-scheduler run, or an executor that freezes a device as a
successful fast path.

## Reproduction

From the repository root, use the corrected snapshot and the opt-in release
test. The snapshot is not part of the repository and must be supplied locally.

```powershell
$env:MCHPRS_SAVED_PLOT = 'F:\rustrepos\MCHPRS\target\build-audit-tps\prod\p-1,-1-FPU27-2026-10-27'
cargo test -p mchprs_core --locked --release --lib saved_fpu_research_greedy_discovers_source_regions -- --ignored --nocapture --test-threads=1
cargo test -p mchprs_core --locked --release --lib event_netlist -- --nocapture
```

The Counter comparison also requires the locally generated native reference
`target/build-audit-tps/counter-native-1m.json`. With that reference present,
the measured standalone run was:

```powershell
$env:CARGO_TARGET_DIR = 'E:\codex-builds\source-region-target'
$env:CARGO_INCREMENTAL = '0'
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --tier 6 --ticks 1000002 --iterations 3 --reference target/build-audit-tps/counter-native-1m.json --output E:\codex-builds\counter-cleanup-20261028.json
```

The exact historical experimental logs, when retained locally, are under
`E:\codex-builds`, including `fpu-bulk-settled-20261028.log`,
`fpu-bulk-divider-audit-20261028.log` and
`fpu-coverage-activity-20261028.log`. They are not checked-in fixtures. A
replay must report its input, binary, options, sample count and observable
outputs; an absent local snapshot or log must be reported rather than replaced
with an unrelated run.
