# Redpiler local connectivity cache: prototype plan

Status: implementation proposal, not an implemented or measured cache.

Date: 2026-11-04.

## 1. Objective and decision

Test whether caching individual wire-connectivity tables makes recompilation
cheaper while preserving results after a small world edit. An edit must change
the keys of affected entries, rather than discard the entire cache.

The first experiment caches only the local tables built for token-dependent
wires in `WireGraph::compile`. It does not cache a `Program`, a runtime, a
scratch world, a complete wire graph, or a whole compiler pass.

The experiment succeeds only if reuse is correct and key construction, locking,
reference conversion, and retained memory cost less than the work skipped.

This is a separate experiment from the per-chunk inventory proposal in
[REDPILER_OPTIMIZATION_SCOPE.md](REDPILER_OPTIMIZATION_SCOPE.md). Do not combine
the two caches or the runtime publication changes in the first implementation.

## 2. Why this boundary

Relevant code:

- [routing.rs](../crates/core/src/redpiler/sampled/routing.rs):
  `wire_token_axes`, `compile_local`, `WireGraph::compile`.
- [scratch.rs](../crates/core/src/redpiler/scratch.rs):
  `discover_wire_ports`, `WirePorts`, `ScratchWorld::probe_jobs`,
  `ScratchWorld::finish_probe`.
- [power.rs](../crates/core/src/redstone/power.rs):
  power-source direction checks called by wire-port discovery.

Currently, wire compilation performs these operations:

1. Discover the wire's candidate movable-token axes.
2. Enumerate the Cartesian product of their positions, including absent tokens.
3. Reset scratch state and place the tokens for each state.
4. Discover neighboring wires and direct sources.
5. Convert wire positions to the current graph's indices.
6. Fold the table to one row if every outcome is identical.

The reusable result is between steps 4 and 5: connectivity expressed using world
positions, including invalid overlapping-token states.

The current wire-port query reads block states within Manhattan distance two
of the wire. Its current power-query call paths do not require block entities or
tick execution. This is a dependency contract to prove with tests, not an
assumption to leave undocumented. Changes to those callees must update the
contract or disable this cache path.

Do not apply the same footprint to reader tables, movement probes, or source
probes. Their dependency boundaries are different.

## 3. What this can and cannot save

Potential savings are token placement, scratch restoration between local
states, repeated local wire-port discovery, and rebuilding identical tables.
The first implementation attempts reuse only when candidate token axes exist.
The one-state path without candidate tokens is already inexpensive and remains
unchanged.

These costs remain fresh:

- Snapshot capture, parsing, component and token discovery.
- Candidate-axis selection and cache-key construction.
- Mapping cached positions into the current graph.
- Reader tables, reader dependency traversal, and arrangement lowering.
- Movement validation/lowering, source sampling, and runtime initialization.

Historical context, not an expected speedup: the earlier PM1 measurement spent
10.6 seconds in the entire arrangement-discovery phase, 44.0 seconds in movement
validation/lowering, and 55.6 seconds in source sampling. This prototype covers
only part of arrangement discovery. Even an excellent hit rate cannot remove
the latter two costs. Obtain fresh measurements against the implementation
revision before making a performance claim.

## 4. Scope and non-goals

### Included

- One private cache type with an explicit memory budget.
- Position-based local wire tables and exact local-input keys.
- Reuse across compilations in one benchmark process.
- Existing Rayon probe execution, with short cache critical sections.
- Focused correctness tests and a repeatable experiment harness.

### Deferred

- Production plot ownership and compiler-worker plumbing.
- Disk serialization, process-to-process reuse, TTL, background maintenance,
  new dependencies, and player-facing configuration or commands.
- A generic cache framework or interpreter-wide dependency recorder.
- Reader, movement, source-order, and whole-program caches.
- Relative-position sharing between different copies of a circuit.
- Ignoring powered-state bits or otherwise normalizing block states.
- Fixing movement-group lookup complexity in the same change.

Keep exact block-state IDs initially. Simulation may change many local keys,
reducing reuse after a run. Measure that behavior before considering any
normalization of states.

