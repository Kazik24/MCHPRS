# Classic redstone implementation progress

## 2026-10-16: read-only discovery started

The existing graph has no committed-signal net implementation. The current
`contract-static-dust-paths` pass retains Turbo, intermediate wire writes and
callbacks, and therefore is not the requested optimization. Historical
`7470205` DirectBackend packed a node, channel and attenuation into each forward
link; that representation is useful but supplies no current callback-order proof.

The first census runs on the complete prepared graph, before changing execution.
Potential electrical wire relations define whole regions, independently of the
saved contact choice. Discovery follows conducted power to component leaves and
inventories canonical callback-only wire neighbors, variable ingresses, receiver
inputs, observers and synchronous boundary targets. Decline counts are
overlapping, so one candidate can explain more than one failed condition.

`classic_discovery_census` is an explicitly ignored test; its output is
`target/classic-discovery.json`. Structural candidate counts are not accepted
coverage: saved geometry/strengths, imported work and exact boundary effects must
still be certified. No runtime pass, proof bound or default enablement is chosen
from these preliminary counts.

The intended runtime state is one committed four-bit signal per accepted net,
updated at its original source-route entry occurrence. Retained tables must bind
that occurrence, owned wire IDs with attenuation, complete receiver inputs and
ordered effective scheduling operations. The boundary proof determines the
operation operands; the implementation must not infer correctness from final
strengths or device delays.

## 2026-10-16: opt-in implementation and admission contract

The pass is registered after preparation and before instant plans at O2+, behind
`CompilerOptions.classic_redstone` / `--classic-redstone`. It remains disabled by
default. It owns complete fixed wire regions, never a convenient subset. Local
pistons elsewhere do not reject a region; observer/contact/independent-input or
private-work boundaries do. Existing dust and instant ownership cannot overlap.

The compact certificate retains source, owned wire attenuations, complete direct
receiver inputs, and route-delivery ordinal bindings. Dense lowering produces
input/op ranges and one byte of committed signal per net. Each retained op holds
one receiver, sampled nonzero polarity, and a 256-bit old/new selection mask
(index `(old << 4) | new`). These masks select an ordered operation stream; they
are not 256 retained execution transcripts. Runtime executes those operations,
preserves live receiver powered/pending state and the existing scheduler, and
performs no private Turbo walk, callback replay or intermediate dust write.

Receiver queries combine committed signal attenuation with any proved direct
live ingress contribution. Wire blocks, inspection, flush/reset and continuation
reconstruct private strengths on demand. Private scalar storage intentionally
remains stale; native comparison normalizes that storage through the same public
projection. Export JSON adds typed net ownership, inputs, occurrence bindings,
compact masks and execution/proof edge markers. The viewer displays direct net
edges while retaining inspectable proof topology.

Admission does not infer that a delayed receiver sees only final wire power.
A cold Graph reference recorder checks all 256 ingress old/new strength pairs,
including each meaningful receiver sample and its order. Powered state cannot
change during the admitted synchronous episode; pending membership can only
become present, so first samples of each polarity suffice for all receiver
powered/pending states. Independent validation replays the supplied ordered
certificate against fresh samples rather than invoking the builder's compaction.
Cancellation is checked during proof and validation. Repeated graph validation
repeats this finite proof; it is a deliberate compile-time cost, not a cached
unchecked certificate.

The source output route must reach its first Live private Update through a
prefix containing only fixed/inert shape handlers and ordinary inert/delayed
handlers. Foreign wire propagation and other potentially synchronous emitters
before that occurrence decline. Only this first occurrence commits. Duplicate
later source occurrences and external ordinary output-event occurrences are
proved inert for all 16 stable signal strengths. Their closed origin plus the
non-reentrant prefix closes the interval where source newest and net committed
signals differ: pending jobs do not run inside synchronous route dispatch.
Ingress repeaters additionally require a constant-zero side query and saved
unlocked state, excluding silent locking output changes. Captured Site guards
without matching captured context are false; captured external wire entries
are unsupported. Shape entries do not commit signals.

Bounds were selected after the structural census: 32 wires per complete region,
2,000,000 reference-work units per attempted net, and 32,000,000 cumulative units
including declined proofs. The largest preliminary ANPU structural candidate
was 15 wires. Exceeding a bound leaves the whole region on general execution.

## 2026-10-16: measured coverage and acceptance limits

The initial prepared ANPU census found 34 structural candidates containing 381
wires and 35 receivers among 34,781 wires / 2,971 electrical regions. These were
not certified coverage. Independent upstream inputs at boundary callbacks are
common: downstream wire notifications also update a driving torch/repeater whose
own input depends on an unrelated signal. Such a receiver has two independently
variable inputs and cannot be represented by this first family's one-ingress
query. The implementation declines it; no ANPU speedup or general usefulness is
claimed. The pinned graph fixture `simple_and_gate` supplies a real accepted net.

The ordinary component census was actually executed with the test-only prepare
hook: 93 passed / 1 ignored, observing 134 preparations across 87 test names,
283 wire occurrences / 63 regions and 28 preliminary structural candidates.
These are repeated prepared-graph observations, not unique fixture coverage;
manual draft-only tests do not enter that hook. Raw records are in
`target/classic-ordinary-component-census-20261016.jsonl`.

Focused acceptance covers lengths 1/2/4/8/16, bent branches and fanout, delayed
torch feedback, powered/pending receiver combinations with imported ticks,
short/repeated pulses, FIFO/deadline/priority equality, projection/reset and
interpreter continuation cuts. Negative cases cover observers, contacts,
locking, independent upstream inputs, callback-only wires, stale strengths,
private imported work, captured guards, bounds and corrupt certificates.
The positive JSON fixture is exercised by 15 Python loader tests and 23 viewer
JS tests. Parent independently ran the ignored native DLL classic+O3 test with
Rust query/state verification successfully; its extended version additionally
checks code-cache sharing with independent mutable engine state.

The first paired release measurement on `simple_and_gate` used five alternating
pairs, each 2,000 lever interactions plus two ticks per interaction. Setup/proof
and verification were outside inference timing. Median O2 was 6.1823 ms,
classic 4.6286 ms, interpreter 7.0160 ms. This is a small real positive fixture,
not ANPU evidence. That run preceded the final stable-entry refinement and must
be refreshed before treating its exact counts/timing as final acceptance.

An attempted broad pre-existing experimental suite failed
`compiled_adder_regions_replace_internal_propagation_and_preserve_outputs`
(expected symbolic coverage 1, actual 0 under the existing default-disabled
synthesis path), then spent over five minutes in the explicit arithmetic-history
synthesis test. At the parent's instruction that process was stopped after
preserving output. The unrelated experimental failure was not rewritten.

## 2026-10-16: final conservative admission rerun

The refreshed complete census still accepts only one graph-fixture net and no
ANPU net. ANPU exact first-decline counts are: external/callback-only private
wire 2,372; contact-dependent geometry/power 357; wire bound 155; independent
main input 56; locked saved repeater 10; ingress count 9; potentially reentrant
source prefix 7; side/quasi reads 3; stale saved geometry 2. No reference pair
work is needed after these structural declines. Preparation plus census/proof
reporting took 28.99 seconds for ANPU in the debug test executable.

A concrete positional independent-input witness is the one-wire candidate at
(42,16,12), ingress (41,17,12), receiver (42,15,13). Private callbacks also reach
the ingress receiver, whose main query reads unrelated component (40,17,12).
The exact query cannot be encoded by one ingress signal. The candidate report
retains all source positions in `target/classic-discovery.json`.

After the stable-entry change, 10 focused tests passed (2 ignored) and the
ordinary Graph namespace passed 95 tests (1 ignored). This includes the existing
fixed-point adder FIFO counterexample. The latest debug real-fixture rerun also
passed, regenerating the positive export. Its exact work counts are 20,000 to
16,000 callbacks, 2,000 to 1,000 walks, 1,000 classic entry invocations and 1,000
receiver ops per sample. The accepted direct program uses 584 bytes and one
state byte. The remaining walks belong to an unowned region. Debug preparation
pipeline times were 0.351 ms O2 and 5.106 ms with the pass, including repeated
independent certificate validation; these are compile costs, not inference.
The debug timing output is explicitly labeled `profile: debug` and preserved in
`target/classic-real-fixture-debug.json`. The old release timing above is not
silently relabeled as a measurement of the final code.

## 2026-10-16: final release measurement and DLL integration

The final release executable was rebuilt after the conservative stable-entry
change. Five alternating O2/classic pairs on the pinned `simple_and_gate`
fixture passed final block and ordered-future checks. Per 2,000 interactions
(two ticks each), medians were O2 **6.5703 ms**, classic **4.9148 ms**, interpreter
**7.1444 ms**: classic used 25.20% less inference time than O2 (1.337x ratio).
Raw evidence is `target/classic-real-fixture-release-latest.json`; it is explicitly
labeled release. Exact counts/storage remain those reported in the final debug
rerun above: 20,000→16,000 callbacks; 2,000→1,000 walks; 1,000 classic entries
and receiver ops; 584 direct-program bytes plus one state byte. Repeated
certificate checking raises this tiny fixture's release pipeline time from
0.2732 ms to 4.1186 ms. This compile/inference tradeoff and one positive fixture
do not justify default enablement or an ANPU performance claim.

