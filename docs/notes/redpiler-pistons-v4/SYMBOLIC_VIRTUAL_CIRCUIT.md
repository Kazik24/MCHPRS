# Required AOT symbolic virtual circuit

Status: implementation contract; whole-region compilation is not implemented yet. The target is an **AOT symbolic virtual circuit derived from the instant graph**, not an input-mask table or a loop learned while the circuit runs. The rejected experiment and its observations are archived in [INSTANT_LOOP_COMPILER.md](INSTANT_LOOP_COMPILER.md). Its work-count reductions do not establish this compiler's performance or completion.

The stable-input contract freezes external data from activation through reset. Inputs remain variables in the compiled program: changing a legal operand must not require learning or recompiling a trajectory. The assumption does not authorize unordered internal execution. The existing stable-input adder counterexample still requires preserving causal order and live action revalidation.

## Semantic transformation

1. **Extract the interface.** Use existing region ownership and graph relations to declare data/activation ports, observable outputs, retained memory, ordered boundary notifications, deadlines and projection roots. Do not identify a build by filename, coordinates, hash, or an arithmetic formula.
2. **Execute semantics symbolically during compilation.** State reads return expressions. A conditional write updates its binding as `select(guard, new_value, previous_value)`. Later reads use that updated binding; they cannot be hoisted to the beginning of the phase. Model FIFO request deduplication, acceptance, shared-resource transfers and reset scheduling separately, following the same order as Graph.
3. **Compile internal propagation into dependencies.** Symbolic four-bit power uses maximum and saturating attenuation; geometry alternatives become guards. Derive the influence of internal wires and callbacks on receiving operations. The executable kernel must not contain opaque `WireWalk`, `ResolveSite`, or internal callback-dispatch operations that quietly run Graph again.
4. **Reduce the virtual program.** Begin backward liveness at outputs, necessary memory transitions, ordered boundary effects, scheduled work and cold projection roots. Share pure expressions, fold constants and simplify guards. Eliminate internal wire/reset/notification work only when its influence has been represented or proved dead. Equal expressions may share a DAG node; repeated effect occurrences keep their order and multiplicity.
5. **Lower once.** Publish an immutable expression/effect program and compact mutable registers/phase state. Runtime evaluates that program directly, including response and reset. Materialize private state from cold expressions for presentation, deoptimization or interpreter handoff, without replaying the original graph or consulting physical geometry.

The semantic program is a stateful transducer, not just `outputs = f(inputs)`: it also defines memory updates and future timed effects. Shared payloads retain one ownership state, rather than becoming independent bits on their former pistons. Observer-visible internal cells become explicit observable ports/effects before wire elimination. No physical Motion/progress simulation belongs in this kernel.

## Implementation boundaries

The current `analysis/instant_regions/symbolic_actor.rs` compiles conditions only. It is not the complete symbolic executor described above. The research state/effect DAG in `symbolic.rs` is also not production whole-region tracing. Both may supply existing operations and tests, but neither is evidence that ADDER11BIT has been contracted.

Connect graph-derived symbolic state, ordered execution and effect lowering end to end. Construction budgets may decline an uncertified region to ordinary Graph; they must not enumerate all global operand valuations, impose the finite prototype's three-data-input limit, or substitute concrete execution histories for a symbolic derivation.

The existing pass API remains the integration boundary: a dedicated read-only builder in `passes/` constructs and publishes validated virtual programs. Keep symbolic semantics and reduction outside the runtime; the executor implements the small virtual instruction set once. Do not add a separate block simulator/backend or a new runtime-learning mode.

## Review gates

- A minimal instant has compiler-derived response **and reset**, no internal wire walks, and no runtime learning. Tests independently compare memory, outputs, timing, boundary effects and handoff with Graph/interpreter.
- ADDER1BIT passes every operand case and legal prepared input order using one parameterized virtual program. Arithmetic formulas remain test oracles only.
- ADDER11BIT compiles without a global input-mask loop; inspect retained operations and verify that covered internal wires/notifications are absent from kernel execution. Test carry chains, alternating patterns, all manifest vectors and changed-input transactions.
- Benchmark the same composed graph with and without the AOT kernel, including activation and reset. Report latency/transaction work separately from long held-input windows, and identify compilation, projection and interpreter comparison costs explicitly.

