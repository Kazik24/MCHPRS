# CPU profiling with cargo-flamegraph

Use the existing `cpus` benchmark with the optimized profiling profile and debug symbols:

```powershell
cargo install flamegraph --locked
cargo flamegraph -p mchprs_core --bench cpus --profile profiling --output target/cpu-bubblesort.svg -- --cpu cpu_bubblesort --graph-short --iterations 1
```

Open the SVG in a browser, search for `GraphBackend::tick`, and zoom into its stack. The existing harness runs Graph O0, O1, and O2 sequentially, then the interpreter. One flamegraph aggregates all three Graph windows and has no time axis, so it shows where Graph tick time goes overall but cannot compare O1 with O2 separately. Keep preparation and interpreter stacks out of the Graph tick inspection. Use this flamegraph capture; do not add profiler-specific runtime counters, a separate runner, or a report generator.

On Windows, accept the UAC prompt so Blondie can start its sampler.
