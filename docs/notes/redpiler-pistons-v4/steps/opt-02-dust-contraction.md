# Optimization 02 — replace eligible dust execution with compiled transitions

Status: first query-contraction milestone implemented and awaiting human review. The pass remains experimental and requires `--experimental-dust-paths` with O1; ordinary `-O` does not enable it. This is not the full transition-table/callback-contraction scope below. Every declined candidate retains ordinary Graph/Turbo execution.

## Implemented first milestone

`passes/contract_dust_paths.rs` recognizes private, horizontal paths of two or three wires with one direct electrical ingress (lever or comparator). The input may produce any strength from 0 through 15. The candidate must have fixed non-contact geometry, no other nearby wire, no additional live or conducted source, no external power reader/observer, no comparator reading a path wire, stable saved sides, and entry strengths equal to the input minus wire distance. A parser-generated conducted read of a constant support block is accepted only when its compiled conductor has no incoming power links and is therefore identically zero. Matching imported wire ticks remain intact; a mismatched type declines the candidate.

The plan retains the original wire state slots, input node/site/face, and ordered path node IDs. `GraphBackend` uses it only to bypass local neighbor-power sampling in the seed and captured Turbo power queries; the Turbo walk, ordered routes, intermediate writes, callback dispatch, scheduling, reset, and projection continue through the general executor. The returned diagnostic power tuple keeps the same direct-source/wire classifications as the general query. Later graph passes invalidate certificates before they can become stale. The pass is the second O1 entry but stays disabled unless the experimental option is explicitly selected.

The correctness argument is local: the certificate accounts for every compiled power link into each path wire, admits only unconditional adjacent path-wire links at attenuation one plus the single direct input, and rejects any other nonzero or contact-dependent source. The first wire starts at the input strength and each later wire starts one level lower. Since runtime recomputes from the same live input and captured adjacent path-wire values, each specialized query matches the general query; the executor still performs all corresponding writes and notifications in their original order. This establishes the query rule, not a wholesale replacement of transient propagation.

Focused evidence currently includes all 256 old/new analog strength pairs at all three path positions for both seed and Turbo queries, a separate captured-old/live-new check, a synchronous comparator callback episode comparing ordered callbacks and intermediate wire state, a seeded lever history against general Graph and the interpreter, reset/materialization/continue checks, and negative fixtures for branches, a second source, a powered conductor, an observer, a contact, stale saved strength/sides, and mismatched imported work. A matching imported tick retains its deadline and priority. The CPU_BubbleSort short benchmark examined 5,017 candidate groups: 1,798 were too long, 49 were branches/bends/cycles, 1,674 had a nearby external wire, and 1,496 had contact-dependent geometry. It accepted zero paths and elided zero power samples.

The release benchmark ran five fresh paired 100-tick episodes on the same sign-free in-memory schematic variant for Graph and interpreter. Preparation, initial stimulus, flush, RAM checks, and statistics were outside the timed interval. All Graph episodes matched interpreter RAM. The medians and ranges were:

| Executor | Median | Range |
| --- | ---: | ---: |
| General Graph (O0) | 1.494863 s | 1.449365–1.541250 s |
| Opt-01 only (O1) | 1.473400 s | 1.401390–1.545116 s |
| Opt-01 plus dust contraction (O1 + `--experimental-dust-paths`) | 1.341765 s | 1.289194–1.450645 s |
| Interpreter | 0.739413 s | 0.690143–0.752256 s |

The apparent dust-enabled timing difference is not evidence of a speedup: no candidate was accepted, no dust query was elided, and the O1 and O1+dust ranges overlap. General Graph reported 20,005 scheduled ticks, 2,059,909 callbacks, 32,505 wire walks, 302,784 piston samples, and 27,884 accepted actions/deferred publications. Both O1 cases reported 2,057,492 callbacks and the same other counts. Retained-capacity estimates were 395.08 MiB after callbacks and 268.26 MiB after lowering for O0, versus 395.08 MiB and 185.26 MiB for O1 and O1+dust; these are table estimates, not RSS or peak-memory measurements.

