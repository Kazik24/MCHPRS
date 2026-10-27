# Why the settled-frame architecture failed

Status: investigation and replacement work in progress. The saved FPU has not
achieved useful acceleration. The latest planning targets are COUNTER at least
1M game TPS, the actual saved FPU around 10–40k game TPS, and correct ADDER
execution and handoff. No replacement is deployed.

Compile and `/rp analyze` now show bounded, clickable warnings for scoped
instant admission and the separate whole-plot equation attempt. On the saved
FPU the warning identifies candidate clock `(-267, 86, -181)` and output
`(-201, 82, -244)`, where native ownership overlaps its scoped frame. The
whole-plot attempt separately exceeds its equation construction budget at
`(-84, 57, -274)`. Clicking a position highlights the block. These are compiler
proof limitations, not evidence of a physical circuit defect. Warnings do not
yet cover every rejection path.

The diagnostic build's 16-frame changing-input replay passed native checks,
all six handoffs and the eight-command witness. Its log is
`E:/codex-builds/MCHPRS-diagnostic-saved-fpu-complete-16.log`; the pinned
scale-5 executable is `E:/codex-builds/MCHPRS-diagnostic-current/fpu-scale5-diagnostics.exe`,
SHA-256 `395EE10F55EE567FAE6AF1725867977EC26E369CA19819C69F374DB69C346A3F`.
The source scale was restored to 4. Two ignored FPU readout diagnostics failed
in this investigation; whether those failures predate the warnings is not
established.

The staged scope, ownership contract and acceptance gates are in the
[instant compiler implementation plan](REDPILER_INSTANT_COMPILER_IMPLEMENTATION_PLAN.md),
prepared by Luna6 High and reviewed against the existing compiler/runtime.

## 27 October implementation investigation

The existing Counter O6 executor ran 1,000,002 ticks in three samples at
3.96M, 3.52M and 3.65M wall TPS, matching sampled native progression and
executing 166,667 logical evaluations per sample. The strict O6 adder test now
covers four carry/changing-input histories, ordered requests and 12 handoff
cuts each; the earlier 11-bit FIFO reversal did not reproduce on this tree.
Source identity coverage is limited to pending requests with an unambiguous
native piston-contact witness.

An isolated ownership-transfer experiment passed the 192-frame FPU native
oracle, all six handoffs, and the eight-command witness. It executed 237,318
transferred accepts but reduced callbacks only from 13,314,892 to 12,466,294
(6.37%); instrumented timings did not show useful acceleration. The experiment
is not enabled in the workspace. A separate census found that all 6,570
ordinary fallback wire walks and their nested callbacks account for at most
545,145 callbacks (4.37% of the active-transfer total). A rejected-corridor
prefix therefore cannot reach the target by itself.

The remaining bulk work includes 4,746,609 wire updates and 4,100,582 piston
updates in the active-transfer run. A general compiled episode needs exact
ordered internal effects, current live reads and a native continuation across
foreign callbacks. `execute_transport` owns the queue, captures and layer state;
the current `SignalOp` replay discards that state and freezes later power writes.
It is not yet proved which intermediate writes can be virtualized while
preserving `set_wire` cache invalidation and later observer, command and guard
reads. A tested isolated continuation seam does not eliminate work by itself.
The seam, owner transfer and census prototypes remain outside the workspace;
they are not accepted as the replacement executor.

Artifacts include `E:/codex-builds/bud-transfer-callback-census-192.log`,
`E:/codex-builds/bud-transfer-empty-scope` and
`E:/codex-builds/resumable-transport-experiment.rs`. The full current release
`redpiler::` test filter passed 757 tests with 95 ignored. The compiler fan-index
extraction preserves the existing admission policy; it does not yet implement
the new region execution contract.

An isolated route-prefix experiment executed certified steps before a foreign
callback, then resumed Graph at that callback's original ordinal. On the same
192-frame source and options, its candidate replay had 13,314,460 callbacks
versus 13,314,892 for the baseline; both elided 79 wire walks. Both passed the
saved-plot oracle, six handoffs and eight-command witness. The 432-callback
reduction (0.0032%) is not material; single timing samples do not establish a
speedup. The prototype was not integrated, and its same-route mutation and
proxy-overlap regressions were not completed.

