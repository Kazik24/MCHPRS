# Redpiler optimization passes

Read this before implementing or reviewing a pass. These rules apply to every new optimization, including experimental passes. They describe how to work on the shared graph; they do not authorize implementing additional projects beyond the assigned task.

An optimization must preserve the supported circuit's observable behavior and future execution while reducing runtime work or storage. A smaller graph, a correct final truth table, or a passing CPU benchmark alone does not establish correctness.

## Architecture in one minute

```text
Read-only blocks and imported work
    -> required geometry, electrical, and callback preparation
    -> CompileGraph + EntryState
    -> optional, ordered graph passes
    -> validated graph
    -> lowering to CircuitProgram + CircuitState + imported work
    -> GraphBackend execution
```

`CompileGraph` is the general `CircuitDraft`, not the removed legacy electrical graph. It contains components, power queries, guarded contacts, shared resources, ordered callback routes, generalized dust, and initial state. Power availability and delivered updates are separate relationships.

[rich.rs](rich.rs) coordinates required preparation and lowering. [mod.rs](mod.rs) owns the transformation API and runner. [utils.rs](utils.rs) holds shared pass reporting and table-memory accounting, including preparation limits. [program_graph.rs](../program_graph.rs) defines the graph and structural validation; [graph.rs](../backend/graph.rs) executes lowered data. Runtime redstone executes on the graph without spatial World queries, physical Motion objects, or progress simulation. The independent interpreter remains the behavioral test oracle.

Execution and inspection use shared preparation and pass infrastructure. Preserve the graph actually produced by the selected pipeline in [JSON export](../analysis/export_json.rs); never export an earlier draft while executing a rewritten one.

## File organization — mandatory

Instant region boundaries and the private-cell instruction proxy are documented in
[INSTANT_REGIONS_AND_PROXIES.md](../../../../../docs/notes/redpiler-pistons-v4/INSTANT_REGIONS_AND_PROXIES.md).
At O3, `identify_instant_regions` follows built-in reset compilation and
`compile_instant_proxies` follows boundary identification. Shared regions remain
general execution. Passes invalidate these certificates by default; opt into
preservation only if ownership, original route rows, and reset witnesses remain valid.

At `-O4` and above, `compile_instant_machine` follows O3 proxy compilation and requires `--stable-instant-inputs`.
It derives a compact phase/expression program for bounded closed selections. Full snapshots
remain compile-time proof data; hot ticks evaluate output and effect roots without state-table
lookup or private propagation. Entry mismatches keep general execution; active control changes
reconstruct exact private state and future work before ordinary callbacks. The initial family
covers the whole one-bit adder; larger regions remain ordered proxies. Read
[COMPACT_INSTANT_KERNEL.md](../../../../../docs/notes/redpiler-pistons-v4/COMPACT_INSTANT_KERNEL.md)
before extending admission, normalization, or the bounded derivation budget. The superseded
snapshot prototype is recorded in
[CLOSED_INSTANT_MACHINE.md](../../../../../docs/notes/redpiler-pistons-v4/CLOSED_INSTANT_MACHINE.md).

1. Every optimization pass **must have its own `.rs` file** under `crates/core/src/redpiler/passes/`. Use a descriptive name such as `simplify_guards.rs`, not a batch number.
2. `mod.rs` holds registration and pipeline infrastructure. Do not implement unrelated transformations inside the runner, parser, compiler orchestration, or backend dispatch.
3. Keep pass-specific discovery, proof, and rewrite helpers private to that pass. Break complex work into small, named functions and intermediate variables.
4. Common helpers used by multiple passes may live in `passes/utils.rs`, or `passes/utils/` when several focused modules are needed. Extract a helper when real consumers exist; do not create a generic framework for hypothetical passes. Utilities must not silently run another pass or choose pipeline order.
5. Keep focused tests in the pass's `#[cfg(test)]` module. A larger suite may use a clearly named sibling test file in this folder. Register it so Cargo actually discovers the tests.
6. Runtime instructions, lowering support, and validators belong in their existing layers when a rewrite requires them. A pass must not introduce a second simulator or a fallback backend.
7. Never import functions or primitives between passes - must use utils, mod or other common file for this.

