# G7 — Restore safe compact electrical execution inside Graph

Status: proposed; follows activation/parity of the affected cohort. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md), Side B research and `15-dust-optimization.md`. Use the G3 API for every transformation.

## Goal

Recover useful old Direct performance with reviewed graph rewrites and specialized instructions inside GraphBackend. There is one semantic executor; compact instructions must preserve the same event contract. Simple circuits must not allocate per-node piston/resource data that their program does not use.

## Scope

1. Audit old clamp, dedup, constant fold/coalesce, duplicate logic, unreachable-output and orphan passes against typed power/update/shape semantics. Publish a legality table before porting them.
2. Start with existing proven local simplification opportunities. Do not apply electrical attenuation cutoffs to Update/Shape deliveries: a zero-power path may still deliver a meaningful callback.
3. Preserve ordered callback multiplicity, live/captured guards, imported initial state, pending work, observer visibility and projection when folding/coalescing records. Removed IDs require explicit reference remapping and a documented projection disposition.
4. Add dust elimination only for regions whose intermediate wire/query/callback behavior is proven unobservable under all admitted histories. Contacts, stale strength, observer watches and feedback are exclusions until the corresponding proof exists.
5. Reuse compact strength/link storage where its invariants hold. Optimize validated representation and node instructions; do not revive world snapshots or an unproved ideal response.
6. Keep full general execution as the default for ineligible regions. If mixed instructions interact, prove boundary scheduling/callback equivalence; do not split one circuit across independent clocks/executors.
7. Implement each pass through the general API. Provide an isolated runner test and at least one enabled/disabled pipeline differential episode; for order-sensitive combinations, test valid orders and rejected preconditions. Do not hardcode experiment selection into the compiler runner.

## Review evidence

- Each migrated rewrite has one counterexample that would fail a naive version and a before/after interpreter differential episode.
- Graph export/provenance and reset reconstruction agree after rewrites and ID remapping.
- Both optimization flag states preserve observations and future work.
- Paired measurement on representative ordinary circuits reports compile cost, nodes/links, memory and execution time. Reuse existing benchmark support; no new benchmark framework.
- Clearly report which old optimizations remain disabled and why. Initial generalized execution may be slower; do not imply old speed is preserved without measurements.

## Non-goals

No instant/BUD specialization, multithreading, new physical domain, or forced packing of every rich relation. Those remain separate projects after general correctness.
