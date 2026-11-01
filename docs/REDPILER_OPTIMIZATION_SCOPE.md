# Redpiler optimization scope

## Objectives

1. Reuse compilation work for unchanged parts of a plot through a bounded, in-memory TTL cache of compilation artifacts.
2. Reduce runtime-to-host publication work by aligning visual state publication with the frontend refresh rate, normally 10 Hz during fast execution.

These are separate changes. Neither has a measured speedup yet. Implement and measure them independently.

## 1. Bounded compilation artifact cache

### Proposed first increment

Cache a small, self-contained discovery artifact rather than an entire compiled circuit or a mutable scratch world. Start by measuring the existing phases with `MCHPRS_REDPILER_PHASE_TIMINGS=1`; choose the artifact only after its cost is visible on representative plots.

The lowest-complexity candidate is a per-chunk component inventory: component positions and classifications collected during the component scan. Reuse inventories from unchanged chunks, rescan changed chunks, and merge in the existing `(y, z, x)` scan order before assigning global component indexes. An edit in one chunk can then preserve inventory lookups for other chunks, including chunks belonging to the same circuit.

Keep state-sensitive validation, current block/entity values, scratch capture, piston grouping, electrical discovery, sampling, and finalization fresh. The current parse pass also constructs a mutable scratch world and discovers piston actors/tokens; caching its complete output is a substantially larger change. Only claim savings for the component enumeration actually skipped.

### Cache policy and correctness

- Use a local in-memory cache, with no disk format or new dependency required initially.
- Key entries by live chunk identity and circuit revision, plus the captured selection intersection. Generic `World` callers without reliable version information use the existing uncached path.
- Capture keys from the live source before creating staging snapshots: staging clones have different chunk identities.
- TTL controls retention only. A matching dependency/version key establishes reuse; elapsed time alone never makes an artifact valid.
- Bound retained memory by estimated artifact bytes and entry count. Do not retain an artifact larger than the byte budget. Expire and evict entries during lookup/insertion rather than adding a maintenance thread.
- Keep one cache per plot initially. Drop it when the plot unloads. Use internal defaults first; select budgets and TTL from measured artifact sizes and editing patterns.
- Preserve cancellation and stale-result rejection. Transfer cache ownership through the existing single compilation worker, or return cache additions with its result; avoid sharing mutable compiler state with the plot thread.
- Do not cache failures, runtime state, scheduled work, piston motions, or partially constructed artifacts.

Chunk revisions provide conservative invalidation: a relevant edit invalidates that chunk's inventory, even if most of it is unchanged. Any inventory field that reads neighboring cells must either declare those chunk dependencies or be computed after the cache lookup from the current snapshot. Keep cache entries limited to local classification to avoid adding that dependency machinery initially.

### Code touched

- `crates/core/src/plot/redpiler_compilation/mod.rs`: cache lifetime, live version capture, worker ownership, and result handling.
- `crates/core/src/redpiler/sampled/compiler/passes/parse.rs`: separate reusable local enumeration from current-state validation and global indexing.
- `crates/core/src/redpiler/sampled/compiler.rs`: optional cache input and hit/miss/phase diagnostics.
- `crates/core/src/world/storage.rs`: reuse existing chunk circuit versions; change mutation tracking only if an audit identifies a missing invalidation path.
- Compilation tests and plot fixtures affected by added cache ownership.

### Measurements and acceptance

Record cold compile, unchanged recompile, and recompile after a one-chunk edit, using both sparse and dense plots. Report phase times, total request latency including snapshot creation, hit/miss counts, retained bytes, and eviction counts.

Verify identical inventory order and compiled behavior with and without caching. Cover block/entity edits, chunk replacement or rewind, selection changes, cancellation, expiration, and memory limits. Reuse across unchanged chunks must survive an edit elsewhere in the plot.