Logs are `E:/codex-builds/route-prefix-192.log` and
`E:/codex-builds/route-prefix-baseline-192.log`. The candidate executable SHA-256
is `FE499FE458466AAB4AD56D7459A70859B7C48951ED3C958B26018683765C799B`,
and the baseline is `307F8F18DFF5098D3C4CCF3A825AC2A030957B5722369BA34D73FA65E32E694C`.
The prototype remains archived outside the workspace; source scale was restored
to 4.

### Dead-net and boundary follow-up

The user identifies `(-201,82,-244)` as an unconnected dead adder net. A fresh
saved-crop inspection confirms that this wire is powered by the redstone block
above it, is selected by the generic terminal-wire heuristic, and has possible
local piston and observer callbacks. The 69-edge shortest connection to the FPU
clock in the current island partition includes unconditional wire-neighborhood
geometry reads and other undirected possible interactions; it is not proof of
a live directed event or power path. These callbacks and physical state must
remain native if the wire is excluded from a compiled region.

The current full-plot target heuristic selected 26 wires, 27 repeaters and three
command blocks. This includes the 16 intended numeric wire/repeater pairs, but
the extra selections are not established FPU outputs. The plot also has 35
observable input-indicator lamps. The inventory found 317 distinct pistons in
89 catalog-declined groups; direct contact owners at the 16 numeric terminals
and three command inputs had no local recognition decline. Upstream influence
from the declined actors remains unverified. See
`E:/codex-builds/warning-island-inventory-full.log` and
`E:/codex-builds/warning-island-shortest-crop.log`.
The user identifies the unrecognized piston at `(-114,58,-212)` as a legal
instant OR input in a Divider subtractor, with a furnace reset circuit. The
other OR input uses a side observer and a block-based reset. The first piston
shares a possible payload contact with the piston at `(-116,60,-212)`; cell
discovery rejects their entire group because it cannot find a reset witness for
the first piston. This is a recognition failure, not proof that the circuit is
invalid. Both lie within the replay crop, so the crop cannot establish that
the Divider was excluded. A 5×5×5 saved-block comparison matches repeated
rotated schematic neighborhoods in 124 of 125 block names; the remaining
position is air in the snapshot and glowstone in the compared schematic. The
user confirms that they intentionally removed the glowstone and it has no
effect on the build. The comparison does not uniquely identify the bit.
The target's downward-facing observer watches its own base; the furnace is
the solid cap above it. The other shared-payload piston has a side observer
watching its base. Their complete native reset and callback behavior still
requires verification.

For the first piston, the saved graph contains the zero-attenuation reset path
from the observer's strong output through the furnace conductor to the piston
quasi input. Both links have unconditional guards, and both base-change
routes notify the observer. The furnace is represented as a constant component
because it has a comparator override; reset power tracing currently refuses to
traverse all component conductors. The other shared-payload piston's reset
also passes through a furnace component. Reset tracing now allows an immutable
component to conduct when its block is a solid, opaque cube. In an isolated
comparison, this recognized a 20-actor, 10-resource group and increased the
Divider event plan from 2,034 to 2,054 actors. All 11 fixture histories passed
native comparisons and handoff, with instrumented event entries observed for
the nine matching west-facing actors. This did not activate or benchmark the
saved plot's logical executor; comparator behavior, the complete boundary and
saved-plot acceleration still need the broader acceptance checks.

In an isolated scoped-only output-selection experiment, omitting wires with no
power fanout moved the first crop rejection from the highlighted dead wire to
an equation-support/native-ownership overlap at `(-109,52,-217)`. No logical
device was admitted. This does not establish safe directed ownership or
accelerated execution; native callback, power, conductor, guard and observer
read crossings still require proof.

