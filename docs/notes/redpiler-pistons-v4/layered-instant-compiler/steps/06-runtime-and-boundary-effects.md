# Step 06: execute kernels and replace owned propagation

Status: partial implementation, awaiting review. O4+ with the explicit stable-input
contract builds layered programs and lowers the certified private pull-wave family.
Large shared-resource regions and general boundary effects remain unimplemented.

## Implemented execution path (2026-10-19)

`layers/kernels.rs` lowers cold cell/layer evidence to `InstantKernel`: a reduced
Boolean program, FIFO pull roots, original wire entry levels and short route effects.
Only stationary-entry/request/acceptance roots remain in the hot expression slice.
`backend/graph/instant_kernel.rs` evaluates into preallocated scratch. There is no
runtime compiler, truth-table training, state-array clone or internal Turbo walk.

Each queued request carries its actual source route. Before taking ownership the
executor checks that route, stationary entry, settled corridor levels, early-drop
conditions and the complete existing FIFO prefix. Unrelated requests or untrusted
origins retain ordinary Graph execution before any kernel state change. Current
certificates require one entry route, not a named lever or universal device clock.

Accepted pulls reuse authoritative actor/resource state, deferred publications and
presentation effects. Synchronous computation routes omit owned piston samples and
wire propagation; retained reset notifications keep their original order and guards.
Private cable strengths are materialized at the certified source-removal cut. Reset
and later extensions still execute through ordinary Graph; this is not reset removal.

The former two-cell/one-corridor size restriction is removed. Each admitted corridor
must still prove one private source, one consumer and no other effectful receiver.
A real four-cell/six-wire chain executes one wave with zero first-response Turbo walks
and matches interpreter state/future through 32 ticks. The public-pipeline test also
checks rejection of an untrusted request origin. These examples do not establish
whole-ADDER1BIT/11BIT, Counter, Divider, FPU or CPU kernel coverage.

Use `GraphExecutionCounts::instant_kernel_invocations` for actual executions; a local
cell recognition count, layer artifact or installed-node census is not an invocation.

### Measured scope of this milestone

The production `instant --kernel-wave --iterations 64` benchmark alternates O3 control
and O4 stable-input runs on fresh two-cell/two-wire worlds. It times only the first
computation tick; compilation, stimulus, projection and reset are outside that window.
The medians were 18.85 us for O3 and 3.90 us for O4 (4.83x by median ratio). Ranges were
17.0-29.3 us and 3.6-209.7 us respectively, including an O4 outlier. This measures a
small admitted wave, not whole-cycle or whole-device acceleration.

The corresponding work fell from 49 to 3 callbacks, one to zero wire walks and 23 to
one piston samples, while both committed two actions. Every O4 sample invoked its
kernel once. These counters include the untimed stimulus. Full Graph state/future
checkpoints matched the O3 control after reset; independent interpreter differential
tests cover the two-cell wave and four-cell chain through reset.

The public ADDER11BIT benchmark (three samples per executor, 512 active ticks each)
passed its interpreter output, state/future and continuation checks. O3 stable and O4
stable medians were 34.477 ms and 34.413 ms; interpreter median was 40.162 ms. O4
installed and invoked zero kernels, so this is no demonstrated kernel speedup.

Fresh local admission counts are listed below. Admission is a local reset/cell proof,
not an executable-region certificate or a proof that every remaining actor is memory.

| Fixture | Locally admitted actors | Executable kernels |
|---|---:|---:|
| Divider | 2,034 / 2,055 | 0 |
| FPU | 7,804 / 8,100 | 0 |
| COUNTER | 146 / 163 | 0 |
| PC Counter | 484 / 500 | 0 |
| CPU BubbleSort | 15,200 / 16,561 | 0 |

All five device differential tests passed against the interpreter, including the
100-active-tick CPU episode and interpreter handoff. They exercise general Graph for
these declined regions. Broader reset lifecycles, shared-resource corridors and signal
arrival proofs remain the blockers to replacing their internal execution. Full CPU
sorting, broad fuzzing and peak-memory measurement were not repeated in this batch.

Artifacts: `target/instant-kernel-wave.json`,
`target/layered-adder11-e2e-benchmark.json`, `target/instant-e2e-admission.log` and
`target/instant-e2e-large-devices.log`. Target artifacts are local, reproducible outputs.

Final validation: the release `redpiler::` suite passed 491 tests with 30 ignored;
the separately selected five large-device differential tests passed. Core tests/benches
and server `cargo check --locked` passed, as did focused formatting and diff checks.
Python loader tests (23) and viewer tests (29) also passed. Two existing dead-code
warnings in `fixed_classic.rs` remain. The full workspace suite was not run.

