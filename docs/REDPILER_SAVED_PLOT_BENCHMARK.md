# Saved FPU and Divider replay

`instant --saved-plot` is an opt-in release benchmark of the saved production
snapshot. It shares the profile loop with the ignored native oracle test and
reports actual Graph execution counters, compilation time separately, and the
measured interval. This is direct Graph replay, not server TPS: it omits
`Plot::update`, player and packet work, and disables offline command-output
limits. It performs a full Graph flush every six ticks, whereas the server's
Compiler flush honors `io_only` and its visual sends are scheduled separately.

Use an isolated source copy with `PLOT_SCALE=5`; do not change the normal source.
The executable requires snapshot SHA-256
`426d2395869062817fa698dca1b7a101c3594728cd440389970a4e6004f06d17`
and manifest SHA-256
`981109d849cadaf13a059c295a6d78444d0626a21c72683cf425eb6344b68227`.
Set `MCHPRS_SAVED_PLOT` to the snapshot's absolute path and
`MCHPRS_PROFILE_FRAMES=192`. With the copy as the working directory:

```powershell
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
cargo test -p mchprs_core --locked --release --lib redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_neighbor_graph_fallback_profile -- --ignored --exact --test-threads=1
cargo bench -p mchprs_core --locked --features saved-plot-bench --bench instant -- --saved-plot
```

Run correctness first,
then collect at least three quiet benchmark samples and report each and the median.
The test compares native outputs, command history, physical state and all six
handoff offsets; the benchmark reports native parity as not run. The benchmark
checks the manifest ports, command witness, readout activity and neighboring
activity itself. Record source revision and working tree, compiler version,
build options and executable hash along with the reports.

On 27 October, the shared native oracle passed 192 changing-input frames, all
six handoff offsets and the eight-command witness. Three non-test samples of
the same workload measured 378.35, 377.34 and 379.09 game ticks/s (median
378.35); compilation took 49.46–49.71 seconds separately. Each sample
replaced 79 wire walks; installed kernels or evaluations alone do not establish
useful acceleration. Concurrent activity was not ruled out. Logs are
`target/saved-bench-native.log` and `target/saved-bench-sample-{1,2,3}.log`.
The benchmark executable SHA-256 was
`64BD63173E6CB2F85823B67D6B00A6AAB03D34A93E03914961F05F2D90065556`;
the source was the recorded dirty tree based on `d5de6100`, built with Rust
1.98.1, release LTO disabled and incremental compilation disabled.

`--saved-plot-server` runs the same input-frame generator through a real
`Plot::update` and a `Compiler` with one compressed TCP client. It keeps normal
command-output limits and checks each frame's indicators and readouts, plus the
ordered client-received command text, against an untimed native replay. Command
emission tick timing is not compared. The native comparison retains its command
queue and disables limits; the Plot run drains the queue and enforces limits. A feature-only cap limits each update to
the remaining ticks in the current six-tick input frame; this changes server
batching. The timing includes non-mutating Graph state observations each frame, as well as
the normal `io_only` projection and server visual sends. The report includes final delivery separately. It excludes login,
initial chunks and Minecraft rendering; it is not a measurement of an unmodified
live server under arbitrary clients or load. Its separate full physical-state
and six-phase native test remains required.

```powershell
cargo bench -p mchprs_core --locked --features saved-plot-bench --bench instant -- --saved-plot-server
```
Client packet and byte totals include warmup and the final delivery barrier;
the reported timed interval covers the changing-input frames only.

The 27 October bounded-batch Plot samples measured 321.55, 373.29 and 361.56
game ticks/s (median 361.56); including final delivery they measured 321.51,
373.13 and 361.50. Compilation took 50.29–51.69 seconds separately. All three
passed the per-frame native output and ordered client-command checks, with 1,769
logical evaluations, 231,453 instant-kernel invocations and 13,314,892 callbacks
per sample. The client used fast rendering and a 5 Hz visual send rate. The
samples varied substantially; quiet-machine conditions were not established.
The separate 192-frame, six-phase physical oracle passed in
`target/saved-bench-native-shared.log`; server logs are
`target/saved-bench-server-final-{1,2,3}.log`. The sample executable was
`target/saved-bench-server-samples.exe` with
SHA-256 `80F72AB7FF6E5F98D5394F767BE079F3915005D1BDE288E928DFAC4AAE7D3BC4`
before the subsequent reporting-only addition of execution-elision counters.
The build used the dirty tree based on `d5de6100`, Rust 1.98.1, scale 5 in an
isolated copy, release LTO disabled and incremental compilation disabled.

