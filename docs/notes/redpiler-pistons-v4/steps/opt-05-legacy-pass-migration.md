# Legacy pass portfolio — what can return to the unified graph

Status: source-based implementation scope. This is the fifth document in the optimization set, not a fifth instruction to restore DirectBackend. It inventories the removed pipeline and assigns reusable rules to small verified graph transformations.

## Historical source and current target

The old modules were removed by `63b12c6`. Inspect their immediately preceding version without switching branches:

```powershell
git show 7470205:crates/core/src/redpiler/passes/mod.rs
git show 7470205:crates/core/src/redpiler/passes/clamp_weights.rs
git show 7470205:crates/core/src/redpiler/passes/dedup_links.rs
git show 7470205:crates/core/src/redpiler/passes/constant_fold.rs
git show 7470205:crates/core/src/redpiler/passes/unreachable_output.rs
git show 7470205:crates/core/src/redpiler/passes/constant_coalesce.rs
git show 7470205:crates/core/src/redpiler/passes/coalesce.rs
git show 7470205:crates/core/src/redpiler/passes/prune_orphans.rs
git show 7470205:crates/core/src/redpiler/passes/legacy_regressions.rs
```

Current execution uses `CompileGraph`/CircuitDraft, typed Power/Update/Shape relationships, ordered routes, sites/contact alternatives, shared resources, ordinary/deferred work, and one Graph executor. No old pass may silently convert that representation to power-only links.

The current reusable component identification lives in `crates/core/src/redpiler/components.rs`. Required parser stages live in `passes/rich.rs` and analysis modules. The configurable pass API is `passes/mod.rs`; transformations must remain individually enableable/testable in caller-supplied order.

Read the current reference model documents, migration report, agent guide, and the four new scopes:

- [01: query and update reduction](opt-01-guard-and-update-reduction.md)
- [02: dust contraction](opt-02-dust-contraction.md)
- [03: actuator specialization](opt-03-piston-specialization.md)
- [04: region contraction](opt-04-instant-region-contraction.md)

Old Side B notes are useful historical analysis, but several paths and statuses in them are stale. The source commands above define the version reviewed here. No old backend or pass has been executed by this document.

## What the old pipeline actually did

Required construction: identify nodes -> search power inputs -> clamp weights and select propagation policy. Optional rewrites: deduplicate links -> fold constants to a local fixed point -> prune unreachable comparator power -> coalesce constants -> coalesce duplicate logic -> prune orphans in IO-only mode. Export was a separately enabled output stage.

The old public path suppressed those optional rewrites when comparator presence or a directed cycle selected Native. Thus old passing comparator/feedback tests may not have executed the rewrite being discussed. `--optimize` on and a passing output test are not proof that a particular rule ran.

## Rule-by-rule migration

| Old module | Actual algorithm | Unified-graph disposition and smallest implementation |
| --- | --- | --- |
| `identify_nodes` | Identify electrical blocks, saved state and IO roles; conditionally omit wire displays | Retain `identify_block` through current components/parser. Do not restore identification-time wire omission. Every optional reduction consumes the complete graph. |
| `input_search` | Follow electrical geometry and shortest dust distances, produce Default/Side power edges with a 15-distance cutoff | Reuse attenuation mathematics in Optimization 02. Do not restore lossy world traversal or treat distance-limited power as a bound on updates. |
| `clamp_weights` | Delete all edges with attenuation >= 15 | Restore as a local **power-contribution** rule in Optimization 01. Current strength validation permits 0..15; corresponding callbacks remain independently live. |
| `dedup_links` | For equal source/target/channel retain a lowest-attenuation contribution; implementation scans incoming parallel links | Restore with complete rich-query keys and equal normalized guards first. Use grouping/sorting instead of quadratic incoming scans. Never deduplicate callback occurrences. |
| `constant_fold` | Repeatedly turn settled repeaters/torches/comparators with constant inputs into Constant; skip pending work, mismatched saved output and dynamic command/bulb far overrides | Arithmetic is reusable. Actor removal requires an identity-transition certificate under all callbacks and state/geometry alternatives. First fold pure query expressions; retain physical handlers/state until their effects and reconstruction are proved removable. |
| `unreachable_output` | For a subtract comparator with one constant side input, bound output by `max(saved output, 15 - attenuated side)` and remove sufficiently attenuated outgoing edges; skip pending work | Restore as conservative strength-bound pruning of power records. Preserve callback routes and all state/time ownership. Broader side inputs require a proved lower bound, not their initial maximum. |
| `constant_coalesce` | Union nonconstant connectivity, create shared constant nodes per connected set/strength, remove original constants | Share immutable literal *values*, not physical source sites or resource state. Existing profile interning may already cover the storage opportunity. Implement only after measuring useful remaining duplicate value work. |
| `coalesce` | Merge removable equal type/state siblings with one Default input and compatible attenuation; retain block aliases | Do not restore general actor merging. Share a pure derived calculation through Optimization 04 while keeping separate tick membership, routes, resource identities, and projection. Actor merging needs full temporal equivalence. |
| `prune_orphans` | Reverse DFS from nonremovable IO/pending roots through incoming electrical edges | Replace with semantic liveness using power, callbacks, guards/sites, watches, resource ownership, pending work, host effects and projection. Start with compile-only unused records; no electrical-only execution pruning. |
| `export_graph` / DOT export | Convert electrical-only nodes/links to the old format | An output adapter, not an optimization. Keep current explicit binary/DOT rejection and supported JSON. Do not revive a legacy compiler to produce these formats. |
| Native selection and comparator-ordering rejection | Choose native propagation for comparators/cycles or reject selected callback-sensitive cases | Delete policy remains correct. No pass may restore it as a fallback or use it to avoid proving a rewrite. |

