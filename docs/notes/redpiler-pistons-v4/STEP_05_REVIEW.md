# Step 05 review gate: complete parser audit

Status: checklist prepared, 2026-10-12. Run after Step 05 is implemented, before Step 06. This is a review/test assignment, not authorization to implement execution or optimization.

## Goal and limits

Review Steps 02-05 together: geometry, electrical discovery, callback construction, and assembly into a validated graph. Establish that the parser represents every admitted alternative and the interpreter's query/delivery procedures. A successful compile or fixture inventory is not execution parity; graph-versus-interpreter runtime comparison starts with Step 06.

Read [AGENT_GUIDE](AGENT_GUIDE.md), [ARCHITECTURE](ARCHITECTURE.md), Steps 03-05, `analysis/geometry.rs`, `analysis/electrical.rs`, the Step 05 route/assembly modules, `program_graph.rs`, and their callers/tests. Read connected interpreter power, wire, piston, observer, repeater, comparator, and support procedures. Preserve concurrent/user changes. Review first; report concrete findings separately from fixes.

## 1. Architecture and ownership

- [ ] One immutable program owns topology/query plans; mutable component, piston, and resource state have explicit single owners.
- [ ] Each shared payload has one resource identity and authoritative selected contact. Actor records do not duplicate payload state.
- [ ] Runtime plans require no physical Motion, progress, carried-block model, mutable world grid, spatial discovery, or native redstone fallback.
- [ ] Normal/QC power, Update/Shape delivery, sampled piston intent, and future scheduled work remain separate concepts.
- [ ] Parser functions have clear responsibilities and structured errors; complex decisions are broken into named steps with readable witnesses.
- [ ] New fields and variants have the requested short explanations. No undocumented placeholder handler or TODO claims complete support.
- [ ] Legacy passes/export/backend cannot silently discard the richer graph vocabulary. Public piston activation remains unavailable before Step 10.

## 2. Geometry and admission

- [ ] Stationary entry validation rejects active motion, pending piston requests/work, partial phases, malformed/missing/foreign heads, and missing motion metadata.
- [ ] Ordinary scheduled ticks, imported dust strengths/shapes, and BUD power/pose disagreement are preserved without normalizing the world.
- [ ] Resource contacts close under supported future transfers, detach/drop/recapture, and both actor orders—not just the saved pose.
- [ ] An initially empty ordinary lane receiving a foreign resource is rejected if it could successfully push. A permanently container-blocked push remains legal; a changing head is not treated as a permanent blocker.
- [ ] Unsupported successful longer pushes, transported observers/electrical components/pistons/containers, footprint escape, and unloaded occupancy produce specific witnesses.
- [ ] Fixed Far context for an empty ordinary piston remains intact, including an observer or foreign base.
- [ ] Base profiles include both stationary poses and required unavailable alternatives; prospective heads/resources include all admitted logical alternatives.
- [ ] Support checks cover transient connection states and both poses. Every support rejection explains actor, changed contact, and dependent block.
- [ ] Preparation is bounded/cancellable across the complete pipeline; limits do not stop at initial inventory. Unsupported/incomplete preparation cannot return a certified complete program.

## 3. Electrical query correspondence

- [ ] Weak/strong emission directions match interpreter rules, including redstone block weak 15/strong 0, torches, controls, diodes, and observers.
- [ ] Solid conduction reads adjacent primitive strong inputs once; it never chains conductor charge.
- [ ] Dust-inclusive and dust-excluding conductor channels stay distinct through assembly and lowering. Dust cannot sustain itself through an unattenuated conductor loop.
- [ ] Piston front exclusion and world-up QC geometry are correct for all six orientations. QC changes affect sampling without inventing a callback.
- [ ] Contact selection and availability guard both resource emission and conductor reads; disabled resources supply no stationary material power.
- [ ] Diode rear fallback and comparator side dust inputs use stored strength; ordinary directional emission remains directional.
- [ ] Comparator side eligibility is discovered across contact alternatives, including a redstone resource entering an initially empty side.
- [ ] Direct/far comparator override precedence and threshold 15 match the interpreter. Fixed analog values remain literals; bulb/command overrides reference live state.
- [ ] Dynamic overrides outside the compiled inventory return structured direct/far boundary errors, never panic or become literals.
- [ ] Explicit wires retain canonical 24-site context, directed step/ascent/descent guards, analog strengths, attenuation, and imported shape.

## 4. Ordered callback correspondence

- [ ] Compare actual interpreter route generators/procedures with parsed deliveries; expected results are not generated by calling the parser under test.
- [ ] Compare the complete ordered list: target position/site, Update versus Shape, literal direction or None, eligibility guard, and live/captured operand view where applicable.
- [ ] Duplicate recipients/deliveries remain duplicated. Edge grouping, sorting, or lowering does not define callback order.
- [ ] Cover controls, floor/wall torches, repeaters, comparators, observers, wire shape procedures, and piston mutation/restoration routes in representative orientations.
- [ ] Same-value delivery is retained where the interpreter delivers it; callbacks are not conditional only on changed power.
- [ ] Prospective head contacts and head-to-base forwarding are present even when entry is retracted.
- [ ] Piston Shape order differs correctly from power callback order; diode initial direction and subsequent direction changes remain literal.
- [ ] Diagonal/indirect observer and piston exclusions match interpreter procedures rather than a blanket neighbor rule.
- [ ] Stationary observers can watch bases, heads, resources, dust, and other changing contacts. Generic state writes are not substituted for Shape-triggered behavior.
- [ ] Delivery resolves receivers/eligibility at the correct boundary. Unavailable head/base states and `PublishingHeadRemoval` are not flattened into Ready or Air incorrectly.
- [ ] Independent update routes survive even when there is no electrical path. Electrical distance or constant-power reasoning does not prune them.
- [ ] If captured Turbo operands are deferred to Step 07, the route representation supports them and the missing wire execution is explicitly reported; do not claim Turbo parity from static route tests.