A later isolated directed-fan experiment follows every power and callback
output of a reached catalog owner. On the saved crop it reaches the Divider
group and reports 164 comparator, 32 lamp and 1,207 unrecognized-piston power
link occurrences. These are possible links, not executed components or proof
that the Divider blocks FPU admission. The traversal does not prove which
activation changes which output; it can still merge independent work through
read dependencies. A sound cut needs source-qualified activation occurrences,
certified transitions and their actual effects, with unsupported work left
native. The experiment has not been integrated.

Graph's piston request FIFO keeps the first source route when it deduplicates
by piston and action. Existing layered execution treats that route as an
ordered episode, so route-only identity is not by itself a demonstrated bug.
A finer residual cut would additionally need the winning delivery occurrence
and any captures made before native callbacks. A saved-plot static census
found 73,719 actor/route activation groups and 10,513 layer route groups with
multiple delivery ordinals; these are possible alternatives, not executed
event counts. See `E:/codex-builds/source-region-fpu-occurrence.log`.
An instrumented 16-frame changing-input saved replay observed 32,799 popped
requests in the FPU bounds: 32,359 had multiple possible catalog ordinals for
their actor and first-source route, 121 had one, and 319 had no catalog match.
It delivered 1,026,705 callbacks in 96 measured ticks. The replay passed its
native oracle and six handoffs with zero logical devices; the instrumentation
was not timed. See `E:/codex-builds/source-region-saved-request-census.log`.

An isolated cut inventory selected 5,452 cells and 2,775 complete wire
corridors by spatial bounds for measurement only. In the 96 measured ticks,
the instrumented source/receiver census observed 550,805 internal callbacks,
153,714 outgoing and 5,228 incoming; internal wire walks and callbacks without
source provenance were excluded. Outgoing callbacks included 122,154 to
observers or reset machinery. This is not a source-derived candidate or a
proof those effects can be collapsed. The fallback oracle passed six handoffs;
its eight command outputs occurred in warmup, not the measured interval. See
`E:/codex-builds/observation-cuts-saved-corridors.log`.
Of those 122,154 observer/reset callbacks, 12,624 scheduled a first tick,
27,325 qualified while one was already pending, and 82,205 had an unpowered
captured update that could not schedule. Preserving the first scheduling
position and live pending state remains necessary. These counts alone cannot
justify skipping callbacks or deliver the throughput target.

The existing equation builder can construct the saved FPU's 16 declared
repeater inputs, 35 indicator-lamp inputs and three command inputs in 4,045
Boolean expressions, with 53 live controls (36 levers and 17 buttons) and 15
resource-state feedback inputs and outputs. This is a value-equation census,
not a certified device. All three command roots are constant false despite
eight native tellraw outputs during warmup, so settled values omit command
pulses and cannot replace native execution. See
`E:/codex-builds/functional-saved-inputs.log`.

An isolated saved-plot replay alternated a native command witness with zero
each frame. Two command blocks repeatedly powered at frame phase 2 and emitted
at phase 3; all six handoffs and Graph/interpreter tick comparisons passed,
with at least 24 total command outputs in each comparison including eight
warmup outputs. The third command did not fire for this witness. Existing
event-boundary roots for the three command inputs are also false, while
feedback-aware source lowering declines. See
`E:/codex-builds/command-transition-saved.log`.

The one-shot boundary extractor instead produced three nonconstant command
roots in 45 expressions. In 120 sampled Graph ticks their predicted power
matched all three live command inputs, including the observed pulses; Graph
command history also matched the interpreter. This is snapshot evidence, not
a proof of intra-tick effect ordering or a complete compiled transition. See
`E:/codex-builds/command-transition-oneshot-live.log`.

