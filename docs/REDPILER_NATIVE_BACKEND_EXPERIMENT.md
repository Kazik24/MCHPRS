# Sampled Redpiler → LLVM DLL experiment

## Current correctness work — local routing in Rust and C (2026-11-02)

The compiler now builds local connectivity tables instead of depending on a single-token discovery seed. Small inputs retain dense arrangement tables (4,096 states per group); oversized or overlapping-token inputs retain an immutable routing graph. Dense lowering runs in batches capped at 65,536 source/state slots, with a 64 MiB retained-table estimate. These limits do not bound the entire compiler process or all graph metadata. Large tables remain binary data; no per-occurrence C functions or generated C initializers were added.

The first hybrid Rust ANPU run matched interpreter component states and wire power for 5,000 ticks (`target/anpu-hybrid-shadow2.log`, 52.73 s including setup and comparison; sampled compilation 20.56 s). This comparison does not establish wire-side geometry, all effects, entities or full reset-continuation parity. The independent interpreter matched the frozen reference for its 4,096-tick prefix. These are correctness checks, not paired TPS measurements. The obsolete isolated Cartesian-probe diagnostic was removed after recording its historical measurements below.

The C executor now evaluates the same local routing graph, selects physical token digits, searches dust paths to attenuation 14, and updates arrangement contributions before movement callbacks inside the DLL. Binary auxiliary tables 33–41 contain local records, token axes, ports, wire edges, tagged sources, reader links and token-to-reader dependencies. Table 42 carries comparator modes, static overrides and initial outputs. The ABI descriptor remains version 3; semantic version 10 rejects old DLLs, and snapshot schema 3 includes comparator outputs. Rust reads state at publication/handoff boundaries; it does not execute native ticks. Routing caches and analog input histograms are rebuilt during readback so continued Rust execution uses the live inputs.

Observer fixes distinguish directed shape notifications from undirected dust power callbacks. Unpowered observers ignore the latter; powered observers without pending work can release. Dust watcher discovery uses the interpreter's turbo footprint. Both movement and ordinary wire callbacks preserve that distinction. Tests force the same fixture through dense and routed lowering, and exercise real DLL piston movement, dust paths, source changes, pending ticks and handoff.

Comparator evaluation, analog side-input histograms and scheduled callbacks are implemented in C for static overrides. Dynamic comparator overrides, reactive callbacks and the remaining ANPU component families are still rejected before activation. This is not full LLVM ANPU support. Full-state ANPU acceptance and updated alternating Rust/native TPS trials remain required; the older ZCore throughput figures below are not measurements of this revision.

Build command: `./scripts/build-llvm-native.ps1`, using the existing `target/llvm-23.1.4/bin/clang.exe` and matching `lld-link.exe`. The semantic-10 build took 492.0078 ms and produced a 35,840-byte DLL and 39,178-byte object; shared sources were 34,364 bytes (`engine.c`) plus 30,818 bytes (`native_movement.c`), with a 4,389-byte ABI header. This is one local compiler/link invocation, not a cold/warm trial series or Program conversion time. Exact flags, tool/source hashes and sizes are recorded in `target/native-engine-build.json`.

Validation commands and final results for this revision are recorded below after execution.

## Historical throughput checkpoint — ZCore runs in C

The Windows x64 C executor now admits ZCore12B_FIX and executes its scheduler, wires, torch/observer/repeater callbacks, conditional arrangements, token movement, piston events and motion completion inside the DLL. The host uses `tick` and `read` operations; Rust retains the sampled Program and a publication adapter but never calls its tick runner while LLVM is selected. Full snapshots are currently read at publication boundaries, which adds publication overhead. The implementation is still opt-in with `/rp compile --llvm`.

Correctness: the complete ZCore state was compared with the current sampled Rust runner after 4,509 ticks (413 loading ticks plus 4,096 active ticks). Comparisons include component and projected blocks, contribution slots, arrangement contributions/state, watched-wire strengths, primary/side active input counts, token positions, piston flags, ordered events, scheduled entries/type bindings and active motions/order/deadlines. The run passed. A separate interpreter run matched the final screen after 4,096 active ticks, with 195 screen changes. Small motion tests cover early/normal retraction, redstone-block transport, observer callbacks, publication and reset continuation. The latest loaded-DLL suite has ten passing tests; the large ZCore comparison is explicitly run with `--ignored`.

Test correction: older DLL-dependent tests used a relative DLL path, causing them to skip under Cargo's crate working directory. The loader and tests now resolve the repository DLL path consistently, and the comparisons above were rerun against the actual DLL. Earlier statements that those skipped tests established parity should not be used as evidence.

Performance: seven alternating 10,000-tick pairs used the same fixture, 413 loading ticks, controls, optimized executable and serial semantics. Median Rust kernel throughput was **2,927 TPS** (range 2,362–3,134), versus **5,174 TPS** for C/LLVM (4,969–5,335). The median paired ratio was **1.796×**. Every run produced final-screen SHA-256 `da3eb0df74dc81983b03f811b2bfc64a4e71bc9ed415bf04ab67fd4993746793`. Native runs executed 89,608,494 operation/callback work units including preparation. This measures kernel throughput; per-tick publication and host integration are separate costs.

Setup: the shared C engine built with Clang 23.1.4 `-O3` and lld-link in **408.4 ms**, producing a 27,136-byte DLL from 53,277 bytes of shared C source. ZCore's binary payload was 27,808,808 bytes, with zero generated per-plot C bytes. One admission run took 13.96 s; the interpreter-checked run took 12.92 s. In the longer paired process, median setup was 17.28 s native and 17.85 s Rust; setup ranged upward as repeated fresh graph compilations ran. These are graph/frontend conversion costs plus DLL load/state initialization, separate from C compilation. No graph-specific code expansion or relaxed parallel execution was added.

Reproduction:

```powershell
./scripts/build-llvm-native.ps1
$env:MCHPRS_LLVM_DLL=(Resolve-Path target/native_engine.dll).Path
cargo test -p mchprs_core --lib llvm_ -- --nocapture
cargo test -p mchprs_core --lib llvm_zcore_matches_rust_state_and_motion_each_tick -- --ignored --nocapture
cargo run --profile bench -p mchprs_core --example bench_cpus -- zcore 4096 1 kernel --llvm
cargo run --profile bench -p mchprs_core --example bench_llvm_zcore -- 10000 7
```

The build script records source/tool/DLL SHA-256, exact commands, sizes and elapsed compiler/link time in `target/native-engine-build.json`. Paired results are in `target/redpiler-bench-results/zcore-llvm-paired.json`; state comparison and interpreter benchmark logs were saved under `target/llvm-zcore-shadow.log` and `target/llvm-zcore-first-kernel.log`. ANPU, Chungus and FPU coverage checks and Unix/container deployment are the next requested acceptance work; these figures do not establish their correctness or production portability.

### Broader checks and ANPU baseline investigation

The 512-tick complete-publication ZCore benchmark passed per-tick screen comparison with the interpreter for both backends. With a full state read and publication on every tick, the native candidate measured 103 integrated TPS versus 611 for Rust; this exposes the current full-snapshot publication cost and is not a production performance acceptance. The kernel-only results above exclude that cost. Production reads need to retain dirty-state publication behavior, while verification keeps full snapshots.