## Immediate algorithms worth implementing

### L1 — zero and dominated power links

For validated source strength `s in 0..15`, contribution is `max(0, s-a)`. Attenuation 15 is always zero. For two otherwise identical reads with equal guard, `a_low <= a_high` means the lower-attenuation contribution dominates.

The full identity key is source SiteId, PowerPort including face/dust policy, target and input channel, literal face, and normalized guard. Build groups once, retain a deterministic least-attenuated survivor, and update only the electrical contribution table. Callback edges and route occurrences remain distinct.

Optional guard dominance needs a proof: drop A only if `guard_A => guard_B` and `a_B <= a_A`. Opposite resource-enabled guards are not interchangeable. Reuse Optimization 01's helper rather than writing another implication engine.

### L2 — conservative strength bounds

Use a small interval analysis for pure query contributions. Unknown is `[0,15]`. Do not infer permanence from the enum name Constant or an entry strength alone; establish immutable source ownership and channel eligibility first.

```rust
struct StrengthBounds {
    lower: u8, // The strength guaranteed in every state permitted by the checked source/guard domain.
    upper: u8, // The maximum strength possible in that domain, including imported saved output.
}
```

Transfer rules for a bounded electrical read:

- Attenuation: `[max(0, lower-a), max(0, upper-a)]`.
- Max contributors: maximum of guaranteed lower bounds and maximum of possible upper bounds. A guard that can disable a contributor reduces its guaranteed lower bound to zero.
- Boolean emitter: `[0,15]` unless a stronger invariant is proved.
- Subtract comparator with rear upper bound R and side lower bound L: future upper bound `max(0, R-L)`. Include current saved output S: `U = max(S, max(0, R-L))`.
- A contribution attenuated by at least U is zero. Keep the source component and its callbacks unless a different proof removes them.

Validate 0..15 arithmetic independently by finite enumeration. Dynamic overrides, guard-selected side channels, stale captured states, and delayed lock behavior remain unknown unless their complete domain is established. Side B's fixed-side special case can return before a general interval solver exists.

Use a bounded worklist for propagation when later pure-expression rules need it. Recursive conductor/query cycles must not acquire an invented fixed-point meaning; unknown cycles stay conservative. For sequential actors, a proof must bound both saved state and every later update, not just a static expression.

### L3 — semantic liveness

Separate an executable owner from its physical description. A node can lose execution while still requiring state/projection metadata for reset.

Build temporary reverse-use indexes for every typed reference and root them in:

- retained output strengths and ordered effects;
- host inputs/commands/sounds/block actions;
- observer watched sites and captured operands;
- normal/QC activation, shape/support and resource transitions;
- imported work, pending membership/deadlines and cancellation;
- exact supported flush/inspect/reset reconstruction.

Walk all relevant reference families, not only power edges. Remove unused immutable records first. Remove an executable owner only after an effect/projection certificate establishes why no future operation needs it. `io_only` limits presentation; it does not make hidden state or pending work disposable.

Do not turn this into a general-purpose optimizer graph database. A few temporary vectors/maps over current IDs suffice. If rooting projection conservatively keeps every node live, report that honestly and let a later certified reconstruction rule free execution.

## Deferred algorithms and why they need more proof

### Physical constant folding

A physical component's unchanged output is not the whole transition. Lock/powered flags, imported ticks, emitted notifications, and observed geometry can still matter. A locked repeater with saved output 0 and permanently powered rear input may unlock on a later callback if its saved lock flag does not match the current side input; using the saved lock to fold it to 0 would suppress that behavior.

First establish a complete identity-handler rule or specialize a pure computation. Reuse audited transition/effect checks from Optimizations 01/03 instead of porting old `fold` wholesale. Keep the independent initialized-state and callback-history checks.

