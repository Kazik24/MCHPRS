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

### 2026-10-18 — callback ownership census and guarded-cut implementation

The callback-closure probe (`target/chungus-classic-boundary-closure-census.json`, corresponding `.log`, frozen `target/chungus-classic-closure-census-tests.exe`) merges every canonical neighboring Component wire together with electrical regions, without omitting any callback. CHUNGUS's 30,731 electrical regions become 1,342 conservative closures; the largest has 53,689 wires. There are 514 single-ingress closures, and the unchanged structural inference accepts one 15-wire merged closure (source node 67,192). Other closures retain the original proof bounds and decline checks: 142 exceed the wire bound, 661 are not one-ingress, 273 have an independently driven receiver, and 115 have effectful synchronous callbacks. Merging alone therefore provides a constructive positive candidate but cannot supply broad local synthesis coverage. The one-tick oracle probe passed in 50.98 s (debug, no performance claim).

The builder now tries original electrical regions first, then bounded callback closures not identical to an original region, deduplicating private wire owners. The unchanged transfer inference and all 256 ordered-reference old/new checks remain mandatory. A further guarded-cut implementation is being validated: callback-only neighboring wires must have fixed, inert shape; identical seed/Turbo transfer masks; and electrical queries independent of both private writes and this net's changing ingress. Before a compact entry has any effect, each neighbor's live seed must equal its current power. The reference still visits these neighbors and retains their effects on visited flags, heading, captured-site discovery and queue order; it checks saved stability and no writes across each reference episode. Stability does not mean skipping the reference visit.

Guard failure restores private scalar powers from the **old committed signal**, disables that net permanently in `CircuitState.classic_active`, and runs the original route occurrence in the same frame. A general `set_wire` into an active private owner has the same materialization barrier before its actual write, allowing an outside native walk to retain its original captured state and queue. Wire reads, direct receiver queries, inspection and projection use virtual signals only while the net remains active. Guard sets and the failure policy are exported; re-inference validates the exact supplied guard set independently. This is an implementation under test, not a claim of CHUNGUS acceptance or speedup yet. Default enablement remains unchanged.
### 2026-10-18 — separate weighted electrical inference from callback elimination

The historical 7470205 `input_search` walks backward from each receiver through dust, accumulates attenuation below fifteen, and records distinct main/side source links; optimized identification omits intermediate unobserved wire nodes. Its DirectBackend uses packed forward links and strength counts. Those electrical ideas remain useful, but its callback ordering is not a correctness oracle. The preceding normal-Graph timings still executed Turbo and callbacks: they did not measure a fully collapsed weighted execution graph. Certified first-family coverage was zero, while the deliberately unchecked all-net experiment was faster and demonstrably wrong in analog state and FIFO order.

Added O2 `compile-fixed-signals` as a PlanBuilder after the existing builders. It compiles fixed directional/conductor queries into dense target-indexed maximum-of-weighted-live-state reads, and independently expands dust operands into historical-tracer-style source-to-receiver equations. Main, side and quasi channels remain distinct; paths attenuate by accumulated loss, duplicate source paths retain the minimum loss, constants fold, and losses >=15 disappear. Selectable contacts, unknown guards and unproved geometry retain general execution. Horizontal isolated dust keeps an explicit dot/cross gate. The executable expressions retain CURRENT delivered wire state wherever replacing it with a source would publish undelivered changes. Settled source equations are proof/inspection provenance, not permission to remove an ordered episode.

Plans invalidate with all other derived data, validation re-infers them, and lowering flattens only executable live operands into dense arrays. JSON/viewer distinguish `live_state` from `settled_provenance`; both are inspectable, and their edges have distinct viewer filters. The new electrical query path does not remove the callback table or claim that every inferred equation has an executable callback-collapse certificate. Provenance work is bounded separately (32 million queue terms); exhaustion leaves exact live execution available. That bound was added after the first frozen test executable below.

The first three focused tests pass: two independent analog sources produce the expected main weights [0,1] and side weights [2,1]; all 4,096 combinations of source/source/old-wire strength agree with general queries, including deliberately unsettled dust; and a syntactically valid but wrong attenuation fails independent validation. Initial full regression: 381 pass, 15 ignored, three failures from the expected new JSON table, a test deliberately mutating a source's raw class without discarding its new certificate, and the old negative expectation for an independent callback-only wire now admitted by guarded cuts. Fixtures were regenerated (only O2/O3/O4 gain 981 lines apiece), the raw-class mutation now discards its certificate, and the classic agent updates that negative to assert the new guarded admission. Final rerun and full CHUNGUS coverage are pending.

Frozen debug executable for the first CHUNGUS guarded/weighted-inference probe: `target/chungus-fixed-signals-guarded-debug-tests.exe`, SHA-256 `1e3420d4647b83b68477163c2f78e20da70eb600052c732765badae517a288a5`. Protocol remains the original saved full plot/start button, 1,024 ticks, full actor/comparator/FIFO/pending/reconstructed-state oracle at each tick, certified candidate alongside normal O2 and interpreter. User/background Cargo/rustc appeared during this probe, so its timing is not an isolated performance measurement. No C/LLVM engine was restored; concurrent instant-compiler changes are preserved.
### 2026-10-18 — first full CHUNGUS fixed-signal inference result

The frozen 1,024-tick debug probe passed all 1,025 boundaries: all 251,485 actor positions, 21,944 comparator entity strengths, 5,602 lamps, exact ordered future and logical clock, pending membership and reconstructed component state. Both normal O2 and opt-in classic match the interpreter and each other; 162 changed lamp frames share SHA-256 `1c5ee0aa2b51e6f17186b34b809111663d17b0c3d005e6a6c3d70cc33c4f2589`. Report: `target/chungus-differential-1024-classic.json`; preserved log: `target/chungus-fixed-signals-classic-1024.log`.

Fixed electrical inference covers **332,458 live queries / 451,862 live operands**, with **zero fallbacks**, and produces **124,663 dust-free receiver channels / 177,003 source-to-receiver weighted links**. This is actual whole-CPU backward electrical inference, including multiple source paths and separate channels. It does not leave every candidate hidden behind the classic callback proof.

However, certified runtime collapse still accepts **zero** CHUNGUS nets. Remaining per-candidate declines after guarded cuts are: 11,013 multiple ingress, 9,006 receiver with independent main input, 5,091 effectful synchronous boundary callback, 2,970 callback boundary electrically dependent on private ingress, 1,098 unsupported ingress, 754 silently locking ingress repeater, 527 side/quasi reader, 332 stale saved shape, 208 possibly reentrant ingress prefix, 158 non-inert guarded shape, 65 wire bound, 65 saved locked receiver, 9 nonzero locking input, and 1 noncompact ordered boundary program. Proof work is only 17,862: proof-budget exhaustion is not causing zero coverage. Counts include appended merged-closure candidates, so they are not a partition of the original 30,731 electrical regions.

