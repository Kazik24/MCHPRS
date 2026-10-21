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