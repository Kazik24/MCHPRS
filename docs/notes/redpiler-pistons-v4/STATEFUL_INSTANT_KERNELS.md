# Stateful instant kernels and FPU inference

Promoted to normal O4+ production execution, with live inputs and no required
Cargo feature or stable-input promise. See [production tiers](../../REDPILER_OPTIMIZATION_LEVELS.md).
The measurements below record the earlier opt-in configuration of 2026-10-19.

## What composes

| Circuit | Event kernels | Instant actors | Remaining ordinary pistons |
| --- | ---: | ---: | ---: |
| ADDER11BIT | 1 | 142 | 0 |
| Counter | 1 | 146 | 17 |
| Divider | 1 | 2,034 | 21 |
| FPU | 1 | 7,804 | 296 |
| CPU_BUBBLESORT | 1 | 15,200 | 1,361 |

Counter additionally has sixteen independently certified upper-storage/lower-read
BUD pairs. Their read is a direct resource-enabled/position test; their writes
and pending events retain their original Graph owners. Its seventeenth ordinary
piston is a control, not another admitted memory pair.

These are connected **stateful event programs**, not atomic Boolean functions.
Connectivity comes from the complete cell/corridor port graph, including shared
resources. Each actor has its own entry. It consumes only the already-popped
FIFO request, checks live Main/Quasi power and ready/extended state, and evaluates
the remaining resource predicate. Other requests, source routes, reset clocks,
intermediate strengths and revision-checked publications keep their order.

The compiler substitutes the entry assumptions, folds constants, and removes
dead predicate slices. FPU's 7,804 acceptance entries each reduce to one pin.
That does **not** mean its arithmetic reduces to constants: live power queries,
wires and resource movement still carry the computation. A kernel count alone
does not establish whole-circuit replacement or a speedup.

## Separate passes

`experimental-compile-instant-events` builds the FIFO-local programs from the
validated boundary catalog. Direct actor-to-entry indices avoid searching a
large kernel at runtime. A constant predicate needs no interpreter scratch.
Non-ready, extension and interrupted-extension requests keep general handling.
The experimental pipeline skips old symbolic-layer compilation because every
old admitted actor is covered by the new event family.

`experimental-compile-bud-memory-reads` requires an exact private two-contact
redstone resource, one upper sticky storage piston and one admitted lower reader,
with a single unattenuated Quasi contribution. Its guard must mean exactly
resource-enabled AND resource-at-source. Extra inputs or state-dependent guards
decline. There is no second memory-state owner or reset/write elimination.

`experimental-minimize-wire-captures` removes immutable passive neighbor sites
from the ordered wire machine's mutable state. Their constant profiles remain
read operands; their capture insertion, bias writes and enqueue tests disappear.
Mutable contacts and active receivers retain canonical discovery and dispatch
order. This extends the existing typed fixed-circuit strategy to mixed circuits;
it does not substitute settled dust values or reorder piston requests. Masks
are independently rederived, validated before lowering, cleared by rewrites,
and exported as optional `mutable_captures` fields.

The ordinary Graph state and GraphFuture remain authoritative. During an
accepted event commit, self-updates that immediately return while its base is
pending, inert controls and inert contact deliveries are omitted. Other actors,
shape notifications and reset receivers remain at their original delivery cuts.

## Correctness evidence

ADDER11BIT passes forty pinned arithmetic histories and six interrupted-cycle
handoffs. Counter passes both native histories and checks all sixteen BUD reads
over changing resource positions, availability and publication states. Divider
passes all eleven native histories. Before the passive-capture reduction, FPU
and BubbleSort's event-kernel modes pass the saved-trigger 256-tick and active
100-tick native checks respectively, including handoff. A new complete-sort
BubbleSort measurement with the combined experimental passes remains pending.

Wire minimization passes every old/new analog strength pair, seeded cyclic dust,
and nested-walk continuation. Tests compare mutable captures, intermediate writes,
heading, delivery order, state and work counts; only omitted constant passive
discoveries are removed from the compared diagnostic trace. Forged masks,
mutable-site omissions, missing prerequisites and cancellation are rejected.

The default and experimental regression suites each pass 547 tests. Python
loader tests pass 32 checks and the viewer passes 38. The feature-enabled
workspace check passes with two existing unused-code warnings in the untouched
fixed-classic compiler. Three tests that assumed old layer selection were
updated to check the selected family, retain native comparisons for both request
origins, and explicitly enable the malicious builder under validation.

## Random FPU protocol and measurement

