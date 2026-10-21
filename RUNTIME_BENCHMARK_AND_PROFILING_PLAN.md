# Redpiler runtime benchmarking and profiling

## Plot and connected-client measurements

Use the production-profile plot loop when investigating in-game TPS:

```text
cargo bench --locked -p mchprs_core --bench cpus --features plot-bench -- --plot-client --cpu cpu_bubblesort --graph-tier 5 --seconds 3 --iterations 3 --output target/plot-client.json
```

This mode runs `Plot::update` with one compressed loopback TCP client. It includes
tick batching, timing monitoring, graph projection, visual collection, player and
sidebar updates, packet queueing, compression and socket delivery. The receiver
decodes incremental block updates and checks its final received states. An ordered
end marker waits for delivery, including any packet already removed from the queue
by the writer. Login, initial chunk loading and Minecraft rendering are outside
this measurement. `--screen-only` selects the existing screen presentation mode.

Each sample replays the same number of ticks through the core and checks matching
engine work, projected world state and pending future after interpreter handoff.
Setup, compilation and these checks are outside the measured intervals. Samples
must finish within the fixture's active window; shorten `--seconds` if necessary.
The JSON reports plot TPS, core TPS, final delivery latency, stage durations and
network totals. Writer encoding/compression/write durations overlap plot work;
do not add them to the plot duration. Core and plot runs occur sequentially, so
repeat samples and use stage durations before attributing small differences.

In game, `/tps timings` reports engine execution, graph projection, total plot
updates, visual collection and enqueue time. Network counters cover the current
connections' lifetimes; plot counters cover the loaded plot's lifetime. Compare
two reports over the same active interval to locate the cost on a real client.

The `plot-bench` feature exposes only the in-memory benchmark adapter. It does
not enable Rust test recorders in Redpiler. Packet regression tests reuse the
same constructor.

Use the existing `cpus` harness. Benchmark only active ticking; preparation, compilation, stimulus, publication, reads, and continuation checks remain outside its timed windows. Use paired unprofiled runs to establish speed changes and Samply to find expensive call stacks. Do not add a custom runtime profiling suite.

## Unprofiled comparison

```powershell
cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --graph-tier 3 --iterations 5 --output target/cpu-o3-benchmark.json
```

`--graph-tier 1`, `2`, or `3` selects one tier. O3 includes the current instant/reset specializations. Keep tier, fixture normalization, active tick count, build settings, and validation identical across comparisons. Record individual pairs and their ranges; a lower median alone does not establish a repeatable improvement. Confirm that state comparisons pass. Exact work counts are useful when the change should preserve work; an optimization intended to remove work needs boundary/continuation equivalence rather than unchanged counters.

## Sample the optimized executable

Samply 0.13.1 is installed on this machine. The native Windows recording below completed successfully using its standard elevated helper. Keep the matching executable and PDB together so later builds do not invalidate the capture.

```powershell
cargo bench -p mchprs_core --bench cpus --profile profiling --locked --no-run
New-Item -ItemType Directory -Force target/profiles/cpu-current-o3
```

Copy the executable reported by Cargo and its matching `.pdb` to that directory. Substitute the actual executable filename below; the hash suffix changes between builds.

```powershell
samply record --save-only --no-open --rate 1000 --main-thread-only --keep-etl --profile-name CPU_BubbleSort-current-O3 --output target/profiles/cpu-current-o3/cpu-o3.json.gz -- target/profiles/cpu-current-o3/cpus-b8f3588b7a903875.exe --graph-short --cpu cpu_bubblesort --graph-tier 3 --iterations 5 --label sampled-current-o3 --output target/profiles/cpu-current-o3/episode.json
```

The `profiling` Cargo profile inherits release optimizations and retains debug symbols. Build without `graph-trace`, test recorders, or the deliberately incorrect `unsafe-turbo-fixed-point` feature. The existing benchmark checks correctness while recording. Sampling adds overhead; do not report its elapsed time as an unprofiled performance result.

`--keep-etl` preserves the original Windows trace for reprocessing; this capture used approximately 3.62 GiB of disk space. Omit it if that raw trace is not needed. This is disk storage, not an RSS measurement.

## Inspect only inference

```powershell
samply load --no-open --port 39119 --symbol-dir F:/rustrepos/MCHPRS/target/profiles/cpu-current-o3 target/profiles/cpu-current-o3/cpu-o3.json.gz
```

Open the URL Samply prints. Filter stacks to `GraphBackend::tick_with_world`, `tick_with_host`, or `tick_phases`; exclude preparation, validation, interpreter execution, and publication outside these roots. The harness also performs a first-tick reset check, so an all-Graph-tick filter includes that extra tick in addition to the five measured windows. Compare tiers in separate captures, not a combined flamegraph.

Check that function names and inline frames resolve through the matching PDB before interpreting the profile. In the completed capture, `--unstable-presymbolicate` initially generated generic application names and a fallback `.syms.json` sidecar that masked later PDB lookup. Moving that fallback sidecar aside and loading with the explicit symbol directory resolved every sampled application address. Omit that optional presymbolication flag for this workflow.

Report the sample denominator and distinguish inclusive from self time. Nested percentages overlap: piston action draining can contain callbacks, Turbo walks, site resolution, and guard evaluation. Do not add their inclusive shares. Use a disjoint partition when describing where total time goes. CPU sampling alone does not establish allocation volume, cache misses, or peak memory.

The completed current O3 capture and source-confirmed follow-up opportunities are documented in [CPU_BUBBLESORT_RUNTIME_PROFILE.md](docs/notes/redpiler-pistons-v4/CPU_BUBBLESORT_RUNTIME_PROFILE.md). Raw samples, matching symbols, benchmark checks, and numeric/folded-stack reports are under `target/profiles/cpu-current-o3/`.

The earlier `cargo flamegraph` capture remains historical evidence, but combined multiple tiers and preparation. It cannot compare current O1/O2/O3 performance. A plain Windows flamegraph attempt also failed with `NotAnAdmin`; Samply's standard helper successfully captured this run. Stop the local symbol server after inspection.