### Subsequent CPU proof-cut attempt

See [CPU_BUBBLESORT_KERNEL_PROOFS.md](../../CPU_BUBBLESORT_KERNEL_PROOFS.md)
for full-sort timings and the capability-aware partition attempt. All original
ports remain; unsupported cells/cables and reset/geometry dependencies can form
perimeter cuts. The largest CPU proof group falls from 11,025 cells to 426,
but the current effect/arrival certificates still produce zero CPU kernels.
The rebuilt debug Redpiler suite passed 492 tests with 31 ignored. This is a
compiler decomposition milestone, not a demonstrated CPU runtime speedup.

## Goal

Execute the immutable expressions inside GraphBackend and replace covered propagation,
rather than running a kernel and then replaying the same wires/callbacks anyway.

## Code layout

- One focused runtime module, initially `backend/graph/instant_kernel.rs`.
- Source/entry lookup tables built during lowering, not whole-table searches in dispatch.
- Normal power/site/component/scheduler procedures remain shared. Add the smallest direct
  binding hooks to `sites.rs`, `power.rs` and existing entry dispatch.
- Focused tests in `backend/graph/tests/instant_kernel.rs`; no giant test block in graph.rs.

## Execution algorithm

1. An original qualifying boundary occurrence selects the region entry. Check its local
   readiness/protocol assumptions before suppressing any ordinary execution or mutating state.
2. Sample only live input bindings for that declared frame, including outside ordinary/BUD
   memory. Do not allocate/copy the whole program's component/resource arrays.
3. Evaluate topological Boolean expressions into reusable scratch. Keep the input frame only
   while reset continuations need it; normal kernels do not retain per-piston consumed masks.
   Sampling/request construction and accepted computation remain separate. Queue an entry
   in the existing piston-action FIFO when its accepted work belongs to that phase. Batch
   consecutive internal uses only after proving that intervening ordinary work is irrelevant;
   otherwise use a continuation cut in the same queue, or decline before taking ownership.
4. Replay the compact boundary-effect stream. `WriteOutput` updates boundary shadows;
   `Notify` calls the ordinary typed handler with its original direction/capture meaning;
   reset schedules use existing NodeIds, half-tick offsets, priorities and deduplication.
5. Outside readers resolve compiled source power/geometry through those shadows. Never let
   an outside comparator/conductor/dust query read a stale original internal wire slot.
   Cover fast Stored/Output reads and direct guard-state predicates on owned actors/resources,
   not only `resolve_site`. Lower those external observations to the same authoritative
   boundary binding; do not create a second mutable payload copy to satisfy a forgotten read.
6. A transient high followed by low is represented by a few boundary operations. The ordinary
   repeater handler schedules its response; an observer/BUD gets the qualifying notification.
7. Exclusive dispatch ownership prevents covered wire seed/Turbo handlers, piston samples,
   accepted actions and proxy routes from running in addition to the kernel. General devices
   and declined regions still use Graph's current handlers and clock.
8. If an outside callback can synchronously change/reenter the unfinished region, honor an
   admitted continuation boundary or decline before mutation. Do not queue it into another
   tick or overwrite its writes under the external-order relaxation.

No runtime World clone, Turbo evaluator, region training, truth-table lookup built on demand,
wire snapshot construction, per-layer heap allocation or loop over all program pistons.
An initial first-response test may stop at the supported cut before a reset deadline, but
that is partial experimental coverage, not a runnable public circuit.

## Validation

- One ordinary component activates a kernel without a selected lever in the owned region.
- An unrelated scheduled device executes during the same logical tick normally.
- BUD data-only changes retain memory; qualified same-value updates still reach it.
- Sub-tick repeater pulse, comparator feedback, head observer, shape direction and profile reads.
- Two regions sharing an outside source do not reenter/claim one another accidentally.
- On rejection, input state/pending work are unchanged; general execution is used exactly once.
- Accepted first-response ADDER1BIT cases compare real outputs, ordinary memory and pending
  work against native/ordinary Graph. A kernel counter must be positive and owned wire-walk
  count must be zero. Inspect general callbacks by ownership, not total count alone.

## Gate

Real kernel execution replaces internal work, with complete implemented effect variants and
direct source bindings. No unconditional trace formatting is added. Runtime counters measure
actual work; removed physical actions are not counted as secretly executed actions.
Whole-cycle correctness and public activation still require steps 07-08.
