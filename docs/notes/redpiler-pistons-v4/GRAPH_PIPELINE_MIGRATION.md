# One compiler graph, an extensible pass pipeline, and one Graph runtime

Status (2026-10-13): G1–G3 remain awaiting human review; their four review findings were fixed and checked in batch 02. G4 is partially implemented, G5 has focused but incomplete differential coverage, G6 is incomplete, and G7 is not started. This report does not claim prior human approval.

## Agreed direction

- Parsing belongs to the redpiler pass pipeline, not to a backend constructor.
- Ordinary components, dust, observers, pistons, and shared contacts use one general compilation representation.
- GraphBackend remains the general executor. Do not move it into Direct or wrap two unchanged executors and call that unification. Direct's useful compact representations become reviewed graph optimizations or specialized instructions inside Graph.
- Keep Direct and Native available during migration and review. Retire both after supported ordinary circuits have migrated and remaining admission-policy decisions are explicit. The final production runtime has Graph only, with no native fallback on preparation/execution failure.
- Batch 02 policy clarification: implement command and note blocks in Graph before retiring their legacy paths. Trapdoors are not required now; a structured compilation rejection is an explicitly accepted regression. Preserve existing command/note behavior during migration rather than rejecting those components to satisfy retirement.
- The pass API is general and supports developer-defined passes, custom ordered pipelines, enabling/disabling individual transformations and isolated testing. Production defaults remain deterministic; no pass implementation or enable decision is hardcoded into the runner.
- Preserve the interpreter's observable timing, callback direction/order/duplicates, live versus captured reads, acceptance/cancellation, shared resource ownership, and reset continuation.
- No runtime spatial block queries, physical piston executor, motion/progress model, or interpreted dust execution in Graph.
- Keep parser stages readable and independently debuggable. Work ordinarily stops after one step for review; batch 02 is the user's explicit exception for sequential G4–G6 work with one review at the end.

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

The user authorized batch 02 for Luna6: G4–G6, including closure of the G1–G3 review findings. Its internal verification checkpoints replace intermediate human-review pauses for that batch; the final review and correctness/retirement gates remain.

## References and implementation discipline

Read `docs/REDSTONE_MODEL.md`, `docs/PISTON_MODEL.md`, `docs/INTERPRETER_ARCHITECTURE.md`, this directory's `ARCHITECTURE.md`, `AGENT_GUIDE.md`, and the assigned step. The interpreter remains the reference; existing Direct and Native are regression subjects, not proof that a rewrite is correct. Historical scraps explain failures and are not replacement specifications.

Primary files: `redpiler/mod.rs`, `passes/mod.rs`, `passes/identify_nodes.rs`, `passes/input_search.rs`, `compile_graph.rs`, `program_graph.rs`, `analysis/{geometry,electrical,callbacks,export_json}.rs`, `backend/{mod,graph,native,queue}.rs`, and `backend/direct/{mod,compile,update,tick,node}.rs`, all under `crates/core/src/`.

Preserve concurrent changes. Do not rename tests into step-number folders. Explain every new field/enum variant in one sentence, follow surrounding code, reuse the scheduler and ordered notification helpers, avoid speculative abstractions/dependencies, and prefer named steps over dense expressions. No optimization inferred solely from a saved pose or settled outputs. Do not silently weaken admission to make fixtures pass.

## Batch 02 implementation status and coverage matrix

G1–G3 are implemented in the working tree and await the first human review; this batch does not claim they were previously approved. It closes cancellation for empty/all-disabled pipelines, enforces one piston Far contact binding, splits compiler preparation from admission/publication, and reports transformation records separately from wire elision. Their regression tests are included in the focused redpiler runs below.

G4's Graph construction boundary is implemented: the shared preparation/lowering artifact captures loaded target facts and imported work, lowers one position map, and passes a `LoweredCircuit` to a Graph constructor with no World argument. Imported work keeps its original order, duplicates, expected/current type, deadline, and priority. Graph handles command and note state, host execution, feedback, and presentation. G4 remains partial because the broader legacy capability inventory and runtime parity are unfinished. The hostless API conservatively asks for a host before any due work in a graph with a non-chain command block; this prevents partial command consumption while leaving circuits without command blocks hostless.

