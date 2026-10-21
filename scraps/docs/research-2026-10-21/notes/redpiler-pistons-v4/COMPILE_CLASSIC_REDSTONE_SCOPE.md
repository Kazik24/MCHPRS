# Compile classic redstone into direct signal dependencies

Status: implementation scope, 2026-10-15. Proposed pass: `compile-classic-redstone`, at O2, using the existing Graph backend.

Compile eligible fixed wiring into weighted signal dependencies and direct receiver operations. Accepted wiring must execute without Wire Turbo, spatial neighbor resolution, internal update/shape dispatch, or intermediate dust writes. Eligibility is local: unrelated pistons elsewhere in a selection do not disqualify its classic circuits.

The compatibility target is the current interpreter's observable component behavior, presentation effects, scheduler and pending-work order, and reset/continuation behavior. Internal dust callbacks may disappear only when their effects have been accounted for. Recover the useful representation from historical DirectBackend at `7470205`; its old admission rules and successful workloads do not establish compatibility for new candidates.

## Representation and ownership

The desired electrical representation is:

```text
source -> committed net signal -> attenuation -> receiver input
```

For a general fixed input, the arithmetic is `max_i(signal_i.saturating_sub(attenuation_i))`, with separate main and side channels. Strengths remain four-bit values. The first implementation admits one variable ingress per wire region; branches and fanout do not themselves require walking wires at runtime.

Use flat tables and dense IDs in the existing program, with four pieces of data:

| Data | Meaning |
| --- | --- |
| Direct net | One admitted wire region, its ingress binding, and its ordered boundary operations. |
| Direct input | A committed net signal, attenuation, and existing receiving node/input port. |
| Net state | The strength last delivered into that net, owned by `CircuitState`. |
| Wire reconstruction | Original wire ownership and fixed properties, plus the attenuation needed to reconstruct its strength. |

The committed signal is essential. A component's output can change before its route reaches a particular wire. Reading the component's newest output for every collapsed wire would publish that wire's change too early. Update the net at the original admitted entry occurrence, preserving its position relative to other route effects. Initialize it only when the saved wire strengths establish the entry relation.

Retain existing component state owners and the common scheduler. Lower direct inputs and outgoing bindings to contiguous ranges; use no runtime target hashing or geometry search for accepted inputs. Start with maximum over the small direct input list. Per-strength histograms can follow if fan-in measurements justify their mutable state and initialization cost.

Private wires need no strength writes in the hot path. Their projected state is derived from the committed net signal, not the source's possibly newer output. Inspection, full flush, reset, and any legitimate general read must use this projection. Keep node IDs and position ownership stable in the first implementation.

## First supported family

Discover whole wire regions from the compiled electrical and notification relationships. Apply these conditions to all possible bindings, rather than only the saved contact selection:

1. Wire geometry, support, directional emission, and conducting paths are fixed. Seed and Turbo transfer rules must agree on the admitted topology. Dust shapes already match the fixed geometry.
2. Exactly one variable electrical ingress exists. Fixed sources may be folded into the transfer only when the same proof covers them. Initially admit ordinary lever, button, pressure-plate, torch, and repeater output sources; their existing use/tick behavior stays authoritative.
3. Meaningful receivers initially comprise torches and repeater main inputs whose locking input is proven always zero. Each owned receiver's complete main-input query must depend only on this net, its ingress, and compile-time constants; another variable reader or independently owned net declines this first rule. Their update handlers can schedule work but cannot synchronously change another signal in the region. Repeater delays and priority rules remain unchanged.
4. Inventory every electrical read and every possible notification crossing the region, including callback-only wire neighbors in the full canonical neighborhood. All such effects must be owned by this plan or proven inert. No outside wire walk can capture or update private wires.
5. No observer watches a private wire or an affected property whose omitted changes it could observe. No piston, selectable contact, quasi-connectivity sample, comparator, bulb, note block, command block, or other effectful handler participates in the region's synchronous boundary.
6. No receiver can synchronously reenter the region or change its ingress during an invocation. Ordinary delayed feedback through torches and repeaters is allowed when the same entry invariant holds at every invocation; do not reject all graph cycles solely because they contain delays.
7. Saved wire strengths satisfy the derived transfer for the initial committed input. Imported work cannot target a private wire or inject an unrepresented event. Existing pending receiver ticks may remain only if the proof covers their pending-membership, deadline, priority, and order semantics.