Whole-region symbolic derivation, reduction, integration and the AOT ADDER11BIT performance result remain unfinished. The next implementation must meet these gates; an additional disconnected IR, condition-only handler or concrete-cycle cache does not meet them.

## Concrete symbolic compilation algorithm

Compilation uses expressions for **all declared input values at once**. It never runs a chosen concrete operand history to generate production code.

### Entry relation

Define the admitted entry relation explicitly: activation, external data, retained logical memory, readiness and pending-work requirements. Do not assume every piston is independently extended, or that every shared resource has the same owner. Initial symbolic bindings must come from the graph and the declared relation; a runtime entry guard may verify that relation without learning anything.

A stationary, electrically quiescent entry can express internal dust strengths as functions of its sources and topology. Positive wire attenuation bounds useful source paths to fifteen strength steps. A guarded max/attenuation closure can derive these entry expressions without enumerating input vectors. This only establishes entry values: it does **not** authorize fixed-point execution of the response, because transient ordering still matters. If quiescence or uniqueness cannot be certified, retain the necessary logical memory or decline this entry family.

### Symbolic state and ordered work

Use dense state bindings to expression IDs, not a vector of concrete snapshots. Keep actor/resource identity and finite control fields distinct from signal strength. A write under condition `g` changes the binding to `select(g, written, old)`; source-order reads use the current binding.

Represent work as a guarded FIFO of typed event occurrences. Appending two conditional occurrences retains their source order. Deduplication of a request is guarded by the negation of equivalent requests still queued for that valuation; it is not global removal of repeated expressions. Live acceptance reads the state at consumption, not at enqueue. Scheduling similarly preserves pending membership, priority, relative deadline and occurrence order.

Callbacks recurse in native order. Wire walks need separate symbolic capture environments: a site's first captured profile/strength is selected under the valuations that first visit it. Nested walks cannot reuse the outer capture. Conditional headings, topology and receiver variants must be guarded explicitly; choosing one heading from a concrete example is unsound. These environments are **compile-time scratch**, absent from the final virtual executor.

### Symbolic response and reset

Trace the admitted activation and subsequent scheduled phases symbolically. Retain outputs, memory writes, boundary effects and future obligations as expression roots. Pure internal updates have no execution obligation after their influence has been substituted into those roots.

Recognize reset/rearm as a symbolic control relation. Do not require retained data memory to return to its old value: a counter may have `q_next != q`. A reusable program returns to its control entry with updated registers. When folding repeated reset phases, prove expression/control equivalence across all admitted inputs, not equality for one evaluated vector. Canonical expression identities, Boolean equivalence checking and explicit invariant checks can establish these relations; proof-budget exhaustion declines the candidate.

### Reduction and lowering

Perform backward liveness separately for hot causal roots and cold projection roots. Derive compact logical registers where needed instead of retaining one runtime slot per old wire. A wire observed by a boundary observer contributes signal/update effects before it can disappear. If a wire's stored history is essential and has not yet been summarized, the compiler must not claim full contraction of that region.

Lower the reduced DAG and its ordered effects to one virtual instruction set. Validation checks typed references, state ownership, effect order, phase obligations and projection completeness. The runtime must not contain compilation buffers, input-mask tables, concrete proof trajectories or a learner. Report unresolved dependencies and declined regions so an uncontracted ADDER11BIT cannot be mistaken for success.

## Implementation checkpoint, 2026-10-15

The compiler-side semantic derivation now exists under `analysis/instant_regions/compiler/`. It is not registered in the production O4 pipeline yet. The old finite compiler remains the production whole-region path; this checkpoint does not change that fact.

`mchprs_redpiler_opt::symbolic::Diagram` constructs reduced ordered Boolean decisions with shared four-bit strength expressions. ITE uses a heap work stack, canonical identities reduce equivalent Boolean functions, and the result lowers into the existing pure `Circuit`/`Plan`. There is no runtime learner or input-mask loop in construction. Decision/memo tables and work are bounded; exhaustion declines construction. Variable ordering and construction cost still need validation on larger regions.

