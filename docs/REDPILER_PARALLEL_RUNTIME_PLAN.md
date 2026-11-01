# Opt-in parallel Redpiler runtime

## Objective

Introduce an experimental runtime selected by `/rp compile --parallel`. It processes work concurrently within explicitly defined update rounds and preserves scheduled deadlines and phase barriers. It deliberately relaxes picotick callback order. The existing serial runtime remains the default and the compatibility reference.

This document scopes implementation; it does not claim a speedup or native equivalence. The ZCore profile places 92.7% of sampled kernel time in piston event processing and motion completion. Parallelizing only scheduled ticks, which account for 7.3%, would leave most of that workload serial.

## Rayon example and its implications

Source: [Rayon Fibonacci example](https://github.com/rayon-rs/rayon/blob/main/rayon-demo/src/fibonacci/mod.rs).

The example compares recursive `rayon::join`, recursive splitting with a sequential leaf computation, and a sequential iterative algorithm. Its unequal branches illustrate load balancing; its deliberately tiny jobs demonstrate scheduling overhead. Apply those lessons by dividing meaningful batches, using sequential leaves, and measuring against the existing efficient implementation. Do not create a Rayon job for every contribution flip or callback.

Rayon is already declared in `crates/core/Cargo.toml`. Use a persistent, shared runtime pool with bounded worker count, rather than creating threads per tick or a pool per plot. Keep host publication outside worker tasks.

## 1. Define the relaxed execution model first

The execution contract is now defined in [Parallel Redpiler execution semantics v1](REDPILER_PARALLEL_SEMANTICS.md). That specification is normative for selection, visibility, notification multiplicity, priority boundaries, piston conflicts, and failure behavior. The implementation sections below describe how to realize it.

Each round reads one committed state and commits atomically. Select at most one event per component, preserve required notifications and scheduled entries, and coalesce only classified level reevaluations. Settle electrical consequences before the next scheduled batch or mechanical frontier. Deadlines and phase order remain explicit; simultaneous changes can eliminate pulses that exist only through serial interleaving. Canonical IDs resolve scheduling and mechanical conflicts independently of worker timing.

Call these **parallel update rounds** in code and diagnostics. Claim nanotick fidelity only after mapping this model to the native nanotick definition and demonstrating it. This is low-level parallel execution within circuit phases, including connected circuits; whole disconnected-circuit partitioning alone does not fulfill the objective.

### Determinism

Given the same input and admission result, 1, 2, 4, and more workers must produce identical parallel-mode states at barriers. Parallel mode may differ from the existing serial/native runtime in documented ordering-sensitive circuits. A one-worker implementation of these round semantics is the correctness oracle for the parallel executor.

## 2. Activation and interfaces

Extend `CompilerOptions` with an execution selection:

```rust
enum ExecutionMode { Serial, Parallel }
// CompilerOptions: execution: ExecutionMode; default Serial
```

Accept `--parallel` in the existing Redpiler compilation command, completion, and help. Keep publication flags orthogonal. Show the selected execution mode in compile diagnostics/sidebar or current-flags reporting. The command flag opts into altered semantics for that compilation; it is not enabled globally by introducing the dependency.

Reuse the existing compiler for structural discovery. Add a parallel preparation pass after the program is validated to generate ownership and dependency indexes. Represent the selected executor explicitly behind the `Compiler` facade:

```rust
enum Executor {
    Serial(Runtime),
    Parallel(ParallelRuntime),
}
```

Adapt tick, control input, publication, pending-entry export, reset, and statistics through this facade. Avoid a parallel branch per contribution inside the existing serial hot loop. An additional Cargo feature can gate packaging later if required; the requested opt-in is the circuit compilation flag.

Audit explicit `CompilerOptions` literals, flag parsing/completion tests, diagnostics, fixture constructors, and tests accessing `runtime` directly. A preparation or admission failure must reject compilation with a precise reason; do not silently change execution mode.

## 3. Data model and ownership

### Immutable compiled plan

```rust
struct ParallelPlan {
    component_owner: Vec<usize>,
    cell_owner: Vec<usize>,
    contribution_owner: Vec<usize>,
    shards: Vec<ShardPlan>,
    // immutable source/target adjacency, token groups, and movement plans
}
struct ShardPlan {
    components: Vec<usize>,
    cells: Vec<usize>,
    read_dependencies: Vec<usize>,
}
```

Assign contribution ownership to its target's owner, so input counts and histograms have exactly one writer. Assign each projection cell exactly one owner. Colocate a piston contact group and its tokens where practical; overlapping ownership domains must merge or use the explicit arbitration described below. Shards are scheduling/storage units and may have dependency edges to one another. Use graph-local placement to reduce cross-shard traffic, then balance estimated operation cost, not simply component counts.

### Mutable state

```rust
struct ParallelRuntime {
    plan: ParallelPlan,
    shards: Vec<ShardState>,
    clock: RuntimeClock,
    scheduled: RoundScheduler,
    piston_coordinator: PistonCoordinator,
    failure: Option<ScratchError>,
}
struct ShardState {
    // owned component/cell states, input aggregates, contributions
    ready: Vec<Event>,
    effects: Vec<Effect>,
    next_ready: Vec<Event>,
    dirty_cells: Vec<usize>,
}
```

Use compact typed event/effect enums. Examples: `EvaluateComponent`, `ScheduledTick`, `MovementArrival`, `MovementDeparture`, `SetContribution`, `SetBlockState`, `ScheduleTick`, `Notify`, and output emissions. IDs remain stable during execution. Include a stable producer ID and local sequence on effects that need deterministic ordering. Preserve captured event payloads where using the latest block state would lose an edge.

For the first semantic prototype, use a frozen round snapshot and reusable effect buffers. Measure full-state copying explicitly. For the production layout, read immutable committed shard state during evaluation and commit only after all readers finish; workers return effects, so evaluation needs no mutable access to committed state. Reuse buffers. If later owner-local evaluation requires mutations, separate committed arrays from scratch state and export only the ghost values needed by other shards. Do not copy the whole `Program` or whole runtime every round.

## 4. Exact round algorithm

Implement the algorithm sequentially first:

```text
select the next eligible priority frontier at current logical time
while frontier is nonempty:
    evaluate each event against committed state -> ordered local effects
    validate effect ownership and detect exclusive-write conflicts
    route effects to destination owners
    commit contribution and state effects using defined conflict rules
    capture changed outputs and resulting notifications
    install new scheduled entries with unchanged absolute deadlines
    build the next eligible frontier, preserving required event multiplicity
    charge work; stop on failure or convergence limit
```

### Evaluate

Replace recursive mutation through `update`, `execute_output_program`, `set_token_pos`, and `run_move_steps` with evaluation that emits effects. Reuse pure block logic where it already exists. Operations that depend on an earlier mutation in the same serial callback become a next-round dependency. Never run the current methods concurrently against one `&mut Runtime` or hide shared mutation behind raw pointers.

### Commit and conflicts

- A fixed or conditional link has one logical contribution slot. A component evaluation writes each of its source-owned outputs once per evaluation. Resolve legitimate multiple events for one source in their defined local sequence; forbid ambiguous multiple writers to the same contribution slot during admission or route them through the component owner in sequence.
- For each target, apply the resulting old-to-new contribution changes to its active count/histogram under its sole owner. Queue reevaluation after all changes for that round have committed.
- Aggregate counts are reductions, but replacements are not plain additions: retain the previous value of each contribution slot so decrements and repeated writes remain correct.
- Detect state-write conflicts instead of applying arbitrary last-worker-wins behavior. For ordinary component state, the component owner evaluates its pending local events in the specified order.
- Keep command output, notes, and scheduling effects separate from coalesced state changes. Merge ordered outputs by logical phase, round, producer component ID, and producer-local sequence. This gives reproducibility, not original global picotick order.

Track round count and charged operations. Preflight a whole round against the global budget and capacities; reject it atomically on exhaustion, reporting the first offending canonical effect or dispatch. Stop publication/handoff at a consistent committed and electrically settled boundary. A failed frontier that cannot be exported to native execution retains compiler ownership. Evaluation must not leak effects directly into host state.

## 5. Piston-specific parallelism

This is the central experiment for ZCore, because movement dominates its kernel.

1. Evaluate eligible piston event conditions against committed state in parallel. Produce `PistonProposal` records containing actor, action, token, read dependencies, claimed cells, and intended motion transition.
2. Nominate one request per contact group in canonical actor/event order. Resolve cross-group read/write conflicts using the specification's deterministic coordinator: conflicting valid requests remain pending for the next request round, while invalid requests are consumed. Never validate one proposal against another worker's partially applied state.
3. Commit accepted departures together. Update token occupancy and motion records before evaluating movement notifications in subsequent rounds.
4. Expand movement plans into contribution and notification effects. Compute independent token/group work in parallel, but give a joint arrangement group one owner. Multiple token moves affecting that group must compute one coherent final arrangement and preserve required departure/arrival event payloads.
5. Process due motion completions similarly: collect, resolve landing conflicts, commit occupancy/state, then run notification rounds. Preserve logical landing deadlines and interrupted-motion rules.
6. Move conflict-free groups from the serial coordinator to owner-local processing only after measurements show arbitration cost matters. Keep cross-group claims coordinated.

Do not parallelize direct calls to current `set_token_pos`: they mutate group arrangement state, contribution aggregates, wire power, phase stamps, dirty lists, and motion-related state. Relaxing callback order does not make those simultaneous writes safe.

Initially retain serial arbitration for piston proposals while parallelizing expensive notification/contribution processing. If one large joint group or global settling chain dominates, the available parallel speedup will be limited; record that result rather than weakening conflict rules implicitly.

## 6. Rayon scheduling and task granularity

Use `pool.install` around a round or a batch of rounds. Evaluate work with parallel iteration over sizable event chunks, or recursively split an event slice with `rayon::join` and a sequential cutoff:

```rust
fn evaluate_batch(events: &[Event], snapshot: &Snapshot) -> BatchEffects {
    if estimated_work(events) <= cutoff {
        return evaluate_serial(events, snapshot);
    }
    let (left, right) = split_by_estimated_work(events);
    let (a, b) = rayon::join(
        || evaluate_batch(left, snapshot),
        || evaluate_batch(right, snapshot),
    );
    merge_in_input_order(a, b)
}
```

Choose either the slice/chunk or recursive implementation first, based on measured imbalance. The pseudocode's merge must reuse buffers or collect indexed results; repeated allocation and concatenation can erase gains. Return worker/task-local results, route them once, and commit target-owned batches in parallel where disjoint mutable borrowing is expressible safely.

Estimate work from active flips, movement steps, and fanout. Calibrate cutoff from measured task cost; do not borrow Fibonacci's recursion shape or a magic task size. A small frontier uses sequential execution with the same relaxed semantics. Never switch back to original serial semantics midway through a compilation because frontier size changes.

Use a shared bounded pool across plots; account for concurrent plot workers and compiler/probe threads. Benchmark 1/2/4/8 workers and multiple active plots. Rayon's work stealing can balance uneven independent tasks, but it cannot shorten a causal dependency chain. Record scheduling/barrier time separately.

## 7. Runtime lifecycle and publication

Apply control inputs only at committed barriers, then enqueue their consequences. The plot thread retains ownership of player/world interactions. Publish at the existing visual cadence from committed state, with no worker touching `PlotWorld` or packet senders.

On reset or save/handoff, finish the current round, join workers, export coherent blocks/entities, pending ticks, piston motions/events, command state, and effects, then relinquish ownership. Preserve existing export validation. Do not continue native simulation from a partially committed parallel round. On failure, discard uncommitted effect buffers and retain the last coherent state plus a structured failure; define how the existing reset path exports that state.

Command blocks have shared output limits/state. Initially evaluate their output state changes through a deterministic coordinator while allowing their ordinary signal reevaluation in the round engine. Preserve deadline, queue-capacity, and output-limit checks; worker-local copies of global quotas would incorrectly multiply allowed output.

## 8. Implementation stages and affected code

1. **Semantics prototype:** add round event/effect types and a one-worker oracle. Implement basic electrical components, contribution commit, priorities, and settling. Provide a test-only path before exposing the command flag.
2. **Movement support:** implement coordinated piston proposals, arrivals/departures, joint arrangement ownership, motion completion, and notification rounds. Admit only fully supported circuits with clear errors for exclusions.
3. **Rayon executor:** parallelize evaluation and disjoint commit; introduce measured cutoffs and reusable buffers. Compare every barrier with the one-worker oracle.
4. **Lifecycle and opt-in:** connect `--parallel`, completion/help, diagnostics, worker pool, input controls, publication, reset/export, and tests of option combinations.
5. **Performance decision:** profile and benchmark ZCore/FPU and multiple active plots. Retain only if the parallel executor provides useful throughput under the accepted relaxed semantics.

Primary files: `redpiler/options.rs`, `redpiler/mod.rs`, a new `sampled/parallel/` implementation, `sampled/program.rs`, compiler preparation/finalization, `sampled/scheduler.rs`, shared pure component logic, plot command declarations/completion, scoreboard/diagnostics, headless/benchmark adapters, and tests with direct runtime/options access. Keep the existing serial executor's hot path unchanged except for extracting genuinely shared pure helpers.

## 9. Validation and benchmark methodology

### Correctness

- Compare the round oracle with parallel execution at every barrier for 1/2/4/8 workers, repeated runs, and different chunk sizes/cutoffs.
- Check blocks, token occupancy, group arrangements, every contribution and input histogram, pending deadlines/priorities, motion state, emitted edges, notes, commands, dirty state, and deterministic failures.
- Test simultaneous competing sources, short pulses, feedback and oscillation, repeater locking, observer edges, comparator rear/side inputs, same-cell piston claims, opposed tokens, interrupted motion, and multiple completions sharing a deadline.
- Test zero-delay work generated while draining a priority bucket, queue exhaustion, semantic budget exhaustion, control input, pause, reset during motion, command output quotas, and both publication modes.
- Compare parallel mode against native/serial execution for circuits known to be insensitive to within-round ordering. For race-sensitive fixtures, assert the documented relaxed result; report divergences explicitly. Default serial-mode regression tests must continue passing.

### Performance

Use the current retained movement-layout and SmallVec changes as the baseline. Run at least seven alternating paired trials per workload with the established 10,000-tick protocol, identical builds/CPU settings, warmup, and input sequences. Include original serial, one-worker round engine, and multiworker round engine: the middle result exposes semantic-refactor overhead.

Measure wall-clock tick throughput, total CPU time, worker utilization, frontier sizes, rounds per phase, task durations, barrier/dispatch time, effects generated, routing/commit cost, conflict rate, snapshot/ghost-copy bytes, allocations, and retained memory. Report latency and total host throughput with multiple plots, not only one plot using every core. Kernel timings exclude compilation/publication; measure those separately in an integrated run.

Use ZCore and FPU, a large connected circuit, disconnected circuits, and small/serial-chain workloads. A large number of cores cannot compensate for narrow frontiers or a global round barrier per tiny update. If the one-worker round engine or repeated barriers consume the expected gain, revisit batching/ownership before adding further concurrency.

## Acceptance and limits

The deliverable is an opt-in experimental executor with defined, reproducible relaxed ordering, safe state ownership, bounded work, coherent reset/export, and measured scaling. It must not claim serial or native picotick equivalence. A compilation flag selects semantics explicitly even when workload size causes execution to use one worker.

Reject unsound shared mutation, nondeterministic conflict resolution, missing event multiplicity, or lost validation as shortcuts. Accept changed circuit results only where they follow the documented relaxed execution contract. Performance remains an experiment: the supplied profile establishes where the work is, but not how much independent work exists at each barrier.
