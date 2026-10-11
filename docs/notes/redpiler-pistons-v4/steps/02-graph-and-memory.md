# Step 02: Graph vocabulary and memory ownership

Status: not started. [Step 01](./01-reference-contract.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Introduce the minimum typed graph/state vocabulary and validate its ownership before parsing or simulating pistons.

## Read these sources

- [architecture](../ARCHITECTURE.md), especially layers, memory ownership, and struct shapes.
- [compile_graph](../../../../crates/core/src/redpiler/compile_graph.rs), [backend selection](../../../../crates/core/src/redpiler/backend/mod.rs), and [direct packed nodes](../../../../crates/core/src/redpiler/backend/direct/node.rs).
- [pass pipeline](../../../../crates/core/src/redpiler/passes/mod.rs), [deduplication](../../../../crates/core/src/redpiler/passes/dedup_links.rs), [clamping](../../../../crates/core/src/redpiler/passes/clamp_weights.rs), and [binary export](../../../../crates/core/src/redpiler/passes/export_graph.rs).

## Scope and implementation

Construction types, lowering validation, and compatibility of existing ordinary callers. Do not activate a rich backend.

1. Add Piston/Contact construction nodes and distinct Power, Update, and Shape links. Preserve convenient ordinary Default/Side construction and make their meaning explicit.
2. Establish program, circuit-state, and transient-walk ownership. Use NodeIds for required circuit nodes and contacts, with immutable SiteIds for ordered dust context. PistonId/ResourceId/ContactId index their description/state tables; no Motion identity domain is introduced.
3. Define contact alternatives, compact piston state, shared resource selection, routes, and staging tables required by the next parser step. Avoid empty executor frameworks and unused future specialization variants.
4. Search all graph node/link matches and direct/legacy export consumers. Old rewrites and packed lowering must only accept their electrical contract. Rich export must fail clearly rather than serialize truncated data.
5. Validate node/edge/table references, strengths, kind/state correspondence, range bounds, and guard acyclicity before dense lowering. Use checked indexing initially.

## Acceptance checks

Keep ordinary graph/backend tests passing. Add a small synthetic mixed-link graph proving callbacks survive electrical-only clamping/deduplication or are explicitly excluded. Check invalid references/strengths fail before publication.

Verify new tests are registered and actually run. Use the existing targeted release/locked test workflow described in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review exact Rust fields and invariants. Demonstrate that shared resource selection and wire strength each have one authority, and that callback order is not inferred from edge iteration.

The new vocabulary is readable and validated, and existing ordinary compilation/export behavior is preserved.

## Leave out

No new dependencies, generic VM, packed bit expansion, Motion/progress model, mutable world-cell state, optimization semantics, world adapter, or active piston compilation.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 03](./03-geometry-and-payload-parser.md) until the user reviews this result.