A test-only six-tick synchronous shadow transition failed against the saved
plot: across 16 samples per phase, power-root mismatches were `[0, 0, 416,
416, 0, 8]` and resource-state mismatches were `[0, 0, 0, 120, 120, 120]`.
The model misses transient and restoration phases. At 168 actual command
callback cuts per handoff phase, the three one-shot roots matched Graph power;
that observation does not prove the complete transition. Graph remained
authoritative. See `E:/codex-builds/functional-shadow-transition.log`.
The lamp mismatch comes from a reset observer powering the same solid
conductor as an input lever during phases 2–3. A traced feedback payload moves
to Near during retraction, is enabled in phase 2, moves back disabled in phase
3, and is enabled at its source in phase 5; revision-checked deferred events
accompany both movements. A single settled feedback bit does not represent
that lifecycle. The full one-shot extraction of 54 outputs constructed 115,386
expressions, with thousands of live state and fired inputs; it remains neither
validated nor an executable FPU device. See
`E:/codex-builds/functional-shadow-resource-trace.log` and
`E:/codex-builds/functional-full-one-shot.log`.
Reducing that one-shot program to the 54 power roots leaves 246 expressions
and 136 inputs. A shadow comparison matched live Graph power for every output
at 96 phase snapshots when `InstantFired` was assumed to mean retracted or not
ready and all other inputs were sampled from Graph. That assumption is not
certified and the input sampling retains native work; the measured equation
evaluation alone is not throughput. See
`E:/codex-builds/functional-oneshot-lifecycle.log`.
Those 70 electrical inputs classify as 35 levers and 35 observers, with no
direct wire sources. Following the 32 referenced actors through their
eligibility roots reaches 7,828 actors and 115,346 expressions, including
thousands of comparator and observer bindings; the compact output slice is
therefore not a self-contained FPU calculation. These are typed dependencies,
not counts of executed events. See
`E:/codex-builds/functional-oneshot-closure.log`.
An experimental assumption that all 35 relevant observers are powered during
phases 2–3 matched the sampled output powers but mispredicted nine observer
bindings on every sampled frame, so it cannot preserve state or handoff.
Trying to synthesize steady power for all 32 output-dependent actors also hit
the existing source-nesting bound. See
`E:/codex-builds/functional-virtual-observer.log`.
The 35 watched indicator actors' 70 Main/Quasi power roots reduce to 36
expressions over 36 lever inputs. A test-only virtual observer model using
those roots matched all 35 observer inputs and 54 power outputs across 96
alternating-input phase snapshots without reading Graph actor revisions after
seeding. Other actor/resource state remained native; this is not a complete
transition or acceleration. See
`E:/codex-builds/functional-control-actor-clock.log`.

An isolated iterative feedback experiment uses a bounded cycle scan and cuts
only canonical Far resource states. It built the output and actor-input roots
without the recursive depth failure and matched 54 power roots and the 15
feedback roots shared with the recursive builder over 96 sampled phases. The
64 actor power roots were not checked in that run. It also introduces 30
additional state inputs, whose physical
transitions have not been proved, and reads live Graph state. The broader
initial cut was rejected because it could incorrectly replace a live Near
state with false. The corrected experiment is not integrated; cumulative
budgets, cancellation, ownership and full handoff remain open. See
`E:/codex-builds/feedback-scc-safe-saved.log`.
An isolated self-contained shadow then assumed simultaneous feedback updates
and Far restoration at phase 3, seeded once and driven by random controls. At
handoff phase zero it mismatched 182 of 720 resource checks in each of phases
0–2, and actor power mismatches by phase were `[105, 105, 225, 227, 107, 29]`
out of 1,024 checks per phase. The Graph/native oracle passed; the shadow did
not. The assumption is insufficient for a compiled device.
On an identical random-input Graph snapshot, both the 54- and 118-target
iterative circuits agree with each other but differ from the 54-target
recursive circuit on a public root and a shared feedback root. The divergence
is therefore between the builders, not evidence that adding targets caused
it. The SCC experiment is not a proven drop-in builder.
Against the same random-input Graph snapshots, recursive and iterative
repeater-root errors per phase were `[22,22,22,104,107,22]` and
`[7,7,7,104,107,22]`, respectively, out of 256 checks per phase. Both missed
`[0,0,298,298,0,0]` of 560 lamp checks per phase. Command snapshots matched
but do not capture their within-tick pulses. Neither settled model is an
equivalent replacement. See `E:/codex-builds/feedback-physical-comparison.log`.