The current native-demo DLL integration also passed (1 test, 0.63 seconds),
including specialized query verification, O3/classic coexistence, reset and
interpreter continuation, and a second backend hitting the same immutable code
cache without sharing mutable state. The final positive export passed Python
15/15 and viewer 23/23 again. `git diff --check` passed.

Broader targeted checks passed 13 program-graph tests and 74 pass tests; the
remaining pass test exposed its expected default pipeline list missing the new
disabled classic registration. That test-only expectation is now corrected;
it must be rerun in the parent's next debug test cycle. A separate before/after
measurement isolating the new default-disabled hot-path branches was not made.
The pass remains opt-in while broader coverage/performance work is outstanding.
## 2026-10-16: parent follow-up checks complete

The corrected `optimization_levels_select_rewrites_before_dependent_plan_builders` test passed in the parent's feature-enabled debug test binary. The final native classic/cache test passed again after the native guard/context and capture-index work (0.65 s), and `cargo check --workspace --locked` passed including the server and feature-disabled core. These supersede the earlier pending pipeline-list rerun. The C BubbleSort experiment independently completed all 10,036 native shadow ticks plus its paired full sort; it accepted zero classic nets, so its native throughput gain is separate from this pass's small-fixture gain. Native performance details and remaining limits are in `LLVM_NATIVE_INFERENCE_DEMO.md`.

## 2026-10-18: explicitly unchecked full-ANPU experiment

The user authorized ignoring boundary safety to measure coverage, speed and
outputs. This experiment is compiled only under `cfg(test)` in
`backend/graph/tests/unchecked_classic_anpu.rs`; production admission, defaults,
certificates and native C are unchanged. It is an UNSAFE compiled weighted
final-state collapse, not the certified classic old/new-mask runtime.

The model accepts every typed component wire, without the production 32-wire,
one-ingress or boundary restrictions. Initial resolved directed wire edges form
weakly connected ownership groups; direction remains part of each attenuation
path. A breadth-first traversal limited by the four-bit signal range compiles
all external source-to-wire minimum weights. Multi-ingress maxima read live
component output strengths. Any external query containing selectable contacts
or state guards, including inactive branches, instead samples live general
block power at the wire. Only dust topology and the initial candidate boundary
selection are frozen; external contact power is not frozen to its initial value.

Execution writes final strengths and issues one callback per surviving boundary
site, in sorted wire / canonical-slot order, discarding intermediate dust states,
duplicate notifications, original Turbo ordering and meaningful transient samples.
Ordinary receiver handlers and the global scheduler still run. Nested callbacks
keep the experiment hook installed. A separate existing dynamic fixed-point mode
provides the broad live-geometry comparison.

The harness uses the existing frozen ANPU SHA, origin and start button, no
prewarm, all 1,024 screen pixels and the latest frozen frame at every game tick.
It continues through output mismatches, recording first differing pixel, state,
ordered scheduled work / pending membership, changed-frame count and whole screen
trace hash. The future comparison specifically covers scheduler entries and
pending membership, not all piston request/deferred internals. Three alternating
400-tick samples are followed by a 5,000-tick sample when practical.

Reported times are exploratory release UNIT-TEST execution, not production TPS:
mode ticks are interleaved, with state/future/output comparisons outside execution
timers, so caches are perturbed. Activation and projection are separate timers;
source sampling and explicit work counters are charged to execution. Existing
unit-test power-link instrumentation and trace branches remain in normal Graph;
the collapse bypasses many such queries. No claim isolates that instrumentation
or predicts an equivalent production speedup. No other compilation or performance
job runs while sampling.

### Unchecked v1: all wires covered, weighted output is wrong

The pinned v1 release unit-test executable is
`target/unchecked-classic-anpu-v1-tests.exe`, SHA256
`72c4e3ba581684e4571bfef8a77ef088db11bc79e05f1268ab442bc7800e7ca5`.
Raw reports/logs are `target/unchecked-classic-anpu-v1-{400,5000}.{json,log}`.
The weighted model compiles all **34,781 component wires** into **5,543** nets:
4,477 single-source, 1,019 multi-source, 47 zero-source; largest 189 wires;
66,183 retained source-to-wire terms and 1,803 live contact-block samples across
net source tables. There are no admission declines. This initial resolved
partition differs from the production census of potential electrical regions.
Its first extra weighted compilation took 87.6 ms (outside inference timers).

For three 400-tick samples, median execution was normal O2 **0.758306 s**,
dynamic fixed point **0.714011 s**, compiled weights **0.119330 s**. The apparent
6.355x compiled ratio is not a correctness-preserving speedup. Compiled queued
work/pending first differs at tick 12, full state at tick 150 (component at
102,36,62), and screen at tick 316: pixel 646 / position (148,77,52), actual 0
versus expected 1. It has 85 mismatching screen observations out of 401 and
retains only the initial screen frame, while the exact run has three frames.
The dynamic fixed-point run matches every screen/full-state observation through
400, but ordered queued work/pending differs first at tick 14 (375 observations).
The v1 combined diagnostic does not establish whether this is queue order only.

The 5,000-tick run completed without aborting on mismatches. Normal O2 took
**6.326481 s**, dynamic fixed point **5.649015 s** (1.120x ratio), and compiled
weights **0.898799 s** (7.039x apparent ratio). Normal and dynamic fixed point
both match all 5,001 frozen-screen observations, all 14 frames and full
CircuitState; their screen trace hash is `442eec413f9abf91`. Dynamic fixed point
still differs in the combined scheduler/pending diagnostic at 3,414 observations.
Compiled weights mismatch screen at 4,685 observations, state at 4,851 and
scheduler/pending at 4,987; its screen trace hash is `559362f421edac01`.

Actual 5,000-tick Graph callback counts are 16,389,687 normal, 5,062,613 dynamic
fixed point and 1,233,561 compiled. Walk counts are 183,752 / 347,433 / 0.
The compiled path invokes 594,685 nets, samples 2,112,709 source values, evaluates
23,338,323 weighted terms, writes 219,625 final wire powers, and delivers 582,253
boundary callbacks. It also performs fewer piston actions and schedules fewer
jobs, consistent with executing the wrong history. Projection times are separate:
0.411689 / 0.370148 / 0.260399 s. Activation times are separate as well.

All measurements retain the unit-test instrumentation/cache caveats above.
The dynamic solver's matched outputs do not certify future equivalence, other
inputs or circuits. V1's failure does not by itself identify a root cause:
its frozen boundary selection, arbitrary sorted-wire notification order and
final-state/source timing assumptions differ simultaneously.

The one follow-up ablation keeps the compiled weights and final writes but
uses root-first BFS over precompiled directed dust adjacency, live callback
selection, site deduplication and pre-dispatch snapshots like the dynamic solver.
It counts changed members outside the root's directed closure. New diagnostics
separate ordered scheduler, scheduler multiset and pending membership differences.
V1 evidence is preserved independently; no production admission changes follow.

### Boundary-order ablation and correction

V2 preserves `target/unchecked-classic-anpu-v2-tests.exe` (SHA256
`7958e8fcdfc098feae9dbd8a38e842edc2ac7a3522e39557f911e7ce0dfb8531`)
and separate `target/unchecked-classic-anpu-v2-{400,5000}.{json,log}` reports.
Its root-first traversal/live boundary selection exposed an implementation bug:
it still wrote all final powers across a weak ownership group, then notified
only the root's directed incoming closure. The diagnostic caught 53 changed
wires outside that closure. The circuit nearly stopped. V2 timings are discarded
as evidence of algorithm performance; this is an experiment implementation defect,
not a demonstration that preserving boundary order fails.

The added independent future diagnostics do clarify the dynamic fixed-point
comparison: through 5,000 ticks, 3,414 ordered scheduler observations differ, but
scheduler multisets (including positions, delay, type and priority) and pending
maps remain equal at every observation. Full CircuitState and all frozen screen
pixels also match. Thus the observed discrepancy is queue order within the
compared scheduler representation; unexamined future fields and other histories
remain outside that result.

V3 corrects the mismatch between write ownership and notifications: it finds the
root's directed incoming closure first and computes/writes only those members,
then notifies that same closure. All typed wires retain a compiled owner and
source weights. No production validation/default is changed. A final rebuilt
release executable repeats the same 400/5,000-tick measurements.

### Final coherent v3 result: compiled weighted run remains incorrect

The final executable is `target/unchecked-classic-anpu-v3-tests.exe`, SHA256
`e72d77528294c4a6b2876bb6ab90ec95f4782c662b4c1d405835d9eedcc107ea`.
Reproduce the normal short experiment with:

```powershell
& target/unchecked-classic-anpu-v3-tests.exe unchecked_classic_anpu_benchmark --ignored --nocapture
```

For the complete active window, set `MCHPRS_UNCHECKED_ANPU_TICKS=5000` and
`MCHPRS_UNCHECKED_ANPU_SAMPLES=1`. Exact reports/logs are preserved as
`target/unchecked-classic-anpu-v3-{400,5000}.{json,log}`. The default short run
uses 400 ticks and three samples. No compiler/other performance job ran while
sampling. A normal-Graph mismatch with the frozen screen would fail the harness;
experimental mismatches deliberately do not abort it.

V3 retains all 34,781 wire owners, 5,543 groups and the same source/weight
coverage as v1. Each invocation evaluates and writes only its directed root
closure. Changed wire writes outside that closure are **zero**. Source sampling
still includes the full owning group's live source table, and is charged to
execution. Extra weighted compilation took 60.9 ms in the 5,000-tick run,
outside activation/execution/projection timers.

| Execution mode | 400 ticks, median of 3 | 5,000 ticks, one sample | Frozen screen / full state |
| --- | ---: | ---: | --- |
| Normal O2 Graph | 0.631572 s | 5.740476 s | All observations match |
| Dynamic fixed point, unchecked | 0.603147 s | 5.158906 s | All observations match |
| Compiled weighted v3, unchecked | 0.156709 s | 1.253482 s | Wrong |

The compiled ratios are 4.03x (400) and 4.58x (5,000), but do **not** establish
a correct speedup. At 5,000 ticks, compiled state differs at 4,851 observations,
first at tick 150, component (102,36,62). Pending maps and queued job multisets
also first differ at tick 150 and differ at 4,851 observations. Ordered scheduler
entries first differ at tick 14 and differ at 4,975 observations. The screen
first differs at tick 316, pixel 646 / (148,77,52), actual 0 versus expected 1;
4,685 observations mismatch. Only the initial frame survives. Its whole trace
hash is `559362f421edac01`, versus exact `442eec413f9abf91`.

Dynamic fixed point uses about 10.13% less execution time in the long sample
(1.113x ratio). It matches all 5,001 screen/full-CircuitState observations and
all 14 frozen frames. Pending maps and queued job multisets match at every check.
Ordered scheduler entries differ at 3,414 observations, first tick 14. Thus it
is an interesting output-matching result for this one pinned activation episode,
not scheduler equivalence or a general redstone correctness result. The report
still does not compare every piston request/deferred field.

Actual 5,000-tick work:

| Work | Normal O2 | Dynamic fixed point | Compiled v3 |
| --- | ---: | ---: | ---: |
| Graph callbacks | 16,389,687 | 5,062,613 | 1,270,663 |
| Wire walks | 183,752 | 347,433 | 0 |
| Scheduled ticks | 267,998 | 267,998 | 80,562 |
| Piston actions | 10,682 | 10,682 | 7,367 |
| Projection time | 0.360752 s | 0.332406 s | 0.233703 s |

Compiled-specific counters: 621,348 net invocations; 2,217,304 source samples;
23,596,628 weighted terms; 219,624 final wire writes; 616,836 boundary callbacks.
Lower work partly reflects the wrong/stalled execution and cannot be credited
as useful optimization. Activation was separately measured at 0.240 / 0.048 /
0.039 ms, respectively. Instrumentation, interleaving, cache effects and the
single long sample remain the explicit limits of these unit-test timings.

The coherent root-closure/live-boundary variant does not repair compiled output.
This rules out treating v1's arbitrary sorted notification order alone as an
established explanation. Frozen dust topology, sampled source/transfer assumptions
and lost transient samples still differ; this experiment does not identify which
causes the first substantive divergence. No further algorithm variants were made.

Final pinned release checks passed: ordinary Graph namespace **96 passed / 1
ignored**; `classic_` focused filter **11 passed / 3 ignored** (including the new
algebra check and excluding explicitly ignored full/native experiments).
`git diff --check` passed. Production admission/defaults and native C/compiler
size are unchanged. Parent runs the final workspace compilation check separately.

### Final checks and reproduction

Parent's final `cargo check --workspace --locked` passed (3.77 seconds), with
13 existing warnings from disabled symbolic experiments. Targeted rustfmt check
of the new experiment module passed. No additional test or timing run followed.
The last diff check passed; a subsequent attempt to preview a report used the
wrong filename, which did not change the successfully written reports or notes.

The preserved final evidence is [400 ticks / 3 samples](../../../target/unchecked-classic-anpu-v3-400.json)
and [5,000 ticks / 1 sample](../../../target/unchecked-classic-anpu-v3-5000.json),
with [short log](../../../target/unchecked-classic-anpu-v3-400.log) and
[long log](../../../target/unchecked-classic-anpu-v3-5000.log). The final executable
hash remains `e72d77528294c4a6b2876bb6ab90ec95f4782c662b4c1d405835d9eedcc107ea`.

Execution plus separately timed projection totals for the long sample are
6.101228 s normal, 5.491312 s dynamic fixed point, and 1.487184 s incorrect
compiled v3. These are still instrumented unit-test measurements, with activation
and setup excluded.

To rebuild and repeat the short experiment from source:

```powershell
$env:MCHPRS_UNCHECKED_ANPU_TICKS='400'
$env:MCHPRS_UNCHECKED_ANPU_SAMPLES='3'
cargo test -p mchprs_core --release --lib unchecked_classic_anpu_benchmark --locked -- --ignored --nocapture
```

To repeat the long episode using the preserved final executable:

```powershell
$env:MCHPRS_UNCHECKED_ANPU_TICKS='5000'
$env:MCHPRS_UNCHECKED_ANPU_SAMPLES='1'
& target/unchecked-classic-anpu-v3-tests.exe unchecked_classic_anpu_benchmark --ignored --nocapture
```

## 2026-10-18: causal diagnosis and conservative hybrid

Two separate local witnesses explain why blindly accepting ANPU was unsafe.
The bounded debug recorder compares compiled transfer and an independently
collected LIVE adjacency / block-power / 15-round solver on the same current
state, before weighted writes. It retains captured/live root samples, source
terms, neighbor selections, contact observations and piston state. Evidence is
`target/unchecked-classic-local-v3-start0.{jsonl,log}` and
`target/unchecked-classic-local-entry-fixed-t150.{jsonl,log}`.

First, at tick 14, root (123,29,71) has sampled/live old power 0 and ordinary
seed power 0. Graph and the live fixed-point mode therefore do not enter a
wire episode. V3 nevertheless reads already-new source Node13961 strength 15
and writes root 7 (attenuation 8) and wire (123,28,72) strength 8 (attenuation 7).
Frozen and live incoming neighbors agree at this witness. This is premature
publication at an unchanged seed, not proof that a nearby piston caused it.
The test-only hook now runs only AFTER the ordinary seed equality check and
root publication. A genuine admitted episode still forces its root boundary
notification even if weighted evaluation makes no additional writes. A tiny
three-wire regression sets a source to 15 without delivering its output route,
then checks that a far seed remaining 0 does not invoke or publish the net.

Second, fixing entry semantics alone exposes a real topology failure at tick
150. At root (112,37,75), sampled old is 0, ordinary seed is 14, and the live
solver yields 14; frozen weights yield 0. Live incoming now includes powered
wire Node24706 at (113,38,75), strength 15, absent from the initial adjacency.
The ABOVE site70528 / Contact1472 at (112,38,75) was initially black concrete
and is now air, opening the staircase connection. Piston490 at (112,40,75) is
extended, facing down, with head absent and BasePending state (revision 1) at
this observation. Repeated compiled rewrites of seed 14 back to 0 cause nested
notifications and a cycle between roots (112,37,75) and (110,37,75). A 64 MiB
thread-stack rerun also overflows: this was not merely the debug default stack.
The later first post-tick difference at (102,36,62) is downstream evidence,
not the earliest local cause.

The user explicitly requested preserving piston and other effectful interfaces.
The hybrid forms whole potential electrical wire regions over all possible
staircase slots and recursively conducted wire dependencies, ignoring guard
selection and including contact variants. It marks regions for ordinary Turbo
when they touch selectable contacts/pistons, resource/contact guards, all possible
captured/guarded effectful callback receivers, observer/reset watches, side/quasi
reads or other synchronous effectful power readers. Preservation also propagates
backwards across cross-region wire callbacks. Delayed torches, proved unlocked
repeaters and inert controls can remain external handlers. Entire regions fall
through to ordinary Turbo with fixed-point mode explicitly false; the experiment
hook stays installed for nested eligible calls. No arbitrary piston radius or
global exclusion of otherwise unrelated pistons is used.