The required preparation driver `rich.rs` is not an optional optimization. Disabling optimization must still produce a complete, executable general graph.

## Start here

Before editing:

- Read the assigned pass scope and the relevant [redstone model](../../../../../docs/REDSTONE_MODEL.md), [piston model](../../../../../docs/PISTON_MODEL.md), and [interpreter architecture](../../../../../docs/INTERPRETER_ARCHITECTURE.md).
- Inspect the current worktree and local instructions. Preserve concurrent edits and fixture data. Reports describe previous snapshots; verify the current source.
- Trace the affected operation through parsing, graph references, validation, lowering, runtime dispatch, export, reset, and tests. Search every caller of a shared helper before changing it.
- Write down the transformation, its eligibility conditions, the protected boundary, and a concrete counterexample it must decline.
- Choose the smallest complete milestone. Reuse existing graph operations, recorders, generators, and dependencies.

Implement the assigned slice, run meaningful checks, and stop at the agreed review checkpoint. Do not broaden admission rules, modify a schematic to make a pass succeed, or start another optimization without task authorization. Correctness defects in the baseline must be identified and fixed or excluded explicitly before using that baseline as an oracle.

## Every pass must explain itself

Put a concise module-level `//!` explanation in the pass file. Keep it updated when the implementation changes. It must answer:

| Question                | Required explanation                                                                |
| ----------------------- | ----------------------------------------------------------------------------------- |
| Why does it exist?      | The concrete repeated work or retained data it removes.                             |
| What does it rewrite?   | The input pattern and the resulting representation.                                 |
| When is it legal?       | Explicit conditions covering state, contacts, guards, pending work, and boundaries. |
| What remains general?   | Unsupported candidates, proof-budget limits, and deliberate exclusions.             |
| Why is it correct?      | The equivalence argument, with links or test names for its evidence.                |
| What does it depend on? | Required graph invariants and pass-order assumptions.                               |
| What does it cost?      | Discovery/proof complexity, temporary memory, and expected runtime benefit.         |

For a complex proof, keep the short argument next to the implementation and link a detailed document. Mark unproven or experimental status explicitly. Tests, fuzzing, exhaustive finite checks, and mathematical arguments provide different evidence; state precisely what each establishes.

Use descriptive function names such as `find_candidates`, `check_boundary`, `prove_rewrite`, and `apply_rewrite` when these are distinct steps. These are suggestions, not mandatory wrappers. Explain every new struct field and enum payload in one short sentence: what it represents and who owns its value. Comment semantic reasons and limitations rather than narrating obvious Rust operations.

## Use the existing pass API

| API                         | Responsibility                                                                        |
| --------------------------- | ------------------------------------------------------------------------------------- |
| `GraphPass::name`           | A stable, nonempty name, unique within the supplied pipeline.                         |
| `GraphPass::validate_input` | Check required whole-input invariants before mutation; an error stops compilation.    |
| `GraphPass::run`            | Discover, prove, and rewrite eligible candidates using the provided context.          |
| `PassContext::monitor`      | Cancellation and progress reporting.                                                  |
| `PassContext::options`      | Compiler settings; the supplied pipeline determines which passes run.                 |
| `PassContext::entry`        | Read-only entry clock and pending work, which must participate in eligibility checks. |
| `PassEntry::enabled`        | Enable or skip an individual entry without changing caller-supplied order.            |
| `PassPipeline`              | The exact requested pass sequence, including parameterized implementations.           |

The runner checks cancellation, validates the graph before execution and after each enabled pass, and records transformation statistics. It rejects empty or duplicate names, precondition failures, pass errors, and malformed output. Disabled entries do not validate or run that pass.

**A candidate that is not optimizable is not an invalid circuit.** Leave it unchanged and continue. Use errors for actual violated pass contracts or failed execution, not ordinary lack of an eligible motif. A proof timeout, excessive state space, unknown dependency, or unsupported optimization case retains general execution.

The API and custom compiler entry point remain crate-private. `O0` skips optimization passes. The default transformation order is:

| Tier | Pass | Work removed |
| --- | --- | --- |
| O1 | `guard-and-update-reduction` | Proven constant conditions and ineffective contributions. |
| O1 | `prune-inert-callbacks` | Delivery rows with neither an explicit receiver nor an ordinary dispatcher fallback. |
| O1 | `prune-unhandled-updates` | Live ordinary notifications whose fixed handler is an identity. |
| O1 | `index-turbo-callbacks` | Repeated searches through unrelated callback rows. |
| O1 | `contract-static-dust-paths` | General power queries on certified private dust paths. |
| O2 | `simplify-state-guards` | Redundant predicates on the same state field. |
| O2 | `specialize-wire-queries` | Repeated property/strength queries through compiled wire input expressions; whole Turbo walks whose distinct neighbors can never contain a wire. |
| O2 | `compile-wire-hops` | Per-walk hash indexing, full block-view construction during capture, and repeated neighbor-slot mapping. |
| O3 | `simplify-instant-resets` | Two conductor traversals in a private reset mechanism and general Turbo execution of its certified two-wire output. |
| O3 | `specialize-instant-pistons` | Exact fixed-conductor power reads in observer-reset mechanisms, including shared payloads and shared reset drivers. |
| O3 | `compile-builtin-resets` | Ordinary reset-observer execution and empty output-route dispatch, replaced by shared built-in reset clocks. |

The unoptimized pipeline still runs the complete general graph. Keep pass order explicit; do not silently reorder passes or turn a custom pipeline into the default. When adding a pass, add it to the corresponding optimization tier; do not introduce a dedicated setting for the pass.

Derived dust paths, Turbo offsets, wire-query facts and instant-reset plans are invalidated before enabled passes by default. Override a `preserves_*` method only with an argument that the affected references and assumptions remain valid. Disabled entries preserve metadata. Wire-query facts certify geometry, block class, possible wire inputs, and absence of neighboring wires, never strength or contact selection; their shared inference rule lives in `program_graph/wire_query_facts.rs` so graph validation can independently reject stale certificates. An isolated-wire certificate preserves the complete captured callback schedule, including duplicate deliveries and nested execution; it does not permit dropping those callbacks.

The private instant-output pass recognizes a closed graph pattern, not a schematic name. Its two-wire transfer removes internal wire callbacks, discovery, maps and queues while retaining every intermediate write and outside callback. The actor, resource, observer and global scheduler remain their existing state owners. Recognition deliberately declines shared resources, extra sources/readers, unsupported orientations and uncertain routes. O2 is the pass-off baseline and O3 enables this first family; recognition and execution checks live in [simplify_instant_resets.rs](simplify_instant_resets.rs) and [graph_instant_reset_tests.rs](../backend/graph/tests/instant_reset.rs).

Long discovery/proof loops must check cancellation at sensible boundaries. Compute eligibility and replacement data before mutating each candidate. The compiler stages the graph and publishes only after success; the runner does not roll back failed mutations. Do not clone an entire large graph merely to provide rollback, and never mutate the input World from a pass.


### Broader instant mechanism recognition

`specialize_instant_pistons.rs` follows reset power from a base-watching observer through a fixed conductor into QC. It accepts the adder's shared redstone/wool resources, downward actuators, shared reset observers and multiple power contributors. It recognizes all 13 ADDER1BIT actors and expands their fixed conducted input reads without changing state, event order or reset behavior. The existing private two-wire output certificate remains separate and retains its stronger geometry/ownership conditions.

The exact identity is `guard_outer ? max_i(guard_i ? source_i.saturating_sub(a_i) : 0).saturating_sub(a_outer) : 0 = max_i((guard_outer && guard_i) ? source_i.saturating_sub(min(15, a_i + a_outer)) : 0)`. Source strengths are four-bit; guards and reads are pure. No callback or scheduled work executes during either query, so both expressions see the same live state. Query equality at every operation preserves sampling, acceptance and subsequent execution by induction. Mutable contacts are retained as source reads; only immutable fixed conductors are expanded. Recursive conductor programs stay general.

