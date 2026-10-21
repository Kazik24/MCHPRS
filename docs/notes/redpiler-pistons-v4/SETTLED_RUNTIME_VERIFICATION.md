# Settled inference: COUNTER and divider

## Execution model

`SettledMachine` holds the current register bits and samples only external
boundary inputs. `step` evaluates from the old bits and then commits all
proposed writes together. `observe` refreshes power outputs without committing
writes. Identical inputs reuse expression values; changed values propagate
through a summarized dirty bitset in topological order. Neither path records
input combinations or learns a counter cycle.

The caller still owns computation events, clock cadence, outside notifications,
output publication and interpreter reconstruction. This machine has not replaced
the physical Graph backend. A kernel evaluation rate is not server TPS.

## COUNTER contracts

The plain counter artifact contains 606 shared Boolean expressions. Its 16
register bits describe redstone payload presence at their canonical Far contacts;
their complement is the public count. The actual register-cone control in the
settled artifact is an active-low torch, not the user lever. Stable native
computation samples are `[Torch, Observer, Lever] = [false, false, true]`.

Native checks independently compare an autonomously evolving logical word
against 260 increments, crossing `0x00ff -> 0x0100`, without feeding native
register values back after initialization. Separate seeded checks cover long
carries and full-width wrap. Bounded symbolic proofs check the production
register equations and all sixteen output roots for all word values without
enumerating them; the output proof uses the verified computation-frame inputs.

In the tested fixture, storage becomes stable every six game ticks. Lever OFF
stops computation events and preserves the count: stopping at phase zero leaves
count 3, while stopping at phases one through five allows one pending increment
to count 4. Re-enabling adds six counts over the next 36 ticks. Evaluating an
inactive computation equation is not equivalent to stopping the clock.

Output publication is a separate phase from committing storage. Native output
repeaters lag the stable memory count. Phase four can still contain partial-word
glitches (count 3 temporarily reads 1); phase five is the observed valid bank
window. Its output checks use `observe` with the latched computation-frame inputs
rather than feeding live reset pulses into the datapath or committing a second
state transition. Internal transient bank glitches need not be reproduced under
the agreed stable-output contract. The native trajectory checks 259 phase-five
bank publications in addition to 260 stable storage updates.

These fixture histories do not constitute a production clock certificate.
Activation still needs a source-derived proof of the generator's closed timing
cone, its phase and pending work, the outside effect frontier, and a bounded
interpreter handoff. A hardcoded six-tick timer is not that proof.

## Inference measurement

On an AMD Ryzen 9 5950X, the release test binary measured five alternating pairs
of 65,536 logical computation steps. Each pair starts from the same register
state, commits its own next-state bits, and verifies final heap/bitmap state and
outputs. The control samples are the native stable computation frame above.
Setup, compilation, the initial full evaluation and state assertions are outside
the timed loops. No Cargo builds ran during measurement.

| Queue | Median ns / logical step | Logical steps / second | Operations / step |
| --- | ---: | ---: | ---: |
| Previous binary heap | 898.2 | 1,113,291 | 86.7 |
| Summarized dirty bitset | 839.0 | 1,191,910 | 86.7 |

The bitset reduced kernel time by about 6.6%. The complete 16-bit trajectory
during timing is a workload, not a cached transition table. These rates exclude
the clock adapter, outside notifications, projection and handoff; the production
1,000,000 game-TPS target remains unverified.

Evidence: `target/logical-inference/counter-kernel-release.log` and
`counter-runtime-release-build.log`. Reproduce with the ignored test
`settled_counter_heap_vs_bitmap_release_microbenchmark` in a release build.

The final Redpiler regression passed 638 tests with 77 ignored; workspace checking
for all targets and formatting also passed. Evidence is under
`target/logical-inference/settled-runtime-*.log`.

## Divider scope

The fixed divider graph contains 16,408 nodes, 1,095 recognized instant cells,
1,593 logical inputs, 15,207 expressions, 161 outputs and 5,188 retained effects.
It has no recognized logical register roots. A zero decline count therefore
does not mean it is an operand-only whole-device kernel.

The native differential diagnostic compared all ten output repeater queries
over three changing operand vectors and ON64/OFF128/ON128 phases: 90 output
comparisons across nine checkpoints, with zero skips. It samples the declared
physical boundary state, including piston availability; it does not prove that
the physical storage can be removed. The fixture's numerical quotient format
is uncertified, so these are differential checks, not a division arithmetic
oracle. See `target/logical-inference/divider-settled-native.log`.