Normal compilation took 36.4190 s and opt-in classic 59.3712 s. Both executed the same 128,365 scheduled ticks, 6,045,241 callbacks and 69,763 Turbo walks. Core timers were 2.7491423 s / 2.7348094 s, interpreter 5.0494119 s; full-oracle projections 21.0235094 s / 20.9816827 s; total wall time 172.66 s. These are contended debug measurements, not a speedup claim. The small difference between the two Graph timings cannot represent classic elimination because both have zero nets and identical work.

Next investigated boundary is an independently driven receiver: a zero-contribution entry guard could admit the compact episode while retaining the receiver's complete live query outside that episode. If another input becomes active, materialize the OLD committed private values and execute the original occurrence. Such receivers must never receive a global own-net-only input override. This is a proposed guarded extension requiring complete proof/runtime/reset/export/tests, not an established result.
### 2026-10-18 — focused integration checks after guarded-cut hardening

The classic helper now reads typed initial wire sides rather than stale raw block sides, and constant-site Solid/Transparent predicates use Profile flags. Focused witnesses cover those cases and guarded boundary strength changing from saved zero to stable fifteen before a later fresh-input deoptimization. These harden structural admission rather than assuming the saved raw representation is authoritative.

Viewer validation passes **24/24**, including explicit live-versus-settled edge types and malformed weighted operand/semantics rejection (`target/classic-fixed-signals-viewer.log`). Latest complete redpiler run builds and reports **383 passed, 6 failed, 15 ignored** (`target/classic-fixed-signals-final-regression.log`). Every classic, fixed-signal, snapshot and builtin-reset test passes. The six failures are the concurrent stationary instant-cell tests at `tests/instant_cells.rs:165`, whose shared preparation currently declines `MissingResetWake` for adder owners. An immediately preceding compile attempt also caught the in-progress CellUse/CellRetractionEffect interface migration; its owner corrected that source before the retry. These unrelated changes were preserved, not reverted or patched to force this experiment green.

Added a separate ignored paired query benchmark that compiles CHUNGUS once, clones the identical lowered program/state, and disables only the new live-query table in one test engine. It alternates tick order and checks complete CircuitState, FIFO, pending map and clock at every boundary outside the timing window. This isolates general-query evaluation versus compiled live operands, without pretending either version has eliminated dust walks. It is not a replacement for the independent interpreter comparison above, and unit-test query instrumentation remains enabled. The benchmark has not yet been executed in this snapshot.
### 2026-10-18 — guarded independent receiver inputs: first certified CHUNGUS nets

Extended the delayed Torch/unlocked-Repeater receiver family to permit independent Main terms guarded to contribute zero before a compact entry. Such a receiver NEVER gets an own-net-only global input override; its original complete live Main query remains active between and during scheduled events. Outside operands are explicit source/attenuation terms, nonzero constant contribution declines, all structural external wire dependencies (including initially omitted directional paths) need immutable non-dot geometry, and entry failure deoptimizes before committing or scheduling. Independent validation reconstructs the exact outside term set. JSON/viewer distinguish these guarded inputs and validate their term references/weights/guarded flags. Runtime terms and proof work retain existing bounds.

The positive fixture exercises another input turning on while the first net is virtual, a falling first-source entry that must materialize old state before fallback, exact FIFO/actor state against interpreter, missing-guard forgery rejection and reset/native continuation. The first focused run passes 23 tests but one negative fixture was incorrectly built: the supposedly isolated near-Torch wire actually connects through the Torch/support geometry. Its replacement explicitly asserts a remote wire has native raw-dot shape, then injects a structural Main dependency with no callback edge; this preserves the omitted-direction counterexample. That test-only correction awaits rebuilding. The four fixed-signal tests pass and viewer remains 24/24.

Frozen source before the test correction: `target/chungus-classic-outside-main-debug-tests.exe`, SHA-256 `72259992abd4163c4fd599c44c76d79db44af09bf91dd7228c26bac0a29a801f`. A full-plot 64-tick independent differential passes all 65 boundaries (all actors, analog entities, FIFO/clock, pending, reconstructed state, lamps). Report `target/chungus-differential-64-classic.json`, log `target/chungus-classic-outside-main-64.log`. It certifies **257 nets / 2,488 wires (1.862% of CHUNGUS dust)**, 1,262 receiver bindings including 710 guarded inputs, 718 outside terms, 3,634 guarded boundary-wire occurrences, 637 entries and 2,982 compact operations. After startup, 243 nets remain active and 14 have safely deoptimized. Only two kernel invocations/seven receiver operations occur during this short prefix: native walks fall 625 -> 623, callbacks 94,786 -> 94,666, and scheduled ticks remain 1,115. These counts establish constructive coverage, not a significant runtime gain.

Classic compilation takes 97.56607 s versus normal 30.1918071 s; core timers 0.0262789/0.0271835 s are too short and contended to support a performance claim. Total wall time is 131.14 s. Proof work now reaches **32,000,179**, and 5,700 further structurally admitted candidates decline the unchanged total work budget. This is a different limiter than the earlier zero-coverage run's 17,862 work. Potential improvement under review is to prove fixed external-wire stability once from structural independence, keep every actual native reference visit and all 256 old/new replays, then verify unchanged guarded state per replay instead of repeatedly re-deriving immutable geometry. No bound increase or relaxed callback boundary is authorized by that observation.

Live-query certificate payload is 26,002,440 bytes and projected runtime tables 22,875,252 bytes on CHUNGUS. The 177,003 settled source links remain provenance while private runtime execution uses only certified kernels. The old callback table is retained for exact fallback; neither the 1.862% executed wire coverage nor the weighted topology report should be described as full callback elimination.
### 2026-10-18 — paired live-query execution at 4,096 ticks

`chungus_fixed_queries_paired_execution` passes all **4,097** complete state/FIFO/pending/clock checks. Only the compiled live-query table differs between engines; source, program graph, initial state, original start-button activation and all other optimizations are identical. Both execute exactly 380,926 scheduled ticks, 20,076,058 callbacks and 210,566 Turbo walks. The table changes electrical read evaluation, not event semantics.

Debug execution timers: **general 13.8076567 s, compiled 11.6074012 s**, ratio **1.189556x** (15.94% less core time). Activation is 0.0008714 / 0.0006844 s, common compile 31.8943607 s, complete test wall 72.51 s. Evidence: `target/chungus-fixed-query-paired-4096.{json,log}`, frozen outside-Main executable identified above. It still contains a redundant per-operand proof-mode branch, subsequently removed from source because cold references clear all derived plans and set actual hypothetical source values. Unit-test link-visit instrumentation remains enabled, and background rustc (PID 40868) was observed during the run. Alternating paired execution controls some contention, but this is one contended debug sample, not an isolated production or release speedup. An optimized build check is planned; no performance claim is made for the short 257-net classic prefix.