Verification on this snapshot: the filtered Redpiler suite passed in debug and release (219 passed, 4 ignored each); core test/bench checks, the `mroww` check, formatting, and `git diff --check` passed. The benchmark command was `cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 5`.

Not established by this milestone: removal of internal callback dispatch, a compiled transient transition sequence, every Turbo heading/interleaving context, comparator-reader-specific decline attribution, or broader randomized analog component histories against the interpreter. These remain explicit review limits; they do not affect candidates that the certificate declines.

## Goal and first family

Remove repeated neighborhood discovery, guard reads, and internal wire callbacks from a private static dust region while preserving its complete externally observable transition trace. A shortest-path formula for final power is useful, but is not the runtime replacement by itself.

The first dynamic family should be deliberately small: a one-input, two- or three-wire path, with fixed geometry, one qualifying ingress pattern, no other wire connections, and a terminal boundary that cannot synchronously write back into the path. A supported ordinary lamp boundary is a candidate to investigate, not an already proved example. The pass must decline if the actual neighborhood/notification graph has additional ingress or feedback.

Do not silently require input stability or reset waiting. Arbitrary admitted input operations between completed synchronous updates remain legal. If a specialized path requires a stronger protocol, this project must explicitly enforce it or leave that path general.

## References and affected code

Read `docs/REDSTONE_MODEL.md` sections on power, support, scheduled ticks, and Turbo; read `docs/PISTON_MODEL.md` for contact-driven topology. Read [Optimization 01](opt-01-guard-and-update-reduction.md), the current migration report, and the agent guide.

Inspect:

- `crates/core/src/redpiler/program_graph.rs`: WirePlan, WireRelation, guarded power channels, routes, state ownership, and lowering.
- `analysis/electrical.rs`: discovery of ascend/descend and conducted/stored/directional channels.
- `analysis/callbacks.rs`: captured Turbo routes and effective receiver alternatives.
- `backend/graph.rs`: seed calculation, shape changes, heading rules, captured per-walk sites, nested walks, delivery, and presentation/reset.
- `crates/core/src/redstone/wire/mod.rs`, `wire/turbo.rs`, and existing Turbo tests as independent reference behavior.
- Graph nested-walk, slope, regulated-emission, imported-wire-tick, and continuation tests.
- Existing JSON export and the CPU short harness.

Old Step 15 and Side B notes are background; their references to removed Direct modules and old implementation status are historical. This scope uses the unified graph and does not require waiting for full FPU implementation.

## Boundary and eligibility conditions

Build the candidate closure from both power and callback relations. The exact boundary includes every source read, ingress delivery, egress power read/delivery, outside observer watch, shape/support dependency, captured operand, scheduled wire position, and reset/inspection requirement.

The first family requires:

1. All path geometry, support, connectivity, and relevant profile predicates are invariant. No contact alternative anywhere used by the compiled 24-site neighborhoods may change them.
2. One inventoried variable electrical input with a declared strength domain. A binary emitter gives `{0,15}` only after proof; otherwise use `0..15`.
3. No unseen ingress from a second source, neighboring wire, external update producer, observer, or downstream return callback. Include indirect conducted channels and notifications two cells away.
4. Every intermediate wire query is private or represented by an explicit boundary-read plan. The first family excludes external observers, comparators reading intermediate stored strength, QC consumers, and external captured walks through the path.
5. Initial wire strength and shape satisfy the proposed stable-boundary invariant. A stale saved wire strength is legal general entry; decline the optimization instead of normalizing it.
6. Boundary handlers cannot re-enter or modify the path during its compiled transition. If reentry exists, retain the original walk until the specialized engine explicitly supports it.
7. No transport, support-loss, host command, or unaccounted sound/action effect occurs inside the region.
8. Pending imported wire ticks preserve their typed positions, duplicates, deadlines, and native no-op dispatch. A retained tick handle may outlive an executable wire; do not drop work because the node was reduced.

Use the facts to recognize one *parametric motif*, not a schematic filename. An observer or contact added to the motif must either extend its certificate correctly or make the pass decline without rejecting compilation.

## Algorithm 1 — recover the electrical relation