Report recognized mechanisms, rewritten input owners, private output kernels and absorbed lifecycles separately. Observer-reset provenance does not authorize eliminating a lifecycle or replacing it with a truth table. Discovery/type rejection tests, rotated/transformed fixtures, real before/after runtime queries with multiple independent guards and strengths, and full adder differential/continuation tests establish this slice. The source catalogue and all-actor mechanism map are in [ADDER1BIT_INSTANT_RECOGNITION_RESEARCH.md](../../../../../docs/notes/redpiler-pistons-v4/ADDER1BIT_INSTANT_RECOGNITION_RESEARCH.md).

### Built-in reset clocks

`compile_builtin_resets.rs` is the first executable reset replacement. It converts recognized reset observers with matching raw orientation and no outgoing callback deliveries into `BuiltinReset { watched, facing }` nodes. Existing node IDs, component state and scheduler entries remain the single state authority. Shared observers become one shared clock; a separate clock per piston would duplicate reset ownership. Unsupported output circuits remain ordinary observers.

A qualifying notification schedules the original one-game-tick transition through the shared scheduler, which suppresses an already-pending matching entry. A rising transition schedules its falling transition before updating reset output, exactly as before. The clock writes the original powered/output state and marks the projected cell dirty. It has no output route. Correctly oriented reset-emission links become `PowerPort::Output`, retaining source, target, guard and attenuation. Fixed-component `Output`/`Stored` reads access scalar state directly; dynamic contacts and other directional/conducted queries retain their general path. Other weak/strong clock faces still return zero. Imported stale, duplicate, untyped and priority-sensitive work retains its original target identity and order. Projection continues to produce the original observer block and pending ticks; no Motion or second state owner is introduced.

The equivalence argument is local: (1) notification qualification is identical, including captured powered operands and `None` direction; (2) scheduler insertion, pending checks and ON/OFF updates are identical; (3) the skipped output route is empty; (4) block projection, power channels and handoff use the same state and IDs. This establishes the same transition after every input event, independent of an arithmetic protocol. Validation rejects a built-in clock with an output route or an incompatible raw observer state. Discovery and source/state tests are in `backend/graph/tests/builtin_reset.rs`; full adder and private-output continuation tests also execute the built-in path.

This lowering avoids ordinary observer handling, a duplicate pending lookup, empty-route dispatch and reset-source block reconstruction. It does **not** yet delete ingress notifications, skip scheduler transitions, absorb actor/resource actions or contract connecting dust. Retain and report those work counts honestly. The export preserves `kind: observer` as the projected identity and adds `builtin_reset: true`; the viewer labels it as a built-in reset. Older exports omit the additive marker.

### Compiled wire hops

`compile_wire_hops.rs` binds each unambiguous wire root and reuses its existing canonical neighbor IDs as direct hop destinations. It does not duplicate adjacency or store a second electrical state. Optimized walks reuse dense site-to-slot scratch, capture only profile/receiver/properties, and retain their discovered neighbor-slot arrays. Scratch is owned by an invocation; nested callbacks take another scratch record from the backend pool, and returning a walk clears only its discovered entries. General O0 walks retain sparse lookup as the comparison path.

The optimization preserves heading, layer queues, admission-time guards, captured versus live reads, intermediate writes, callbacks and scheduler work. It does not replace an arbitrary network with its settled voltages or collapse a whole wire chain into a single observable event. Those stronger reductions still need a region-boundary proof. See [COMPILED_WIRE_HOPS.md](../../../../../docs/notes/redpiler-pistons-v4/COMPILED_WIRE_HOPS.md).

The `preserves_wire_hops` hook defaults to false. Later passes retain root bindings only if component-site ownership remains valid; the three O3 instant/reset rewrites preserve these roots explicitly. Validation rejects dangling, ambiguous and wrong-owner roots. JSON adds optional `hop_root` references; older exports remain valid.

## Behavioral contract

Correctness means equivalence for every permitted control history from an admitted entry, including its saved state and pending work. Restricting a proof to binary inputs or a trigger/reset protocol requires proving that restriction or enforcing it at the boundary. One fixture's usual operating sequence is insufficient.

Preserve every applicable protected behavior:

- Directional and analog power, attenuation, Main/Side/Quasi channels, comparator overrides, and conducted reads with their dust policies.
- Update and Shape distinctions, ordered delivery occurrences, duplicates, same-value notifications, and live versus captured operands.
- Generalized dust's intermediate writes, nested Turbo walks, and the captured neighborhood used by a particular walk.
- Piston sampling, independent QC data and activation, FIFO requests, acceptance-time revalidation, cancellation, availability, and deferred publication.
- Shared-resource identity, contact selection, drop/recapture, revision validity, and stale-event handling.
- Scheduler membership, imported typed work, duplicates, priorities, phase boundaries, and deadlines.
- Synchronous command results, note/block actions and sounds, observable intermediate states, and the host's logical clock.
- State inspection, cell projection, reset, and interpreter continuation with equivalent future work.

Internal work counters and explanatory traces may change after a proved reduction. Define the comparison projection explicitly; do not weaken it to hide lost semantic events. Outside callbacks and host actions cannot be erased just because final outputs agree.

An unchanged power value does not prove that a delivered callback is redundant. A BUD can require an update while its QC value is unchanged. Equal saved outputs do not prove that two stateful actors can be merged.

## Proof discipline

Use the smallest proof method sufficient for the rewrite:

- **Local algebra:** show an identity over the full valid operand domain. For example, for the same complete source query, target channel, direction, and guard, `max(0, s - a)` dominates `max(0, s - b)` when `a <= b`. This permits removing the weaker power contribution, not a separately meaningful callback.
- **Structural rules:** inventory every reader, writer, callback, watch, shared owner, scheduler obligation, and projection dependency crossing a candidate boundary. Explain why the matched structure establishes the rule's assumptions.
- **Finite transition checks:** define a complete bounded state and input-label domain, then check related transitions and future observations. Include pending queues and captured operands when relevant. A truth table covers a pure function; it does not prove temporal equivalence.
- **Proof certificates:** when needed, retain enough checked facts to validate a specialized plan. Later rewrites must preserve those facts or invalidate/recheck the certificate before lowering.

Do not build a theorem-prover framework or add SAT/SMT dependencies for identities, finite strength domains, or small motifs. Exhaustive enumeration proves only the declared complete finite model. A queue bound must be justified; truncating a queue or input history changes the problem. Budget exhaustion must decline the optimization.

`CompileGraph::validate()` proves structural integrity, not behavioral equivalence. Matching the unoptimized Graph is useful, but both implementations can share a defect; keep independent interpreter checks for behavior-changing rewrites.

## Graph mutation hygiene

- Preserve single ownership of mutable component, piston, and resource state. Derived caches need explicit initialization and complete invalidation; they cannot become competing authorities.
- Remap every affected reference when compacting IDs: graph tables, guards, routes, contacts, plans, initial state, export, and lowering. Imported work currently binds by position during lowering; deleting a node must preserve an equivalent typed target or a proved no-op disposition.
- Update callback links and route occurrences together. Route sequences are nondecreasing native event groups, not necessarily unique or contiguous; guarded alternatives can share a sequence. Preserve surviving occurrence order and multiplicity.
- Do not leave stale indexes or cached proofs after mutation. Avoid ID-changing rewrites until their complete remapping and consumers are implemented.
- Keep discovery, proof, and output deterministic. Hash-map iteration must not choose semantic delivery order or serialized identity.
- Document idempotence or a well-founded progress measure for repeatable passes. Never run an unbounded fixed-point loop or silently revisit whole tables per candidate.
- Precompute reverse indexes for repeated dependency queries when needed. Reuse bounded scratch storage; release compile-time proof data before runtime unless a retained plan requires it.
- Preserve validated runtime bindings. Specialization should remove proved work rather than repeatedly checking the same static fact during ticking.

## Testing — evidence required before default enablement

Every nontrivial rewrite needs extensive coverage appropriate to its semantic scope:

1. **Rule tests:** positive cases, boundary values, invalid references, and negative near-matches. Assert the actual rewrite and its preserved invariants, not just that it returns `Ok`.
2. **Generated/property or fuzz tests:** exercise eligible and ineligible graphs and input histories with reproducible seeds. Record the seed, configuration, inputs, and first divergence; keep minimized failures as named regressions.
3. **Differential execution:** compare unoptimized Graph, optimized Graph, and the interpreter on meaningful stimuli and real pinned schematics when applicable. Compare ordered boundary observations and pending future work, not only final RAM.
4. **Integration:** test the pass alone, disabled, in its approved sequence, and around relevant neighboring passes. Verify cancellation, failure before publication, export, lowering, imported entry work, and reset/continue.
5. **Proof-checker witnesses:** where a checker or certificate is used, deliberately corrupt a condition, boundary, timer, or remap and demonstrate rejection.