Concurrent user cleanup deleted/refactored unrelated symbolic/native/export
code. The first hybrid debug build encountered 141 unrelated missing-helper
errors in that work in progress. It was not reverted. Validation proceeds in
an isolated detached HEAD576ae31 worktree at
`F:/rustrepos/MCHPRS-boundary-experiment-20261018`, overlaying only the experiment
module and minimal test-only Graph/wire/fixed-point hooks. The only main-file
cleanup typo repaired was the explicitly authorized test-recorder getter.


## 2026-10-18: compiler experiment removal

C/LLVM and the older disabled finite/symbolic synthesis code have been removed.
The certified classic pass, Rust direct-net execution, JSON/viewer support and
the existing test-only unchecked ANPU experiment remain. Native integration
results above are historical: the DLL feature, APIs and native-only tests no
longer exist in this checkout. Sparse contact projection remains shared Graph
behavior. Production admission and default enablement were not broadened.

The complete pre-removal compiler source is preserved in commit
`576ae3137492dfa3dcacaf787ac6c9e482840d6e`. See
[ALGORITHMS_AND_RECOVERY.md](ALGORITHMS_AND_RECOVERY.md) for the current classic
algorithm, archived compiler algorithms, source locations and recovery steps.

### Hybrid debug validation and exact preservation coverage

The isolated hybrid debug run passes all 5,001 screen observations, all 14 frozen
frames, full CircuitState, pending maps, queued-job multisets, and the newly
compared ordered piston requests / deferred work / clock. Ordered scheduler
entries still differ at 974 observations, first tick 144; this is not future
execution equivalence. Raw debug reports are
`target/unchecked-classic-v4-debug-{400,5000}.json`.

The source at the tick-14 premature-publication witness, Node13961, is now
resolved as an ordinary delay-1 repeater at **(127,28,68)**. The new entry regression
passes. The small piston preservation fixture was adjusted so its base has a
normal compiled component site; it then passes, showing that an entire three-wire
contact region is preserved while a remote independent three-wire line remains
compiled. All three focused unchecked regressions pass; the full experiment
remains explicitly ignored.

The hybrid keeps **32,757 of 34,781 wires** on ordinary O2 Turbo and compiles
**2,024 wires** in **483 initial static nets**. There are 2,971 potential electrical
regions: 2,491 preserved and 480 eligible (initial resolved nets can split one
potential region). Reasons overlap and are counted after whole-region closure:

| Preservation reason | Regions | Wires |
| --- | ---: | ---: |
| Callback into preserved region | 1,406 | 13,181 |
| Contact/resource callback guard | 448 | 10,814 |
| Contact/resource power guard | 236 | 4,045 |
| Effectful callback receiver | 1,070 | 19,158 |
| Effectful electrical dependency | 281 | 6,822 |
| Effectful power reader | 350 | 11,209 |
| Observer/reset watch | 32 | 32 |
| Piston/contact dependency | 3 | 307 |
| Piston/effectful neighborhood | 1,073 | 19,407 |
| Selectable contact callback | 448 | 10,814 |
| Selectable contact dependency | 236 | 4,045 |
| Selectable contact neighborhood | 461 | 11,485 |
| Side/quasi power reader | 90 | 5,789 |

The long debug trace executes both paths: 11,164 weighted invocations, 59,635
weighted final writes and 127,707 boundary callbacks, alongside positive ordinary
fallback walk work. No timing claim uses these debug measurements.

The stronger future checks also qualify the historical dynamic fixed-point
result: although its screen/full state/pending/job multisets match in the same
5,000-tick trace, ordered piston/deferred/clock observations differ at **1,461**
checks (the short trace first differs at tick 74). Thus earlier order-only
statements apply specifically to scheduler entries, not every future-work field.
The new hybrid does not exhibit that piston/deferred difference in this trace.

### Final isolated release hybrid: observed output fixed, no useful speedup

The frozen executable is `target/unchecked-classic-anpu-v4-hybrid-tests.exe`,
SHA256 `2b2d51fad82f8a5772e1e94c7e3cc7e20d549b5db6bd97188d76257ffbe1c572`.
It was built from isolated HEAD576ae31 plus the synchronized test-only overlay,
so its embedded fixture root is the isolated worktree. Main/source and isolated
experiment, wire and fixed-point files were synchronized; no production
admission/default or native C change was made by this experiment. Concurrent
user cleanup remains separate.

Final release measurements, with no other compilation/performance job active:

| Mode | 400 ticks, median of 3 | 5,000 ticks, one sample |
| --- | ---: | ---: |
| Normal O2 Graph | 0.651153 s | 5.896580 s |
| Dynamic fixed point, unchecked | 0.618797 s | 5.355971 s |
| Seed-admitted weighted hybrid | 0.648084 s | 5.993920 s |

The short median differs by only about 0.47%, within this exploratory run's
variation. In the long sample the hybrid is **1.65% slower**, so this experiment
shows **no useful speedup**. Execution plus separately measured projection is
6.281922 s normal versus 6.358326 s hybrid (1.22% slower). Projection alone is
0.385343 / 0.364405 s, and activation is 0.242 / 0.034 ms, respectively.
Extra hybrid compilation/mask construction took 165.9 ms, outside those timers.
All earlier unit-test instrumentation/interleaving/cache caveats still apply;
these values are not production TPS estimates.

The hybrid now matches all 5,001 frozen screen observations, all 14 frames and
full CircuitState. Trace hash is exact `442eec413f9abf91`. Pending maps, queued-job
multisets and ordered piston requests / deferred work / clock also match at
every observation. Ordered scheduler entries differ on **974** observations,
first tick **144**, so there is still no future-order equivalence or general
correctness certificate. The first mismatch is at queued ordinal 22: normal
orders two-tick Normal-priority jobs at (125,44,62), (125,44,64), (125,44,66),
while the hybrid orders the same job set beginning at (125,44,76), (125,44,74),
(125,44,72). Full first queue windows are retained in the JSON report. No further
algorithm changes were made after observing this limitation.

Actual 5,000-tick work shows both paths execute:

| Work | Normal O2 | Hybrid |
| --- | ---: | ---: |
| Graph callbacks | 16,389,687 | 15,835,892 |
| General wire walks | 183,752 | 172,629 |
| Scheduled ticks | 267,998 | 267,998 |
| Piston actions | 10,682 | 10,682 |
| Piston samples | 327,585 | 327,585 |

Hybrid-specific work: 11,164 weighted invocations, 14,541 source samples,
85,493 weighted terms, 59,635 weighted final writes (ordinary admitted root
publication is separate), and 127,707 boundary callbacks. Writes outside the
directed root closure are zero. Only **2,024 wires (about 5.82%)** are compiled;
**32,757 (about 94.18%)** retain ordinary Turbo under the whole-region protection
rules. This conservative preservation explains why the previous incorrect
all-wire ratios do not translate into useful performance here.

Final frozen release tests passed: focused unchecked regressions **3 passed /
1 ignored**; ordinary Graph **96 passed / 1 ignored**; `classic_` filter
**13 passed / 3 ignored**. The larger main-workspace cleanup is not silently
restored or represented as validation of this isolated snapshot.

Final linked evidence: [short report](../../../target/unchecked-classic-v4-release-400.json),
[long report](../../../target/unchecked-classic-v4-release-5000.json),
[short log](../../../target/unchecked-classic-v4-release-400.log),
[long log](../../../target/unchecked-classic-v4-release-5000.log).
The JSON mode named `compiled_weighted_unsafe` is the hybrid when the report's
`coverage.preservation` is populated; exact retained/general counts are explicit.

Repeat the short measurement using the preserved executable (keep the isolated
fixture worktree present):

```powershell
$env:MCHPRS_UNCHECKED_ANPU_HYBRID='1'
$env:MCHPRS_UNCHECKED_ANPU_TICKS='400'
$env:MCHPRS_UNCHECKED_ANPU_SAMPLES='3'
& F:/rustrepos/MCHPRS/target/unchecked-classic-anpu-v4-hybrid-tests.exe unchecked_classic_anpu_benchmark --ignored --nocapture
```

Use `TICKS=5000` and `SAMPLES=1` for the long run. To rebuild the isolated frozen
overlay, run from `F:/rustrepos/MCHPRS-boundary-experiment-20261018`:

```powershell
$env:CARGO_TARGET_DIR='F:/rustrepos/MCHPRS/target'
cargo test -p mchprs_core --release --lib unchecked_ --locked --no-run
```

Final integration check: parent's `cargo check --workspace --all-targets --locked`
passed on the current main workingtree in 4.43 seconds, without warnings. This
also compiles the experiment tests against the completed concurrent cleanup.
The measured executable still belongs to the explicitly recorded isolated
HEAD576ae31 snapshot; the check does not relabel its measurements as a main-tree
benchmark. Targeted rustfmt and owned-file diff checks passed.

