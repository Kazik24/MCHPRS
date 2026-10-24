> Archived experiment — removed from production on 2026-10-24. The bounded
> replay below could not prove whole-circuit timing, especially for large builds
> with many inputs. `/rp analyze --timing` is no longer supported. Historical
> findings below describe the removed implementation, not a current feature.

# Timing analysis: sampling hazards, arrival margins and witnesses

Status: the first numerical late-inhibit checker is implemented and passed
focused validation. Broader static timing proofs below remain planned.
The arbitrary route previews are gone. `analyze --timing` needs no optimization
level or compiler input promise; it probes isolated native snapshot episodes.
Written 2026-10-24. This replaces the route-preview approach, not the simulator.

## Implemented first family: observed late inhibition

Usage: `/rp analyze --timing`. O0 is sufficient. An earlier provisional automatic
O4 upgrade was removed because this checker needs the base inventory, not O4
kernel/layer compilation. Explicit optimization flags keep their usual meaning.

The cold helper in [plot/redpiler_timing.rs](../../../crates/core/src/plot/redpiler_timing.rs)
discovers currently positive fixed redstone blocks and powered levers, excluding
movable contact positions. Each probe forks the same snapshot, removes one
redstone block or toggles one lever off through native notifications, and holds
other sources at snapshot values. These are **hypothetical source transitions**,
including possible supply removal, not inferred declarations of intended ports.

It reuses native `prepare_operation` and `advance_operation`; the cohort size
calculation is shared with public `nanotick_advance`. No timing recorder or
formatting is inserted into normal Graph execution. After an actual retraction
has changed a consumer base, a distinct later accepted retraction is checked for
making that consumer's effective power change from false to true. This is an
observed late inhibition: power that would have prevented acceptance became
effective only after the action. Merely queued or cancelled requests do not
qualify. A nearby wire drop is recorded as an optional physical effect witness;
it is not automatically an intended output port or truth-table bit.

The scope is the first piston-event phase of each probe. Pending entry work is
unknown. Bounds are 16 independent sources, 64 queue cohorts, 512 native
operations, 8,192 power checks and 8,192 consumer comparisons per source;
incomplete coverage is reported. These limits bound explicit diagnostic work,
not the duration of an individual native synchronous callback. Rising edges,
arbitrary simultaneous control combinations, later reset phases and all-history
absence of hazards are not certified. A legitimate pulse circuit can exhibit
this ordering intentionally; the report proves the observed mechanism, not the
builder's intended function.

The supplied `NANOTICK_EXAMPLE.schem` is the regression target. Its falling
source splits into an ordinary path and a path containing one extra piston.
Native piston cohorts are:

1. First delay piston and ordinary source piston.
2. Second delay piston and downstream consumer.
3. Inhibitor piston, which restores consumer power too late.

The required report is **+1 nanotick (cohort 2→3)**. The output wire falls 15→0,
which this fixture decodes as an unwanted logical-one event. Native tests cover
all four rotations; analyzer tests compare O0 and O4. A physically rewired
in-memory balanced variant bypasses the extra stage and should keep output
power at 15 without a late-inhibit finding. The saved schematic is not modified.

Chat shows a summary, the probe assumption, at most four two-line witnesses,
and grouped incomplete reasons. Same-cohort ordering is described as **later
within the same nanotick**, never as zero extra delay implying safety. The
remaining document describes broader future analysis, not guarantees already
provided by this first family.

Validation: the `timing` filter passed 11 tests (3 ignored research tests), the
`nanotick` filter passed 5 (1 ignored), and the actual packet-decoded
`/rp analyze --timing` command regression passed. These filters overlap once:
16 distinct tests passed. The native fixture regression covers all four
rotations; O0/O4 analyzer probes and the in-memory balanced variant agree with
the native observations. Pending entry, cancellation, cancelled requests,
actual operation-budget exhaustion, output truncation and same-cohort wording
are covered. The command test checks the +1 warning, witness positions and
unchanged blocks/piston state/scheduler with no compiler activation.

`cargo test -p mchprs_core --lib --locked --no-run`,
`cargo check -p mroww --locked`, focused rustfmt checks and `git diff --check`
passed. The generic memory nano-step regression expects Cargo's package working
directory (`crates/core`) when invoking the test executable directly; running
it from repository root initially failed to locate its relative schematic, and
it passed from the correct directory. Early compile/test-fixture mistakes and
an obsolete UI assertion were corrected before the final runs. Full-workspace,
release-runtime, large FPU/CPU replays and benchmarks were not run for this batch.

