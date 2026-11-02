# Performance check of the piston wire-shape fix

The fix has substantial measured performance regressions on ZCore and the active arithmetic fixtures. It is not performance-neutral. This pass changes no runtime code or correctness assertions.

## Comparison

Baseline: clean `30c87fcd5bb25b65c1794056cfc3eca842628eb0`, immediately before the uncommitted wire-shape fix. Candidate: the current worktree with that fix. Baseline sources and all build/measurement artifacts are isolated under `E:/codex-builds/piston-wire-performance-20261102/`; Cargo uses the existing E: cache. Ignored arithmetic fixture files were copied to the baseline so both builds use identical inputs.

Both Rust executables use the **optimized development profile**, `opt-level=1`, dependency `opt-level=3`, with debug information. Native LLVM engines use the same Clang `-O3` command, with semantic 19 for baseline and 20 for candidate. These are controlled comparisons, **not release/LTO throughput estimates**.

Host: Ryzen 9 5950X, Windows x64, Rust 1.98.1. Each benchmark process uses affinity mask `0xff` (eight logical processors) and `RAYON_NUM_THREADS=4`. No builds run during measurements. Baseline/candidate process order alternates between trials. CPU and arithmetic results use three trials per source variant; the small controls use five. No timing outliers were removed.

## Results

Times below are medians. Positive percentages mean **more elapsed time**, not a TPS percentage change.

| Workload / metric | Baseline | Wire-shape fix | Change |
| --- | ---: | ---: | ---: |
| ZCore Rust: 5,000 ticks, no publication | 3.833 s | 6.685 s | **+74.4%** |
| ZCore LLVM: 5,000 ticks, no publication | 1.695 s | 2.021 s | **+19.2%** |
| ZCore Rust circuit compilation | 24.424 s | 31.178 s | **+27.7%** |
| ZCore LLVM circuit setup | 27.551 s | 35.143 s | **+27.6%** |
| ZCore Rust full reset/publication | 249.0 ms | 340.0 ms | **+36.6%** |
| ZCore LLVM full reset/publication | 276.0 ms | 378.5 ms | **+37.2%** |
| ZCore peak process working set | 1,649.4 MiB | 1,960.4 MiB | **+18.9%, +311 MiB** |
| 11-bit adder: 5,760 evaluation ticks plus full publication | 336.6 ms | 432.6 ms | **+28.5%** |
| 11-bit adder circuit compilation | 165.6 ms | 173.5 ms | +4.8% |
| Active counter: 4,000 ticks, no publication | 47.2 ms | 75.1 ms | **+59.1%** |
| Active counter: 4,000 ticks plus full publication | 233.0 ms | 267.1 ms | **+14.7%** |
| Active counter circuit compilation | 272.5 ms | 304.6 ms | **+11.8%** |
| ANPU Rust: 50,000 total ticks, no publication | 389.2 ms | 384.1 ms | -1.3% |
| ANPU LLVM: 50,000 total ticks, no publication | 174.3 ms | 174.0 ms | -0.2% |
| Logic gate Rust: 2,000,000 ticks with input toggles | 340 ms | 352 ms | +3.4% |
| Logic gate LLVM: 2,000,000 ticks with input toggles | 168 ms | 172 ms | +2.6% |

The ZCore slowdown occurs in every paired comparison: Rust tick-time increases range from **48.7% to 93.6%**, LLVM from **14.5% to 51.8%**. The adder increases range from **26.0% to 32.5%**. Active counter tick-plus-publication increases range from **11.0% to 18.2%**. These are larger and more consistent than the logic-gate control differences. Peak working set covers the entire paired Rust/LLVM benchmark process, including compilation and fixture construction; it is not a measurement of steady-state engine memory alone.

ANPU's original 5,000-tick window was noisy: Rust median tick time changed -1.7%, LLVM +18.3%, with one LLVM pair actually faster. The three longer 50,000-total-tick checks did **not** reproduce a tick-time regression. Total ticks must not be read as continuously active CPU work: simply extending this prefix does not scale the measured work proportionally. Consequently ANPU TPS from these windows should not be extrapolated to sustained CPU throughput.

The small counter microbenchmark has only 8–17 ms timed regions and is not used to judge active-counter performance. The active-counter row above comes from `bench_arithmetic`, not that microbenchmark.

## Validation and interpretation

- All CPU screen hashes match between baseline/candidate and Rust/LLVM within each benchmark window. This is output consistency evidence, not a proof of identical internal histories or a replacement for the separate correctness regressions.
- The arithmetic harness initially failed in its default mode: it reads piston payload outputs while redstone-only publication hides those changes. Both variants were rebuilt with `redpiler-full-publication`; all transient sum/carry and interpreter comparisons then passed across 240 repeated adder operations per trial. No assertion was disabled and `MCHPRS_BENCH_ALLOW_DIVERGENCE` was not used.
- Circuit compilation and full reset are outside the CPU tick timer. Arithmetic input setting, preparation, and rearming are outside its evaluation tick-plus-publication timer. Peak memory is sampled through Windows process memory counters.
- The baseline omits required wire-shape behavior. Some extra work is therefore a correctness cost; the measurements do not justify removing notification repetitions or changing scheduling order to recover performance.

Source inspection identifies concrete follow-up candidates, rather than a sampled CPU profile: [`shapes.rs`](../../../crates/core/src/redpiler/sampled/shapes.rs) retains 256-byte state-transition maps for local token arrangements; `Program::start` deep-clones the program, including those maps, and [`source_order.rs`](../../../crates/core/src/redpiler/sampled/passes/source_order.rs) starts sampling runtimes. [`Runtime::notify_shape`](../../../crates/core/src/redpiler/sampled/executor.rs) also performs repeated position-keyed shape-program lookups. Sharing immutable maps and indexing shape programs by existing projection cells are candidates to investigate while retaining every meaningful callback. Their individual share of the measured slowdown has **not** been profiled.

## Artifacts and reproduction

Retained repository evidence: [all 64 successful process runs](evidence/piston-wire-performance.json), [summary](evidence/piston-wire-performance-summary.json), and [source/binary provenance](evidence/piston-wire-performance-provenance.json). Arithmetic assertions and CPU screen-hash comparisons passed. Source hashes were rechecked after measurement.

The E: investigation directory retains the executable binaries, per-run logs/JSON, orchestration scripts, and the baseline native DLL; the candidate DLL remains in the original `piston-wire-shape-20261102` directory. The provenance records source/executable hashes and compiler settings. Existing fixed-name benchmark outputs in the main checkout were restored and verified byte-for-byte against their pre-run backups.

The harnesses are unchanged repository examples:

```powershell
cargo build -p mchprs_core --example bench_llvm_zcore --example bench_llvm_small
cargo build -p mchprs_core --features redpiler-full-publication --example bench_arithmetic

# Run both source variants with their matching native DLL; serialize all timing runs.
bench_llvm_zcore.exe 5000 1
bench_llvm_zcore.exe 5000 1 --anpu
bench_llvm_zcore.exe 50000 1 --anpu
bench_arithmetic.exe <unique-output-dir> 1 4000 240
bench_llvm_small.exe 2000000
bench_llvm_small.exe 2000000 --llvm
```

Remaining gaps: release/LTO builds, Linux, sampled CPU stacks, and full frozen CPU replays were not measured. The present evidence is sufficient to flag regressions in this build configuration; it does not establish production TPS or the exact hot-function breakdown.
