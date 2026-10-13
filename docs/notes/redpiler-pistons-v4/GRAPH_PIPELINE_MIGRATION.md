# One compiler graph, an extensible pass pipeline, and one Graph runtime

Status (2026-10-13): G1–G6 are implemented in this worktree and await human review; G7 is not started. This report does not claim prior human approval. The verification below covers focused redpiler, host, and CPU episodes, not every possible circuit history.

## Agreed direction

- Parsing belongs to the redpiler pass pipeline, not to a backend constructor.
- Ordinary components, dust, observers, pistons, and shared contacts use one general compilation representation.
- GraphBackend remains the general executor. Do not move it into Direct or wrap two unchanged executors and call that unification. Direct's useful compact representations become reviewed graph optimizations or specialized instructions inside Graph.
- The compiled runtime has Graph only. Unsupported input fails during preparation; preparation or execution errors never select another simulator.
- Batch 02 policy clarification: implement command and note blocks in Graph before retiring their legacy paths. Trapdoors are not required now; a structured compilation rejection is an explicitly accepted regression. Preserve existing command/note behavior during migration rather than rejecting those components to satisfy retirement.
- The pass API is general and supports developer-defined passes, custom ordered pipelines, enabling/disabling individual transformations and isolated testing. Production defaults remain deterministic; no pass implementation or enable decision is hardcoded into the runner.
- Preserve the interpreter's observable timing, callback direction/order/duplicates, live versus captured reads, acceptance/cancellation, shared resource ownership, and reset continuation.
- No runtime spatial block queries, physical piston executor, motion/progress model, or interpreted dust execution in Graph.
- Keep parser stages readable and independently debuggable. Work ordinarily stops after one step for review; batch 02 is the user's explicit exception for sequential G4–G6 work with one review at the end.

## Why the current structure needs changing

Public compilation, read-only candidate inspection, JSON export, and execution now share geometry, electrical, and callback preparation and the configurable graph-pass API. Lowering constructs the sole compiled executor, `GraphBackend`; its constructor consumes prepared data without World access.

The former electrical-only `CompileGraph` and its Direct/Native consumers have been removed. The general `CircuitDraft`/`CompileGraph` tables now carry callback, power, guard, contact, resource, component, and inspection data through passes and lowering.

## Target pipeline

```text
World + entry ticks + options
  -> read-only admission
  -> identify components and bounded piston geometry
  -> discover electrical channels and dust plans
  -> build ordered Update / Shape routes
  -> validate general CompileGraph
  -> configurable, ordered graph passes (optional rewrites)
  -> validate transformed graph
  -> export this graph, if requested
  -> lower once to program + initial state + imported work
  -> construct GraphBackend
  -> publish transactionally
```

The required parser stages run with or without `--optimize`. Skipping optimization must preserve the complete graph. Dust elimination moves out of identification and into a reviewed semantic optimization. Parsing is the required front-end phase of the redpiler pipeline, internally split into geometry, electrical and callback stages; it shares monitoring and diagnostics without forcing the same arguments as graph transformations. Optional graph passes operate on a complete CompileGraph. Parser stages may read the world; execution follows compiled references. Flush/reset are explicit boundary adapters.

## General pass API

The compiler supplies a default pipeline; developers can construct another pipeline programmatically and run individual passes in tests. Do not create a public per-pass command flag, plugin loader, dynamic library system, configuration language or general instruction VM as part of this work. The existing optimization flag selects defaults; the API permits experiments independently of that flag.