## Goal

Identify where an observable device can sample the wrong value because data,
inhibition or an update reaches it in the wrong causal order. Report the device,
the competing paths, the exact supported input conditions and a useful timing
margin. Do not print ordinary routes as warnings or call an unproved piston
non-instant.

A warning should answer:

1. Which block and which read/acceptance are affected?
2. What arrives too late, or changes too early?
3. Can this change an output, stored value, accepted action or scheduled work?
4. Under which inputs and starting state does it happen?
5. Is the gap exact for one verified scenario, bounded across a stated protocol,
   or unknown? What unit does the number use?

## Semantics and units

Use [PISTON_MODEL.md](../../../docs/PISTON_MODEL.md), particularly section 5,
and [REDSTONE_MODEL.md](../../../docs/REDSTONE_MODEL.md), section 15.2.

- Game ticks measure actual scheduler delay. Preserve phase and priority as
  separate ordering facts; same tick does not mean simultaneous execution.
- Public nano stepping consumes a count-snapshot of the current phase's queue.
  It is not one callback, one wire hop, one accepted piston or one dust layer.
  Newly inserted higher-priority work may run within that count-snapshot.
- A synchronous callback or Turbo walk can contain an entire chain of changes.
  Its internal read/write order matters even if both events share a public
  nano-step boundary.
- Existing `CausalLayer` indices are conditional FIFO frontiers. They are useful
  proof evidence, but cannot be relabelled as absolute nanotick timestamps.

For a concrete replay, record game tick, existing execution phase, public
nano-step cohort and the ordered cut inside the operation. Match the existing
fine-step queue-count rule if reporting public nano steps. If the implementation
only knows causal depth, label it **causal stages**, not nano steps.

Only subtract counts that share a defined origin, unit, phase and scenario.
For events in different phases, report both phases and the game-tick difference;
do not flatten a lexicographic timestamp into an invented numeric duration.
Callbacks with the same event sequence are alternative branches, not elapsed
time. Callback ordinals, route IDs and wire attenuation are not time either.

An exact diagnostic can say, for example:

```text
Confirmed late inhibit at (x,y,z), input scenario S:
consumer accepted in piston cohort 1; inhibit became effective in cohort 3.
Gap: 2 piston queue cohorts in this replay; both events occurred in game tick T.
Witness: input A -> ... -> consumer; input B -> ... -> inhibit.
Effect: accepted payload owner differs from the inhibited reference transaction.
```

That count is not a guarantee that adding two buffers fixes the build. Added
work can change queue order and reset behavior; a proposed repair needs replay.
If the issue is inside one synchronous operation, show the two ordered cuts
and say **same cohort, inhibit applied after acceptance**, without inventing a
fractional nano tick or advertising zero margin as safe.

## Scope and assumptions

Analyze the compiler's supported circuit subset. Do not require a dedicated
trigger, a universal global clock or a fixture-specific adder pattern.

Stable external inputs and synchronous input changes are separate contracts.
`--stable-instant-inputs` does not establish that unrelated controls change at
the same boundary. An input scenario must name its activation group, transition
directions, initial data/memory and observation/reset window. Without one,
preserve independent controls and report conditional possibilities or unknowns.

Do not generate the full Cartesian product of all CPU/FPU input histories.
Start with a structural census and analyze only the dependency cone of each
candidate. Missing reset, cancellation or budget exhaustion is not a safe result.

## Algorithm

### 1. Find semantic sampling points

Use the shared prepared graph and its typed power/QC, Update/Shape, contact,
comparator and scheduling relationships. Build an index of:

- piston callback power samples and later FIFO acceptance rechecks;
- BUD data ports and the separate notifications that sample them;
- repeater scheduling/dispatch, locking and pending-transition decisions;
- comparator rear/side/override reads and output scheduling;
- observer watched-cell changes, pulse scheduling and reset transitions;
- shared payload ownership checks, source removal and restoration;
- observable output updates, state writes and pending work.

Request, acceptance and publication are different points. A late inhibit after
request but before acceptance can correctly cancel an action; do not warn on
the request alone. A wire value changing late matters only if a relevant reader
can observe the previous value or a transient effect.

Use original sites and receiver kinds for positions. A head callback should
name the actual head target and separately identify its base actor.

### 2. Build backward dependency cones

From each sampling point, follow its actual value/guard/geometry reads and its
qualifying activation paths back to boundary inputs or persistent state.
Record direction, strength threshold, attenuation, guard, live/captured view
and event provenance. Include an observer of geometry even when no power link
connects it to the changed block.

