# Step 04: regions, gate connections and guarded causal layers

Status: partial implementation; connected-region discovery and isolated-cell layers are
implemented. The ADDER1BIT causal-layer gate is not met. Runtime replacement remains disabled.

## Goal

Implement the analysis body of `passes/compile_instant_layers.rs`: partition instant cells,
trace their logical effects from input to output, and produce inspectable guarded layers.
This is the second compiler pass. Its iterations make layers, not more optimization passes.

## Suggested files

Use focused `analysis/instant/regions.rs` and `layers.rs`; add `gates.rs` only if the
connection logic has a distinct real responsibility. Keep the pass file an orchestrator.
Implement the minimal typed Boolean arena needed to express guards; reduction comes in
step 05. Do not recreate a generic symbolic executor as a collection of renamed files.

## Algorithm

1. Join cells only through instant corridors or actual shared ownership. Build indexed
   pin-value and activation adjacency, preserving causal/source witnesses.
2. Resolve local cell pins to boundary inputs or predecessor pins. Carry attenuation and
   polarity decisions from extraction; a long physical path can be electrically absent.
3. Compute SCCs and the acyclic condensation graph. Construct acyclic value equations
   directly, without enumerating input combinations or following every physical route.
4. Seed guarded activations from supported boundary events and admitted reset-entry roles.
   No root lever/common trigger or complete-device recurrence is required.
5. For each frontier, derive sampled requests, queue deduplication and causal precedence.
   Revalidate acceptance against earlier logical owner/inhibit writes. Record both request
   and acceptance predicates; consuming a use on request is a bug.
6. Update the per-use consumed expression only under accepted computation. Derive changed
   pins, resource-owner selectors and outbound effects from cell contracts, then build the
   next frontier. Keep reset transitions out of the consumed computation-use budget.
7. In a monotone SCC, accumulate accepted activations until no new ones are possible.
   In an order-sensitive SCC, preserve small guarded causal blocks and acceptance order.
   If ordering cannot be derived, return a witness. Never choose NodeId/DFS order by default.
8. Retain one logical state expression per pin/owner, not one copy per path history.
   Merge occurrences only when their order, input observation and effects are equivalent.
   Keep guards for a cell eligible at several layers; a failed earlier evaluation does not
   prevent later success. Bound candidate and expression growth explicitly.
9. Validate finite-use provenance and stop on a certified wave cut. A reset/wake is a new
   entry through the scheduler, not an infinite compile-time loop under held input.

## Required invariants

- `accepted(use) => requested(use) AND valid_at_acceptance AND NOT previously_consumed(use)`.
- Per concrete input/state valuation, one computation use is never accepted twice in a wave.
- Failed samples cannot create an unbounded chain of fresh events without a changed cause.
- One concrete wave with N uses has at most N accepted uses; this is not a global O(N)
  bound on conditional construction. Budget overruns decline instead of dropping layers.
- A causal layer is not automatically a parallel-commit set. Shared owners stay ordered.
- Internal inversion/inhibition is not assumed monotone. A negated stable data guard is
  distinct from checking that another internal event has not arrived yet.

## Tests and inspectable results

Start with a tiny pure scalar evaluator for the extracted cell contracts and compare it
with native fixtures. It is a debug oracle for the layer builder, not a new production backend.
Generate bounded cell graphs with fan-in, fanout, positive feedback, inhibited feedback,
late AND inputs, blocked uses, cancelled requests and shared owners. Exhaustively evaluate
small inputs/states and shuffle discovery traversal order; results must not depend on DFS/BFS.

Export each layer's use, requested/accepted guard, cause and consumed state for diagnosis.
Build all ADDER1BIT layers and account for all pinned actors. Reproduce the known adder
counterexample's real repeater output with the declared physical/atomic-edge protocol.
If a fixture needs a second computation use or unresolved owner ordering, report it; do
not silently fall back and claim that the layer compiler succeeded.

## Gate

ADDER1BIT compiles to a finite cell/layer artifact without wire snapshots or arbitrary
internal wire-strength inputs. Output/effect interpretation agrees with the independent
oracle for its admitted histories. Record construction sizes, largest SCC and decline
reasons before designing the runtime evaluator.

## Implemented review slice

