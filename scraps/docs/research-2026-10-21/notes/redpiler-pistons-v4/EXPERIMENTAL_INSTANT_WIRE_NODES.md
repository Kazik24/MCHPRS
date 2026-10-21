# Experimental instant wire nodes

The certified single-wire pass is now selected by normal production O4+;
see [production tiers](../../../../../docs/REDPILER_OPTIMIZATION_LEVELS.md). Its original
diagnostic name remains `experimental-compile-instant-wire-nodes`.
The experiments below record the earlier opt-in configuration of 2026-10-19.

## Implemented boundary

An isolated wire whose possible callback receivers and conducted electrical
consumers belong to recognized instant cells becomes a local signal transfer.
This first slice covers a **single-wire net**, including multiple sources and
fanout; it does not replace a multi-wire corridor with its final truth table.
Controls are inert recipients. Reset observers belonging to recognized cells
retain their scheduled behavior. Lamps, classic gates, ordinary pistons, other
possible dust and overlapping private execution owners decline.

The builder derives a 24-bit receiver mask from every selectable contact
alternative, not the imported pose. A separate validator rederives the mask.
Rewrites discard it, lowering respects proxy route ownership, and JSON/Python/
viewer inspection carries and checks the additive `instant_node` field.

Execution still samples power at the original seed and writes the original
wire state. It captures only possible meaningful recipients, chooses admission
before delivering work, and follows native heading 3 and its repeated first
four slots. Each actor/reset receives its first qualifying operation once.
All admitted receivers only enqueue work; they cannot change geometry or
live inputs during this episode, so later identical samples are redundant.
Piston requests retain the original Turbo route and first insertion order.
There is no whole-wave ownership requirement and no new state owner.

The wire's stored shape and strength remain available for projection, reset,
continued general execution and independent observations. Shape updates retain
their original path. Runtime counters distinguish executed signal transfers
from ordinary wire walks. Callback and piston-sample reductions reflect removed
operations, not a change to accepted-action accounting.

## Reset scope

The existing `compile-builtin-resets` pass already lowers eligible private
observer execution to a reset clock. This experiment uses that clock where
available. It does not erase reset deadlines or shared pulses: these rearm the
cell and must survive reset/continue. Broader reset-network contraction needs
its own ordered boundary proof.

## Validation and measurement

The focused tests cover falling signals, restoration before acceptance,
shared sources, inhibited fanout, first-request provenance, complete machine
state against general Graph, native boundaries, reset and interpreter
continuation. Negative checks cover classic/dust consumers, forged masks and
cancellation before publication. Additive export readers accept older files
and reject invalid masks.

The ignored `instant_wire_nodes_cpu_fpu_paired_measurement` test derives O5
baseline and candidate programs from the same graph and imported work. It
alternates three execution pairs: 1,000 active BubbleSort ticks or 256 FPU ticks
with four trigger transitions, excluding loading/compilation/projection from
execution timers. It then checks 100 CPU or 256 FPU ticks against native,
ordered future work each tick, full snapshots at four checkpoints, and 16
interpreter handoff ticks. Reports are under `target/instant-wire-nodes`.
These are bounded internal-work measurements, not complete sort or verified
FPU arithmetic throughput. The executable uses the release profile with fat LTO
and test instrumentation (`cfg(test)`), so these are paired harness timings,
not a production-server throughput claim.

### Results: no demonstrated speedup

Three alternating pairs, no compiler process observed during measurement:

| Workload | Baseline median | Enabled median | Time change | Wire walks removed | Callbacks removed |
| --- | ---: | ---: | ---: | ---: | ---: |
| BubbleSort, 1,000 ticks | 7.193611 s | 7.288752 s | +1.32% | 29,979 / 395,187 (7.59%) | 119,256 / 20,848,193 (0.572%) |
| FPU diagnostic, 256 ticks | 0.570631 s | 0.584561 s | +2.44% | 532 / 32,652 (1.63%) | 1,016 / 1,616,380 (0.063%) |

Timing varies between pairs; these results do not establish a statistically
significant slowdown, and they do not demonstrate a gain. Keep the pass off by
default. It eliminates too little of the dominant work to justify enablement.
Every admitted wire already qualified for O5's existing isolated Turbo replay;
these were cheaper episodes than the general multi-wire walks dominating the
profiles. The removed-walk percentage therefore overstates the share of CPU
cost this experiment can recover.

BubbleSort admits 2,209 / 41,354 wires; metadata construction/publication takes
95.05 ms. FPU admits 405 / 25,060 wires in 54.11 ms. These builder times exclude
independent validation, which rederives the certificate. Piston sample savings
are 12,621 and 88 respectively. Accepted actions, scheduled transitions,
deferred publications and kernel invocations agree in every pair. Pair-end
full projected state and future agree. Independent native checks passed for
100 BubbleSort ticks and 256 FPU ticks, plus 16 handoff ticks each.