Native admission was attempted on all requested builds. ANPU contains comparators, hoppers, note blocks and pressure plates in addition to ZCore's component families; Chungus contains comparators, hoppers and pressure plates. FPU includes comparators and three unknown/command-style components. These were explicitly rejected by the current C admission check. Chungus native admission took 112.99 s, FPU 9.89 s; the active FPU interpreter reference measured 169 TPS over its 10,000-tick input window. No LLVM TPS is assigned to rejected Programs.

The Rust sampled ANPU baseline also diverged from the interpreter: its final screen differed at 4,096 ticks, and its 512-tick run produced no screen changes while the interpreter matched the frozen reference. A first-divergence diagnostic traced the first electrical difference to tick 70: an empty extending piston head failed to notify an adjacent quasi-powered piston until motion completion. A small regression reproduced both extension and tick-74 head-removal/retraction failures before their fixes; immediate head-neighbor callbacks now pass that regression. The same small circuit failed against the old loaded DLL at its first tick, so these notifications were also added to C. The native semantic version is now 8, rejecting stale modules. Clang/link time was 440.7 ms and DLL size 28,160 bytes (`target/native-engine-build.json`). The earlier ZCore throughput figures predate these semantic fixes and require rechecking.

The next electrical difference was tick 278: wire `(99,49,109)` should receive power 5 from the lit torch `(98,48,118)` at attenuation 10 through several opened concrete gates. Interpreter source discovery includes that source; the compiled arrangement only contained nearby sources. `group_probe.rs` discarded source links discovered during joint probes unless the dependency already existed in that probe group. The new regression `joint_movement_discovers_sources_hidden_behind_two_tokens` fails before retaining those links and passes afterward. New dependencies are attributed by restoring individual tokens rather than attributing every probe token indiscriminately. After these changes, `cargo test -p mchprs_core --lib redpiler::sampled::tests:: -- --nocapture` passed 62 tests (one large test ignored), and the actual loaded-DLL `llvm_` suite passed 11 tests (one large test ignored). The full ZCore Rust/native snapshot comparison was then run explicitly and passed all 4,509 ticks in 87.11 s. This rechecks Rust/C parity, not interpreter parity or current TPS.

This correction currently makes ANPU reject explicitly during compilation instead of admitting incomplete tables. It exposes dependencies with eight ternary token axes (6,561 states) at `(84,36,62)` and ten axes (59,049 states) at `(84,37,64)`, exceeding the unchanged 4,096-state limit. Merely raising that limit would multiply compile work and movement tables. ANPU is **not yet supported**, and no current LLVM ANPU TPS is claimed. A MAIR parser/runtime cross-check is in progress to establish whether its representation handles these paths differently and whether its executor agrees with the interpreter.

Current reproduction commands:

```powershell
cargo test -p mchprs_core --lib empty_piston_head_notifies_adjacent_quasi_powered_piston_immediately -- --nocapture
cargo test -p mchprs_core --lib joint_movement_discovers_sources_hidden_behind_two_tokens -- --nocapture
$env:MCHPRS_REDPILER_BOUNDARY_TRACE='1'
$env:MCHPRS_ANPU_VERIFY_TICKS='4096'
cargo test -p mchprs_core --lib anpu_first_native_compiled_divergence -- --ignored --nocapture
./scripts/build-llvm-native.ps1
cargo test -p mchprs_core --lib llvm_ -- --nocapture
```

Diagnostics are in `target/anpu-first-divergence.json`, `target/anpu-arrangement-limit.log`, `target/anpu-joint-regression.log` (before), `target/anpu-joint-regression-fixed.log` (after), and `target/llvm-piston-before.log`. Baseline correctness and a bounded representation for the newly discovered paths take priority over widening C admission.

The current MCHPRS interpreter was checked independently with `cargo test -p mchprs_core --lib anpu_interpreter_prefix_matches_frozen_reference -- --ignored --nocapture`. All saved checkpoints within the prefix and all 14 frozen screen frames through tick 4,096 matched (10.92 s test wall time). This checks the interpreter without compiling a sampled Program; log: `target/anpu-interpreter-oracle.log`.

### MAIR cross-check: parses and compiles ANPU, but does not execute it correctly

The isolated .NET wrapper in `target/anpu-mair-check/Program.cs` used the unmodified MAIR decompiler rebuild at `target/mair-crosscheck/bin/MAIR.Core.dll`; the original shipped binary is unavailable. DLL SHA-256: `1510a2596486a0df240750bc96c7b0f446697fdf9fe6f3039f9b73787c99d5ac`; wrapper SHA-256: `91655e8d8e16cbef8179093a19caca4c38494a513714a2df236e178c4504d5bb`. All 521 schematic palette states round-tripped exactly after canonical property ordering, with zero skipped states. The loader placed 116,374 non-air blocks at the same `(8,8,8)` offset, restored 539 container signals and 815 comparator entities, and used start control `(130,76,64)`. Schematic SHA-256 matches the frozen ANPU reference (`f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec`). Compilation succeeded in 4,292.6 ms: 26,621 nodes, 34,626 links and 34,781 wires. This is compilation success, not execution correctness.

The unmodified MAIR compiled replay first differs from the frozen MCHPRS screen at tick 544 and produces four screen frames through tick 4,096 instead of 14. Its own interpreter removes dust over hoppers at tick 58: the support block `(60,35,15)` is correctly parsed as a hopper, but MAIR's `BlockInfo.SturdyTop` does not classify it as supporting dust, and `MAIR.Core.Redstone/Wire.cs::NeighborChanged` removes the wire. Current MCHPRS wire callbacks retain it. Consequently, this unmodified MAIR interpreter is not an interchangeable oracle for the current MCHPRS semantics.

A separate, explicitly modified diagnostic changes only MAIR's in-memory hopper `SturdyTop` flags (ten states), without editing the reference source or DLL. It then reproduces the same **tick-278** first MAIR interpreter/compiled state mismatch at `(99,49,109)`: interpreter power 5, compiled power 0. The mismatch persists through ticks 279–280. Its compiled screen still diverges at tick 544. The modified MAIR interpreter itself differs from the frozen MCHPRS screen at tick 810, so this override is a focused reproduction, not a new correctness baseline.

Cross-reference against `E:/codex-builds/RedGear-reference-decompiled`: `MAIR.Core.Rsc.Compilation/Compiler.cs::ProbeAllArrangements` (line 1277) passes `probe.Links.Contains` into `Probe`; `FindChangedLinks` (1159–1164) discards new dependencies through that predicate. `BuildWires` (580) retains only discovered sources, and `MAIR.Core.Rsc.Engine/Engine.cs::GetWirePower` (1731) reads those precomputed lists, with no dynamic source discovery. The saved MAIR table for the failing wire has one three-state group, four nearby sources, and no distant torch `(98,48,118)` in any state (`target/anpu-mair-check/wire-table.json`). This corroborates the sampled compiler's joint-source omission rather than an LLVM execution error.

Reproduction with the retained wrapper and local .NET SDK:

```powershell
dotnet run --project target/anpu-mair-check/Check.csproj --configuration Release -- F:/rustrepos/MCHPRS 4096
dotnet run --project target/anpu-mair-check/Check.csproj --configuration Release -- F:/rustrepos/MCHPRS 4096 hopper-support
```

Evidence: `target/anpu-mair-check/run.log`, `result.json` (unmodified), `palette.json` (empty mismatch list), `probe-280.log`, `hopper-support.log`, and `result-4096-hopper-support.json`. Next decision: represent jointly opened dust paths without enumerating the full Cartesian product of all token positions; retaining the incomplete MAIR tables or simply increasing the existing state cap does not establish correct ANPU support.

Deployment requirements now include Unix shared libraries and a dedicated Docker work volume containing the compiler cache. LLVM is to be installed in the image at build time. MROWW must own artifact cleanup and reset its managed cache on process startup, with a process lock and ownership marker preventing deletion of unrelated or active work. This deployment work remains pending while the baseline correctness issue is resolved.

## Goal

### ANPU compiler repair: isolated correctness and setup-cost failures

The repair is not yet an ANPU acceptance. The sampled suite passed 63 tests, and the focused compressed-axis tests (including a real DLL concrete-piston comparison) pass. Electrical digit maps retain physical token cells, motion callbacks and publication; concrete positions without any face-adjacent wire may share the absent electrical digit. Non-wire readers additionally preserve all positions within Chebyshev distance two. Group keys include both token IDs and digit maps; startup, transition deltas, validators, diagnostic readers and native table 27 use mapped digits. Invalid maps are rejected. The per-group limit was experimentally increased from 4,096 to 65,536 to measure whether dense tables remained viable; this is **not a production acceptance or an aggregate memory bound**.

Full-circuit trials did not establish a solution: one attempt rejected an uncompressed non-wire group after 151.47 s; another hit the 300-second compile timeout. Later diagnostics were interrupted while still in joint discovery. The last log is `target/anpu-bounded-shadow4.log`; it contains no completed ANPU compilation or execution result. Candidate electrical aliases and job-local dependency proofs were deduplicated, but the remaining exhaustive state/candidate sweeps still dominate setup. No new TPS is claimed.

After reviewing this with two Terra 6.1 research agents, implementation changes were frozen to isolate the defects:

1. `joint_movement_discovers_sources_without_a_single_token_seed` removes the intermediate source from the existing two-blocker fixture. With both blockers absent, shared interpreter dependency extraction finds the far redstone block at attenuation eight. The compiled group omits it. This ignored diagnostic fails in **0.03 s**, while the original seeded fixture passes in **0.03 s**. Joint jobs are created only from single-token changes, so a completely hidden path has no discovery seed. Merely retaining new links in existing jobs does not fix that case. The fixed five-block candidate-nearness filter is another completeness assumption that must not be treated as a proof of all relevant dependencies.
2. `isolated_anpu_joint_probe_cost` loads ANPU but does not compile it. It queries one real wire target `(84,37,72)` with a controlled prefix of the concrete gates in columns `x=84/86`, plus one fixed candidate gate. It runs one serial baseline/candidate sweep and measures scratch reversion, placement, input extraction and boundary checks separately. It performs no restoration probes, fixed-point rounds or final table construction, so these numbers must not be reported as full compilation time.

| Binary axes | States | Baseline + candidate probes | Sweep wall time |
|---:|---:|---:|---:|
| 2 | 4 | 8 | 0.32 ms |
| 6 | 64 | 128 | 4.0 ms |
| 10 | 1,024 | 2,048 | 83.0 ms |
| 12 | 4,096 | 8,192 | 360.3 ms |
| 14 | 16,384 | 32,768 | 1,538.1 ms |

At fourteen axes, input discovery used 1,330.9 ms (86.5%), reversion 99.3 ms, placement 60.8 ms and boundary checks 34.8 ms. The rest is harness/loop overhead. The existing joint algorithm multiplies the state sweep by the number of distinct candidate states, repeats it across targets and growing dependency groups, and may add own-token restoration probes. Raising the cap or optimizing DLL compilation cannot remove this Cartesian product.

```powershell
cargo test -p mchprs_core --lib joint_movement_discovers_sources_hidden_behind_two_tokens -- --nocapture
cargo test -p mchprs_core --lib joint_movement_discovers_sources_without_a_single_token_seed -- --ignored --nocapture
$env:MCHPRS_PROBE_AXES='14'
cargo test -p mchprs_core --lib isolated_anpu_joint_probe_cost -- --ignored --nocapture
```

Logs/results: `target/anpu-seeded-repro.log`, `target/anpu-no-seed-repro.log`, `target/anpu-isolated-probe-14.log`, `target/anpu-isolated-probe-cost.json`. The no-seed diagnostic intentionally remains failing until discovery is corrected. These focused measurements identify the algorithmic issue without repeatedly compiling all of ANPU.

Next decision: use a conservative local connectivity representation (or an equivalently sparse decision representation) that can discover a path without a single-token seed and avoids a dense global token-state product. Query-guided probing alone can fix seeding but still has an exponential worst case and does not remove the current dense runtime tables. Before changing runtime representation, require tiny exhaustive comparisons against the shared interpreter extractor, including no-seed paths, alternate routes/attenuation and intermediate departures. Callback ordering, source updates and the loaded C executor must be included in the eventual implementation; a Rust-only routing shortcut is not LLVM ANPU support.

Reproduce the previous native engine's performance improvement using the current sampled backend: compile its validated representation into a circuit-specific DLL and execute the circuit inside that module. Target approximately 1.5× kernel throughput as the experimental ambition, with measured correctness and setup costs. This is an opt-in experiment, not a proposed default switch.

The primary deliverable is a complete native tick runner with useful specialization. A DLL that only forwards tick calls into Rust does not satisfy that objective. Early query-only or generic-engine stages are development checkpoints, not completion.

Keep the current serial semantics, including ordered callbacks, scheduled priorities, piston events, motion completion, operation limits, and reset continuation. Do not combine this experiment with the relaxed parallel runtime: that would make it difficult to distinguish code-generation gains from changed simulation work.

## 1. Recovered architecture and results