`passes/compile_instant_layers.rs` is a read-only builder selected only by
`experimental_layer_pipeline` in tests. Public O0–O4 selection is unchanged. Its result
is cold `InstantPlans.layers` data and is dropped at runtime lowering. Replacing the
boundary catalog clears old layers; validation recomputes the source-derived artifact
and rejects stale layers or layers without their original catalog. Cancellation and
construction-budget failures abort the builder before publication.

`analysis/instant/regions.rs` reuses the boundary owner map. It joins an owned producer
and reader only when they reference the same original dependency. Shared outside
fanout and output-only rows do not merge regions. Mutable vectors accumulate members
and ports before one immutable conversion; joining endpoints uses one representative
instead of a producer/reader Cartesian product. Work and retained-payload accounting
are bounded; allocator overhead is not a peak-RSS measurement.

`analysis/instant/layers.rs` currently targets one actor with one private payload and
its recognized reset observer, without an internal wire corridor. Its boundary events
remain separate from positive-after-guard,
direction-and-attenuation power predicates. A small shared Boolean DAG derives:

```text
requested = qualifying_event AND sampled_ready AND sampled_extended AND NOT sampled_power
accepted  = requested AND live_validity AND NOT consumed_before
consumed_after = consumed_before OR accepted
```

The input binding states exactly which predicate and semantic phase it reads. Multiple
qualified boundary occurrences can activate the same single use. Immediate callback
outputs retain base/Far/Near phase order and original delivery ordinals, including repeated
use of the same route in different phases. Route IDs never supply causal order.

The stationary-entry root describes the original request cut. The private token moves
only under `accepted AND present_at_acceptance`; Far callbacks use that predicate,
while base/Near callbacks use acceptance. Local reset power is read from the existing
observer, including the recognized static-conductor path, at each declared cut. A reset
Power output remains a live value root instead of becoming an accepted-action pulse.
Self-updates whose possible receivers are only this actor or inert variants remain
explicit lifecycle rows: once acceptance makes the base pending, they cannot enqueue
another computation request. Foreign, nested or other component receivers remain unproved.
The original scheduler retains reset and restoration work; it is not compiled away here.

GuardState outputs are values at the post-pull cut, not pulses: availability becomes
BasePending after acceptance, the head disappears, and a transferred resource becomes
disabled at Near. The actor's logical extended bit remains unchanged until deferred
publication, following the existing Graph semantics. Later reset-output and Far Shape
routes remain explicit ordinary-Graph lifecycle rows as well as both restoration contacts.

The real `parser_admits_a_private_reset_cell_without_synthetic_contracts` fixture is
created through ordinary block preparation and cell discovery. It produces one region,
one causal layer, one actor and one private resource: 46 shared Boolean expressions,
20 phase-qualified inputs, 37 value/effect output rows and 125 retained lifecycle rows.
It predicts the native accepted pull and source vacancy, checks the post-pull state
roots against the real action, and compares ordinary Graph/native state and pending work
through 16 ticks of reset. That last comparison does not execute a layer kernel.

Every original output in this fixture must have either an output root or a retained
lifecycle occurrence. Adding an external payload observer is a separate negative test:
its ObserverWatch/contact-profile outputs still lack a geometry proof, so the whole
candidate declines. It is not accepted by deleting that observation.

This is not yet the complete algorithm above. Connected multi-cell regions, internal
wire-value substitution, shared-resource arbitration and SCC/feedback layer construction
remain unimplemented. They produce whole-component decline witnesses rather than partial
compiled regions. In particular, a `PowerAtCut` witness identifies an actor's original
directional contribution and the sample/acceptance phase where arrival is not established.
No original callbacks, wires, reset clocks or scheduled work are removed.

The fresh 22-fixture census accounts for every cell and corridor exactly once, either
in a compiled layer artifact or in a whole-component decline. Representative results:

| Input | Admitted cells | Admitted / total actors | Corridors | Compiled layer actors |
| --- | ---: | ---: | ---: | ---: |
| ADDER1BIT | 6 | 13 / 13 | 4 | 0 |
| ADDER11BIT | 65 | 142 / 142 | 15 | 0 |
| `counter_basic` IO fixture | 74 | 106 / 163 | 4 | 0 |
| Private direct-cell regression | 1 | 1 / 1 | 0 | 1 |

