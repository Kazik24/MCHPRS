# Step 04: Guarded electrical connections

Status: implemented; awaiting human review. [Step 03](./03-geometry-and-payload-parser.md) is reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Compile live normal, side, QC, and conductor dependencies for every admitted geometry variant.

## Read these sources

- [power predicates](../../../../crates/core/src/redstone/power.rs), [ordinary input search](../../../../crates/core/src/redpiler/passes/input_search.rs), and [node identification](../../../../crates/core/src/redpiler/passes/identify_nodes.rs).
- [redstone model](../../../REDSTONE_MODEL.md), sections 3, 6, 9, and 10.
- [piston power predicate](../../../../crates/core/src/redstone/piston.rs) and [comparator input](../../../../crates/core/src/redstone/comparator.rs).

## Scope and implementation

Electrical discovery and pure graph query plans/guards. Do not infer callback delivery from these links.

1. Compile directional primitive weak/strong emission and receiver channels. Solid conduction is a maximum of adjacent primitive strong inputs; keep dust-inclusive and dust-excluding reads separate, and do not chain conductor-to-conductor charge.
2. Keep normal and QC piston ports separate, with front exclusion and world-up QC geometry. Both feed the sampled extension predicate; neither causes activation by itself.
3. Make resource emission/conduction conditional on contact selection and availability. A disabled resource supplies no stationary material power; base/head ports retain their own parsed rules. No moving-block runtime profile is needed.
4. Keep explicit dust vertices and stored strengths, including shape-independent diode rear fallback and comparator-side reads. Compile all required directed step/ascent/descent and shape-query context. Do not collapse paths or normalize imported dust, even with -O.
5. Preserve attenuation/max semantics, comparator main/side eligibility, direct/far override precedence, fixed analog values, and live component-backed overrides. Keep static context entities separate from movable material admission.
6. Represent local guards without global pose enumeration. Record source, channel, face, guard, and attenuation provenance. Prepare strong-query and wire context references needed by the executor.

## Acceptance checks

Directional emitters; redstone weak 15/strong 0; one-cube conduction; attenuation at 14/15; normal versus QC with world-up preserved; mobile conductor joining/removing paths; disabled resource zero contribution; comparator override replacing input.

Verify new tests are registered and actually run. Use the existing targeted release/locked test workflow described in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review a small guarded channel calculation across selected/disabled contact alternatives. Demonstrate that changing QC data can change the sampled value without producing any callback.

Electrical links describe every admitted local variant and preserve live query rules; updates remain a separate relation.

## Leave out

No equilibrium-only dust replacement, Boolean-only analog boundary, BUD classification, guard solver dependency, or successful runtime admission.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Do not implement [Step 05](./05-callback-parser.md) until the user reviews this result.
