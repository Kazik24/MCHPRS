# Current production FPU inference

## Workload and checks

The 2026-10-20 benchmark uses the existing production `instant` harness with
`test_data/piston-research/FPU_MROWW.schem`, SHA-256
`88dc95cd6e048561a4fc2fe8f53dbecbc81e8adfda108eb27ba8e12966c0a5dc`.
The saved trigger lever at local `(6,36,76)` must attach to green wool at
`(6,36,75)`; the harness checks its support block.

All modes use xorshift64(13,7,17), seed `0x19a4110b5eed`, 35 operand/opcode levers,
64 initial ticks and eight warm vectors. Each of 32 measured vectors sets the
trigger OFF, applies changed levers in manifest order, holds OFF64, then ON64.
There are 4,096 timed ticks and 5,184 total ticks before handoff. Input actions
and ticking are timed; loading, compilation, warm-up, output observations,
projection and continuation checks are excluded. BubbleSort's existing reported
window times ticking only, so the two devices have different stimulus policies.

The compared modes are native interpreter, production O5, and production O5 with
stable inputs. O5 excludes the guard-packing experiment that regressed BubbleSort.
The harness uses an 8x compilation budget; server budgets are controlled by rank.
A fresh interpreter capture supplies the same input stream and per-tick output
trace. Compiled runs validate output traces independently through O0 and their
selected mode. After timing, reset exports graph-owned future work to the World;
blocks, entities, scheduler and semantic piston state are compared with the native
final checkpoint. Another 16 interpreter ticks must match the native continuation.
Checkpoint chat fields are empty metadata, not a command-message oracle.

The first stronger check rejected the raw piston digest alone: blocks, entities,
scheduler and outputs matched. That digest includes interpreter-private motion
identities, their allocation counter and `scheduled_advanced`. The replacement
comparison follows existing continuation tests: it requires `BetweenTicks` with
empty movement work and compares the logical tick, phase, ordered events and
ordered motions including position, progress, previous progress, last tick and
carried entity. It retains raw hashes for diagnosis. Identity allocation and the
scheduled flag are not compared at this boundary; the latter is overwritten when
the next tick begins. Physical state and semantic pending work retain hard
equality gates.

Graph counters include initialization, warm-up and measured execution. They are
captured before reset. Five rounds rotate mode order. No builds, tests or
profiling run during the timing batch. The production executable uses release
optimization and fat LTO; source and executable hashes are retained under
`target/fpu-current-production/semantic-provenance.json`. The final executable
is `instant-semantic.exe`, SHA-256
`23DDBE668F68F9AFC58E53A4B1F18840A86A39B575BA860921CB94549BC27B26`.

The native output bank remains `0xffff`, so this workload measures **switching
cost**, with raw TPS reported separately from verified arithmetic throughput.
Whole-world and continuation checks prevent matching a constant output alone
from being treated as equivalence.

## Results

All five rotated rounds pass. Raw runs, preflight checks, the independent
`verified-summary.json` and the analysis are stored in
`target/fpu-current-production/semantic-measurements/`.
The earlier raw-digest failure remains under
`target/fpu-current-production/measurements/`.

| Mode | Median seconds | Median switching TPS | TPS range | Ratio to interpreter median |
|---|---:|---:|---:|---:|
| Interpreter | 10.583794 | **387.01** | 382.91–406.23 | 1.00x |
| O5 | 3.195677 | **1,281.73** | 1,177.18–1,638.65 | 3.31x |
| O5 + stable inputs | 3.039691 | **1,347.51** | 1,222.42–1,502.23 | 3.48x |

| Round | Interpreter seconds | O5 seconds | O5 + stable seconds |
|---|---:|---:|---:|
| 1 | 10.697126 | 3.347644 | 3.132194 |
| 2 | 10.688477 | 2.851522 | 2.726615 |
| 3 | 10.583598 | 2.499613 | 2.740952 |
| 4 | 10.583794 | 3.479494 | 3.039691 |
| 5 | 10.082875 | 3.195677 | 3.350728 |

Median paired inference-time reductions against native are **68.71%** for O5
and **71.28%** for stable-input O5. Stable mode wins three of five rounds against
ordinary O5, with a **4.38%** median paired reduction. Individual differences
range from 9.66% more time to 12.64% less time. This is a modest, variable
comparison; the evidence does not establish that stable mode wins every run.
Ratios of median seconds differ from medians of paired ratios: stable mode's
ratio-of-medians reduction against O5 is 4.88%.

The fastest single compiled run is ordinary O5 at **1,638.65 TPS**. It is not
the five-run median. Preflights (384.46 / 1,417.14 / 1,334.37 TPS) are excluded
from the timing summary. Compilation and unrelated host activity can change
cache/frequency conditions between runs; no particular cause of the observed
spread was isolated.

The independent checker verifies all 15 reports against the fresh native input
vectors, per-tick output trace, block/entity/scheduler checkpoints, semantic
piston future and 16 native continuation ticks. Work counts repeat exactly in
each mode; scheduled ticks, piston actions and deferred publications also agree
between O5 variants. All frozen executable and recorded source hashes match.