No production integration or default enablement follows. The remaining ordered
scheduler mismatch is a real limit, and genuine multi-ingress episodes still
lack a proof against sampling another ingress's newest undelivered output. The
next useful investigation is the first tick-144 receiver/scheduling trace:
preserve original signal-commit occurrences and first effective receiver attempts
before trying to lower more protected wiring. The present hybrid fixes the
observed screen/state failure but does not deliver a useful speedup.

### Follow-up: the 94% preservation rate is an over-approximation

The user correctly challenged interpreting the hybrid's low admission as an
inherent property of ANPU. This mask preserves whole potential electrical regions
when any member meets its narrow effectful-receiver rules, then propagates
preservation backward across all wire callback deliveries. Of the 32,757 retained
wires, 19,576 belong to initially marked regions and **13,181 additional wires**
are retained solely by that callback propagation. This is not a measurement of
how many wires must remain interpreted with precise boundary operations.

The experiment also unions every candidate staircase slot without first
eliminating edges whose fixed geometry guards can never hold, and scans callback
variants without filtering statically false guards. Its ordinary receiver list
is deliberately narrow: lamps, comparators and potentially locked repeaters can
cause whole-net preservation. These are conservative experiment shortcuts, not
proof that all incoming wiring must retain Turbo walks.

The next useful design is to cut at real interfaces: keep stateful component
handlers and genuinely movable topology on general execution; compile fixed
wire interiors; retain each input's last delivered value; and publish the ordered
receiver samples/callbacks at those interfaces. Physical proximity or a callback
path alone must not infect the entire upstream network. The remaining FIFO
witness is evidence that final strengths alone do not implement this contract.
No new coverage or speed claim follows from this analysis; those require a new
boundary-aware census and implementation.

## 2026-10-18 — narrower direct boundary mask and ordered native-episode tapes (v5)

The next experiment replaces the v4 final-strength collapse with a guarded straight-line `Capture / WriteWire / Call` program recorded from one complete ordinary Turbo episode. It is test-only (`MCHPRS_UNCHECKED_ANPU_TAPES=1` in the existing ANPU harness), does not change certified classic admission/defaults, and retains intermediate wire writes. Ordinary seed equality and root publication run before cache lookup. A hit guards the actual initial consulted wire shapes/powers and emitting component powered/locked/output fields; it does not replace committed wire state with newest-source maxima. Pending membership is not a cache key. Boundary captures are rebound at their original discovery positions, and ordinary handlers execute in the original callback order against current scheduler/receiver state.

The structural mask now marks only direct dynamic neighborhoods, power dependencies, dynamic delivery selection, and unsupported synchronous/effectful handlers. It does not propagate rejection across whole potential wire regions. A native episode crossing a marked wire is recorded as a fallback and completes in its original Turbo frame; it is never split into a newly seeded walk. Specialized Observer and component-form Piston update handlers remain conservatively unsupported in this version, even though their delayed/queued behavior may permit a later precise extension. Enum-form piston callbacks are allowed if all other topology/query conditions remain fixed. Selectable contacts remain a direct boundary.

The ANPU census is 34,781 wires: 5,337 direct core wires (15.34%) and 29,444 structurally eligible wires (84.66%), with **zero propagated-region rejection**. Overlapping reason counts are 4,400 direct contact neighborhoods, 1,803 direct dynamic power queries, 3,767 dynamic delivery selections, and 1,788 unsupported handlers. The tri-state guard filter rejects unknown state/contact predicates and prunes statically false branches in the matching captured-target context; this pinned graph has zero statically impossible Turbo deliveries under that filter. Structural eligibility is not replay coverage.

Retained tapes are bounded to eight variants per entry root and 32 MiB of boxed opcode/guard payload plus tape/Arc headers; fixed lookup/mask/set allocation overhead is excluded from that payload limit. Learning seals after the first otherwise acceptable tape exceeding the remaining byte budget. Full variant buckets and sealed-cache misses execute ordinary Turbo without recording. Aborted recordings are discarded before capture compaction. Accepted tapes retain only captures consumed by a boundary Call, remapping slots while preserving each retained discovery position and every initial-state guard. All emitting captured neighbors remain conservatively keyed, including potentially irrelevant emission directions; this creates safe extra misses and competes for the eight-variant limit.

The first three focused regressions pass: an unchanged seed cannot publish an undelivered lever output; source property changes force a miss; and the same tape hits across changed lamp captured powered/locked flags and pending membership while matching native state/scheduling. The meaningful lamp-powered change changes scheduling behavior; its lock field is inert. The independent canonical adder regression passes three cold/reset repetitions plus native continuation, with 135 ordinary root-core fallbacks and zero cache hits: this validates preservation around that boundary, not cached arithmetic coverage.

Before capture compaction, debug ANPU checks matched every screen pixel, full component/resource/piston state, ordered queued jobs, pending membership, and ordered piston/deferred/clock representation at all 5,001 observations through 5,000 ticks (all 14 frozen screen change frames). The preserved interim report is `target/unchecked-classic-v5-precompact-debug-5000.json` and matching `.log`. This version was slower (6.9086574 s normal versus 10.8084773 s tapes) and saturated its payload budget with unused captures; it is not the final timing candidate. Compaction then passed the three focused regressions and another 400-tick exact comparison: 6,682 hits of 18,955 admitted episodes, 17,879 wires represented by accepted tapes, 376,737 private wire callbacks elided, 7,760 variants, and 15,504,484 retained payload bytes. Captures replayed fell from 973,863 to 30,612 in the corresponding 400-tick checks. The final release measurements below include the additional cold-path rejection shortcuts and header accounting.

### Final v5 release measurements and validation

The final executable is [unchecked-classic-anpu-v5-ordered-tapes-tests.exe](../../../target/unchecked-classic-anpu-v5-ordered-tapes-tests.exe), SHA-256 `63da5609d0f2baa6e7de51b75c40a605dac6ce5f6030541dcc1350a50bbe7436`. It was built from the main working tree, including the completed cleanup and these test-only additions, rather than the detached v4 snapshot. A concurrent user commit `ba9bfb6` (`docs`) incorporated most of this work; the final cold-path changes remained in the working tree during validation. No production admission/default changed.

The three alternating 400-tick samples and one fresh-cache 5,000-tick sample produced:

| Window | Normal Graph execution | Ordered tapes execution | Result |
| --- | ---: | ---: | --- |
| 400 ticks, median of 3 | 0.6868943 s | 0.8595157 s | 25.13% slower using the ratio of median times; median paired slowdown 28.51% |
| 5,000 ticks, one pair | 6.1871434 s | 5.5526765 s | 10.25% less execution time, 1.1143x speedup |
| 5,000 ticks, execution plus projection | 6.5830853 s | 5.9336633 s | 9.87% less combined time |

The 5,000-tick projections were 0.3959419 s normal and 0.3809868 s tapes. Activation was separately measured (0.0003297 s / 0.0002553 s); there was no prewarm. Every one of the 5,001 post-activation observations matched the frozen screen, full graph state, ordered scheduled jobs, job multiset, pending membership, and ordered piston request/deferred/clock representation. All 14 frozen screen frames appeared. Every 400-tick sample also had zero mismatches in all these categories. The dynamic fixed-point comparator still matched current state/screen but differed in FIFO at 3,414 long-window observations and piston/deferred/clock representation at 1,461; it remains unsuitable as an equivalence result.

Actual long-window tape coverage/work differs substantially from structural eligibility:

- 108,990 replay hits and 74,762 native reference episodes out of 183,752 admitted seed episodes: **59.31% hit rate**. The native episodes consist of 11,423 accepted first-recordings, 21,917 core-crossing fallbacks, 10,672 direct-root-core fallbacks, and 30,750 full-variant-bucket fallbacks. The cache did not reach its byte cap or seal.
- 20,205 distinct wires (58.09% of all ANPU wires) occur in accepted tapes. This is accepted-program coverage, not a claim that every represented wire was replayed. Structural eligibility remains 29,444 wires (84.66%). Fallback prefix coverage is only the recorded prefix before an abort; it must not be presented as all wires subsequently visited by native fallback.
- 11,423 variants retain **23,967,148 payload bytes** (about 24.0 MB / 22.86 MiB), excluding fixed masks/indices, BTree allocation overhead, and transient recording allocations. No source-mutation or reentry fallback occurred in this workload.
- Private wire callbacks fall from 16,389,687 total normal callbacks to 10,648,108 total callbacks: **5,741,579 private callbacks removed**, 35.03% of the original total. Native walks fall from 183,752 to 74,762. Replay still performs 1,547,652 intermediate wire writes, 505,644 rebound captures, and 1,510,910 boundary Calls. This is not a strength-only direct-net runtime and does not eliminate private dust storage.
- Normal and tape executions both perform 267,998 scheduled ticks, 327,585 piston samples, and 10,682 piston actions. The independent adder regression again passes all three repetitions/reset/continuation checks, with 135 root-core fallbacks and no replay hits.

