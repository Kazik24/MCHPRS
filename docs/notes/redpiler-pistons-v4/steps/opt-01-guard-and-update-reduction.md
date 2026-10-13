# Optimization 01 — simplify queries and remove ineffective callbacks

Status: implemented, awaiting human review (2026-10-13). This document remains the scope and evidence checklist for opt-01; no later optimization project is included.

## Goal and first useful result

Reduce runtime guard evaluation and callback dispatch without changing any component transition. Start with literal guard simplification, exact guard sharing, zero/dominated power contributions, and deliveries that are provably impossible or invoke an identity operation. Do not begin by recognizing complete circuits.

The reviewed production CPU slice executes about 2.06 million callbacks and 303,000 piston samples in 100 ticks. Sharing guards reduces storage; deleting proved ineffective deliveries reduces execution. Report these benefits separately. Callback counts are a target, not a CPU-time profile or a promised speedup.

This is supporting work for [dust contraction](opt-02-dust-contraction.md), [actuator specialization](opt-03-piston-specialization.md), and [region contraction](opt-04-instant-region-contraction.md). Those documents do not authorize this pass to remove their mechanisms before certification.

## Read and touch

Read `docs/REDSTONE_MODEL.md`, `docs/PISTON_MODEL.md`, the current migration report, and the agent guide. Inspect:

- `crates/core/src/redpiler/passes/mod.rs`: `GraphPass`, supplied pipeline order, cancellation, validation, and default pipeline.
- `program_graph.rs`: guards, sites, profiles, contact variants, routes, power links, comparator plans, wire relations, validators, and lowering.
- `analysis/electrical.rs` and `analysis/callbacks.rs`: guard construction and callback alternatives.
- `backend/graph.rs`: `guard_matches_using`, `deliver_route`, `update_component`, effective receiver fallback, and wire-specific dispatch.
- `analysis/export_json.rs`: actual post-pipeline artifact and reference serialization.
- Existing callback-route, deep parser, program validation, reference-contract, and Graph tests.

Use paths relative to `crates/core/src/redpiler/` above. Search every caller before changing a shared helper. The current worktree may contain audit fixes; verify their state instead of relying on an older report. Do not edit concurrent work or resurrect legacy modules.

## Contract

The parser always produces the complete graph. The optional pass runs through the existing API; `--optimize` off retains general execution. Unknown candidates retain their original records. A failed proof is not a compilation error.

Preserve live/captured operands, directional and analog channels, both conductor dust policies, ordered callbacks and duplicate occurrences, same-value notifications, pending membership/deadlines, host effects, and reset/inspection. Do not freeze mutable predicates to their entry values. Fix outstanding semantic audit defects in the affected path before using it as the comparison baseline.

This pass does not suppress piston samples because power is unchanged, deduplicate Update/Shape deliveries, alter scheduler timing, or cache live results across callbacks. Test counters and internal explanatory logs are not permission to erase a protected boundary event; any normalization must be explicit in the observation contract.

## Algorithm A — normalize a pure guard DAG

A DAG is a graph of shared expressions with no cycles. Reuse structural validation to check references and cycles, then obtain a topological child-before-parent order. Support forward references; do not assume guard IDs already have that order.

For each old guard, construct a normalized record from already normalized children:

| Rule | Reason |
| --- | --- |
| `All([]) -> true`, `Any([]) -> false` | Existing Boolean evaluation semantics |
| `All(true, x) -> x`, `Any(false, x) -> x` | Boolean identities |
| `All(false, ...) -> false`, `Any(true, ...) -> true` | Boolean absorbing values |
| `Not(true/false)` and `Not(Not(x))` | Exact Boolean equivalence |
| One-child `All`/`Any` -> child | No other operand exists |
| Repeated identical child IDs -> one occurrence | Guard evaluation has no simulation effects |
| Identical complete normalized record -> shared ID | Same predicate, including IDs, view, and operands |

Preserve child order in the first implementation. Keep guard 0 as `Always` while existing constructors rely on that convention. Represent false with one shared `Not(Always)` record; add no new enum solely for that constant.

Fold a `Guard::Site` only for a Live read of an actually immutable `SiteSource::Constant` profile and a property the runtime resolves entirely from that profile. A component site's properties and a contact's selected profile are not immutable. Do not fold Captured reads in the first slice: an absent captured operand evaluates differently from a present fixed profile. `Not` of an absent captured-profile test follows the actual Boolean evaluator, not a made-up unknown-value rule.

Use an exact compile-time interning map with a complete key; derive hashing on existing types where appropriate. Do not store the map in runtime or add another profile interner without measured duplicates. Canonicalization must not silently defeat receiver-guard validation, which currently recognizes particular implication structures.

## Algorithm B — simplify power contributions

Read the actual runtime expression `max(0, source_strength - attenuation)` and retain its query identity. All strengths must already validate in `0..15`.