### 2026-10-18 — proof reuse preserves native boundary visits and work bounds

The proof implementation now derives fixed external-wire geometry and actual seed stability once before the 256-pair matrix. Structural independence excludes private writes and the changing ingress; seed and Turbo transfer masks must agree. An unchanged guarded visit cannot propagate farther work, although its native discovery, visited flags and heading/bias effects still execute in every replay. Each replay checks **no guarded dirty write at all**, unchanged typed guard state against an O(1) baseline lookup, and zero independent Main contribution. This is stronger than only checking a matching final value. All 256 old/new strengths and exact ordered receiver samples remain checked.

Work accounting charges the initial full guard check once, then the actual per-guard state checks per replay, retaining the original 2,000,000 per-net and 32,000,000 total bounds. It also removes the previous linear search through saved state for each guarded node. Runtime entry guards and permanent general fallback are unchanged; this optimization affects cold certification cost only.

A single baseline copy also protects failed proofs: cleanup restores every dirty component, including unexpected writes beyond proposed ownership/guards, then restores the manually modified source/private snapshot. The existing O(nodes) dirty reset performs this cleanup without a new runtime write hook. A focused negative fixture deliberately removes a declared source dependency while keeping native geometry: the rejected episode reaches a farther unowned wire driven by a constant source. The regression checks the complete reference component state returns to its initial baseline. This new proof-cost/cleanup revision awaits the next focused build and CHUNGUS census; the preceding 257-net results are preserved unchanged.

### Next exact static compilation step: dense Turbo admission, not a settled substitute

A concrete next experiment can specialize the remaining Turbo callback selection on every fixed wire. Its input is the existing canonical 24-site WirePlan, the original ordered Turbo delivery candidates, and fixed source-class/property proofs. Its output can be a dense per-slot `NoCall` / `Admitted` / `GeneralRange` selection, with the original site identity and proven fixed observed receiver retained. The current native loop uses the selected row to decide admission, then dispatches the **observed site's receiver**; a replacement must preserve that distinction rather than silently substituting a candidate row's receiver. Measure row/byte coverage before choosing whether a bitset plus existing fixed-site metadata is sufficient or a typed receiver table is worthwhile.

Legality must evaluate each original candidate in order under the exact discovery context. A `Captured` Site predicate is present only for its matching target site; fixed class, profile, solidity and transparency can be resolved there, while captured powered/locked fields must still be sampled at discovery. A missing/mismatching capture stays false. Boolean identities can expose a permanently first admitted candidate or prove no admission; a genuinely dynamic `Guard::State`, selectable Contact, uncertain profile/class or any unresolved earlier candidate retains the original ordered candidate range. Proving a later candidate true does not permit skipping an earlier unknown candidate. Repeated canonical site aliases retain their shared capture/visited identity. Static-impossible candidates should be reported separately from dynamic fallbacks.

Runtime retains the original root seed admission and publication point, `TURBO_ORDER` heading selection, layer queues, queue membership/duplicate behavior, neighbor discovery order, visited/bias effects, intermediate wire writes, capture timing and all native boundary handlers. It samples no source early and computes no final-state replacement for held intermediate input. A fixed receiver address only removes class/guard/row lookup; powered, locked, pending and electrical inputs remain live or captured at their original occurrences. The same-frame general fallback and guarded classic deoptimization remain available.

Only lowering may omit Turbo delivery rows whose **every executable consumer** has a validated dense selection. Shape routes, ordinary component output routes, instant/proxy consumers and any unresolved slot still need their exact semantics. Audit route consumers explicitly before dropping storage; do not assume a route named Turbo is used only by the wire walk. Keep the original graph rows and source ordinals as export/proof provenance, marked as non-executable after lowering, and rebuild lowered indexes against their actual retained representation. Independent validation should re-infer each dense selection from the original rows and fixed-site facts. A paired full-state/FIFO oracle and an independent interpreter run must precede performance claims. This is a proposed next scope, **not implemented or measured in this checkpoint**.

### 2026-10-18 — dense fixed Turbo admission implementation, validation pending

Implemented `compile-wire-callbacks` at O2 as a narrow execution plan: each wire has two 24-bit masks, known slots and admitted slots. In original candidate order, fixed false guards are skipped, a fixed true guard admits, and the first unresolved guard preserves the original selection path. Captured-site identity, constant profile flags, fixed component class and Boolean identities are resolved; state predicates and selectable contacts remain dynamic. Independent graph validation re-infers the masks. JSON/viewer expose and check the masks. The native walk still discovers/captures the same sites, follows the same heading/layer queues, writes the same intermediate dust state and invokes the observed receiver with the original captured values.

All-fixed lowering can omit the unused grouped Turbo callback index; original route rows remain for provenance and general consumers. The paired test control rebuilds the original grouped index while removing only the masks. No route-row deletion, event collapse, new source sampling or scheduler replacement is included. Focused tests cover mutable receiver output/lock state, forged masks, dynamic state guards, mismatching captured-site identity, contacts and restoration of the fair general-index control.

The first two integrated compile attempts were blocked by concurrent instant-boundary refactoring errors, with no diagnostic in this callback implementation. Logs are `target/classic-dense-callback-integration.log` and `target/classic-dense-callback-integration-retry.log`. Consequently neither test success nor CHUNGUS callback coverage/performance is claimed yet. The earlier scope paragraph described the proposal at its checkpoint; this section records the implementation now awaiting integrated verification.

### 2026-10-18 — proof-reuse CHUNGUS result and queue-elimination objective

The frozen proof-reuse debug executable `target/chungus-classic-proof-reuse-debug-tests.exe` (SHA-256 `ccd95521a6f60ffb84c4b39872499fc0e7addb686abecd3e061d6b4711b00fed`) passes all 1,025 full interpreter boundaries over 1,024 ticks. Preserved report `target/chungus-classic-proof-reuse-1024.json`, log `target/chungus-classic-proof-reuse-1024.log`. It certifies 1,676 nets / 8,757 wires (6.553%), 4,268 guarded Main inputs, 4,306 outside terms and 14,767 guard occurrences. After execution 1,411 nets remain active and 265 safely deoptimize; 153 kernel invocations execute 450 receiver operations. Proof work is 26,496,949, below the unchanged 32-million bound, with no budget declines. Structural family restrictions now dominate.

