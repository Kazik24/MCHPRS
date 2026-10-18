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

`analysis/instant/layers.rs` currently supports one actor without a shared payload or
internal wire corridor. Its boundary events remain separate from positive-after-guard,
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

This is not yet the complete algorithm above. Connected multi-cell regions, internal
wire-value substitution, shared-resource arbitration and SCC/feedback layer construction
remain unimplemented. They produce whole-component decline witnesses rather than partial
compiled regions. In particular, a `PowerAtCut` witness identifies an actor's original
directional contribution and the sample/acceptance phase where arrival is not established.
No original callbacks, wires, reset clocks or scheduled work are removed.

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

The scalar compiler tests use synthetic, already-qualified cell/read contracts; they
do not bypass parser admission and then claim a real schematic compiled. They sweep
all 16 attenuation values, 16 sampled strengths, 16 acceptance strengths, both readiness
and extended bits at both cuts, and all four two-event combinations (262,144 valuations). Separate
tests check live-power cancellation and callback phase/multiplicity preservation.

Fixture tests exercise the experimental builder, deterministic ownership/accounting,
stale-plan rejection, missing prerequisites, cancellation, and original graph execution
coverage. Their exports are written to `target/layered-instant-layers/`; compiled actor
counts must be read separately from Step 03 candidate actor counts. A declined region
is not compiler success or runtime acceleration. Final commands/results are recorded
below after the integrated run.
