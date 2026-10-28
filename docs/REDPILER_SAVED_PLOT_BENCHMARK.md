# Saved FPU and Divider replay

### OneShot compiler removal check, 27 October 2026

An isolated copy of the current dirty workspace used `PLOT_SCALE=5`; the shared workspace remained at 4. The copy's [source hash manifest](../target/current-oneshot-removed-source-hashes.txt) has SHA-256 `DBA2ED6AFCB5738A43D424B6C83F6F1F3360A5B212E5E721D0B86021C93C287C`. Rust was 1.98.1; release LTO and incremental compilation were disabled. The saved snapshot and port manifest matched the hashes listed below.

The [192-frame alternating command-witness oracle](../target/current-oneshot-removed-witness-192.log) passed native per-tick command and output comparison, with 192 measured command outputs and eight warmup outputs. The main phase compared 1,176 Graph ticks, 70,649 physical positions at handoff, and 16 native continuation ticks; each of the other five handoff phases used 16 frames. No logical device was installed: the saved plot used Graph fallback.

Three sequential bounded-batch server samples passed native per-frame indicator and readout comparisons and ordered client-command text comparisons: **357.48, 400.83 and 389.36 game ticks/s**, median **389.36**; including final delivery: 357.44, 400.71 and 389.31. Compilation took 49.19–50.34 seconds. Each sample recorded 13,313,286 callbacks and eight command messages, all from warmup. Server comparison does not check command emission tick timing; concurrent system activity was not measured. [Sample 1](../target/current-oneshot-removed-server-1.log), [sample 2](../target/current-oneshot-removed-server-2.log), [sample 3](../target/current-oneshot-removed-server-3.log).

From `E:/codex-builds/oneshot-removed-current-source-20261027`:

```powershell
$env:CARGO_TARGET_DIR = 'E:/codex-builds/warning-island-target'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
$env:MCHPRS_SAVED_PLOT = 'F:/rustrepos/MCHPRS/target/build-audit-tps/prod/p-1,-1-goal-start'
$env:MCHPRS_PROFILE_FRAMES = '192'
cargo test -p mchprs_core --locked --release --lib redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_changing_inputs_refire_commands_with_six_handoffs -- --ignored --exact --test-threads=1 --nocapture
cargo bench -p mchprs_core --locked --features saved-plot-bench --bench instant --no-run
& 'F:/rustrepos/MCHPRS/target/current-oneshot-removed-server.exe' --saved-plot-server
```

The server command ran three times sequentially. The preserved [oracle executable](../target/current-oneshot-removed-oracle.exe) has SHA-256 `27EE2C1E846FBB60E9ED0EBEF939F31310888DD084CFA531F17F91420291999C`; the [server executable](../target/current-oneshot-removed-server.exe) has SHA-256 `2A53D5286FEFBC46D84367782A3487CF691FD414708F25C38172EAE4A83E4303`.

### Single-cell runtime removal check, 27 October 2026

An isolated copy of the dirty workspace based on commit `13b50706abb6e751b1916a496caf1cc1a58f0d9f` used `PLOT_SCALE=5`; the workspace remained at 4. Its complete file-hash manifest is [post-removal-scale5-source-hashes.txt](../target/post-removal-scale5-source-hashes.txt), SHA-256 `12675B743992580AB848436C22A575D1049AEF22BD664BBB508BA6E7DBF45DAB`. Rust was 1.98.1; release LTO and incremental compilation were disabled. Snapshot and manifest hashes matched the pinned inputs below.

The 192-frame native oracle passed: it compared 1,192 ticks, including native continuation, and 70,649 physical positions at each of six handoff phases. It observed eight warmup command outputs. The saved plot declined logical-device admission and installed zero logical devices; this is Graph fallback, not FPU activation. [Oracle log](../target/post-removal-native-192.log).

Three sequential bounded-batch server samples passed native per-frame output and ordered client-command comparisons. They measured **358.38, 394.99 and 374.09 game ticks/s**, median **374.09**; including final delivery the measurements were 358.34, 394.94 and 374.04. Compilation took 48.03–48.17 seconds. Each sample reported eight client command messages and 13,313,250 callbacks. The reported 1,769 logical kernel evaluations are other Graph optimizations, not logical-device admission. The server comparison does not compare command emission tick timing. Concurrent system activity was not measured. [Sample 1](../target/post-removal-server-1.log), [sample 2](../target/post-removal-server-2.log), [sample 3](../target/post-removal-server-3.log).

From `F:/rustrepos/MCHPRS/target/post-removal-scale5-source`:

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

The last command ran three times sequentially. The preserved oracle executable is [post-removal-saved-oracle.exe](../target/post-removal-saved-oracle.exe), SHA-256 `4300CDEECD91062A2D81063BFA0DC5C390F8DC4CE5BA287FEAACAD0DC7E12C21`; the server executable is [post-removal-saved-server.exe](../target/post-removal-saved-server.exe), SHA-256 `0FEAD3EFA4B488A4DA02910993CC5C40DB60682B49DA18C39370F39BC7C27710`.