Initially use a bounded region size and proof-work budget. A discovery census must select useful bounds before default enablement. An oversized or unproved region stays general without rejecting the selection. Do not split a larger wire region into independent nets unless the cut also establishes complete read and notification ownership.

Terminal lamps, fixed analog/comparator boundaries, repeater locking, multiple variable ingresses, and contact-backed gating are subsequent admission extensions. They use the same signal representation but need their own boundary argument. Do not include them merely because their final arithmetic is easy to calculate.

## Boundary inference and compatibility proof

Static attenuation proves final electrical values; it does not prove receiver behavior. Compile the effect of an admitted wire episode into direct receiver operations in native order. These operations use explicit sampled inputs and the existing scheduling primitives; they do not dispatch Minecraft neighbor callbacks or invoke a runtime wire interpreter.

The proof must establish all of the following:

- Every admitted invocation starts in the saved/previously established net relation and completes in that relation for its new delivered input.
- Each private wire's final value equals its direct transfer, including attenuation cutoff and source removal.
- Receiver samples that can affect scheduling are represented with their original captured/live meaning. A falling wave may expose an earlier nonzero sample before its final zero; using only the final maximum is insufficient.
- Direct operations preserve the first effective scheduling attempt, pending-membership behavior, deadline, priority, and insertion order across receivers and relative to surrounding source-route effects.
- Repeated samples disappear only after an idempotence argument for the receiver and all admitted pending states. A component delay alone does not make samples or receiver order interchangeable.
- No omitted intermediate write or shape change is observable through a general reader, observer, nested callback, presentation effect, or pending operation.

Infer geometry and weighted transfers structurally. For the first single-ingress family, use the finite old/new strength domain to validate a proposed compact boundary program: all 256 strength pairs, the supported receiver powered/pending states, and relevant captured-state cases. This check must follow an induction argument that the admitted entry relation covers every future invocation. Enumerating example histories or a truth table alone is insufficient.

Do not store 256 event transcripts per instance or enumerate complete CPU states. The retained program consists of direct operands and the ordered operations their proof permits. A bounded reference replay during compilation may validate a candidate; production execution must use the compiled operations. If no compact exact boundary program can be established within budget, decline that candidate.

## Integration in the existing compiler

Use a read-only `PlanBuilder` in `passes/compile_classic_redstone.rs`, registered as `compile-classic-redstone` at O2. The builder derives the signal program and its ownership witness from the complete `CompileGraph`. Source routes remain the semantic reference; lowering replaces only occurrences owned by an accepted plan. This follows the existing instant-proxy pattern of retaining witnesses for validation/export and removing their duplicate executable deliveries at lowering.

Run after graph rewrites, before other plan builders that could claim the same wires. Every later builder must skip already owned regions. Validation must reject overlapping ownership between classic nets, dust shortcuts, instant proxies, symbolic actors, and machines. Upstream rewrites clear the plan through the existing execution-plan invalidation path. O0 remains a complete general graph, and custom pipeline order must retain its existing explicit semantics.

