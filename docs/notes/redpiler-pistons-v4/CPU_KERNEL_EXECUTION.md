# CPU kernel execution

`optimize-instant-kernels-cpu` is a separate, experimental O4+ stable-input
builder, selected by Cargo feature `experimental-cpu-kernels`. That feature
also selects the existing stateful event-kernel family. It runs after event
composition and consumes the certified kernel entries and original power graph.

The new program replaces repeated pure power-query and acceptance-predicate
traversal. It does not replace dynamic dust propagation or execute an entire
kernel as a simultaneous Boolean operation. CircuitState and GraphFuture remain
the authoritative machine and pending work. No cached shadow circuit is created.

## Representation and execution

Each admitted actor has one flat descriptor containing separate Main, Quasi and
acceptance predicates. Each predicate refers to contiguous typed-read and clause
ranges. A pure query samples at most 64 live Boolean operands into one `u64`.
Parallel required/forbidden mask arrays then implement DNF:

```
any clause: (sample & required) == required && (sample & forbidden) == 0
```

The compiler folds constants, removes contradictory and duplicate clauses,
removes subsumed small clauses, eliminates unused reads, and groups reads by
state owner for locality. A dense actor-to-descriptor index avoids searching or
cloning the event kernel's Arc for an admitted action. The hot representation
has no per-actor operand allocations. There is no new dependency.

AVX2 matching compares four 64-bit clauses at once. CPU/OS detection occurs when
the backend is created. Short predicates and tails use scalar word operations;
CPUs without AVX2 and other architectures use the portable scalar matcher.
AVX2 is confined to a private checked wrapper and full four-lane slice loads.
The read gather still reads the original typed state. SIMD does not batch piston
effects, cross FIFO boundaries or move capture times.

## Semantic boundaries

- Main is evaluated before Quasi, preserving the original positive-Main short
  circuit. Ordered contact variants retain first-match selection, including
  earlier non-emitting variants and the no-selected-profile case.
- Strength reads sample live delivered wire/component strength with the original
  attenuation threshold. Wires are never expanded into equilibrium equations.
- Only immutable source orientation/profile facts fold. Mutable component/contact
  properties use the original pure guard or source query when specialization is
  unsupported. Legacy classic-net ownership uses original input-query fallbacks.
- Each admitted action still checks ready/extended state and pending interrupted
  resource extensions, then commits through the existing mutation helpers.
  Request order, source witnesses, callbacks, scheduled deadlines, revisions and
  deferred effects retain their owners. No request is removed or reordered.
- Expansion is bounded to 64 reads, 64 clauses and 16,384 construction work units
  per predicate. Oversized power predicates retain exact Main/Quasi input reads;
  unsupported acceptance entries retain their original event program. Retained
  CPU tables are capped at one eighth of the graph preparation memory limit.
- Publication is typed. Rewrites clear the plan; replacing event plans clears
  CPU plans. Validation independently rebuilds the CPU program from its source
  graph and rejects stale or forged data before lowering.

The shared JSON export includes the actual flat program under `cpu_kernels`
and capability `cpu_kernel_execution`. Masks are 16-digit hexadecimal strings,
preserving bit 63 through JSON, Python integers and JavaScript BigInt. Export,
loader and viewer validate references, masks and descriptor ranges.

## Verification and measurements

Focused tests cover scalar/AVX2 equivalence through all tails and high bits,
truth-table preservation after read remapping, ordered contact priority,
negated missing profiles, cancellation and forged plan rejection. Packed
ADDER11BIT runs all 40 pinned native arithmetic histories and six partial-cycle
interpreter handoffs. Randomized Adder/Counter snapshots compare Main and Quasi
separately over live strengths, resource locations/enabled flags and all piston
availability states. Recorded execution also compares optimized live power reads
with the original evaluator at each sampled cut.

The isolated FPU benchmark compares current event kernels plus BUD/capture
reductions against the same graph with CPU packing alone. It reuses the pinned
`FPU_MROWW.schem` and green-wool trigger, random seed `0x19a4110b5eed`, four warm
vectors and 32 measured OFF64/ON64 vectors. Correctness includes 4,672 native
ticks, scheduled/queued/deferred work and interpreter handoff. Five alternating
release pairs require exact authoritative state, complete GraphFuture and work
count equality. Inference uses an empty command-effect host without original
circuit blocks. Native outputs remain `0xffff`: timing measures switching cost,
not useful arithmetic operations per second.

The accompanying [TPS report](CPU_KERNEL_TPS_REPORT.md) compares the interpreter
with the maximum compatible pipeline on all five requested devices and records
the exact selected passes. Known incorrect fixed-point experiments are excluded.

The measured FPU CPU program covers all 7,804 event actors. It contains 15,554
strength reads, 11,730 state reads and 4,337 retained source-query fallbacks.
Of 23,412 Main/Quasi/acceptance descriptors, 487 have four or more clauses and
can use AVX2. Most descriptors execute scalar word operations. This is a compact
live-query program over the graph's state; the remaining source queries and
dynamic wire machine still contribute inference cost.

Five isolated release pairs measured **6.396827 → 5.897059 s median** for
4,096 measured ticks: **7.8128% less inference time, 1.08475x throughput**.
Every pair improved. The baseline already has event kernels, passive capture
minimization, BUD reads and the compatible single-wire pass; CPU packing is the
only candidate change. All pairs preserve complete GraphFuture and work counts.
The pass run took 0.313 s in this diagnostic pipeline. Its 24,313 mask pairs,
31,621 reads and 7,804 descriptors contain 1,080,620 bytes of logical packed
payload (1.03 MiB), excluding dense actor bindings, Vec spare capacity and cold
original graph/kernel tables. Runtime retains a dense binding per physical actor.

| Pair | Existing event kernels, seconds | With CPU packing, seconds |
|---|---:|---:|
| 1 | 6.396827 | 6.120129 |
| 2 | 6.175133 | 5.766160 |
| 3 | 6.319363 | 5.558152 |
| 4 | 6.577737 | 5.897059 |
| 5 | 6.565385 | 6.172962 |

Raw evidence: `target/cpu-kernels/fpu-random-paired.json`, `fpu-summary.json`,
`fpu-isolated.log` and `fpu-packed-kernel.json`. The isolated pairs are a separate
graph-versus-graph batch from the interpreter TPS matrix, with different warmup
context and desktop-host activity. Their paired percentage isolates this pass;
cross-batch absolute TPS does not isolate an additional optimization.

Validation results: 553 redpiler tests passed with the combined event/CPU/wire-node
features; the final frozen executable also passed all six focused CPU tests.
Python loader tests: 33 passed. Viewer tests: 39 passed. Workspace checking
passed with the combined features and two pre-existing fixed-classic dead-code
warnings. The actual packed Counter graph was loaded through the Python loader.
