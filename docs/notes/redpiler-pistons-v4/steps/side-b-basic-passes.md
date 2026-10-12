# Side B: Basic pass research and first safe transformations

Status: research scoped, 2026-10-12. No rewrite is authorized by this document until its small implementation slice is reviewed.

## Agent assignment

Read [AGENT_GUIDE](../AGENT_GUIDE.md), [ARCHITECTURE](../ARCHITECTURE.md), Step 15, current `program_graph.rs`, `analysis/electrical.rs`, and every existing pass before proposing reuse. Keep ongoing Step 05 work untouched. Report concrete legality conditions and examples, not a list of generic optimizer names.

The first graph must represent the circuit correctly. Research can identify immutable-description simplifications now; all optimization implementation waits for executable differential parity through Steps 06-10. Preserve analog power, QC sampling, callbacks including duplicates/order, observers, requests/acceptance/cancellation, pending work, resource identity, and exact continuation. No motion or interpreter fallback.

## Findings from the current sources

The existing pipeline identifies nodes, searches links, clamps weights, then optionally deduplicates links, folds constants, prunes unreachable comparator outputs, coalesces constants/duplicate logic, and prunes orphans. It operates on legacy `CompileGraph`, whose links cannot express the richer callback/contact/query model. Do not run those passes on a lossy conversion of the new graph.

The richer graph currently has validation in `CircuitDraft`, explicit guards, contact selectors, separate dust query channels, ordered routes, shared resource state, and explicit wire plans. Its electrical draft is still a preparation plan, not an executable graph. These are the boundaries the research must respect.

## B1: Pass inventory and legality table

Intermediate goal: document which small passes are useful, when they may run, and exactly what they may touch.

| Candidate | First useful scope | Required restriction / witness |
| --- | --- | --- |
| Validate references and state ownership | Every preparation/rewrite boundary | Keep format/stage validity separate from successful execution admission. |
| Simplify guard expressions | Immutable preparation | Fold only literal Always/Not/All/Any identities; never freeze state/site predicates using the initial pose. Preserve operand views. |
| Intern identical profiles and guards | Immutable preparation | Compare complete semantic records; remap every reference and provenance. Do not merge logical resources or stateful nodes. |
| Remove duplicate power dependencies | After channel plans assemble | Only identical max-input dependencies with identical full source/target/channel/guard/face/attenuation. Never deduplicate route deliveries, Update, or Shape records. |
| Remove branches proven statically false | After guard simplification | Literal impossibility only initially; a disabled resource or unpowered input is not unreachable. |
| Index incoming links / route tables | Program lowering | Representation improvement, preserve route order and duplicate deliveries. No power solver or behavioral rewrite. |
| Prune unreachable behavior | Later, after executable parity | Reachability includes callback, shape, guard/site, observer, scheduler, host output, resource, and reset dependencies—not only power links. |
| Fold electrical constants / coalesce logic | Later, after executable parity | Prove there are no discarded callbacks, state, sampling, scheduled work, analog boundary, or continuation effects. A constant power value does not imply an inert node. |
| Eliminate dust | Step 15 | Preserve ordered wire walks, stored strengths/shapes, live/captured observations, nested walks, observers, and boundaries. |
| Piston/reset, BUD, instant grouping | Subsequent research | Require replayable certificates for arbitrary admitted input histories; no inference from one pose, missing edge, proximity, or actor count. |

Compare each candidate with actual legacy implementations and record reuse as: reusable helper, reusable rule after proof, or incompatible. Provide one counterexample per unsafe legacy rule. Identify preparation CPU/memory costs through measurement before proposing more passes.

## B2: First implementation slice, subject to review

Start implementation only after Steps 06-10 establish executable differential parity and continuation. Step 05 approval alone is insufficient; B1 can proceed as research now.

Recommend only local guard simplification plus exact profile/guard interning if measured duplication warrants it. Keep these in one readable preparation operation or small pass module; do not create a plugin/pass framework. Return an explicit old-to-new ID map when records change. Keep source provenance and report before/after counts. Validate after transformation.

Tests must enumerate logical piston/resource states for a small guarded-contact example and compare selected profiles/receivers plus power eligibility before/after. Include live/captured site predicates, both conductor dust policies, normal/QC inputs, and a duplicate callback route that remains unchanged. Exercise dangling references introduced by an incorrect remap. Run the transformation twice and require stable semantic output.

Exact power-link deduplication is a separate next slice only when assembly establishes max semantics for those links; review it before implementation. Do not merge conductor query policies or stored/directional wire channels. Do not sort callback procedures as a side effect of sorting graph records.

## Review deliverable

Save the completed research and proposed minimal pass sequence alongside this step. Include source references, legality/preconditions, counterexamples, expected benefit, and the concrete next implementation prompt. Use Side A exports for before/after inspection when available, but do not make basic correctness checks depend on the viewer.

Do not claim a speedup without paired measurements on identical fixtures and inputs. Correct graph behavior is the gate; export size alone is not execution performance. Runtime-sensitive passes wait for Steps 06-10 and the reference comparison harness.