The next divider step is to classify its actual storage and controller and
remove intermediate physical-state dependencies. Full FPU arithmetic and its
1,000 game-TPS target remain unverified.

## Current Graph backend measurements

Three release samples per executor on the same Ryzen 9 5950X produced the
following medians. These measure the existing physical Graph backend, not
activation of the settled machine above. Builds and other benchmarks did not
run concurrently. Loading, compilation, warmup and visual publication are
outside the timers.

| Fixture | Interpreted game TPS | O6 game TPS | Speedup |
| --- | ---: | ---: | ---: |
| COUNTER | 1,915 | 12,220 | 6.38x |
| ADDER11BIT | 11,064 | 42,699 | 3.86x |
| Divider | 2,074 | 6,878 | 3.32x |
| Full FPU | 204 | 778 | 3.82x |

COUNTER measures 60,000 game ticks per sample with progression and native
checkpoint checks. The paired adder workload measures 512 ticks and 64 lever
operations, produces 27 distinct decoded sums, and passes native trace,
reset and continuation comparisons. Divider uses eight changing raw input
vectors over 2,048 ticks; full FPU uses eight vectors over 512 ticks with the
green trigger held OFF and the far-side dust outputs observed. Both pass the
separate untimed per-game-tick native trace validation and timed boundary
checks. Their numerical operand/result formats remain uncertified; trace
equivalence is not an arithmetic oracle.

The FPU protocol is `fpu-green-trigger-held-off-far-output-v2`, using fixture
SHA-256 `90035d2285c964f92150ee74686465150680e005c672fb39287e7a59b9f6bf1a`.
It produces eight distinct output words. Its O6 median is 512 ticks divided
by 0.6581357 seconds, with no factor-of-two adjustment. The COUNTER and FPU
production targets remain unmet. Timer workloads differ between fixtures;
the paired speedups compare identical workloads within each fixture.

Raw reports are `target/logical-inference/bench-counter-{native,o6}.json`,
`bench-adder11-o6.json`, `bench-divider-{native,o6}.json` and
`bench-fpu-{native,o6}.json`. The release benchmark build log is
`other-builds-bench-build.log` in the same directory.

## First production integration, 2026-10-21

O6 with `--stable-instant-inputs` now compiles certified finite pull waves in
its normal pipeline and evaluates their Boolean DAGs incrementally at the
original entry cut. Stationary entry, wire levels, request provenance and FIFO
ownership remain mandatory. Commits retain Graph state, outside callbacks and
deferred work. Unknown entry provenance keeps ordinary physical acceptance.
This integrates finite computation waves, not the whole-device `SettledMachine`.

A paired release microbenchmark of an admitted two-cell wave measures one
computation tick, excluding setup, compilation, stimulus and reset. Across 32
alternating pairs after four warmup pairs, the medians are 5.90 us for O5 stable
and 3.90 us for O6 stable (1.513x). Every pair compares complete projected state
and pending work. O6 performs one 64-expression evaluation, three callbacks
and one piston sample, versus ten callbacks and three samples in O5; both
perform two physical actions. This is not a game-TPS measurement of a full build.
Evidence: `target/instant-kernel-wave.json` and
`target/logical-inference/production-kernel-wave-final.log`.

Initial large-build runs exposed overhead from replacing single-cell flat CPU
handlers with incremental waves. The final lowering therefore retains the
existing event/CPU path for single-actor waves with no owned wire corridor.
In the final release reports neither COUNTER nor full FPU executes incremental
waves: both report zero logical evaluations. Their medians are 12,139 game TPS
for COUNTER (three 60,000-tick samples) and 764 game TPS for FPU (three samples,
eight changing vectors over 512 ticks, trigger OFF). Native reference checks
pass. These results demonstrate no whole-build speedup from this integration;
the production targets remain unmet. The earlier O5 stable COUNTER control
measured 12,590 TPS, but runs were not alternating and do not establish the
size of a regression. Raw reports are
`target/logical-inference/bench-counter-o6-integrated-final.json`,
`bench-counter-o5-integrated-control.json` and
`bench-fpu-o6-integrated-final.json` in the same directory.