Actual dynamic elimination is much smaller than static coverage: walks 69,763 -> 69,610 (0.219% fewer), callbacks 6,045,241 -> 6,036,919 (0.138% fewer), scheduled ticks unchanged at 128,365. Core times 2.0764713 / 2.0738366 seconds do NOT establish a speedup. Compilation is 42.2031 / 151.8006 seconds, full-oracle projection 15.4182 / 15.6616 seconds, wall 250.69 seconds. The next priority is broader execution replacement, rather than spending more work only increasing this narrow certificate family.

The user clarified that combinational classic wiring should normally need no propagation queue and that final machine state matters more than firing order. Treat queue ordering as a diagnostic unless changing it affects observable state or future behavior. This permits weighted direct evaluation and removal of redundant notifications; it does not permit the known tick-512 analog/state divergence. Delayed gates retain timing and pending state until their equivalent replacement is established. Distinguish dust propagation queues from delayed-event storage: eliminating the former does not automatically remove the latter.

A test-only tick-512 trace now records live ingress terms, the first focused wrong wire write and comparator pending/deadline state. Working hypothesis (not yet a confirmed trace result): recomputing an entire component from every source's newest output publishes values before their original route occurrence. Investigate committed ingress channels and sparse weighted dependencies rather than a Cartesian enumeration of all analog inputs. Full-tick state, analog values and future continuation remain correctness checks; a matching lamp image alone is insufficient.

Dense canonical pending lookup is also implemented as an additive mirror of the exact existing identity map, including duplicate counts/import/pop/reset handling. It removes hash lookup overhead without replacing event timing or claiming queue elimination. Focused tests compare it against the original map for canonical duplicates, stale/untyped identities, scheduling suppression and reset/continuation. Integration tests and timings remain pending. The latest shared build had one remaining unrelated instant-corridor borrow error; corrected the call to pass `&cell_owners`, with no semantic change, then retried compilation.

Dense-callback integration update: the shared frozen debug build succeeds; the classic filter passes 27 tests (4 ignored), including the nonvacuous failed-reference cleanup regression. Canonical pending tests (2) and fixed-signal tests (4) pass. Two callback tests pass; the contact test initially fails during preparation because its wire lacks support, before exercising callback selection. Added immovable Obsidian beneath that fixture wire; this test-only correction awaits the next shared build. No production callback failure has been observed.

### 2026-10-18 — callback/pending validation and first focused tick-512 trace

The shared build now succeeds. Frozen debug `target/classic-dense-callback-final-debug-tests.exe`, SHA-256 `4a77512ada4bb2eb26962e7831793dbfa7a63d745c323f0c1e8ded9cce4b215b`: classic filter 27 passed / 4 ignored; dense pending 2 passed; fixed electrical queries 4 passed; fixed callback selection 3 passed after adding a fixed Obsidian support beneath the near-piston contact fixture. That fixture correction changes no production semantics. Viewer 26/26 and Python loader 20/20 pass. JSON fixture regeneration adds only the expected O2/O3/O4 derived tables (1,109 lines each relative to HEAD, including the previous 981 fixed-query lines).

Full redpiler run reports 405 passed, 1 failed, 17 ignored (`target/classic-dense-callback-final-regression.log`). Its one failure is reset_elision.rs deliberately deleting route rows while keeping the newly derived callback-selection certificate; independent validation correctly rejects that stale certificate. The negative test now clears callback selections on BOTH controls alongside its existing callback-index clear. Its targeted rerun passes all cases with the original expected state/future/output divergences (`target/classic-dense-callback-reset-elision-fix.log`). This is a fixture invalidation fix, not weakened certificate checking. Latest debug trace binary SHA-256 `d54a18f46aafc27f87c7cf7f70d16ef8aaaff29a4421368f03cbe3a5cdf2523a4`.

The first focused CHUNGUS trace passes its reproduction assertions (`target/chungus-tick512-focused.log`). IMPORTANT correction to the working hypothesis above: the focused wire is a ONE-wire weighted component whose sole source is the comparator itself. Both engines finish tick 511 with comparator output/wire 7 and the same pending comparator deadline +1. At tick 512 normal becomes 13 with no pending work, while unchecked becomes 9 with an extra +2 comparator tick. No focused erroneous weighted write was logged: root seeding already agrees with its actual comparator source. The discrepancy therefore occurs at or before the comparator's scheduled sample, not from a wrong attenuation on this last wire. A second trace adds comparator main/side operands and contributing inputs at the actual handler occurrence. Newest-source publication remains a plausible mechanism elsewhere, not an established cause of this witness.

Optimized release build completes in 6m32s. Frozen `target/classic-dense-callback-release-tests.exe`, SHA-256 `f3e06b217ddcd9cee600946013a60b1c1cc5021bea261f1e6220163e8e2348b5`; release callback 3/3 and pending 2/2 pass. This executable contains production optimizations plus test instrumentation; the later reset-erasure fixture correction and added trace details do not alter the production execution path being benchmarked. Performance run remains pending at this append.

### 2026-10-18 — optimized paired execution and binary-family opportunity

Optimized combined query/callback/pending comparison PASSES all 4,097 complete state/FIFO/pending/clock boundaries over 4,096 CHUNGUS ticks. Same single lowered program, initial state and start-button press; only the test baseline disables the three new execution tables, restoring original grouped callback indexing and original pending-map lookup. Frozen release executable and SHA are recorded above. Preserved evidence `target/chungus-combined-release-paired-4096.{json,log}`. Core time is **13.8874445 -> 9.8406668 seconds**, ratio **1.411230x**, **29.139% less core time**. Common compile 46.0070054 seconds, total wall 89.48 seconds. A user/background cargo/rustc build (49148/51092) ran during the experiment; alternating order qualifies this as a paired contention-affected sample, not an isolated production claim. Unit-test query counters remain enabled. No new independent interpreter oracle was run in this paired test.

All 133,623 wires have fully compiled callback admission: 3,206,952 decided slots, 965,035 admitted slots, zero dynamic slots. Runtime grouped callback-index storage is **13,896,792 -> 0 bytes**. All 332,458 live electrical queries retain zero fallback. Both engines still execute exactly 380,926 scheduled ticks, 20,076,058 callbacks and 210,566 dust walks; this improvement reduces read/dispatch overhead rather than eliminating propagation. Kernel invocations are zero in this normal O2 comparison. Do not describe these counts as a fully synthesized classic graph.

Binary electrical candidate census passes in 8.79 seconds, without transition replay or CPU execution (`target/chungus-binary-classic-candidate-census.{json,log}`). Source cardinality region/wire counts: 0=115/638; 1=20,062/81,286; 2=6,320/15,668; 3=1,446/5,168; 4=1,279/7,346; >4=1,509/23,517. With supported binary source kinds, consistent saved state, no source locking/Side dependency, and existing wire bound, 1–4-source electrical opportunity is 19,359 regions / 87,567 wires (65.53%); the NEW 2–4-source extension alone is 5,179 / 19,385 (14.51%). These are opportunity counts, not legal callback boundaries, certified transition coverage, runtime active coverage or speed estimates. The report preserves precise overlapping decline counts.