```rust
trait GraphPass {
    // This stable name identifies the pass in configuration and reports.
    fn name(&self) -> &'static str;
    // This rejects input graphs that do not satisfy this pass's particular assumptions.
    fn validate_input(&self, graph: &CompileGraph) -> Result<(), PassError>;
    // This applies the transformation to the sole construction graph.
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<PassOutcome, PassError>;
}

struct PassContext<'a> {
    monitor: &'a TaskMonitor,       // This shares cancellation, progress and statistics with the compilation job.
    options: &'a CompilerOptions,  // This exposes compiler settings without deciding which passes the runner enables.
    entry: &'a EntryState,          // This exposes pending work and entry time that constrain valid rewrites.
}

struct PassEntry {
    pass: Box<dyn GraphPass>, // This owns a built-in or developer-defined transformation without a closed pass enum.
    enabled: bool,           // This controls this transformation independently of other entries.
}

struct PassPipeline {
    entries: Vec<PassEntry>, // This fixes the explicit execution order, including disabled entries reported as skipped.
}

struct PassOutcome {
    remap: Option<GraphRemap>, // This records any identity changes needed to update retained compiler/projection references; None means IDs remain stable.
}
```

These are proposed minimal shapes. Pass implementations can hold their own experiment parameters. Validate unique stable names, check cancellation, report before/after counts and duration, and run each enabled pass's input checks before mutation. Validate the complete graph at pipeline entry/exit and after each pass in the experiment/test path; use existing graph validation rather than a new constraint engine. Passes update all references inside their graph and preserve pending-entry/projection references or return explicit remaps applied by the compiler. `GraphRemap` uses existing old-to-new mappings for affected typed tables and the removed-record dispositions required by Side B; introduce only mappings actually needed by a reviewed rewrite, not speculative copies of every table. Disabled entries do not mutate data. A failure discards the unpublished compilation; per-pass graph cloning/rollback is not mandatory.

Reordering is an experiment, not a promise that transformations commute. A failed precondition must identify the pass; sequence-specific differential tests remain required even when structural validation succeeds. Export/lowering are fixed output stages outside the reorderable transformation list. If all rewrites are disabled, parsing, validation, export when requested, and lowering still run. Source facts needed by a rewrite belong in immutable compilation metadata; no implicit reread of original blocks to resurrect removed nodes. Do not require World in the GraphPass API.

## Representation and memory ownership

These are ownership sketches, not instructions to introduce duplicate staging structures. Reuse the current types and rename only when their callers can be migrated together.

```rust
// Promote the existing CircuitDraft to the general CompileGraph.
// Rename today's electrical StableGraph to LegacyCompileGraph while it is needed.
struct PreparedCircuit {
    graph: CompileGraph, // This owns mutable construction tables, their initial component/piston/resource values, and source provenance.
    entry: EntryState,   // This owns entry time and imported pending work before execution is published.
}

struct EntryState {
    logical_tick: u64,              // This is the interpreter time at the compilation boundary.
    pending_ticks: Vec<TickEntry>,  // This retains imported deadlines, priorities, block types, ordering, and multiplicity.
}

struct LoweredCircuit {
    program: CircuitProgram, // This owns immutable dense descriptions and compiled query/route references.
    state: CircuitState,     // This owns each component, piston, and shared resource's authoritative mutable state.
    ticks: Vec<ImportedTick>,// This owns validated compiled tick targets and metadata ready for scheduler import.
    logical_tick: u64,       // This initializes the executor clock without runtime world queries.
}

struct ImportedTick {
    target: Option<NodeId>,     // This identifies a compiled receiver, or retains receiverless imported work when the contract permits it.
    position: BlockPos,         // This identifies the original tick for diagnostics and interpreter handoff.
    delay: u32,                 // This is the remaining imported half-tick deadline in the existing scheduler convention.
    priority: TickPriority,     // This preserves the interpreter's ordering class.
    registry_type: u32,         // This identifies the block type for which this tick was scheduled.
    current_registry_type: u32, // This records the entry target's type so stale work is retained until dispatch.
}
```

`CompileGraph` retains nodes, typed links, power queries, guards, contacts, routes, wire plans, comparator plans, immutable profiles, piston/resource descriptions and initial values from `CircuitDraft`. Preserve inspection-only wire relations/provenance in compilation metadata where needed by the existing JSON contract; do not make them a second runtime authority. Existing typed IDs may remain valid throughout construction if rewrites supply explicit remapping; do not force `StableGraph` merely to preserve its old name.