Use directed channel relations discovered by the parser, including correct climb/drop and conductor policies. A spatial path does not imply that every weak/strong electrical direction is interchangeable.

For a proved static steady network:

`strength_at(v) = max_i max(0, input_strength(i) - shortest_attenuation(i, v))`

Accumulate attenuation with a capped BFS for unit wire costs, or a small nonnegative shortest-path algorithm if the supported relation costs differ. Paths that cannot contribute within strength 0..15 need no longer electrical reachability search. This does not bound callback propagation or remove Shape/Update records.

For the first one-input path, the formula is simply an input minus a known distance. Precompute each retained directional output's emission eligibility using the invariant geometry. Keep the source port, literal face, and include/exclude-dust policy of every external query. Never substitute a settled formula for the original diode stored-wire read without certification.

This formula provides the invariant at completed private transitions and the projection of stable wire values. It does not prove the ordered path taken from the old invariant to the new one.

## Algorithm 2 — compile the transient behavior

Choose one of these implementation methods according to demonstrated size and correctness:

- **Small transition table first:** enumerate every old/new input-strength pair, permitted seed/direction, and remaining finite context of the first family. Record a sequence of private wire writes and boundary deliveries. Share identical rows where all fields agree.
- **Parametric transition plan later:** derive a named rising/falling path procedure from the same rules and prove it against the complete small-family table. This removes table duplication when longer paths are added.

For an unconstrained one-input strength, old/new pairs give 256 cases; for proved binary input, four. Additional contexts must be explicit. Do not claim 256 cases are exhaustive if heading, ingress, initial wire state, or capture adds another variable.

Each retained boundary callback must sample the same intermediate values as before. A compiled step that only sends the final strength changes behavior. Execute private writes before their associated delivery; preserve duplicate occurrences, directions, nesting boundaries, and captured values. Where a caller's captured supplied block differs from the current block, keep that distinction.

A graph-theoretically acyclic wire path can still have transient back-propagation because adjacent wire power reads are bidirectional. Do not equate an undirected tree with a callback-free acyclic signal function.

Do not introduce a physical World adapter or copy Turbo into a second runtime. Compile-time verification may replay an isolated unoptimized Graph candidate using existing graph transitions. If production replay requires extensive extraction/duplication, use a small structural template plus exhaustive tests against the existing executor instead; document which part is the rule proof and which part is regression evidence.

## Minimal program shapes

These illustrate a first small-family implementation; adapt to actual existing storage and use named operations rather than a generic VM:

```rust
struct DustPathPlan {
    input: SiteId,                  // The already compiled input site used by this path.
    wires: Box<[NodeId]>,           // The original wire identities retained for state/projection and typed ticks.
    outputs: Box<[SiteId]>,         // The boundary sites whose ordered reads/deliveries remain visible.
    transitions: Box<[DustTransition]>, // The checked rows for the finite first-family input/context domain.
}

struct DustTransition {
    old_strength: u8,               // The previous input value required by this transition row.
    new_strength: u8,               // The incoming input value used by this row.
    ingress: u32,                   // The index of the certified seed/direction/view alternative.
    steps: Box<[DustStep]>,         // The exact ordered private writes and retained boundary deliveries.
}

struct DustPathState {
    last_strength: u8,             // The derived prior input value used to select a transition row, seeded from entry.
}

enum DustStep {
    WriteWire {
        node: NodeId,              // The existing authoritative wire state updated by this private step.
        value: u8,                 // The exact strength published before the following boundary read.
    },
    Deliver {
        route: RouteId,            // The existing ordered route owning this boundary occurrence.
        ordinal: u32,              // The physical occurrence invoked with its original direction and view.
    },
}
```

The prior-input cache is derived, not a second source owner. Update it after a completed certified transition; the no-reentry condition and complete ingress inventory must justify its validity. Equivalent rows may permit deriving the old class from authoritative wire state instead, avoiding the cache. The sketch is incomplete if a positive motif needs captured-site data not recoverable from these fields. Add the smallest explicit capture record for that motif or decline it; do not read live data in place of a captured operand. Do not let a Deliver operation call a full unfiltered route inadvertently.