The graph semantic modules have these responsibilities:

| Module | Responsibility |
|---|---|
| `compiler.rs` | Guard reads, guarded contact alternatives, live caches, captured site values and construction work bounds. |
| `state.rs` | SSA component/actor/resource bindings, guarded writes and revision witnesses. |
| `power.rs` | Stored/live/captured power channels, maximum strength and attenuation. |
| `shape.rs` | Raw/regulated dust sides and exact neighbor-shape transitions. |
| `wire.rs` | Seed and captured-neighborhood strength expressions. |
| `walk.rs` | Compiler-only conditional captures, visited flags, directional bias, layers and ordered queues. |
| `routes.rs` | Synchronous ordered callback expansion into the receiving semantic compiler. |
| `work.rs` | Guarded request deduplication and the existing priority ring scheduler, reused during compilation. |
| `execution/` | Scheduled phases, live piston action acceptance, payload transfers, discrete connection publication and supported host-free component transitions. |

The traversal direction loop covers nine finite direction encodings, not global data valuations. A guarded FIFO occurrence is present only for its selecting valuations; a duplicate request is removed only where an equivalent request is still queued. Nested walks own separate capture environments. Those compiler structures must never become runtime kernel instructions.

Revision witnesses retain ordered conditional increment predicates and their source revision. Returning a payload to its original contact does not restore the validity of an old publication. Since construction cannot contain 2^64 writes, a witness is unchanged exactly when no increment since its stamp is selected. Exact wrapping revisions can be reconstructed from selected increments without introducing hot 64-bit expression registers.

Completed checks at this checkpoint:

- The optimizer crate passed 17 tests, including Boolean canonicalization, 22 symbolic inputs, 4096 nested decisions, all four-bit maximum/attenuation values, sequential writes and budgets.
- Both real adder graphs matched concrete Graph for initial guards and all electrical channels. ADDER11BIT's 23 control inputs remain independent variables in the read plans.
- Every legal pair of stored/raw dust shapes was compared for all six update directions: 6561 pairs per direction.
- One derived dust program per topology matched Graph for all 512 old-strength/new-strength/activation cases on a straight path and on a branched cycle, including ordered boundary occurrences.
- Conditional pending membership, priority ordering, rearming, request deduplication and shared-resource round-trip/revision semantics passed focused tests.
- An O0 ADDER1BIT derivation compiled all four controls as variables and twelve ticks of event semantics once. All sixteen verification cases matched full component/actor/resource state, wrapping revisions, pending membership, queued requests, deadlines and ordered accepted piston block actions. This is a bounded initial episode, **not** a reusable adder kernel or a prepared arithmetic-contract proof.

The first O0 ADDER11BIT event derivation reached the Boolean work budget. Repeating over O1 also exhausted the default 16,777,216 Boolean construction visits in tick one, with only 7,645 decisions and about 110,000 semantic visits consumed. This was a construction-work limit, not a large decision table. The event-derivation tests now allow 268,435,456 Boolean/semantic visits while retaining the existing 1,048,576 decision/memo-table limits; the public production pipeline's budgets are unchanged.

With that work allowance, both O1 graph episodes passed. ADDER11BIT kept all 23 control inputs symbolic and derived twelve ticks once; the arena contained 11,573 decisions, lowering retained 965 pure operations, 19,229 diagnostic roots and 1,668 guarded accepted-action occurrences. Fifty independent verification vectors (zero, all-one, alternating, every single set control and every single cleared control) matched Graph's full final state, revisions, pending membership, ordered future work and accepted action effects/deadlines. ADDER1BIT passed all sixteen control vectors with 163 decisions and 47 lowered pure operations. These roots deliberately include diagnostic internal state and revision observations; these figures are not a final hot kernel layout or a runtime speedup. Neither episode yet follows all prepared arithmetic fixture transactions or establishes a reusable entry/reset relation.