Artifacts: `target/instant-wire-nodes/{cpu_bubblesort,fpu}-paired.json`,
`summary.json`, `production-paired.log`, `production-focused.log` and
`provenance.json`. The source diff and frozen executable sit beside them.

The earlier kernel rejection census also explains why this cannot solve most
of the CPU/FPU cost: the O3-prepared research graph had 3,307 CPU and 3,092 FPU
`SignalArrivalUnproven` regions before more specific cable-proof rejections.
Unsupported outside-wire feedback at merged action cuts is a major barrier;
reducing isolated seed episodes does not establish those missing causal cuts.
The baseline census is preserved under `target/composition-rejections-baseline`.


## Next boundary

Multi-wire nets can expose different intermediate strengths and request order.
Extend this pass with an ordered sparse transfer certificate for those nets;
retain the first qualifying request and every relevant source-arrival cut.
Mixed consumers must remain general. Closed membership alone is insufficient
permission to erase intermediate electrical events.

The concrete extension is local to a net: certify fixed wire geometry and
instant-only recipients, then compile each supported stationary source vector
and one-source transition into ordered receiver samples. Retain the relevant
intermediate threshold crossings rather than only the final power. Small fan-in
nets can exhaust source transitions; runtime entry checks must retain general
execution for unsettled wire state or unproved source changes. This avoids the
current whole-kernel requirement to own the surrounding action queue. It is a
proposed next step, not a certificate implemented by this pass.

## Memory-only retry (2026-10-20)

`analysis::tests::research::layered_devices::memory_only` retries the existing
closed multi-wire fixed-point experiment without rejecting it for changed wire
strengths, revision counters, or pending-work order. Both sides use the current
O5 pipeline with the isolated single-wire experiment disabled, then only the
candidate installs `unsafe-abstract-instant-nets`. The experiment stays outside
production tiers.

BubbleSort watches the frozen RAM decoder's 768 piston bits: four banks at
local Y=7,17,27,37, sixteen words per bank, twelve bits per word. A published
piston encodes `!extended`; a moving base is unreadable. Every baseline sample,
including unreadability, is checked against native execution. Candidate value
differences count only when both bits are readable. Availability differences
are recorded separately. Execution continues after a difference, through all
10,036 ticks and the final sorted output bank.

FPU uses `FPU_MROWW.schem`, the green-wool trigger, and the existing seeded
random-input history (four warm-up and thirty-two measured vectors, 64 ticks
per trigger level). Its ordinary pistons outside the instant kernel are
observation candidates; these fixtures do not provide a certified FPU memory
map. They must not be reported as proven BUD storage pairs. The Counter/ANPU
vertical-pair recognizer finds no pairs in these two prepared fixtures.

The probes also count native bit transitions to detect inactive histories and
record the first resource-location difference separately from the RAM verdict.
Graph inference receives empty command hosts. Native runs independently; it is
not an inference fallback. Timings are one interleaved diagnostic run, not clean
alternating performance pairs. The current harness uses release optimization
with LTO disabled and 256 codegen units.

Raw reports: `target/instant-wire-nodes/memory-only/{cpu_bubblesort,fpu}.json`.
Run the ignored `memory_only::` tests with `--nocapture --test-threads=1`.

### Results

| Build | Ticks | Probed pistons | Active probes / native bit transitions | Readable bit differences | Availability differences |
| --- | ---: | ---: | ---: | ---: | ---: |
| CPU_BUBBLESORT | 10,036 | 768 documented RAM bits | 64 / 512 | 0 | 0 |
| FPU_MROWW | 4,672 | 296 ordinary-piston candidates | 164 / 9,254 | 0 | 0 |

BubbleSort's complete four-bank RAM trace matches native, including unreadable
samples during motion. Its final output bank is correctly sorted 0..15 on all
three executions. FPU's probe trace also matches, including availability; this
does not establish arithmetic correctness or completeness of the memory map.
There are 512 unavailable CPU samples and 10,354 unavailable FPU samples, with
the same cells unavailable in baseline and candidate.

The first resource-location differences are CPU tick 26 and FPU tick 65. Both
payloads are disabled at those samples. These internal differences do not
become differences in the observed storage histories. Scheduled tick, accepted
piston action, and deferred-publication totals are identical in both runs;
this statement does not claim their ordering is identical.

| Build | Admitted multi-wire nets / wires | General walks, baseline -> candidate | Callbacks, baseline -> candidate | Diagnostic inference seconds, baseline -> candidate |
| --- | ---: | ---: | ---: | ---: |
| CPU_BUBBLESORT | 1,408 / 19,362 | 3,882,011 -> 1,933,735 (-50.19%) | 204,031,794 -> 165,996,351 (-18.64%) | 51.260 -> 55.317 (+7.92%) |
| FPU_MROWW | 1,006 / 10,918 | 604,145 -> 399,212 (-33.92%) | 33,596,586 -> 25,364,027 (-24.50%) | 7.992 -> 8.491 (+6.25%) |

