# Step 01: establish the wave contract and independent oracle

Status: expanded corpus contract gate implemented and tested; awaiting human review.
Read [architecture](../ARCHITECTURE.md)
and [migration](../MIGRATION.md).
Implement this step only, then stop for human review. No compiler/pipeline activation.

## Goal

Establish exactly what one atomic input frame, one computation use, one reset and one safe
observation mean for the actual fixtures. The single-use rule must be evidence-backed
before it becomes the algorithm's termination argument.

## Work

1. Reuse `backend/graph/tests/adder.rs`, current manifest loaders and native reference-contract
   recorders. Do not create another schematic loader or gate simulator as the oracle.
2. Add `backend/graph/tests/instant_contract.rs`, registered from the existing graph test tree.
   Represent a frame with old/new boundary data, the qualifying input event(s), and their
   declared order/provenance. Explain every field. Keep the adapter test-only initially.
3. For real adder cases, use physical data preparation followed by the fixture activation
   edge. Sample all data at that edge. Keep sequential preparation events visible to both
   executors; do not label them a simultaneous atomic frame merely because no tick elapsed.
4. Add an explicit synthetic multi-source frame protocol if needed: apply all source values
   first and release declared notifications afterward in the same way for native and Graph.
   Label this as a new contract-specific input protocol, not ordinary Minecraft interaction.
5. Decode native observations into accepted computation uses, reset/restoration uses and
   boundary effects. Annotate actors by their mechanism role, not by an arbitrary event count.
   A no-change sample or cancelled request does not consume an actor.
6. Census repeat uses, shared transfers, reset-generated responses and observation windows.
   Define the earliest supported local cut for a wave. Do not use a whole-world idle test.
7. Compare the known `prepared-0-1-1` adder case under the declared stimulus protocol. Record
   whether a mismatch concerns source preparation, internal causality, or an actual output.
8. Establish old/new data, output levels/pulses, external BUD memory and scheduled deadlines
   as the comparison projection. Raw internal wires/heads during an admitted wave are not
   mandatory equivalence roots; reconstruct and compare them at the safe cut where promised.

## Fixtures and required checks

- `INSTANT_BLOCKED`: failed requests do not count as computation use.
- `INSTANT_CHAIN`: causal progression is not one game tick per piston.
- `INSTANT_RESET_REDSTONE` and `_2`: distinguish reset from computation and repeated waves.
- Shared OR and AND fixtures: accepted uses, not samples, determine the logical result.
- Independent BUD data/update fixture: data alone cannot write memory.
- ADDER1BIT all pinned cases; census ADDER11BIT without synthesizing it.
- Include a sub-tick repeater pulse and an external observer of a head/contact.

Reuse the `instant-pistons-io/fixtures/*.json` manifests and hashes. Use the equivalent
`test-prefix-20261008` family to expose different geometries; do not change saved schematics.
The historical actor expectations are 13 and 142 for the pinned adders; verify them from
the current loaded fixture rather than accepting an old report as fresh evidence.

## Review artifact

Provide per-fixture wave boundaries, accepted computation counts by actor, reset roles,
boundary observations and the smallest counterexample to any proposed assumption.
Append edge cases to the single central register. A short status note may link the table;
do not create a second counterexample catalog.

## Gate

The input protocol and consumed-use definition are unambiguous and executable in tests.
If a required actor fires twice in one intended wave or has unmodelled retained memory,
report that fact before step 02. Do not erase it or split waves without a justified cut.
No speed result is expected from this step.

## Implementation review notes

The first supported causal cut is an actual `PistonEvents`/`PistonActions` FIFO drain,
not every traversal, callback or change of input. Source review establishes the following
single-retraction invariant for the admitted subset in which bases cannot be moved:

1. An accepted native retraction replaces its base with a moving source block. Graph sets
   `PistonAvailability::BasePending` (possibly through its head-removal subphase).
2. A subsequent request cannot accept another retraction against that unavailable base.
3. Only the subsequent movement/deferred-publication phase restores the base. Its resulting
   requests belong to a later drain and hence a later computation wave.

This is a bound on accepted **retractions**, not all accepted actions. An extension and an
early/drop retraction can occur in one drain. A local cell still needs its own decoder and
action-role proof before either action can be called computation or reset. Census tests
must preserve all three action variants and all phases rather than enforcing this bound
by filtering away inconvenient events.

`backend/graph/tests/instant_contract.rs` now records the declared physical setter order,
old/sampled port values, readiness wait and distinct falling trigger. A setter order is
checked as a permutation over unique data ports. Native traces retain accepted event order
and count samples and all three action kinds separately. No synthetic atomic-input adapter
was needed for these physical fixture protocols.

The two focused debug tests passed on 2026-10-18:

| Fixture | Histories | Actors / recognized reset provenance | Response samples | Action drains | Extend / Retract / early-drop |
| --- | ---: | ---: | ---: | ---: | ---: |
| ADDER1BIT | 48 (8 vectors x 6 setter orders) | 13 / 13 | 33,456 | 336 | 1,008 / 1,008 / 0 |
| ADDER11BIT | 40 pinned arithmetic vectors | 142 / 142 | 194,932 | 320 | 5,288 / 5,288 / 0 |

Every recorded actor obeyed the single-retraction bound in each real action drain. Repeated
reset responses are later waves, not a permanently consumed actor. Arithmetic checks use
the existing payload/repeater windows. The one-bit test separately replays `prepared-0-1-1`
through native and ordinary Graph, checking ordered observations and projected full future.
The eleven-bit census alone is a native reference check, not kernel execution or a new
Graph differential proof. No compiler or speed result follows from these counts.

## Directed ports and the PC-counter gate

These counts are **oracle ports from the pinned manifests**, not results of the future
boundary-identification pass. A physical observation site is not necessarily an independent
logical signal, and a data dependency does not automatically qualify an activation.

| Fixture | Data/control inputs | Ordinary output observations |
| --- | --- | --- |
| ADDER1BIT | Three scalar data inputs A/B/Cin, plus a separate trigger | Sum payload, Sum repeater, Carry repeater: three sites observing two arithmetic results |
| ADDER11BIT | Two 11-bit buses A/B, plus a separate trigger; no external Cin in this manifest | Eleven Sum payloads and eleven Sum repeaters: two observations of the same 11-bit result; no declared Carry output |
| Revised PC counter | Sixteen data controls and seven other physical controls; their directions remain separate | Sixteen active-low output repeaters, plus memory/geometry and pending-work comparisons |

Keep all declared observation bindings: the payload and repeater can expose the same Sum
bit at different times. Calling them one arithmetic result does not authorize merging
their output ports, omitting a repeater pulse or ignoring either observation window.

The revised PC schematic is pinned by SHA-256
`641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7`.
`analysis/tests/research/pc_counter_contract.rs` reuses the existing research loader and
the frozen 4096-tick interpreter count reference. The first executed test passed 128
ticks against native, ordinary Graph O3 and frozen output words, with full selected-cell,
entity and ordered pending-tick comparisons at ticks 1/9/15/22/64/128. It observed 81,721
samples, 2,957 ordinary retractions, 19 early drops and 2,823 extensions during the
response window. Early drops are not stationary pull-computation uses.

That protocol enables all 23 controls. Separate tests also passed 736 ticks of independent
rising/falling control histories and 672 ticks exercising all seven other controls under
two stable 16-bit bus patterns. They compare output every tick, and full memory/geometry,
entities and pending ticks at 138 and 126 checkpoints respectively. The full native
4096-tick frozen output reference passed as well. Do not name controls from the nearest sign alone. The
new classifier currently reports 500 saved actors, 243 stationary candidate cells with
307 actor uses, and 32 declined mixed-ownership groups. These are local first-response
candidates, not accelerated execution or a final partition into 243 instant regions.

The historical six-piston `PC_COUNTER_MINIMAL_FAIL.schem` is absent from this checkout;
only its admission report is present under `scraps/docs/merge-domains`. Its separated
QC power and READA/READB callbacks remain a regression requirement, not a freshly tested
fixture. The current revised PC counter is tested independently of that historical report.

## Corpus coverage policy

Inventory all schematic sources, including graph examples and intentional rejected imports.
Reuse the core-pack, IO-pack, TEST-family and research protocol loaders rather than adding
another schematic loader. Keep three levels separate in the review artifact:

1. Raw import/entry smoke: verifies the file can be loaded and a bounded native episode runs.
2. Declared protocol census: retains ports, action order, action kinds and final observations;
   counts alone do not prove arithmetic, memory or timing correctness.
3. Functional/differential oracle: asserts decoded results and/or Graph versus native state,
   external observations and pending work under the same physical stimulus history.

An empty manifest is an explicit missing protocol, not a passing CPU test. A structured
Graph rejection is reported separately from native execution. Existing TEST-family helpers
add their documented controls and missing floor supports to both worlds; label those worlds
as derived fixtures rather than claiming an unchanged raw schematic replay.

The executed broader coverage and its precise limits are listed in
[FIXTURE_COVERAGE.md](../FIXTURE_COVERAGE.md). All 18 TEST-family derived worlds admitted
and matched ordinary Graph/native replay, including the five WEIRD fixtures. The new
boundary pass must still discover and validate the expected interfaces; these results
are an independent oracle, not evidence that boundary extraction already exists.