## 5. Assembly and validation

- [ ] Validate every node/site/profile/guard/contact/resource/piston/wire/route reference, including comparator plans and conductor targets that are not ordinary node-to-node edges.
- [ ] Remapping preserves geometry IDs and newly appended electrical IDs, or provides a complete explicit mapping. No profile/guard/site reference refers to the wrong table.
- [ ] Contact guards cannot recursively depend on their own site selection; guard cycles and invalid selectors are rejected.
- [ ] Contact alternatives select the intended profile/receiver across small enumerated logical states. Diagnose ambiguous or missing selections in reachable examples.
- [ ] State counts/kinds, resource initial contacts, strength ranges, and canonical wire-neighbor ordering are checked before program publication.
- [ ] Route ownership, output-route bindings, and piston mutation/restoration bindings are complete for claimed supported sources.
- [ ] Ordinary pending ticks are retained for the upcoming scheduler; do not claim complete continuation export until scheduler/frontier state exists.
- [ ] Repeated preparation yields deterministic semantic output without mutation. A late assembly failure leaves no active backend or partial published result.
- [ ] Distinguish complete graph validation from public execution admission. Verify existing compiler rejection still preserves the interpreter world.

## 6. Required small witnesses

Build or reuse focused fixtures and report exact expected/actual facts:

1. Empty ordinary actuator with a fixed Far observer: contact alternatives and ordered mutation/restoration routes.
2. Independent BUD data/update inputs: separate QC dependency and actual update route; data change alone creates no delivery.
3. Sticky redstone resource and inert conductor: selected, disabled, and relocated contact branches.
4. Shared OR: one resource, all allowed contacts/actors, both action-order possibilities represented.
5. Perpendicular resource arrival at an ordinary lane: successful push rejection, permanently blocked admission, changing-head rejection.
6. Imported shaped dust beside diode/comparator: stored strength versus directional emission, with conductor dust policies separated.
7. Direct/far comparator overrides: literals, live bulb/command outputs, and external-source rejection.
8. Ordered duplicate/same-value callbacks, prospective head forwarding, observer Shape trigger, and indirect exclusion.
9. Cancellation/budget exhaustion and late validation failure: blocks, relevant entities, scheduler entries, and piston work remain unchanged.

Enumerate a small set of legal logical states for guarded examples and independently evaluate interpreter query facts where those states can be materialized as stationary worlds. Intermediate unavailable states need explicit native reference episodes or source-backed witnesses; do not invent unreachable Cartesian state combinations as parity evidence.

## 7. Real circuit compilation matrix

Use existing loaders and frozen manifests/hashes. Record stage reached, counts, errors, and the exact artifact/trace when available. Do not change fixtures to obtain admission.

| Fixture group | Required evidence at this gate |
| --- | --- |
| Minimal supported TEST examples | Full preparation/assembly succeeds, with meaningful controls and callback plans inspected. |
| Shared OR and minimal QC/BUD examples | Shared contacts and independent data/update dependencies are demonstrated. |
| ADDER1BIT | Complete graph construction, shared resources, inhibition/cancellation-related route coverage. |
| ADDER_11BIT / repository's actual filename | Parser robustness smoke check; counts, bounded preparation, deterministic result. |
| Divider, ANPU, Counter, FPU | Exploratory preparation attempts and recorded rejection witnesses; failures become coverage work for their assigned later milestones, not invented support claims. |
| Unsupported TEST_WEIRD cases | Explicit scope/rejection reason; exclusions are documented rather than silent skips. |

For each fixture distinguish full assembled graph, geometry-only inventory, structured rejection, and not attempted. Do not claim arithmetic correctness or acceptance cancellation merely because parsing succeeded.

## 8. Checks and evidence

- [ ] Discover tests before running them; record actual filters and nonzero counts.
- [ ] Rerun Step 01 reference contract, Step 02 graph validation, geometry, electrical, and Step 05 route/assembly tests against the same final checkout.
- [ ] Run applicable existing redstone/observer/piston regressions plus legacy compiler/backend tests affected by shared helpers.
- [ ] Run `cargo check -p mchprs_core --tests --locked`, `cargo fmt --all -- --check`, and `git diff --check`.
- [ ] Consider the broader core suite once targeted checks pass; record whether it ran, failed, or was skipped and why. Do not claim it passed from targeted tests.
- [ ] Record preparation duration and graph sizes for larger fixtures as diagnostics, without imposing an invented speed target or claiming execution acceleration.
- [ ] If Side A is available, export representative drafts/programs and inspect them; review must still be possible without the viewer.

## Verdict and handoff

Report a short architecture verdict, numbered concrete findings with witnesses, executed checks, the fixture matrix, and known deferred behavior. Approve Step 06 only when supported assembly and route semantics have no blocking findings. Defer actual runtime differential claims to Steps 06-09, and compile/reset continuation claims to Step 10.

No source fixes or extra milestones are implied by this checklist. If review finds a defect, scope the smallest correction and its regression before proceeding.