Remaining integration gates are explicit entry/memory relations, prepared activation and reusable reset/rearm, boundary/hot versus cold root selection, immutable kernel publication and validation, execution through O4, interpreter handoff, and release transaction benchmarks. Host commands, notes and sounds are not silently summarized by this compiler family: unsupported reachable effects must decline its admission while remaining supported by general Graph where already implemented.

Final checkpoint validation: `cargo test -p mchprs_core --lib redpiler --locked -- --test-threads=1` passed 380 tests with 9 ignored; `cargo test -p mchprs_redpiler_opt --locked` passed 17 tests. Focused rustfmt and `git -c core.safecrlf=false diff --check` passed. The new whole-event tests compare against Graph; prepared arithmetic transactions, direct interpreter/handoff validation of the AOT program, release verification and runtime benchmarking are still outstanding. No production activation, performance claim or completed-goal status follows from these results.

## Parameterized ready entry and reset checkpoint

`compiler/entry.rs` now derives a candidate ready-entry relation from graph semantics. It declares typed control and retained-memory inputs, derives dust strength with fifteen attenuating rounds, and solves ordinary electrical and main-driven actuator ready equations. QC-only actors retain their independent extension memory. Conserved resources with only one contact unblocked by the candidate heads have no independent position bit. Other resource positions remain symbolic. Nothing in this derivation uses arithmetic formulas, schematic names or global operand-mask enumeration.

This is an admission family, **not a claim that all stationary states satisfy these equations**. Before using a resulting program, admission must verify the entire projected component/actor/resource state, readiness, activation level, empty imported/scheduled/request/publication work, and every independent condition. Checking just the conditions or reading the input bindings is insufficient. `Entry::read_inputs` intentionally does not pretend to perform admission. Runtime publication of this admission check remains to be implemented. The typed bindings and local wire-shape codebook now live in `program_graph/instant_entry.rs`, so runtime admission can read them without depending on the symbolic compiler module; this move does not itself add runtime admission.

Two findings determined the representation:

- A quiescent wire can retain a shape different from fresh geometry. Replacing stored sides with freshly regulated sides already disagreed with a real prepared adder. `entry/shapes.rs` therefore closes each wire's possible stored shapes under the native six directional shape transitions and compiled profile alternatives. This enumerates at most 81 local shapes, never circuit input combinations. Only the small codebook ordinal is independent memory; fixed shapes need no register.
- One giant Boolean admission predicate creates an unnecessary Cartesian product. Independent occupancy, codebook and equilibrium conditions stay separate roots. Ready actuator equations plus conserved-payload occupancy reduce unreachable mechanical combinations before tracing events. The formerly failing one-bit response no longer requires enlarging the decision/memo-table budget.

The ready relation reconstructs all eight prepared ADDER1BIT cases and all forty prepared ADDER11BIT cases. It retains 45 and 460 declared control/memory reads respectively; dead reads disappear from the pure operation graph. Entry reconstruction lowers to 88 and 932 pure operations. A single symbolic sixteen-tick response per graph matches the full scalar state after every tick in those cases, lowering to 102 and 884 pure operations. ADDER11BIT construction retains 169,325 decisions in its scratch arena; this is not the size of the retained executable program. The new lowered-program input reader matches the draft-graph reader on all 48 prepared cases, including resource-contact and stored-wire-shape ordinals.

The extended test derives one thirty-two-tick program: sixteen ticks with activation low, then sixteen with activation high. It reuses that same program after rearming and changing all data controls. `tests/symbolic_observation.rs` checks full scalar state, wrapping revisions relative to actual entry origins, pending membership, ordered scheduler deadlines/priorities, FIFO requests, deferred actor **and payload** revision witnesses, and ordered accepted presentation effects after every phase. The existing Graph-only version passed all 48 cases and both activations; its diagnostic programs contain 444 and 6,749 operations including admission, every cold state and future, and presentation roots. These figures are not a hot-root layout or a runtime benchmark.