A lowered scheduler target must be derived during compilation. Currently `GraphBackend::import_ticks` reads the world for uncompiled targets; move that admission/metadata capture to lowering without losing stale ticks or duplicates. `ImportedTick` is a replacement for that boundary work, not an additional permanent queue.

```rust
struct GraphBackend {
    program: CircuitProgram,                         // This owns the immutable instructions and connections used by execution.
    state: CircuitState,                             // This owns live component strengths, piston availability, and resource selections.
    scheduler: TickScheduler<ScheduledNodeTick>,      // This owns priority-ordered ordinary scheduled work.
    pending: FxHashMap<ScheduledNodeTick, usize>,     // This indexes scheduler membership and multiplicity without owning another queue.
    piston_requests: VecDeque<PistonRequest>,        // This owns queued requests awaiting action validation in insertion order.
    deferred_pistons: VecDeque<DeferredPistonEvent>,  // This owns timed contact publications with cancellation revisions.
    clock: Clock,                                   // This records logical time and the current execution phase.
    positions: FxHashMap<BlockPos, NodeId>,          // This maps host controls and handoff positions to compiled node identities.
    component_sites: Vec<Option<SiteId>>,            // This identifies compiled sites for component queries without spatial discovery.
    dirty: Vec<bool>,                               // This tracks which component presentations need publication.
    effects: Vec<PresentationEffect>,               // This buffers sound/block-action presentation without delaying simulation feedback.
    counts: GraphExecutionCounts,                   // This reports work and is never consulted to decide behavior.
}
```

This reuses the current Graph executor's fields and behavior. Keep Turbo walk frames local/transient, including their sparse captured observations; no second persistent wire-strength table. Split existing Graph methods into readable modules only when needed; do not relocate the whole executor merely to preserve the Direct name. Port missing ordinary component semantics selectively, against the interpreter, rather than copying Direct's callback shortcuts.

`Runtime::Graph(Box<GraphBackend>)` is the sole compiled runtime variant. Interpreter execution remains the independent reference path. Unification does not mean every node carries piston state: piston/resource tables are empty for circuits without those behaviors.

Keep the existing scheduler and logical phases as the correctness baseline. Later optimization may collapse certified combinational or instant regions, but cannot invent observable nanotick delays or erase ordinary delayed behavior. The current passing tests are evidence for covered histories, not proof of general Graph correctness.

## Steps and review gates

| Step | Deliverable | Main review gate |
| --- | --- | --- |
| [G1](steps/graph-01-shared-preparation.md) | One shared parser orchestration outside backends | Same drafts, errors, read-only behavior, and runtime traces |
| [G2](steps/graph-02-common-graph-and-export.md) | General CompileGraph and artifact export | Complete typed graph; export matches this compilation |
| [G3](steps/graph-03-pass-api.md) | Extensible, configurable graph-pass API | Custom passes, order/enable controls, preconditions and accurate reporting |
| [G4](steps/graph-04-runtime-boundary.md) | Graph consumes lowered data and covers ordinary semantics | World-independent construction/execution and continuation |
| [G5](steps/graph-05-ordinary-parity.md) | Ordinary circuit differential audit | Comparator, feedback, dust and pending-work parity |
| [G6](steps/graph-06-activation.md) | Activate Graph for the supported scope and retire Direct/Native | Public lifecycle parity and explicit unsupported-case policy |
| [G7](steps/graph-07-safe-optimizations.md) | Restore reviewed compact electrical optimizations | Same observable semantics, explicit eligibility and performance evidence |

Each step includes discovery and focused tests. G5 expands ordinary coverage; it does not postpone all correctness checks until the end. Failures block changing selection for the affected cohort and retiring its legacy implementation. CPU-sized fixture preparation alone is not execution parity. G7 shares legality research with Side B and Step 15; it does not authorize later instant/BUD specializations.

