# LLVM Redpiler backend experiment

### 2026-11-02 — semantic-19 LLVM correctness and separate speed validation

After the user reprioritized a correct and fast LLVM runtime, the current implementation was rebuilt with `./scripts/build-llvm-native.ps1` and saved separately as `target/llvm-speed-v19.dll`. The build took 572.7 ms and produced 41,472 bytes, SHA-256 `0AC9D7C5347ECF4C660F951D419E77141352FE8E25C37DE8A1438F5214FD56DE` (`target/llvm-speed-v19-dll-build.log`). No additional runtime optimization or algorithm was introduced for this timing pass. HEAD had independently advanced to `ea297c16`, containing the previously validated RAM/shared-Program changes.

With `MCHPRS_LLVM_DLL` explicitly selecting this DLL, `cargo test -p mchprs_core --lib llvm_` ran **22 passed, one failed, three ignored** (`target/llvm-speed-v19-focused.log`). The remaining failure is the retained publication fixture's assertion that Rust must produce a movement-driven side change; movement shape notifications are deferred in both sampled backends. It is not a newly observed Rust/LLVM disagreement. Full state, movement, effects, scheduler order, RAM and reset parity checks for semantic 19 are recorded below; they are distinct from interpreter correctness.

`cargo build --profile bench -p mchprs_core --example bench_llvm_zcore` completed before copying its executable to `target/llvm-speed-v19-bench.exe` (`target/llvm-speed-v19-bench-build.log`). Separate kernel-only commands:

```powershell
$env:MCHPRS_LLVM_DLL = (Resolve-Path target/llvm-speed-v19.dll).Path
target/llvm-speed-v19-bench.exe 5000 3
target/llvm-speed-v19-bench.exe 5000 3 --anpu
```

All investigator builds and fixture replays were paused before timing began; they resumed afterward. Each command alternates Rust/LLVM order between pairs and measures 5,000 active ticks per sample, with setup, publication/reset and snapshots outside the kernel interval. Existing default JSON was preserved under `target/redpiler-bench-results/llvm-speed-v19-prior-*`; new raw logs are `target/llvm-speed-v19-{zcore,anpu}-pairs.log`, and JSON is `target/redpiler-bench-results/llvm-speed-v19-{zcore,anpu}-pairs.json`.

| Circuit/backend | Trial 1 TPS | Trial 2 TPS | Trial 3 TPS | Median TPS |
| --- | ---: | ---: | ---: | ---: |
| ZCore Rust | 2,391.0 | 2,566.5 | 2,502.0 | **2,502.0** |
| ZCore LLVM | 4,555.4 | 4,324.8 | 4,435.8 | **4,435.8** |
| ANPU Rust | 19,901.9 | 20,203.5 | 17,862.7 | **19,901.9** |
| ANPU LLVM | 34,064.2 | 35,304.8 | 32,912.7 | **34,064.2** |

Median-throughput ratios are approximately 1.77 for ZCore and 1.71 for ANPU. Every paired final screen matched; ZCore hash is `563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b`, ANPU `f0a6cab3b43c6986b0af8431bf0f74e03c40465c414f4d80e6b55e078baf9a83`. Matching final screens here is a benchmark consistency check; the independent 5,000-tick full-state backend comparisons and interpreter screen checks below supply stronger correctness evidence. These are kernel measurements, not controlled full-publication throughput. Rust's 2,600–3,000 target is still not established by this sample set.

ZCore Rust setup ranged 15.19–23.96 s, LLVM 20.52–24.70 s. ANPU Rust setup ranged 5.87–6.57 s, LLVM 6.23–6.88 s. These are in-process first/subsequent observations, not controlled cold-cache figures. DLL build time is reported separately above.

An isolated action-39 dependency build exhausted F: before the measurements. The agent's attempt to remove its generated build directory recursively was rejected by automatic review without a specific reason. The primary agent verified and removed only its known generated `libmchprs_core-e8539a986b63f695.rlib` file, freeing approximately 297 MB without deleting a directory, source, fixture or evidence. Subsequent investigator builds were redirected to E: (ample free space), outside the primary source checkout. No failed or overlapping run is used in the performance table.

Four other fixture failures remain under separate Luna xhigh investigations, with exact seeded short-pulse sequences preserved. A further agent is checking those same sequences against the available decompiled MAIR rebuild and its own interpreter. The MAIR original shipped binary is unavailable; no MAIR correctness result is claimed yet. Pending questions include callback order, wire refresh coverage and stale captured wire power; no broad fix is justified by warnings alone.

#### MAIR replay of the four current fixture failures

The user explicitly requested checking whether MAIR is also broken on these cases. A Luna xhigh agent built an isolated wrapper under `E:/codex-builds/mchprs-mair-redstonedb-20261102/`, referencing the existing `target/mair-crosscheck/bin/MAIR.Core.dll` without modifying it. Assembly version is 1.0.0.0; SHA-256 `1510a2596486a0df240750bc96c7b0f446697fdf9fe6f3039f9b73787c99d5ac`. This is the existing rebuild from `E:/codex-builds/RedGear-reference-decompiled`; the original shipped binary is unavailable.

Exact action lists were exported using the locked Rust `rand 0.10.3` `StdRng` and the current test's original seed/ranges, not .NET Random. Schematics were not cropped; no settling tick was inserted before input. MAIR accepted all schematic states (Skipped=0). Comparisons cover every compiled node and wire after every wait tick, exact block state for non-wires and power for wires. Wire sides and a separate all-block tick-0 projection scan were not checked. The MAIR loader directly loads a plot while MCHPRS uses clipboard paste, so reference/load differences remain possible.

| Fixture | MAIR interpreter/compiled result | Relation to current MCHPRS failure |
| --- | --- | --- |
| Eight-bit carry-cancel adder, `8111b000090d8f0b3e1370448bcb64a3` | No disagreement across 66 actions / 831 ticks (176 nodes, 614 wires). At action 39 / tick 5 target `(14,9,8)` is **0 in both**. | MCHPRS interpreter is **1**, Rust **0**. This is not the same internal MAIR compiler mismatch; the reference execution differs. MAIR parity does not establish which reference is correct. |
| 8-bit Carry Cancel Adder, `2cdc24e68c78d7f0cdce7a141648385a` | First disagreement action 43 / tick 1, `(11,15,9)`: interpreter **14**, compiled **0** (111 nodes, 196 wires). | Same action, tick, position and power direction as MCHPRS. |
| 8-bit Adder, `2735cc14b1051cab4197df60265878b2` | First disagreement action 63 / tick 1, `(11,17,14)`: interpreter **13**, compiled **0** (174 nodes, 425 wires). | Same as MCHPRS. |
| Four In A Row Detector, `b6a6e870229ace7eb2afd8cb387a514e` | First disagreement action 1 / tick 2, rail `(12,9,z10..13)`: interpreter **0**, compiled **1–4** (335 nodes, 2,103 wires). Post-use and tick 1 match. | Same as MCHPRS. |

The first three fixtures have 17 levers; Four In A Row has 84. Final results are `<fixture-id>.final.json`, exact actions `<fixture-id>.actions.json` under the E: wrapper directory. The first adder's source/pending checkpoint is `8111b000090d8f0b3e1370448bcb64a3.source-result.json`: before action 39 at total tick 694, both MAIR engines show target power 1, comparator sources `(14,9,9)` powered and `(13,9,9)` unpowered; both target powers become 0 on local tick 5. No new MAIR source or DLL patch was made. Its previously documented hopper-support semantics difference remains another reason not to use it as a general oracle.

The MCHPRS-specific investigations continue separately. Four In A Row has a proven tick-2 trigger: torch `(10,9,23)` changes off and native handling reaches wire `(10,9,22)`; subsequent native turbo work clears the zero-effective-input captured rail while the compiled source has no refresh dependency for those far wires. A fixed ordered graph plan keyed to the actual changed-wire callback is a candidate, not a validated implementation. In the second carry-cancel adder, exact baseline/current replay has already found an earlier ordered-pending difference at action 18; investigation is tracing source-input filtering before attributing the later action-43 visible failure. No additional production fixes for these four cases are claimed.

#### Dedicated LLVM/Rust parity for all four short-pulse traces

A retained regression `llvm_redstonedb_short_pulse_traces_match_rust` now reuses the existing `compare_execution` helper for all four complete fixture traces. It uses the focused test's original lever ordering, seed and random range types (including i32 wait generation), and asserts the recorded trace lengths: 831, 813, 834 and 3,496 ticks. It checks projected block states, effects, ordered backend pending entries and movement after every tick. It deliberately does not replace the failing interpreter comparison or relax any assertion there.

With `CARGO_TARGET_DIR=E:/codex-builds/mchprs-redstonedb-investigations-20261102/cargo-target` and `MCHPRS_LLVM_DLL` explicitly selecting `target/llvm-speed-v19.dll`, `cargo test -p mchprs_core --lib llvm_redstonedb_short_pulse_traces_match_rust -- --nocapture` passed in 1.86 s. Log: `E:/codex-builds/mchprs-redstonedb-investigations-20261102/llvm-four-fixtures-parity.log`. These known failing interpreter cases are therefore also directly tested for backend parity. No additional LLVM-specific mismatch was observed; this is not a claim of interpreter correctness for those circuits.

Further isolated baseline/current investigations establish that the first carry-cancel failure also predates the RAM repair. Source comparator `(10,10,11)` feeds target comparator `(13,9,9)` on rear and side paths with attenuation 4 and 3. Its output falls 7→0 at action 39/tick 3; native callbacks expose rear/side on/on → on/off → off/off. The intermediate rear3/side0 schedules the target for tick5. Sampled execution updates the side then batches the ordinary rear contribution before `UpdateComparator`, so it misses that transient pulse, schedules on tick4 for tick6, and retains wire power0 when native has1 at tick5. Target13 has no rear/far override. Logs: `E:/codex-builds/mchprs-redstonedb-investigations-20261102/baseline-action39.log` and `candidate-action39-side.log`. MAIR's own interpreter does not produce that pulse at the same checkpoint; it cannot arbitrate which interpreter matches Java semantics here. A callback between the side and rear updates is a narrow candidate to validate, not an implemented fix.

For the second carry-cancel fixture, both baseline and current first reverse a comparator/torch scheduled FIFO at action18. The native false-edge callback reaches comparator `(10,24,18)` with `input_on=(true,true)` before torch `(13,24,16)`; source-order filtering drops that comparator callback because the source edge is false, and fallback places it after the torch. At action43 there is also a repeater delay mismatch at `(10,15,10)` (native1 versus sampled2), preceding the wire mismatch. The action18 defect is proven; its causal link to the later action43 state mismatch is unverified. An isolated source-only callback-filter prototype was saved but not built or merged. Logs/snapshots: `target/action43-investigation-20261102/`.

For the 8-bit adder, exact baseline/current replay also matches: first observed pending-membership difference at action3/tick4, with a native comparator tick at `(12,14,13)` absent in Rust; first checked block-state difference remains action63/tick1 at `(11,17,14)`. This predates the RAM repair. The connection between these two events is under investigation; compilation's repeated-edge warning alone is not a causal explanation.

The 8-bit adder's final isolated trace confirms that the compiled comparator callback already exists and executes at action3/tick4: the target is nonreactive with three incoming links, stored/calculated output0, rear12/side13, and powered false. It therefore schedules nothing, while the interpreter schedules a tick. The interpreter's block-entity output at that exact callback was not captured, so the scheduling cause is unresolved; adding broad source-order edges is not justified. The action63 source's first/third probe frames have identical blocks and all45 callback events, but different pending work. Immediately after input there is no projected block mismatch; tick1 drops the compiled contribution from comparator `(15,17,15)` to the failing wire from9 to0. The relationship to the earlier missing tick is unproven. Logs: `E:/codex-builds/mchprs-redstonedb-investigations-20261102/action63-baseline-traced.log` and `action63-current-causal.log`. All four requested investigations and the MAIR comparison have reported; no further production change was made for these failures.

### 2026-11-02 — three-defect goal

The user narrowed the active goal to torch piston watcher order, empty-piston observer FIFO, and LLVM mutable wire-side storage/export. Movement wire-shape notification fixes, source-transition lowering, the ANPU tick-218 investigation and piston dropping-block behavior are deferred. The independently expanded RedstoneDB corpus is preserved. HEAD advanced independently from `c01ee103` to `6ef1b165` during the work, including the selected code changes; no reset or commit was performed by this agent.