The real COUNTER register mapping validates all sixteen register resources and
storage actors and can stage their canonical quiescent rows without changing
unowned state. It remains test-only: scheduler/callback closure and bounded
interpreter reconstruction have not been proved.

The controller investigation distinguishes stable equations from all-phase
support. Stable substitution hides a live auxiliary reset resource from the
clock's Main input. A self-watching observer has no output deliveries, but
another observer on that same base has real callback deliveries. The new
all-phase power/callback support walker is diagnostic only; it does not permit
a hardcoded period or suppression of those callbacks. Whole-device activation
still needs a source-derived computation/publication cut and handoff proof.
The COUNTER all-phase support diagnostic conservatively declines at BUD storage
piston 113/resource 97 when following power and notification support. The stable
clock query test passes, but this diagnostic is not a closed-controller proof.
Evidence: `target/logical-inference/production-controller-support-diagnostic.log`.
The rejection does not retain edge provenance, so it does not distinguish a
power-state dependency from a notification-only dependency. Resource 97 names
the encountered storage actor's register, not necessarily the incoming edge.
Final verification for this stage: all 650 Redpiler tests pass (78 ignored).
The controller source diagnostic also passes when run explicitly. Workspace
checking with `cargo check --workspace --all-targets --locked`, formatting and
`git diff --check` pass, with no compiler warnings. Logs are
`target/logical-inference/production-integration-redpiler-tests-final.log`,
`production-integration-workspace-check-final.log` and
`production-integration-test-build-final.log` in the same directory.
## O6 logical-only COUNTER experiment

The O6 pass now automatically compiles the recognized clock/BUD family to
selected stable output roots and register next-state roots. The executor uses
`SettledMachine`, its cached dependency values, and a small logical phase machine
for the qualified two-piston/torch/observer controller. Primitive delays are
explicit; no complete input/state cases, learned clock period, or physical
callback transcript is stored. There is no extra compilation flag.

This is the deliberately restricted valid-device experiment: internal glitches,
visual piston activity, reset notifications and physical update ordering are
not part of its contract. Only logical BUD state and stable output publication
are updated. The cold geometry remains available for projection; the kernel
runs no internal wire, route, piston-action or scheduled-component executor.
The recognized clock family and normal stationary startup are assumptions,
not a proof for arbitrary redstone clocks.

The minimal test extracts one real BUD register and its matching output query
from the COUNTER fixture. Its two-root kernel matches the native circuit across
260 computations/publications. The actual automatic O6 test checks all sixteen
register bits and public output bits across 260 computations, through the byte
carry, then normal OFF and restart. It asserts zero physical callbacks, wire
walks, piston samples, piston actions, deferred publications and scheduled ticks.
This is stable-boundary validation; internal-state equality is intentionally
not required.

Interpreter reconstruction remains unfinished for this path. FPU and BubbleSort
still use the general backend; the experiment does not yet provide logical-only
execution for them. FPU topology inspection identifies uniquely owned feedback
payloads, but no production feedback-state model has been activated.
Release verification: three 60,000-game-tick runs match the frozen native
publication checkpoints. The median tick-timer rate is 5,117,707 game TPS.
A sustained three-sample run of 6,000,000 game ticks per sample measures
4,966,290 game TPS by tick timers and **4,243,471 game TPS** by the complete
benchmark loop wall clock, including timer/checkpoint/publication overhead.
Loading, compilation and warmup are excluded in both measures. No builds,
tests or other benchmark jobs ran concurrently with these measurements.

Each sustained sample executes 1,000,000 logical computations and 86,664,560
Boolean expression operations. Physical callbacks, wire walks, piston samples,
piston actions, deferred publications and scheduled component ticks are all
zero. All 30,003 sampled publication words across the three long runs match the
counter oracle, including repeated 16-bit wraps. The short run uses the frozen
native reference; the long run uses the counter arithmetic oracle, not a new
six-million-tick native replay. The counter equations and normal native clock
cadence have separate differential/symbolic validation as described above.

The experiment exceeds the 1,000,000 game-TPS COUNTER target on this workload.
It does not establish FPU throughput or interpreter handoff correctness.
All 653 Redpiler tests pass (78 ignored), workspace/all-target checking is
warning-free, and formatting/diff checks pass. The native full-device regression
also covers normal stop and restart; internal physics is intentionally absent.