An isolated one-shot shadow assumed plan-order activations, phase-5 rearm and
frozen unmodeled native inputs. It exceeded its 250,000 eligibility-visit cap
in the second random-input frame. In its only complete frame, 71 of 6,453
resource positions disagreed in phases 3 and 4 despite matching sampled power
and actor lifecycle roots. It retained 23,187 frozen non-control predicates and
176 unmapped observer slots. This assumption is insufficient and the model was
not activated. See `E:/codex-builds/virtual-firing-random.log`.
An affected-expression worklist reduced the first two frames to 15,736 and
18,051 actor visits, and separate location/enablement removed the initial
phase-3/4 location errors. But assuming every eligible actor fires produced
2,413 predicted firings in the second random frame, including actors with no
native request; state, revision and power comparisons then diverged. The
prototype still froze many native inputs and did not materialize a virtual
FIFO or handoff. See `E:/codex-builds/virtual-firing-worklist-random.log`.

A guarded lamp-receiver experiment made the real 84-wire clock branch eligible
for exact transport while preserving its ordered native lamp callbacks. The
96-tick saved replay and six handoffs passed. Against the same-binary
baseline it replaced 544 wire walks and elided 25,104 wire visits, but kept
318,014 piston samples and zero logical devices; one offline timing was 295
versus 288 ticks/s. No speedup is established and the experiment was not
integrated. See `E:/codex-builds/guarded-lamp-same-source-candidate.log`.

## First boundary slice: correct but insufficient

The conservative route handoff slice passes 33 focused tests and the release
`redpiler::` filter (752 passed, 95 ignored). Its pinned scale-5 executable is
`target/build-audit-tps/prod/fpu-scale5-pinned.exe`, SHA-256
`09BB106EF311F79AAD5A286C69D86CBE938409EFFF8D3A01CDC8CF84723BF752`.
Source `PLOT_SCALE` was restored to 4 after pinning.

The full 192-frame saved replay measures **372.90 game TPS**, with 73 installed
PullWave actors, 1,769 expression evaluations and only **79 actually bypassed
wire walks**. It still delivers **13,358,924 callbacks**, versus 13,358,912 in
the cleanup baseline. All 431,826 piston actions remain. This is not useful
saved-plot acceleration and does not approach the requested 10–40k range. The
different wall times are not a controlled attribution of speedup to this slice.

The replay compares 1,192 native ticks, all six handoff offsets, and 70,649
physical positions at handoff and after native continuation. Its exact manifest
port/readout mapping checks pass. It observes 135 distinct readout words, but
zero command emissions or command execution timestamp changes. That random
history alone does not cover command effects. The corrected explicit saved-plot
witness below closes that coverage gap; the small region test also exercises
nonzero `say` and `tellraw` output.

Reproduce from the pinned executable with `MCHPRS_PROFILE_FRAMES=192`:

```powershell
$env:MCHPRS_PROFILE_FRAMES = '192'
& ./target/build-audit-tps/prod/fpu-scale5-pinned.exe `
  redpiler::backend::graph::logical_device_saved_plot_tests::saved_fpu_neighbor_graph_fallback_profile `
  --ignored --nocapture --exact --test-threads=1
```

Artifacts: [full replay log](../target/build-audit-tps/prod/fpu-region-cut-profile-192.log),
[measurements and oracle](../target/build-audit-tps/prod/fpu-region-cut-profile-192.json),
[source patch](../target/build-audit-tps/prod/fpu-region-cut-source.patch), and
[new test source](../target/build-audit-tps/prod/fpu-region-cut-region_boundary.rs).
The patch applies to `d5de61002883ee08aa8dcac84373710962a25fd1`; the new test source
must also be placed at `crates/core/src/redpiler/backend/graph/tests/instant_layer_entry/region_boundary.rs`.
Building a replacement replay executable requires Rust `PLOT_SCALE=5`, release
LTO disabled and incremental compilation disabled; restore the source constant
to 4 after pinning that executable.

### Saved-plot command witness

The corrected selector-011 input emits eight tellraw outputs during 24 warmup
ticks and changes two command entities' execution timestamps. The following
16 changing-input frames match native execution every tick, at all six handoff
offsets and after native continuation, including that nonzero output history.
The first attempted witness reversed the opcode-port weights and selected 110;
its zero command history was a harness error, not evidence of a broken FPU.