Decision: before adding a separate multi-source wire-pattern IR, implement the smaller analog extension in the existing scalar net representation: Comparator as a single ingress, and delayed Comparator receivers with proved zero Side and no direct/far override. Existing 256 old/new strength cases already cover analog input. A receiver sample must carry computed output/powered, with live pending/current stored value at runtime; first sample plus first DISTINCT sample suffices to preserve its scheduling decision for every stored pair. Their chronological positions among other receivers still matter. Existing work/operation limits, outside-Main guards, independent validation, projection/reset and interpreter continuation remain mandatory. This extension is assigned but is NOT implemented or measured at this append.

### 2026-10-18 — tick-512 comparator sample and queue-free design boundary

The ignored bounded trace `unchecked_chungus_tick512_traces_live_sources_and_receiver_timing` passes in 51.64 s with no per-tick World projection. Its frozen debug executable is `target/classic-tick512-leaves-trace-debug-tests.exe`; complete output is `target/chungus-tick512-leaves-trace.log`. Reproduction:

```powershell
& .\target\classic-tick512-leaves-trace-debug-tests.exe redpiler::backend::graph::unchecked_classic_anpu::chungus_tick512_diagnostic::unchecked_chungus_tick512_traces_live_sources_and_receiver_timing --ignored --nocapture
```

The comparator at `(100,80,212)` has one flattened live Main operand: wire `NodeId(73250)` at `(101,81,212)`, attenuation 0. At the end of tick 511, both normal Graph and unchecked weighted execution read strength 9 from that wire; the comparator stores output 7 and has a Normal tick due in one tick. The Side input is zero. At the actual tick-512 comparator handler, Graph reads the wire at 13 and calculates output 13; weighted reads it at 9 and calculates 9. After the entire tick, the input wire is 13 in both. Graph has output 13 and no pending tick, while weighted retains output 9 and an additional Normal tick due in two ticks. The focused output wire `(99,79,212)` is a one-wire net whose sole source is this comparator; its final mismatch follows the different comparator sample.

The ordered future at the end of tick 511 shows same-deadline, Normal-priority entries at `(100,88,212)`, `(100,84,212)`, `(100,82,212)`, `(100,80,212)` in that order for Graph. Weighted execution orders them `(100,88,212)`, `(100,80,212)`, `(100,84,212)`, `(100,82,212)`. This proves that the focal y=80 sample occurs after the y=82 entry in Graph and before it in weighted, and that the order difference changes later state/future work. The trace does not establish which exact upstream route changes wire `NodeId(73250)` or whether the y=82 versus y=84 handler is its first effective driver. Root-first weighted net writes followed by root-first BFS/canonical-slot boundary delivery are a plausible source of the changed enqueue order, based on `UncheckedProgram::run`, but remain a mechanism inference rather than a directly traced edge.

This witness narrows the queue-free design boundary. A dedicated classic executor can remove per-wire propagation work while keeping the existing delayed-event scheduler and its due time, priority, pending deduplication and FIFO semantics. It should commit each source channel only at its original effective route occurrence, compute wire/input strength from committed channels and per-target attenuation, then execute the proof's ordered first-effective receiver operations at that boundary through existing Graph state/scheduling primitives. For multiple ingresses, retain one committed scalar per source and compute `wire_power(p) = max(fixed_power(p), max_i(signal_i.saturating_sub(attenuation_i(p))))`; update channels sequentially at their route occurrences and select operations from each resulting aggregate old/new pair. This avoids enumerating combinations of every analog input. Comparator admission still requires the existing proof for its computed output/powered sample and explicit treatment of Side/override reads; a delayed Comparator continues to sample at its scheduled handler. Keep all receiver operations in proven order relative to other receivers. This is a design constraint, not an implemented fix or a newly certified CHUNGUS region.

### 2026-10-18 — quieter optimized confirmation and isolated callback gain

Two subsequent paired release runs pass all 4,097 complete state/FIFO/pending/clock boundaries apiece with identical scheduled/callback/walk counts. No Cargo/rustc process was observed at launch or the periodic checks during these runs; no agent build or heavy diagnostic ran concurrently. They retain the same frozen release test executable and unit-test instrumentation limitation. Single-sample harness timings are not a production throughput guarantee.

Callback-only comparison (`target/chungus-callback-release-paired-4096.{json,log}`): **8.4093113 -> 7.4986264 seconds**, **1.121447x**, approximately **10.83% less core time**. Both engines keep compiled electrical queries and dense pending lookup; baseline restores the original grouped callback index. Common compilation 37.5087313 seconds, wall 64.52 seconds. This isolates the dense admission-mask benefit.

Combined quiet confirmation (`target/chungus-combined-release-confirm-4096.{json,log}`): **10.2630475 -> 7.4696876 seconds**, **1.373959x**, approximately **27.22% less core time**. Common compilation 36.8879093 seconds, wall 65.14 seconds. Baseline disables compiled electrical queries, callback masks and dense pending lookup together. This corroborates the earlier contention-affected 1.411x sample, without claiming that any dust queue was removed by these normal-O2 improvements.

Analog operation-budget clarification: the existing Boolean cap (four operations per receiver plus eight) cannot even represent sixteen constant analog result payloads for a one-wire Comparator receiver. The new representation uses a type-aware finite budget: four per Boolean receiver, thirty-two per Comparator receiver, plus eight. Thirty-two accounts for the first two distinct samples across the sixteen-strength alphabet. Wire/receiver bounds and the 256-case / 2-million-per-net / 32-million-total proof limits remain unchanged. If merged chronological operations still exceed that finite budget, the candidate declines. This is a representation bound adjustment for the new payload, not a relaxation of sampled-state or scheduling proof.

### 2026-10-18 — binary opportunity census and smaller analog Comparator extension

The ignored electrical census completes in 8.56 seconds without reference replay: 30,731 regions / 133,623 wires. Fixed binary sources in groups of 2–4 account for 5,179 regions / 19,385 wires (14.51%); the 1–4-source opportunity totals 19,359 regions / 87,567 wires (65.53%). These are electrical unions after guard reduction, not accepted execution kernels. Callback ownership, transition representability, source-prefix legality, guarded boundaries and proof work remain untested by this census. Report `target/chungus-binary-classic-candidate-census.json` includes cardinalities, source types and overlapping refusal reasons. A full binary wire-pattern IR was therefore deferred in favor of a smaller analog extension.