The reporting-only rebuild is pinned as `target/saved-bench-server-counters.exe`
(SHA-256 `0B60D04E9134BB9BBB8841B4D51F1C0225C875E098F3E45ECAC3905228DDE867`).
Its 16-frame server smoke passed native output and ordered command comparisons;
`target/saved-bench-server-counters-smoke.log` reports 16,501 instant-kernel
invocations, zero kernel wire walks elided, 13,518 transition hits, 545,396
transition operations and 364,311 transition wire visits elided. These are
smoke-workload counts and should not be scaled to the 192-frame samples.

### Furnace reset recognition check, 27 October 2026

A fresh isolated copy of workspace HEAD `e650513c6802d4b211e5901616c9ee818b2a5b11`
with its then-current dirty changes, including the furnace reset-recognition and
warning changes, was built with Rust 1.98.1. Only the copy used `PLOT_SCALE=5`;
the workspace retained scale 4. Release LTO and incremental compilation were
disabled. The snapshot and manifest matched the pinned hashes above. The
reset-recognition source SHA-256 was
`4B13AF614B2AFDC617728CC83EBE13F9BBE85E28A6D18B6C5D3BD10679763D66`.

The 192-frame native oracle passed. Its main run compared 1,192 ticks including
16 interpreted continuation ticks; every tick compared 1,186 lamps, repeaters
and comparators, 1,066 comparator entities, all compiled command-block entities,
and command-output history. The warmup produced eight command outputs. All six
handoff offsets compared 70,649 physical positions at handoff and after
continuation; the five other offsets used 16-frame histories. The report says
logical-device admission was declined and zero logical devices were installed.

Three sequential bounded-batch server samples passed their per-frame indicator
and readout comparisons and ordered eight-command client-output checks. They
measured **352.41, 381.68 and 388.92 game ticks/s** (median **381.68**);
including final delivery: 352.32, 381.56 and 388.87 ticks/s. Compilation took
49.59–49.87 seconds separately. Each sample recorded 13,313,250 callbacks,
233,331 instant-kernel invocations, 79 kernel wire walks elided, 1,769 logical
evaluations and 431,823 piston actions. The earlier 361.56 median used a
separate executable and unpaired run; the broad timing spread and differing
conditions do not establish a speedup or production logical-device activation.
The server comparison does not compare command emission tick timing; its
bounded batches and non-mutating frame observations remain part of the scope
described above.

From `E:/codex-builds/divider-furnace-saved-bench`, the commands were:

```powershell
$env:CARGO_TARGET_DIR = 'E:/codex-builds/warning-island-target'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
$env:MCHPRS_SAVED_PLOT = 'F:/rustrepos/MCHPRS/target/build-audit-tps/prod/p-1,-1-goal-start'
$env:MCHPRS_PROFILE_FRAMES = '192'
cargo test -p mchprs_core --locked --release --lib redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_neighbor_graph_fallback_profile -- --ignored --exact --test-threads=1 --nocapture
cargo bench -p mchprs_core --locked --features saved-plot-bench --bench instant --no-run
& 'E:/codex-builds/warning-island-target/release/deps/instant-82e83835fd95208b.exe' --saved-plot-server
```

The final command ran three times sequentially. The oracle executable SHA-256
was `D87D3F5BD17276E4CE427878FDFC9CCF0BBECD4D3D10FA3ED8BA24BE2547AED8`;
the benchmark executable SHA-256 was
`3FC8CA887A2AF03AAA3DD8A8723C3E802B0F68AC92E6FD42914AF1A5692C8A26`.
Logs are `E:/codex-builds/divider-furnace-saved-native.log` and
`E:/codex-builds/divider-furnace-server-{1,2,3}.log`.