| Capability | Attempted | Graph admitted/executed | Specifically rejected | Unexpected rejection | Divergence | Incomplete |
| --- | --- | --- | --- | --- | --- | --- |
| Comparators, delayed feedback, dust, controls, observers, bulbs | Existing focused Graph regressions and selected interpreter comparisons | Covered episodes pass, including comparator feedback scheduling from a command result | External stored-wire boundary is rejected with a structured error | Parser can still reject uninventoried update receivers | None in the passing focused episodes | Full analog/fork/reconvergence/loop/control history matrix |
| Imported scheduled work | Matching, stale, duplicate, priority/deadline, uncompiled air, unloaded target, reset/handoff tests | Matching Graph work dispatches; stale/air work remains through due dispatch; duplicates and handoff deadlines are retained | Unknown/unloaded and matching non-air uncompiled receivers return structured errors | None in covered target classes | None reported by the focused cases | Full scheduler wrap and all type-replacement histories |
| Commands | Impulse, automatic/repeating, conditional chain, saved condition, synchronous result, hostless failure, missing entity, reset handoff | Focused Graph episodes pass; synchronous result is compared with independent interpreter output and comparator continuation | Missing host fails before potentially command-reaching due work; unsupported external command remains a host error | No unexpected rejection in covered command cases | None in the focused command/interpreter episodes | Every command failure/output mode, ordering/dedup case, and complete chain differential matrix |
| Notes | Rising/falling power, held power, blocked sound, sampled instrument, imported no-op tick, reset work | Focused Graph state/presentation checks and interpreter block-state comparisons pass | None | None in covered notes | None reported in the focused cases | Full sound/action comparison and changing contact/topology histories |
| Hoppers and other legacy update receivers | Direct/Native/interpreter handler inventory; Graph callback admission inspected | Not admitted as complete Graph behavior | No approved rejection policy | Some selections return `UninventoriedReceiver` although the interpreter handles them | Not run as Graph runtime episodes | Component implementation or explicit policy and complete callback parity |
| Trapdoors | Selected public compilation with piston forcing Graph | No Graph execution | `UnsupportedComponent` names trapdoor and position; input blocks/entities/ticks/events and activation remain unchanged | None; this is the explicitly approved regression | Not applicable | None for the selected trapdoor contract |
| Public activation / backend retirement | Public compiler and runtime paths inspected | Piston path remains Graph; ordinary path remains legacy | No new cohort rejection policy | N/A | G5 not established for ordinary public selection | G6 incomplete; Direct/Native and their production callers remain |

The table describes focused batch evidence, not a complete semantic guarantee. Existing fuzz tests that use public compilation while ordinary selection still chooses Native are not counted as Graph parity. General differential fuzzing, complete export-consumer migration, full-workspace checks, and performance measurements were not run. The older binary/DOT export consumers were not changed to carry command plans or all typed callback/contact semantics; they remain outside the activated Graph path pending a format-specific migration or rejection.

Focused validation discovered 208 redpiler tests with `cargo test -p mchprs_core --lib redpiler:: --locked -- --list`. Both `cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1` and `cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1` passed 205 tests with 3 ignored. `cargo check -p mchprs_core --tests --locked`, `cargo check -p mroww --locked`, `cargo fmt --all -- --check`, and `git diff --check` also passed. These results cover the focused redpiler suites, not the full workspace, broad fuzzing, or performance.

The command feedback trace asserts dispatch of the command, its output callback route, and comparator scheduling in that order. The host records `{"text":"[@] graph"}` synchronously and the comparator's live output query reads strength 1 in that same step; after the comparator's scheduled tick, Graph and interpreter stored output match. The note check captures a Bell at pitch 1 with category 2 and volume 3 on a rising edge, compares the powered block state to the interpreter, and verifies that an occupied above site produces no sound. The command reset test hands off its one-tick pending entry, compares the entry with the interpreter scheduler, recompiles Graph, then compares command entity/output, comparator state, and remaining ticks after continuation.