## 5. Cache input contract

### 5.1 Fixed block footprint

For wire position `p`, define the ordered offsets:

```text
D = sorted lexicographically by (dx, dy, dz):
    all offsets with abs(dx) + abs(dy) + abs(dz) <= 2
```

There are 25 offsets, including `(0, 0, 0)`. Store all 25 raw block-state IDs,
including air. Use checked coordinate arithmetic.

The footprint is conservative: changing an unused block inside it may miss the
cache. That is acceptable for this prototype. Omitting a relevant input is not.

Build the key from the restored scratch basis, before synthetic token writes.
Do not use block states left by the previous job or by the last enumerated state.

### 5.2 Token inputs

Retain the candidate-axis order produced by the current discovery code. For each
axis, record:

- Token block-state ID.
- Initial world position, including `None`.
- Ordered legal-cell positions.

Cell order defines state digits and must not be sorted independently. Retain
duplicate descriptors for distinct tokens; do not merge them by value.

Do not include global token indices in the key. Reordering descriptors can cause
a conservative miss; it must never cause an incorrectly interpreted table.

### 5.3 Conservative pilot eligibility

The benchmark must report how many wires are bypassed for each condition.
Initially use the cache only when:

1. Candidate axes exist and their state product passes the existing limit.
2. The full block footprint is inside the scratch world's admitted read context.
3. Every token position that the enumeration can clear or write is inside the
   capture bounds and has no block entity in the restored basis.
4. The probe starts at a quiescent entry: between ticks, with no pending ticks,
   piston events, motions, or pending movement work.

Otherwise execute the existing uncached path. Do not clear scheduled work,
change bounds, or modify the input merely to make it eligible.

Determine read admissibility without performing an unknown read. An optional
cache-key read must not set a boundary-error flag for a cell that the original
algorithm would never have read. In particular, do not interpret an unloaded
chunk as known air.

This restricted entry contract keeps the first proof small. If it excludes most
real work, report that result before expanding the cache's dependency model.

## 6. Proposed data structures

The following Rust-like declarations describe the intended representation.
They are not an API commitment or a request to introduce wrapper types for
existing positions or block IDs.

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    block_id: u32,
    initial: Option<BlockPos>,
    cells: Vec<BlockPos>, // existing state-digit order
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct WireTableKey {
    wire: BlockPos,
    block_ids: [u32; 25], // fixed offset order D
    axes: Vec<TokenKey>, // current candidate-axis order
}

struct CachedWireTable {
    constant: bool,
    ports: Vec<Option<WirePorts>>,
}

struct CacheEntry {
    table: Arc<CachedWireTable>,
    estimated_bytes: usize,
}

struct WireTableCache {
    entries: HashMap<WireTableKey, CacheEntry>,
    insertion_order: VecDeque<WireTableKey>,
    retained_bytes: usize,
    budget_bytes: usize,
}
```

Use the existing `WirePorts`: both `wires` and `sources` are world positions.
`None` means that token positions overlap in that state; it is not an error to
discard or a missing row to fill later.

The current `compile_local` either retains all candidate axes or clears all of
them when all outcomes are equal. Therefore `constant` is sufficient:

- `true`: one table row, no retained axes.
- `false`: full Cartesian table, with the current candidate axes reattached.

No component indices, graph indices, token indices, or `Operation` offsets live
in a cached table. If partial axis elimination is introduced later, this
representation and its tests must change with it.

Use normal hash-map hashing plus full key equality. A hash collision must never
establish a hit by itself. The cache is process-local, so no persistent compiler
version or block-registry serialization format is needed.

## 7. Algorithms

### 7.1 Lookup and compilation

```text
for each wire job, in the existing probe_jobs execution:
    check cancellation
    restore scratch basis through the existing job lifecycle
    discover candidate axes
    check original state-count limit

    if no cache or pilot eligibility fails:
        run existing indexed compilation unchanged
        return its result

    construct exact local key from the restored basis and axis descriptors
    briefly lock cache and look up key
    clone Arc to an immutable table if found; release lock

    on hit:
        check cancellation
        run current entry/boundary validation
        convert cached positions to current graph indices
        attach current axes, or none for a constant table
        charge output rows against the existing graph-wide size limit
        return node

    on miss:
        enumerate using existing compile_local with discover_wire_ports
        keep position-based output before graph-index conversion
        preserve per-state reference checks at their original evaluation point
        preserve scratch.finish_probe checks during enumeration
        convert positions and apply current validation and size limits
        if successful and not cancelled:
            offer immutable position-based table to cache
        return node
