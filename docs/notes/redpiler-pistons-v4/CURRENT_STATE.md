# Redpiler current state

The [fresh packed-kernel ETW profile](CPU_FPU_PACKED_KERNEL_PROFILE.md) measures
current production O5: propagation owns 68.2% of BubbleSort and 63.8% of FPU
inference samples. Capture and general route/contact guards remain the largest
targets; kernel predicates are about 4.6–4.8%. This supersedes the older runtime
profile's singleton-kernel census for current performance planning.

The certified passes are now enabled in normal production
[O1–O5 tiers](../../REDPILER_OPTIMIZATION_LEVELS.md). O4 selects connected
live-input event kernels, BUD reads, passive capture elimination, closed wire
transfers and certified local classic nets; O5 adds CPU packing. No experimental
Cargo features or stable-input promise is needed for O4/O5. The measurements below
refer to the frozen configurations recorded in their reports before promotion.

The separate [CPU kernel packing pass](CPU_KERNEL_EXECUTION.md) compiles live
kernel power/acceptance predicates into contiguous reads and minimized bit-mask
clauses, with AVX2 and scalar execution. It preserves original FIFO effects and
dynamic wire state. [Interpreter versus maximum-compilation TPS](CPU_KERNEL_TPS_REPORT.md)
is measured on ADDER11BIT, Counter, Divider, FPU_MROWW and complete BubbleSort.
Five isolated FPU pairs show 7.81% less inference time from CPU packing alone.
The maximum compatible pipeline reaches 194.2 TPS on complete BubbleSort versus
58.7 interpreter TPS (3.31x), with all five full sorts and pending-work checks
passing. FPU remains a switching diagnostic rather than arithmetic throughput.

The experimental [stateful event compiler](STATEFUL_INSTANT_KERNELS.md) now
composes ADDER11BIT, Counter, Divider, FPU and BubbleSort into one instant kernel
each. FPU covers 7,804 instant actors, with 296 ordinary pistons outside it.
Separate BUD-read and passive-wire-capture reductions retain original state and
FIFO ownership. FPU's 4,672 random-input native ticks and handoff pass. Five
alternating release-harness pairs measure 8.622 → 6.897 s (20.0% less time), with
LTO disabled. Its native output stays `0xffff`, so this is switching cost rather
than arithmetic throughput; complete wire-free circuit replacement remains open.

The separate O3+ [wire-capture compilation pass](WIRE_CAPTURE_COMPILATION.md)
preserves work counts and full BubbleSort correctness. Five release-harness
pairs show small median changes (about 1.2% CPU and 1.3% FPU), with substantial
timing variation; this does not establish a large speedup. These earlier timings
exclude the newer stateful event and passive-capture reductions.

A broader [closed-network and composition investigation](INSTANT_NETWORK_BOUNDARY_EXPERIMENT.md)
now identifies multi-wire instant-only islands and tests their contraction.
The largest candidate groups reach 10,228 CPU actors and 5,747 FPU actors.
A single 16-wire net produces wrong CPU resource positions at tick 26 when
collapsed to settled values. The next boundary must preserve ordered requests
and reset events; the separate unsafe mode stays outside normal execution.

The opt-in [experimental instant wire-node pass](EXPERIMENTAL_INSTANT_WIRE_NODES.md)
now replaces eligible closed single-wire walks with ordered signal transfers.
Three release-harness pairs show **no speedup**: CPU 7.194 -> 7.289 s per
1,000 ticks; FPU diagnostic 0.571 -> 0.585 s per 256 ticks. It remains off by
default. Multi-wire nets and broader reset elision remain outside this slice.

## BubbleSort/FPU profiling and wire-shape pass, 2026-10-19

The separate O3+ `compile-wire-shapes` pass reduces full O5 BubbleSort inference
from 66.924560 to 57.690753 seconds across three clean release pairs: **1.160x
throughput**, with identical RAM, checkpoints and execution counts. A pair that
overlapped an external compiler is retained separately and replaced explicitly.

Fresh frozen-source profiles put Turbo propagation at about 68% of CPU inference
and 61% of FPU diagnostic inference. All 2,197 CPU and 2,360 FPU installed kernels
are singletons with zero owned wires. Next: a separate wire-capture descriptor
pass, followed by active signal-arrival proofs that actually eliminate walks.
The FPU raw-mask experiment has constant output and a piston-digest mismatch;
it establishes neither numerical throughput nor complete state equivalence.
See the [profile, final graph census and ranked next steps](CPU_FPU_FINAL_GRAPH_PROFILE.md)
for source provenance, correctness scope and reproducible artifacts. Concurrent
main-tree changes after this frozen snapshot are outside these measurements.