The raw reports/logs are [400 JSON](../../../target/unchecked-classic-v5-release-400.json), [400 log](../../../target/unchecked-classic-v5-release-400.log), [5,000 JSON](../../../target/unchecked-classic-v5-release-5000.json), and [5,000 log](../../../target/unchecked-classic-v5-release-5000.log). The metric named `build_seconds` (1.9840911 s in the long run) measures only recorded-native learning time around the reference walk. It excludes later compaction/admission allocation and native work from early fallback. **Execution timing includes all these costs**, cache lookup, guard checks, and live source sampling. The separately reported setup `compile_seconds` also includes the legacy weighted-model census construction retained by the shared harness; it is outside the execution comparison.

These are release **unit-test** measurements of one frozen ANPU episode, with tick-interleaved modes and alternating mode order, not production TPS or a general proof. Full state/future/screen comparisons and projection run outside execution timers and perturb caches. The existing test-only query recorder increments `power_links_examined` on the general power path. The 400-tick cold loss and single long-pair sample limit the performance claim to this measured workload. No promotion or integration follows from it. The narrower boundary experiment establishes that retaining native sample/FIFO order can preserve this workload while replaying useful fixed interiors; its deliberately conservative source guards, finite variant bound, and whole-episode core-crossing fallback remain constraints.

To reproduce from the repository root in PowerShell (use the pinned executable for the exact reported build):

```powershell
$env:MCHPRS_UNCHECKED_ANPU_TAPES = '1'
$env:MCHPRS_UNCHECKED_ANPU_TICKS = '400'
$env:MCHPRS_UNCHECKED_ANPU_SAMPLES = '3'
& .\target\unchecked-classic-anpu-v5-ordered-tapes-tests.exe unchecked_classic_anpu_benchmark --ignored --nocapture
$env:MCHPRS_UNCHECKED_ANPU_TICKS = '5000'
$env:MCHPRS_UNCHECKED_ANPU_SAMPLES = '1'
& .\target\unchecked-classic-anpu-v5-ordered-tapes-tests.exe unchecked_classic_anpu_benchmark --ignored --nocapture
```

Final validation: the frozen release executable passed the three focused tape regressions; the complete Graph namespace passed **155 tests, 4 ignored** in 18.11 s, including the independent adder FIFO regression; the `classic_` filter passed **16 tests, 3 ignored** in 0.29 s (overlapping the Graph namespace). Logs are [Graph regressions](../../../target/unchecked-classic-v5-release-graph-tests.log), [classic regressions](../../../target/unchecked-classic-v5-release-classic-tests.log), and [focused regressions](../../../target/unchecked-classic-v5-release-tests.log). Targeted rustfmt and owned-file diff checks passed. The parent independently ran `cargo check --workspace --all-targets --locked` on the current main checkout: **passed in 4.64 s, no warnings**. No further builds or algorithm variants were run after these results.

## 2026-10-18 — CHUNGUS becomes the fixed-topology synthesis framework

The user redirected the next synthesis experiment to the existing CHUNGUS Mandelbrot plot. The unchanged fixture `crates/core/benches/chungus_mandelbrot_plot` is 6,602,379 bytes, SHA-256 `31624f7aa8e050f065f6858ba54007fe75c6f33e333138ee77d8dd6cc5291d78`. The existing benchmark loads the full `(0,0,0)..(255,255,255)` plot, presses the floor stone button at `(187,99,115)` once without prewarm, and offers a historical full run of 12,411,975 ticks. It supplies no frozen output reference; the independent interpreter is therefore the next bounded oracle. No full-length run has been attempted.

A new ignored discovery test, `chungus_fixed_topology_inventory_and_preparation_census`, first saves a load-only census, then invokes required graph preparation under the unchanged 1 GiB table-estimate bound and the existing maximum 8x analysis-work budget. It does not invoke the tape cache or infer synthesis eligibility from tape limits. Discovery found 506,126 non-air blocks over 2,138,112 inspected cells, zero pending ticks, zero pistons/heads/moving blocks, zero contacts/resources/observations in geometry, and no admission issues. It found 251,485 electrical actors:

| Actor | Count |
| --- | ---: |
| Wire | 133,623 |
| Repeater | 53,959 |
| Torch | 26,467 |
| Comparator | 21,944 |
| Constant / comparator-override source | 9,776 |
| Lamp | 5,602 |
| Lever | 74 |
| Button | 17 |
| Hopper | 17 |
| Pressure plate | 6 |

There are no observers, copper bulbs, command blocks, trapdoors, or movable geometry in this inventory. Initial dust-power dependencies have 179,592 directed edges and 30,731 weak components, largest 113 wires; these are initial-world dust connectivity counts, not synthesized source-channel counts. The `initial_nonzero_output_actors` field reports `identify_block`'s stored NodeState strength, which includes powered lamp state; it must not be interpreted as a count of physically emitting sources.

The unmodified baseline reproduced the preparation failure in 2.9616353 s (debug): **callback estimate 1,116,643,632 bytes exceeds 1,073,741,824 bytes**. Completed electrical preparation contains 1,032,939 sites, 804,778 power links, 1,603,476 wire relations, and 21,944 comparator input plans. Its guards contain 8,819,119 entries at capacity 16,777,216, estimated at 417,618,960 bytes; the complete recorded electrical table estimate is 587,244,824 bytes. Callback preparation declines before a complete callback table snapshot or executable graph exists. These are construction/storage limits, not a structural reason this fixed circuit cannot be synthesized. No memory bound was raised and no required semantics were omitted.

Baseline evidence is preserved as [inventory](../../../target/chungus-baseline-inventory.json), [preparation census](../../../target/chungus-baseline-census.json), [log](../../../target/chungus-baseline-census.log), and [frozen debug executable](../../../target/chungus-baseline-census-tests.exe), executable SHA-256 `b81ccabd942e247c4201012ac6159ef92486634efd99974b0c60e960459e1826`. An initial compile encountered a census-only incorrect `StickyPiston` variant name (fixed to the actual combined Piston representation), and a concurrent `instant_contract.rs` mutability edit fixed by its owner; that failed build is separately preserved in `target/chungus-baseline-initial-build-failure.log`.

The next bounded scaffold is `chungus_bounded_interpreter_graph_differential`, default 64 ticks (`MCHPRS_CHUNGUS_TICKS` overrides). It will compare every actor's raw block state, every comparator entity output, ordered pending TickEntry/FIFO and logical clock against the interpreter after the original button activation and every tick, saving first mismatches before asserting. It also inventories O2 fixed query facts, compiled transfers, remaining runtime geometry, hop roots, guards/routes and actual work. This is a baseline/framework test, not a claim of complete Mandelbrot computation or static synthesis acceptance. The proposed compiled representation retains ordered graph fanout, source-route occurrences, native seed admission, held receiver samples, intermediate strengths and delayed scheduling; all-static topology does not justify replacing these with unordered final maxima.

## 2026-10-18 — construction-time immutable geometry and impossible relation pruning

Required electrical preparation now folds `Live` site geometry while inserting guards, rather than retaining millions of fixed predicates until O1. Constant sites use the actual Profile flags (including deliberately noncanonical flags); fixed component sites use their fixed block class. Solidity, transparency, wire class and profile identity are immutable here; powered/locked/output strength are never folded. Contacts and every `Captured` predicate remain general, including the capture-presence requirement. `Always` and `Not(Always)` are canonical constants. Boolean identities propagate during insertion. Guard child-slice storage is counted from actual retained child lengths rather than an assumed four children per guard capacity. The new builder is preparation-local and does not change the graph representation or optional pass contract.

The unchanged CHUNGUS fixture now completes required preparation under the unchanged 1 GiB table bound. Electrical guards fall from **8,819,119 to 2** (capacity 4, 96 retained bytes); recorded electrical tables fall from **587,244,824 to 169,625,960 bytes**. Complete callbacks record **543,590,792 bytes**, 369,713 routes, 9,830,337 delivery rows, and 3,482,700 callback links. O2's existing pruning retains 3,482,700 delivery rows (64.6% fewer than raw preparation). All 133,623 wires receive fully known 24-slot geometry, compiled seed/Turbo strength transfers with zero conditional geometry, and bound hop roots. This is complete fixed-topology query coverage, not whole-net collapse: the ordinary ordered Turbo walk, held samples, intermediate writes, live conducted power and delayed handlers remain.

The first post-fold CHUNGUS differential passes **65 boundaries through 64 ticks**, including all 251,485 actor states, 21,944 comparator entity strengths, exact scheduled TickEntry order and logical clock. The original start button is pressed once, without prewarm. Six lamp-change frames occur; actual Graph work is 1,115 scheduled ticks, 94,786 callbacks and 625 wire walks. Debug timings were contaminated by concurrent regression execution and are not performance evidence. Artifacts are `target/chungus-guard-fold-{census,differential-64}.json` and corresponding logs/frozen executable.

