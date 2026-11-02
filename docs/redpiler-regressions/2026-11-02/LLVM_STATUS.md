# LLVM status — semantic 19, 2026-11-02

LLVM matches the sampled Rust backend in the tested scenarios below and is faster in the separate kernel trials. It is **not a generally interpreter-correct or release-ready backend**: the shared compiler/runtime still has the four documented fixture failures and deferred movement-side limitations.

## Implementation

- Shared sampled `Program`, existing serial execution architecture; no interpreter ticks or native world-wire walks were added to runtime execution.
- Torch piston watcher order and empty-piston observer FIFO fixes are implemented in the shared compilation/runtime paths and LLVM where applicable.
- LLVM carries validated wire-side bytes through initial table52, writable state and snapshot version6. Movement-driven wire-shape notification behavior remains deferred in **both** sampled backends.
- The RAM reactive-torch callback-order fix is implemented and tested in both backends against the interpreter. It admits existing notification-only callbacks into the shared source program; it does not add an event engine.
- LLVM API semantic version19 rejects older DLLs. The existing source-hashed cache identity and opt-in behavior remain.

Investigated production revision: `ea297c163a21e33b75c43f271f19c108c99d0112`. The new four-fixture parity regression was added afterward as a test-only diff, preserved in [llvm-short-pulse-regression.patch](evidence/llvm-short-pulse-regression.patch).

## DLL and validation

Tested DLL: `F:/rustrepos/MCHPRS/target/llvm-speed-v19.dll`, 41,472 bytes, semantic19, SHA-256 `0AC9D7C5347ECF4C660F951D419E77141352FE8E25C37DE8A1438F5214FD56DE`. Build: 572.7 ms, Clang/LLD23.1.4, `-O3`, no added LTO. Full tool/source hashes and flags are in [llvm-native-build.json](evidence/llvm-native-build.json). A rebuilt DLL may differ bytewise through build metadata; verify semantic identity and tests as well as the artifact hash.

All DLL tests explicitly selected the DLL with `MCHPRS_LLVM_DLL`; these were executed tests, not skipped validation.

| Check | Result / saved evidence |
| --- | --- |
| ZCore interpreter screen, Rust, full publication | Every Run tick1–5000 matched; [log](evidence/zcore-rust-screen.log) |
| ZCore interpreter screen, LLVM, full publication | Every Run tick1–5000 matched; [log](evidence/zcore-llvm-screen.log) |
| ANPU Rust/LLVM full state/effects/order | 5,000 ticks passed; [log](evidence/llvm-anpu-5000.log) |
| ZCore Rust/LLVM full state/movement/order | 413 preparation +5,000 active ticks passed; [log](evidence/llvm-zcore-5000.log) |
| ANPU reset/handoff | 2,000 compiled ticks +128 interpreted continuation ticks passed between backends; [log](evidence/llvm-anpu-reset.log) |
| Full RAM fixture vs interpreter and between backends | Passed; [log](evidence/ram-interpreter-rust-llvm.log) |
| Four seeded failing fixtures, LLVM vs Rust | All831/813/834/3496 ticks matched projected states, effects, ordered pending and movement; [log](evidence/llvm-four-fixtures-parity.log) |
| Interpreter redstone suite after RAM fix | 143 passed, 4 ignored; [log](evidence/redstone-suite.log) |
| Last `llvm_` filtered suite | 22 passed, 1 failed, 3 ignored; [log](evidence/llvm-focused.log) |
| Last full Redpiler suite | 181 passed, 3 failed, 18 ignored, before the later dedicated parity test; [log](evidence/redpiler-suite.log) |

The LLVM filtered failure is the retained publication/reset fixture's requirement that Rust produce a movement-side change. That prerequisite still fails on Rust, rather than demonstrating a new Rust/LLVM state disagreement. The three full-suite failures are that fixture, piston-landing wire shape, and the aggregate RedstoneDB test containing the four failures in this bundle. These tests were not removed or weakened. Standalone later parity passes do not make the full suite green.

Backend parity does not establish interpreter correctness. Reset parity between two compiled runs is not a proof that either matches an independently interpreted original circuit.

## Separate kernel performance