First retain existing wire state slots and position bindings; replace the repeated calculation, not every byte of projection metadata. Once proven, original wire descriptions can become non-executable projection records with values derived from the path invariant. Their ongoing state must remain reconstructible at every supported flush/inspect/reset boundary, including a retained pending transition.

Existing Graph owns all state and scheduler execution. A compact path is an instruction/model inside that executor, with no separate clock, fallback interpreter, or independent activation loop.

## Proof helpers to build

1. **Boundary closure checker:** identifies all reads, ingress/egress routes, watchers, and possible return writes. Start with a small explicit whitelist of terminal handlers; unknown effects decline.
2. **Static topology checker:** proves geometry/channel predicates invariant from the actual site/contact representation. Initial pose or visually straight dust is insufficient.
3. **Path transition checker:** for each finite context, compares the compiled plan with the unoptimized path's ordered boundary trace and successor state.
4. **Stable-state invariant checker:** validates entry and proves that every supported input operation returns to the invariant. This induction, rather than one settled truth table, covers repeated histories.
5. **Projection checker:** verifies reconstructed wire strengths/shapes and pending typed work after every allowed host boundary.

Use finite enumeration and ordinary equality for the small family. No SAT solver is required. Bound path size, transition count, trace length, and proof work; hitting a bound retains general execution. Compilation may be expensive, but must not enumerate `16^n` for a large CPU network or retain huge proof traces in runtime.

A compile-time certificate names the checked family, original IDs, full strength/ingress domain, boundary, invariant, transition-row coverage, and reconstruction rule. Tests against the independent interpreter support the implementation. They do not establish completeness of an omitted state variable.

## Rewrite obligations

- Use the existing GraphPass API and validate before/after. Recognition produces a plan before mutation.
- Preserve external SiteIds/routes and typed pending work where possible in the first slice.
- Remove only the internal power/walk dependencies replaced by the plan; preserve unrelated updates and effectful boundary deliveries.
- Route sequences are native event labels; alternatives can share a label and duplicate events remain separate. Use occurrence ordinals in transition steps.
- Update WirePlan/WireRelation users, guard references, callback-edge correspondence, lowering bindings, and runtime dispatch together. Search for all site and node users, including canonical neighbor arrays of nearby *unreduced* wires.
- Do not leave a neighboring general walk pointing to a deleted wire. Either keep its necessary state handle or decline the candidate.
- JSON exports the post-pass replacement/provenance honestly. Do not imply an original executable wire survives when only its projection description remains.

## Milestones and witnesses

1. **Candidate-only analysis:** recognize one concrete path and decline a branch, a nearby observer, a contact affecting ascent, a stored-strength comparator reader, and a return callback. No runtime changes.
2. **Exhaustive small path:** compile and check all input pairs plus complete ingress/view contexts. Preserve every boundary event and intermediate read. Demonstrate one negative mutation of a plan detected by the checker.
3. **Integrated execution:** use the compact path inside Graph, compare arbitrary short repeated input histories, and hand off reset with pending ordinary ticks. Run with optimization off, this pass alone, and approved combinations with Optimization 01.
4. **Measured expansion:** only expand path size or region shape when the first rule passes. A longer path, multiple emitters, reconvergence, slopes, or dynamic contacts each requires new proof scope; none is automatic.

Required tests include analog levels 0/1/14/15 and all levels for finite rows, rise/fall/reversal, equal-value update, duplicate seeds, supplied captured/live disagreement, shape asymmetry, nested outer-walk exclusion, and reset at every supported operation boundary. Use existing schematics where the topology admits the family and preserve hashes/reference outputs.

Completion requires one admitted real reduction and one unmodified declined candidate. Report executable wires, callbacks, wire walks, power reads, retained reconstruction state, and paired runtime. The short CPU workload is a coverage measurement: if it contains no eligible path, report zero coverage instead of weakening the rule.

Not included: blanket dust elimination, a final-power fixed-point replacement, dynamic piston topology compression, multithreading, or a new simulator. Risk: correct final strengths can still hide wrong transient callbacks, captured reads, and timer deadlines.