Implemented single Comparator ingress using the existing 0..15 committed signal and 256 old/new reference cases. Comparator scheduled ticks establish output before their original output route; powered/output consistency is validated without assuming binary strength. Existing prefix/reentry restrictions remain, with ordinary delayed Comparator callbacks admitted alongside Torch/unlocked Repeater. Comparator receivers initially require provably zero Side, no private Side/Quasi reads, and no direct/far override. Existing independent Main zero guards and permanent general deoptimization remain intact.

Receiver operations now carry a typed Boolean sample or the actual reference callback's computed Comparator `(strength, powered)` pair. For each transition, retaining the first pair and the first distinct pair at their original chronological positions preserves the first effective scheduling attempt for every stored pair and pending membership: if the first pair differs, it schedules; otherwise the first distinct pair does. Runtime compares against live receiver state and schedules one tick at the original High/Normal priority. It does not read final Main to recreate an intermediate sample. Wire projection, reset and committed-source state representation are unchanged.

The old Boolean operation limit cannot accommodate sixteen analog payload values. The authorized type-aware representation bound is now `8 + 4 * Boolean receivers + 32 * Comparator receivers` (two retained positions for each of sixteen strengths). All 256 reference cases, the 2,000,000 per-net / 32,000,000 total proof-work bounds and existing wire/receiver ownership limits remain unchanged. Projected table accounting uses the actual `ClassicOp` size, so richer payload storage is charged. Candidates exceeding the compact operation bound still decline.

JSON adds typed `sample`; Boolean exports retain legacy `nonzero`, while Comparator exports carry a null legacy field. Rust validation, Python loader and viewer accept/check the typed payload. New focused tests cover single-wire Comparator sources/receivers, all sixteen source strengths, pending-preserving pulses, native captured/live callbacks, forged analog values, zero-Side/override refusals, reset/interpreter continuation and interleaved first-effective FIFO minimization across stored pairs and pending states. This source revision awaits the shared build; no analog-extension coverage or speedup is claimed yet.

Before the shared build, zero-Side admission was hardened against an initially omitted directional dust input: all guard-blind structural Side wire dependencies now decline, for source Repeaters and delayed receivers alike. A regression injects a remote raw-dot Weak Side edge, confirms its saved electrical expression is empty, and requires refusal for both Repeater and Comparator receivers. A later dot-to-cross API change therefore cannot invalidate a supposedly permanent zero Side premise. This is deliberately more conservative than proving individual Side geometry paths inert.

### 2026-10-18 — analog extension validated; runtime coverage remains the bottleneck

The Comparator source/receiver extension passes **33 focused classic tests** (5 research ignored), viewer **26/26**, Python loader **20/20**, and full redpiler regression **417 passed / 0 failed / 18 ignored**. The final continuation fixture constructs both interpreter handoff worlds with identical original Comparator blocks; final focused rerun also passes 33/33. Logs `target/classic-comparator-{focused,final-focused,regression,viewer,python}.log`. Frozen differential executable `target/classic-comparator-debug-tests.exe`, SHA-256 `1ae5364d7df2e8caa76c2af562edbf4c5ab6f86391e578ae0937877269013a8d`; final test-fixture executable `target/classic-comparator-final-debug-tests.exe`, SHA-256 `f040570de9c134bfe6e2fb1ca036cf75789ad4841f32a29757207eb5d653a87a`. Production code is identical between these two snapshots; the last change corrects only reset test setup. Independent source review found no concrete blocker after checking virtual-wire queries, captured/live receiver properties, the Side geometry premise and first-two-distinct scheduling. All selected source diff whitespace checks pass.

The full saved-plot CHUNGUS differential passes all **1,025 boundaries over 1,024 ticks**, including the known unsafe tick-512 case: all 251,485 actor positions, 21,944 comparator entity strengths, lamps, exact FIFO/clock/pending and reconstructed private state match the interpreter. Normal and certified opt-in modes each have zero mismatch observations. Preserved report `target/chungus-classic-comparator-1024.json`, log `target/chungus-classic-comparator-1024.log` (generic differential report is copied before any future overwrite).

Static certified coverage is **1,795 nets / 8,994 wires (6.73%)**, 7,872 direct receiver inputs, 4,468 guarded Main inputs, 4,511 outside terms, 15,248 guarded-wire occurrences, 3,573 entries and 20,933 compact operations. After execution, 1,478 nets remain active and 317 have safely deoptimized. Proof work is **28,274,770**, below the unchanged 32-million bound; no proof-budget declines occur. Projected direct tables are about 13.31 MB, including dense node/entry indexes; reported certificate payload is about 1.27 MB. One byte each stores committed signal and active status per net.

IMPORTANT negative result: dynamic elimination is EXACTLY unchanged from the previous proof-reuse prefix: **153 kernel invocations / 450 receiver operations**, walks **69,763 -> 69,610**, callbacks **6,045,241 -> 6,036,919**, scheduled ticks **128,365** in both modes. Newly accepted analog nets are not exercised by this prefix. Do not infer execution savings from the static 237 additional wires. Debug core times **2.4468224 / 2.4318461 seconds** do not establish a meaningful incremental speedup. Compilation **51.3582798 / 186.1147089 seconds**, full-oracle projection **19.5820965 / 19.8547648 seconds**, wall **310.69 seconds**; root regression/build work overlapped the early preparation, and this is not an optimized paired timing experiment. The dedicated classic pass remains opt-in; the measured normal-O2 electrical/callback/pending improvement is separate.

Precise current structural declines (including appended closure candidates) show where broad synthesis is blocked: 10,933 not-one-ingress; 3,909 independent Main guard initially nonzero; 2,970 callback boundary depends on private ingress; 2,478 source repeater locking; 1,993 outside directional wire dot/cross; 1,603 receiver already assigned another net; 1,438 structural Side-wire dependency; 954 saved locked receiver; 790 remaining effectful synchronous callback; 584 nonzero/variable receiver Side; 583 private Side/Quasi read; 330 stale shape; 295 comparator overrides; 234 source-prefix reentry; 158 non-inert guard shape; 65 wire bound; 47 noncompact operation order; 44 other external shape; 10 unsupported ingress; 2 stale strength. No claim that these imply a fundamental need for a dust queue. They constrain this first-family certificate and should guide a broader parameterized multi-input/receiver program. The successful 27.22% normal-O2 speed result removes lookup overhead; most dust traversal still remains and must not be represented as already eliminated.

### 2026-10-18 — whole fixed-circuit executable prototype: compiler checkpoint

The next demo compiles a validated lowered `CircuitProgram` into `FixedClassicProgram`; it does not enumerate multi-source states or extend local net proof families. The new module owns only immutable executable tables. Existing `CircuitState` and `GraphFuture` remain the sole mutable owners, and the original program remains available externally for projection/oracle checks in this prototype.