```

The lookup does not replay token writes. Eligibility checks and fresh boundary
validation must make skipping those writes safe. In particular, the cached path
must not accept a token write outside the capture that the uncached path rejects.

The driver must restore scratch state before a subsequent job for either result
path. A hit must not depend on the dirty-state side effects of a previous miss.

### 7.2 Position conversion

For every successful table row:

1. Map each wire position through the current `WireGraph` position index.
2. Preserve the existing error for an unrepresented routed wire.
3. Keep source positions unchanged for the later source-binding stage.
4. Preserve row order, port order, invalid rows, and table folding.

A missing reference is a current compilation error, not an excuse to trust an
old cached index. This is why cache hits must run the same reference conversion
as misses.

On a miss, do not postpone a current per-state missing-reference error until
after later states have been evaluated. Retain position-based data alongside
the normal conversion/checks so the extraction does not change which failure
is reported first. Exercise ordered failures in the regression tests.

### 7.3 Invalidation by exact input matching

There is no edit listener or plot-wide invalidation flag in the prototype.

- An edit inside an entry's footprint changes its key.
- Changed candidate tokens or token definitions change its key.
- A distant edit outside both dependencies leaves the key unchanged.
- An insertion that shifts global IDs leaves reusable position-based results
  valid; binding uses the new graph indices.
- An edit followed by undo may match an older retained entry.

Old entries remain harmless until evicted because each lookup requires complete
input equality. Recompute candidate axes on every compile; do not cache that
selection and thereby miss newly introduced token dependencies.

### 7.4 Memory budget and eviction

Start the experiment with a 32 MiB cache budget, passed to the cache constructor
by the harness. Tests may use smaller budgets. This is an experiment setting,
not a new server option or a claimed production default.

Use insertion-order eviction, not LRU:

```text
estimate key, queue-key copy, entry, table, and nested vector capacities
if entry alone exceeds budget:
    do not retain it; return the computed result normally

lock cache
if identical key already exists:
    keep existing entry; do not enqueue a duplicate
else:
    evict oldest entries until the new entry fits
    insert entry and enqueue its key
    update retained-byte accounting