The implementation is recoverable from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e` (`C inference engine experiment`). Commit `d917e568` removed it. Inspect history without reverting the current backend:

```powershell
git show 576ae313:docs/notes/redpiler-pistons-v4/LLVM_NATIVE_INFERENCE_DEMO.md
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.rs
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine/emit.rs
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.h
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.c
git show 576ae313:crates/core/src/redpiler/backend/graph/native_query.rs
```

### Successful design to carry forward

1. Rust performed normal circuit discovery and lowering.
2. The emitter serialized immutable graph tables and generated graph-specific power/guard functions.
3. Clang compiled C at `-O3` to an x86-64 Windows object; lld linked a DLL.
4. The DLL owned tick scheduling, pending membership, ordered callbacks, wire execution, piston/resource transitions, and deferred effects. Unsupported expression roots used the general C evaluator, with zero Rust graph-evaluator fallback during native inference.
5. Rust loaded the module, supplied independent mutable engine instances, projected state into the host, and implemented verification/handoff.
6. Shared immutable machine code was reused; mutable engine state was never shared between benchmark instances.
7. Sparse dirty projection was required to retain the kernel gain in integrated timings.

The historical Rust/C interface used explicit C records, allocator callbacks, loaded function pointers, ordered future snapshots, dirty-state transfer, and ordered effect queues. Reuse these architectural ideas. Port current sampled semantics rather than resurrecting the old graph/contact/guard machinery.

### Performance evidence

The old experiment eventually succeeded; source size and cold setup were its practical limits. The work log records:

| Experiment | Result | Scope |
| --- | --- | --- |
| Early query-only full sort | Native 90.877 s vs Rust 75.369 s | About 2% native query coverage; not the successful final architecture |
| Final v13 full sort, 10,036 ticks | Rust core 66.327 s vs native 49.040 s | 1.3525× kernel throughput in one paired full-sort sample |
| Same v13 pair with projection | Rust 71.797 s vs native 57.321 s | 1.2526× combined throughput |
| Final v13 repeat, three 1,000-tick pairs | Median Rust core 6.480 s vs native 4.972 s | 1.3032× core and 1.2038× combined throughput; all pairs favor native |
| Earlier specialized v12 repeat, three 1,000-tick pairs | 1.2050× ratio of median core times | Repeatable gain; different pinned stage from v13 |

The v13 work log also records three alternating 1,000-tick repeatability pairs in its “Sparse projection repeatability” section. Preserve these as historical evidence, not timings for the current backend. Complete per-tick state/future/effect verification, frozen RAM, reset, and interpreted continuation passed in the recorded full-sort run.

The user's approximately 1.5× recollection motivates reproducing the experiment. The exact recovered v13 full-sort figure above is 1.3525× core throughput; benchmark the new backend independently rather than assuming either figure transfers unchanged.

## 2. Why the old source became enormous

The raw O0 representation had 129,379 nodes, 522,011 sites, 226,471 routes, 5,812,695 deliveries, 3,758,922 guards, 41,354 wires, 16,561 pistons, and 13,118 resources. One designated-initializer export was **1,074,091,660 bytes**. A later measured raw source was 1,069,701,237 bytes, producing a 244,669,952-byte DLL and taking 131.3 seconds for native setup.

The final specialized v13 representation was smaller but still expensive:

| Artifact/cost | Historical measurement |
| --- | ---: |
| Generated C | 451,136,648 bytes |
| DLL | 118,535,168 bytes |
| Original cold native setup | 501.814 seconds |
| Immutable table initializer text | 350,005,491 bytes, about 77.6% of C source |
| Guard definitions | 385,113 |
| Query definitions | 71,155 |

The final file was about 451 MB decimal, while earlier versions exceeded the reported 500 MB problem by a wide margin. Largest final text tables included 1,162,483 delivery entries consuming 115,345,887 source bytes. Repeated field names inflated C source beyond binary payload size.

A giant query switch also caused an aborted compile after more than ten CPU minutes. The later design used bounded sparse function-pointer dispatch. Literal guard folding and verified code reuse improved practicality, but cold compilation remained substantial.

The new design must bound generated instructions and keep table payloads out of C initializer text. Smaller instruction streams help, but do not by themselves guarantee a small translation unit or short LLVM optimization time.

## 3. Historical sampled Program size census

Existing measurements were read and checked from:

- `target/container-census-20261101/report.txt`
- `target/container-census-20261101/{zcore,fpu,chungus}.json`
- `target/container-census-20261101/row-statistics.csv`
- `target/kernel-plan-trials/smallvec-memory.log`

These are retained-capacity measurements from earlier source snapshots, not a fresh compile of today's worktree. They include nested vector buffers and exclude allocator bookkeeping, map metadata/spare buckets, some deep command data, Arc headers, and temporary compiler allocations. They are neither full process memory nor predicted DLL sizes. Later SmallVec and flattened-plan changes alter the current totals.

### Overall retained vector buffers

| Fixture | Program bytes | Program MiB | Initial Runtime MiB |
| --- | ---: | ---: | ---: |
| ZCore12B_FIX | 131,607,405 | 125.51 | 19.59 |
| FPU_27 | 69,982,424 | 66.74 | 10.09 |
| Chungus | 178,114,019 | 169.86 | 56.25 |

### Logical array lengths

| Program array | ZCore | FPU | Chungus |
| --- | ---: | ---: | ---: |
| Components / `positions` | 87,786 | 46,702 | 246,601 |
| `operations` | 35,578 | 41,839 | 1,269,014 |
| `links` | 79,569 | 46,620 | 466,517 |
| `movement_steps` | 159,417 | 93,742 | 0 |
| `movement_flips` | 166,314 | 96,954 | 0 |
| `movement_wires` | 35,958 | 13,481 | 0 |
| `movement_groups` | 27,019 | 13,880 | 0 |
| `input_arrangements` | 73,869 | 44,453 | 0 |
| `arrangement_links` | 75,448 | 45,749 | 0 |
| Pistons | 16,834 | 8,100 | 0 |
| Tokens | 13,169 | 6,609 | 0 |

### Important ZCore buffers

“Used” below is `len * Rust element size`; “retained” is `capacity * element size`. Pointer-bearing records exclude their nested payload in this table.

| Array | Used MiB | Retained MiB |
| --- | ---: | ---: |
| `operations` | 0.543 | 1.000 |
| `links` | 1.821 | 3.000 |
| `movement_steps` | 4.865 | 8.000 |
| `movement_flips` | 6.344 | 10.000 |
| `output_programs` | 2.679 | 4.019 |
| `arrangement_links` | 4.605 | 7.500 |

The deep arrangement tables are also important: ZCore has 248,841 `input_arrangements.inputs.sources` rows but only 88,645 source entries. That family retains 14,476,536 bytes in container and payload storage. Its parent input-row family records another 7,745,040 container/payload bytes, with overlapping parent/child storage accounting; do not add arbitrary family totals to the already computed Program total. FPU has 152,204 such source rows and 55,370 entries. Empty row headers and excess capacity are material costs, not generated instruction counts.

Recent flattened-plan logging for ZCore additionally records 62,852 `movement_transitions` (2,621,440 retained bytes), 200,700 `movement_ranges` (4,194,304 retained bytes), and 29,937 entries in `movement_cells` (872,208 retained bytes including row storage). These later buffers must be counted in a fresh census before choosing the native encoding.

### Consequence for code generation

ZCore's 35,578 ordinary operations are far fewer than the old million-entry delivery table, but movement is a separate, substantial stream. Chungus has over 1.26 million ordinary operations: generating a distinct C function or long unrolled body for every occurrence can still reproduce source/compile bloat.

Refresh the complete census on the current source before implementation. Report lengths, capacities, actual element sizes, deep payloads, row histograms, and the reachable runtime subset. Generate payload from logical lengths only. Do not serialize Rust allocation slack, Vec/SmallVec headers, duplicate compiler proof tables, or lookup maps that the native tick never uses.

## 4. Proposed pipeline

```text
World snapshot
  -> existing sampled discovery / validation / instruction sharing
  -> native preparation: reachable tables + compact typed records
  -> binary Program blob + bounded specialized C functions
  -> Clang -O3 object(s) + lld -> experiment DLL
  -> native scheduler / signal execution / piston movement
  -> dirty-state and ordered-event export
  -> existing Rust host publication / reset handoff