- Remove a contribution attenuated by 15, or by a proved upper bound on its source. This removes a power record, never its separately meaningful callback.
- For equal `(source site, source port, target, literal face, guard)`, retain the least attenuation. Source ports include directional Weak/Strong, Stored, and Conducted with its dust policy. Target channels include Main, Side, Quasi, and conductor policy.
- First support exactly equal normalized guards. Broader dominance is optional: dropping A requires proving `guard_A => guard_B` and `attenuation_B <= attenuation_A` for the otherwise identical query. Do not infer implication from one selected contact.

Group by complete keys using an existing map or deterministic sorting; retain stable survivor ordering for inspection. Signal reads are pure; callback order is a separate table. Reject no circuit merely because a query cannot be simplified.

## Algorithm C — classify callback effects

Implement a small explicit effect classifier for the actual dispatch branch. Resolve the *effective* receiver, including `deliver_route`'s fallback from a component site when the stored receiver is `None`. The wire path also has its own candidate filtering. `receiver == None` by itself is not a no-op proof.

The first rules may remove:

1. An occurrence whose normalized guard is false in every execution context.
2. An ordinary Update whose effective handler is an unconditional no-op, such as an inert constant/control receiving a neighbor update. Do not confuse that with the control's separate host-use operation.
3. A Shape delivery whose exact handler is an unconditional no-op and whose geometry/support checks were completely discharged by admission. Keep geometric sites and contact bindings.

Inspect each switch arm before listing it as eligible. Exclude wires, comparators, observers, actuating pistons, command/note blocks, and any receiver with scheduling, routing, host, or support effects from blanket identity rules. A dead source can still deliver a necessary QC activation.

Later receiver-specific rules require a universal identity argument over their state domain, including pending ticks and supplied captured state. Do not implement adjacent duplicate suppression in the first milestone: nested work can change the second occurrence's meaning.

## Minimal proof helpers and certificate shapes

Build compile-time helpers inside this pass, not a theorem-prover framework:

```rust
enum PredicateFact {
    Always, // The predicate is true in every context permitted by the certificate.
    Never,  // The predicate is false in every context permitted by the certificate.
    Unknown, // The predicate remains dynamic or has not been proved constant.
}

enum RemovalReason {
    ImpossibleGuard,     // The occurrence cannot execute under the proved predicate domain.
    IdentityHandler,     // Executing this exact handler has no protected effect.
    ZeroPower,           // This electrical contribution is always zero.
    DominatedPower,      // A retained equivalent read contributes at least as much strength.
}

struct DeliveryRemoval {
    route: RouteId,       // The original ordered route containing this occurrence.
    ordinal: u32,        // The physical occurrence index, distinguishing equal sequences and duplicates.
    original_sequence: u32, // The native event number retained as diagnostic provenance.
    reason: RemovalReason, // The checked local rule permitting removal.
}

struct GuardRewrite {
    old_to_new: Vec<GuardId>, // The replacement ID for every original guard, including shared records.
}
```

These are shape sketches, not a request for parallel copies of existing IDs. Add one sentence for every new field. Keep certificates temporary or in compile/export provenance; execution does not reevaluate proofs.

The proof helpers needed now are: exact guard rewriting, immutable-property evaluation, complete-key power dominance, and audited handler-effect classification. An implication checker can initially answer only trivial structural cases; unknown means no deletion. SAT/SMT and complete reachable-state enumeration are not required for this project.

## Rewrite and reference handling

Build the rewrite decisions before mutating the draft. Check cancellation, then apply a small deterministic rewrite. Do not clone the entire CPU graph for rollback; the compiler already owns an unpublished candidate. A rejected candidate makes no edits; a fatal pass error prevents publication.

Remap guards in contact selectors, power links, all guarded link kinds, comparator far overrides, wire relations, and route deliveries. Search the actual structs for every GuardId owner. Update callback links and deliveries together with their multiset correspondence preserved.

Current routes require nondecreasing sequences, not contiguous numbering. Multiple guarded alternatives can share one native-event sequence. Keep existing sequence numbers when deleting occurrences; gaps are legal. Do not renumber by delivery ordinal or merge two duplicate native events. Preserve original ordinals in the deletion report.

Do not compact node/site/contact/resource IDs in the first slice. Remove unreachable guard records only after remapping every owner. Shared guard truth does not merge actor/resource identities. Run structural and receiver-binding validation after each completed rewrite.

## Implementation milestones

1. **Baseline and normalizer:** collect guard kinds, exact duplicate counts, child-reference counts, and executed power/callback work on a small fixture and the short CPU workload. Implement identities and complete remapping; no delivery deletion yet.
2. **Power cleanup:** implement attenuation-15 pruning and equal-guard dominance. Provide a side-channel and mutually exclusive guard counterexample.
3. **Callback cleanup:** implement false-guard and explicit unconditional no-op rules. Show an admitted real fixture with fewer delivered callbacks and unchanged protected observations.
4. **Review:** report retained records, runtime work, rewrite reasons, and paired execution time. Receiver-specific identity rules or broader liveness are later reviewed slices, not required to build a generic engine now.

## Required tests and proof witnesses