The compiler atomically rejects contacts, pistons/resources, specialized/unknown actors, state guards, existing private classic/instant ownership, unresolved electrical queries and incomplete fixed wire geometry/callback selection. It lowers all CHUNGUS actor kinds (including Comparator, locking Repeater, Lamp, Hopper, control sources and constants), live Main/Side/Quasi weighted operands, fixed comparator override selection, original ordered output-route Seed/Shape/Update instructions, canonical site aliases, fixed raw sides, seed/Turbo transfer masks and captured callback admission. No receiver ownership, single-ingress restriction or source-vector truth table is involved.

No-dust operands preserve their distinct sampling rules: solid-block conducted power stays live, nonsolid Torch/Repeater/control emission reads captured powered flags during a walk, Comparator emission reads live analog output, and fixed constants become immediates. Site capture descriptors distinguish Wire/Gate/Inert; runtime need not reconstruct general observed profiles or select contact variants. Captured Turbo route rows are represented by admission/site dispatch rather than replayed as ordinary output-route instructions; ordinary routes retain only Live rows under the actual absent-capture guard context.

Compiler source and its field contract are now frozen for the runtime/integration owners. This checkpoint is not an executed coverage or performance result. Difficult wire transitions remain a compiled conditional ordered propagation program; achieving complete executable lowering must not be described as complete queue-free weighted-net collapse.

## 2026-10-18 — Whole fixed CHUNGUS executable prototype (first validation)

The previous certified single-ingress family covered only 8,994/133,623 CHUNGUS wires and removed about 0.22% of actual walks. The target is now 100% of fixed CHUNGUS, without silently executing unsupported wires through the general walker. A test-only, atomic whole-circuit lowering emits typed node/input, wire-transfer, ordered receiver-operation, and capture-alias tables. CircuitState and GraphFuture remain the only state and future owners. No C/LLVM path was restored.

The first lowering accepted all 251,485 actors and 133,623 wires, 369,713 routes / 2,517,665 receiver operations, 421,644 signal operands, and 1,032,940 site aliases in 0.1007093 seconds after normal preparation. This is 100% executable coverage, not 100% queue-free weighted collapse: exact conditional propagation and ordered frontiers remain where intermediate receiver samples matter. There are no whole-program source truth-table enumerations or small-region proof bounds.

The focused multiple-analog-ingress/Main/Side/reset test passed. A first CHUNGUS run completed every state/FIFO/pending/clock check at boundaries 0..1024 against Graph and independent interpreter, including the known tick-512 comparator counterexample. The test then failed only while saving its JSON report: the isolated verification worktree had no target subdirectory. The directory was created and the run repeated; the test now creates its report parent directory itself.

Verification isolation: concurrent unfinished instant-layer work currently prevents the shared source from building. A detached snapshot at target/fixed-classic-verify retains this work and disables only its unfinished experimental layer modules/exports/tests for verification; see that snapshot's ISOLATED_VERIFICATION.md. No original instant-layer source was reverted or fixed by this experiment. Whole-fixed compiler/runtime source is identical in the snapshot. The prototype is still cfg(test), and original Graph storage remains available for projection and the differential oracle.

Focused fixed-engine edge regressions are now staged in `backend/graph/tests/fixed_classic_edges.rs`: literal RedstoneBlock fifteen with a false powered flag, atomic refusal of a constant boundary-wire capture, and an independently interpreted locking-Repeater circuit driving both Lamp and Hopper through a conductor. The last fixture requires actual locked, lit and disabled states and compares full Graph state/FIFO/pending plus native projected state and scheduled work after every control/tick. These new tests await the coordinated shared build; no result is asserted here.

### Whole fixed prototype baseline report

The repeated run passed all 1,025 boundaries against both Graph and the independent interpreter. Timed core execution: Graph 3.0341706 s, fixed typed executor 2.0235601 s, ratio 1.499422x (development opt-level=1; unit-test counters retained, not a production claim). Activation 0.0009214 / 0.0006258 s. Both delivered 128,365 scheduled component ticks and counted 6,045,241 meaningful callbacks. The fixed executor used 69,763 ordered episodes and 4,422,713 wire visits; its general wire-walk count and general scratch pool remained zero. This deliberately demonstrates executable replacement, not elimination of all propagation work or scheduler events.

Evidence: target/chungus-whole-fixed-classic-baseline-1024.json and target/chungus-whole-fixed-classic-1024.log; frozen executable target/whole-fixed-classic-debug-tests.exe, SHA256 25b52d2c083da0405693c1c2c5e754a26ad76786ae08d21cdee18f07d2c82245. No warmup, original start button, full saved plot, no modified schematic. The CPU's complete Mandelbrot run has not been performed.

Next safe runtime simplification: omit capture records for constant inert aliases, which cannot be visited wires or receiver effects and whose electrical contributions were already inlined. Preserve eager captures for mutable wires/gates and preserve their layer bookkeeping even when a particular source does not admit the callback: another source can share the alias and admit it later. Descriptor copies can remove Arc refcount operations from gate/shape/route dispatch. These changes need fresh full-state validation and release timing.

## 2026-10-18 — Optimized whole-executor evidence and pipeline integration

The inert-capture/Arc simplification passed the focused analog, literal RedstoneBlock, and live repeater-lock/lamp/hopper checks, and again passed CHUNGUS 0..1024 against both Graph and native. Its development timing was collected while a release build was active and is not used to claim an improvement over the first development sample. The cut-boundary-wire fixture revealed preparation already rejects the outside wire as UninventoriedReceiver; the regression now checks that actual refusal rather than assuming it reaches fixed lowering.

A separate quiet release run of the frozen prototype checked all 4,097 CHUNGUS boundaries against normal Graph, including complete component state, exact future FIFO/deadlines/priorities, pending map/multiplicities and clock. Graph core 5.6157706 s; whole fixed executor 4.4640832 s; 1.2579897x / 20.51% less timed core execution. Both engines delivered 380,926 scheduled component ticks and counted 20,076,058 callbacks. Fixed execution had 210,566 ordered episodes / 15,105,341 wire visits, with zero general walker count and no general walk scratch allocated. This run uses unit-test instrumentation; independent native was checked in the separate 1,024-tick run. No Cargo/rustc build was running during this paired timing.

Evidence: target/chungus-whole-fixed-classic-prototype-release-4096.{json,log}; target/whole-fixed-classic-prototype-release-tests.exe SHA256 b18295f950df325572382640ed407cd8985641d44e86bcbc827ca3377aad76ed. Lowered prototype payload 100,100,492 bytes, plus original Graph retained; a material memory limitation, not a claim of compressed storage.