Vary analog strengths, saved state, stale/imported ticks, repeated same-value updates, rapid trigger/reset histories, both action orders, shared ownership, captured/live disagreements, nested walks, and observer visibility as the rule requires. A failing safety condition must leave the original candidate executable.

Reuse the existing [Graph tests](../backend/graph/tests/components.rs), [seeded fuzz tests](../backend/graph/tests/fuzz.rs), [adder episodes](../backend/graph/tests/adder.rs), [continuation tests](../backend/graph/tests/continuation.rs), and [reference contract](../../redstone/instant_piston_tests/reference_contract.rs). Add focused generators rather than a second fixture loader or simulator.

Discover tests before running them. A command matching zero tests proves nothing. Run affected suites in debug and release with `--locked`; complete the broader Redpiler suite for cross-cutting rewrites. Check the server consumer when the API changes. Typical checks are:

```text
cargo test -p mchprs_core --lib redpiler::passes:: --locked -- --list
cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1
cargo check -p mchprs_core --tests --locked
cargo check -p mroww --locked
cargo fmt -p mchprs_core -- --check
git diff --check
```

Add the pass's actual discovery filter and any ignored fixture/fuzz runs to the review report. Do not claim that ignored tests ran with the ordinary suite.

## Runtime performance evidence

Runtime performance is the primary objective. Preparation/proof time is outside the execution benchmark, but temporary and retained memory must remain bounded. Report compile-time cost separately.

Use the existing [CPU benchmark](../../../benches/cpus.rs), fresh identical inputs, release builds, active work, and at least five paired samples. Keep setup, compilation, the starting stimulus, flush, and verification outside timing. Compare unoptimized Graph, optimized Graph, and the interpreter with the same episode. The current external bench cannot directly construct crate-private custom passes; use an approved default configuration or the smallest necessary reviewed selection interface.

Report medians and ranges, candidate coverage, runtime work counts, and retained program/state memory. Existing frozen counts describe the general baseline; a certified reduction needs new behavior checks and explicit before/after counts, not silently edited expectations. Fixtures and their hashes must remain unchanged; any already approved temporary normalization must be identical and disclosed for all compared inputs.

Pass durations, before/after node and link counts, and retained-table estimates remain preparation statistics. For detailed runtime attribution, use an external profiler on a separate run. Do not add nested profiler samples or treat them as exclusive time; table-byte estimates are not peak RSS.

Smaller tables alone do not demonstrate acceleration. Event counts alone do not show CPU attribution. Report an ineffective or slower optimization honestly; do not promise a speedup multiplier before measurement.

## Review handoff

The final report must state:

- The implemented rule, eligibility conditions, and why it is correct.
- Changed callers, validators, lowerers, runtime plans, export, and projection behavior.
- A positive real example and a counterexample the pass declined.
- Mathematical/checker evidence, registered tests actually run, fuzz seeds/budgets, and unresolved failures.
- Paired runtime results, work reduction, memory accounting, and candidate coverage.
- What was not checked, remaining risks, and in what opt effort is enabled.

Default enablement requires reviewed correctness evidence and measured benefit. Unsupported proof cases must retain the general graph; no Native/Direct fallback, TODO execution branch, guessed output, or broader unsupported circuit rejection may substitute for a proof.

## Experimental unordered dust settling

O3's separate `--stable-instant-inputs` option promises stable external inputs from the activating falling edge until reset, and composes admitted instant groups through connecting dust and variable-input fanout. Internal derived signals still change. Its ordered proxy preserves shared/reset memory; it does **not** enable unordered settling. Read [the contract, autonomous execution target and real-output counterexample](../../../../../docs/notes/redpiler-pistons-v4/STABLE_INSTANT_REGION_CONTRACT.md) before deleting internal work or changing the boundary.

