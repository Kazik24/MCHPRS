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

1. Every optimization pass **must have its own `.rs` file** under `crates/core/src/redpiler/passes/`. Use a descriptive name such as `simplify_guards.rs`, not a batch number.
2. `mod.rs` holds registration and pipeline infrastructure. Do not implement unrelated transformations inside the runner, parser, compiler orchestration, or backend dispatch.
3. Keep pass-specific discovery, proof, and rewrite helpers private to that pass. Break complex work into small, named functions and intermediate variables.
4. Common helpers used by multiple passes may live in `passes/utils.rs`, or `passes/utils/` when several focused modules are needed. Extract a helper when real consumers exist; do not create a generic framework for hypothetical passes. Utilities must not silently run another pass or choose pipeline order.
5. Keep focused tests in the pass's `#[cfg(test)]` module. A larger suite may use a clearly named sibling test file in this folder. Register it so Cargo actually discovers the tests.
6. Runtime instructions, lowering support, and validators belong in their existing layers when a rewrite requires them. A pass must not introduce a second simulator or a fallback backend.

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

Currently, the API and custom compiler entry point are crate-private, the default transformation list is empty, and `--optimize` performs no approved rich rewrites. When registering a reviewed default pass, make its enablement follow the compiler option and update tests and documentation. Keep supplied experimental order explicit; do not silently reorder passes or turn a custom pipeline into the default.

Long discovery/proof loops must check cancellation at sensible boundaries. Compute eligibility and replacement data before mutating each candidate. The compiler stages the graph and publishes only after success; the runner does not roll back failed mutations. Do not clone an entire large graph merely to provide rollback, and never mutate the input World from a pass.

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

Reuse the existing [Graph tests](../backend/graph_tests.rs), [seeded fuzz tests](../backend/graph_fuzz_tests.rs), [adder episodes](../backend/graph_adder_tests.rs), [continuation tests](../backend/graph_continuation_tests.rs), and [reference contract](../../redstone/instant_piston_tests/reference_contract.rs). Add focused generators rather than a second fixture loader or simulator.

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

Measure speed in an uninstrumented build. `graph-profile` helps locate work but changes the measured execution. Timings are inclusive and overlap; never add nested buckets or call them self-time. For cumulative profiling, snapshot immediately before and after the timed window and report the difference. Table-byte estimates are not peak RSS.

Smaller tables alone do not demonstrate acceleration. Event counts alone do not show CPU attribution. Report an ineffective or slower optimization honestly; do not promise a speedup multiplier before measurement.

## Review handoff

The final report must state:

- The implemented rule, eligibility conditions, and why it is correct.
- Changed callers, validators, lowerers, runtime plans, export, and projection behavior.
- A positive real example and a counterexample the pass declined.
- Mathematical/checker evidence, registered tests actually run, fuzz seeds/budgets, and unresolved failures.
- Paired runtime results, work reduction, memory accounting, and candidate coverage.
- What was not checked, remaining risks, and whether the pass is experimental or enabled by default.

Default enablement requires reviewed correctness evidence and measured benefit. Unsupported proof cases must retain the general graph; no Native/Direct fallback, TODO execution branch, guessed output, or broader unsupported circuit rejection may substitute for a proof.

## Scoped optimization projects

These documents provide implementation milestones and concrete proof requirements. They remain scopes until their assigned implementation and review are complete:

- [01 — guard and update reduction](../../../../../docs/notes/redpiler-pistons-v4/steps/opt-01-guard-and-update-reduction.md).
- [02 — dust contraction](../../../../../docs/notes/redpiler-pistons-v4/steps/opt-02-dust-contraction.md).
- [03 — piston specialization](../../../../../docs/notes/redpiler-pistons-v4/steps/opt-03-piston-specialization.md).
- [04 — instant-region contraction](../../../../../docs/notes/redpiler-pistons-v4/steps/opt-04-instant-region-contraction.md).
- [05 — removed legacy passes and safe equivalents](../../../../../docs/notes/redpiler-pistons-v4/steps/opt-05-legacy-pass-migration.md).