Evidence under `target/logical-inference/`:
`bench-counter-logical-only.json`, `bench-counter-logical-only-long.json`,
`logical-only-counter-validation.json`, `logical-only-provenance.json`,
`logical-only-final-redpiler-tests.log`, `logical-only-final-workspace-check.log`
and `logical-only-final-release-build.log`. The frozen benchmark executable is
`logical-only-instant.exe`.

## FPU observable-tick contract

The held-OFF native FPU reference (`fpu-held-off-native.json`) has 504 output-word
changes across 2,016 adjacent game-tick comparisons within its 32 input holds.
It repeatedly publishes the computed word and `0xffff` while the inputs remain
unchanged. Matching just the final word of an input hold is insufficient to
establish valid-result publication timing. The agreed contract permits transient
glitches to disappear: a raw tick difference is diagnostic, rather than proof of
incorrect execution. Valid publication ticks and required BUD write events must
be identified from event state and preserved; `0xffff` alone cannot classify a
tick as a reset glitch because it can also be a legitimate output word.

The current experimental extractor represents shared-Far feedback payloads with
one explicit Boolean state bit and simultaneous next-state writes. Up to five
owners can pull one payload; the pull condition is OR of owner pull requests
(equivalently, source presence is AND of owners staying powered). The winning
owner and private destination choice are discarded by the O6 model. This
is a candidate equation representation, not evidence that advancing every
feedback bit once per game tick matches native operation. Activation remains
unchanged until a differential trace establishes the required event timing.
BubbleSort additionally requires explicit BUD write/update events: an unchanged
signal value alone does not establish that no memory update is needed.

The first closed FPU extraction declines at `ResourceId(4970)`. Its two-contact
payload is in a group containing actors 7101 and 7638; the catalog rejects the
group because actor 7638 lacks an admitted reset witness. The COUNTER timing
recognizer also finds no qualifying empty-lane clock in this FPU. Neither a
Boolean value substituted for that resource nor a copied six-tick schedule has
been activated. Broader support diagnostics are test-only, and the native trace
comparison requires a closed input/state dependency set before execution.

The broader support census reaches 115 unresolved resource leaves before the
expression builder exhausts its budget. It requires a larger Windows test
thread stack (`RUST_MIN_STACK=33554432`); the default stack overflowed during
the diagnostic expansion. This census is not a completed FPU kernel or an
output-equivalence result. The next candidate extractor can infer local
shared-source lane equations without catalog reset admission, but remains
test-only and subject to the native differential checks.

Shared graph-query memoization now survives successive logical power queries
within the same immutable compilation context and expression arena. It stores
completed equations only, discarding pending recursion markers after errors.
The builder also charges retained operand storage at interning time, so repeated
equivalent Boolean expressions do not consume the retained-memory budget again.
Neither change stores input-value combinations or a native callback transcript.

## BUD event investigation

The BUD read descriptor is not a general logical memory executor. Ordinary
pistons still own requests, acceptance, motion and resource publication. Native
updates sample Main/QC power before requesting an action, and execution rechecks
live power before accepting it. A separate test-only detailed recorder now
captures these cuts without changing the frozen Sample/Applied trace format.
Request entries precede queue deduplication; Accept entries are attempts, and
only Applied establishes success.

The COUNTER probe covers six OFF phases. Matched native request/acceptance power
is unchanged in these windows. All six drained storage states match O6, and
running/restarted outputs match at valid publication cuts relative to each start.
Absolute tick modulo six is not a valid restart oracle. A real gap remains:
native output clears to zero while stopped and remains there through the
24-tick drain, while O6 retains its last published word. The ignored differential
records this discrepancy before failing; runtime semantics have not been changed
pending discussion. Running throughput measurements above do not establish
correct stopped-output behavior.

ANPU has 896 fixture-labeled lower readers with upper storage topology witnesses,
but the current O5 BUD read recognizer covers zero of them. The focused probe
records raw native events for an active pair even at zero recognition coverage;
selection from topology is explicitly distinguished from admitted recognition.
BubbleSort's 768 RAM observation positions likewise do not by themselves identify
storage ownership. Its focused native probe follows the frozen load/reset/start
protocol and traces one observed bit through active tick 98, plus the candidate
upper piston and nearby updates. A changed-word checkpoint is not proof of the
first physical transition when other bits make the whole word unavailable.

Evidence: `target/anpu-bud-recognizer-native-trace.json`,
`target/logical-inference/counter-bud-off-phases.json`,
`target/logical-inference/counter-o6-off-phase-differential.json`, and
`target/logical-inference/cpu-bubblesort-first-ram-transition.json`.
These are focused diagnostics, not whole-device logical equivalence proofs.