Changes under validation:

* Torch watcher recipients now use the existing interpreter notification enumerator **at compilation only**. Execution retains the compiled component IDs. `cargo test -p mchprs_core --lib torch_piston_watchers_follow_interpreter_notification_order -- --nocapture` passed (`target/three-defects-torch-focused.log`). After `cargo build --profile bench -p mchprs_core --example bench_cpus --example bench_llvm_zcore` completed, its executables were copied to `target/three-defects-torch-fresh-{cpus,kernel}.exe`. With `MCHPRS_BENCH_PUBLISH_ALL=1`, `target/three-defects-torch-fresh-cpus.exe zcore 5000 1 integrated` passed the full per-tick screen check (`target/three-defects-torch-fresh-screen.log`, `target/redpiler-bench-results/three-defects-torch-fresh-screen.json`). An earlier copy without `fresh` was made before Cargo completed; its screen and kernel logs are invalid as candidate evidence and its kernel timing also overlapped other work. They are retained, not used for conclusions.
* Empty-piston extension now delivers its existing base source notification after its head/home notifications in both Rust and LLVM. No notification, wire traversal or queue has been added. `cargo test -p mchprs_core --lib empty_piston -- --nocapture` passed all five selected tests, including the retained Rust FIFO regression, quasi-power behavior and a new LLVM/interpreter test exercising four settled extension/retraction phases (`target/three-defects-observer-focused.log`).
* LLVM table 52 now contains validated initial wire-side encodings; the DLL stores these in a writable byte per component and exports them through snapshot version 6. Rust projection reads the exported sides instead of reconstructing them from initial blocks. Both validators reject reserved side digits, excess bits and nonzero sides on non-wire components. Semantic identity is **18**, intentionally distinct from both checkpoint 12 and historical experimental 13–17. Existing cache identity includes the changed C source; cache ownership and architecture are unchanged. No movement shape callback or side-transition algorithm is added here. `cargo test -p mchprs_core --lib llvm_wire_sides_round_trip_and_reject_invalid_encoding -- --nocapture` passes, including a projection whose initial sides differ from the DLL and corruption of each packed direction and upper bits (`target/three-defects-sides-focused.log`).

`./scripts/build-llvm-native.ps1` rebuilt the final DLL in 517.6 ms, 41,472 bytes (`target/three-defects-observer-dll.log`). All DLL-selected commands explicitly set `MCHPRS_LLVM_DLL=(Resolve-Path target/native_engine.dll).Path`. The first side-state-only LLVM run passed 19 tests, failed the deferred movement publication/reset fixture, and ignored three (`target/three-defects-sides-llvm.log`).

`cargo test -p mchprs_core --lib redpiler::` then reported **179 passed, three failed, 17 ignored** (`target/three-defects-suite.log`). The failures are the retained piston landing wire-shape and movement-driven LLVM publication/reset tests, plus the independently added RedstoneDB **8-bit Ram** fixture. The latter expects an unlit west-facing wall torch at `(11,9,13)` after lever `(10,8,13)`, tick 2; compiled execution leaves it lit. The fixture NBT contains no piston or observer palette entry, so the selected Rust watcher/movement fixes do not affect its execution path; the test uses Rust, not LLVM. It is retained and not reported as passing. The user requested a separate Luna xhigh investigation of this defect; heavy reproduction is delayed until throughput measurement finishes.

`cargo test -p mchprs_core --lib redstone::` passed **143**, ignored four (`target/three-defects-redstone.log`). Final benchmarks and broader Rust/LLVM comparisons are pending below; none of these focused results establishes movement wire-shape correctness.

The combined candidate's `target/three-defects-final-cpus.exe zcore 5000 1 integrated` passed with `MCHPRS_BENCH_PUBLISH_ALL=1`: 244 interpreter screen changes, no per-tick divergence (`target/three-defects-final-screen.log`, `target/redpiler-bench-results/three-defects-final-screen.json`). Both named final executables were copied only after the final bench build completed (`target/three-defects-final-build.log`).

Three separate 5,000-tick Rust comparisons ran in baseline/torch-only/combined order for trials 1 and 3 and reversed order for trial 2. Each invocation was `<saved-executable> 5000 1 --rust-only`, with no tests or agent reproduction overlapping. The baseline was `target/anpu-resume-20261102-baseline-kernel.exe`, torch-only `target/three-defects-torch-fresh-kernel.exe`, and combined `target/three-defects-final-kernel.exe`. Logs: `target/three-defects-kernel-{1,2,3}-{baseline,torch,final}.log`; matching JSON files under `target/redpiler-bench-results/`. Rust TPS:

| Variant | Trial 1 | Trial 2 | Trial 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Saved baseline | 1,696.5 | 2,643.1 | 2,448.0 | 2,448.0 |
| Torch-only | 1,020.6 | 2,346.3 | 2,187.6 | 2,187.6 |
| Combined | 1,765.0 | 2,079.8 | 2,149.3 | 2,079.8 |

All nine final screen hashes matched `563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b`. The combined median is approximately 15% below the baseline median; the median paired ratio is approximately 0.878, with a large spread. The cause is not isolated, and the target is not achieved. The user subsequently explicitly accepted deferring investigation of a 15–20% drop and prioritized correctness. Further performance work is deferred accordingly; no broad optimization or notification rollback is justified by these measurements alone.

With the rebuilt semantic-18 DLL selected, `MCHPRS_LLVM_VERIFY_TICKS=5000; cargo test -p mchprs_core --lib llvm_anpu_matches_rust_state_and_effects_each_tick -- --ignored --nocapture` passed (`target/three-defects-anpu-parity.log`, 105.96 s). This compares the two sampled backends, including full wire states, ordered pending work and movement; it is not interpreter correctness. ZCore parity and reset-continuation validation follow below.

`MCHPRS_LLVM_VERIFY_TICKS=4096; cargo test -p mchprs_core --lib llvm_zcore_matches_rust_state_and_motion_each_tick -- --ignored --nocapture` passed after the 413-tick loading preparation (`target/three-defects-zcore-parity.log`, 141.68 s). `cargo test -p mchprs_core --lib llvm_anpu_reset_and_continuation_match_rust -- --ignored --nocapture` passed after 2,000 compiled ANPU ticks and 128 interpreted continuation ticks (`target/three-defects-anpu-reset.log`, 37.81 s). These validate tested backend parity, not the deferred interpreter FIFO or movement-side issues. Cross-compiling `engine.c` with the same Clang and `--target=x86_64-unknown-linux-gnu -std=c11 -O3 -ffreestanding -fno-builtin -Werror` succeeded (`target/three-defects-linux-engine.o`, `target/three-defects-linux-compile.log`); Linux loading/execution was not tested.

#### RAM release blocker: valid replay diagnosis

The user subsequently made the 8-bit Ram mismatch a release blocker and asked for the smallest architecture-fitting solution. A requested `gpt-6-luna`/`xhigh` agent reproduced it in detached worktree `target/ram-torch-investigation` without changing the primary checkout. In the original fixture and lever sequence `(10,8,9)`, `(11,8,11)`, then `(10,8,13)`, the interpreter schedules upper torch `(11,9,13)` before lower torch `(12,8,13)`. Interpreter callbacks are upper, upper, lower; pending membership coalesces the repeated upper callback. Rust schedules lower before upper. At tick 2 the interpreter upper torch is unlit but compiled upper torch remains lit, because its delayed recheck sees the lower torch already switched off.

The failing lever's powered output program is only `[Contribution(2851), UpdateTorch(7)]`, feeding the lower torch. The upper torch has no electrical dependency on that lever; it is an initially reactive neighbor, handled by the later `update_reactive` tail. This is a callback-order compilation defect, not a scheduler FIFO defect or missing torch tick. Existing source-order sampling runs before reactive-neighbor discovery, so it cannot order this callback with the lower torch's callback. Quiet probe input filtering also needs to distinguish a callback from an electrical contribution: a callback remains meaningful when the source does not change that target's input.

Candidate under isolated validation: discover the existing reactive neighbors before source-order sampling, admit reactive torch callbacks into the existing per-source target list without inventing electrical links, retain their observed notification order independently of the source power bit, and execute the resulting explicit torch operation in place in both Rust and LLVM. Keep pending membership, priorities, fallback handling and operation limits. No staged source lowering, context variants, runtime world walks or scheduler sorting are proposed. The primary checkout now contains a small two-torch/support FIFO regression under validation; the original full fixture is still required to accept a fix.

The isolated candidate passed the entire original 8-bit Ram fixture and then stopped at Four In A Row Detector. An isolated unchanged-baseline replay of Four In A Row produced the same wire mismatch, establishing that this later failure predates the candidate. Main implementation is narrower than the prototype's input filter: only a callback-only target, encoded as exactly `[UpdateTorch(_)]` with no contribution operation, bypasses the source-input power filter. Existing electrically linked targets retain their prior filtering. Existing reactive-neighbor discovery now precedes source-order sampling; no new pass or runtime machinery is introduced. Explicit torch updates execute at their sampled position in Rust and LLVM, and the existing reactive fallback remains intact. LLVM semantic identity is **19**; table 52 and snapshot version 6 are unchanged from the side-state fix.

Before the runtime change, `cargo test -p mchprs_core --lib reactive_torch_callbacks_preserve_source_fifo -- --nocapture` failed immediately after input with `[lower, upper]` versus `[upper, lower]` (`target/ram-fifo-small-before.log`). After the change, both Rust and freshly rebuilt DLL variants of that regression passed through four input phases with block-state and ordered pending comparisons (`target/ram-fifo-small-after.log`). `./scripts/build-llvm-native.ps1` built the semantic-19 DLL in 563.5 ms, 41,472 bytes (`target/ram-fifo-dll-v19.log`). The new `llvm_redstonedb_ram_matches_rust_and_interpreter` test passed the original full RAM lever sequence, checking all projected blocks against the interpreter and LLVM against Rust, including ordered backend pending work (`target/ram-fifo-full-ram.log`, 0.90 s).

This is an actual component output defect: at tick 2 the unlit interpreter torch emits no power, while the lit compiled torch can emit power. No manually mapped RAM input/output port oracle or proof of changed stored/readout bits is claimed. Full block-state replay demonstrates the fix for the tested sequence, rather than just matching an exported pending list.

The expanded main RedstoneDB test independently acquired seeded short-pulse actions and collection of all fixture failures. The next full suite reported **181 passed, three failed, 18 ignored** (`target/ram-fifo-redpiler-suite.log`), before addition of the dedicated full-RAM test. The remaining top-level failures were the two deferred movement-side tests and the RedstoneDB aggregate. The aggregate now lists four other fixtures: Eight-bit carry-cancel adder (8111b000090d8f0b3e1370448bcb64a3), 8-bit Carry Cancel Adder (2cdc24e68c78d7f0cdce7a141648385a), 8-bit Adder (2735cc14b1051cab4197df60265878b2), and Four In A Row Detector (b6a6e870229ace7eb2afd8cb387a514e). RAM no longer fails, including the expanded actions. Only Four In A Row has an independently established baseline comparison here; the three short-pulse adder failures have not been attributed by this work. None were removed, skipped or weakened by this agent.

`cargo build --profile bench -p mchprs_core --example bench_cpus --example bench_llvm_zcore` completed before copying `target/ram-fifo-v19-{cpus,kernel}.exe`. With full publication, the post-RAM Rust ZCore screen comparison passed all 5,000 Run ticks (`target/ram-fifo-zcore-rust-screen.log`, `target/redpiler-bench-results/ram-fifo-zcore-rust-screen.json`). LLVM screen, large backend parity and reset checks are being repeated for semantic 19. Performance investigation remains deferred at the user's request; the semantic-18 measurements above must not be represented as semantic-19 kernel measurements.

Semantic-19 acceptance checks then completed successfully: the LLVM full-publication interpreter screen also matched all 5,000 Run ticks (`target/ram-fifo-zcore-llvm-screen.log`, `target/redpiler-bench-results/ram-fifo-zcore-llvm-screen.json`). With `MCHPRS_LLVM_DLL` explicitly selecting the fresh DLL and `MCHPRS_LLVM_VERIFY_TICKS=5000`, both `llvm_anpu_matches_rust_state_and_effects_each_tick` (67.89 s) and `llvm_zcore_matches_rust_state_and_motion_each_tick` (124.38 s, preparation plus 5,000 active ticks) passed (`target/ram-fifo-{anpu,zcore}-parity.log`). `llvm_anpu_reset_and_continuation_match_rust` passed again (16.94 s, `target/ram-fifo-anpu-reset.log`). Interpreter redstone tests passed 143 with four ignored after the RAM change (`target/ram-fifo-redstone.log`).