```

Preserve two native paths within the same DLL:

1. A complete generic native evaluator for the sampled representation, including queues and movement. This is the correctness foundation and handles large/cold sequences.
2. Circuit-specific native functions for selected ordered operation/movement bodies. They remove opcode dispatch and embed validated topology/index constants while using live mutable state.

Measure generic native and specialized native separately against Rust. Both languages already use LLVM; the intended advantage is specialization, compact representation, and removal of dispatch/dependent lookup work. The generic port is useful for validation and fallback inside the DLL, but is not assumed to produce the final performance gain alone.

Do not introduce LLVM library bindings in the first experiment. Reuse the established C → Clang/LLVM → DLL workflow. Consider direct LLVM IR/object emission only if measurements identify the C frontend as a remaining bottleneck after binary table separation.

## 5. Compact Program data and source-size controls

### Native encoding

Build a separate immutable native representation from validated `Program`. Use explicit fixed-width C-compatible records and flat payloads with checked offset/count ranges. Example starting layouts:

| Record | Proposed encoding | Nominal bytes |
| --- | --- | ---: |
| Operation | `u32 operand`, `u8 opcode`, explicit padding | 8 |
| Electrical link | `u32 source`, `u32 target`, `u8 attenuation`, flags/padding | 12 |
| Range | `u32 start`, `u32 count` | 8 |
| Output program | Two ranges | 16 |
| Movement step | `u32 node`, wire range, flags/padding | 16 |
| Arrangement flip | `u32 target`, `u32 source`, `u32 link`, attenuation/source-kind/flags/padding | 16 |

These are proposed ABI records, not Rust struct casts. Validate actual size, alignment, field offsets, integer narrowing, and maximum aggregate offsets. Reject oversized programs before allocating or generating code. Encode enum tags, option sentinels, booleans, and flag masks explicitly.

For scale, the three ZCore streams `operations + movement_steps + movement_flips` would contain 5,496,320 bytes under the proposed 8/16/16-byte records, before ranges, other tables, and alignment. This is a calculated encoding example from the historical counts, not a measured complete native blob.

Flatten nested lists to offsets plus contiguous elements. Share identical immutable sequences where the existing ordering/identity rules permit. Preserve component/link identity even when instruction storage is shared. Retain only runtime-reachable metadata after a field-by-field reader audit. Original token positions used to project vacated geometry are runtime-required metadata, even though named as initial state.

### Separate binary data from executable text

Rust owns an immutable validated blob and passes a versioned table view to the DLL. The blob may be persisted as a binary artifact for inspection/reuse; load it into owned aligned native records with bounds validation. Do not cast unaligned file bytes to structs, or dump pointer-bearing Rust objects.

Generic native loops read this blob. Circuit-specific C contains index constants and selected code, not designated initializers for every Program row. This keeps C frontend work proportional to selected code rather than all graph metadata. The DLL need not embed the entire blob to be circuit-specific.

Start with explicit internal experiment limits: 8 MiB specialized generated C total and 64 KiB per generated body, measured in actual emitted bytes; adjust only through reported experiments. Exceeding a code budget routes that body to the generic native evaluator, without changing semantics or falling back to Rust. Limit table bytes separately through checked allocation budgets and report them. Avoid a giant switch that invites whole-program inlining. Dispatch by validated body ID through a bounded function table and keep the general native evaluator available.

Compile generic runtime code separately from specialized translation units. Share repeated bodies by canonical content and compatible operand bindings. Reuse shared rise/fall instruction ranges already in Program. Do not clone an entire movement body for every joint arrangement if a compact range parameter gives the same ordered work.

## 6. Native mutable state and ABI

Use a versioned `NativeApi` with ABI version, descriptor sizes, semantic-version fingerprint, target/CPU requirements, and create/destroy/tick/use-control/export functions. Explicitly validate the C/Rust layout contract at module load. Rust `Block`, Vec, HashMap, SmallVec, Option, and Rust enum layouts do not cross the ABI directly.

The native state contains compact current block descriptors, comparator levels, contribution arrays/histograms, input caches if retained, token positions/occupancy, joint arrangements, piston flags/motions, phase stamps, scheduler storage, pending counters, dirty cells, and effect buffers. Represent block type/properties in explicit native fields; convert to Rust blocks at host export. Keep enough information to preserve all admitted component behavior and exact reset state.

Start each engine from converted entry state and imported typed scheduled work. Use the existing supported-entry validation, including piston-state restrictions. Keep native buffers persistent across ticks. Use host allocator callbacks or fully native-owned allocation with matching destruction; never free memory through the wrong allocator. Bound queues and propagate allocation, capacity, and semantic-operation failures with component IDs/error codes.

One native tick executes all existing sampled phases, including the two scheduler drains and cursor advancement, piston event FIFO, motion completion, and nested ordered updates. Preserve early-retract, interrupted-motion, joint arrangement, side-input, observer, copper-bulb, note, and command semantics. Initialization, controls, and snapshot APIs must also preserve operation accounting and errors.

Keep the library handle alive until every engine and function pointer using it has been destroyed. Share immutable code/blob ownership across instances while allocating independent mutable state. No module unload or program replacement during an executing call.

## 7. Specialize the measured hot path

The current ZCore profile attributes 65.1% of kernel samples inclusively to `set_token_pos -> run_move_steps`; arrangements/flips account for 24.9%, pre-callback wire refresh 12.4%, with additional ordered and watcher delivery costs.

Prioritize native specialization of movement-plan execution and shared output programs:

1. Select the current movement range from live arrangement state.
2. Emit direct native calls or bounded straight-line code for ordered flips, with constant link/target/attenuation metadata and live source strengths.
3. Preserve input aggregate updates, wire refresh ordering, switched-node deduplication, watchers, callbacks, and operation limits exactly.
4. Specialize constant source kinds only when their invariant is validated. A movable redstone block can have a constant intrinsic strength while its position/connectivity remains dynamic.
5. Retain native generic loops for large ranges and dynamic branches. Record calls and executed operations by specialized/general native path, so cold target counts cannot masquerade as hot coverage.
6. Test compiler optimization levels and bounded inlining only after correctness. Preserve code-size budgets; many functions or repeated full-body inlining can increase instruction-cache and compile costs.

Profile after specialization: identify whether dispatch, indexing, input histograms, or the unchanged semantic work now dominates. Do not assume LLVM will remove callbacks whose ordering remains observable.

## 8. Host integration, flags, and toolchain

Begin with a headless benchmark-only native feature/selection, using explicit compiler/linker paths. Expose `/rp compile --native` after full lifecycle validation. Keep it independent of `--publish-all`, `--publish-redstone`, and IO-only options. Reject combining `--native` and `--parallel` during this experiment. Default execution remains the current Rust runner.

No per-query or per-flip Rust callback during native inference. Host-only effects are queued with their captured payloads and exported in order. Full command support must preserve shared output quotas and command state; early stages may explicitly reject unsupported circuits before activation. FPU census includes command entities, so FPU admission cannot be assumed until those semantics are implemented.

Publish dirty state only at host refresh boundaries or forced synchronization. Avoid copying the entire native state into a shadow Rust Runtime after every tick. Read-only verification can use complete snapshots outside performance timing. Reset exports all blocks/entities, relative ordered scheduled entries, piston events/motions, and queued effects required for interpreted continuation. Keep ownership on export failure.

Compile in the existing isolated worker, with cancellation, stale-snapshot rejection, and one large job per plot. Invoke tools with structured argument arrays, bound diagnostic capture, kill/join task-owned child processes on cancellation, and distinguish missing-tool, emit, compile, link, load, and activation errors. Compilation failure never partially activates the DLL.

The old portable Clang path is historical evidence, not an available-tool guarantee. In the present shell, `clang`/`clang-cl` were not resolved and the discovered `lld-link` belongs to the ST toolchain. Confirm a matching x86-64 Clang/linker installation before implementation benchmarks. Record executable versions, hashes, flags, and target features.

For initial reuse, cache immutable modules only with bounded retention. Key executable code by ABI/semantic version, generic runtime and generated source hashes, compiler/linker versions or binary hashes, flags, target/CPU features, and any embedded program constants. Validate each blob separately against the code contract. Hash initial state only when actually baked into generated code. A path string alone is not toolchain identity. Report cold compilation separately from warm module reuse.

## 9. Implementation sequence and deliverables

1. **Fresh census and feasibility:** enumerate runtime-read fields, classify compiler-only tables, export compact binary descriptors, record source/blob estimates for ZCore/FPU/Chungus. Deliver a size report and descriptor validator before porting execution.
2. **ABI and small full engine:** implement version/layout handshake, lifecycle, blocks, fixed contributions, scheduler, controls, and effects on tiny circuits. Exercise a real compiled/loaded DLL. Zero Rust tick fallback is required even here; unsupported circuits decline explicitly.
3. **Movement parity:** port arrangements, ordered movement plans, piston event/motion machinery, input/wire updates, and operation/error handling. Validate ZCore intermediate state and reset continuation against the Rust sampled backend.
4. **Coverage:** add remaining admitted components, reactive paths, commands/notes, and FPU/Chungus coverage. Record unsupported semantics as a list, not a silent fallback.
5. **Circuit specialization:** emit bounded shared output/movement bodies targeting profile evidence. Measure generic native vs specialized native to isolate the benefit.
6. **Cold/warm measurement:** run paired benchmarks with separate generation, blob conversion, compiler, linker, load, initialization, kernel, projection, and total request latency. Verify cancellation and module/state isolation.
7. **Optional server integration:** wire the compilation flag, diagnostics, publication, save/reset, toolchain error messages, and module lifetime only after headless acceptance.

Likely new code: `redpiler/sampled/native/` containing ABI descriptors, compact exporter, source generator, module loader/process runner, native runtime C/header, and verifier. Adapt `redpiler/mod.rs`, options, compiler preparation, sampled runtime initialization/publication adapters, benchmark examples, and targeted tests. Add dynamic loading only under the experiment feature if no suitable existing dependency is present; reuse installed hashing/process facilities where available.

## 10. Correctness and performance protocol

Freeze a baseline from the current retained optimized Rust runner, including movement layout, SmallVec, diagnostic-load, wire-cache, and publication changes actually present in that build. Record source revision/worktree patch, fixture hash/import preparation, toolchain, affinity, and flags. Compare the same admitted representation and input sequence.

### Correctness outside timing

- Run a native/Rust shadow after every control and logical tick. Compare blocks/projection, comparator outputs, contribution slots and input aggregates, token and arrangement state, piston flags, active motion identities/deadlines/order, relative ordered scheduler entries/type bindings, pending membership, and ordered notes/commands.
- Compare dirty export and full publication under both publication feature configurations and supported override flags. Check that sparse export does not lose hidden IO-only work.
- Test reset in active motion and stale scheduled entries, then compare at least 32 interpreted continuation ticks. Include final native-interpreter screens plus intermediate observations.
- Check zero-delay scheduling, priority/FIFO, duplicate entries, operation/queue exhaustion, source-state changes during callbacks, equal-strength competing inputs, analog inputs, interrupted motion, two-token groups, notes, command quotas, and independent states sharing one DLL.
- Run specialized/general native comparison separately where practical. Validate emitted ranges and source-generation limits on small malformed/corrupted descriptor fixtures.

### Timings

Use at least seven alternating baseline/candidate pairs on ZCore and FPU, including established 10,000-tick runs; include Chungus as a large non-piston stress case. Add tiny circuits and idle/active checks. Verify actual native dispatch/work counters and meaningful output so idle engines cannot appear fast. Measure correctness in separate runs; no per-tick timers or shadow snapshots inside kernel-only loops.

Report kernel TPS/time, integrated tick plus publication time at the same cadence, total CPU usage, generated source bytes, emitted function counts, largest body, blob bytes, DLL `.text`/read-only data/file size, cold wall time, compiler peak memory where measurable, and warm setup. Kernel improvement and reduced publication cost must remain distinguishable.

For amortization, compute `extra cold setup / measured per-tick time saving`; report required ticks or workload episodes only when the denominator is positive. Do not omit setup because timing instances reused a module. Treat approximately 1.5× as the goal, and retain conclusions based on measured paired results and variance.

## Decision

Proceed as an experiment to reproduce the demonstrated old native-engine gain on the sampled IR. The new representation offers a credible reduction in generated-code volume, especially for piston CPUs, and exposes a narrower hot path for specialization. Binary table separation and code budgets make compile size controlled by design. Whether native specialization reaches the desired gain, and whether cold setup is economical, remain measurements to obtain.

## Implementation log

### 2026-11-01 — ABI spike and `--llvm` opt-in

Changes in this worktree:

- Added `crates/core/src/redpiler/sampled/native.rs` with ABI version 1 descriptors and a checked little-endian binary payload for the operation and ordinary-link tables. Operations are fixed 8-byte records; electrical links are fixed 12-byte records. Checked conversions reject offsets/counts/operands that do not fit `u32`. Validation checks the magic/version/header, exact table ranges, operation tags and operand domains, link component IDs, and reserved bytes. The payload contains logical rows only and does not serialize Rust structs or allocation capacity.
- Added `abi.h` with matching C records and compile-time size/offset assertions: 16-byte API descriptor and 52-byte Program descriptor. The Rust test checks alignment/field offsets and rejects a corrupted table offset.
- Added `/rp compile --llvm` parsing and completion. The command rejects it before resetting the current compiler; `Compiler::compile` also rejects direct API calls before compilation/activation because no native executor is wired yet. Default sampled compilation is unchanged. There is no `--parallel` flag in this command path to combine with it.

Commands and results:

```powershell
cargo test -p mchprs_core --lib redpiler::sampled::native::tests::descriptor_layout_and_bounds_are_stable
```

Passed: focused descriptor layout/bounds test, direct API rejection/no-activation test, and command completion/parser test. The operation/link serializer is exercised by the census diagnostic after normal Program compilation; it is not used for execution, so this is not Rust/native state or execution parity.

Toolchain discovery with `Get-Command clang,clang-cl,lld-link,clang++,gcc,cl` found only `lld-link.exe` from `C:\ST\STM32CubeCLT_1.20.0\st-arm-clang\bin`; no matching x86-64 Clang was resolved, so no DLL was emitted or loaded. The historical recovered implementation was inspected at `576ae313` (`native_engine.rs`, `emit.rs`, `native_engine.h`, `native_engine.c`, and `LLVM_NATIVE_INFERENCE_DEMO.md`); its table-plus-engine split is the model, but its graph ABI is not reused.


### Fresh sampled census, current worktree

The reusable runner is `crates/core/examples/census_sampled.rs`; it loads ZCore through the existing CPU fixture helper, FPU from `FPU_27.schem`, and Chungus through its existing saved-plot helper. It calls the normal sampled compile path with `MCHPRS_REDPILER_PHASE_TIMINGS=1`. Exact command:

```powershell
$env:MCHPRS_REDPILER_PHASE_TIMINGS='1'
cargo run -p mchprs_core --example census_sampled *> target/native-backend-census-20261101.log
```

All three current compiles completed. The sampled compiler's phase logger recorded 19.697 s for ZCore12B_FIX, 16.898 s for FPU_27, and 137.980 s for Chungus; Chungus spent 122.285 s in source transition sampling. Cargo's separate incremental build took 1m39s. These are census/setup compile times, not native compilation or inference benchmarks. Fresh counts and lower-bound logical binary sizes for the currently encoded operation/link subset:

| Fixture | Components | Operations | Links | ABI blob bytes (header + 8-byte operations + 12-byte links) |
| --- | ---: | ---: | ---: | ---: |
| ZCore12B_FIX | 87,786 | 35,578 | 79,569 | 1,239,468 |
| FPU_27 | 46,702 | 41,839 | 46,620 | 894,168 |
| Chungus | 246,601 | 1,269,014 | 466,517 | 15,750,332 |

These are current logical counts; the binary-size column is **not** a complete native Program size. The emitted subset omits component/state descriptors, scheduler state, arrangements, watched/reactive wires and all piston movement tables. For scale, current retained buffers reported by the existing logger include ZCore: operations 1,048,576 bytes, links 3,145,728, movement steps 8,388,608, movement transitions 2,621,440, movement ranges 4,194,304 and movement cells 872,208; FPU: 1,048,576, 1,572,864, 4,194,304, 1,310,720, 2,097,152 and 431,792 bytes respectively. Chungus retained 33,554,432 bytes for operations and 12,582,912 for links, with no movement rows. The logged sum of selected flat and row buffers was 68,925,497 / 36,394,228 / 167,932,705 bytes for ZCore/FPU/Chungus; it excludes unlisted nested payloads and HashMap/allocator overhead, so it must not be compared as a full Program total with the historical census table.

No C was generated (0 source bytes), and no blob was written to disk; the in-memory logical subset sizes are in the table. No DLL was emitted (0 DLL bytes), and native cold compile time is unavailable because there is no matching compiler.

The runtime reader audit used direct `Runtime` references in `executor.rs`, `input.rs`, `scheduler.rs`, and `commands.rs`. Runtime execution reads component positions/IDs, output programs and operations, electrical and arrangement links, incoming/outgoing indexes, source and watcher maps, reactive tables, flattened movement plans, piston/token/projection bindings, cell geometry/occupancy indexes, and repeater/comparator records. The `component_indices` and `geometry_indices` maps are also runtime-reachable; command handling reads the former, while geometry queries read the latter. Candidate compiler-only data after flattening includes the captured `geometry` list, `movement_groups`, `token_movement_groups`, `token_updates`, `token_watchers`, and `token_sources`; these can be dropped only after checking construction, validation, reset, and host-export paths. The new descriptor is only the operations plus ordinary links slice.

The Chungus sampled compile took 137.980 s and peaked at roughly 4.75 GB working set during compile-time source sampling. This is sampled compiler setup cost, not native cold compile time. The complete row histograms and deep retained-byte breakdown have not yet been refreshed for the current worktree; the selected logger fields above are not a substitute. Generic native scheduling/execution, movement, reset continuation, DLL load, and paired Rust/native performance remain unimplemented and unmeasured. No executor or performance gain is claimed at this checkpoint.

### 2026-11-01 — generic DLL wired into Compiler; active small-circuit comparison

- `Compiler` now owns either the existing sampled Rust runtime or an exclusive native DLL runtime. `--llvm` compiles the same sampled `Program`, encodes its operation/link/component/source rows, dynamically loads `target/native_engine.dll` (override with `MCHPRS_LLVM_DLL`), and routes controls, ticks, publication, and reset through the DLL. The DLL receives tables as a compact binary blob; Clang compiles the shared C executor, not per-plot C source. The fixture's binary Program blob is 112 bytes and generated per-plot C source is 0 bytes.
- The supported subset is checked before activation. It permits static lever/button/redstone-block sources feeding lamps, and only `Contribution`/`UpdateLamp` operations. Pre-existing scheduled ticks, command effects, arrangements/watched wires, and piston state/movement are explicitly rejected. Publication exports component states to the world; reset exports ordered pending events as relative `TickEntry`s so the Rust scheduler can continue. A parity test runs a button/lamp pulse, publishes each tick, resets with the button release pending, and compares the scheduler and 15 interpreted continuation ticks.
- Toolchain: `clang version 23.1.4` targeting `x86_64-pc-windows-msvc`, with `lld-link 23.1.4`; no system x64 Clang was available. `target/llvm-23.1.4` is a local ignored toolchain install. Rebuild the shared DLL from the repository root with PowerShell:

```powershell
target/llvm-23.1.4/bin/clang.exe --target=x86_64-pc-windows-msvc -std=c11 -O3 -ffreestanding -fno-builtin -Werror -I crates/core/src/redpiler/sampled/native -c crates/core/src/redpiler/sampled/native/engine.c -o target/native_engine.obj
target/llvm-23.1.4/bin/lld-link.exe /dll /noentry /nodefaultlib /machine:x64 /out:target/native_engine.dll target/native_engine.obj
```

- Measured C build at this benchmark checkpoint: cold Clang+link 142.4 ms; warm 130.6 ms. Shared engine source was 15,858 bytes, object 5,033 bytes, DLL 5,120 bytes. These wall times are local process measurements and exclude downloading/extracting LLVM. The ABI-v1 API descriptor is 16 bytes, Program descriptor 52 bytes, component record 32 bytes, and pending-tick record 12 bytes.
- Active kernel benchmark command: `cargo run --profile bench -p mchprs_core --example bench_llvm_small -- 100000`. It alternates seven Rust/native pairs. Each measured tick toggles a lever into a lamp and advances the engine; correctness is checked separately by the reset-continuation parity test. The 100,000-iteration native runs each reported 225,575 executed operation/callback work units (including 256 warmup iterations), confirming active scheduled work. No publication is timed.
- Raw Rust TPS by pair: 20.242, 18.569, 20.226, 19.869, 18.471, 20.544, 20.027 million. Raw LLVM TPS: 12.654, 12.323, 13.483, 12.836, 12.842, 12.970, 12.772 million. Median Rust: 20.027 million TPS; median LLVM: 12.836 million TPS, or 64.1% of Rust throughput (35.9% slower). The median paired LLVM/Rust ratio is 0.646. This is a small active control/scheduling kernel, not a CPU schematic result and not an integrated publication benchmark.
- Runtime setup includes sampled Program compilation, DLL load, binary encoding, and native state creation. In the same seven pairs, Rust setup was 75.9, 69.5, 65.2, 61.4, 78.6, 65.5, 63.4 ms; native setup was 71.6, 76.8, 65.4, 62.8, 75.0, 65.0, 64.3 ms. Medians were 65.5 ms Rust and 65.4 ms native. An optimized Cargo example rebuild took 45.35 s during this run; this is Rust build time, distinct from plot setup and the 153 ms shared C DLL build.
- Validation command: `cargo test -p mchprs_core --lib redpiler::tests::llvm_dll_executes_button_lamp_and_hands_off_reset_tick -- --nocapture`. This exercises actual DLL control/tick/publication, pending-event export, reset, and interpreted continuation. The test uses the locally built target DLL and skips when no DLL is present. Work counter is diagnostic only and can be printed with `MCHPRS_LLVM_DIAGNOSTICS=1`.
- Actual target-plot admission command: `cargo run --profile bench -p mchprs_core --example check_llvm_zcore`. ZCore sampled compilation took 12,615.6 ms in that optimized run, then failed admission with `LLVM currently supports only static lever/button-to-lamp circuits`; `Compiler::is_active()` was false. The earlier Rust kernel result for the same ZCore fixture was 3,242.3 TPS (4,096 ticks, one final-reset-match run); it is not a paired trial with LLVM because ZCore cannot execute in this backend.
- Decision: the generic executor is correct for this narrow circuit but is slower than sampled Rust even before publication. Do not enable it for the named benchmark plots or infer a gain. The next stage remains the plan's census-guided movement implementation: widen explicit support only after adding piston-state/scheduler parity and movement/handoff validation, then profile and specialize bounded hot paths. ZCore/FPU/Chungus TPS with LLVM remains unavailable because each is rejected before native activation.

### ZCore admission work remaining

The recorded ZCore census contains 16,834 piston actors, 13,169 movable tokens, 75,448 conditional arrangement links, 43,454 wire components, 966 repeaters, and 17,657 watched-wire strength slots. The present DLL has no representation for these records. Its tick also lacks the sampled runner's piston-event FIFO and motion phases. Admitting this Program based only on its operation/link tables would omit movement and produce incorrect results.

The next implementation must serialize the piston bindings, token cells, arrangement links and flattened movement transitions/ranges, plus watched-wire and repeater records, using bounded binary tables. The native state then needs typed scheduled entries, piston-event ordering, motions and occupancy, arrangement contributions and input aggregates. Port `run_piston_events`, `set_token_pos`/`run_move_steps`, and `tick_motions` in the sampled runner's phase order, with publication and reset exporting live motion and pending work. Keep ZCore rejected until those paths are exercised against the Rust runner at intermediate ticks and across reset; the existing small-circuit TPS does not establish ZCore performance.

The native tick no longer clears the operation counter before scheduled work: control operations since the previous tick now count toward the same limit, matching the sampled runner's accounting boundary. The DLL was rebuilt with Clang, and `cargo test -p mchprs_core --lib llvm_ -- --nocapture` passed the button/reset comparison and a test that exhausts the budget with controls before a scheduled callback. This fixes one invariant in the existing subset; it does not add support for ZCore or the remaining Program tables.

### Pending tick import and read API

The DLL interface now names its snapshot operations `ne_read_state` and `ne_read_pending`; `ne_tick` advances the engine. Rust reads state only for publication or reset and applies it to the host world. The ABI descriptor remains version 1 and the semantic version is now 3; the loader rejects older DLLs. `ne_import_tick` imports up to 64 initial typed button/lamp ticks, including duplicates and their priority/order. Delays must fit the sampled scheduler's 32 slots, zero-delay entries must use normal priority, and passive, untyped, stale, or unsupported component ticks are rejected before activation. Other component types and piston handoff remain unsupported.

`cargo test -p mchprs_core --lib llvm_ -- --nocapture` passed four tests, including a powered button with a pre-existing release, per-tick comparison with interpreted execution, reset continuation, priority/FIFO duplicates, and rejection of an untyped entry. Rebuilding with the Clang/linker commands above took 160.7 ms on this run; the shared C source was 16,449 bytes, the object 5,305 bytes and the DLL 5,632 bytes. This build time excludes toolchain acquisition. The earlier TPS figures were obtained with the preceding DLL version and are not measurements of this version. No ZCore native throughput or movement parity is claimed.

### C scheduler port

The native scheduler now follows the sampled Rust scheduler's 32 delay buckets and four FIFO priorities. A free list reuses event slots, and per-component pending counters replace membership scans. Enqueue and pending checks take constant work; draining a callback checks at most four priorities, independent of circuit component count. `ne_read_pending` walks the queues in deadline/priority/FIFO order in O(128 + pending entries) work instead of repeatedly scanning the entire allocation. Event slots shrink from 32 to 8 bytes, with an additional four-byte pending counter per component and 1,024 bytes of queue heads/tails. The DLL semantic version is now 4, and `tick`/`read` remain the host boundary.

The DLL rebuilt successfully with Clang `-O3 -Werror`, and the four existing LLVM tests passed with the new queue implementation. Added a 257-tick repeated button/lamp comparison to exercise ring wraparound and free-slot reuse, followed by pending-state comparison and 32 interpreted continuation ticks. This scheduler port is groundwork for large graphs; piston, arrangement, wire and repeater execution remain unported, and ZCore remains rejected.

### Repeater callbacks in C

Ported static repeater updates and scheduled callbacks directly from the sampled runner: all four delays, fixed locking, diode-output priority, rear-input transitions, and the rescheduled falling edge of short pulses. Repeaters can feed other repeaters or lamps through ordinary links. Side-lock inputs, movement and watched-wire dependencies remain rejected. Component record kind 4 carries repeater powered state plus delay/fixed-lock/diode flags in the two formerly reserved bytes; the descriptor layout stays unchanged and the DLL semantic version is 5. Native redstone publication now includes repeaters when IO-only publication is disabled.

Corrected an existing exporter bug: Rust stores output ranges in falling/rising order, while the C record stores rising/falling order. The encoder now writes the correct order. The repeater comparison also exercises intentionally asymmetric output ranges so identical edge bodies cannot hide a reversal.

`cargo test -p mchprs_core --lib llvm_ -- --nocapture` passed six tests. The focused repeater test subsequently passed all 32 combinations of delay, fixed locking, diode priority and symmetric/asymmetric output programs, comparing component states and typed pending scheduler entries after each of 96 ticks. The Clang/link build took 213.7 ms; C source 17,761 bytes, object 6,733 bytes, DLL 7,168 bytes. These are shared-executor build figures, not ZCore compilation or throughput. ZCore still requires native wire, arrangement and piston paths; it remains rejected before activation. No new TPS result is claimed.
