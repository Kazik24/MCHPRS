# Current engine baseline before packed classic execution

Date: 2026-10-18. The results below measure the existing production Graph backend. No proposed packed execution engine is enabled. The benchmark harness is `crates/core/benches/classic_baseline.rs`.

## Measurement contract

- CHUNGUS: first 16,384 active game ticks after pressing the saved Mandelbrot start button, default O2 whole fixed execution.
- ANPU: first 5,000 active game ticks after starting the saved Pong machine, measured separately at O2 and O3.
- Five samples per configuration. Every sample reloads the original saved machine and compiles afresh; it does not continue where the previous sample stopped.
- One contiguous `Compiler::tick_with_world` loop is timed. Loading, compilation, activation, display projection, reset, checking and native continuation are excluded. No intermediate display flush occurs in the timed loop.
- Compilation, activation, one final full flush, and reset have separate timings in the JSON. These are not a per-frame display benchmark or server/network throughput.
- Production `--release` build, default release settings including fat LTO. No test-only fixed-classic work counters, graph tracing, or unsafe fixed-point feature. Ordinary production Graph counters remain enabled.
- Windows x64, AMD Ryzen 9 5950X (16 cores / 32 logical processors), 85,816,561,664 bytes OS-reported physical memory. The benchmark inherits affinity mask `0x10`, logical CPU 4. Configurations run sequentially; no Cargo/rustc process runs alongside the measurements.
- Compiler: rustc 1.98.1 (48a229cea, 2026-09-01), LLVM 22.1.8. Default target settings; no custom native-CPU tuning was added.
- Compiler settings: budget multiplier 8, `classic_redstone=false`, `stable_instant_inputs=false`. The first flag controls the older optional local-net pass; **whole fixed compilation is enabled by default at O2**, and the harness requires complete whole-fixed CHUNGUS ownership. ANPU retains its supported general/O3 specialized execution. Native fallback is reported by coverage.
- Bounds are `world.get_corners()` and are included in each sample. No schematic normalization or unsafe boundary relaxation is applied.

## Correctness contract

The harness runs the independent native interpreter over the same complete window. Every measured sample must match all projected blocks and block entities, ordered chat contents, and the exact exported future after reset. It must also match the independent machine after 32 further native ticks. Scheduled work and callbacks must be nonzero, preventing an idle no-op result. ANPU's native run additionally checks the frozen screen frames inside the window.

This timed loop validates the complete machine at the endpoint and continuation; it does not observe every intermediate tick. The earlier full-tick differential tests remain separate evidence. In particular, prior CHUNGUS checks covered 1,025 native boundaries and 4,097 paired exact-state boundaries; the recent current-source Redpiler suite passed 450 tests. The historical 12,411,975-tick Mandelbrot completion is outside this baseline.

## Reproduction

Build before measuring, then freeze the executable so later workspace edits cannot change it:

```powershell
cargo build -p mchprs_core --release --bench classic_baseline
$binary = Get-ChildItem target/release/deps/classic_baseline-*.exe |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
Copy-Item -LiteralPath $binary.FullName -Destination target/classic-baseline-release.exe
[System.Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = [IntPtr]16

& target/classic-baseline-release.exe --cpu chungus --graph-tier 2 --ticks 16384 --iterations 5 --output target/classic-baseline-chungus-o2.json
& target/classic-baseline-release.exe --cpu anpu --graph-tier 2 --ticks 5000 --iterations 5 --output target/classic-baseline-anpu-o2.json
& target/classic-baseline-release.exe --cpu anpu --graph-tier 3 --ticks 5000 --iterations 5 --output target/classic-baseline-anpu-o3.json
```

The frozen executable used for this baseline has SHA256 `4fbd1cf367023c7fabe84a7f727ce0be70a04402ad3c3b80a932bfb8b4c484d3`. Reports include source/fixture/executable hashes, dirty-worktree provenance, coverage, work counts, all sample times, and medians/ranges. Environment metadata is retained at `target/classic-baseline-environment.json`.

Keep this frozen executable when comparing the future packed engine. Use the same machine, affinity, inputs, windows and checking contract; alternate old/new runs and compare medians. Static wire coverage alone is not a performance result. Report active game ticks/s, not half-ticks, idle ticks or a throughput extrapolation from fewer active ticks.

## Measured results

Five fresh samples per row. TPS means game ticks per second.

| Machine | Tier | Active ticks/sample | Median core time | Median TPS | TPS range | Median time/tick |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| CHUNGUS | O2 | 16,384 | 13.109408 s | 1,249.8 | 1,219.0–1,257.3 | 800.1 us |
| ANPU | O2 | 5,000 | 2.887614 s | 1,731.5 | 1,684.5–1,760.2 | 577.5 us |
| ANPU | O3 | 5,000 | 2.986678 s | 1,674.1 | 1,615.7–1,792.9 | 597.3 us |

Use **CHUNGUS O2 and ANPU O2** as the primary baselines. The O3 ANPU samples varied more and show no demonstrated gain. Its coverage and work counts are identical to O2; this is not evidence that O3 fundamentally slows the machine.

| Machine | Tier | Median compile | Median one final full flush | Median reset |
| --- | --- | ---: | ---: | ---: |
| CHUNGUS | O2 | 28.359 s | 7.150 ms | 58.961 ms |
| ANPU | O2 | 7.667 s | 4.115 ms | 17.394 ms |
| ANPU | O3 | 10.239 s | 4.080 ms | 17.862 ms |

The single final flush cost is not a per-tick or per-frame flush estimate: it accumulates dirty state across the complete window. Compile times include validation/lowering and use the same pinned CPU as inference.

## Ownership and work

CHUNGUS executes all 251,485 actors and all 133,623 wires through the whole fixed program. Across 16,384 ticks it performs 1,379,485 scheduled component ticks and counts 76,786,878 callbacks. The generic wire-walk count is zero because traversal executes the typed fixed program; ordered wire propagation still exists. There is no native fallback.

ANPU has 59,998 actors, including 34,781 wires and 2,898 pistons. Both O2 and O3 report zero whole-fixed actors, zero compiled local-net wires, and zero private proxies/built-in reset clocks/instant instruction routes for this machine. Execution remains general Graph, with no native fallback. Each 5,000-tick window performs 267,998 scheduled component ticks, 16,389,643 callbacks, 183,751 generic wire walks, 327,585 piston samples and 10,682 piston actions. All work counts repeat exactly across samples and tiers. This is a concrete target for mixed classic-network compilation.

## Verification and retained artifacts

All **15/15** measured samples matched the complete native endpoint state/future and its 32-tick native continuation. Each ANPU native reference also matched 14 pinned screen frames in the 5,000-tick window. No circuit was declined or normalized in these runs. Earlier intermediate-boundary tests remain separate; endpoint equality alone is not a proof of every intermediate sample.

Raw reports/logs: `target/classic-baseline-chungus-o2.{json,log}`, `target/classic-baseline-anpu-o2.{json,log}`, `target/classic-baseline-anpu-o3.{json,log}`. Machine-readable comparison summary: `target/classic-baseline-summary.json`. Build/check logs: `target/classic-baseline-release-build.log` and `target/classic-baseline-check.log`. A separately named copy of the frozen baseline binary is retained as `target/classic-baseline-before-packed-20261018.exe`.

The benchmark compiled under the production release profile and passed its native self-checks; formatting and diff whitespace checks also passed. The GraphExecutionCounts type has no Serialize implementation, so the harness exports explicit scalar window deltas instead of adding serialization to production state. No production inference code changed for this measurement.
