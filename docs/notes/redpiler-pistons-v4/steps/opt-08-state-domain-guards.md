# Opt-08 — simplify guards using state-field domains

Status: implementation scope only. Read the [shared guard plan](../GUARD_OPTIMIZATION_PLAN.md) and its validation-witness policy first.

## Purpose and boundary

Use facts about a single stored field to remove tests that ordinary Boolean normalization cannot recognize. For example, `ResourceAt(r, a) && ResourceAt(r, b)` is false when `a != b`; the two predicates inspect one scalar location, not independent Booleans.

Implement `passes/simplify_state_guards.rs`, with a stable pass name such as `simplify-state-guards`. This pass rewrites pure guard expressions. It does not recognize piston circuits, alter delivery records or resource transitions, cache results, or infer state from the current pose.

The first useful milestone is finding and simplifying frequently evaluated contradictory or redundant same-field predicates. If the current CPU has little such work, report that before broadening the algorithm.

## Files and consumers

Inspect `StateTest`, `PistonAvailability`, `PistonState`, `ResourceState`, `ResourceSpec`, and guard validation in `program_graph.rs`; `normalize_guards`, the interner, and reference updates in `passes/simplify_guards.rs`; and all state writers plus `guard_matches_using` in `backend/graph.rs`. Inspect guard construction in `analysis/geometry.rs`, `analysis/electrical.rs`, and `analysis/callbacks.rs`.

Reach the pass registry, shared remapping utilities, JSON post-pass export, and tests. No runtime enum addition is required: emit the existing `Guard` forms, using `Not(Always)` for false.

## State vocabulary

Suggested compile-time key; use the existing ID types:

```rust
enum StateField {
    /// This identifies one piston's independent extension Boolean.
    Extended(PistonId),
    /// This identifies one piston's independent head-occupancy Boolean.
    HeadPresent(PistonId),
    /// This identifies one piston's five-valued availability field.
    Availability(PistonId),
    /// This identifies one resource's scalar contact selection.
    ResourceContact(ResourceId),
    /// This identifies one resource's independent activation Boolean.
    ResourceEnabled(ResourceId),
}

struct FieldLiteral {
    /// This identifies the exact owner and state field being tested.
    field: StateField,
    /// This identifies a typed field value encoded within that field's domain.
    value: u32,
    /// This distinguishes equality from inequality against that value.
    equal: bool,
}
```

The numeric value is compile-time data, not a replacement for typed runtime state. Contact values must be translated through the resource's allowed-contact list; enum values need an explicit mapping, not an assumed Rust discriminant or an unchecked shift.

`Ready`, `HeadPending`, `PublishingHeadRemoval`, `BasePendingHeadPresent`, and `BasePending` exhaust the current availability enum. Booleans have two values. A resource's declared contacts provide its location domain only after auditing that every runtime writer and imported/restored state preserves membership, including when disabled. If that closure is not established, omit resource-domain exhaustiveness rules; distinct-contact equality contradictions remain valid.

Do not use cross-field assumptions. In particular, `ResourceEnabled(r)` is not implied by `ResourceAt(r, c)`, and head presence is not equivalent to extension. Do not infer mutual exclusion between two different resources from their entry positions.

## Algorithm

1. Validate references/cycles, obtain child-before-parent order, and reuse exact interning. Support forward references and shared children.
2. Recognize a `State` leaf or `Not(State)` as a field literal. Keep all other expressions as opaque residual children. Never reinterpret a Site predicate as a state field.
3. At an `All`, group direct eligible literals by `StateField`. Their allowed-value sets intersect. At an `Any`, their sets unite. Do not union/intersect across different fields, owners, or an intervening mixed Boolean expression.
4. Detect an empty set, a full proven domain, repeated literals, and subsumed equalities/inequalities. Replace only the affected same-field group with an equivalent smaller expression.
5. Emit the cheapest supported existing form: constant, one equality/inequality, a short `Any` of equalities, or a short `All` of inequalities. Choose deterministically. Never expand a two-literal expression into a long domain listing.
6. Reassemble residual children in deterministic order, perform existing identity cleanup, and intern the result. Choose replacement roots under the shared validation-witness policy.
7. Apply complete guard-reference updates and compact unreachable guards once. Keep stable graph IDs outside the guard table. Check cancellation during traversal, per-field work, and application.