`instant --saved-plot` is an opt-in release benchmark of the saved production
snapshot. It shares the profile loop with the ignored native oracle test and
reports actual Graph execution counters, compilation time separately, and the
measured interval. This is direct Graph replay, not server TPS: it omits
`Plot::update`, player and packet work, and disables offline command-output
limits. It performs a full Graph flush every six ticks, whereas the server's
Compiler flush honors `io_only` and its visual sends are scheduled separately.

Use a build with `PLOT_SCALE=5`; the workspace now defaults to that scale.
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
# Greedy event discovery, 27 October 2026

The first greedy discovery slice does **not** yet accelerate the saved FPU. An
isolated scale-5 replay completed 16 alternating zero and manifest-derived
command-witness frames. It compared native command entities and output history
on every tick, observed 16 measured command outputs and eight warmup outputs,
and passed all six handoffs with 70,649 physical positions and 16 native
continuation ticks each. It admitted zero logical devices. The measured interval
had 936,210 callbacks, 17,664 instant-kernel invocations and 192 logical-kernel
evaluations; the latter are other Graph optimizations, not logical devices.
The direct Graph replay reported 404.94 ticks/s, but it ran alongside a build
and is neither an isolated measurement nor server TPS.

An instrumented census of the same source and input selected 9,479 of 9,906
catalog owners and retained 186,037 event occurrences. Its partition had
7,924 regions: 3,429 certified and 4,495 declined, including 4,492 with
`SignalArrivalUnproven`, two with `BoundaryEffectUnsupported`, and one with
`ProofBudgetExceeded`. The cumulative conservative retained-byte accounting
was 264,092,756 bytes against a 268,435,456-byte limit; this is not a measured
peak allocation. All reached owners were selected, so this run does not show
a useful trimming benefit. Certification does not establish runtime execution.
Logical-device admission separately declined its ownership/scope proof.

For comparison, the preceding same-input replay whose discovery exhausted its
budget recorded 936,170 callbacks and 17,800 instant-kernel invocations; the
40 additional callbacks in the completed-discovery replay show no meaningful
work reduction. Both runs preserved native output and command behavior. The
isolated diagnostics added logging only; the shared workspace remained at
`PLOT_SCALE=4`. The production Redpiler suite passed 774 tests, with 92 ignored.

From an isolated copy with `PLOT_SCALE=5`, Rust 1.98.1 and release LTO and
incremental compilation disabled, the replay command was:

```powershell
$env:CARGO_TARGET_DIR = 'E:/codex-builds/warning-island-target'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
$env:MCHPRS_SAVED_PLOT = 'F:/rustrepos/MCHPRS/target/build-audit-tps/prod/p-1,-1-goal-start'
$env:MCHPRS_PROFILE_FRAMES = '16'
cargo test -p mchprs_core --locked --release --lib redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_changing_inputs_refire_commands_with_six_handoffs -- --ignored --exact --test-threads=1 --nocapture
```

The compact replay log is `E:/codex-builds/greedy-scale5-budget-compact.log`,
SHA-256 `13DDDEC7EF71E0FCB056AC25280977112851E0F02F32576EEA604A58B29607D3`;
the census log is `E:/codex-builds/greedy-scale5-census.log`, SHA-256
`0D48D6BD3B93567A8CF1F796C7D03CAF52E79414F36E59454F165F666A792461`.
The workspace Redpiler source hash manifest is
`target/greedy-compiler-source-hashes.txt`, SHA-256
`6B8EE8F9B01A4862423F59A60E32B1A7E22227B7EA1A3F37ABA0050A68635D78`.
The manifest covers Redpiler source files, not the complete workspace or the
diagnostic-only logging in the isolated copy.

An isolated FPU_27 diagnostic used the updated full-plot snapshot, SHA-256
`261E7C67467D788023B0B809F439CF390E09D0E449AB103FCC5C75BE75BCF319`.
The 16-frame alternating command-witness replay passed the native comparison
and all six handoffs, emitted 16 measured command outputs, and installed zero
logical devices. It recorded 936,587 callbacks and 33,360 piston actions.
Its 403.38 direct Graph ticks/s was diagnostic timing, not a controlled server
benchmark.

Of those actions, 33,200 belonged to catalog actors inside the FPU bounds.
Across the entire catalog, 1,850 regions had an action or an observed
source-qualified wire boundary sample; 1,816 had at least one such sample. This
is an activity census, not a count of regions that can safely be accelerated.
The largest declined connected region had 96 FPU actors and 15 wires; 16 actors
performed 256 actions, and one actor received 24 native boundary-wire samples.
Its resource-state digest stayed constant over these 16 settled cuts while the
readout alternated; that observation does not establish whether the region can
affect other outputs or inputs.