The final expanded test additionally compares every projected Graph/interpreter boundary and the fixture's arithmetic output windows. It passed in the broad debug suite: `cargo test -p mchprs_core --lib redpiler --locked -- --test-threads=1` completed with **384 passed, 9 ignored**. The 48 prepared cases each execute twice with different data through the same derived program, giving 3,072 checked response/reset phases. A separate regression checks that a QC-only actor's extension remains a symbolic memory input and an invalid activation ID fails without indexing outside the graph. `cargo test -p mchprs_core --lib symbolic_entry_tests --release --locked -- --test-threads=1 --nocapture` passed all **four** focused tests on the same source. Focused rustfmt and `git -c core.safecrlf=false diff --check` also passed. No timing from this instrumented correctness test is treated as a runtime benchmark.

The sixteen/sixteen timing is a **verification episode**, not a production reset deadline or an imposed user protocol. Remaining work is to prove reusable symbolic phase recurrence and reset/rearm for the admitted histories, select hot and cold roots, publish and validate the immutable program, replace the old O4 enumerator, and execute/hand off/benchmark the resulting kernels. No automatic runtime compilation or learner has been introduced. Public O4 behavior is unchanged by this checkpoint.

## Universal phase recurrence checkpoint

`compiler/cycle.rs` now proves recurrence using Boolean functions, not a set of sampled trajectories. Its compiler-only snapshot contains complete scalar state, pending membership, ordered scheduled work, FIFO requests and ordered deferred publications. Deadlines are relative to the current scheduler/logical clock. Actor and payload witnesses are independent: each records the exact selected increment count since its captured revision. This normalizes revision origins without forgetting stale work or return/recapture histories.

For identical normalized state and future, deterministic Graph execution under the same frozen inputs has the same subsequent effects. The candidate comparison is deliberately stronger than equality under a few prepared vectors: scalar and membership functions must be canonically identical, and queue equivalence must hold for every valuation of the declared symbolic inputs. No order-independence assumption is used.

The ordered-word proof has a small explicit contract:

1. Group conditional occurrences by immutable event identity: node/deadline/priority for ticks, actor/action for requests, and actor/deadline/direction for publications.
2. Prove occurrences of the same identity mutually exclusive. An identity may occur in many physical rows, but at most one can be selected by any valuation. If this cannot be proved, decline this proof family; do not deduplicate it. Structurally identical words already have the trivial identity proof.
3. Prove equal membership functions and equal selected payload values for each identity, including independent revision witnesses and optional resource presence.
4. For every pair of identities that can coexist, prove the same `before(A, B)` function. Statically separated intervals avoid constructing this function; interleaved alternatives retain their exact guard-dependent order.

For a fixed valuation, uniqueness and equal membership give the same finite set of identities. Equality of every pair's relative order gives the same sequence; payload equality then gives the same word. This proves the whole conditional word without assigning a global symbolic rank to every occurrence. Tests independently select the subsequences to check proof soundness, completeness for unique identities, meaningful duplicates and mutually exclusive reordered alternatives.

Earlier dense/sparse suffix-product comparisons and a global rank-count comparison consumed their budgets while constructing combinations irrelevant to the universal proof. They were removed rather than kept as extra compiler modes. The retained proof groups by local identity and rejects a candidate as soon as a counterexample is represented by a nonfalse canonical function.

Both prepared adders now have a **universal symbolic recurrence**:

| Graph | Prefix in ticks | Period in ticks | Scratch decisions | Diagnostic pure operations |
|---|---:|---:|---:|---:|
| ADDER1BIT | 5 | 12 | 5,142 | 383 |
| ADDER11BIT | 4 | 6 | 176,667 | 5,976 |

These figures precede the final dead-proof-work lowering edit; they describe diagnostic plans containing cold state/future/effect roots, not a final hot instruction layout. The periods are proven periods, not claims of the smallest possible periods. Construction uses one symbolic derivation per graph. Verification alone iterates the fixture vectors.

The new cycle regression replayed four periods for every prepared fixture vector, comparing full state, wrapping revision totals, ordered future work with captured witnesses, presentation effects, projected interpreter continuation state, and arithmetic output windows. Its 48 cases cover 1,544 phase boundaries. Periodic revision deltas are applied to entry origins; concrete snapshots are not used to derive the code or discover recurrence.