The Rilax one-cell probe confirms the same upper-storage/lower-reader topology
seen in ANPU: a downward sticky storage piston owns one two-contact redstone-block
resource, and the reader four blocks below has one unattenuated Quasi contribution
from that resource's Far contact. Fixture labels differ: Rilax observes upper
storage positions, while ANPU's labels select lower readers. That label difference
does not establish different memory semantics. Rilax's ordinary storage actors are
intentionally outside instant-cell admission.

Native Rilax tests establish sampling on both write-enable edges, unchanged-data
samples, latched reads, short-pulse filtering, read publication at tick 10 and
release clearing at tick 12 for the declared protocol. The focused replay writes
bit zero, observes seven ordinary updater retractions, and clears the read output
on release. Evidence: `target/rilax-bud-single-cell-detail.json`. Ten Rilax
regressions and the additional ignored detailed probe pass. The original fixture
has a documented missing decoder torch at address 1; existing tests keep it as a
negative case and separately test a derived repaired world. The binary is unchanged.

Current verification also passes all 44 native piston regressions and warning-free
workspace/all-target checking. A broader core test attempt reports failures outside
these changes (including client packet expectations), so no whole-suite green
claim is made. The known O6 stopped-output diagnostic remains failing as described
above; generic logical memory execution is not implemented by these probes.

## FPU settled observer experiment

Under the user's fully instant FPU contract, the test-only computation extractor
now folds observer/reset outputs OFF before reduction. This removes all 353
undeclared observer inputs: the candidate closes with 22,193 expressions, 53
control predicates and 15 feedback candidates. Ordinary settled extraction still
retains its existing observer/effect behavior. Ten settled extraction regressions
pass, and workspace/all-target checking and formatting pass without warnings.

The native diagnostic now executes 2,048 ticks across warmup and 16 held-input
vectors. Fresh-frame evaluation initializes canonical Far state and converges in
one to four logical passes in these cases, producing the same candidate words as
carried feedback. This is within-computation iteration, not a demonstrated need
for persistent cross-computation memory. Candidate words occur in the final
native hold window for 15 vectors, but multiple native plateaus prevent interpreting
that alone as valid publication equivalence. For vector 0, candidate `0x8446`
never appears in the final 24 native ticks, which alternate `0x8000` and `0x8800`
with `0xffff` pulses. Reset-frame initialization does not resolve that discrepancy.
The raw mismatch count includes allowed transients and is diagnostic only.

Evidence: `target/logical-inference/fpu-computation-frame-trace.json` and
`fpu-observer-fold-native-probe.log` in that directory. The probe's successful test
status establishes dependency closure and trace completion, not output parity.
No logical FPU throughput or production activation is claimed.

## Provisional FPU kernel throughput

The now-closed candidate can be benchmarked independently of native execution.
On a Ryzen 9 5950X, the normal release profile with fat LTO measures the following
medians across three samples of 4,096 changing operand pairs per selector:

| Raw selector mask | Calculations/s | Microseconds/calculation | Mean logical passes | Distinct output words |
| --- | ---: | ---: | ---: | ---: |
| 000 | 33,050 | 30.26 | 1.00 | 1,238 |
| 001 | 15,055 | 66.42 | 2.90 | 3,650 |
| 010 | 19,697 | 50.77 | 4.00 | 3,030 |
| 011 | 10,846 | 92.20 | 3.89 | 3,913 |
| 100 | 19,548 | 51.16 | 4.00 | 2,349 |
| 101 | 11,062 | 90.40 | 3.88 | 3,912 |
| 110 | 9,337 | 107.10 | 3.84 | 3,531 |
| 111 | 460,748 | 2.17 | 1.00 | 1 |

These masks index the manifest selector controls; they are not certified numeric
opcode names. Pattern 111 has one constant output and is excluded from the
9,337–33,050 calculations/s range. Every timed calculation applies pre-generated
input values, resets temporary feedback to canonical Far, evaluates the actual
incremental Boolean graph until fixed point, and collects output/checksum data.
Graph/evaluator allocation and stimulus generation are outside timing. The cache
holds only current expression values, not whole input/result cases. All 98,304
timed calculations converge; an untimed 16-vector differential also verifies the
reusable evaluator matches the existing experimental SettledMachine.

