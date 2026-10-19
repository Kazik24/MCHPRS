# Experimental instant wire nodes

2026-10-19. Separate opt-in pass: `experimental-compile-instant-wire-nodes`.
Build core with `--features experimental-instant-wire-nodes`; O3 and higher
append the builder after the normal pipeline. Ordinary builds are unchanged.

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