The research tests are `fpu_random_inputs_one_kernel_and_paired_inference`
(requires changing outputs) and `fpu_random_inputs_one_kernel_diagnostic_measurement`
(allows a clearly labelled switching-cost diagnostic). They use deterministic
xorshift64(13,7,17), seed `0x19a4110b5eed`, sixteen blue and sixteen red raw operand
bits plus three opcode bits in manifest order. It initializes ON for 64 ticks,
then holds each vector OFF64/ON64: four warm-up and 32 measured episodes.
Input levers update separately, in manifest order, after the trigger goes OFF.

The trigger is the saved lever at local `(6,36,76)`, attached to green wool at
`(6,36,75)`. The schematic palette, lever facing and attachment were checked;
the port does not refer to an alternate control.

Native validation checks output words every tick, ordered scheduled work,
queued actions and deferred motion projections, full block/entity checkpoints
at phase boundaries, command output, and sixteen native continuation ticks.
The paired comparison additionally checks authoritative Graph state and exact
GraphFuture, including pending indices, request source routes and deferred
revision/deadline order.

The timed backend is constructed from the lowered program and state. It receives
an **empty** PlotWorld only as a command-effect host; no original circuit blocks
are supplied to inference. This is necessary because FPU includes command blocks.
It does not imply that all original wire-machine operations have been removed.
Loading, lowering, warm-up, projection and validation are outside the timers;
input actions, Graph ticks and command effects are inside them.

All **4,672 native ticks**, including initialization and warm-up, match outputs,
scheduled FIFO, queued actions and deferred motion progress/order. Phase-boundary
blocks/entities, command output and sixteen native continuation ticks also match.
Each paired run additionally has identical authoritative graph state and future.

The output bank remains **`0xffff` throughout both native and Graph execution**.
The strict changing-output test therefore fails its workload-activity requirement;
the separate diagnostic test passes. This agrees with the older native control
reports. Random raw-port equivalence is not an IEEE arithmetic oracle, and there
is no certified completed-operation rate or result latency here.

A separate native-only probe also holds new operands before a clean falling
edge. Preparing while ON takes 2,624 ticks; preparing while OFF, resetting ON,
then activating takes 4,160 ticks. Both eight-vector probes still produce only
`0xffff`. Their green attachment is asserted against the imported blocks. These
are protocol diagnostics, not a newly certified operating contract. See
[the probe](../../../target/instant-event-kernels/native-protocol-probe.json).

## Five alternating random-input timing pairs

This is a frozen **release test harness**, with LTO disabled, 256 codegen units,
incremental compilation enabled, and logical CPU 4 affinity. Compilation and
other tests were finished before the measured pairs. It compares existing O5
stable-input execution with the event-kernel and passive-capture reductions in
the same executable. FPU has no admitted BUD-read pairs, so that pass adds no
runtime reads in this comparison. This is not a production fat-LTO timing.

| Pair | Existing graph | Reduced graph |
| --- | ---: | ---: |
| 1 | 7.812357 s | 7.298828 s |
| 2 | 8.844383 s | 6.713148 s |
| 3 | 8.621521 s | 6.792536 s |
| 4 | 9.267513 s | 6.896598 s |
| 5 | 8.430775 s | 7.589115 s |
| Median | **8.621521 s** | **6.896598 s** |

For 32 measured input sets / 4,096 ticks, the median is **20.01% less time** or
**1.250x switching throughput**. Every pair favours the candidate. This is a
combined-pass result, not an isolated attribution to either pass.

The source catalog has 175,051 cell pins; executable acceptance slices retain
7,804 pins. Neighborhood masks retain 137,116 mutable operands out of 601,440,
removing 77.20% of the structural neighbor slots from mutable walk bookkeeping.
This does not mean 77.20% fewer runtime walks or callbacks. Measured walks remain
**520,846** on both sides; callbacks decrease **29,086,849 → 28,907,366** (0.62%).
Piston samples, accepted actions, scheduled ticks and deferred publications agree.

The [raw pairs](../../../target/instant-event-kernels/fpu-random-paired.json),
[summary](../../../target/instant-event-kernels/fpu-random-summary.json),
[focused executable-entry records](../../../target/instant-event-kernels/fpu-kernel.json)
and [source/executable provenance](../../../target/instant-event-kernels/provenance-v14.json)
retain the experiment. The entry-record JSON is an inspection slice referring to
source graph IDs, not a standalone serialization of the complete executable.

The unsafe settled-net experiment remains disabled. Its known request-order
counterexample is not bypassed by these passes. Complete wire-free FPU execution
still needs an ordered transition certificate or an equivalent compiled event
graph that replaces the retained wire machine.