Both ignored probes completed successfully. The existing causal ADDER1BIT
request-order regression was also rerun successfully; its wrong arithmetic
response remains a counterexample to unrestricted contraction.

For an abstract CPU replacement, equality of internal pending queues is too
strict as the sole rejection test. The tested RAM boundary admits changes that
full physical-state equality rejects. These histories support further logical
contraction research; they do not certify arbitrary inputs, all output timings,
or interpreter handoff.

The research executor still discovers topology, allocates net scratch, and
uses general source queries at runtime. The next performance experiment should
compile fixed topology and weighted source-to-receiver transfers into the kernel
and eliminate that work. Fewer walks alone have not delivered a speedup here.

## Typed piston graph and relaxed-boundary follow-up

The test-only `outside_electrical_consumer` relaxation was measured before replaying
large histories. It admits 720 additional BubbleSort multi-wire nets (1,440 wires),
raising the census from 1,408 / 19,362 to 2,128 / 20,802. FPU remains 1,006 / 10,918
and Counter 4 / 685, so those devices do not need redundant relaxed replays. The
relaxation still rejects nets with outside signal sources, unrecognized piston
receivers, classic consumers, missing instant receivers, reset ownership, or wire
execution ownership. This relaxed mode exists only in tests; the default census and
production builder remain strict.

The independently labeled `test_data/instant-pistons-io/fixtures/counter_basic.json`
probe checks all 16 storage pistons found by the BUD memory-read compiler, rather
than treating ordinary piston candidates as memory. Over the frozen 126-tick input
history, five cells were active and native storage changed 32 times. Strict and
relaxed candidates both matched every baseline bit and availability sample. Under
relaxation, the 685 admitted wires reduced callback count 99,862 -> 43,630 and wire
walks 1,374 -> 405 in this short diagnostic, but elapsed executor time increased
from 12.81 ms to 17.99 ms; this is not a speedup claim or a paired benchmark. The
first unrelated disabled-resource location difference remains at tick 44.

The pinned `NANOTICK_EXAMPLE.schem` graph export is
`target/instant-wire-nodes/piston-graph/nanotick-example.json`; its SHA-256 matches
`test_data/instant-pistons/fixtures/nanotick_example.json`. After O5 preparation,
the existing typed catalog preserves five separate piston actors/five resources,
35 piston callback routes, three wire corridors (3, 4, and 1 wire), and 552 typed
boundary ports. The strict and relaxed censuses each admit two multi-wire nets / seven
wires. The four-rotation native regression
`supplied_nanotick_counterexample_activates_consumer_before_inhibit_arrives` passes:
the downstream piston retracts at operation 4 and the inhibitor arrives at operation
5, so any compiled graph must preserve that native transient rather than impose the
schematic author's intended inhibit-first result. The graph export is an inventory,
not an alternate executor or a proof of full interface completeness.

The executable local boundary already present in the graph pipeline is the isolated
single-wire `experimental-compile-instant-wire-nodes` pass. It substitutes ordered
receiver-slot deliveries for one dust node's traversal while retaining each piston
as an independent stateful actor, resource, request source, and reset dependency.
Its native-continuation/reset test and shared-source/inhibited-fanout test pass. The
pass declines non-isolated dust, outside consumers, lamps/classic loads, other dust,
and unrecognized receivers; it does not flatten the unequal NANOTICK chains into a
settled Boolean. This is the bounded direct-edge lowering supported by the present
tests; the wider multi-wire net pass still uses runtime settlement and has not shown
a diagnostic time improvement.

Raw follow-up artifacts are `target/instant-wire-nodes/memory-only/admission-delta.json`,
`counter.json`, `counter_relaxed_electrical_consumers.json`, and
`target/instant-wire-nodes/piston-graph/nanotick-example.json`.

The relaxed full-history replay then completed because BubbleSort had a nonzero
admission delta. Across all 10,036 ticks, all 768 documented RAM bits matched the
unchanged O5 baseline (including 512 matching unavailable samples); 64 cells were
active and there were 512 native transitions. Native, baseline, and candidate final
output banks were each sorted 0..15. Relative to the strict candidate, the relaxed
candidate recorded 28,800 more instant-wire-node invocations (2,263,198 vs
2,234,398) and 14,400 fewer general wire walks (1,919,335 vs 1,933,735). Callbacks,
piston samples/actions, scheduled ticks, and deferred publications were identical
between the two candidates. These aggregate counters show a real execution delta;
they do not identify which of the additional 720 nets were invoked. Against the
unchanged baseline, relaxed walks were 1,919,335 vs 3,882,011 and callbacks were
165,996,351 vs 204,031,794. The single relaxed diagnostic run took 51.698 s vs
47.536 s baseline (+8.75%); it is not an alternating release benchmark and does not
show a runtime speedup. Relaxed FPU and Counter replays also passed, but their
admission delta was zero and their strict histories already provide the useful
contract coverage.