| Area | Required change |
| --- | --- |
| `passes/compile_classic_redstone.rs` | Discovery, eligibility checks, transfer construction, boundary proof, cancellation, and bounded reporting. |
| `passes/mod.rs` and `passes/planning.rs` | Register the builder and its typed output; define ownership-aware builder order. |
| `program_graph/execution_plans.rs` | Store derived plans and witnesses; invalidate them with existing plans. |
| `program_graph/validation.rs` | Independently check references, complete boundaries, transfer/proof witnesses, entry bindings, and exclusive ownership. Share pure certificate rules through `program_graph` where needed; do not import pass implementation helpers. |
| `program_graph/lowering.rs` and program/state types | Lower dense direct tables, committed signal state, entry-occurrence bindings, and reconstruction metadata. |
| `backend/graph.rs` and a focused runtime module | Execute owned entry occurrences and direct receiver operations; preserve surrounding route order and the common scheduler. |
| `backend/graph/power.rs`, `projection.rs`, and inspection/reset paths | Read direct inputs and reconstruct private wires consistently for execution and handoff. |
| `analysis/export_json.rs` and existing viewer/validator code | Export accepted nets, direct edges, ownership, and reconstruction provenance; display the optimized representation actually executed. Keep old exports readable. |
| Pass statistics and benchmark support | Report coverage, declines, proof cost, retained bytes, and work eliminated using existing reporting mechanisms. |

Use additive JSON records with typed references. Replaced callbacks must be marked as source/proof provenance rather than displayed as executable edges. Regenerate affected fixture exports and update their readers in the implementation change.

This pass begins after required callback preparation. It cannot fix CHUNGUS's recorded preparation-memory failure by itself. Reducing redundant guard/callback construction is a separate preparation change; do not raise limits or omit required semantics to claim coverage.

## Delivery and acceptance

1. **Discovery checkpoint.** Run inference without execution changes over all graph fixtures, the ordinary component suite, and ANPU. Report accepted/declined regions, wire and receiver counts, largest candidates, projected table costs, and precise decline reasons. Identify real positive fixtures for the first family and select bounded proof limits. Historical census percentages are leads, not accepted coverage.
2. **Executable first family.** Complete the builder, independent validation, lowering, direct runtime, projection, export/viewer support, and focused differential tests together. Accepted invocations must construct zero `WireWalk`s, perform zero private callback dispatches, and issue no intermediate dust writes. Removing only power-query lookups does not meet this milestone.
3. **Compatibility and performance checkpoint.** Compare interpreter, O0, and enabled O2 on the same circuits, entry work, and control histories. Run focused tests and affected existing suites, then paired release benchmarks. Report whole-circuit time, compile/proof time, storage, accepted invocations, callbacks/walks eliminated, and the remaining general work. Enable the pass by default only after this checkpoint.

The test matrix must include rise/fall, repeated and short pulses, attenuation cutoff, straight/bent/branched wiring, native receiver insertion order, simultaneous scheduled sources, pending receiver ticks, and delayed feedback. Exercise inspection/flush/reset and native continuation after every relevant cut. Require matching receiver state, ordered future work and projected cells, not only final lamp values or CPU RAM.

Negative cases must cover observer-watched dust, comparator/locking reads, piston cuts, callback-only neighboring wires, a contact with an inactive wire variant, reentry, stale saved wire strengths, imported private-wire work, budget exhaustion, corrupted witnesses, and overlapping plan ownership. They must retain general execution and remain compilable. Preserve the existing adder FIFO counterexample as a regression against accidental settling-based admission.

For the first family, test the net's committed signal explicitly: a source change before its admitted route occurrence must not change a reconstructed wire or downstream input early. Repeat the check when a source route contains duplicate entries and unrelated effects.

## Scope limits

This scope adds one optional optimization and its necessary runtime/inspection support. It does not restore DirectBackend, introduce another scheduler or simulator, enable fixed-point propagation globally, require stable CPU input protocols, or recognize schematic names. Further families broaden eligibility only after establishing their own entry and boundary invariants.

Background: [existing pass contract](../../../../../crates/core/src/redpiler/passes/README.md), [classic redstone research](../../../redpiler-pistons-v4/SIMPLE_REDSTONE_ABSTRACTION_RESEARCH.md), and [the ordering counterexample](DUST_FIXED_POINT_INVESTIGATION.md#proven-ordering-counterexample).
