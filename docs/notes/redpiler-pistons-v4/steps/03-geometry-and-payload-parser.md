# Step 03: Stationary geometry and bounded payload parser

Status: implemented; awaiting human review. Step 02 was reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Produce a read-only geometry result describing all supported connection contacts and shared resource identities.

## Read these sources

- [analysis](../../../../crates/core/src/redpiler/analysis/mod.rs) and [candidate graph](../../../../crates/core/src/redpiler/analysis/graph.rs).
- [piston transport](../../../../crates/core/src/redstone/piston.rs), especially payload-line validation and mutation order.
- [support rules](../../../../crates/core/src/interaction.rs), [block facts](../../../../crates/blocks/src/blocks/mod.rs), and [world boundary](../../../../crates/core/src/world/mod.rs).
- [findings](../FINDINGS.md), including shared arithmetic payloads and ANPU stationary Far context.

## Scope and implementation

Dedicated geometry parser, profiles, lanes, footprint closure, shared identity, and structured diagnostics. Public piston activation remains unavailable until Step 10.

1. Split inventory, entry validation, material classification, lane discovery, shared resource resolution, and footprint validation into named operations. Use actual occupied cells and prospective destinations, not historical palette entries.
2. Reject moving cells/motions, partial phase, pending piston events, malformed heads, and missing motion metadata. Preserve ordinary ticks and BUD power/pose disagreement.
3. Admit empty ordinary lanes and supported sticky single-payload lanes. Keep a foreign base or observer at Far as fixed context for an empty ordinary actor. Reject transported observers, electrical components, containers, other pistons, and unknown movable materials.
4. Close allowed contacts under shared transfers/drop/recapture within the lane model. Establish possible contact selections and distinguish legal blocked movement from an unsupported successful longer push. Never treat unknown external occupancy as air.
5. Validate support and query/callback facts during transient connection states, not just stationary poses. Include prospective head contacts and read/delivery context. Extract logical alternatives; do not generate moving-block runtime nodes. Bound and cancel preparation with explicit errors; incomplete work cannot certify admission.
6. Return contact profiles, nodes, shared resources, initial logical state, and source-position witnesses. Read-only staging may handle the current runtime-unavailable marker explicitly; never broadly ignore genuine entry/geometry issues.

## Acceptance checks

Small valid/malformed heads in multiple facings; moving entry; supported and unsupported material; empty ordinary Far context; shared OR/adders inventory; boundary escape; support loss; cancellation/budget failure with full input snapshot unchanged.

Verify new tests are registered and actually run. Use the existing targeted release/locked test workflow described in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Show one parsed empty ordinary lane, one shared payload group, and a rejection naming actor/block/position. Explain why the finite footprint covers successful operations, not merely the saved pose.

Deterministic read-only geometry preparation is complete or fails with a concrete witness; it does not promise runtime parity yet.

## Leave out

No family recognizers, instant/reset certificates, physical movement simulator, mutable cell array, electrical solving, runtime fallback, or radius-based recursive platform ownership.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 04](./04-electrical-parser.md) until the user reviews this result.