The validated executable is `fpu-command-selector3-scale5.exe`, SHA-256
`EBE3207D397AC2F999463477BD61774E2650906C5F0C5CD1ADDA8CC0E4A83924`,
under `target/build-audit-tps/prod/`. Run the same exact test above with this
executable, `MCHPRS_PROFILE_FRAMES=16`, and
`MCHPRS_SAVED_PLOT=F:\rustrepos\MCHPRS\target\build-audit-tps\prod\p-1,-1-goal-start`.
See the [witness log](../target/build-audit-tps/prod/fpu-command-selector3-profile-16.log)
and [native oracle report](../target/build-audit-tps/prod/fpu-command-selector3-profile-16.json).
No physical FPU defect is demonstrated by these checks. This is native
equivalence and command coverage, not an arithmetic oracle or speed acceptance.

## What the measurements establish

On cleanup commit `d5de6100`, the production standalone benchmark and the same
benchmark using the fixed FPU schematic both fail their runtime activation
assertion. The saved plot runs through Graph at about 274 game TPS, with zero
logical evaluations and 13,358,912 callbacks over 1,152 measured ticks. Its
192-frame replay passes native comparison and all six handoff phases, but emits
zero commands. That establishes a useful fallback oracle, not acceleration or
command coverage. Commands need an exercised, nonzero history.

The earlier 43–48k TPS standalone measurements used a narrower boundary. They
demonstrated changing readouts and command activity, but did not establish full
input-indicator, observer-pulse and physical-state continuation equivalence.
They cannot be used as the correctness or performance baseline for the complete
saved plot. Reproduction commands and artifact links are in the
[build report](REDPILER_BUILD_REPORT_2026-10-25.md).

## The first architectural divergence

The saved scoped frame fails certification, before lowering or runtime
activation. This differs from the legacy whole-plot equation attempt, which
also encounters an equation budget failure. Increasing that budget does not
repair scoped admission.

`logical/islands.rs::partition` constructs an undirected interaction envelope.
`logical/settled.rs::compile_scoped_devices_with_plan` then requires one clock
per envelope. This makes interaction stronger than an execution-region boundary:
independently clocked devices can become one rejected candidate.

Within a candidate, `logical/native_inputs.rs::Builder::drain_power_traces`
adds traced electrical reads to a physical ownership closure. Callback receivers,
guards, actors and shared resources recursively expand it. Subsequently,
`settled.rs::scoped_computation_device_scope` rejects public outputs in that
closure, and `positive_computation_nodes` rejects overlapping equation support.
Read dependencies thereby become exclusive execution claims.

A recorded dependency is a native wire at `(-271,83,-191)`, a piston at
`(-270,82,-191)`, its Shape-East route `137996:0`, and an equation wire at
`(-267,82,-191)`. Continuing through real Update receivers reaches the terminal
node at `(-201,82,-244)` through route `134620:2`. These are diagnostic positions
and IDs from an earlier replay, not compiler selection rules. The important
property is the chain of actual reads and callbacks: removing an optional
terminal does not separate the native and equation cones. The saved bounding-box
crop also fails, but that crop contains the highlighted wire at
`(-201,82,-244)`. The user identifies it as part of an unconnected dead adder,
so the crop does not establish that this neighboring circuit was excluded.
The standalone fixture also declines scoped admission, but its first conflicting
wire is at fixture-local `(72,32,9)`: translated by the saved FPU origin, that
is the same `(-201,82,-244)` wire. It therefore does not establish an FPU-only
failure without this adder. This corrects the earlier assumption that the
standalone fixture excluded the reported dead net.

## Why deleting the overlap check would be wrong

The closure is overly strong as a *region discovery policy*, but its rejection
protects a real limitation of the executor.

`backend/graph/logical_device.rs::SettledFrame` evaluates settled equations at a
clock cut and publishes electrical results. It has adapters for particular
retained cells and outputs, not a general physical actor timeline. Graph skips
certified input callbacks into frame-owned nodes. It therefore cannot simply
continue executing arbitrary surrounding native circuitry while private actors,
wire transitions and reset callbacks disappear.

