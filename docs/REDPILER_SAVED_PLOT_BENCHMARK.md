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