Keep one immutable graph and compact ID-based indexes. Share cones and expression
subgraphs; do not clone the entire circuit per candidate. Reuse `BoundaryCatalog`,
`LayerInputRole`, `LayerUse`, source equations and validated contact facts where
their proof scope matches. O0 missing optional plans means evidence must be
derived or marked unavailable, not an empty analysis.

### 3. Extract candidate hazards

Join value and activation cones at real readers. Useful candidate families:

| Family | Required relationship |
| --- | --- |
| Late inhibit | An inhibition effect competes with the consumer's acceptance read. |
| Data/update race | A defined data preparation competes with the BUD transaction's sample/recheck. |
| Shared-owner race | Feasible actor actions compete for one payload before ownership is settled. |
| Sampled glitch | A receiver can sample an intermediate rather than the intended settled level. |
| Rearm violation | The next permitted activation precedes required reset/readiness. |

Two unequal path lengths alone are not a hazard. For example, a max-power join
may remain unchanged until both inputs fall. Resolve the Boolean/strength and
guard conditions before escalating a structural candidate.

### 4. Preserve causal scheduling in a bounded event analysis

Reuse the existing execution semantics; do not create an approximate second
Turbo or piston simulator. Existing layer equations can summarize certified
pure corridors, but any summary used here must preserve the relevant sampling
cuts, intermediate effects and shared-resource prefix order.

Maintain a happens-before relation for emitted changes and samples. Follow
synchronous nested calls before returning to the caller; enqueue FIFO requests
at their actual cut; revalidate acceptance against the then-current state.
Scheduled components preserve due tick, priority and pending multiplicity.
Captured receiver properties and live power reads remain separate operands.

For a concrete scenario, an exact cold replay uses the existing Graph procedures
with a minimal diagnostic event observer. Validate witnesses against native
execution/fine stepping. Do not add formatting, full traces or allocations to
normal ticking; instrumentation must be absent or inactive outside diagnostics.
If exact public cohort membership is not exposed by the shared dispatcher,
implement/reuse its queue-count stepping contract before publishing that metric.

For general results, propagate feasible symbolic branches through these same
semantic decisions, with branch/state budgets. Interval arrival windows can
screen candidates but do not prove a race: competing paths must be jointly
feasible. Use existing Boolean reduction for easy predicates. A later solver
can answer difficult feasibility queries; no SAT dependency is required for
the first useful milestone. Never call an unverified solver model a finding.

Strongly connected components containing persistent state become memory
interfaces. Cycles without a justified finite-use/reset contract are unknown;
do not assign them a topological depth. Instant single-use behavior applies
only where the existing certificate establishes it for that epoch.

### 5. Compute qualified margins

For each candidate, identify the required value-change event E and consumer
sampling/acceptance point R under an explicitly stated transaction contract.
When E and R are comparable in one verified scenario, report their order and
their supported tick/cohort counts. The sample sees old versus new data by
actual read semantics, not by a heuristic comparison of path length.

For all-history bounds, keep the predicate under which each ordering holds.
Compute the extrema of `arrival(E) - sample(R)` over the SAME feasible histories;
do not subtract independently computed earliest/latest times from incompatible
branches. State whether a range is tight or merely a conservative bound. A
bounded replay gives a counterexample, not a proof over every history.

For BUD storage, lack of continuous resampling is intentional. Only warn about
a violated declared transaction or a demonstrated externally distinguishable
sampling outcome. Likewise, a pulse is not a lost repeater activation until its
actual scheduler and cancellation behavior establish that effect.

### 6. Verify observable impact and reduce the report

Replay the candidate input/state history against the original interpreter and
compare relevant output signals, payload ownership, persistent memory, effects
and pending work at the named cuts. Compare with the stated transaction's
expected result where available. A timing-sensitive mechanism with no intended
protocol is not automatically faulty.

Deduplicate by receiver, sampled port, hazard kind and equivalent input condition.
Prefer one shortest causal witness per family, preserving alternate witnesses
in the saved report. Repeated divider slices should become one family with a
count and representative coordinates, not eight arbitrary route messages.

## Result model and display

Keep a small cold report, separate from `CircuitProgram` and tick state.
If these types are introduced, every field has this responsibility:

```rust
struct TimingFinding {
    kind: HazardKind,           // Names the concrete sampling or reset hazard.
    sink: TimingEndpoint,       // Identifies the actual receiving site and semantic read.
    condition: ConditionId,     // Refers to the report-owned input/state predicate.
    evidence: TimingEvidence,   // Distinguishes a verified case, proof, possibility or unknown.
    margin: TimingMargin,      // Stores only counts/ranges with explicit comparable units.
    witness: WitnessId,        // Refers to shared cause/sample paths and replay provenance.
}
```

`HazardKind`: late inhibit, data/update race, shared ownership, sampled glitch,
or rearm violation. `TimingEndpoint` references original site/actor IDs and the
read kind. `TimingEvidence`: verified scenario, proven under protocol, feasible
candidate awaiting replay, or unresolved with a reason. `TimingMargin`: exact
count with its unit/origin, qualified range, ordered same-operation cuts, or
unknown. Enum payloads must document the meaning and ownership of every field.
Use existing coordinate/graph ID types; do not introduce stringly typed timing.

Save snapshot identity, options, assumptions, coverage, budgets/truncation,
scenario and witness with the report. IDs are not valid after edits/recompilation.
Chat shows counts by evidence class plus a bounded set of actionable findings.
Unknowns receive a separate summary with affected counts and missing evidence;
no findings in an incomplete analysis must never be described as safe.

## Implementation milestones and review gates

1. **Remove misleading route previews (this batch).** Keep honest availability
   and existing conditional-layer evidence. No new timing engine yet.
2. **Structural sampling census.** Add `analysis/diagnostics/timing/` modules
   only as responsibilities emerge: dependency extraction, analysis and report
   formatting. Export data/update/sample/acceptance/reset witnesses and coverage;
   do not emit numerical margins yet.
3. **Exact replay witnesses.** Start with an isolated late-inhibit fixture and
   an independent BUD data/update fixture. Produce a reproducible input history,
   actual wrong/distinguishable result and a phase-qualified gap. Check native
   traces and nano stepping before naming counts nano steps.
4. **Conditional proof on bounded cones.** Add symbolic branches, tight bounds
   where feasible, finite-use/reset treatment and explicit unknown reasons.
   Each confirmed warning requires a feasible witness or a checked proof.
5. **Large-build aggregation.** Analyze ADDER1BIT/11BIT, Divider, COUNTER,
   PC COUNTER and FPU. Group repeated slices and preserve independent controls.
   Measure analysis time/memory separately from runtime; retain cancellation
   and the existing shared preparation path.

Each milestone gets a human review. The first release need not prove every
FPU history: a small set of confirmed, reproducible hazards is more useful than
a circuit-wide list of speculative warnings.

## Required counterexamples and tests

- Inhibit before request, between request/acceptance, and after acceptance.
- Different causal lengths with an unchanged max-power join: no false warning.
- Stable input but shared-payload FIFO dependence: preserve real output effects.
- BUD data without update: intentional held memory, not automatically a fault.
- Separate BUD data/update controls, both orders and recheck cancellation.
- Repeater short pulses, locking and pending ticks; observer geometry pulses.
- Guard-exclusive paths whose independent timing intervals overlap but no joint
  input assignment activates both; do not report a confirmed race.
- Multiple notifications sharing one sequence, captured/live differences,
  nested Turbo callbacks, same-cohort hazards and phase-crossing work.
- Clock reactivation before reset versus a permitted post-reset activation.
- Rotation/translation invariance, source IDs changing without changed results,
  real head versus base coordinates and repeated divider-slice aggregation.
- Unsupported cones, analysis budgets and cancellation: report incomplete,
  never fabricated exact numbers or a safe circuit.

Use existing fixture loaders, Graph/native comparison and reference-contract
recorders. Do not create another fixture loader, simulator, profiling framework
or hard-coded FPU/Divider recognizer. No production execution changes or new
optimization passes are authorized by this scope document.

## References

- [Current timing report](../../../crates/core/src/redpiler/analysis/diagnostics/timing.rs)
- [Shared candidate preparation](../../../crates/core/src/redpiler/analysis/graph.rs)
- [Boundary representation](../../../crates/core/src/redpiler/program_graph/instant_boundaries.rs)
- [Sampling roles and causal layers](../../../crates/core/src/redpiler/program_graph/instant_layers.rs)
- [Ordered Graph phases](../../../crates/core/src/redpiler/backend/graph.rs)
- [Piston sampling and acceptance](../../../crates/core/src/redpiler/backend/graph/pistons.rs)
- [Player analysis plan](REDPILER_PLAYER_ANALYSIS_PLAN.md)