A second defect was overconservative classic discovery: even provably false wire relations joined components. The existing guard simplification rewrite now removes only relations classified `Never`, after guard remapping, and shrinks their allocation. Unknown/contact/captured relations remain. This changes discovery and storage, not runtime wire adjacency or callback order. Its focused regression retains a captured wire-class condition and verifies idempotence after removing literal and compound false edges.

Validation to this point: the construction guard test passes; the Graph namespace passes **157 tests, 6 ignored**; classic-filter regressions pass **16 tests, 3 ignored**; guard simplification passes **8 tests**. Analysis passes 101 tests with one expected checked-in JSON snapshot mismatch. The five ADDER_1BIT exports were regenerated and the snapshot check passes. Reviewing old/new JSON confirmed nodes, sites, profiles, pistons, resources, and every ordered route/callback occurrence remain unchanged after ignoring guard-ID remapping. O0 guards change 2,986 -> 997; O1 guards 680 -> 516; O2–O4 guards 452 -> 387. O1–O4 wire relations change 384 -> 73; O0 retains all 384. Power-link counts and ordered callbacks are unchanged. Baseline copies are saved in `target/adder-export-before-guard-fold`; these changes do not replace or normalize schematic inputs.

Next experiments compare normal fixed Graph, certified classic collapse, and a deliberately unchecked all-fixed weighted collapse on the same CHUNGUS oracle. Fixed topology establishes that geometry is compilable; it does not prove final-only maxima can replace intermediate receiver samples or ordered events. A further compile-time bottleneck is being corrected: ClassicAnalysis currently scans all routes, nodes and electrical targets per candidate. Reverse reader and occurrence indexing must preserve the same eligibility checks and proof bounds before a full CHUNGUS classic census is meaningful.

### CHUNGUS preparation repaired without raising limits; first bounded execution

The parent added construction-time folding of immutable **live** geometry guards in electrical preparation. Contacts and captured-presence semantics remain dynamic; no signal power/state was folded. The post-fold frozen debug executable is `target/chungus-guard-fold-tests.exe`, SHA-256 `a4963955b2b2863702dfe2f38d9da64c6be06f254dc208bf50dc021d0e135e06`. Full preparation now succeeds in 4.1588852 s, with two guards (96 bytes retained capacity), electrical tables 169,625,960 bytes and completed callback tables 543,590,792 bytes under the unchanged 1 GiB bound. Prepared callback routes contain 9,830,337 delivery rows; O2 removes 6,347,637 inert rows, retaining 3,482,700.

The first O2 64-tick run matched the interpreter at all 65 boundaries for all 251,485 actor blocks, 21,944 comparator entity outputs, ordered scheduled ticks/FIFO and logical clock. It observed six lamp frames over 5,602 lamps, trace SHA-256 `846ddabe644f08b387787347688d0d9eccd80a963b2ad4a28759a32751a10e09`. All **133,623 wires** already have fully known 24-slot geometry, compiled seed/Turbo transfers with zero runtime-conditional geometry, and direct hop roots; 353 are isolated. Remaining tables include 714,492 power links, 1,032,940 sites and 3,482,700 delivery rows. This demonstrates full fixed-topology query/hop specialization, not elimination of the native propagation state machine. Actual work is 1,115 scheduled ticks, 94,786 callbacks and 625 wire walks. Debug timing was concurrent with correctness regressions and is not a performance claim. Evidence: [post-fold census](../../../target/chungus-guard-fold-census.json), [64-tick baseline](../../../target/chungus-guard-fold-differential-64.json), and corresponding `.log` files.

The parent then removed only provably false wire relations in the existing guard simplification pass. The retained relation count falls from 1,603,476 to **179,592**, exactly the independent load-only dust-dependency count; O2 retains only one Always guard. This changes coarse region discovery, not Turbo queue/capture semantics.

To make the certified pass's discovery scale to these 30,731 regions, `ClassicAnalysis` now builds guard-blind reverse input-reader and source/receiver/fallback-target route indexes once. Each candidate uses the union of touched targets/routes in the original sorted order, then performs the unchanged guard, query, ownership and boundary checks. Conducted dependencies retain the same depth-eight semantics, including guarded/false alternatives; observer/reset watches and existing dust/instant owners are also indexed. The independent validator reuses this input analysis and still replays the reference certificate. No proof bounds, eligibility or candidate order changed. A new positive/near-match regression adds an indirect conducted Side read without a callback edge: the original line remains eligible, while the near-match still declines with `private wire read through side or quasi port`. The focused classic suite passed 17 tests (3 ignored).

The indexed frozen debug executable is `target/chungus-indexed-classic-tests.exe`, SHA-256 `1947dd98b0269da822a638a05be75ca1bec30bfa94308ebd10f85744821a6b5c`. Its 64-tick opt-in certified run accepts **zero nets**; reference proof work is only 17,862, so proof budgets are not the limiting factor. Exact decline counts across all 30,731 regions are:

| Existing first-family decline | Regions |
| --- | ---: |
| Callback-only or external private wire | 30,032 |
| Receiver has independent main input | 273 |
| Effectful synchronous boundary callback | 115 |
| Not exactly one variable ingress | 107 |
| Region wire bound | 65 |
| Unsupported ingress component | 64 |
| Private wire read through Side/Quasi | 37 |
| Stale saved wire geometry | 17 |
| Ingress route prefix may emit/reenter | 13 |
| Ingress repeater may lock silently | 4 |
| Saved repeater locked | 3 |
| Boundary operation bound | 1 |

These are limitations of the existing closed, one-ingress first family, not reasons CHUNGUS's fixed circuitry cannot be synthesized. Normal and opt-in classic still match all 65 interpreter boundaries, with identical work because no net was accepted. Evidence: [classic census/differential](../../../target/chungus-indexed-classic-64.json) and [log](../../../target/chungus-indexed-classic-64.log).

The deliberately unchecked weighted mode (`MCHPRS_CHUNGUS_UNCHECKED_WEIGHTED=1`, mutually exclusive with `MCHPRS_CHUNGUS_CLASSIC`) covers **all 133,623 wires and 30,731 networks**, with 20,062 single-source, 10,554 multi-source and 115 zero-source networks, largest 113 wires, 314,227 weighted source terms and zero contact samples. It uses the prior v4 seed-admitted/root-closure algorithm, not certified classic IR. In its first 64 ticks actor blocks and comparator outputs match, but FIFO differs at **38 of 65 observations, first tick 16**. The repeater bank at `(106, y=62..72, 127)` is scheduled in the reverse order. Thus this counterexample requires no piston or moving boundary. It removes native walks and reduces callbacks from 94,786 to 23,440, but this early debug reduction does not establish an equivalent implementation or a release speedup. Evidence: [unchecked differential](../../../target/chungus-indexed-weighted-64.json) and [log](../../../target/chungus-indexed-weighted-64.log). Longer release checks will separately report per-mode lamp traces, exact first FIFO ordinal, reconstructed internal component state, pending membership and clock, retaining mismatches without aborting the unsafe run.

## 2026-10-18 — compiling fixed raw wire sides (validation in progress)

CHUNGUS's query masks already compile all dust-strength topology, but `raw_wire_sides` still resolved 24 neighboring block views for every wire shape/directional-output query. The existing O2 `specialize-wire-queries` builder now additionally infers raw N/S/E/W sides when all 24 source classes are fixed. It runs the same native raw-side function once and stores four two-bit sides in the immutable query facts. Contacts still decline this cache, even if their possible occupants happen to share solidity/transparency: connection class and diode facing are also required. Captured predicates and the Turbo algorithm are unchanged.

Runtime reconstructs those four sides and injects **the supplied wire argument's power**, which can be an older capture, not necessarily newest live state. Current dot/cross regulation and `on_neighbor_changed_from` still execute against that original supplied wire. This is a pure local geometry substitution; it does not collapse wire propagation, delete receiver operations, deduplicate callbacks, reorder queues, or publish source output early. Draft validation independently recomputes complete facts, and JSON/viewer validators check the optional byte's four ternary fields and fixed-neighbor precondition. Older exports may omit it.

New focused tests exercise all 81 supplied side combinations and 16 power levels with changing live source strengths; an isolated wire verifies dot/cross regulation remains dependent on the supplied wire; a valid-but-wrong encoded side is rejected as a stale certificate; dynamic contacts retain fallback. DTO syntax checks cover all 256 byte encodings. The first compile exposed a Rust numeric-inference ambiguity in the decode closure; the shared file was corrected to explicit `u32` shifts/`u8` mask before retry. The viewer suite passes **23/23** after updating snapshot classifications: folding fixed predicates changes 20 formerly unknown links to enabled and six to disabled, leaving four unknown links. Runtime correctness/performance results will be appended after the focused build and frozen release comparison finish.

### 2026-10-18 — CHUNGUS before raw-side caching: 4,096-tick release oracle