The original three selected fixes, plus the RAM callback-order repair, are complete within that scope. This is not a general interpreter-correctness or release-readiness claim. Movement-side regressions, the deferred ANPU tick-218 question, four other failing corpus fixtures, and Linux runtime validation remain unresolved. The user has requested separate Luna xhigh investigations of all four current corpus failures; no fix for those is claimed here.

### 2026-11-02 — resumed defect and architecture review

The resumed checkout was clean at `846996e4`. No applicable `AGENTS.md` was found in the checkout or its parent directories. The user excludes piston dropping-block semantics and asks for defects, an architecture review, and the smallest graph-bounded fix before implementation. Production sources have not been changed during this review. RedstoneDB files changed independently during the review and are being preserved.

Fresh reproduction commands (PowerShell; each command's output redirected to its separately named log):

```powershell
./scripts/build-llvm-native.ps1
$env:MCHPRS_LLVM_DLL = (Resolve-Path target/native_engine.dll).Path
cargo test -p mchprs_core --lib redpiler::
cargo test -p mchprs_core --lib redstonedb_levers_match_interpreter
cargo build --profile bench -p mchprs_core --example bench_cpus --example bench_llvm_zcore
$env:MCHPRS_BENCH_PUBLISH_ALL = '1'
target/anpu-resume-20261102-baseline-cpus.exe zcore 5000 1 integrated
target/anpu-resume-20261102-baseline-kernel.exe 5000 3
```

The two named executables are copies of the freshly built examples. Existing default benchmark JSON files were copied to `target/redpiler-bench-results/anpu-resume-20261102-prior-*` before running. New screen evidence is `target/anpu-resume-20261102-baseline-screen.log` and `target/redpiler-bench-results/anpu-resume-20261102-baseline-screen.json`. The full-publication screen comparison passed through 5,000 Run ticks. Integrated timing is not a kernel performance measurement. Kernel trials run separately in alternating Rust/LLVM order; their log is `target/anpu-resume-20261102-baseline-kernel.log`.

The fresh DLL built in 520.9 ms, 40,960 bytes. The DLL-selected Redpiler suite reproduced **176 passed, five failed, 17 ignored**; DLL execution tests ran successfully, including movement/reset, scheduling, and controls. Logs: `target/anpu-resume-20261102-{dll-build,suite-baseline,redstonedb,build-benches}.log`. The separate RedstoneDB test passed before the independently arriving fixture edits.

Defects and architectural findings:

* Torch watcher order: `compiler::update_footprint` uses West-first generic neighbors and a two-hop footprint; the interpreter torch path uses `surrounding_notifications` with direct/vertical-diagonal callbacks in its own order. The retained fixture proves East-before-West. Correct the torch-specific compiled recipient order, without changing the global update order or the scheduler.
* Empty piston observer FIFO: extension currently notifies base source watchers before head shape observers. The interpreter's movement phase delivers destination/head shape notifications before the base notification. Correct the phase placement of existing notifications, preserving power callback placement and avoiding double delivery. This is not evidence of a scheduler FIFO implementation defect.
* Wire sides: `notify_shape` handles observers only; `finish_token_part` restores a token using movement power steps without the directional wire-side callback. Existing runtime `Block` values already store full Rust wire sides. Extend existing movement plans with fixed wire component IDs/directions and fixed local geometry references; apply the historical side transition to stored sides. Do not normalize from final geometry, enumerate joint wire-history states, or traverse world wires at runtime. Departure, extension, retraction and interruption need separate validation.
* LLVM: semantic-12 state/snapshot tables preserve wire power but lack mutable side state. After Rust validation, serialize the same movement instructions and initial sides, retain sides in DLL state, and export them through the existing snapshot. Validate indices/encodings/ranges and advance semantic identity. The ABI test and publication/reset test are two manifestations of this missing behavior.
* Source-order limitation: `source_order.rs` deduplicates target callbacks, logs repeated-transition differences, and skips captured-frame replay for appended output sources. This is a known acceptance gap and a plausible area for the deferred ANPU tick-218 investigation, not a demonstrated explanation for ZCore tick 1,520. Do not revive staged source lowering or context variants on that basis.

The shared `Program` already supplies flattened source operations, movement ranges, arrangement flips, projection cell indices, and watcher lists. Rust and LLVM use serial priority/FIFO scheduling. Retain these structures. Arrangement lowering already caps states at 4,096, temporary slots at 65,536, and estimated dense tables at 768 MiB; these are not a total compile-time or process-memory guarantee. Existing fallback routing searches the compiled wire graph with operation charging and a distance bound; it does not run interpreter wire propagation. This review proposes no additional runtime graph search or interpreter call.

The best candidate is an extension of existing indexed movement instructions, with compile-time recipient selection and constant-size local geometry reads only for an affected wire. Changed-side callbacks must be compiled with their original directions, order and meaningful duplicates and executed through existing component operations. This is not yet an implemented or validated fix. Additional work must be measured: no architecture argument alone guarantees 2,600–3,000 TPS. The old experimental screen divergence at Run tick 1,520 remains unexplained; the backup must not be reapplied wholesale.

The three alternating 5,000-tick baseline pairs completed: Rust **2,304.0 / 2,388.3 / 2,377.8 TPS**, median **2,377.8**; LLVM **2,517.5 / 3,936.1 / 3,856.8 TPS**, median **3,856.8**. All six final screen hashes were `563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b`. Rust setup was 15.24 / 23.45 / 24.01 s; LLVM 22.18 / 28.43 / 24.67 s. This fresh baseline does not reproduce the historical single 2,716.4 Rust sample and does not meet the target. No performance cause was isolated. Raw results: `target/redpiler-bench-results/anpu-resume-20261102-baseline-kernel.json`. The separate full-publication interpreter comparison recorded 244 screen changes and no first difference. No production algorithm was changed or accepted in this review.

The backend implementation now lives in `crates/core/src/redpiler/sampled/llvm/`, including `llvm_movement.c`; Rust refers to `LlvmRuntime` and `LlvmProgram`. The existing C ABI exports, `MCHPRS_LLVM_DLL` override, development DLL filename, cache ownership marker, directory and lock names remain compatible with existing installations. The historical commands and filenames recorded below are retained as they were executed. `./scripts/build-llvm-native.ps1` builds the renamed sources; Clang/link completed in approximately 549 ms on this machine (`target/redpiler-llvm-rename-cbuild.log`).

LLVM DLL LTO has not been benchmarked. The cached build uses Clang `-O3` without `-flto`; `llvm_movement.c` is included into `engine.c`, so these executor sources already form one C translation unit. Cargo's release `lto = "fat"` applies to Rust, not this separately built DLL. The Program is currently binary data, so DLL LTO alone does not specialize for a particular graph. After correctness is established, compare DLL builds with and without LTO using separate correctness checks and alternating active-work throughput trials; record cold compile time, DLL size and warm setup separately.

## Rust routing performance investigation

The user subsequently accepted approximately 2,600 ZCore TPS as a working result and directed attention to correctness and stability. The earlier 3,000 TPS references below describe the target at the time of those attempts; further speed tuning is no longer the immediate gate. Validate Rust on the major builds before porting the confirmed behavior to LLVM.

An independent architecture review recommended retaining the shared sampled Program and bounded dense/routed design. It found the no-reader routing shortcut preserves the zero-work case, and the unchanged-contribution shortcut preserves callbacks and operation charges because those remain in the caller. The budget changes which representation is chosen, and routed searches themselves consume the operation budget; selection therefore affects work accounting and needs explicit limit coverage. The memory estimate excludes scratch, graph, runtime, allocator overhead and concurrent plots and is not a process memory cap. The memory census now reports the arrangement/movement container and deep vector capacities; the estimator test includes shared axes and compressed physical-to-electrical mappings, and a routed-search test checks limit exhaustion and restart. `cargo test -p mchprs_core --lib redpiler::` passed 174 tests, with 14 ignored (`target/redpiler-llvm-audited-suite.log`). Large-build effects and reset continuation remain separate acceptance work.

The 64 MiB dense-table budget rejected 54,352 ZCore readers; 22 others exceeded the state bound. A candidate estimator counts actual source/attenuation changes between movement states instead of assuming every source changes on every transition. This admitted more dense readers: the routed count fell from 54,374 to 50,405. A single instrumented 413-preparation/1,000-active-tick run measured 364.1 TPS, 16.83 s admission, 4,902,234 routed reader searches and 50,489,041 charged wire visits (`target/zcore-rust-dense-budget-profile.log`), compared with 289.2 TPS and 61,023,797 visits in the earlier instrumented run. This is an unpaired exploratory comparison, not a validated performance result. The budget is an estimate of retained logical tables, not a process memory cap. The Rust Redpiler suite passed 156 tests, 12 ignored (`target/redpiler-exact-budget-final-suite.log`).

An experimental per-movement-update root cache preserved the recorded screen hash and charged work, but reduced throughput to 213.5 TPS (`target/zcore-rust-batch-profile.log`); it was removed. A separate candidate removed movement axes when every corresponding input row was identical; it reduced routed readers only to 50,308 and measured 292.0 TPS (`target/zcore-rust-inert-axes-profile.log`), so it was also removed. A diagnostic 512 MiB estimated table budget reduced routed readers to 11,892 and charged wire visits to 5,403,570; it measured 1,734.4 TPS, 15.36 s admission and a 1,474,371,584-byte sampled peak process working set (`target/zcore-rust-large-budget-stderr.log`). Its screen hash matched the smaller-budget run. These are single exploratory runs, and the larger budget is not a production decision. Large-build/interpreter parity remains to be checked. Do not port the Rust changes to LLVM or claim the 3,000 TPS target until these checks pass.

Attempt details and decisions:

| Attempt | Result and reason for decision |
| --- | --- |
| More accurate dense-table estimate at 64 MiB | 54,374 to 50,405 routed readers and 289.2 to 364.1 exploratory TPS. Kept as a candidate, but it leaves most searches and misses the target. |
| Cache each wire root during one movement update | 213.5 TPS. It adds hash lookups and result cloning to every update; their individual costs were not profiled, so the precise cause of the regression is not established. Removed. |
| Remove input axes with identical rows | 50,308 routed readers and 292.0 TPS. It admitted only 97 more readers than the estimate-only candidate and showed no throughput improvement in this single run; the reason for its measured slowdown was not isolated. Removed. |
| 512 MiB estimated dense budget | 1,734.4 TPS, still below target, with a sampled 1.37 GiB working set. The larger budget cuts searches but is not yet justified for production; memory and correctness need broader validation. |
| 1 GiB estimated dense budget | All but 22 readers fit; estimated tables used 622,041,741 bytes. The 1,000-tick run measured 2,281.1 TPS, 15.45 s admission and 1,488,478,208 bytes sampled peak working set (`target/zcore-rust-gigabudget-stderr.log`). A separate 5,000-tick run measured 2,466.3 TPS, 15.10 s admission, and the same final screen hash as the original 5,000-tick Rust run (`target/zcore-rust-gigabudget-5000.log`, `target/zcore-paired-semantic12.log`). This establishes that routing explains much of the regression, but nearly eliminating it still misses 3,000 TPS and the larger memory budget has not been accepted for production. The source was restored to 64 MiB after the experiment. |
| Sampling optimized bench binary | A 20,000-tick `samply record` completed (`target/zcore-rust-gigabudget-profile.json.gz`), but the bench profile's stripped symbols resolve the hot frames as unknown, so this capture cannot identify the remaining bottleneck. Rebuild with the existing `profiling` profile and repeat. |
| Symbolized profiling build | An already completed 20,000-tick capture with the 1 GiB diagnostic budget attributed 3,363 of 8,500 samples in an approximate active-kernel window to `set_token_pos` (including inlined code), 738 to `InputState::change`, and 662 to `routed_values` (`target/zcore-rust-symbol-profile-summary.json`). The time window is approximate, not an independently timed microbenchmark. The user subsequently requested no further Samply use; future investigation will use existing counters and benchmarks. |
| Skip routing for tokens with no routed readers, 768 MiB estimated budget | Three alternating 5,000-tick pairs: before 2,044.8 / 2,039.6 / 2,217.0 TPS; after 2,591.0 / 2,540.5 / 2,532.5 TPS. Medians are 2,044.8 and 2,540.5 TPS; median paired ratio is approximately 1.246. Setup was 14.47–14.91 s before and 14.85–14.91 s after, excluding the separately reported admission overhead and reset. All six final screen hashes and charged operation counts matched. Logged work fell from 3,318,904 routing calls to 85,632, while reader searches and wire visits were unchanged. Logs: `target/zcore-earlyexit-{1,2,3}-{before,after}.log`. Retained as a candidate because it avoids no-op work, but it still misses 3,000 TPS and needs interpreter parity and other-build checks. The source now uses the 768 MiB candidate budget with the user's permission for larger memory use; the estimate is not a process memory limit. |
| Collect routed wire sources in a sorted vector instead of a tree | Three alternating ZCore pairs before 2,568.7 / 2,594.8 / 2,341.1 TPS and after 2,565.8 / 2,420.1 / 2,626.1 TPS (`target/zcore-sourcevec-{1,2,3}-{before,after}.log`). The median paired ratio was approximately 0.999, with mixed individual results, so no gain was demonstrated. Its linear duplicate search also has worse asymptotic behavior on source-heavy graphs. Removed. The candidate's Redpiler suite passed 157 tests, 12 ignored (`target/redpiler-rust-sourcevec-suite.log`); screen hashes matched, which does not establish complete parity. |
| Inline hint on `InputState::change` | Three alternating ZCore pairs before 2,491.8 / 2,360.7 / 2,557.1 TPS and after 2,565.9 / 2,584.5 / 2,560.1 TPS (`target/zcore-inline-{1,2,3}-{before,after}.log`). Median paired ratio was approximately 1.030; results were close to measurement variation and remain below 3,000 TPS. Kept provisionally as a one-line compiler hint, subject to checking other builds and code size. Screen hashes matched. |
| Skip an unchanged movement input contribution | Three alternating ZCore pairs before 2,435.2 / 2,382.8 / 2,303.7 TPS and after 2,565.9 / 2,618.0 / 2,600.4 TPS (`target/zcore-noop-{1,2,3}-{before,after}.log`), median paired ratio approximately 1.099. This avoids a redundant store and input lookup; the input-state updater already ignored equal values. Screen hashes matched, and the Rust Redpiler suite passed 156 tests with 12 ignored (`target/redpiler-rust-noop-suite.log`). Kept as a candidate; 3,000 TPS and large-build/interpreter parity remain unproven. |
| Original simpler estimate with 768 MiB budget | A diagnostic build temporarily restored the previous worst-case formula: 26,573 routed readers, 26,873,697 wire visits, 599.0 TPS over 1,000 active ticks and 16.48 s admission (`target/zcore-legacy-estimator-768.log`). The revised estimate with a similarly sufficient budget routed 22 readers. This single exploratory comparison shows that increasing the budget alone does not recover ZCore throughput; the more accurate estimator was restored and remains under correctness and readability review. |

With the 768 MiB budget and early exit, `$env:MCHPRS_ANPU_VERIFY_TICKS='5000'; cargo test -p mchprs_core --lib anpu_first_native_compiled_divergence -- --ignored --nocapture` passed in 43.56 s (`target/anpu-rust-768-early-interpreter.log`). This checks ANPU component block states and wire power through the requested ticks; it does not independently validate wire-side geometry, entities, effects, or reset continuation. The ordinary Rust Redpiler suite passed 156 tests with 12 ignored (`target/redpiler-rust-768-early-suite.log`).

The same ANPU interpreter check passed again after the no-op contribution change and LLVM naming cleanup in 41.70 s (`target/anpu-rust-audited-interpreter.log`), with the same scope and limitations.

A stricter ANPU reset-continuation acceptance attempt failed immediately after 2,000 active ticks: at `(123,27,74)` the restored dust had power 14, matching the interpreter, but its four connection directions differed (`target/anpu-rust-reset-continuation.log`). The previous ANPU comparison intentionally ignored wire-side geometry. The new ignored test also intends to compare pending ticks and command output for 128 continued ticks, but did not reach those checks. This is an unresolved Rust handoff issue under investigation; do not claim reset parity.

A test-only attempt to recompute every restored wire's sides from final neighbor geometry fixed that first wire but failed at `(118,28,74)`: the interpreter retained a cross while recomputation produced a missing east side (`target/anpu-rust-reset-regulate-diagnostic.log`). Wire update history matters; final-neighbor normalization is not a correct general handoff fix and was removed. Runtime investigation shows `refresh_wire_power` updates power but not connection sides, while `notify_shape` handles observers. The fix needs to preserve the interpreter's ordered wire-side updates, including its cross/dot rules, before LLVM parity can be claimed.

With `MCHPRS_ANPU_VERIFY_WIRE_SIDES=1`, the existing interpreter comparison found the first runtime wire-side divergence at tick 16, `(128,36,65)`: interpreter north `Side`, sampled north `Up`, equal zero power (`target/anpu-wire-first-divergence.log`, `target/anpu-first-divergence.json`). Therefore this is not limited to reset publication. Movement probes currently retain deduplicated power callbacks and discard directions; they are insufficient to replay shape callbacks. The interpreter's piston shape path uses its separate ordered face notifications and `wire::on_neighbor_changed_from`, including cross/dot history. Investigation continues before implementation.

The neighborhood trace (`target/anpu-wire-neighborhood-diagnostic.log`) showed a moving piston above the wire becoming solid concrete at tick 16. The new focused `piston_payload_landing_updates_wire_shape_in_interpreter_order` test reproduces the same `Up` versus `Side` discrepancy at tick 2 (`target/rust-wire-landing-repro.log`). Interpreter motion completion issues ordered shape notifications after restoring the payload; sampled `finish_token_part` currently delivers movement power steps but omits the wire-shape notification. Shape changes can themselves trigger wire-neighbor callbacks, so fixing only the visible side value would be incomplete.

An initial Rust candidate applies ordered wire-shape updates at payload landing and replays the shape-triggered neighbor callbacks. It passes the isolated landing test with a watching observer and pending-tick comparison (`target/rust-wire-landing-candidate.log`). The ANPU wire-side diagnostic then advanced past tick 16 but failed at tick 18 when the payload departed (`target/anpu-wire-landing-candidate.log`). Landing alone is incomplete; extension destination writes, departure, empty heads and retracting base restoration must also follow the interpreter's actual ordering. The candidate is not yet accepted or ported to LLVM.

Adding the corresponding retraction departure phase passes the expanded landing/retraction observer-and-scheduler fixture (`target/rust-wire-landing-departure-candidate.log`) and advances the first ANPU wire-side divergence to tick 166 (`target/anpu-wire-departure-candidate.log`). It remains incomplete. At the user's request the runtime must not depend on interpreter-owned implementation code; the initial candidate's pure helper calls are being moved to backend-neutral rules before further work and before any LLVM port.

The in-progress piston-phase candidate causes two existing LLVM/Rust comparison tests to fail at tick 2 because LLVM still returns the previous wire sides (`target/redpiler-wire-phases-candidate-suite.log`: 173 passed, two failed, 15 ignored). This is a known cross-backend parity gap, not an accepted regression. Finish validating the Rust behavior and port the same ordered shape/notification handling into the DLL before those tests can pass.

After adding additional piston-phase shape notifications, the strict ANPU component/block comparison, including wire sides, passed 2,000 ticks (`target/anpu-wire-phases-candidate.log`). The reset-continuation test then reached the scheduler comparison and failed at reset: both sides had the same 133 pending tick entries, but FIFO order differed within equal deadline/priority groups (`target/anpu-rust-wire-phases-reset.log`). It did not reach continuation or command-output checks. Ordering is being investigated; the runtime wire candidate is still not accepted. Its new shape logic now calls backend-neutral pure geometry rules, rather than interpreter entry points.

The order difference is already present in the Rust runner before reset; restoring to the host preserved that ordering (`target/anpu-rust-reset-order-diagnostic.log`). The first mismatch is the third pending entry, with identical deadline and priority and no missing or extra entries. This points to callback scheduling order, not loss of entries during handoff. Ordered per-tick comparisons are being added to find the first divergence.

Adding a second observer to the small movement fixture reproduced a different FIFO reversal: the initial candidate scheduled the observer watching the wire before the observer watching the moved block, whereas the interpreter does the opposite (`target/rust-wire-fifo-repro.log`). Combining observer and wire shape deliveries in one ordered pass passes this fixture and a separate empty-piston observer-order fixture (`target/rust-wire-ordered-candidate.log`, `target/rust-empty-piston-fifo.log`). ANPU still has a separate first pending-order divergence at tick 150 (`target/anpu-wire-ordered-pending.log`): the runner schedules repeaters at x=108 and x=110 in the opposite order. A narrow interpreter callback trace shows the first callbacks at x=112, 110, 108, 106 while the sampled schedules x=112, 108, 110, 106 (`target/anpu-pending-first-trace.log`). Investigation continues.

The corresponding piston event trace shows the same reversal before those repeater callbacks: interpreter retracts pistons at x=112, 110, 108, 106; sampled execution retracts x=112, 108, 110, 106 (`target/anpu-pending-piston-order.log`). There were no nearby active motions at the start of that tick (`target/anpu-pending-motion-order.log`). This rules out fixing the issue by sorting the exported pending-tick list; the request/callback order must be corrected at its source.

Further test-only traces found the two requests came from the same wall torch at `(109,40,75)` (`target/anpu-piston-source-origin.log`). The sampled compiler's torch watcher footprint used west/east-first ordering and an overbroad two-hop neighborhood, while the interpreter's torch callback uses `surrounding_notifications` and visits east before west. `torch_piston_watchers_follow_interpreter_notification_order` reproduced the reversal in a small circuit (`target/rust-torch-piston-order-repro.log`). The candidate now builds the footprint using the shared neutral notification order; validation is in progress. No interpreter is called by the runtime for this ordering.

The focused torch-order test passed (`target/rust-torch-piston-order-fixed.log`). The strict ANPU state, wire-side and pending-order comparison advanced from tick 150 to a new pending-order mismatch at tick 218 (`target/anpu-torch-order-candidate.log`): both sides had 267 entries, but a pair of equal-deadline repeaters at x=108 and x=92 appeared in different order. This is not full parity and is under investigation.

The interrupted tick-218 source diagnostic was rerun with `$env:MCHPRS_ANPU_VERIFY_TICKS='220'; $env:MCHPRS_ANPU_VERIFY_PENDING='1'; $env:MCHPRS_ANPU_VERIFY_WIRE_SIDES='1'; $env:MCHPRS_ANPU_TRACE_PENDING='1'; cargo test -p mchprs_core --lib anpu_first_native_compiled_divergence -- --ignored --nocapture`. It failed at the same pending-order mismatch (`target/anpu-pending-218-source-rerun.log`): both runtime updates originate from the falling torch at `(100,53,124)`. A temporary compiler-probe diagnostic (`target/anpu-pending-218-probe.log`) showed that the probe itself sees the first callbacks with off inputs in order x=108, x=92, while the live interpreter sees x=92, x=108. The probe's quiet basis has the repeaters unpowered; both are powered in the live trace. This establishes differing sampled and live contexts, but does not establish which state difference causes the reversal. Selecting the first or last callback without reproducing the cause would not fix the discrepancy. The temporary probe logging was removed; the older coordinate-specific diagnostics remain pending a regression that replaces them. The compile-time frame comparison also excludes appended output sources such as torches, and its existing mismatch diagnostics do not reject a program. Next isolate a small circuit where the same source transition has different order in different valid states; determine whether ordered sampled programs can represent it before changing execution semantics.

An interpreter snapshot immediately before that torch tick (`target/anpu-edge-snapshot.json`, captured with a temporary hook now removed) was replayed without advancing unrelated scheduled work. `cargo test -p mchprs_core --lib anpu_torch_edge_neighborhood_diagnostic -- --ignored --nocapture` (`target/anpu-edge-neighborhood.log`) reproduced the interpreter's off-input callbacks and repeater queue in x=92, x=108 order with bounds ±15 x, ±6 y, ±10 z. Compiling that extracted neighborhood reproduced the mismatch: the Program queues x=108, x=92 (`target/anpu-edge-neighborhood-compile.log`). Shrinking the neighborhood to ±12 x, ±4 y, ±6 z instead gives x=108, x=92; that crop removes relevant circuit state or geometry and cannot serve as the regression. The test currently depends on an untracked diagnostic snapshot and is not yet a self-contained regression. This result does not isolate the responsible state or show that one Program can serve both contexts.

Controlled snapshot mutations (`target/anpu-edge-controlled-contexts.log`, same diagnostic command) found that turning off all other nearby torches reverses the off-input callback and equal-priority pending order, with geometry fixed. Changing the target repeaters to unpowered did not reverse the first off-input callbacks, but it introduced earlier high-priority scheduling, so pending order there is not comparable. Setting all wire power to zero produced no target callbacks. These mutations did not settle the network and may be inconsistent states; they indicate where to investigate, not proof that two reachable contexts require different fixed programs. The diagnostic records binary input-on observations, not exact input strengths. A self-contained reduced fixture and valid prepared contexts are still needed.

Switching individual lit torches off in the same snapshot identified `(100,53,122)` as sufficient to reverse the first off-input callback order (`target/anpu-edge-individual-torches.log`); no other individually changed lit torch in that neighborhood did so. This remains an artificial state until its preceding source transition and resulting pending work are replayed.

The diagnostic then switched that neighboring torch off and delivered its normal torch-change notifications before applying the target torch's falling edge (`target/anpu-edge-peer-prefall.log`). The target repeaters had no pending ticks immediately before the target edge, and their resulting equal-priority queue was x=108, x=92, opposite to the captured live neighborhood. This removes the stale wire-power objection for that immediate transition, but the test had not yet verified that the peer's input permitted it to fall or inspected all unrelated pending work, so reachability and representation requirements remain open.

The follow-up (`target/anpu-edge-peer-validity.log`) confirms the neighboring torch's input does call for it to turn off. Its transition schedules five other repeater ticks, which the test retains; neither target repeater is among them. It then yields x=108, x=92 for the target edge, versus x=92, x=108 with the neighboring torch still lit. This is a locally consistent alternate transition sequence, but the snapshot does not include the original whole-plot scheduler or block entities, so it is not yet a proof of whole-plot reachability. The diagnostic only prints results and does not assert correctness.

The same compiled Program was started twice from the captured neighborhood and the peer transition replayed in the second run (`target/anpu-edge-shared-program.log`). Both compiled target inputs were 8 before and 0 after the target edge, and the Program queued x=108 before x=92 in both runs. The live-neighborhood interpreter queues the opposite order; the propagated peer-off interpreter queues the compiled order. The peer-off compiled run also contains additional normal-priority torch ticks, so these runs do not establish full queue parity. The fixture lacks the original global pending work and exact interpreter input strengths during each callback; inspect intermediate contributions and callback points before concluding whether the existing operation stream can represent both contexts.

With temporary test-only callback and compiled-contribution tracing (`$env:MCHPRS_ANPU_EDGE_STRENGTH='1'; cargo test -p mchprs_core --lib anpu_torch_edge_neighborhood_diagnostic -- --ignored --nocapture`, `target/anpu-edge-intermediate-strengths.log`), both interpreter contexts reported the same initial target callback strengths: x=108:6, x=92:6, x=108:4, x=92:2, x=92:2, x=108:2. With the peer lit, x=92 first reached 0; after the peer's propagated fall, x=108 first reached 0. The compiled target program instead delivered one callback to each at 0, in x=108 then x=92 order in both contexts. Its printed target input contributions contained no direct link from the neighboring torch. Thus the sampled program loses intermediate wire propagation, and its current observed execution does not distinguish these contexts. This does not yet prove that no ordering of the existing instructions could encode both contexts; test that against a self-contained fixture before considering a bounded representation. Temporary coordinate-specific strength traces remain until the diagnostic is replaced.

The first crop is not a complete consistent-state fixture: applying normal wire updates changes boundary wires (`cargo test -p mchprs_core --lib anpu_edge_snapshot_wire_consistency -- --ignored --nocapture`, `target/anpu-edge-wire-consistency.log`; the initial diagnostic used saved states in later checks, so its reported count is not reliable). A temporary interpreter-boundary hook captured the original queue immediately after popping the target torch tick (`target/anpu-edge-pending.json`, 244 entries); the hook was removed. The queue includes zero-delay normal ticks for `(100,53,120)` and `(100,53,122)`. Most entries lie outside the crop, and the existing compiler rejects scheduled entries outside compilation bounds. Therefore neither the cropped empty-queue comparison nor the manually reordered peer is acceptance evidence for the complete scheduled state. Preserve and compare the relevant original queue while building a self-contained, electrically consistent fixture.

Replaying normal wire updates on the crop reached a pass with no directly changed wires after 11 initial changes, retaining the 18 scheduled ticks they generated (`target/anpu-edge-settled-replay.log`). Both local target orders persisted. Compiling that changed state with its actual pending queue and piston state failed explicitly with `scratch entry has queued piston work` at `(86,53,122)` (`target/anpu-edge-settled-compiled.log`): the boundary changes also generated piston events. This attempt is not regression coverage, and discarding those events would invalidate it. The existing test currently fails at that admission check when explicitly run. Preserve the original boundary inputs or execute the resulting work correctly before comparing the backend.

An exploratory bounding-box sweep (`cargo test -p mchprs_core --lib anpu_edge_snapshot_boundary_sweep -- --ignored --nocapture`, `target/anpu-edge-boundary-sweep.log`) did not find an unchanged-wire boundary among six tested crops. The original 1,771-block crop changed 11 wires and queued 10 piston events; a 5,108-block crop changed six wires, queued 13 ticks and no piston events. The latter is easier to compile but still changes boundary inputs and is not a substitute for the requested equivalent-state regression. Repeating the current `Contribution` operation is not sufficient to represent the measured intermediate strengths: it reads the source's current output and would write the same final value on each repetition. No production lowering or runtime fix has been selected.

The larger settled crop was also rejected by the compiler: its snapshot contains a moving-piston block but did not capture the corresponding entity (`target/anpu-edge-large-settled-compiled.log`, position `(80,53,110)`). Copying only block IDs is insufficient to construct an equivalent starting state. This diagnostic failure is not an LLVM or production regression test. Preserve movement entities and pending work, or reduce to a fixture that does not cut through active motion.

The acceptance target is now the ignored `anpu_ordered_pending_ticks_match_interpreter` test. It loads the original ANPU fixture independently for the interpreter and compiler, applies the same start control, and compares full wire states at sampled Program positions and the ordered pending queue after every tick through 218. Run it with `cargo test -p mchprs_core --lib anpu_ordered_pending_ticks_match_interpreter -- --ignored --nocapture`; it is expected to fail until the ordering defect is fixed. The three snapshot-only diagnostic tests and their temporary strength-tracing hooks were removed; the historical logs and commands above describe past investigation and are no longer runnable against the current source. Any smaller reproducer must preserve the failure through normal replay, including boundary inputs, entities and scheduling history.

The first run of this test (`target/anpu-ordered-pending-regression.log`) reached tick 218 with no earlier checked wire or queue mismatch, then failed at queue index 65: both queues contain 267 entries, with x=108 first in the compiled queue and x=92 first in the interpreter queue. This is an intentionally failing correctness regression, not a passing test or a fix.

After adding the input inlining hint and no-op contribution check, `$env:MCHPRS_BENCH_PUBLISH_ALL='1'; & target/release/examples/bench_cpus.exe zcore 5000 1 integrated` passed with no first screen divergence over 5,000 ticks and 244 interpreter screen changes (`target/zcore-rust-768-interpreter-screen.log`, `target/redpiler-bench-results/zcore-production.json`). This checks per-tick published screen output, not every block, entity, effect, or reset continuation. Its per-tick timed/published throughput is not comparable to the separate kernel-only trials.

The audited Rust candidate's `target/release/examples/bench_fpu.exe` run admitted FPU in 7.35 s and matched the interpreter's output trace for 10,000 ticks with 1,108 control changes (`target/fpu-rust-audited-interpreter.log`, `target/redpiler-bench-results/fpu.json`). The output changed; no first divergent tick was found. The instrumented harness measured 2,443 tick TPS and 810 TPS including publication, but this is one sample with per-tick timers, not a paired performance comparison or complete state/effect/reset parity.

The first audited Chungus attempt was stopped because the PowerShell memory-monitor variable was inferred as a 32-bit integer and overflowed above 2 GiB; its output was not used. The retry uses a 64-bit counter and stops the benchmark if sampled working set exceeds 5 GiB.

The retry, `target/release/examples/bench_chungus.exe 128 1`, passed per-tick block comparison for 506,126 positions and observed 12,526 block changes (`target/chungus-rust-audited-out.log`). Compilation took 108.35 s; sampled peak working set was 4,769,775,616 bytes. The single run measured 18,219 tick TPS and 5,453 TPS including publication, both with per-tick timing. Entities and event traces were not compared, so this is not full behavioral parity or a paired benchmark.

ANPU early-exit attempt, same 768 MiB budget: three alternating 5,000-tick pairs before 16,613.4 / 18,912.1 / 18,783.3 TPS and after 18,241.0 / 17,004.3 / 19,601.3 TPS (`target/anpu-earlyexit-{1,2,3}-{before,after}.log`); all final screen hashes matched. The kernels lasted only 0.26–0.30 s and vary enough that these runs cannot establish whether there is an ANPU regression. Longer paired trials are needed. ANPU used an estimated 98,182,891 bytes of dense tables, with 2,247 routed readers all rejected by the state bound, not the budget.

The attempted 50,000-tick ANPU continuation went mostly idle and reported approximately 187,000 TPS over a 0.267 s kernel (`target/anpu-earlyexit-long-1-before.log`), so it was stopped after the first pair and is not an active-work performance result. A future longer comparison must repeat active executions or otherwise sustain representative work; simply increasing tick count is invalid.

Commands for the Rust-only trials, run from the repository root in PowerShell after building each corresponding candidate:

```powershell
cargo build --profile bench --features redpiler-trace -p mchprs_core --example bench_llvm_zcore
$env:MCHPRS_REDPILER_PHASE_TIMINGS = '1'
& target/release/examples/bench_llvm_zcore.exe 1000 1 --rust-only
cargo test -p mchprs_core --lib redpiler:: -- --skip llvm_
```

The 512 MiB run sampled the benchmark process working set every 500 ms and stopped it if a sample exceeded 5 GiB; sampled peak is a lower bound on instantaneous peak. Setup, kernel and reset timing are reported separately by the runner. Preparation includes 413 interpreter ticks; routing counters include preparation and active ticks, whereas TPS covers the active kernel. The 64 MiB and 512 MiB builds used different temporary budget constants, and rerunning the current source alone does not recreate both candidates.

## ZCore topology investigation

The phase census found 94.01 s of the 101.84 s sampled compilation in topology lowering, with 54,374 routed readers. The original local-graph builder scanned all movable tokens for each wire and all wires and tokens for each reader. A candidate now indexes token cells once and queries local wire/contact positions, preserving sorted token axes and handling overlapping token cells. The temporary index is released after reader construction. A focused test compares indexed candidates with the former brute-force predicates; the Redpiler suite passed 172 tests (14 ignored, `target/redpiler-spatial-index-suite.log`). An otherwise comparable ZCore phase census reduced topology lowering from 94.01 s to 9.12 s, and sampled compilation from 101.84 s to 16.95 s (admission 102.34 s to 17.44 s), while retaining the same 62,436 targets, 55,621 arrangement groups and 54,374 routed targets (`target/zcore-census-current.log`, `target/zcore-census-spatial.log`). These are single runs, not paired timing trials. Runtime routing counters behind the existing `redpiler-trace` feature were added to distinguish routed work from other execution costs; their results are not yet available.

The FPU command-block encoder and standalone C state machine are still experimental and not wired into native admission. The Rust command-spec tests passed in the preceding suite; a freestanding Windows Clang/lld harness for impulse, chain, repeat, effect queue and limiter/error ordering returned status 0. This is not a DLL integration or interpreter-parity result. The standalone C queue is bounded, while the interpreter permits unlimited queued effects when output limits are disabled; it must not be activated in that form. A possible integration is a resumable DLL tick with a bounded event mailbox drained by the host without executing circuit work, with u64 output counters and explicit limiter-state synchronization.

## ZCore current measurement in progress

The first optimized 5,000-tick Rust kernel trial with the hybrid routing compiler measured 283.5 TPS and 102.1 s setup (`target/zcore-paired-semantic12.log`). The earlier 2,927 TPS and roughly 13–18 s setup were measured before the routing changes; this indicates a substantial regression but is not yet a completed paired comparison. The LLVM half measured 790.3 TPS with 104.98 s setup; final screen hashes matched. This one pair is 2.79x faster in LLVM, but both runtimes regressed substantially versus the older checkpoint. Raw data: `target/redpiler-bench-results/zcore-llvm-paired-semantic12.json`. A separate phase census (`target/zcore-census-current.log`) measured 94.01 s in topology lowering out of 101.84 s total, with 54,374 routed targets among 62,436 reader targets, 55,621 arrangement groups and a 67,108,286-byte retained-table estimate. A `redpiler-trace` Rust-only run (`target/zcore-routing-profile.log`) recorded 733,578 token changes, 5,938,398 reader searches and 61,023,797 wire visits in 66,798,836 semantic operations over 413 preparation plus 1,000 active ticks. It measured 289.2 instrumented active TPS. Routing visits account for 91.4% of charged work; these counts include preparation and do not by themselves measure time per operation. The prior 4,509-tick Rust/LLVM comparison passed with the baseline semantic-12 DLL; the routing optimization candidate still requires its own ZCore acceptance.

## Native boundary review

The DLL casts Program tables to aligned C records. Rust now copies encoded bytes into an eight-byte-aligned allocation retained for the module lifetime; the C entry points reject unaligned Program and state buffers and reject a bad Program magic before reading tables. The loaded-module tests exercise malformed descriptors; an additional unaligned state-buffer assertion is pending a rerun. The module descriptor checks ABI and semantic version before activation. These checks do not establish safety against a malicious operator-supplied native module: `MCHPRS_LLVM_DLL` is an explicitly trusted override.

The process-owned work directory is locked, requires a marker before recursive cleanup, and resets at server startup. The Windows loader now uses the same cache build path as Linux. Linux cache integration exposed and fixed canonicalization of the `ld.lld` symlink, which had caused the generic LLD driver to be invoked; a cache-built module test passed afterward. Container restart and concurrent-process tests, cancellation propagation to Clang, and a complete unsafe/OS audit remain open.

## Chungus first paired run

One alternating Rust/LLVM pair of 128 ticks used `bench_chungus.exe 128 1 --paired` with complete publication and the ANPU routing candidate DLL. Rust compiled in 109.98 s, measured 16,714 tick-only TPS and 4,512 TPS including publication; LLVM compiled in 102.21 s, measured 27,192 tick-only TPS and 36.3 TPS including publication. Both traces matched interpreter block states at 506,126 positions, and 12,526 changed blocks were observed. Each kernel sample was only 4.7–7.7 ms, so these are rough single-pair estimates; entity and effect parity were not checked. Raw results are in `target/redpiler-bench-results/chungus-paired.json`. The Rust process reached a sampled peak resident set of 4.77 GB during compilation, close to the 6 GB container limit.

The native readback path marked every projection cell dirty on each read, forcing full-world publication despite few changes. A candidate now compares old and new projected cells and preserves any previously dirty cells. Its loaded-DLL suite passed 17 tests (2 long comparisons ignored), including repeated readback, movement and comparator checks (`target/llvm-dirty-aligned-suite.log`); the full Redpiler suite passed 164 tests (`target/redpiler-dirty-aligned-suite.log`). A second 128-tick pair still matched the interpreter block trace, but improved native tick-plus-publication only from 36.3 to 41.6 TPS (Rust 5,481 TPS); native tick-only was 28,248 TPS (Rust 18,610 TPS). This is one short pair; raw results are in `target/redpiler-bench-results/chungus-paired-dirty-candidate.json`. Full native snapshot transfer and analog histogram reconstruction remain additional costs. A further candidate skips execution-only routing and histogram reconstruction during publication, rebuilding them for explicit native-to-Rust execution handoff. The first full suite caught an incorrect expected JSON shape in a new, not-yet-wired FPU command encoder test. After correcting that expectation, 171 Redpiler tests passed, 14 ignored (`target/redpiler-publication-fast-suite-fixed.log`); this includes publication followed by comparator handoff. One new 128-tick pair passed the block trace: Rust measured 16,468 tick-only / 5,347 with publication TPS, LLVM 27,673 tick-only / 67.3 with publication TPS; setup was 95.87 s and 97.97 s respectively (`target/redpiler-bench-results/chungus-paired-fast-publication.json`). This is a 1.86x improvement over the first native publication sample but remains a severe gap; full snapshots and full projection scanning still require work.

## FPU initial admission and performance check

The current Rust sampled compiler admits `FPU_27.schem`: one active 10,000-tick input-window trial reported 28.49 s compilation, 431.6 tick-only TPS, 305.0 TPS including publication, and no sampled output divergence (`target/redpiler-bench-results/fpu-rust-trial1.json`). LLVM explicitly rejected the same Program after 28.43 s (`target/redpiler-bench-results/fpu-llvm-trial1.json`), so no native TPS or paired comparison exists. Diagnostics show 46,702 components including three `Unknown` components; the FPU palette contains three impulse command blocks with tellraw messages for Inf, division by zero and NaN, and command-block execution remains unsupported by native admission. Their saved success counts show historical execution; the benchmark does not establish whether its input sequence activates them. The fixture's earlier rejection of state 13700 is stale: it is inert light-blue glazed terracotta, and the current parser admits inert registry states. Further command-block investigation and full state/effect parity are required before FPU LLVM admission.

## ANPU routing fix: paired remeasurement

The candidate initialized routing distances once and clears only entries in the BFS queue after each root. It passed the same 5,000-tick Rust/DLL state comparison (`target/llvm-anpu-routefix-shadow.log`, 81.64 s). Three alternating 5,000-tick pairs with `MCHPRS_LLVM_DLL=target/native-routefix.dll` gave Rust 13,647.0, 15,623.4 and 15,962.8 TPS (median 15,623.4); LLVM 29,125.8, 25,313.1 and 29,128.0 TPS (median 29,125.8). The median paired LLVM/Rust ratio was 1.825. The median LLVM rate increased 15.95 times from the earlier 1,826.6 TPS baseline. All final screen hashes matched. Raw data: `target/redpiler-bench-results/anpu-llvm-paired-routefix-semantic12.json`; timing excludes publication. These local paired trials are not a production throughput guarantee; the source change still needs the complete test suite and other builds rechecked. Next decision: retain this bounded visited-queue change if those checks pass, then profile any remaining large-build regressions before adding specialization.

## ANPU throughput regression (semantic version 12)

Three alternating 5,000-tick kernel-only Rust/LLVM pairs used `target/release/examples/bench_llvm_zcore.exe 5000 3 --anpu`, with the semantic-12 DLL selected through `MCHPRS_LLVM_DLL`. Rust TPS: 16,529.0, 16,159.5, 14,853.5 (median 16,159.5); LLVM TPS: 1,866.3, 1,826.6, 1,737.0 (median 1,826.6). Median LLVM/Rust throughput is 0.113, approximately 8.8 times slower. Sampled setup was 19.1–20.6 s and the final screen hash matched on every trial. Full snapshots/publication are outside the timing. The full 5,000-tick Rust/DLL state comparison was run separately as described below. Raw results: `target/redpiler-bench-results/anpu-llvm-paired-baseline-semantic12.json`; log: `target/anpu-paired-semantic12.log`.

Inspection found that the C routing evaluator cleared all 34,781 wire distances for every BFS root; Rust clears only visited wires. A candidate C change clears the existing BFS queue after each root and initializes its distance array once. Its separate DLL (`target/native-routefix.dll`) built in 520.7 ms and passed the 5,000-tick ANPU Rust/DLL state comparison in 81.64 s (`target/llvm-anpu-routefix-shadow.log`); paired remeasurement is running. This is an unverified hypothesis about the performance gap, not yet an attributed profile result.

## ANPU native comparison (semantic version 12)

`$env:MCHPRS_LLVM_VERIFY_TICKS='5000'; cargo test -p mchprs_core --lib llvm_anpu_matches_rust_state_and_effects_each_tick -- --ignored --nocapture` passed 5,000 ticks against the sampled Rust runner. It compares projected blocks, component states, ordered pending ticks, piston and token state, motion queues, input counts, routing and arrangement contributions, comparator outputs, reactive wire strengths and queued notes after each tick. Sampled compilation took 25,092 ms; the complete comparison took 79.42 s. Log: `target/llvm-anpu-shadow-first.log`. This does not by itself prove interpreter parity, effect publication, reset continuation, TPS or production readiness.

The C engine now handles reactive wire and neighbor callbacks, static comparator overrides, hoppers, pressure plates and note event queues. Note events carry the instrument sampled at the edge and require explicit acknowledgement after host publication. Dynamic comparator overrides and command blocks remain unsupported. A separate 5,000-tick Rust/interpreter component-and-wire-power comparison passed before the latest changes; it must be repeated with complete state/effect and reset checks.

The ZCore local piston contact pruning passed its focused test and the 4,509-tick Rust/DLL state comparison with the semantic-12 baseline DLL (`target/llvm-zcore-semantic12.log`). The reported 1,027.40 s elapsed includes pauses during ANPU timing and is not a compilation or throughput measurement. The routing optimization candidate still needs its own ZCore comparison. Its Windows failed-publication note acknowledgement regression passed (`target/llvm-note-publication-routefix.log`). Server startup now resets the process-owned cache and Windows loading uses the same cache mechanism. `cargo test -p mchprs_core --lib redpiler:: -- --nocapture` passed 162 tests with 14 ignored, including a cache test covering the ownership marker, lock, restart cleanup and preservation of a sibling file (`target/redpiler-semantic12-final.log`). Linux `cargo check --locked -p mchprs_core --lib` passed in WSL Ubuntu with Rust 1.99; Clang 18 and LLD also built the C shared object. Loaded-module tests passed: `MCHPRS_LLVM_DLL=/tmp/mchprs-native-routefix.so MCHPRS_LLVM_LINKER=/usr/lib/llvm-18/bin/ld.lld cargo test --locked -p mchprs_core --lib llvm_ -- --nocapture` in WSL Ubuntu: 16 passed, 2 long comparisons ignored, including note acknowledgement after failed publication (`target/llvm-linux-tests.log`). After the changed-cell and buffer-alignment changes, the Linux loaded-module suite passed 17 tests with 2 long comparisons ignored (`target/llvm-linux-dirty-aligned.log`). The .so was built from the current C candidate with Clang 18 and LLD. A separate test without the prebuilt-module override initially failed because canonicalizing the `ld.lld` symlink changed its invocation name to generic `lld`; preserving the executable path fixed it. The cache-built comparator test then passed (`target/llvm-linux-cache-integration-fixed.log`). Docker image integration remains unverified. The semantic-12 DLL build took 656.7 ms and produced a 40,960-byte DLL; this is one build, not a cold/warm series.

## Current correctness work — local routing in Rust and C (2026-11-02)

The compiler now builds local connectivity tables instead of depending on a single-token discovery seed. Small inputs retain dense arrangement tables (4,096 states per group); oversized or overlapping-token inputs retain an immutable routing graph. Dense lowering runs in batches capped at 65,536 source/state slots, with a 64 MiB retained-table estimate. These limits do not bound the entire compiler process or all graph metadata. Large tables remain binary data; no per-occurrence C functions or generated C initializers were added.

The first hybrid Rust ANPU run matched interpreter component states and wire power for 5,000 ticks (`target/anpu-hybrid-shadow2.log`, 52.73 s including setup and comparison; sampled compilation 20.56 s). This comparison does not establish wire-side geometry, all effects, entities or full reset-continuation parity. The independent interpreter matched the frozen reference for its 4,096-tick prefix. These are correctness checks, not paired TPS measurements. The obsolete isolated Cartesian-probe diagnostic was removed after recording its historical measurements below.

The C executor now evaluates the same local routing graph, selects physical token digits, searches dust paths to attenuation 14, and updates arrangement contributions before movement callbacks inside the DLL. Binary auxiliary tables 33–41 contain local records, token axes, ports, wire edges, tagged sources, reader links and token-to-reader dependencies. Table 42 carries comparator modes, static overrides and initial outputs. The ABI descriptor remains version 3; semantic version 10 rejects old DLLs, and snapshot schema 3 includes comparator outputs. Rust reads state at publication/handoff boundaries; it does not execute native ticks. Routing caches and analog input histograms are rebuilt during readback so continued Rust execution uses the live inputs.

Observer fixes distinguish directed shape notifications from undirected dust power callbacks. Unpowered observers ignore the latter; powered observers without pending work can release. Dust watcher discovery uses the interpreter's turbo footprint. Both movement and ordinary wire callbacks preserve that distinction. Tests force the same fixture through dense and routed lowering, and exercise real DLL piston movement, dust paths, source changes, pending ticks and handoff.

Comparator evaluation, analog side-input histograms and scheduled callbacks are implemented in C for static overrides. Dynamic comparator overrides, reactive callbacks and the remaining ANPU component families are still rejected before activation. This is not full LLVM ANPU support. Full-state ANPU acceptance and updated alternating Rust/native TPS trials remain required; the older ZCore throughput figures below are not measurements of this revision.

Build command: `./scripts/build-llvm-native.ps1`, using the existing `target/llvm-23.1.4/bin/clang.exe` and matching `lld-link.exe`. The semantic-10 build took 492.0078 ms and produced a 35,840-byte DLL and 39,178-byte object; shared sources were 34,364 bytes (`engine.c`) plus 30,818 bytes (`native_movement.c`), with a 4,389-byte ABI header. This is one local compiler/link invocation, not a cold/warm trial series or Program conversion time. Exact flags, tool/source hashes and sizes are recorded in `target/native-engine-build.json`.

Validation: `cargo test -p mchprs_core --lib llvm_ -- --nocapture` passed 15 tests (1 ignored); `cargo test -p mchprs_core --lib redpiler:: -- --nocapture` passed 160 tests (13 ignored), including the actual loaded-DLL routing and comparator handoff tests. Logs: `target/llvm-routing-suite-final.log`, `target/redpiler-routing-suite-final.log`. A subsequent explicit large ZCore comparison failed during sampled compilation at `(232,20,184)` with `local connectivity exceeds state bound`, before DLL execution (`target/llvm-zcore-routing-shadow.log`, 40.59 s). This is an unresolved regression in conservative piston reader axis selection; the earlier ZCore acceptance does not apply to the new hybrid compiler until repaired and rerun. The new full ANPU comparison has not yet been rerun after the last memory/callback changes.

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

### 2026-11-02 — staged source-edge lowering candidate rejected

An isolated checkout at `F:/rustrepos/MCHPRS-compiler-fix` tested retaining unlocked repeater callbacks while their cached old input was still positive, before the sampled final contribution update. The unchanged full-ANPU regression was copied into that checkout. Command: `cargo test -p mchprs_core --lib anpu_ordered_pending_ticks_match_interpreter -- --ignored --nocapture`, with `CARGO_TARGET_DIR=F:/rustrepos/MCHPRS/target`.

With the existing reactive contribution prepass, the candidate failed earlier at tick 22 (64 pending entries on each side), because final contributions were applied before the newly retained callbacks (`target/anpu-staged-candidate.log`). A second diagnostic disabled contribution hoisting and reactive callback skipping while retaining the final reactive reevaluation. It advanced to tick 218 but failed with the original 267-entry FIFO reversal (`target/anpu-staged-sequential-candidate.log`). This does not establish that removing hoisting is generally correct; it only tests that staging alone does not resolve this mismatch.

Neither candidate was merged. Their combined patch is retained as `target/anpu-staged-rejected.patch`; the experimental compiler/executor edits were restored in the isolated checkout. The primary worktree's compiler and executor were not changed. A compiler fix still needs a validated way to represent or select the context-dependent zero-crossing order; the experiment does not prove which bounded representation is sufficient. LLVM port and reset continuation remain unfinished.

### 2026-11-02 — LLVM wire-shape parity baseline

The interpreter/Rust tick-218 FIFO discrepancy is deferred. `./scripts/build-llvm-native.ps1` rebuilt `target/native_engine.dll` (40,960 bytes, 524.5 ms). With `MCHPRS_LLVM_DLL` explicitly set to that path, `cargo test -p mchprs_core --lib llvm_ -- --nocapture` loaded the DLL and reproduced the two recorded failures at tick 2: `llvm_compressed_concrete_axes_preserve_motion_and_wire_state` and `llvm_routed_movement_and_handoff_match_rust` (15 passed, two failed, two ignored; `target/llvm-shape-before.log`).

With the same DLL and `MCHPRS_LLVM_VERIFY_TICKS=256`, `cargo test -p mchprs_core --lib llvm_anpu_matches_rust_state_and_effects_each_tick -- --ignored --nocapture` loaded ANPU, compiled its shared Program in 7,114 ms, and failed its full projection comparison at tick 15 (`target/llvm-anpu-shape-before.log`). The test did not reach 256 ticks or establish reset, effects, or interpreter parity.

Rust performs ordered directional wire-shape updates and subsequent wire-neighbor callbacks during movement; LLVM's `notify_shape` only delivers precomputed observer notifications. LLVM stores wire power but not mutable wire sides, and its state/publication paths reconstruct sides from the initial block. A complete port therefore needs live geometry and callback ordering, mutable side state and validated export, as well as the matching movement phase calls. No executor or ABI fix has been applied yet; the two tests and ANPU comparison remain failing.

The first port milestone adds a packed byte of wire sides to the DLL's component state, initializes it from a validated binary Program table, and exports it in snapshot version 6. The Rust snapshot reader validates all four sides before applying them to the publication/reset projection. LLVM semantic version 13 rejects older DLLs. This does not yet update sides during movement: both known wire-side parity failures remain expected. The rebuilt DLL was 41,472 bytes and took 555.7 ms (`./scripts/build-llvm-native.ps1`). `cargo test -p mchprs_core --lib llvm_read_marks_changes_without_losing_unpublished_cells -- --nocapture` passed against it, and `cargo test -p mchprs_core --lib llvm_wire_sides_round_trip_and_reject_invalid_encoding -- --nocapture` passed its initial round-trip and Rust-side malformed-table check. Ordered movement notifications, live geometry, and changed-side tests remain to be implemented.

After adding the DLL-side malformed-table rejection assertion, `MCHPRS_LLVM_DLL=target/native_engine.dll` with `cargo test -p mchprs_core --lib llvm_ -- --nocapture` ran the test successfully and recorded 16 passed, the same two wire-side failures, and two ignored (`target/llvm-shape-state-stage.log`). This is an intermediate result, not movement or full reset parity.

The next candidate ports Rust's ordered piston-phase shape calls and wire-neighbor callbacks into `llvm_movement.c`. Binary tables contain fixed shape targets, geometry references and ordered neighbor targets; the DLL reads live token/head occupancy, applies the same directional cross/dot rules, and schedules callbacks without a Rust or interpreter tick. The DLL semantic version is 14. `./scripts/build-llvm-native.ps1` produced a 46,080-byte DLL in 560.9 ms. With `MCHPRS_LLVM_DLL` set to this rebuilt DLL, `cargo test -p mchprs_core --lib llvm_ -- --nocapture` passed 19 tests (two ignored), including both previous wire-side failures and a new landing/retraction observer-order comparison (`target/llvm-shape-port-suite.log`).

`cargo test -p mchprs_core --lib redpiler::` passed 181 tests (16 ignored; `target/llvm-shape-full-redpiler-final.log`), and `cargo test -p mchprs_core --lib redstone::` passed 143 (four ignored; `target/llvm-shape-redstone-tests.log`). The focused `llvm_wire_shapes_publish_and_reset_like_rust_in_each_mode` test passed with full, redstone-only and IO-only publication, comparing a changing wire side, reset state and ordered pending ticks. With `MCHPRS_LLVM_VERIFY_TICKS=5000`, `cargo test -p mchprs_core --lib llvm_anpu_matches_rust_state_and_effects_each_tick -- --ignored --nocapture` passed in 62.20 s (`target/llvm-anpu-shape-5000.log`): it compares Rust and LLVM state, full projected blocks, pending order, motion and sampled note effects, not the interpreter. The separate interpreter/Rust tick-218 FIFO discrepancy remains deferred. ZCore and wider acceptance remain in progress; this is not a production-readiness or performance claim.

With `MCHPRS_LLVM_VERIFY_TICKS=4096`, `cargo test -p mchprs_core --lib llvm_zcore_matches_rust_state_and_motion_each_tick -- --ignored --nocapture` passed after ZCore preparation and 4,096 further ticks in 138.80 s (`target/llvm-zcore-shape-candidate.log`). The landing/retraction fixture also passed four initial wire-side patterns, including cross and dot (`target/llvm-wire-shape-cases.log`). One earlier attempt to compile that fixture during the ZCore run failed with Windows LNK1104 because the running test executable was locked; rerunning after ZCore finished passed. These comparisons do not establish interpreter parity.

`cargo test -p mchprs_core --lib llvm_anpu_reset_and_continuation_match_rust -- --ignored --nocapture` passed in 16.77 s with the rebuilt DLL (`target/llvm-anpu-reset-candidate.log`). Both backends ran ANPU for 2,000 ticks, reset into separate original worlds, and the test compared projected positions, comparator entities, ordered pending entries, piston state and chat at reset and through 128 interpreted continuation ticks. This compares the two handoffs; it is not an independent interpreter oracle for the compiled execution.

A first paired ZCore benchmark attempt was stopped after its first Rust sample because a further DLL visibility correction was identified. It used a `redpiler-trace` build and the preceding DLL; the single 389 TPS Rust result (`target/llvm-zcore-shape-paired.log`) is not a paired result and cannot establish the new candidate's performance. Rebuild and rerun both backends without that feature after correctness checks.

The follow-up visibility correction retains callbacks when a moved redstone block projects the same block as the compiled component. The rebuilt DLL remains 46,080 bytes and took 572.4 ms. The unused legacy direct-publication method was removed because it reconstructed wire sides from initial state; production publication and reset already use the validated full snapshot path.

Against that corrected DLL, `cargo test -p mchprs_core --lib redpiler::` passed 181 tests (17 ignored; `target/llvm-shape-redpiler-v14.log`). The 5,000-tick ANPU Rust/LLVM comparison was repeated and passed in 71.06 s (`target/llvm-anpu-shape-final.log`). Its scope is the same as the earlier comparison and does not resolve the deferred interpreter discrepancy.

The corrected DLL also passed the ZCore preparation plus 4,096-tick Rust/LLVM comparison in 134.87 s (`target/llvm-zcore-shape-final.log`). The measured execution compares ordered pending ticks, full projected blocks and movement/electrical state each tick; setup and throughput are assessed separately.

Three alternating untraced ZCore pairs used `cargo build --profile bench -p mchprs_core --example bench_llvm_zcore`, then `MCHPRS_LLVM_DLL=target/native_engine.dll target/release/examples/bench_llvm_zcore.exe 5000 3`. Rust kernel TPS was 398.8 / 398.1 / 391.7 (median 398.1); LLVM was 2,324.5 / 2,659.9 / 2,514.4 (median 2,514.4), median paired ratio approximately 6.42. All final screen hashes matched. First Rust and LLVM setup were 14.54 s and 19.22 s; subsequent Rust setup was 21.28–22.01 s and LLVM 20.23–23.36 s. These are first/subsequent in-process observations, not controlled cold-cache measurements. The DLL build was separately 572.4 ms. Raw results are preserved in `target/redpiler-bench-results/zcore-llvm-paired-shape-v14.json` and `target/llvm-zcore-shape-paired-final.log`. The Rust throughput is substantially below the prior ~2,600 TPS measurements and requires investigation; the timing does not by itself isolate the cause.

The same untraced executable and DLL with `5000 3 --anpu` measured Rust 14,925 / 15,020 / 15,089 TPS (median 15,020) and LLVM 26,742 / 30,587 / 27,075 TPS (median 27,075), median paired ratio approximately 1.79. All final screen hashes matched; first Rust/LLVM setup was 5.77/6.13 s, later setups 6.35–6.45/6.29–6.84 s. The screen check is not full correctness; the separate 5,000-tick state comparison above supplies the Rust/LLVM comparison. Logs and results: `target/llvm-anpu-shape-paired-final.log`, `target/redpiler-bench-results/anpu-llvm-paired-shape-v14.json`.

After the callback visibility correction, the semantic version was advanced from 14 to 15 so a DLL built before that correction cannot be loaded silently. The rebuilt semantic-15 DLL is 46,080 bytes; Clang/link took 588.1 ms. The paired measurements above used the corrected semantic-14 build, whose only subsequent C change is the descriptor version.

An initial Rust-only fast-path screen skipped projecting neighboring components that cannot receive a shape notification and used the existing component cell index for the remaining targets. One 1,000-tick baseline/candidate pair measured 407.8/434.7 TPS with the same screen hash (`target/zcore-shape-fastpath-before.log`, `target/zcore-shape-fastpath-after.log`); this is too small a sample to establish a reliable gain and remains far below the earlier throughput. A shared Program table of fixed shape targets is the next candidate; live occupancy, geometry and ordered delivery remain runtime work.

The fixed-shape-target-only candidate screened at 475.7 TPS over 1,000 ticks with the same screen hash (`target/zcore-shape-plan-screen.log`), still well below the target and not a paired performance conclusion. The next candidate also compiles the existing ordered wire-neighbor destinations into the shared Program, including duplicates and potential piston heads, so Rust and LLVM consume the same plan. LLVM semantic version 16 reflects its compacted callback-table format. The semantic-16 DLL builds in 589.1 ms and is 46,080 bytes; validation and timing are in progress.

The combined fixed-target and wire-neighbor candidate screened at 802.2 TPS over 1,000 ticks (`target/zcore-shape-wire-plan-screen.log`), with the same screen hash as the 407.8-TPS baseline and the 475.7-TPS target-only screen. These are sequential single runs, so they identify a promising direction rather than a final performance claim. The retained Program memory census now includes the two shape-plan vectors and their callback payload. Longer alternating comparisons and complete cross-backend correctness remain necessary.

Additional table validation checks that a callback component belongs to its encoded cell; this advances the LLVM semantic version to 17. The rebuilt DLL is 45,568 bytes and built in 577.1 ms. The callback table and fixed shape targets are now produced once in the shared Program and serialized for LLVM, rather than discovered independently by each backend.

Three alternating 5,000-tick Rust-only ZCore pairs compared the saved pre-plan executable with the shared-target/wire-neighbor candidate. Baseline TPS: 403.8 / 402.5 / 407.1 (median 403.8); candidate: 762.2 / 798.7 / 784.0 (median 784.0), median paired ratio approximately 1.93. Every final screen hash matched (`target/zcore-wireplan-pair-{1,2,3}-{before,after}.log`). This is a material gain but still below the earlier ~2,600 TPS. The change does not change sampled semantics; long full-state comparisons are still needed for the shared plan. Passing projection-cell IDs directly and precomputing wire-geometry references are further candidates under test, not included in these measurements.

The geometry candidate stores fixed references to the same 13 positions used by the pure wire-side rules, with live projection reads for movable cells; both Rust and LLVM consume these references. It also passes already known cell IDs into Rust shape notifications. The 4,096-neighborhood wire oracle was extended to compare the indexed geometry calculation. This candidate has not yet completed validation or paired timing.

Three alternating 5,000-tick ZCore Rust pairs compared the wire-neighbor plan against the geometry-reference and direct-cell candidate: baseline 730.9 / 774.3 / 770.6 TPS (median 770.6), candidate 1,325.2 / 1,520.0 / 1,523.3 TPS (median 1,520.0), median paired ratio approximately 1.96. All final screen hashes matched (`target/zcore-geometryplan-pair-{1,2,3}-{before,after}.log`). The 4,096-neighborhood indexed geometry oracle and full Redpiler suite passed; longer large-build comparisons remain necessary. A further candidate treats geometry as fixed only at projection cells which no token or piston can occupy; its performance and semantic checks are pending.

That fixed-cell folding candidate screened at 1,390.2 TPS over 1,000 ticks (`target/zcore-shape-static-plan-screen.log`), versus 1,439.8 in the earlier geometry-reference screen, with the same screen hash. These unpaired single runs cannot establish a regression, but demonstrate no gain sufficient to justify the additional invariant; the candidate and its temporary test were removed.

### 2026-11-02 — stopped experiment and checkpoint decision

The user stopped this implementation and requested a return to a known-good checkpoint while preserving RedstoneDB work. Candidate checkpoint `143f1940` is the parent of `2bbbcb4f`, which introduced the ordered wire-shape/movement notification implementation. It is being checked in the separate `F:/rustrepos/MCHPRS-zcore-recovery` worktree before choosing the rollback. This document is retained across the rollback; older paragraphs describe historical code and tests, not necessarily the restored checkout.

The clean `143f1940` checkout was then verified with `MCHPRS_BENCH_PUBLISH_ALL=1` and `bench_cpus.exe zcore 5000 1 integrated`: all 5,000 interpreter screens matched, with 244 screen changes (`target/zcore-recovery-checkpoint-screen.log`; complete output under `F:/rustrepos/MCHPRS-zcore-recovery/target/redpiler-bench-results/zcore-production.json`). A separate `bench_llvm_zcore.exe 5000 1 --rust-only` run measured 2,716.4 kernel TPS with the interpreter-matching final screen hash (`target/zcore-recovery-checkpoint-kernel.log`). This is one performance sample. The user also requested retaining regression tests; failures that expose missing shape and notification behavior on this checkpoint must remain visible.

The fresh ZCore interpreter/Rust comparison used `MCHPRS_BENCH_PUBLISH_ALL=1` and `target/release/examples/bench_cpus.exe zcore 5000 1 integrated`. It first diverged on the screen at Run tick 1,520, and the final screens differed: interpreter SHA-256 `563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b`; experimental Rust `c29c76db0e30bac4eff223304f237dd409a32fa26a42f8803a860d72df4fc23b`. The interpreter produced 244 screen changes. The command measures per-tick publication as well as ticks and is a correctness check, not the kernel benchmark. Full results were copied to `F:/rustrepos/MCHPRS-experiment-backup-20261102/zcore-interpreter-screen-experiment.json`; the log is `target/zcore-interpreter-screen-current.log`.

The original ANPU issue was separate: strict interpreter/Rust comparison first found equal-deadline repeater FIFO order reversed at tick 218, before reset. A falling torch at `(100,53,124)` was identified as the common source; a neighboring torch at `(100,53,122)` changed the observed callback order in exploratory cropped snapshots. The crop cut connections and omitted original work/entities, so it was not proof of two reachable full-plot contexts. A full-fixture ignored regression was added; it must not be treated as passing on either checkpoint without rerunning it. Source-order sampling currently deduplicates target callbacks and bypasses its replay check for appended sources such as torches. Repeating a contribution cannot recreate an intermediate level because it reads the source's current output; delaying a contribution might preserve some Boolean callbacks but was not enough to resolve ANPU. The rejected staged-lowering experiment failed at tick 22 with reactive hoisting and still failed at tick 218 when run sequentially (`target/anpu-staged-rejected.patch`, `target/anpu-staged-candidate.log`, `target/anpu-staged-sequential-candidate.log`). No context variants or interpreter-style runtime propagation were accepted.

Wire-shape work was motivated by a real ANPU interpreter/Rust wire-side difference at tick 16 during a piston payload landing. Recomputing sides from final geometry failed because cross/dot behavior depends on notification history. Rust therefore added directional shape notifications at extension, retraction, departure, landing and interrupted motion, including ordered observer and wire-neighbor callbacks; a torch-to-piston watcher-order correction moved a strict pending mismatch from tick 150 to 218. The separate LLVM port serialized initial wire sides, fixed target and live geometry references, replayed ordered callbacks and exported sides in snapshots. It passed the two formerly failing wire-side fixtures, a Rust/LLVM ANPU 5,000-tick state comparison, ZCore preparation plus 4,096 Rust/LLVM ticks, and a Rust/LLVM ANPU reset plus 128 interpreted continuation ticks. Those results establish parity only between the two sampled backends for the tested scenarios, not interpreter correctness: the fresh ZCore screen test above invalidates any broader claim.

Performance attempts are recorded in the tables and dated paragraphs above. After the notification changes, three ZCore pairs measured median Rust 398.1 TPS and LLVM 2,514.4 TPS; a saved pre-notification Rust executable measured 2,246.6 TPS on the same machine in one 5,000-tick run. The old and new screen hashes differed, so their workloads cannot be assumed identical. Both recorded 85,632 routed-token changes and 3,801,225 wire visits; reported work rose from 28,932,024 to 43,227,549. Fixed notification targets and ordered wire-neighbor plans improved median Rust from 403.8 to 784.0 TPS in three alternating pairs; indexed live geometry references and direct cell IDs improved a subsequent median from 770.6 to 1,520.0 TPS. The plan retained about 18.86 MB of additional vector capacity in the ZCore census. A fixed-geometry folding screen showed no demonstrated gain and was removed. None of these experiments met the requested 2,600–3,000 Rust TPS target or established correct ZCore output.

Before rollback, the complete tracked diff against `143f1940` was saved at `F:/rustrepos/MCHPRS-experiment-backup-20261102/experiment-since-checkpoint.patch`; the untracked LLVM `shape.rs` and semantic-17 DLL were saved alongside it. The RedstoneDB module, scraper, fixtures and module declaration are unrelated work and must be preserved. Do not apply the patch wholesale without reviewing it: it includes experimental runtime changes, tests and this document, and the LLVM shape port relies on the Rust behavior being reverted.

The main branch was reset to `143f1940`, and production Redpiler, interpreter and LLVM sources were restored to that checkpoint. The checkpoint DLL was rebuilt with `./scripts/build-llvm-native.ps1` in 557.0 ms (40,960 bytes). RedstoneDB files and their test declaration remain; `cargo test -p mchprs_core --lib redstonedb_levers_match_interpreter` passed. The user requested preserving tests and regressions: the three Rust notification fixtures, the LLVM publication and movement comparisons, the full ANPU FIFO regression and the experimental indexed-geometry oracle remain in the worktree. The indexed geometry helper is test-only and is not used by the checkpoint runtime. Shape-table corruption cases remain conditional on that ABI existing; the retained wire-side ABI test explicitly fails while it is absent.

With the rebuilt checkpoint DLL explicitly selected, `cargo test -p mchprs_core --lib redpiler::` produced 176 passed, five failed and 17 ignored (`target/zcore-rollback-redpiler-tests.log`). The five visible failures are the retained torch watcher-order, empty-piston observer FIFO, piston-landing wire-shape, LLVM wire-side ABI, and LLVM wire-shape publication/reset regressions. They are known limitations of the restored checkpoint, not newly fixed or skipped. The ZCore screen and 2,716.4 TPS results above are separate from these failing wider-correctness checks. The active LLVM parity/performance goal is paused.

The user prioritized matching ZCore's interpreter screen and recovering roughly 2,600–3,000 Rust TPS. A saved pre-notification Rust executable rerun on the current machine measured 2,376.3 TPS over 1,000 ticks and had the same final screen hash as the current geometry-plan screen (`target/zcore-historical-current-machine.log`). At 5,000 ticks it measured 2,246.6 TPS and had screen hash `563a79fb...`, whereas the current candidate's 5,000-tick screen hash is `c29c76db...` (`target/zcore-historical-work-census.log`). Equal screen at one tick does not prove equal internal state; the later screen difference shows that throughput cannot be interpreted independently of circuit behavior. Compare both outputs to the interpreter and use operation counters to investigate the remaining overhead.

The same 5,000-tick run with `MCHPRS_REDPILER_PHASE_TIMINGS=1` and `redpiler-trace` recorded identical routed-token changes (85,632), routed readers (85,632), and wire visits (3,801,225) in the old and current Rust builds, while total reported work rose from 28,932,024 to 43,227,549 operations (`target/zcore-historical-work-census.log`, `target/zcore-current-work-census.log`). The 14,295,525 additional work units are consistent with the added notification behavior; the counters alone do not assign every unit or prove identical internal circuit state. The current shape-target, wire-callback and geometry vectors retained approximately 18.86 MB of capacity on this run. The current census overlapped a build and is not a throughput measurement.
