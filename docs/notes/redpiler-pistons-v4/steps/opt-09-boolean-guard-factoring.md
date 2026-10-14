# Opt-09 — factor repeated Boolean guard work

Status: implementation scope only. Read the [shared guard plan](../GUARD_OPTIMIZATION_PLAN.md), especially callback-validation witnesses and the benchmark gate.

## Purpose

Remove repeated pure tests within one guard evaluation. Exact DAG interning saves storage but does not memoize evaluation: a shared child reached twice can still execute twice. Factoring can evaluate it once without introducing a mutable cache.

Implement `passes/factor_guards.rs`, with a stable name such as `factor-guards`. Keep the existing guard representation and evaluator. Do not reorder or combine callback occurrences, and do not share a computed guard result across handlers or nested walks.

Inspect `passes/simplify_guards.rs`, opt-08's actual shared helpers if implemented, `program_graph.rs` guard consumers/implication checks, `backend/graph.rs` guard semantics, and `analysis/export_json.rs`. No parser or scheduler redesign is required.

## Exact operand identity

A leaf identity includes its complete typed predicate. For Site tests, that means SiteId, OperandView, test kind, and ProfileId when present. `Captured(s, Profile(p))` and `Live(s, Profile(p))` are different atoms. So are different resources, pistons, contacts, and fields.

An absent capture makes its positive Site predicate false; its negation is true. Treat this as the existing Boolean semantics. It is not an unknown-value logic and does not justify replacing a Captured atom with a Live atom.

## First rule family

Implement only these identities initially, including their `All`/`Any` duals:

```text
A AND NOT A                 -> false
A OR NOT A                  -> true
A AND (A OR B)              -> A
A OR (A AND B)              -> A
(A AND B) OR (A AND C)      -> A AND (B OR C)
(A OR B) AND (A OR C)       -> A OR (B AND C)
```

Allow `B`/`C` to be short residual lists. Match exact shared expressions, not guessed implications. Identity/complement rules need no state-domain assumptions. Opt-08's same-field reasoning stays in its own pass.

## Algorithm

1. Obtain child-before-parent order and intern children using real shared helpers, without altering old guards while candidates are built.
2. Recognize direct complements and absorption first. Build a temporary child-membership set per bounded join. Guard evaluation is pure, so removing repeated reads is legal.
3. At `Any(All(...), All(...))`, identify a common literal or exact subtree and factor it out. Use the analogous operation for `All(Any(...), Any(...))`. Match adjacent sibling branches only, and put the replacement in their original position. Factoring branches across an intervening short-circuit alternative can move expensive reads ahead of it and increase execution work.
4. Start with a common first child, in original child order. This has a simple execution-cost argument: it avoids repeating that prefix while preserving the order of the remaining tests. General non-prefix or nonadjacent factoring is a separate measured extension, not part of the first batch.
5. Keep residual children in their prior order. Apply identity cleanup and interning, then estimate evaluation work. Accept only a strictly smaller bounded cost; break ties deterministically by original position.
6. Choose owner roots using the shared witness checks, compact unreachable guards once, and update all references. Preserve callbacks and links one-for-one except for their equivalent guard references.

Suggested pass-private candidate:

```rust
struct FactorCandidate {
    /// This identifies the original pure expression being considered.
    original: GuardId,
    /// This identifies the interned equivalent expression proposed as its replacement.
    replacement: GuardId,
    /// This counts interpreter-style node visits removed by the local cost model.
    saved_visits: u32,
}
```

Do not retain candidates or the matching indexes in runtime state.

Initial internal limits: at most eight children per inspected join and at most eight members per inspected nested join. Inspect child pairs within that bounded join; never compare every guard against every other guard. Report oversized candidates as declines. Check cancellation at traversal and join boundaries.

For the first version, cost an expression by bounded expanded node visits, counting repeated references as repeated work rather than one shared DAG node. For prefix factoring, independently enumerate abstract operand valuations in tests and assert that evaluation visits do not increase for any assignment. Do not equate this abstract cost with wall-clock speed; specialized runtime plans can change actual costs.

Perform one bounded bottom-up sweep. Every accepted replacement must strictly decrease the chosen measure. If another run can expose additional improvements, document that and bound the number of sweeps explicitly; do not promise idempotence without checking it or introduce an unbounded fixed-point loop.

## Validation boundary

Do not flatten a required contact witness merely because `All(A,B)` is equivalent to separate children A/B. The current validator may need the exact `All(A,B)` node as an implication witness.

Build owner-specific candidates while keeping original contact guards intact, as described in the shared plan. Retain the old expression when the existing receiver proof is lost. This pass may use less aggressive factoring at a callback root than at a power root. Record those declines; do not disable validation or accept an arbitrary receiver because the final output matched once.

Do not sort deliveries, merge guard alternatives into one notification, or remove meaningful duplicate notifications. Pure-expression factoring does not prove that two handler invocations are equivalent to one.

## Correctness evidence

Each rule is a propositional identity. Prove the identity over every Boolean assignment to its distinct complete operands, then use substitution to cover arbitrary pure subexpressions. This proof does not assume a stationary piston or a missing update.

Add a small independent test-only truth-table checker. Exhaustively check templates and generated expressions with up to ten distinct atoms; cap generator depth/size. Larger generated cases use reproducible assignments and the independent evaluator. Never label sampled assignments an exhaustive proof.

Required adversarial cases:

- Complement/absorption in both Boolean directions, including constants and empty residuals.
- Shared subtrees, forward references, and repeated child IDs.
- Nonadjacent common-prefix branches separated by a passing alternative; retain their original arrangement rather than evaluating the later branch early.
- Different fields/IDs and Live/Captured leaves that appear similar but must not match.
- Missing capture, stale captured contact versus current live contact, and negated profile tests.
- A contact witness that factoring would erase, and a power owner for which the same rewrite is accepted.
- Callback-link occurrence correspondence, duplicate sequence labels, empty routes, and JSON round-trip/reference validation.
- Disabled execution, cancellation, deliberate budget exhaustion, deterministic output, and bounded repeated execution.
- Composition before/after opt-08 and with index/dust/compiled-plan metadata rebuilt or invalidated as required.
- BUD, observer, shared payload, nested Turbo, command-feedback, and reset/continue episodes against general Graph and the interpreter.

## Milestones and measurement

1. Census dynamic complement, absorption, and common-prefix patterns after the current selected pipeline. Record roots blocked by witness preservation.
2. Implement complement/absorption and their exhaustive tests; compare runtime operations and memory.
3. Implement common-prefix factoring only when its hot coverage justifies it. Keep non-prefix distribution/reordering out.
4. Run broader checks and five-pair uninstrumented benchmarks with all neighboring passes identical.

Report retained guards separately from evaluated guard operations. Root counts and semantic event counts should remain equal; recursive operation counts may fall. Measure factoring both alone and with compiled patterns: a smaller AST can remove a specialization opportunity, so their gains cannot be assumed additive.

If hot coverage is negligible, stop after the census or the small rule family. SAT solving, global Boolean minimization, BDDs, cross-callback caches, and arbitrary clause distribution are outside this pass.
