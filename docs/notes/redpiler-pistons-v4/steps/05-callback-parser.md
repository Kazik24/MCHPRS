# Step 05: Ordered neighbor and shape routes

Status: implemented; awaiting human review. [Step 04](./04-electrical-parser.md) is reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Compile the actual procedures that deliver callbacks, including future geometry and unchanged-value delivery.

## Read these sources

- [notification procedures](../../../../crates/core/src/redstone/mod.rs), [piston shape/notify](../../../../crates/core/src/redstone/piston.rs), and [shape/support](../../../../crates/core/src/interaction.rs).
- [repeater](../../../../crates/core/src/redstone/repeater.rs), [comparator](../../../../crates/core/src/redstone/comparator.rs), and [notification-route tests](../../../../crates/core/src/redpiler/analysis/tests/research/notification_routes.rs).
- [redstone model](../../../REDSTONE_MODEL.md), sections 4, 12, and 13.

## Scope and implementation

Read-only ordered route construction and correspondence with graph callback edges. Runtime scheduling is Step 06.

1. Compile source-specific route templates for controls, torches, diodes, observers, wire shape, and piston mutation/restoration. Reuse existing position-only route generators during compilation.
2. Retain Update versus Shape, literal direction/None, duplicate deliveries, and source procedure/sequence position. Do not sort targets or deduplicate event routes.
3. Include prospective head contacts and head-to-base forwarding without moving-block nodes. Select eligibility at delivery, using live state for ordinary routes and captured state where Turbo supplies it.
4. Preserve diagonal observer/piston exclusions, piston shape order versus power order, and repeater/comparator first-direction differences.
5. Associate observers with watched site/contact IDs, including switched contacts and dust. A generic state write is not an observer trigger.
6. Validate every potential target/guard binding. Electrical distance cutoffs do not remove independent update routes.

## Acceptance checks

Compare native route recipients/directions/order for representative orientations, same-value comparator notification, prospective head forwarding, shape-triggered observer versus excluded indirect power delivery, and repeated recipients.

Verify new tests are registered and actually run. Use the existing targeted release/locked test workflow described in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show power and update dependencies of the minimal separated-QC/update mechanism. Demonstrate that route presence includes ordered activation semantics and is not reduced to a value-change edge.

Ordered route tables and their typed graph relations are complete for admitted geometry, with explicit delivery-time operand rules.

## Leave out

No simulated tick loop, memory inference, callback deduplication, observer old/new-ID detector, or blanket 'all neighbors update' helper.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 06](./06-scheduler-and-components.md) until the user reviews this result.