The frozen before-shape release executable is `target/chungus-before-raw-shape-release-tests.exe`, SHA-256 `a0a57f4e03a7ccc1e4047f47488dfffa0411a11c72477069cd4ac2e5ffa45023`. The original startup was run without prewarm for 4,096 ticks (4,097 checked boundaries), approximately 0.033% of the historical 12,411,975-tick Mandelbrot benchmark. There is no frozen completed-output reference; the independent interpreter is the oracle. Evidence: [weighted comparison](../../../target/chungus-before-raw-shape-release-weighted-4096.json), its `.log`, and [64-tick certified comparison](../../../target/chungus-before-raw-shape-release-classic-64.json), its `.log`.

Normal O2 Graph matches all actor states, comparator entities, exact FIFO/clock and lamps throughout. The unchecked all-wire weighted collapse matches the lamp trace at every boundary (639 frames, SHA-256 `36cca86c9766563344b3aeb64ec1daa2568959a01a2be96ce312e79a02bc0bc8`), but **does not preserve execution semantics**: actor and comparator outputs each differ at 289 observations, ordered future differs at 3,501 observations, and candidate-vs-normal reconstructed component/pending/clock mismatch counts are respectively 289/727/0. First FIFO difference is tick 16, ordinal 1 of 13: the normal High-priority repeater tick (two ticks remaining) is at `(106,62,127)` versus weighted `(106,72,127)`. First pending-membership difference is tick 262. The earlier report retained only the first overall mismatch, so separate first actor/analog witnesses are added to the next frozen scaffold. Lamp agreement over this bounded window does not establish CPU equivalence or a completed Mandelbrot result.

Measured execution is interpreter 9.3908165 s, normal Graph 8.3859093 s, and unchecked weighted 5.4070569 s (1.5509x normal/weighted, 35.52% less execution time, **incorrect state/future**). Graph compilation is 20.0001199 s; weighted attachment is 0.3351476 s. Normal performs 380,926 scheduled ticks, 20,076,058 callbacks and 210,566 walks; weighted performs 380,406 scheduled ticks, 6,142,125 callbacks and no native walks. Weighted execution records 475,074 net invocations, 1,129,712 final writes, 4,875,595 boundary callbacks, 1,960,640 source samples and 15,491,639 weighted terms; no writes occur outside the admitted root closure.

The full run takes 199.39 s because the oracle harness materializes **every actor via `project_future` after every tick**, then compares states and queues outside the execution timer. Full-oracle projection alone is 54.6352442 s for normal and 54.6722620 s for weighted; this is not production dirty projection/render cost. Tick-interleaved execution is cache-perturbed by these checks, and release unit-test power-query instrumentation remains enabled. These are exploratory execution timings, not production TPS. The 64-tick certified release comparison matches every check but still accepts zero nets, as the unchanged first-family census predicts.

### Raw-side validation before final release timing

The focused `wire_query` filter passes **18 tests**, including the new captured-power/raw-side cases and all existing dynamic-contact and noncanonical-profile tests. The frozen debug executable `target/chungus-raw-shape-debug-tests.exe` passes the complete redpiler namespace: **375 passed, 15 ignored**, 68.42 s. This covers ordinary redstone, analog comparators, piston/BUD boundaries, capture semantics, reset/interpreter continuation, pass invalidation, classic certificates, and JSON snapshots. The exact five named adder export fixtures were regenerated for the new optional raw-side facts. `cargo check --workspace --all-targets --locked` passes in 23.50 s; warnings come from concurrent instant-cell work. Viewer tests pass **23/23**. Logs: `target/chungus-raw-shape-{focused-debug,redpiler-regressions,workspace-check,viewer-tests}.log`.

A read-only review by the classic-redstone agent also checked fixed class/facing versus mutable power, contacts, noncanonical Profile flags, packed N/S/E/W order, supplied captured power, independent certificate re-inference and viewer validation. It found no defect. The final release build uses the repository's unchanged optimized release profile with fat LTO; no compiler optimization flag or memory/proof bound was relaxed. The preceding weighted-collapse experiment remains test-only and unqualified for production because its hidden analog state and future work disagree despite matching every sampled lamp.

### 2026-10-18 — cached raw sides: full fixed-wire coverage and bounded equivalence

The after-shape release executable is `target/chungus-after-raw-shape-release-tests.exe`, SHA-256 `089ea31fd3d1f5711c1de4738f8a73041c8df4bfa007771e792480c5bb0b53b0` (fat-LTO build 4m32). The identical original startup and 4,096-tick protocol passed: normal O2 Graph matches all 4,097 interpreter boundaries for actor states, comparator entity strengths, exact FIFO/clock, pending membership, reconstructed internal state and lamps. All **133,623/133,623 wires** now have compiled raw-side encodings, in addition to fully known geometry, compiled transfers without runtime geometry and hop roots. There are still 369,713 routes and 3,482,700 ordered delivery rows; this optimization removes repeated block-view reconstruction for raw shape queries, not the ordered propagation state machine. Read-only review confirmed fixed class/facing proofs, exclusion of contacts, supplied captured-wire power preservation, unchanged dot/cross regulation, valid two-bit encodings and certificate re-inference.

Evidence: [after-shape comparison](../../../target/chungus-after-raw-shape-release-weighted-4096.json), [log](../../../target/chungus-after-raw-shape-release-weighted-4096.log). Normal work is exactly unchanged from before: 380,926 scheduled ticks, 20,076,058 callbacks and 210,566 walks. The same 639 lamp frames have trace SHA-256 `36cca86c9766563344b3aeb64ec1daa2568959a01a2be96ce312e79a02bc0bc8`.

| Execution timer | Before raw-side cache | After raw-side cache |
| --- | ---: | ---: |
| Interpreter control | 9.3908165 s | 11.0265831 s |
| Normal O2 Graph | 8.3859093 s | 8.6492060 s |
| Unchecked weighted, incorrect | 5.4070569 s | 5.4047909 s |

A concurrent user/background Cargo/rustc process was observed during the after run. Interpreter execution increased 17.42%, full-oracle projection increased from 54.6352442 to 64.0626537 s for normal Graph, and complete run wall time increased from 199.39 to 235.98 s. Normal Graph execution increased 3.14%; normalizing by the interpreter control suggests a 12.16% reduction, but this single contended run does **not** establish an isolated speedup. No automatic repeats were run. Compilation was 24.5259633 s and weighted attachment 0.3494021 s; these are similarly affected. The unit-test instrumentation, interleaved oracle checks, bounded horizon and full-projection caveats above still apply.

Unchecked weighted behavior is unchanged: 289 actor-state, 289 comparator-entity and 3,501 ordered-future mismatch observations; pending membership differs at 727 boundaries, while clocks and lamps match. Separate witnesses now locate the first actor mismatch at tick **512**, wire `(99,79,212)`, normal power 13 versus weighted 9; the first comparator mismatch is the same tick at `(100,80,212)`, output 13 versus 9. The first missing pending item is tick **262**, at `(82,69,120)`, registry type 441 (normal 142 pending items, weighted 141). First FIFO difference remains tick 16 at ordinal 1, the reversed repeater bank described above. Weighted work is exactly unchanged, including zero writes outside the directed root closure. These fixed-topology counterexamples reject unordered final-state collapse as an equivalent implementation; they do not establish any impossibility of ordered graph synthesis. The certified first family still accepts zero CHUNGUS nets, so no broader collapse is enabled or claimed.

Parent validation on the frozen after-shape implementation: release wire-query tests **18 passed**; debug redpiler **375 passed, 15 ignored**; workspace check passed; viewer **23 passed**. Reproduction (PowerShell): remove `Env:MCHPRS_CHUNGUS_CLASSIC` if present, set `$env:MCHPRS_CHUNGUS_TICKS='4096'` and `$env:MCHPRS_CHUNGUS_UNCHECKED_WEIGHTED='1'`, then run `target/chungus-after-raw-shape-release-tests.exe chungus_bounded_interpreter_graph_differential --ignored --nocapture`. The generic report is `target/chungus-differential-4096-unchecked-weighted.json`; the named preserved copy above is the evidence for this run.

The single requested repeat is preserved as `target/chungus-after-raw-shape-release-clean-weighted-4096.{json,log}`. Despite the filename, it is **not a clean performance sample**: no Cargo/rustc was present at launch, but a later process check found Cargo PID 25500 and rustc PID 42792 (335 CPU seconds), and Cargo PID 49636 remained at completion. No processes were stopped and no further repeats were started. It passed the same correctness checks and retained exactly the same normal work and weighted mismatch counts. Wall time was 240.86 s; execution was interpreter 11.2432607 s, normal Graph 8.9450112 s, and incorrect weighted 5.7132561 s; full-oracle projection was 66.1265856/65.8796075 s respectively. Both after-shape samples are contention-qualified; they establish fixed-query coverage and bounded equivalence of normal Graph, but cannot establish an isolated raw-side-cache speedup. Performance experiments stop here while the compiler's closed first-family boundary limitations are investigated further.