### Physical node coalescing

Equal repeaters with one electrical source can receive different position-bound callbacks and acquire separate pending deadlines. Their equality at compilation does not make their future histories equal. Different raw positions/routes cannot be replaced by one state plus display aliases.

Share their input predicate or pure arithmetic result if useful; preserve their individual event handlers and queues. Merging actors is a state-machine proof in Optimization 04, including stale typed work and reset. A Boolean attenuation exception requires a proved `{0,15}` source domain; saved output 0/15 alone does not establish that domain.

### Constant/resource coalescing

Two redstone tokens both emit 15 but have different owners, contacts, enabled flags, and cancellation revisions. Disabling or moving one must not change the other. Fixed source sites can also have different direction/support/watch obligations.

Existing profiles/literal scalars may be shared, but SiteId/ResourceId and mutable state remain independent. Do not reintroduce undirected connectivity as an ownership proof.

## Minimal proof helpers required

Reuse the earlier documents' helpers wherever their exact domain applies:

1. Arithmetic enumeration over 0..15 for clamp/dominance/bound formulas.
2. Complete-key guard equality and conservative implication for conditional power contributions.
3. Immutable-source/domain checks and conservative interval bounds.
4. Explicit transition/effect summaries before physical constant folding or deletion.
5. Semantic reverse-reference closure and reconstruction checks for liveness.
6. Bounded temporal equivalence for any physical coalescing, supplied by Optimization 04 rather than a second model checker.

No SAT/SMT dependency is required for L1/L2/L3. Proof unknown means keep the original contribution/owner. Never restore native propagation as the unknown case.

## Concrete counterexamples to retain

- Attenuation 15 removes electrical contribution but an independent update still activates a QC receiver.
- Same endpoints with `ResourceEnabled(r)` and `Not(ResourceEnabled(r))` require different power alternatives.
- A comparator's saved output is larger than its future constant bound; it remains observable until reevaluation.
- Side source strength 15 behind attenuation 14 gives side strength 1, not 15. A guarded side source can disappear and permit full output.
- Compare-mode same-value ticks still issue notifications; a pending tick cannot disappear because a future value is constant.
- A shared input drives two receivers with independent update histories/deadlines; display aliases do not preserve that work.
- An output-inactive branch is the only update generator for a BUD/QC circuit.
- Two equal redstone resources move independently; value sharing must not merge tokens.
- A hidden node under `io_only` is still needed for reset continuation.
- Captured and live wire strength/profile disagree during nested propagation.

Use the historical `legacy_regressions.rs` to recover intent, then require a real rewrite and independent interpreter comparison. A migrated test with optimization enabled but an empty/no-op pass pipeline is not evidence for the new rule.

## Implementation order and review checkpoints

1. **L1 in Optimization 01:** implement clamp and exact-guard dominance with full typed keys. One positive and one guard/channel negative case; report actual removed contributions.
2. **L2 separately:** add the fixed-side subtract bound, including saved output and guard uncertainty. Expand bounds only after the small rule is reviewed.
3. **L3 compile records first:** remove unused immutable records with full remapping, then one proved ineffective executable branch. Preserve projection and imported work.
4. **Pure sharing:** use measured duplicate expressions/literals inside Optimization 04. Do not implement physical actor coalescing as a shortcut.

Keep transformations separately selectable through the existing API. Test a pass alone, approved combinations, optimization off/on, cancellation, invalid output, and deterministic second application. Do not restore a fixed legacy sequence merely for symmetry.

ID compaction requires explicit maps across every owner, including new specialized plans and entry work. Routes need monotone native-event sequences with guarded alternative grouping and duplicate multiplicity preserved; contiguous renumbering is not currently required. Keep original-ID provenance for diagnostics/export.

## Verification and completion

Discover and execute descriptive pass tests, graph validators, callback/electrical regressions, and affected general runtime cases in debug/release with `--locked`. Run core/server checks, formatting, and diff checks. Avoid restoring packed-layout-only tests for an implementation that no longer exists.

Use the same 100-tick CPU workload, fresh input worlds, five paired production samples, and identical sign removal in memory only. Record fixture hash, source revision/dirty state, exact pass list/options, and observation contract. Separate table estimates, executable work, and elapsed runtime; do not repeat the incorrect double-counted lowered-memory estimate from the earlier report.

Completion of this portfolio is incremental: report which numerical/liveness rules are implemented and measured, which are declined, and which actor-level rewrites remain deferred. Do not claim all old performance was recovered because old modules were recreated.

Not included: Direct/Native restoration, packed backend limits, legacy binary compatibility, electrical-only orphan pruning, or unproved physical coalescing. Risk: a valid electrical identity can still erase necessary updates, independent state, or future work if applied to whole actors.