If component enumeration is a small fraction of total compilation time, stop after the measurement or the small cache experiment. Caching sampled source programs or piston arrangement results requires dependency footprints and index remapping and should be scoped separately. A fast inventory lookup cannot eliminate those later passes.

## 2. Publication optimization

### Existing behavior

`Plot::update` executes a batch of compiled ticks and then calls `redpiler.flush()`. Client block packets have a separate cadence based on `effective_send_rate()`, which already supports a fast-execution cap. Publication can therefore happen more often than visible refreshes.

Runtime dirty cells are deduplicated and retain their latest state until publication. Full piston publication also builds moving parts, reconciles exported identities and host motion records, and synchronizes piston events. This work is enabled by the `redpiler-full-publication` feature; ordinary builds already omit that full piston projection during regular flushes.

### Proposed change

Publish accumulated visual state when a visual refresh is due, normally every 100 ms at 10 Hz, then flush client block changes. Reuse the existing effective visual rate and user limits. When packet sending is disabled, retain a bounded host synchronization interval so host state and effects cannot remain stale indefinitely.

Between publications, defer:

- Dirty block projection and comparator entity synchronization.
- Full moving-piston block/entity projection, exported-motion reconciliation, and host piston-state synchronization when that feature is enabled.
- Command-block entity copying, which currently scans the runtime's command entities on each flush.

The compiled runtime continues all logical ticks, signal propagation, piston simulation, and command-block execution. The host receives the latest state at publication; intermediate visual states are coalesced.

### Effects and forced synchronization

Separate event delivery from visual projection where necessary. Noteblock output currently permits one queued note per component until publication; simply slowing the whole flush can suppress additional rising edges. Preserve note delivery at the existing batch boundary or explicitly preserve all required events before changing that cadence. Command emissions must retain order and all accepted messages; drain them independently if publication would change output latency or queue-limit behavior.

Preserve immediate publication after control interactions and manual `/advance`. Preserve complete publication before reset, interpreter handoff, save/export paths that require current world state, and host interactions that resume native behavior. Audit read-only host consumers, including chunk loading, neighbor snapshots, screen checks, and diagnostics, and document which may observe up to one publication interval of stale state.

Handle stopping or pausing before the next deadline: the last accumulated state must still publish. Do not increase the existing simulation time slice to enforce a 100 ms refresh; the deadline is a target subject to execution and scheduling time.

### Code touched

- `crates/core/src/plot/mod.rs`: publication deadline, packet cadence, final update on pause, and timing accounting.
- `crates/core/src/redpiler/mod.rs`: separate visual projection from effects as needed; retain complete reset publication.
- `crates/core/src/redpiler/sampled/commands.rs`: distinguish entity synchronization from emission delivery if required.
- `crates/core/src/redpiler/sampled.rs` and `sampled/executor.rs`: preserve dirty state and note/event behavior between publications.
- `crates/core/src/plot/packet_handlers.rs` and `commands/mod.rs`: audit immediate control and manual-advance flushes.
- Publication, client synchronization, noteblock, command-block, and piston tests; timing fixtures if the plot gains a deadline field.

### Measurements and acceptance

Compare current behavior against 10 Hz publication on a large piston circuit, a wire/logic circuit, and a command/noteblock circuit. Measure kernel ticks per second, publication count and time, packet collection/enqueue time, and refresh latency. Exercise both full-publication feature settings and screen-only rendering.

Verify that many ticks without publication produce the same final host state as frequent publication, including reset during active piston motion and restoration of scheduled work. Check input responsiveness, manual advance, pausing, disabled packet sending, and event ordering. A note or command must not disappear solely because projection is delayed.

## Suggested sequence

1. Capture compile phase and runtime publication timings on representative plots.
2. Implement publication cadence with effects and forced synchronization preserved; measure throughput and latency.
3. Implement the bounded component-inventory cache only if the component scan cost justifies it.
4. Use the remaining hot phases to decide whether deeper partial compilation is worth its dependency-tracking cost.