- Boolean rules including empty and repeated operands, forward references, and invalid/cyclic references.
- Live and Captured reads disagree; captured operand is absent; contact profile changes during a nested callback.
- Every valid local piston availability/head/resource-selection combination used by the fixture, including disabled payloads. Enumerating a conservative state set is guard-rule evidence, not proof of physical reachability.
- Two power links have different guards, ports, conductor dust policies, or Main/Side/Quasi channels: preserve them.
- Strengths 0..15 and attenuation 0..15 establish the arithmetic dominance rule.
- An unchanged comparator tick still notifies; a QC-powered actor needs an independent delivered update; repeated lamp updates preserve the first deadline.
- Duplicate callbacks separated by a nested state change remain ordered and distinct.
- Rewrite is deterministic and idempotent, and corrupting one remap or removal reason is detected.

Compare original Graph, reduced Graph, and interpreter using existing recorders. Keep protected observations and future work equal; expected optimization work counters may fall. Preserve JSON validity and complete callback-edge correspondence.

## Performance and completion

Extend the existing short CPU harness to select a supplied/default optimized pipeline and compare fresh unoptimized Graph, optimized Graph, and interpreter runs. Keep 100 active ticks, five paired samples, and all preparation/flush/checking outside timing. Remove fixed work-count equality only for certified reductions, replacing it with behavior checks and explicit before/after counts. Do not silently update a frozen count to hide a difference.

The current pass types/custom compiler entry point are crate-private; the external bench cannot assume it can construct arbitrary custom passes. Use `CompilerOptions.optimize` for the approved production default, and core unit tests for pass isolation. If custom benchmark order is needed, add the smallest reviewed configuration entry point; do not create a registry/plugin framework.

Discover new descriptive tests before running them. Run the affected pass/validation/callback/Graph suites in debug and release with `--locked`, core/server checks, core formatting, and diff checks. Do not claim success from zero matching tests or smaller JSON alone.

Completion means at least one real callback reduction, correct remapping, a declined unsafe candidate, and a measured paired result. Report graph size separately from guard/query/callback work and runtime. Stop for human review.

Not included: a generic liveness solver, duplicate callback elimination, new runtime actors, region caching, or native fallback. Risk: a receiver fallback or missing captured operand can invalidate an apparently trivial deletion.

## Implementation evidence — 2026-10-13

The pass is registered in the general pipeline only when `CompilerOptions.optimize` is true. It preserves guard 0 as `Always`, orders guard dependencies child-first even when IDs are forward-referenced, folds Boolean constants and repeated operands, interns complete normalized expressions with exact equality after hash lookup, and folds `Guard::Site` only for Live reads from immutable Constant profiles. Captured reads and dynamic component/contact reads remain unknown. It remaps contact selectors, callback links, power links, comparator far overrides, wire relations, and route deliveries before validation.

Power cleanup removes attenuation-15 and statically impossible contributions, then keeps the least attenuation for exact source-port, destination-channel, face, and normalized-guard keys. Callback cleanup removes impossible guards, Updates to the unconditional no-op Lever/Button/PressurePlate/Constant handlers (including the effective Component receiver fallback), and parser-validated Shape events to non-component Constant sites. It preserves duplicate meaningful deliveries, route order, original sequence gaps, and callback-link multiplicity. Component Shape deliveries stay because a captured Turbo route may use their eligibility to dispatch an Update; contacts, wires, pistons, observers, comparators, commands, and notes remain untouched.

The short release benchmark ran five paired fresh sign-free BubbleSort inputs, removing the same in-memory sign from Graph and interpreter worlds. Each run executed 100 active ticks; compilation, activation setup, flush, RAM reads, and checks were outside timing. Public compilation asserted the Graph backend. Graph and optimized Graph matched interpreter RAM on every sample. Median/range were 1.206323 s [1.189515–1.264577] unoptimized Graph, 1.208503 s [1.163095–1.218643] optimized Graph, and 0.648246 s [0.631941–0.652365] interpreter. The ranges overlap; this run shows no runtime speedup.

Unoptimized versus optimized work and lowered tables were: callbacks 2,059,909 -> 2,057,492; guards 3,758,922 -> 814,416; route deliveries 5,812,695 -> 5,354,102; callback links 1,973,211 -> 1,973,211; power links 196,784 -> 190,806. Other execution counts remained equal: 20,005 scheduled ticks, 32,505 wire walks, 302,784 piston samples, 27,884 accepted actions, and 27,884 deferred publications. Estimated retained tables after lowering were 267.95 MiB versus 184.94 MiB; these are retained table estimates, not RSS or peak memory.

Verification completed: pass tests were discovered (13 tests under the pass filter; 7 new opt-01 tests) and run; the integrated Redpiler suite passed in debug and release (211 passed, 4 ignored in each); all 7 reference-contract tests passed; `cargo check -p mchprs_core --tests --locked`, `cargo check -p mroww --locked`, formatting, diff checks, and the five-pair benchmark passed. The CPU episode checks final RAM for the first 100 active ticks, not the full sort/history. No broad fuzz campaign, full CPU equivalence, peak-RSS measurement, or runtime speedup is established. Keep the pass behind `--optimize` pending review of the proof boundaries and the lack of measured runtime improvement.