Three alternating 5,000-active-tick pairs per circuit; investigator builds/replays were paused. Setup, publication/reset and snapshots were outside the timed kernel.

| Circuit/backend | Trial1 TPS | Trial2 TPS | Trial3 TPS | Median TPS |
| --- | ---: | ---: | ---: | ---: |
| ZCore Rust | 2,391.0 | 2,566.5 | 2,502.0 | **2,502.0** |
| ZCore LLVM | 4,555.4 | 4,324.8 | 4,435.8 | **4,435.8** |
| ANPU Rust | 19,901.9 | 20,203.5 | 17,862.7 | **19,901.9** |
| ANPU LLVM | 34,064.2 | 35,304.8 | 32,912.7 | **34,064.2** |

Median-throughput ratios: about1.77× for ZCore, 1.71× for ANPU. All paired final screen hashes matched. Hash equality is benchmark consistency evidence, not a substitute for the full-state checks above. Rust's requested2,600–3,000 TPS range is not demonstrated by this sample set.

ZCore Rust setup ranged15.19–23.96s, LLVM20.52–24.70s. ANPU Rust5.87–6.57s, LLVM6.23–6.88s. These are first/subsequent in-process observations, not controlled cold-cache timings. Full-publication throughput was not benchmarked as a controlled performance comparison.

Raw data: [ZCore JSON](evidence/zcore-kernel-pairs.json), [ZCore log](evidence/zcore-kernel-pairs.log), [ANPU JSON](evidence/anpu-kernel-pairs.json), [ANPU log](evidence/anpu-kernel-pairs.log).

## Reproduction commands

Run from the primary checkout containing the implemented changes. If reproducing the new four-fixture test from clean `ea297c16`, first apply the saved test-only patch. Confirm that one test executes and passes; zero selected tests is not validation.

```powershell
./scripts/build-llvm-native.ps1
$env:MCHPRS_LLVM_DLL = (Resolve-Path target/native_engine.dll).Path
$env:CARGO_TARGET_DIR = 'E:/codex-builds/llvm-validation-target'
cargo test -p mchprs_core --lib llvm_redstonedb_short_pulse_traces_match_rust -- --nocapture
$env:MCHPRS_LLVM_VERIFY_TICKS = '5000'
cargo test -p mchprs_core --lib llvm_anpu_matches_rust_state_and_effects_each_tick -- --ignored --nocapture
cargo test -p mchprs_core --lib llvm_zcore_matches_rust_state_and_motion_each_tick -- --ignored --nocapture
cargo test -p mchprs_core --lib llvm_anpu_reset_and_continuation_match_rust -- --ignored --nocapture
cargo build --profile bench -p mchprs_core --example bench_cpus --example bench_llvm_zcore
$env:MCHPRS_BENCH_PUBLISH_ALL = '1'
& "$env:CARGO_TARGET_DIR/release/examples/bench_cpus.exe" zcore 5000 1 integrated
& "$env:CARGO_TARGET_DIR/release/examples/bench_cpus.exe" zcore 5000 1 integrated --llvm
& "$env:CARGO_TARGET_DIR/release/examples/bench_llvm_zcore.exe" 5000 3
& "$env:CARGO_TARGET_DIR/release/examples/bench_llvm_zcore.exe" 5000 3 --anpu
```

The examples write fixed default JSON names under `target/redpiler-bench-results` regardless of `CARGO_TARGET_DIR`. Copy any existing defaults before rerunning, and save new outputs under new names. Preserve the archived evidence directory. Run timing separately from builds, tests and debugger/replay work.

## Remaining limits

- All four documented interpreter/compiler disagreements remain unfixed; LLVM shares the tested Rust behavior there.
- Movement wire-side history/notifications, broader movement/ANPU correctness and the deferred ANPU tick218 investigation remain unresolved. Piston-dropping-block behavior was explicitly excluded.
- Linux loading/execution was not tested. A prior semantic18 Linux object cross-compile passed; it is not semantic19 runtime validation.
- Original MAIR binary unavailable; comparison uses its decompiler rebuild. MAIR interpreter/load semantics differ in some cases.
- No deployment/cache redesign, added LTO, FPU command support or general event engine was included. The checks do not establish support for arbitrary circuits or all effects/reset contexts.
