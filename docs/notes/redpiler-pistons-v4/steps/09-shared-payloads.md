# Step 09: Shared graph resources and contact recapture

Status: not started. [Step 08](./08-piston-transitions.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Handle shared blocks as conserved graph resources selected among certified contacts, including release and recapture.

## Read these sources

- [shared OR/inhibition tests](../../../../crates/core/src/redstone/instant_piston_tests.rs), [IO/QC checks](../../../../crates/core/src/redstone/instant_piston_tests/io.rs), and [reference piston rules](../../../../crates/core/src/redstone/piston.rs).
- [replacement/continuation tests](../../../../crates/core/src/redstone/piston/tests.rs), [piston model](../../../PISTON_MODEL.md), and architecture sections 6-8 and 14.
- [contact discovery step](./03-geometry-and-payload-parser.md).

## Scope and implementation

Extend isolated graph operations to shared contact groups. Strengthen parser closure or logical distinctions only when concrete traces require it. Shared state is not a miniature moving-block world.

1. Resolve a lane's live selected resource at sampling and acceptance. ResourceState.at/enabled is the only location/availability authority. Actors do not retain private payload ownership.
2. Allow temporary detachment, early release, and recapture among the resource's admitted contacts. An actor temporarily without a resource is valid when the group conserves it.
3. Preflight live contact conflicts before writes. Apply the reference's connection/notification order and preserve fixed foreign bases/observers. Two resources cannot claim the same exclusive contact.
4. After each complete semantic change check conserved token count, allowed contact membership, unambiguous contact variants, and legal availability. Disabled resources remain conserved; they do not need a carried-block container.
5. Use independent revisions for resources and actors. Reversing one actor must not cancel another valid resource event, and an old restoration event must not enable a recaptured/replaced resource. Test transfer to another actor while the original actor still has a valid base/head publication event.
6. Certify closure for all admitted successful local transfers and arbitrary legal control histories, including possible longer push lines and support-sensitive dust. Use conservative local alternatives and structured rejection witnesses, not global pose products.
7. Check abstraction sufficiency: if two histories produce the same graph state but different future output/update behavior, isolate the missing distinction. Propose the smallest logical correction or reject that mechanic; never add physical progress by default.
8. Do not use a schematic name, actor count, or permanent ownership assumption as an admission rule. If SingleResource lanes cannot cover a requested target, present the concrete unsupported transition before widening the model.

## Acceptance checks

Run shared OR with both actor orders, overlapping held controls, early reversal/release/recapture, repeated triggers, and supported three-actor sharing. Include obsolete delayed-event cancellation and independent base/resource restoration, adder inhibition episodes, and a negative closure/support witness. Compare channels, routes, timing, and pending futures.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one shared resource's contact table and two legal histories selecting different contacts. Show an obsolete event being ignored by revision and a concrete unsupported-transition diagnostic.

Shared resource selection is conserved and graph-native, with explicit closure and abstraction arguments for admitted histories.

## Leave out

No Motion records, private copies, mutable spatial occupancy grid, global Cartesian pose enumeration, SAT/BDD framework, or admission from stationary inventory alone.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 10](./10-activation-and-handoff.md) until the user reviews this result.
