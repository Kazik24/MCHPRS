# Benchmark and profiling report — 2026-10-31

## Profile summary

I captured Redpiler compile-phase timings for FPU with randomized inputs, ZCore12B_FIX, and ANPU. Windows CPU stack sampling was blocked: `wpr -start CPU -filemode` returned `0xc5585011` (“Failed to enable the policy to profile system performance”) because this session is not elevated. No ETL or sampled call-stack profile was produced. The available profiles below are compiler phase timings plus each harness's tick/publication timings; they do not identify runtime hot functions.

| Circuit | Profiled workload | Redpiler phase profile | Admission result |
|---|---|---|---|
| FPU | Trigger normalized off; opcode `010`; four seeded random raw operand masks, each held for 128 ticks (512 ticks total). Seed `0x4d4350485253`. | Scratch snapshot 56 ms; piston actor/token discovery 123 ms; component scan 7 ms; total 206 ms. | Rejected at `(147,8,52)`: unsupported component, loaded as `Unknown { id: 13700 }`. Native outputs changed across five raw words. |
| ZCore12B_FIX | 512 native ticks after CPU preparation; three pinned profile runs. | Median total 800 ms (797–802 ms); median scratch snapshot 85 ms, actor/token discovery 574 ms, component scan 121 ms. | Rejected at `(164,52,170)`: unsupported component, loaded as `Unknown { id: 20899 }`. |
| ANPU | 5,000 native ticks per run; five pinned profile runs. | Median total 549 ms (502–811 ms); median scratch snapshot 47 ms, actor/token discovery 119 ms, component scan 80 ms, electrical dependency discovery 281 ms. | Rejected at `(129,76,62)`: unsupported watched-wire source dependency at a stone pressure plate. |

FPU input masks were `0x2dcf00637`, `0x28c788bfb`, `0x28c65e42c`, and `0x21549a4e4`. These are raw switch states; the operand bit order and numerical interpretation are not certified. The native output varied, but there is no arithmetic oracle for this fixture.

Profile logs and machine-readable reports remain under `target/` in the original benchmark output folder:

- `profiles/fpu-compile-profile.log` and `fpu.json`
- `profiles/zcore-compile-profile.log` and `zcore-production.json`
- `pinned/anpu.stderr.log` and `anpu.json`

The circuit import and Redpiler compile timings are harness measurements. The separate Rust optimized build took 65 seconds on the first compile in this session; later incremental example builds took about 31–46 seconds. Those are build-system times, not per-circuit compile times.

## Divergences found

- **COUNTER:** Native matched the frozen reference. At tick 4, memory bit 0 is `M` (a moving piston) in native/reference output, while Redpiler reports `0` (stationary extended piston). The harness also logs a repeated source-probe transition difference at `(47,38,42)`. This points to a mismatch in the compiled timing/state for piston movement; the exact lowering path still needs isolation.
- **ADDER11BIT:** The first recorded mismatch is case 1 (`1023+1`), tick 1, `sum_payload`: Redpiler returns `0`, expected `1024`. The compile run logs repeated source-probe transition differences at `(54,31,z)` for `z=46,50,...,82`. Unstable source transition/order inference is a plausible cause, but the specific missing output path is not isolated.
- **CPU_BUBBLESORT:** All three samples first diverge at tick 86, RAM word 49: native returns `None`, Redpiler returns `14`. The RAM reader returns `None` when that slot is not a piston block; this is consistent with a piston movement transient, though the exact block at the divergent tick was not captured. Many sampled source probes also reported repeated-transition differences.
- **ANPU:** Redpiler declines the circuit at the pressure-plate watched-wire source dependency. The native replay assertions pass; compiled TPS is unavailable.
- **ZCore12B_FIX:** Redpiler declines an unknown block (`id 20899`) during component scanning. Compiled TPS is unavailable.
- **FPU:** Redpiler declines an unknown block (`id 13700`) during component scanning. The random-input native window runs with trigger off and produces changing output words; compiled TPS is unavailable.

The repeated source-probe warnings in ADDER, BubbleSort, and COUNTER are concrete compiler diagnostics that align with the parity failures. They suggest the compiler is observing different transition frames for the same source during ordering discovery. This remains a likely explanation, not a proven single root cause.

## TPS readings (exploratory)

The user noted the host may be unstable. The ANPU samples confirm substantial drift, so these readings are diagnostic observations, not an accepted new baseline. Earlier COUNTER/ADDER/BubbleSort runs were unpinned; the requested profile runs used logical CPU 2 affinity. Do not compare these values as if they came from one controlled session.

| Circuit | Native TPS | Redpiler TPS | Redpiler compile time | Scope / status |
|---|---:|---:|---:|---|
| ANPU | 752 median; 509–873 across 5 samples | N/A | 0.63 s median wall; 0.55 s median phase total | 5,000 ticks; native frozen checks pass; compiler rejects. |
| COUNTER | 2,929 median; 2,806–2,959 | 69,347 median tick+flush | 1.556 s median | 600 ticks; Redpiler speed is not correctness-valid due to tick-4 moving-piston divergence. |
| ADDER11BIT | 3,282 median; 3,247–3,386 | 52,998 median tick+flush | 0.642 s median | 576 active ticks; Redpiler speed is not correctness-valid due to the first output mismatch. |
| CPU_BUBBLESORT | 124.2 median; 121.3–124.3 | 1,416 median tick+redstone-only publication | 14.45 s median | 512 ticks; all samples diverge at tick 86. |
| ZCore12B_FIX | 75.9 median; 66.4–79.4 | N/A | 0.80 s median phase total; 1.10 s median wrapper wall | 512 ticks; compiler rejects the unknown block. |
| FPU | 135.2 (one profiled sample) | N/A | 0.206 s phase total; 0.249 s wrapper wall | 512 ticks including random input changes; compiler rejects the unknown block. |

TPS is headless simulation throughput without clients or network traffic. COUNTER and ADDER tick+flush values include `try_flush`; BubbleSort publication is explicitly redstone-only. FPU includes switch updates in its native window, so its TPS is not comparable to the CPU tick-only figures.

## Reproduction details

- Source state: the first runs started with HEAD `c7eefc55608dbc9c72f0640c114284da29ca46e3` and a dirty worktree; those changes were committed as `e6f4eac24c0bdf861cbedb786a0b1133fb32b9ea` during the session. The later profile runs used that committed source. Five newer uncommitted Redpiler edits (last modified after 21:32) were not included.
- Host: AMD Ryzen 9 5950X (16 cores / 32 logical processors), Windows 11 Home, `x86_64-pc-windows-msvc`, Rust 1.98.1 / LLVM 22.1.8.
- Optimized runs used Cargo's `bench` profile. The native and Redpiler harnesses used frozen schematic hashes; raw JSON outputs remain under `target/`.
- `MCHPRS_REDPILER_PHASE_TIMINGS=1` enables the compile-phase profile. `MCHPRS_BENCH_ALLOW_DIVERGENCE=1` was used only to let diagnostic runs finish and collect repeated samples; without it, the existing fail-fast assertions remain active.

The FPU runner now resets the fixture's saved trigger/opcode state and generates seeded random inputs. The arithmetic and CPU example runners can continue diagnostic sampling after a divergence when the environment flag is set. These changes do not repair the observed compiler mismatches.

## What remains unverified

No sampled CPU call stacks were captured because Windows denied WPR system-profile access. No clean-tree rebuild or full test suite was run. The TPS samples are unstable and partly use different affinity settings, so they should not be adopted as the new baseline without a controlled rerun.