The Boolean library now bounds disposable ITE memo storage by eviction between operations while retaining canonical function identities and the cumulative work budget. It also lowers only root-reachable decisions to `Circuit`: dead proof expressions no longer survive indirectly in `Plan::original_to_slot`. Tests cover all two-variable ITE combinations across eviction, exact selected counts, bounded construction, and elimination of unreachable proof work before layout. On the final source, **20 optimizer tests pass in debug and release**, and `cargo test -p mchprs_core --lib redpiler --locked -- --test-threads=1` passes **387 tests, 9 ignored**. Focused rustfmt and diff checks pass. The new core recurrence tests have not yet been run in release; no runtime benchmark or full-workspace result is claimed.

### O4 runtime integration checkpoint

The production O4 pipeline publishes validated immutable `SymbolicKernel` programs into
`CircuitProgram`. Graph-derived activation candidates and typed entry reads are used in production;
fixture ports and arithmetic expectations remain test oracles. Admission evaluates the already
compiled Boolean plan once with live inputs before the falling edge. This is ordinary execution
of AOT code, without symbolic construction, input-mask enumeration or runtime learning.

The initial integration checked the complete ready state and empty future, executed the ordinary
falling-edge wave, then admitted the kernel only if its complete activation state, queues and
ordered effects agree. A cursor stores evaluated roots, the next phase, completed periods and
independent actor/resource revision origins. It preserves wrapping increments and converts captured
revision ages into the runtime's negative modular offsets.

The initial full-state phase executor passed both adder fixtures: all 8 one-bit and 40 eleven-bit
prepared cases, 48 ticks per case, full state and ordered future comparisons, piston effect order,
interpreter output/timing checks and an eight-tick interpreter continuation. Revision origins were
placed near `u64::MAX`. The old finite machine was removed from both test programs, so it could not
satisfy the kernel-hit assertions. Those tests also asserted no callback, Turbo walk or piston
sample work during admitted phase ticks.

The subsequent hot/cold split prebinds electrical scalar slots at compile time. Phase execution
applies those writes and ordered effects; full internal wire state, payload/actor values and future
queues are cold and reconstructed for projection, inspection or input interruption. Both fixture
tests passed in release for this split before the subsequent direct-activation edit. The retained source graph still provides
general execution after deoptimization; covered wire propagation is absent from kernel ticks.

Direct compiled activation now replaces the initial falling-edge wave too. Entry-relative revision
roots establish the ready-state baseline before the input changes; activation and periodic frames
restore independent live origins modulo 2^64. Both final-source fixture differential tests pass and
assert zero callback, Turbo walk and piston-sample increments on activation as well as phase ticks.
The three symbolic projection tests also passed: input interruption, repeated/mixed flush modes,
interpreter continuation and shifted/wrapping revision origins. The strengthened interruption test
also changes an operand, rearms through the ordinary graph, then confirms fresh AOT admission and
24 further kernel ticks before interpreter handoff at each of five interruption cuts.

Verification on the direct-activation source: the two adder tests pass in debug (48 prepared cases,
2,304 admitted phase ticks plus 48 compiled falling edges); the three projection tests pass, and
the expanded rearm test passes separately. Both adder tests passed release before the final
direct-activation change; that result does not validate the exact final release source. No runtime
benchmark has run for this integration. The source tree retains unrelated uncommitted edits.

Remaining work:

- Expand interruption, rearm, flush-mode and clock-wrap tests for the symbolic cursor, and complete
  final debug/release differential validation after direct activation.
- Remove the old finite enumerator, mask-dependent lowering and cursor; migrate JSON inspection and
  update memory accounting. Its three-input restriction does not constrain the symbolic programs.
- Benchmark real release transactions for both adders, including activation/reset timing, kernel hits
  and removed work. Keep preparation, proof and projection outside inference timing and report their
  placement explicitly.

No release speed result or completed-goal claim follows from this checkpoint.