## Algorithms used

O5 compiles bounded exact transport episodes into ordered `SignalOp` programs:
`Capture`, `Select`, `SetPower` and `Call`. Before replay, it checks the captured
wire/control inputs that affect the episode. If the preflight does not match, it
runs the dense transport executor for that episode before applying optimized
effects. On a match, it replays the saved operations, retaining power writes and
boundary calls at their original sampling cuts. Transcript minimization removes
known-false selections and unused captures; unknown boundary admission remains
live. The implementation is in
[`transitions.rs`](../../../crates/core/src/redpiler/backend/graph/transitions.rs),
with the operation definitions and shared minimizer in
[`signal_transitions.rs`](../../../crates/core/src/redpiler/program_graph/signal_transitions.rs)
and the static channel compiler in
[`signal_channels.rs`](../../../crates/core/src/redpiler/program_graph/signal_channels.rs).

The CPU predicate path short-circuits small typed predicates and uses scalar or
AVX2 clause-mask comparisons for larger eligible predicates. Vector comparisons
stay within a pure query; they do not reorder transport operations. Stable
instant logic is enabled only by `--stable-instant-inputs`. It evaluates compiled
pure power expressions under that input-stability promise, then retains ordered
boundary effects and deferred state handling. Internal reads come from graph
state and compiled typed inputs rather than consulting the original World as a
power oracle. See
[`stable_instant_logic.rs`](../../../crates/core/src/redpiler/backend/graph/stable_instant_logic.rs),
[`cpu_kernels.rs`](../../../crates/core/src/redpiler/backend/graph/cpu_kernels.rs)
and the [O5 pass pipeline](../../../crates/core/src/redpiler/passes/mod.rs).
These passes reduce physical walks and replay work, but many callbacks and piston
samples remain. They do not remove the whole physical graph or claim complete
compiled-circuit execution.

## Reproduction

Use the frozen executable and the native report as the reference. The benchmark
command below reproduces the measured random workload; change the mode flag for
the other two runs:

```powershell
$exe = ".\target\fpu-current-production\instant-semantic.exe"
$common = @("--component", "fpu_legal", "--changing-inputs", "--workload", "random", "--seed", "28192451288813", "--input-every", "64", "--episodes", "32", "--iterations", "1")
& $exe @common "--interpreted" "--output" "target/fpu-current-production/semantic-measurements/native-reference.json"
& $exe @common "--optimize" "--reference" "target/fpu-current-production/semantic-measurements/native-reference.json" "--output" "target/fpu-current-production/semantic-measurements/o5.json"
& $exe @common "--optimize" "--stable-instant-inputs" "--reference" "target/fpu-current-production/semantic-measurements/native-reference.json" "--output" "target/fpu-current-production/semantic-measurements/o5-stable.json"
```

The default `--input-every` is 1, so keep the explicit 64 to reproduce this
protocol. The harness also offers `random-walk` and `sequence`; those workloads
were not measured in this report. Historical reports for the user's earlier
roughly 2,000 TPS results were not found, so their workload settings are unknown.

## Remaining work

These are algorithmic counts over all 5,184 graph ticks, not a CPU-time profile:

| Work | O5 | O5 + stable inputs |
|---|---:|---:|
| Callbacks | 19,690,741 | 18,797,375 |
| Piston samples | 8,053,639 | 7,510,648 |
| Piston actions / deferred publications | 714,270 | 714,270 |
| Transition replay operations | 23,532,838 | 19,954,588 |
| Remaining dense wire visits | 906,320 | 770,367 |
| Transition-elided dense wire visits | 15,194,460 | 11,447,713 |
| Stable-logic invocations | 0 | 1,081,387 |
| Stable-logic wire-expression evaluations | 0 | 4,405,149 |
| Stable boundary calls | 0 | 510,541 |

Transition replay removes 94.37% of its dense visits in ordinary O5 and 93.69%
in stable mode. Stable logic bypasses additional transport entries; these
percentages have different denominators and should not be compared as total
propagation coverage. Fewer counted operations alone are not a runtime cost
estimate. Millions of retained callbacks, captures and samples remain candidates
for further reduction. Stable logic also precedes cached transcript replay, so
generic per-net cost selection deserves measurement rather than assuming every
eligible Boolean program is cheaper.

The next larger candidate remains compiled boundary effects that eliminate
effect-free callbacks and captures while preserving piston sampling cuts,
first-request provenance and pending work. Whole-circuit graph replacement is
unfinished; this benchmark does not establish a floating-point arithmetic rate.

The retained older reports show 584 TPS for maximum compilation against 168 TPS
interpreted, and 640 to 695 TPS in a separate CPU-packing comparison. Those use
four warm vectors and a non-LTO test harness; they are not paired with this new
production batch. The earlier 1,000–2,000 TPS implementations mentioned by the
user have not yet been located in retained artifacts.
