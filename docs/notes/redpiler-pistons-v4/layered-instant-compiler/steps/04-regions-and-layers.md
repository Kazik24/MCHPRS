# Step 04: regions, gate connections and guarded causal layers

Status: planned; requires approved boundary catalog. Runtime remains disabled.

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