Initial bounds: use a fixed small bitset for Boolean/availability domains, and a resource bitset only for domains of at most 64 distinct contacts. Larger resources still permit equality/inequality contradiction and duplicate checks using the mentioned literal values; decline exhaustive-set rewrites there. Do not allocate a full contact-domain bitmap for every resource/guard pair.

Process each join once over bounded direct children; do not distribute through nested `All`/`Any` to expose candidates. Opt-09 handles separate factoring rules. Guard groups above the configured internal work budget retain their original form. There is no user-facing proof-budget setting in the first version.

Examples:

```text
At(r,a) AND At(r,b)                 -> false, if a != b
At(r,a) AND NOT At(r,b)             -> At(r,a), if a != b
Availability(p,Ready) AND
  Availability(p,BasePending)       -> false
Enabled(r) AND NOT Enabled(r)       -> false
At(r,a) OR At(r,b)                  -> true only if {a,b} is r's complete proven domain
At(r,a) AND Enabled(r)              -> unchanged by this pass
Extended(p) AND NOT HeadPresent(p)  -> unchanged by this pass
```

## Correctness argument and checker

For a field with domain `D`, each equality/inequality denotes a subset of `D`. An `All` group denotes intersection and an `Any` group denotes union. Replacing that group with an expression denoting the same subset preserves its Boolean value for every allowed field value. Residual predicates are unchanged; Boolean congruence preserves the whole guard.

Use a small independent test evaluator to enumerate each applicable domain and compare the old and new group. For mixed expressions, enumerate the product of the small field domains and independent residual Boolean atoms. Bound generated cases; this is a pure-function proof/test model, not a queue or circuit-state explorer.

The proof of resource-domain closure is separate from set arithmetic. Record which state constructors, transfer writers, and continuation paths establish it. Passing initial-state validation alone is insufficient. If closure is uncertain, retain the smaller always-valid rule family.

No solver, temporal theorem engine, persisted certificate table, or mutable cache is required.

## Tests

- Every Boolean value and all five availability values; positive/negative literals, empty/full sets, and repeated values.
- Resource domains of 1, 2, 3, 64, and more than 64 contacts; distinct owners and nonconsecutive IDs.
- Disabled resources at every permitted contact. Never collapse `At` using `enabled`.
- All combinations of extension/head bits and availability values; do not assume only stationary combinations exist.
- Mixed `All`/`Any`, negated groups, opaque Site leaves, missing Captured operands, forward references, and shared DAG children.
- Cases where callback implication witnesses survive, cases that retain the old root, and a false guard requiring its witness to remain syntactically present.
- Complete owner remapping, callback-link multiplicity, JSON references, cancellation, disabled execution, deterministic output, and a second run producing no further change.
- Generated expressions checked against an independently implemented evaluator, with recorded seeds and minimized counterexamples.
- Existing BUD/shared-OR/adder, nested Turbo, observer, and reset/continue episodes against O0 and the interpreter.

## Milestones and measurement

1. Census actual matching root visits, grouped by rule and context, on CPU plus small fixtures. Stored pattern counts are insufficient.
2. Implement Boolean/enum and distinct-contact contradiction rules with the set-equivalence tests.
3. Add resource-domain exhaustiveness only if its writer/restore closure audit is complete and there is useful coverage.
4. Integrate remapping/witness checks, run the shared validation gate, and benchmark one-pass deltas.

Report successful root replacements, witness/budget declines, dynamic leaf operations avoided, retained bytes, and uninstrumented median/range. Semantic callbacks, samples, actions, imported work, and host effects must match. Fewer recursive operations are expected; fewer meaningful dispatched callbacks are not.

Stop at the implemented, tested, measured family. Cross-resource occupancy invariants and whole-piston reachability constraints are separate projects.