The measured reduction justified deeper integration. The normal O2+ pipeline now attempts a read-only compile-fixed-classic plan under the existing --classic-redstone option, after query/geometry/admission prerequisites. No additional per-pass option was added. On whole-program decline, compile-classic-redstone can still certify local nets; if the whole program owns execution, local ownership remains empty. CompileGraph and CircuitProgram use one borrowed-view compiler implementation, with cancellation and executable-memory bounds. ExecutionPlans owns the accepted Arc and diagnostic; lowering moves the same allocation into CircuitProgram, and runtime caches a shared immutable reference, keeping CircuitState/GraphFuture as sole authorities.

Stale/forged executable plans are checked by deterministic reconstruction against the draft and existing validated query/wire certificates. This checker is not an independent semantic proof; independent state/future tests remain necessary. Decline text is diagnostic history with no executable ownership, so later local-net planning may coexist with that recorded decline. Only actor output and wire shape routes generate route operations: Turbo route bodies are unreachable under the typed site instruction path and are removed without changing route IDs. The final integrated payload and timings still need measurement. Whole fixed execution has separate coverage fields and is explicitly distinct from queue-free local net ownership. Extra research counters are cfg(test) and absent in production.

### Normal pipeline verification and integration issues

Eight focused Rust checks passed after pipeline integration: draft/lowered compiler parity and forged-mask rejection; cancellation; pre-execution boundary-wire refusal; literal RedstoneBlock15; live locking repeater/lamp/hopper state and FIFO vs native; normal pipeline publication and same-Arc execution; multi-analog-source/Main/Side/reset continuation; and whole-program decline on a remote Observer while a local classic net continues with correct native state/future and disjoint ownership. A normal-pipeline CHUNGUS run passed all 1,025 boundaries vs Graph and native (target/chungus-whole-fixed-classic-pipeline-1024.log). This earlier integrated binary still retained all wire output routes; see correction below before quoting final payload size.

A broad isolated snapshot run initially had 424 passes, two failures, and 19 ignored tests. The pass-order expectation was missing the new optional builder and still placed the local builder before prerequisites; its expected order/disabled selection was updated. The O0 ADDER JSON byte comparison differed only in line endings introduced by detached-worktree checkout: recursively parsed JSON had zero differences, and the original workspace fixture already matched the generated LF bytes. Fixture bytes were copied from the original workspace into the snapshot; original fixtures were not regenerated or changed for this experiment.

The first export-module build exposed four ordinary DTO type errors (same-named node/site types shadowed base DTOs and byte validation mixed DTO/program types). Those were corrected and a real accepted-plan export-validation test was added. The first test-only counter gating used expression attributes unsupported by stable Rust; counters now sit in cfg(test) statement blocks. These failures happened before runtime testing, and no unsafe execution was accepted.

Route dead-code correction: legacy wire NodeSpec.output_route also names Turbo. Marking every node output executable accidentally kept those bodies, so the first DCE attempt removed nothing. Only electrical actor output routes and wire shape routes are now roots; wire propagation never calls fire_route on a wire and instead executes typed site admission. Route IDs and original inspection tables remain intact. Final release tests must exercise this corrected elimination.

Main workspace production check passed: cargo check -p mchprs_core --lib (target/whole-fixed-classic-production-check.log). It reports only the two unused test-adapter warnings. Viewer tests passed 28/28. Python loader tests ran 22 with one failure in the concurrently changing instant-layer lifecycle test (instant_layer_region:7 invalid phase/reason); no loader code was changed by this work. Diff whitespace check passed with the repository's CRLF handling. Unit-test verification still uses the documented snapshot exclusions for unfinished experimental instant-layer modules.

## 2026-10-18 — Final whole CHUNGUS acceptance and release result

Final tested binary: target/whole-fixed-classic-final-release-tests.exe, SHA256 55c72b7c61c549d5ff3feb51212ce7a284a8be09b084e11c52d1f321887bea6f. Normal O2 --classic-redstone preparation/planning/lowering attaches the whole program automatically. All 251,485 CHUNGUS actors and 133,623/133,623 wires execute the typed fixed program, with no generic wire-walk fallback or scratch allocation. This is 100% executable coverage for this saved CPU, not a claim that all dust state, ordered propagation, callbacks, or delayed scheduling have disappeared.

Final correctness: 1,025 completed boundaries against both normal Graph and the independent interpreter, including all actor blocks, all 21,944 comparator strengths and exact future FIFO. A separate 4,097-boundary paired run checked complete CircuitState, pending identities/multiplicities, deadlines/priorities/insertion order and logical clock. Both runs passed, including the previously wrong tick-512 comparator cluster. Focused Rust checks passed 10/10. The full Redpiler suite passed 428/428, with 19 research tests intentionally ignored; viewer 28/28; production library cargo check passed in the original workspace. Rust unit verification uses the documented isolated snapshot because unrelated experimental layer code was still changing. The previously noted unrelated Python layer-lifecycle failure remains recorded above.

Final optimized paired core time over 4,096 ticks: ordinary O2 Graph 5.7010647 s, whole fixed executor 4.5085177 s, ratio 1.2645098x, approximately 20.92% less execution time. Neither Cargo/rustc nor the regression suite was active during the timed inference phase. General preparation overlapped the tail of the regression suite, so compile times in this report must not be used as a compile-speed comparison. Both engines executed 380,926 scheduled component ticks and counted 20,076,058 callbacks; fixed propagation retained 210,566 ordered episodes / 15,105,341 wire visits while the generic walk count stayed zero. Evidence: target/chungus-whole-fixed-classic-final-release-4096.{json,log}, target/chungus-whole-fixed-classic-final-native-1024.{json,log}, target/whole-fixed-classic-final-suite-confirm.log and target/whole-fixed-classic-final-focused.log. These are instrumented unit-test timings, not an uninstrumented server-throughput guarantee.

Payload correction: the final executable tables still contain 100,100,492 bytes and 2,517,665 route operations. Turbo rows were already captured-only and filtered by the original lowering; making unreachable-route roots explicit therefore did not reduce this payload. The retained operations are live output/shape route instructions. The earlier note attributing the lack of reduction to wire NodeSpec.output_route was incorrect: current callback preparation gives wires separate shape and Turbo routes. The Rust comment now states the actual electrical-output/wire-shape roots. General graph tables remain for projection/inspection, so compact storage and complete propagation elimination are still separate optimization opportunities, not delivered claims.

Export compatibility: the additive fixed_classic tables bump JSON schema minor 1.1 to 1.2. Every existing ADDER fixture differed only in this version field after semantic comparison; only that metadata was updated in all five original fixture files, preserving their graph data and existing unrelated changes. No C/LLVM engine was restored. The existing option remains default-off. Full historical 12,411,975-tick Mandelbrot completion was not run.