The user authorized batch 02 for Luna6: G4–G6, including closure of the G1–G3 review findings. Its internal verification checkpoints replace intermediate human-review pauses for that batch; the final review and correctness/retirement gates remain.

## References and implementation discipline

Read `docs/REDSTONE_MODEL.md`, `docs/PISTON_MODEL.md`, `docs/INTERPRETER_ARCHITECTURE.md`, this directory's `ARCHITECTURE.md`, `AGENT_GUIDE.md`, and the assigned step. The interpreter remains the reference. Historical Direct/Native test names and reports are not proof that a Graph rewrite is correct.

Primary files: `redpiler/{mod,components,program_graph}.rs`, `passes/{mod,rich}.rs`, `analysis/{geometry,electrical,callbacks,export_json,graph}.rs`, and `backend/{mod,graph,queue}.rs`, all under `crates/core/src/`.

Preserve concurrent changes. Do not rename tests into step-number folders. Explain every new field/enum variant in one sentence, follow surrounding code, reuse the scheduler and ordered notification helpers, avoid speculative abstractions/dependencies, and prefer named steps over dense expressions. No optimization inferred solely from a saved pose or settled outputs. Do not silently weaken admission to make fixtures pass.

## Current implementation status and coverage matrix

G1–G3 are implemented in this worktree and await their first human review; this batch does not claim prior approval. The cancellation, Far-contact consistency, preparation separation, and pass-reporting findings are fixed with regressions. G4–G6 are implemented for the retained supported subset and await review. G7 has not started.

G4 constructs Graph from one validated lowered artifact. Imported work preserves original order and multiplicity, scheduled/current type, deadline, priority, and compiled receiver binding. Graph execution does not discover topology through World reads; `tick_with_world` publishes the logical tick at the host boundary. Command and note state, scheduling, synchronous command feedback, and note presentation remain in Graph; host command execution and presentation are explicit boundaries.

| Capability | Attempted and result | Explicit rejection or remaining limit |
| --- | --- | --- |
| Ordinary feedback, comparators, dust, controls, observers, bulbs | Existing Graph and interpreter differential episodes pass, including comparator feedback and ordered callback/presentation checks. Public tests assert Graph identity. | Exhaustive analog fork/reconvergence, every delayed loop, and every control history were not run. |
| Imported scheduled work and continuation | Matching, stale, duplicate, priority/deadline, uncompiled-air, type replacement, and reset/handoff cases are covered by focused tests. | Unloaded and matching non-air uncompiled targets fail admission; full scheduler wrap histories were not tested. |
| Commands | Impulse, repeating/automatic, conditional chain, synchronous success result, comparator feedback, host preflight, failure path, and reset handoff have Graph/interpreter checks. | Hostless work that may reach a non-chain command fails before consuming that work. Full host command failure/output matrix was not run. |
| Notes | Rising/held/falling power, blocked sound, sampled instrument/pitch, presentation eligibility, stale imported note work, and reset work have focused checks. | Exhaustive changes to above-block/contact topology and all sound/action variants were not tested. |
| Pistons, TEST family, adders | Existing shared-resource, head/base, transition, cancellation, continuation, TEST-family, and one-/eleven-bit adder episodes pass Graph/interpreter checks. | Full arbitrary piston histories and the entire CPU execution history are outside this batch. |
| Hoppers and trapdoors | Selected blocks and required external callback dependencies are rejected transactionally with type, position, and reason. | These components remain unsupported by policy. |
| CPU_BubbleSort sign/head | Saved schematic retains sign and block entities; the repaired east-facing head at `(181,14,166)` matches piston `(180,14,166)`. Missing/malformed head is a structured error. | The saved sign's support becomes `moving_piston` in an admitted state, so the exact selection returns a structured support-loss error. The 100-tick benchmark removes the same sign in memory from Graph and interpreter inputs only. |
| Public activation and exports | Public compilation, tick, flush, reset, analysis, and inspection now use the common Graph pipeline. `Runtime` has only Graph. JSON inspection remains supported. | Legacy binary/DOT flags return a clear prepublication error because those formats cannot represent rich callback/contact semantics. Dynamic external-wire boundary queries, moving entry, transported unsupported components, and unloaded dependencies remain structured errors. |

