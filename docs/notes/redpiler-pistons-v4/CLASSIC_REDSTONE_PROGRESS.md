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