Each of the first three inputs forms one declined connected component. Their first
unproved request-sample contributions are respectively `power_link:114` at `piston:0`,
`power_link:1071` at `piston:0`, and `power_link:2798` at `piston:16`.
These are original graph references, not schematic-name recognition rules.

Execution remains **0% new instant kernels, 100% existing Graph, 0% native fallback**
for this experimental pipeline, including the positive direct-cell artifact. This is
execution ownership, not sampled CPU time. The 22 schematic fixtures all still have
zero compiled layer actors; the direct-cell regression is a separate constructed input.

## Why the adder gate remains open

Step 03's complete connectivity is not a temporal certificate. The native regression
`fixed_point_reordered_adder_requests_change_the_sum_payload` was rerun and reproduced
the known real-output difference. Actor 3 can queue an unpowered request and become
powered before acceptance; reversing the earlier actor 6/12 requests changes the result.

Neither an OR of eventual source values nor a graph traversal rank establishes the
power seen at those two cuts. The missing next implementation is a source-derived
electrical arrival/activation certificate across the relevant corridor, followed by
ordered cell acceptance and local shared-owner writes. It must discharge that obligation
without importing arbitrary internal wire strengths as kernel inputs or restoring the
removed symbolic Turbo interpreter. The existing stable-external-input contract is kept;
no new order-independence assumption has been introduced.

## Verification scope

The scalar Boolean-builder tests use synthetic, already-qualified cell/read bindings;
they do not establish real parser admission. They sweep
all 16 attenuation values, 16 sampled strengths, 16 acceptance strengths, both readiness
and extended bits at both cuts, and all four two-event combinations (262,144 valuations). Separate
tests check live-power cancellation and callback phase/multiplicity preservation.

Fixture tests exercise the experimental builder, deterministic ownership/accounting,
stale-plan rejection, missing prerequisites, cancellation, and original graph execution
coverage. Their exports are written to `target/layered-instant-layers/`; compiled actor
counts must be read separately from Step 03 candidate actor counts. A declined region
is not compiler success or runtime acceleration. Final commands/results are recorded
below.

The support walk visits shared cell pins once per use and checks cancellation and a
work budget, so asking for a decline witness does not enumerate exponentially many
paths through a shared Boolean DAG. Partition unions use one endpoint representative;
shared outside fanout cannot create a Cartesian join or merge unrelated owners.

### Integrated run, 2026-10-18

- `cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1`:
  **450 passed, 19 ignored**, including the native adder-order counterexample, both
  adders' existing boundary/handoff checks, all 22 layer-accounting fixtures, both
  real private-cell admission tests and the 15 focused layer/partition unit tests.
  Log: `target/instant-layers-final-debug.log`.
- `cargo test -p mchprs_core --lib redpiler::analysis::instant:: --locked -- --test-threads=1`:
  **15 passed**. Log: `target/instant-layers-unit-tests.log`.
- `cargo check -p mchprs_core --tests --benches --locked` and
  `cargo check -p mroww --locked`: passed. The concurrent fixed-classic work still
  emits two dead-code warnings; this batch did not edit those sources.
- Python loader: **22 passed**; viewer: **28 passed**. Both external validators also
  accepted all **23** generated layer exports under their default limits, including
  the real positive artifact. No manual browser-layout inspection was performed.
- Focused Rust formatting and `git diff --check`: passed.

The first broad run found four new synthetic tests whose helper referenced missing
restoration route 9. Its actual test graph uses route 3; that fixture reference was
fixed, the focused tests rerun, and the full suite above rerun successfully. The
original real-cell success test was not weakened into a report-only census.

Not run: release/full-workspace suites, full CPU sorting, new broad fuzz campaigns or
runtime benchmarks. Runtime replacement, multi-cell acceptance and the ADDER1BIT
causal-layer gate remain unimplemented; this is not a completed Step 04 or speedup.

### Divider, COUNTER and revised PC COUNTER check, 2026-10-18