The pinned repaired schematic hash remains `803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611`. The independent interpreter reference has a 50,000-tick replay. An earlier explicit Graph/interpreter run with identical in-memory sign removal sorted RAM words 48–63 to `0..15` and matched RAM at ticks 0, 10, 100, 1,000, 5,000, and 10,035. This batch did not repeat the full sort.

The required release short benchmark was run before edits and after all executor changes, each with five fresh pairs of 100 active ticks and preparation, stimulus, flush, RAM checks, and counter checks outside timing:

| Executor | Before median (range) | After median (range) |
| --- | --- | --- |
| Graph | 4.215202 s (4.045955–4.592488) | 1.284687 s (1.241218–1.301638) |
| Interpreter | 0.626306 s (0.590885–0.736583) | 0.649654 s (0.642933–0.695880) |

Graph's median fell about 3.28× (69.5%). Graph remains about 1.98× slower than the interpreter on this short CPU episode; the interpreter measurements overlap the before/after ranges. Every final sample asserted `BackendKind::Graph`, matching RAM, and identical work counts: 20,005 scheduled ticks, 2,059,909 callbacks, 32,505 wire walks, 302,784 piston samples, 27,884 piston actions, and 27,884 deferred publications. Callback delivery is the largest counted repeated workload (about 20,599 callbacks per active tick); no per-function CPU profile was run, so the exact elapsed-time share is unknown.

Final retained-table estimates were 19.7 MiB after geometry, 194.7 MiB after electrical analysis, 411.08 MiB after callbacks, 405.3 MiB at lowering input, and 400.99 MiB after lowering. Largest callback tables were deliveries (133.0 MiB), guards (102.7 MiB), and callback links (64.0 MiB). These are estimated retained capacities, not RSS or peak allocation. Lowered estimates are about 77.2 MiB higher than the earlier 323.78 MiB artifact because the program now retains direct plan/contact bindings; narrowing the three contact-range arrays to `u32` endpoints reduced the estimate from 402.05 to 400.99 MiB. The historical ~42 GiB preparation report remains unexplained. A separate earlier test-only trace-retention spike reached 12.45 GiB; detailed text recording is now opt-in controllable and absent from release code. The production speedup comes from executor lookup/allocation/state-read changes, not from removing `cfg(test)` instrumentation.

## Verification record

`cargo test -p mchprs_core --lib redpiler:: --locked -- --list` discovered 202 tests. The debug and release redpiler suites each passed 198 tests with 4 ignored (512 filtered out). The specifically ignored release test `cpu_bubblesort_graph_and_interpreter_match_short_sign_free_episode` passed for 100 active ticks. The plot analysis summary test passed once; an earlier `--exact` filter omitted its module path and ran zero tests, then the non-exact filter ran the intended test. `cargo check -p mchprs_core --tests --locked`, `cargo check -p mchprs_core --bench cpus --locked`, `cargo check -p mroww --locked`, `cargo fmt -p mchprs_core -- --check`, and `git diff --check` passed. The exact benchmark command is `cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 5`.

The command trace checks synchronous command output before comparator feedback reads strength 1 in the same logical execution phase; the comparator's scheduled output then matches the interpreter. The note test captures a Bell at pitch 1 with category 2 and volume 3 on a rising edge, checks the powered block state against the interpreter, and verifies an occupied above site emits no sound. Reset continuation hands off a pending command tick, recompiles Graph, and compares command entity/output, comparator state, and pending work. Full workspace tests, broad fuzzing, full CPU replay in this batch, and per-function profiling were not run.