`backend/graph/projection.rs::project_future` serializes actual actor/resource
state and pending work. It cannot reconstruct missing history from final power
values. A settled value is not proof of a piston pose, a pending retraction, an
observer pulse, or the first source of a queued request.

The partial PullWave path has a related limitation. `Composition` describes
expressions relative to wave-entry roots; its ordered route steps do not make
those roots live after a foreign callback. Re-evaluating the original expression
against post-action state is also unsafe: it changes the meaning of entry-state
leaves after some transitions have already executed. A continuation needs a
residual program derived for its particular cut.

The old computation equations also intentionally discard physical information:
`compile_computation_with_reset_exclusions` retains shared-Far presence while
discarding which Near contact won the resource. Its observer-off and stable-wire
assumptions describe a qualified computation boundary, not every physical phase.
These values cannot be turned into a general physical materializer by adding
more output slots. Exact transitions must retain source-ordered selection,
captured pre-callback locals, revisions and pending work.

## Replacement contract

There are three separate questions:

1. Which pure equations compute a value?
2. Which executor exclusively updates each actor, resource and component field?
3. Which executor performs each observable effect, at what ordered occurrence?

A read binds a value at its actual execution cut. It does not transfer mutable
state ownership. Shared resources retain one owner across all actors that can
move them. Independent clocks remain independently scheduled.

The execution unit is a resumable region transition. Each admitted cut has
source-derived entry conditions, live reads, ordered writes and effects, and a
continuation derived for the resulting state. Its runtime rules are:

1. Enter only at the actual event/source occurrence for which the cut was proved.
2. Sample the cut's reads from authoritative current state.
3. Apply writes in native order. Publish preceding writes before an effect can
   observe them.
4. Invoke synchronous foreign callbacks at their original occurrence, including
   their captured receiver and properties. Let nested effects finish before
   resuming. Queue insertion and scheduled work remain separate operations.
5. After an external effect, use a certified residual cut with fresh live reads;
   never reuse an unproven frozen suffix.
6. Keep Graph's real FIFO and first request source authoritative. An unexpected
   popped request discards the predicted continuation before further effects.
7. At tick boundaries, reset or handoff, authoritative state and future must
   describe the exact native continuation. If a region keeps private state,
   materialization is part of its proof, not a presentation approximation.

An unsupported cut remains native. Surrounding supported cuts may compile when
their state ownership and interfaces are certified. A failure must not trigger
a whole-plot rejection or silently erase an effect.

## What to keep and what to retire

Keep Graph as the canonical fallback and queue owner; source route occurrences,
captured-site semantics, guards, actor acceptance and resource rules; pure
expression extraction and reduction; local cell transition evidence; and the
independent native replay oracle.

Replace the competing frame and wave ownership policies with one transition
contract. Retire their obsolete branches only as the replacement covers their
callers and tests. Do not preserve an unsound fast path to retain a benchmark
number. The abandoned sample-coalescing experiment is outside the source tree;
it did not address the architectural failure.

## Acceptance gate

Compilation counts and installed plans are not execution evidence. Acceptance
requires actual executed regions and a substantial callback/work reduction on
the changing-input saved plot, measured against the same revision and options.
The latest requested FPU target is approximately 10–40k game TPS for the actual
saved plot with changing inputs. The earlier strict 40k target was revised when
the user requested a scoped implementation plan covering COUNTER (at least 1M
game TPS), FPU and ADDERS. A standalone FPU fixture does not meet the saved-plot
target.

Correctness requires native outputs every game tick; FIFO order and source;
actor/resource state, reset and scheduled work; all six input-frame handoff
offsets followed by native continuation; nonzero command/tellraw output; and
independent, connected, differently clocked and unsupported-neighbor fixtures.
Performance does not waive any of these checks.

Native circuit health is a separate investigation. Zero commands or a constant
indicator trace alone does not establish a broken plot: the exercised operand
history and observation phase must first be checked. Report a demonstrated
physical defect or replay-protocol error separately from compiler rejection;
do not silently repair or simplify the saved circuit to obtain a benchmark.