Cold setup measures fixture load 154.6 ms, graph preparation/passes 6,978.3 ms,
and logical extraction 701.5 ms. The Rust release binary build is separate and
excluded. No other builds/tests/benchmarks ran during measurement. Evidence is
`target/logical-inference/fpu-kernel-benchmark.json` and its `.log` companion.

This establishes provisional kernel throughput only. The user now explicitly
accepts the nonstable native case and the reported candidate value difference for
this experiment; it no longer blocks FPU work. The candidate is not yet installed
as a production FPU device.
No conversion to game TPS is made without a validated publication adapter/cadence.

The exact 16 input vectors are exported to
`target/logical-inference/fpu-input-vectors.csv` and `.json`; the JSON includes the
first vector's per-lever positions/states, fixture hash and replay protocol.
The first vector is raw mask `0x4D6035310`: light-blue bank `0x5310`, red bank
`0xD603`, selector levers of weights 4/2/1 OFF/OFF/ON, green trigger held OFF.
Masks index manifest lever order; they are not numerically certified IEEE words.

## O6 live power compilation for BubbleSort and ANPU

O6 now compiles Main and Quasi piston power into flat Boolean predicates for
ordinary memory actors as well as admitted instant actors. The pass reuses the
existing bounded predicate compiler and mask evaluator; it does not enumerate
input combinations or memoize whole circuit cases. Every request and acceptance
reads current component/resource state. Classic Main overlays and recognized BUD
Quasi shortcuts retain exact per-port input reads where needed. Scheduling,
unchanged-value notifications, resource ownership and publication remain on Graph.

The full fixture regressions verify:

- BubbleSort: 16,561 of 16,561 actors, 57,893 packed reads, zero whole-input
  fallback reads; all 10,036 frozen native RAM ticks and 32 interpreter ticks
  after materializing Graph state and queued work.
- ANPU: 2,898 of 2,898 actors, 6,288 packed reads, zero whole-input fallback
  reads; all screen pixels through 5,000 ticks, exactly 14 transition frames,
  and all 1,552 frozen ordered BUD update rows. BUD comparisons include sample
  and accepted-action counts plus SHA-256 of the original native-format entries,
  including tick, phase, position, sampled power and event identity.
- Ordinary Rilax memory: each Main/Quasi root agrees with recursive reads under
  changing live state; a write-enable pulse preserves complete state, scheduler,
  pending requests, deferred work and ordered operations against a backend with
  the new power plan disabled.

The normal O4/O5/O6 changing-input and interpreter-handoff regression, COUNTER's
stable publication regression, packed-mask evaluator checks and export binding
validation also pass. The large CPU replays remain explicitly ignored tests;
run `cargo test -p mchprs_core --lib power_quer -- --include-ignored --nocapture --test-threads=1`.

This is production compilation of live piston power logic, not whole-device
contraction: physical wire propagation and timed memory events still execute.
The qualified COUNTER clock replacement and FPU computation-frame assumptions
are not used to replace either sequential CPU's event timing.

Release measurements on the same Ryzen 9 5950X use three fresh runs per tier:

| Build / measured window | O5 median game TPS | O6 median game TPS |
| --- | ---: | ---: |
| BubbleSort / complete 10,036 ticks | 294 | 309 |
| ANPU / first 400 active ticks | 1,595 | 1,720 |

These time `tick_with_world` calls only; loading, compilation, display flushing,
output probes and handoff checks are excluded. No other builds/tests/benchmarks
ran during measurement. O5/O6 active, reset and continuation checkpoints match;
BubbleSort RAM hashes and ANPU's restored future queues also match. BubbleSort
O6 sample times are 28.940, 32.476 and 33.582 seconds, versus O5's 33.190, 34.120
and 35.630 seconds, so the modest median difference overlaps sample variation.
Median cold compilation is 15.883 seconds for BubbleSort and 8.690 for ANPU at O6.

The full O6 sort still performs 93,365,384 callbacks, 35,415,396 piston samples,
3,295,083 actions and 11,143,303 compiled transport wire visits. General wire
walks are already zero. Further contraction must target event/reset fanout and
physical actions; compiling more power reads alone will not provide COUNTER-like
throughput. Evidence: `target/logical-inference/bubblesort-o{5,6}-power-*.json`
and `anpu-o{5,6}-power-*.json`.