The opt-in `research/layered_devices.rs` tests now prepare these three saved schematics
through `experimental_layer_pipeline` at O3 with stable instant inputs. They validate
lowering, account for every actor once, and account for every admitted cell/corridor
once across compiled and declined components. The real loader verifies fixture hashes;
schematics, signs and block entities are preserved.

| Device | Admitted / total actors | Cells | Corridors | Typed boundary ports | Compiled layer actors |
| --- | ---: | ---: | ---: | ---: | ---: |
| Divider (`fpu_divider`) | 1,099 / 2,055 | 1,009 | 527 | 307,271 | 0 |
| COUNTER (`counter_basic`) | 106 / 163 | 74 | 4 | 19,574 | 0 |
| Revised PC COUNTER | 307 / 500 | 243 | 200 | 79,551 | 0 |

These are actor-admission counts, not external bit counts or executed-kernel coverage.
Unadmitted actors retain their explicit cell-admission decisions; this check does not
establish that all of them are ordinary memory or that none are missed instants.

Each admitted network forms one connected component, and each component declines with
`SignalArrivalUnproven`. The builder still rejects multi-cell/corridor components before
constructing their layers. This is an implementation limitation, not evidence that the
devices are non-instant or that every actor fails an individual proof. First original
request-sample witnesses are:

| Device | Actor / world position | Original power link | Source world position |
| --- | --- | ---: | --- |
| Divider | `piston:28` / `(58,33,44)` | 18,388 | `(58,33,42)` |
| COUNTER | `piston:16` / `(54,34,44)` | 2,798 | `(52,34,44)` |
| PC COUNTER | `piston:0` / `(57,34,42)` | 6,863 | `(58,34,42)` |

All three run **0% new instant kernels, 100% existing Graph, 0% native fallback**.
The interpreter is a separate oracle. No internal circuit is removed by these cold plans.

Correctness checks passed:

- Divider: all 11 manifest histories, **2,172 Graph/interpreter ticks**, including held
  falling activation, reactivation, operand preparations and the zero-divisor history.
  Declared observations, clock, phase and ordered pending ticks match after every tick;
  full in-bounds blocks/entities match after every manifest operation. This verifies
  interpreter equivalence, not an independently specified arithmetic encoding.
- COUNTER: both saved-idle and release histories, **126 Graph/interpreter ticks**, with
  the same observation, full-state and pending-work checks.
- PC COUNTER through the new pipeline: one warm-up plus **128 count ticks**, with all
  23 controls enabled in native source order. Every 16-bit output word matches both the
  interpreter and the pinned reference. Full memory/geometry/entity checkpoints and
  per-tick clock, phase and ordered pending work match.
- Each Divider/COUNTER history and the PC episode also passes **16 interpreted ticks
  after handoff**, with future-work checks each tick and a full final-state comparison.
- The existing general-Graph PC suite also passes: 23 separate rising/falling control
  histories (**736 ticks**), two stable 16-bit data patterns crossed with seven controls
  (**672 ticks**), the 128 frozen output windows and the multiple-interface catalog.
  This additional suite uses the existing public/general pipeline, not an executing
  experimental layer kernel.

Two initial failures were test-harness mistakes, corrected before the passing run:
Divider's `diagnose` marker is a read-only checkpoint rather than a lever operation;
and handoff must replace already materialized inspection ticks/motions before reset,
otherwise the harness imports Graph's future twice. Runtime code was unchanged.

Commands and artifacts:

```text
cargo test -p mchprs_core --lib layered_devices:: --locked -- --ignored --nocapture --test-threads=1
# 3 passed; target/layered-devices-check.log
cargo test -p mchprs_core --lib pc_counter_contract:: --locked -- --nocapture --test-threads=1
# 4 passed, 1 ignored; target/pc-counter-contract-check.log
```

Machine-readable census/correctness summaries: `target/layered-instant-devices/`
(`divider.json`, `counter.json`, `pc_counter.json`). These are compact research reports,
not graph-schema exports. Focused Rust formatting and `git diff --check` passed.

Not run in this check: release/full-workspace suites, the 4,096-tick PC reference,
new fuzz campaigns or runtime benchmarks. Passing general Graph replay does not prove
layer-kernel correctness or acceleration. Next compiler work remains source-derived
electrical arrival across corridors and ordered multi-cell/shared-resource acceptance.