unlock cache
```

The FIFO queue duplicates keys in this simple representation. Account for that
copy, including token and cell vectors. Include metadata and a conservative
hash-map allocation allowance; describe retained bytes as an estimate rather
than exact process memory. Account for container capacity retained after
eviction, and release oversized empty storage if necessary to keep the estimate
within budget.

An `Arc` held by an in-flight worker can temporarily outlive cache eviction.
Distinguish bytes retained by the cache from transient worker memory and graph
output memory. Report process memory separately when available.

If the working set exceeds the budget, FIFO can produce poor warm hit rates.
Report misses and evictions; do not silently grow the budget or claim a benefit
from an unbounded cache.

### 7.5 Rayon and failure handling

- Share one explicitly owned cache through `Arc<Mutex<WireTableCache>>` in the
  experiment, using only installed dependencies and the standard library.
- Perform key construction, table enumeration, reference conversion, and
  diagnostics formatting outside the lock.
- Permit duplicate computation for simultaneous misses. Recheck the key during
  insertion; do not add per-key promises or worker waits in the first attempt.
- Preserve `probe_jobs` result ordering and existing progress accounting.
- Preserve cancellation and graph-wide row accounting on hits and misses.
- Do not cache errors, partially computed tables, or cancelled computations.
- If the cache lock is poisoned, bypass caching for that operation instead of
  making an otherwise valid compilation fail or trusting poisoned cache state.

No lock may be held while invoking interpreter work or waiting for another
worker. The cache contains immutable artifacts, never mutable scratch worlds.

## 8. Implementation sequence and affected code

Each step should leave the uncached path usable and reviewable.

| Step | Work | Expected files | Completion check |
|---|---|---|---|
| 1 | Freeze a baseline and split wire-node timing from the rest of arrangement discovery | `routing.rs`, experiment harness | Uncached stage time and work counts recorded |
| 2 | Prove the two-step dependency contract; test air and token-state behavior | Routing/scratch tests | Guarded reads never escape the footprint or access excluded inputs |
| 3 | Extract position-based wire-table production and a shared conversion step | `routing.rs` | Existing topology tests pass with cache absent |
| 4 | Add exact key construction and eligibility queries | New private cache module, `scratch.rs` only where metadata access is needed | Keys change for relevant edits and remain equal for distant edits |
| 5 | Add bounded storage, accounting, FIFO eviction, and unit tests | Private cache module | Budget, collision, oversized-entry, and duplicate-insert tests pass |
| 6 | Integrate optional lookup into wire-node jobs | `routing.rs`; module declaration as needed | Cached and uncached graph artifacts match |
| 7 | Reuse one cache across fresh compilations in a diagnostic harness | One example or ignored experiment test | Cold, warm, and edited-world trials run in one process |
| 8 | Run correctness and performance experiments; write evidence and decision | Tests and `docs/redpiler-regressions/<date>/` evidence | Explicit go, revise, or stop conclusion |

Keep `WireGraph::compile`'s existing callers and reader-table path working. Use a
private optional cache argument or a private implementation entry point; select
the smaller change after checking all callers. Do not add a public compiler
option just to run the experiment.

The harness owns the cache across fresh scratch worlds and compiler instances.
A cache created inside one `WireGraph::compile` call cannot establish reuse
across compilations and is not this experiment.

Only after a positive result should a separate change give each loaded plot its
cache, preserve it across compilation workers, and drop it on unload. Retain
the existing snapshot activation and stale-result checks in that later change.

## 9. Correctness test matrix

For every applicable case, compare the uncached result with cold-cache and
warm-cache results, including errors. Compare semantic tables and bindings, not
only a screen hash or hit count.

| Case | Required behavior |
|---|---|
| Unchanged world | Equal table; second invocation hits |
| Distant edit in the same chunk | Unaffected entries still hit; whole chunk is not an invalidation unit |
| Relevant block edit | Changed key and recomputation |
| Air becomes solid, then is removed | Miss on the edit; correct result after undo |
| Direction or powered-state change | Full raw block-state key detects change |
| Token type, initial position, cells, or cell order changes | Changed key or eligibility bypass |
| New nearby token dependency | Fresh candidate selection detects it |
| Unrelated insertion shifts component/token IDs | Cached positions rebind correctly, without old indices |
| Distinct tokens with equal descriptors | Remain distinct state axes |
| Overlapping token states | Preserve the same `None` rows |
| All outcomes equal | Preserve the single-row, no-axis representation |
| Unknown read context or out-of-bounds writes | Bypass cache and retain original rejection behavior |
| Pending work or entities excluded by pilot contract | Bypass without changing input or behavior |
| Referenced wire is absent from current graph | Preserve current missing-reference error |
| Pre-cancelled and interrupted work | No successful activation and no partial entry published |
| Graph row budget exceeded using hits | Same size bound still applies |
| Hash collisions | Full key equality prevents an incorrect hit |
| Small budget, oversized table, repeated eviction | Bounded retention; correct uncached result remains available |
| Concurrent duplicate misses | Correct results, one retained entry, consistent accounting |
| One and several Rayon workers | Equal tables and bindings; input-order graph assembly retained |

The dependency-contract test must cover the branches of `discover_wire_ports`,
including conducted sources, steps up/down, directional devices, and unsupported
source errors. It must also make accesses to excluded inputs such as block
entities visible, so a later query change cannot silently invalidate the key
design.

Keep the current topology, movement, admission, and boundary regression tests.
Record existing unrelated failures separately; do not weaken tests to obtain a
green cache experiment.

## 10. Benchmark protocol

### Fixtures

1. A deterministic synthetic layout with many independent token-dependent
   wires, a working set that fits the pilot budget, and editable empty space.
2. Zcore, using the current pinned schematic hash.
3. PM1 fixed, using the current pinned schematic hash.

Do not invent fixture identities from names alone. Record the file hashes used
by the harness. Use a release build and record source revision, dirty-patch
identity, binary hash, exact command, thread count, and cache budget.

### Scenarios

| Scenario | Setup |
|---|---|
| Uncached baseline | No cache lookup or key construction |
| Cold cache | Empty cache and original snapshot |
| Warm cache | Fresh compilation of the same snapshot, retaining cache |
| Distant edit | Warm cache, edit outside a selected entry's dependencies but inside its chunk |
| Local edit | Warm cache, change a relevant block or nearby token |
| Post-run recompile | Compile, run, reset, and compile the resulting actual world with cache retained |
| Capacity pressure | Small budget forces eviction without changing correctness |

Reset the cache between scenario sequences so an edit test does not accidentally
measure entries warmed by a different scenario. Preserve it inside the sequence
being tested. Keep source and world snapshots identical between cached and
uncached comparisons. The post-run scenario must use the real resulting world,
not reload the original fixture to manufacture a high hit rate.

Start with isolated wire-stage experiments. Run end-to-end compilation only
after correctness and stage measurements justify the longer trial. Keep
interpreter execution, state comparisons, and fixture loading outside the
compilation timer and report them separately.

Use three paired baseline/experimental trials where practical, alternating
order. Record background workload and ranges rather than presenting a single
trial as a stable speedup. Use the same fixed Rayon pool size for each pair;
correctness across worker counts is a separate check, not a requirement to run
a very long single-worker PM1 compile.

### Required measurements

- End-to-end compile wall time and wire-node stage wall time.
- Candidate wires, eligible wires, bypass counts by reason.
- Lookups, hits, misses, duplicate inserts, evictions, oversized bypasses.
- Estimated retained bytes and peak retained bytes.
- Key-construction, cache-lock/lookup, enumeration, and conversion work time.
- State rows enumerated versus reused.
- Semantic equivalence result for each scenario.

Worker work-time sums overlap under Rayon. Label them explicitly and do not add
them to derive stage wall time. Collect cheap counters per worker/batch and
aggregate them; do not introduce a contended atomic or mutex operation for each
block read.

## 11. Decision gates

Correctness is mandatory before considering timing:

1. All focused equivalence and invalidation tests pass.
2. Relevant edits cannot produce stale results; unrelated same-chunk edits
   preserve reuse for unaffected entries.
3. Existing cancellation, boundary, and size-limit behavior remains intact.
4. Retention respects the declared budget, with accounting limitations stated.

Initial performance targets, to be evaluated rather than assumed:

- At least 2x faster warm wire-node construction on the fitting synthetic layout.
- No more than 10% cold wire-stage overhead relative to the truly uncached path.
- A repeatable stage saving on a real fixture at the stated budget.
- End-to-end saving large enough to matter relative to run-to-run variability.

If only the synthetic layout improves, keep the result as an experiment and do
not add production plot integration. If key validation, contention, poor
eligibility, eviction, or the small share of total compile time erases the
benefit, record that reason and stop or revise the boundary.

A positive experiment justifies a separate production-integration plan. It does
not establish that source sampling can use the same key or that a whole circuit
is incrementally compiled.

## 12. Deliverables and remaining risks

Deliverables are the private prototype, focused tests, a reproducible harness,
machine-readable timing/counter records, and a short evidence-backed decision.
Do not commit generated executables or large temporary world snapshots as
source artifacts.

Main risks are an incomplete dependency contract, stale global references,
invalid cache-hit shortcuts around probe validation, underestimated retention,
and a warm-cache benchmark that does not resemble actual editing or simulation.

The largest unresolved performance question is whether this small local stage
is worth caching at all. Source sampling and movement lowering remain outside
this prototype, and first-compilation time should not be advertised as improved.
