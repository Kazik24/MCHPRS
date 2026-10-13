# One compiler graph, an extensible pass pipeline, and one Graph runtime

Status: proposed for review, 2026-10-13. This is a migration plan, not evidence of execution parity or authorization to implement all steps at once.

## Agreed direction

- Parsing belongs to the redpiler pass pipeline, not to a backend constructor.
- Ordinary components, dust, observers, pistons, and shared contacts use one general compilation representation.
- GraphBackend remains the general executor. Do not move it into Direct or wrap two unchanged executors and call that unification. Direct's useful compact representations become reviewed graph optimizations or specialized instructions inside Graph.
- Keep Direct and Native available during migration and review. Retire both after supported ordinary circuits have migrated and remaining admission-policy decisions are explicit. The final production runtime has Graph only, with no native fallback on preparation/execution failure.
- Command/note block rejection was described as future policy. Do not change their current admission incidentally during early refactoring. Before retiring legacy backends, explicitly resolve their treatment: reviewed Graph support or a reviewed structured rejection. Determine trapdoor coverage separately; do not preserve legacy execution forever to avoid this decision.
- The pass API is general and supports developer-defined passes, custom ordered pipelines, enabling/disabling individual transformations and isolated testing. Production defaults remain deterministic; no pass implementation or enable decision is hardcoded into the runner.
- Preserve the interpreter's observable timing, callback direction/order/duplicates, live versus captured reads, acceptance/cancellation, shared resource ownership, and reset continuation.
- No runtime spatial block queries, physical piston executor, motion/progress model, or interpreted dust execution in Graph.
- Keep parser stages readable and independently debuggable. An implementation agent completes one step and stops for review.

## Why the current structure needs changing

`Compiler::compile` currently branches for piston selections before the ordinary pass pipeline. `GraphBackend::compile` performs geometry, electrical and callback parsing, constructs `CircuitDraft`, lowers it, and constructs the executor. JSON inspection separately calls the same three parser stages. Consequently the compiled representation, optional rewrites, export and executor are not on one path.

The legacy `CompileGraph` is a `StableGraph<CompileNode, CompileLink>` with main/side electrical links. It cannot carry all the general graph's semantics. Changing a parser return type or putting the entire parser in `identify_nodes` would not fix this. The existing `CircuitDraft` tables are the starting point for the general compilation graph; retain their semantics rather than rediscovering them in the legacy representation.

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

The final runtime may retain the existing enum as `Runtime::Graph(Box<GraphBackend>)` alone to minimize host changes. During reviewed intermediate steps, Direct and Native coexist explicitly. Remove their variants and production implementations only after the Graph coverage/admission gate; preserve meaningful regression tests using the interpreter as oracle. Unification does not mean every node carries piston state: piston/resource tables are empty for circuits without those behaviors, and optimized nodes retain only needed data.

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

## References and implementation discipline

Read `docs/REDSTONE_MODEL.md`, `docs/PISTON_MODEL.md`, `docs/INTERPRETER_ARCHITECTURE.md`, this directory's `ARCHITECTURE.md`, `AGENT_GUIDE.md`, and the assigned step. The interpreter remains the reference; existing Direct and Native are regression subjects, not proof that a rewrite is correct. Historical scraps explain failures and are not replacement specifications.

Primary files: `redpiler/mod.rs`, `passes/mod.rs`, `passes/identify_nodes.rs`, `passes/input_search.rs`, `compile_graph.rs`, `program_graph.rs`, `analysis/{geometry,electrical,callbacks,export_json}.rs`, `backend/{mod,graph,native,queue}.rs`, and `backend/direct/{mod,compile,update,tick,node}.rs`, all under `crates/core/src/`.

Preserve concurrent changes. Do not rename tests into step-number folders. Explain every new field/enum variant in one sentence, follow surrounding code, reuse the scheduler and ordered notification helpers, avoid speculative abstractions/dependencies, and prefer named steps over dense expressions. No optimization inferred solely from a saved pose or settled outputs. Do not silently weaken admission to make fixtures pass.

## Current workspace note

An interrupted implementation added a provisional `passes/rich.rs` and its module declaration. They are unreviewed scaffolding, not G1 completion. Inspect their current consumers during G1 rather than treating them as an approved interface; other agents may have changed the working tree. This document supersedes the earlier generalized-Direct migration direction. No Rust implementation is authorized by this documentation update.