`settle_dust_networks.rs` is deliberately **not** a correctness-preserving pass. Build core with `--features unsafe-turbo-fixed-point` to append it after the selected normal pipeline. The experimental runtime replaces ordered Turbo walks with final-state settling and one notification per boundary site. Seed writes and shape callbacks remain. Boundary routes remain available, while ordered-walk specializations are invalidated. This is off by default and is not an optimization tier; never enable it in a production server.

```text
cargo bench -p mchprs_core --bench cpus --features unsafe-turbo-fixed-point --locked -- --cpu cpu_bubblesort --graph-short --graph-tier 3 --iterations 1 --output target/cpu-dust-fixed-point.json
```

The solver discovers electrical wire neighbors, samples external power once, then builds local attenuation edges. It performs up to fifteen synchronous relaxation rounds in zero-initialized temporary strength arrays, publishing only final values. With fixed non-dust sources and one-level wire attenuation, four-bit power cannot travel beyond fourteen wire edges. Only final changed wires and the triggering root notify boundary sites, once each. It drops intermediate values, duplicate notifications and native order. Nested boundary callbacks can start another solve after the current one has settled.

The harness records interpreter mismatches in `fixed_point_comparisons` instead of enforcing parity in this build. A successful exit does **not** mean correctness. Ordinary builds retain strict comparisons. A stalled circuit is not a speedup. Tests check retained routes, structural validation and lowering, plus long-line rise/decay from every stale strength. Dynamic-contact histories, BUD behavior and instant circuits are not certified.

shortcut: the experiment rediscovers neighborhoods and samples general electrical inputs, compile compact guarded attenuation edges only after measuring useful coverage and proving the boundary contract.

Investigation results and the proposed ordered-boundary certification are in [DUST_FIXED_POINT_INVESTIGATION.md](../../../../../docs/notes/redpiler-pistons-v4/DUST_FIXED_POINT_INVESTIGATION.md). The repaired CPU episode matches sampled RAM but not complete future work; the ADDER1BIT FIFO counterexample prevents general enablement.

### Compiled wire transfer expressions

The existing `specialize-wire-queries` pass also compiles each wire's maximum-of-wire-strengths expression into `query_facts.transfer`. `seed` and `turbo` each contain a `direct` canonical-slot mask and a `conditional` diagonal-slot mask. Constant geometry folds step conditions; contacts remain dynamic where their selection matters. A slot is omitted only when every supported occupant is a non-wire or its geometry condition is false. Potential wire profiles and typed wire receivers both prevent that omission. The graph validator re-infers the whole certificate after every enabled pass.

Runtime reads the selected live wire strengths for a seed and the selected walk snapshots for Turbo. Strong conducted power retains its existing live semantics. The reduction changes no writes, callback selection, heading, FIFO work or reset ownership: pure maxima may reorder their operand reads because no callback executes inside either query. Separate seed and Turbo expressions preserve their different vertical and ascent rules. A malformed or stale expression is rejected before execution; absent metadata uses general queries. Rust, Python and HTML exports validate the additive masks, and older exports remain readable.

This is a first exact lowering of connecting dust calculations, not an atomic instant-region contraction or an electrical fixed-point solver. General Turbo still orders propagation. Evidence is in `program_graph/wire_query_facts.rs` (6,656 local geometry cases), `backend/graph/tests/wire_query.rs` (live/captured dynamic contact and strength checks), and `backend/graph/tests/instant_recognition.rs` (all 32 ADDER1BIT wires, all headings, changed snapshots). Preserving every intermediate write and outside callback deliberately leaves semantic work counts unchanged; benchmark the runtime cost rather than claiming event elimination.


### Pure optimization inside O4 kernel lowering

`mchprs_redpiler_opt` provides pure Boolean rewriting and known-input specialization. O4 lowering uses it to compile the admitted frozen-input kernel into unconditional ordered effect lists; it is not a new graph transformation pass. Keep graph ownership, timing, callback admission and state reconstruction in Redpiler. Pure rewrites must preserve ordered root occurrences and may not erase a dirty touch merely because its value is unchanged. The source kernel remains the validation/export definition; specialized lists are derived runtime data, reported separately in lowered storage estimates. See `COMPACT_INSTANT_KERNEL.md` for the current bounds and measured tradeoff.
