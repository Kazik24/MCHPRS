# Archived experiment: runtime learning of instant reset loops

> Archive status, 2026-10-18: C/LLVM and finite/symbolic synthesis source has been removed.
> Recover the last combined source from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> Historical results and plans below remain preserved; see
> [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current behavior and restoration details.

**Status: rejected for the compiler and removed from production code.** These are historical research notes, not instructions for an available pass or API. The required compiler is AOT: it must derive a symbolic virtual circuit before execution, with inputs represented as variables. Learning one concrete input history at runtime does not satisfy that goal.

The idea may still be useful for the interpreter: exact periodic-state detection can accelerate repeated work without claiming to derive a general circuit function. No interpreter implementation is included in this experiment.

## Observations and actual results

- The real ADDER11BIT fixture could be represented as one composed region. For the first prepared zero-operand case, a separate bounded census found the complete active normalized state repeating with prefix 0 and period 6 ticks. This was not merely a repeated output.
- The compiled six-phase body retained an estimated **8,568 bytes**. This counted body allocations; it was not the whole graph's storage or peak RSS.
- In a 288-tick comparison, including 32 preparation ticks and 256 post-trigger ticks, the prototype executed 281 effect-only steps and compiled twice: one idle preparation loop and one active post-trigger loop.
- General Graph versus the prototype: callbacks **71,115 → 1,782**, wire walks **851 → 21**, piston samples **14,785 → 376**. Logical scheduled ticks remained 850, accepted actions 860, and deferred publications 850. These are executed-work observations, **not measured speedups**.
- Four new non-ignored tests passed, including all **40 ADDER11BIT manifest vectors**, alternating data preparation order, arithmetic observation windows, complete component/actor/resource state, ordered future work, presentation effects, flush, revision wrapping, input invalidation, reset and native continuation. A separate ignored period census was also run and passed.
- The experimental snapshot passed the full Redpiler filter in debug and release: **369 passed, 0 failed, 10 ignored** in each. Debug took 134.60 s; release 79.64 s. These are test-suite durations, not circuit benchmark results, and do not validate the later partially removed tree.
- Logs: `target/instant-loop-final-debug-corrected.log`, `target/instant-loop-final-release.log`, and `target/instant-loop-ui-test-final.log`. Files under `target` are disposable local evidence, not durable fixtures; their essential observations are retained here.
- **No release performance benchmark was run.** No five-pair timings, sampled profile, whole-server throughput, or first-response speedup were established.

## Problems found during implementation

1. An idle loop during input preparation initially looked like an optimization hit. Its counter had to be separated from the post-trigger window. Holding inputs for hundreds of ticks can also exaggerate the practical benefit compared with changing data every reset.
2. The first effect collector incorrectly used Graph's input/component position index to find piston owners. That index deliberately excludes piston actors. Learning therefore declined at the first piston effect, while ordinary Graph execution remained correct. Reusing the existing actor-spec lookup fixed the prototype; the temporary helper extraction has been removed with the experiment.
3. A UI test hardcoded 16 passes. Adding the experimental pass made it fail despite correct reporting of 17. The independent test improvement remains: verify enabled/skipped mode semantics; exact registry names and order are tested in the pass registry.
4. The initial attempt to derive that UI expectation through `redpiler::passes` failed compilation because that module is private. The final test avoids exposing compiler internals.
5. A runtime trace contains concrete values for one input history. An input operation must invalidate it before changing state. This is a different architecture from a symbolic program parameterized by all data inputs.
6. The combined period of several clocks can be much larger than individual periods. Trace budgets can decline otherwise valid large circuits; this technique does not solve the desired scalable symbolic compilation problem.
7. Capturing and hashing full state costs time and temporary memory. Releasing proof snapshots after closure limits retained storage but does not eliminate warm-up cost.
8. Work counters have different meanings: callback/walk/sample reductions reflect omitted general execution, while action/schedule/publication counters retained logical effects. They cannot be added together or interpreted as a CPU profile.
9. The original rollback was interrupted after removing some plan/runtime integration. Remaining benchmark, export, test and helper references had to be removed together before checking the build again.

## Removal boundary and retained work

Removed: the `compile-instant-loops` builder, admission flag, runtime learner/body/probe, loop-specific regression module, public statistics/counters, JSON marker/capability and benchmark assertions/reporting. Temporary machine-state helper extraction was folded back into its original callers.

Retained: the pre-existing unified Graph executor, region composition, finite AOT kernels, single-actor symbolic routines/IR, sign-support fixes, reference tests and unrelated worktree changes. Removing this experiment does not complete or replace the outstanding symbolic AOT compiler.

Post-removal verification: `cargo check -p mchprs_core --tests --benches --locked` and `cargo check -p mroww --locked` passed. The fresh debug run `cargo test -p mchprs_core --lib redpiler --locked -- --test-threads=1` passed **365 tests, 0 failures, 9 ignored** in 116.51 s; log: `target/redpiler-after-loop-removal-debug.log`. The count decreased by the four removed experimental tests and one ignored census. A source search found no remaining experiment names, counters, APIs or temporary helper references under `crates`/`tools`. The post-removal release suite and full workspace were not rerun; the earlier release result above belongs to the experimental snapshot.

The next compiler must derive symbolic expressions, memory updates and ordered effects directly from graph semantics, then remove internal wire/reset/notification work through liveness and logical reduction. Stable external inputs do not prove internal order independence; the known adder counterexample still prohibits unordered propagation.

The required next implementation is specified in [SYMBOLIC_VIRTUAL_CIRCUIT.md](SYMBOLIC_VIRTUAL_CIRCUIT.md). It is a contract for work still to implement, not a claim that the symbolic AOT kernel already exists.

---

## Historical prototype design

The following describes the removed implementation. Names of passes, APIs and files below are historical and must not be reintroduced merely to make the old benchmark command work.

This O4 optimization traces a **closed selection under one stable external-input history**. It replaces an exactly repeated reset loop with state writes, relative pending work and ordered presentation effects. It does not enumerate operand combinations, recognize an adder by name, or derive an adder formula.

This is a bounded trace compiler, **not the general symbolic transaction compiler** described in `COMPACT_INSTANT_KERNEL.md`. The first response and the first complete loop use general Graph execution. This distinction matters when reporting performance: faster repeated reset ticks are not faster first-response latency or transaction throughput.

## Compiler and runtime ownership

`passes/compile_instant_loops.rs` is a read-only plan builder. It admits one composed stable-input region only when it owns every piston, contact, resource and wire. Allowed components are levers, constants, repeaters and built-in resets. Outside observer/wire readers, host effects and other component kinds decline. Existing finite kernels and single-actor symbolic routines retain their own path; loop tracing does not overlap them.

The plan records admission, not a fabricated ahead-of-time body. Lowering carries admission into `CircuitProgram`. `GraphBackend` learns a body on demand from the actual input history. Any external `use_block` or pressure-plate operation discards the body **before** changing state. Reset also discards it. Unsupported imported work disables tracing rather than losing work.

The authoritative component, actor and resource state remains in `CircuitState`; scheduling remains in `GraphFuture`. Compiled execution updates both directly. Flush and interpreter handoff need no private-state replay or reconstruction from a truth table.

## Algorithm

1. At a tick boundary, capture the complete normalized state. It includes all electrical and wire fields, actor poses/availability, resource contacts/enabled bits, ordered scheduler entries with priority and relative deadline, FIFO piston requests, and ordered deferred publications with relative deadlines and revision witnesses.
2. Execute one ordinary Graph tick, preserving the existing synchronous callbacks, Turbo order and action revalidation.
3. Record only its changed scalar values, revision increments, dirty owners, ordered piston presentation effects and resulting ordered future work. Values that change and return to their entry value still retain their revision increments and dirty/effect occurrences.
4. Compare the resulting complete state with previously captured states using hash lookup **and structural equality**. A repeated output or a hash collision is insufficient.
5. On a repeat, discard the prefix and every full-state proof snapshot. Retain only the repeated body's effect frames and its next-phase cursor.
6. Execute subsequent ticks by applying the selected frame. Internal callbacks, samples, power queries and Turbo walks are absent. Scheduled/action/publication counters retain their logical counts; callback/walk/sample counters show actual general execution, and `instant_loop_steps` counts replaced ticks.

The current construction limits are 128 traced ticks and an estimated 8 MiB of trace payload per input history. Exceeding either leaves execution on Graph. The estimate is not peak RSS: hash-table/allocator overhead and temporary capture buffers are additional. Limits bound compilation effort, not operand width. There is no three-input restriction or global input-mask table on this path.

## Why the loop can be reused

For an admitted closed selection, the Graph tick transition is deterministic. Its complete input is the normalized component/resource state and ordered future work. Immutable geometry/program tables do not change. No host callback reads the world or changes an input while a tick runs.

Absolute time and revision origins do not affect this transition: the admitted handlers compare deadlines with the current time and captured revisions with the current revision. Relative deadlines and wrapping revision differences preserve those comparisons. A frame reapplies the recorded wrapping revision increments and rebases future witnesses on the resulting live revisions. The fast path declines near logical-clock overflow.

Therefore, if `T(S)`, `T²(S)`, …, `Tᵖ(S) = S` under unchanged inputs, the same ordered body repeats. Each retained phase reproduces all boundary state, future work and presentation effects. Internal callback occurrences are not externally observed in this admitted family: outside readers and host effects have been excluded. This proof does **not** authorize reordering the original response, dropping input operations, or using the body after a changed input.

Different reset clocks can have a long combined period. Such circuits may hit the budget and decline. This compiler does not solve scalable symbolic compilation of arbitrary stateful regions; that remains separate work.

## Inspection and validation

- JSON export marks `instant_loop_tracing: true` and capability `instant_loop_tracing` when the selection is admitted. This is permission to learn, not proof that a runtime loop has closed.
- `Compiler::graph_instant_loop_statistics()` reports the current compiled period and estimated retained body bytes. It is absent while learning or after decline/invalidation.
- `GraphExecutionCounts::instant_loop_compilations` counts closed loops; `instant_loop_steps` counts ticks actually replaced. Inspect the **timed-window deltas**, since preparation can contain an idle loop.
- `backend/graph/tests/instant_loop.rs` uses the real ADDER11BIT schematic. Tests compare complete normalized memory/future, absolute actor/resource revisions, ordered presentation, dirty ownership, native projection and arithmetic windows. Wider vectors also exercise flush, changed inputs, reset and interpreter continuation.

The proposed benchmark command was (not run):

```text
cargo bench -p mchprs_core --bench cpus -- --graph-short --all-devices --component io/adder_11bits --graph-tier 4 --stable-instant-inputs --iterations 5 --label adder11-trace-loop --output target/adder11bits-instant-loop.json
```

The planned primary A/B was O3 with stable-input composition versus O4 with the same composition, retaining O4 without composition and interpreter for context. **The extra O3-stable executor was proposed but not implemented.** In this fixture O4 had no finite machine or single-actor symbolic routine, so that control would have isolated loop tracing.

The existing benchmark would have excluded preparation and the trigger operation; its following 512 tick calls would have included initial general execution, trace construction and compiled-loop execution. Prefix timing for the first 1/6/16 ticks was proposed **but not implemented**. Flush, observations, reset and native continuation would have been checked outside the timer. Such a result could not establish whole-server throughput or throughput with a new input every cycle. The temporary ADDER11BIT benchmark gate required a loop compiled **after the trigger** and executed in the timed window; that gate has now been removed.

No performance result exists for this prototype. The completed correctness results and limitations are recorded above.