## BubbleSort O5 execution update, 2026-10-19

The complete 10,036-tick sort now passes with the queued Instant entry extension.
Actual kernel invocations rise from 3,565 to 524,910 with the same 2,197 installed
kernel actors. Captured requests retain their actual Turbo route and FIFO actor;
live acceptance, resource ownership, reset and unsupported-entry fallback remain
explicit. Mixed wire-side queries also avoid unnecessary neighbor reconstruction.

Three interleaved production release pairs reduce median inference from
73.149302 to 65.166980 seconds: **1.12249x throughput**, 137.20 to 154.00 active
game ticks/s. All RAM ticks and final/reset/native-continuation checkpoints match.
A separate O5-default control is 65.109283 seconds, so this does not establish an
Instant-only gain. General wire walks remain 3,882,011; whole-CPU contraction is
still incomplete. See the [current timing and proof scope](CPU_BUBBLESORT_RUNTIME_PROFILE.md#2026-10-19-complete-o5-sort-with-queued-instant-entries-and-lazy-geometry).

Final validation passes 527 Redpiler tests (36 ignored), two wire-geometry tests,
26 Python loader tests, 32 viewer tests and the workspace check. The separately
selected FPU replay passes all three histories: 480 native tick pairs and 16
native handoff ticks per case. This does not certify floating-point arithmetic.
See the [suite log](../../../target/bubblesort-final-redpiler.log) and
[FPU replay](../../../target/bubblesort-final-fpu.log).

## Instant boundary update, 2026-10-19

This update supersedes the analysis-only and zero-kernel claims in the earlier
review below. Production O4+ with `--stable-instant-inputs` installs supported
layered kernels inside Graph. Recognition still does not establish whole-device
coverage or a CPU speedup; see [current CPU proof cuts](CPU_BUBBLESORT_KERNEL_PROOFS.md#follow-up-implementation).

Optional per-region construction work/table exhaustion now records
`ProofBudgetExceeded` and leaves the complete region on Graph. Partially built
symbolic waves are discarded. Invalid graph references, arithmetic overflow and
cancellation remain fatal; the work and retained-table limits are unchanged.
Rust export validation, the Python loader and the viewer accept the exported
`proof_budget_exceeded` reason while retaining their reference checks.

The FPU layered replay now compiles: 2,412 symbolic actors and 2,360 installed
singleton kernels. Two regions containing 35 and 15 actors exhaust the proof
budget and remain on Graph. All three histories pass 480 Graph/native tick pairs,
full snapshots after every operation and 16 native handoff ticks per case.
This verifies the tested input/update/reset histories, not numeric floating-point
results or a performance gain. The 100-active-tick BubbleSort replay also passes,
including 16 handoff ticks, with the previous 2,197 installed kernels.

Evidence: [FPU replay](../../../target/instant-boundary-fpu.log),
[FPU coverage](../../../target/layered-instant-devices/fpu.json),
[CPU replay](../../../target/instant-boundary-cpu.log).
The final frozen test build passes 519 Redpiler tests (35 ignored), including
budget/cancellation and partial-wave regressions; the two device replays above
were selected explicitly. Workspace check, 25 Python loader tests and 31 viewer
tests pass. [Full suite log](../../../target/instant-boundary-redpiler-tests.log).
Next: profile construction in the two budget-limited regions and extend active
corridor/reset arrival proofs without replacing callback order with settled power.

## Earlier review of `9831f7e`

Reviewed 2026-10-19 against `9831f7e` and the working tree. Concurrent layered-device
research was already present and continued during review. Performance below comes
from identified retained executables, not a new benchmark of this changing checkout.

Graph is the only compiled executor. It runs substantial classic and admitted piston
circuits correctly, with a much faster fixed-circuit path. The proposed layered instant
compiler is still analysis-only: it has no live kernel execution or demonstrated speedup.

The [compilation and runtime profile](COMPILATION_AND_RUNTIME_PROFILE.md) adds a later
O2/O5 investigation: repeated callback-link validation dominates compilation; ordered
wire work and dependency/property reads dominate execution. Its individual current O5
measurements supplement the paired historical benchmarks below, not an O5 speedup claim.

## What runs today

| Path | Current capability | Important limit |
| --- | --- | --- |
| General Graph | Typed electrical/callback graph, ordered propagation, stationary-piston/resource execution, shared scheduler, projection and interpreter handoff | Conservative admission; not arbitrary Minecraft piston movement |
| O1 | Guard/callback pruning, Turbo indexing and certified dust-path plans | Retains observable ordering and general execution where plans decline |
| O2 | Wire queries/hops/callback plans, fixed signals and dense whole-fixed classic execution | Whole-fixed compilation requires an entirely supported contactless circuit without pistons, resources or private owners |
| O3 and O4 | O2 plus instant-piston/reset specialization, built-in reset clocks, region/proxy plans | O4 adds no pass beyond O3; recognition is not whole-device contraction |
| O5 | O3 plus equivalent delayed-update coalescing before plan construction | Captured/conditional deliveries, Turbo work, locking risks, immediate effects and piston/BUD boundaries constrain eligibility |
| `--classic-redstone`, O2+ | Optional local committed-signal nets inside mixed circuits | One-ingress/boundary proofs and runtime guards limit useful active coverage |
| `--stable-instant-inputs`, O3+ | Existing stable-input region composition | Caller promises input stability through reset; no runtime protocol checker |
| Layered instant compiler | Test-only boundaries, cell contracts, connected tracing, expression reduction/evaluation | Absent from production pipeline; no live effect/reset integration |

`CompilerOptions::default()` selects **O0**. Bare `-O` and `--optimize` select all
configured tiers, including O5; neither implicitly enables the two explicit opt-ins.
The diagnostic name `experimental-coalesce-delayed-updates` survives its O5 promotion.
The unsafe fixed-point Cargo feature remains a separate, deliberately incorrect experiment.

Sources: [options](../../../crates/core/src/redpiler/options.rs),
[pipeline](../../../crates/core/src/redpiler/passes/mod.rs),
[whole-fixed admission](../../../crates/core/src/redpiler/program_graph/fixed_classic.rs).

## What can compile

CHUNGUS admits all **251,485 actors and 133,623 wires** into the whole-fixed executor.
It still executes ordered wire steps and delayed work. Zero *general* wire walks does
not mean zero wire work, a synthesized Boolean CPU, or a queue-free executor.

The fixed executor supports wires, torches, repeaters including locking, comparators,
lamps, hoppers, levers, buttons, pressure plates and constants. Other otherwise-supported
Graph components can make this whole-program optimization decline. General Graph and
the interpreter are distinct: an optimization decline retains Graph execution; a graph
admission failure is not successful compilation through hidden fallback.

ANPU and CPU_BubbleSort run through the mixed general/specialized Graph path. ADDER1BIT
recognizes all 13 piston mechanisms, including shared reset drivers, and retains eligible
private proxies. These successes do not establish a new layered whole-adder kernel.

Graph admission checks piston heads, contact geometry, footprint/support closure and
movement metadata. Ordinary pushes, possible longer pushes, unsupported transported
materials, unknown occupancy and invalid entry work can reject a selection. Trapdoor
redstone behavior is explicitly unsupported. Raw PM1 reports currently reject the original
save for missing moving-piston metadata and the fresh save for a missing piston head;
the corrected PM1 report was unreadable during review, so no success is claimed for it.
These are fixture/Graph admission issues, separate from instant-layer proof failures.

Sources: [admission](../../../crates/core/src/redpiler/analysis/mod.rs),
[geometry](../../../crates/core/src/redpiler/analysis/geometry.rs),
[recognition](ADDER1BIT_INSTANT_RECOGNITION_RESULTS.md),
[device reports](../../../target/layered-instant-devices/).

## Measured speed and compilation cost

These are **active game ticks per second**, not redstone ticks, client frame rate or
whole-server TPS. Production CPU measurements exclude loading, compilation, activation,
projection/reset and verification. No per-tick display flush occurs in their timed loop.

| Workload and retained measurement | Inference result | Compile cost and interpretation |
| --- | --- | --- |
| CHUNGUS O2, stage 2; five alternating pairs, 16,384 ticks each | **3,283.65 ticks/s**, median **4.989565 s**, versus **2,139.18 ticks/s / 7.659017 s** control | **1.535x throughput**, 34.85% less inference time; candidate median preparation **25.336 s** |
| ANPU O2, regional comparison; three alternating pairs, 5,000 ticks each | General **1,694.5 ticks/s**; local classic enabled **1,652.9 ticks/s** | Preparation **7.870 s -> 16.404 s**; overlapping inference ranges, no demonstrated gain |
| BubbleSort full sort, profiling build on 2026-10-18; O4 plus both opt-ins | **10,036 ticks / 80.509886 s**, approximately **124.7 ticks/s** | Preparation **16.444 s**, projection **8.279 s** separate; sampled older build, not current O5 throughput |
| Production O5 | Retained bounded CHUNGUS/ANPU equivalence and complete BubbleSort RAM trace pass | No isolated production O4/O5 timing comparison |
| New layered kernels | **Zero executions** | No runtime speed measurement yet |

CHUNGUS stage 2 used release/fat LTO on Windows x64, Ryzen 9 5950X, pinned to logical
CPU 4. All ten measured endpoint state/future checks and 32-tick interpreter continuations
passed. Counts were unchanged: 76,786,878 callbacks and 1,379,485 scheduled component ticks
per sample. This gain comes from cheaper exact execution, not discarding that work.

The shared component row shrank **104 -> 16 bytes** by boxing cold command entities.
CHUNGUS executable tables shrank approximately **100.10 -> 73.59 MB**, with route
instructions **2,517,665 -> 474,520**; O5 retains **448,681**. Original Graph tables remain
for inspection/projection, so these are payload measurements, not total process RSS.

ANPU's optional local compiler initially admits **266 nets / 2,441 wires (7.02%)**.
Only **670 wires (1.93%)** remain specialized at tick 5,000 after runtime guard fallback.
The whole window executes only **8 net entries / 18 receiver operations**, eliminating
8 of 183,751 general walks. Static admission barely reaches active work. Earlier claims
of zero ANPU local-net coverage are superseded; the measured speed limitation remains.

Evidence: [paired CHUNGUS summary](../../../target/chungus-stage2-summary.json),
[paired ANPU summary](../../../target/anpu-regional-summary.json),
[BubbleSort profile](CPU_BUBBLESORT_RUNTIME_PROFILE.md#2026-10-18-current-full-sort-with-all-inference-flags),
[classic progress log](CLASSIC_REDSTONE_PROGRESS.md).
Historical C/LLVM speed results belong to removed code and are excluded from current throughput.

## Layered instant compiler progress

Steps 01-03 provide contracts/oracles, a supported local-cell family and typed boundary
discovery. Step 02 still lacks broader reset families. Step 04 has certified directly
connected two-cell examples and a narrow straight-cable arrival certificate. Step 05
has backward expression slicing and an allocation-free evaluator. Steps 06-08, live
execution, reset/handoff, broad performance validation and old-path retirement, remain open.

For example, `INSTANT_RESET_REDSTONE` has no observer and receives no cells from the
observer-provenance family. It needs a separate feedback-reset contract. Concurrent
working-tree edits are broadening observer orientation/input-port recognition; the
retained device admission counts below predate that work and are not its validation.

Two parsed examples reduce expressions **442 -> 143** and **316 -> 99**; the latter
removes two internal wire operands. Their reduced programs are compiler artifacts.
Native comparisons exercise existing Graph execution, not application of reduced effects.
ADDER1BIT and ADDER11BIT still fail the broader connected arrival proof.

Retained large-device reports distinguish admitted local actors from compiled regions:

| Device | Locally admitted actors / all piston actors | Compiled layer regions | Existing Graph/native check |
| --- | ---: | ---: | --- |
| Counter | 106 / 163 | 0 | 126 ticks across 2 cases |
| Revised PC counter | 307 / 500 | 0 | 129 Graph/native ticks; 128 frozen-output ticks |
| Divider | 1,099 / 2,055 | 0 | 2,172 ticks across 11 cases |
| FPU | 5,078 / 8,100 | 0 | 480 ticks across 3 cases |
| CPU_BubbleSort | 7,835 / 16,561 | 0 | 100 active ticks; this particular check is not a full sort |

All report `SignalArrivalUnproven` and `runtime_replacement: false`. Local actor
admission is neither arithmetic correctness nor a compiled/executed percentage of the device.
The separate production O5 BubbleSort reference run covers all 10,036 RAM samples.

Evidence: [step 04](layered-instant-compiler/steps/04-regions-and-layers.md),
[step 05](layered-instant-compiler/steps/05-reduction-and-lowering.md),
[device reports](../../../target/layered-instant-devices/).

## Work still required

1. **Mixed fixed execution:** lower supported regions of ANPU/BubbleSort while preserving
   exact live contact, piston, BUD, observer and scheduling boundaries. The whole-program
   fixed admission gate is the immediate implementation limitation.
2. **Layer arrival and shared resources:** prove callback/request ordering across broader
   wire corridors, multiple inputs and token owners. Stable final power alone is insufficient;
   existing adder/FIFO counterexamples must remain valid.
3. **Exclusive layer runtime ownership:** bind reduced outputs, ordered effects and accepted
   actions into Graph; eliminate covered internal execution without losing boundary events.
4. **Reset and handoff:** discharge reset/token obligations, support recurring waves, materialize
   at certified cuts and preserve exact pending work on interpreter continuation.
5. **Passive-net contraction:** remove proven unnecessary propagation, rather than only
   making exact visits cheaper. Multi-ingress direct transfers and ordered receiver programs
   remain future work; SIMD/bitplanes have no delivered implementation or measurement.
6. **Finish validation and measure:** current O5 paired production timing, useful large-device
   layer coverage, full CHUNGUS application completion and server/display throughput.
   The 12,411,975-tick CHUNGUS completion has not been rerun on this implementation.

The mixed BubbleSort profile attributes 81.01% of older inference samples to the union
of Turbo execution and wire-side calculation. This supports investigating wire work, but
those percentages are not a fresh profile of packed CHUNGUS or O5.

## Documentation disposition

Nine superseded reports move to `scraps/docs/redpiler-pistons-v4/`; their evidence is preserved:

| Archived documents | Reason |
| --- | --- |
| `BOUNDARY_DRIVEN_INSTANT_COMPILER.md`, `CLOSED_INSTANT_MACHINE.md`, `COMPACT_INSTANT_KERNEL.md`, `INSTANT_LOOP_COMPILER.md`, `SYMBOLIC_ACTOR_VALIDATION.md`, `SYMBOLIC_VIRTUAL_CIRCUIT.md` | Removed/superseded symbolic, finite-machine and learned-loop approaches; use the layered plan for ongoing implementation |
| `LLVM_NATIVE_INFERENCE_DEMO.md` | Removed C/LLVM executor; historical results and compile-cost lessons |
| `CPU_BUBBLESORT_INFERENCE_PROFILE.md` | Earlier mixed-tier profile superseded by the dated runtime profile |
| `DUST_FIXED_POINT_EXPERIMENT.md` | First solver checkpoint superseded by the investigation and its ordering counterexamples |

Keep the layered plan/steps, fixture coverage and central edge-case register active:
they describe unfinished work with current source and tests, not abandoned proposals.
Keep O3 contracts, wire plans and the dust ordering investigation because they still
constrain execution. Keep `CLASSIC_REDSTONE_PROGRESS.md` as a chronological evidence log;
its newest entries supersede old zero-coverage, experimental-only and timing statements.
Baseline/profile reports remain dated evidence, not current status pages.
`CPU_FRIENDLY_CLASSIC_EXECUTION.md` mixes delivered packing with proposed contraction/SIMD;
its header identifies that split. Recovery commits remain in `ALGORITHMS_AND_RECOVERY.md`.

`scraps/` is Git-ignored by the existing repository policy. The moved reports and archive
index remain local; this summary outside the ignored archive preserves their disposition
and key conclusions.
Their original tracked text is also recoverable with
`git show 9831f7e:docs/notes/redpiler-pistons-v4/<filename>`.

## Verification for this review

The review build passed `cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1`:
**480 passed, zero failed, 30 ignored**, in 154.72 seconds. The ignored large-device and
long-running research checks were not included. Concurrent source edits continued after
that build; the result validates that review snapshot, not every later edit.

`cargo check --workspace --locked` passed, including the server, with two existing
test-adapter dead-code warnings. The local Markdown link check examined **1,781 links**
with **zero newly unresolved targets** relative to the pre-move inventory; it preserves
pre-existing broken historical links and does not validate heading anchors. All nine
archive moves and `git diff --check` passed.

Logs: `target/redpiler-current-state-20261019-tests.log` and
`target/redpiler-current-state-20261019-check.log`. Benchmarks above were read from retained
reports; no new timing run was made alongside concurrent Cargo/research processes.
Local `target/` artifacts are ignored evidence and may be absent from another checkout.