An experimental power-root builder produced 96 actor roots for that region
using 700 inputs and 2,066 expressions. Across the full catalog, retaining 137
dot-mode wires as native leaves allowed construction of 10,070 actor roots
with 112,381 inputs and 373,051 expressions. Without that partition, the first
decline was a directional output dependent on dot/cross state. These are static
power equations, not source-event, ownership, callback, reset, output or handoff
certificates, and no shadow was installed on the saved plot. The experimental
activation index held 306,426 occurrence keys and rows and charged 44,125,344
bytes to its bounded budget.

The activity diagnostic's [log](E:/codex-builds/greedy-shadow-FPU27-all-region-activity.log)
has SHA-256 `921CA66FBBB9068ACFC42285E82611501E658FDC9775E2998F4AC136B846A5A6`;
the pinned executable has SHA-256
`94C2521DA5C9622448B1E9F5DC4F95FBEEE40CFCE71D36DB9F7155C77F1CAFA4`.
The power-partition [log](E:/codex-builds/greedy-shadow-FPU27-partition.log)
has SHA-256 `972DBAAB33BB132044E78D9B0088F134EA9D490BE60E5585C7F9EEB4901F1A23`;
its pinned executable has SHA-256
`20CF29D8231D18365302DF77805D26A88925EC4E44B40740137B125BA2525543`.
Both were instrumented test executables built in a separate scale-5 copy, and
that copy and the shared workspace were restored to scale 4 after the builds.
The activity replay can be repeated with the pinned executable:

```powershell
$env:MCHPRS_SAVED_PLOT = 'F:/rustrepos/MCHPRS/target/build-audit-tps/prod/p-1,-1-FPU27-2026-10-27'
$env:MCHPRS_PROFILE_FRAMES = '16'
& E:/codex-builds/greedy-shadow-FPU27-all-region-activity.exe redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_changing_inputs_refire_commands_with_six_handoffs --ignored --exact --test-threads=1 --nocapture
```

A subsequent diagnostic separated the census by region shape: nine connected
regions certified and 633 connected regions declined; 2,709 declined regions
contained only corridors. The largest declined connected region contained 96
cells and 15 corridors. Its first rejected callback was the previously observed
outside-wire boundary. The census log `E:/codex-builds/greedy-connected-census.log`
has SHA-256 `E906EE828C4FBC244897F6AC5FEF671F5B98F21B48949EDCCB8441E66300F788`.

Allowing certified PullWaves to take precedence over overlapping Event kernels
without a settled circuit did not change this plot's measured work. The final
lowered program has a settled circuit despite admitting zero logical devices,
so it already used the same precedence path. A 16-frame native replay again
passed all six handoffs and observed 16 measured command outputs. It installed
73 PullWave actors and 9,997 Event actors; diagnostic counters recorded 80
actual compiled PullWave route replacements, 24 source-provenance rejections,
and 112 stationary/evaluation rejections. Callback, instant-kernel invocation,
and logical-evaluation counts were unchanged from the preceding same-input
replay. The test-only instrumented log
`E:/codex-builds/greedy-pullwave-settled-presence.log` has SHA-256
`885FE22DED12F0336C6D870A8458E0C36AF54265CE0871EC3AB0233DB8A565AD`.
The strengthened no-settled-circuit four-cell fixture executes a PullWave in
the presence of overlapping Events and matches native state for 32 ticks;
the full Redpiler suite again passed 774 tests, with 92 ignored.

The scale-4 COUNTER benchmark ran three 1,000,002-tick samples without the
saved replay or test suite running. Each admitted an active logical device,
matched the frozen native progression reference, and recorded 166,667 logical
evaluations and zero callbacks. Wall rates were 5.940M, 5.678M and 5.907M
game ticks/s (median 5.907M). System activity was not controlled. With the
same release settings, the command was:

```powershell
& target/release/deps/instant-9886bdaec44393d7.exe --component counter_basic --tier 6 --ticks 1000002 --iterations 3 --reference target/build-audit-tps/counter-native-1m.json --output target/build-audit-tps/counter-greedy-1m.json
```

The benchmark executable, built with `cargo bench -p mchprs_core --locked --bench instant --no-run`, had SHA-256
`E3200088204803884CA48D57A1557D309A7818EDB278BCF7F2EA8301490546A2`;
the native reference had SHA-256
`0A128ADA3869CEEF97F94EBF62214FEFBB29C11EE2372ABB52A63868E6DC2CBA`.
The result JSON had SHA-256
`D224F30E4C2929F62ACA57B707DE2FC391CD59CD63FC53CB610632DB057E4CA7`.

The same scale-4 benchmark executable did not reproduce the historical
standalone FPU device result: the following command failed with `logical
device did not execute once per six-game-tick frame`. It produced no valid TPS
measurement. The captured log at `target/build-audit-tps/fpu-greedy-standalone.log`
has SHA-256 `56457A0DBCD30D002778BFEFF2CB6671BF82CC9DE6A42A3F6FCE789E08CD59BB`.

```powershell
& target/release/deps/instant-9886bdaec44393d7.exe --component fpu_legal --device-frame --tier 6 --episodes 16 --iterations 1 --output target/build-audit-tps/fpu-greedy-standalone.json
```
