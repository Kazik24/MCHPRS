# Tick-history memory limit and compression

Scoped on 4 October 2026 against the implemented `rhistory` in `8eae162` and the current working tree. This is a proposed extension, not an implemented limit or a compression benchmark. Current defaults remain 100 ticks, a 1,000-tick ordinary-user limit, and in-memory interpreter snapshots.

## Proposed behaviour

```toml
# Config.toml; admin settings, read at server startup
rhistory_memory_limit_mib = 2048
rhistory_compression = "none" # later: "lz4" or "zstd"
```

2,048 MiB is exactly 2 GiB (2,147,483,648 bytes). MiB configuration also allows smaller limits without fractional arithmetic. Zero disables recording; negative values and byte-conversion overflow are configuration errors. Existing command syntax stays unchanged. `plots.admin.rewind.unlimited` continues to bypass the tick-count restriction only; it never bypasses memory limits.

The recommended default is **one server-wide history budget**, shared by all loaded plots. A per-plot 2 GiB limit would allow ten recording plots to retain roughly 20 GiB. This recommendation is provisional until the user's scope preference is confirmed; the same accounting can instead be attached to each plot without a shared counter.

`/rhistory` should report available/requested ticks, this plot's stored bytes, global reserved bytes/limit, and compression mode. Requested ticks remain an upper bound: reaching either the tick count or the byte allowance evicts this plot's oldest snapshots. No command silently promises that all requested ticks will fit.

If the next snapshot cannot fit even after releasing this plot's old snapshots, disable and free its recording session, notify occupants once, and let simulation continue. Never keep recording across a missing snapshot: that would make `/rback K` cease to mean exactly K game ticks. Do not evict another plot's history or change its TPS. A rejected enable/re-enable must preserve the existing session; a rejected rewind must preserve history, world state and TPS.

## What a hard limit means

The limit controls allocations owned by the tick buffer: stored payloads, slot metadata, serialization buffers, compression/decompression buffers and any persistent codec workspace. It is **not a cap on the entire Minecraft process**. The live world, client packets, selections/clipboards, runtime stacks and allocator overhead remain ordinary server memory. Reconstructing a replacement live world for rewind can also temporarily coexist with the current world; that is separate from the bounded history buffers. A process RSS guarantee would additionally require an OS/container memory limit and cannot be inferred from this setting.

The current `Snapshot::heap_bytes` is an estimate. Hash-table allocation is approximated, scheduler memory uses live-entry counts, and `Snapshot::capture` clones before accounting. Comparing this value with 2 GiB after capture would permit overshoot and must not be advertised as a hard allocation limit.

For a hard buffer limit, replace retained object graphs with **independent encoded byte snapshots**, initially uncompressed. Use the actual reserved payload capacities and known metadata allocation sizes. Serialization size is appropriate for these stored byte buffers; it is not a substitute for measuring the heap cost of the existing object snapshots.

Introduce a small shared `HistoryBudget` with checked reservation and a drop guard. Reserve before every managed allocation; release on eviction, rewind, disable, unload, errors and temporary-buffer cleanup. Use a short mutex or compare-and-exchange reservation to make concurrent plot admissions atomic. Never hold a global lock while serializing, compressing, ticking or sending packets. Failed allocations roll back reservations.

Account for transient allocations too. Growing a vector may keep both old and new storage alive; replacing a compressed output with a smaller exact allocation may do the same. Reserve this peak before the operation. Prefer pre-sized fallible allocations and reusable bounded workspaces. Do not count only vector length, rely on compression ratios in advance, or build an unbudgeted cloned snapshot first.

## Snapshot representation and capture

Store a small internal header with encoding version, codec, logical tick, raw length and integrity checksum, followed by the payload. The payload contains chunk coordinates/data/entities, ordered typed scheduled ticks, and the complete `PistonState`. History stays transient; no plot/player save-format or client-protocol changes are needed.

Use existing serde/bincode representations for block entities, `ChunkData`, `TickEntry` and piston state where possible. Add a borrowed serialization view in `world/storage.rs` so capture reads committed palette/buffer data and entities without first cloning the entire world. Pending writes must be included without consuming outgoing packet trackers, as current `Chunk::save` does. Do not serialize networking state, history itself or compiler state.

The scheduler has no serde representation. Use its existing `iter_entries()` and `FromIterator<TickEntry>` at the whole-game-tick boundary, without editing Redpiler internals. This normalizes the internal ring cursor rather than storing its exact physical index. Prove that it preserves remaining delays, typed block identity, priority, FIFO ordering and subsequent scheduling, including queue wraparound and same-tick entries. If that equivalence fails, revise the adapter before shipping; do not approximate queue timing.

Capture flow:

1. Calculate encoded size through a borrowed/counting serialization pass; use checked arithmetic and validate all counts.
2. Reserve ring/record metadata plus raw and worst-case codec output/workspace. Release this plot's oldest entries when needed to maintain its newest contiguous tick window.
3. Serialize directly into a reserved raw buffer. For `none`, this becomes the stored payload.
4. For compression, encode into a pre-reserved output buffer. Allocate/retain only the final required capacity, with its temporary peak covered by the reservation. If compression would enlarge the payload, store raw bytes and identify the record as raw.
5. Publish the completed record and release scratch reservations. Only then run the interpreted tick. Failures disable recording as described above; they do not skip or reorder simulation work.

Enablement must reserve its new slot allocation and bounded sample workspace while the previous session is still accounted for. Commit replacement only after validation succeeds. Empty slots count against the budget; an enormous admin tick count must not allocate its vector before checking the byte limit. A projected full-buffer size is informational, not an admission requirement: a smaller byte-limited window may still be useful.

For rewind, borrow the target without first consuming history. Validate version, size, checksum and nested collection counts; reserve decompression buffers and decode the replacement state. Reject malformed payloads and allocation failures without changing the plot. Only after successful reconstruction discard the consumed future, swap simulation state, pause, clear staged messages/WorldEdit history and refresh clients using the existing rewind path. Compressed lengths must never be trusted to trigger an unbounded allocation.

## Live compression options

Use a library inside the server process. Launching a compressor executable for each tick would introduce process management and pipe buffering into the recording path; it does not fit the requested simple implementation.

| Candidate | Proposed role | Considerations |
| --- | --- | --- |
| LZ4 through `lz4_flex` | First live-compression prototype | Pure Rust; caller-provided output buffers and safe encoding/decoding are available. Start with independent blocks and reserve the published worst-case output bound. |
| Zstd through `zstd`/`zstd-safe` | Benchmark alternative at level 1 | Native library integration; bulk compression supports caller-provided output buffers. Codec context/workspace also needs a conservative, tested budget. Consider only after that accounting is explicit. |
| No compression | Baseline and raw fallback | Encoded snapshots still make the byte-buffer cap enforceable and isolate serialization cost. |

Primary references: [lz4_flex implementation and buffer APIs](https://github.com/PSeitz/lz4_flex), [LZ4 block APIs and output bound](https://docs.rs/lz4_flex/latest/lz4_flex/block/index.html), [Zstd Rust bulk compressor](https://github.com/gyscos/zstd-rs/blob/main/src/bulk/compressor.rs), and [Zstandard's codec tradeoffs](https://github.com/facebook/zstd). Dependency versions must be selected and locked at implementation time.

Compress each completed tick snapshot independently, synchronously on its plot thread for the first prototype. Rewinding can then decode only the selected tick, with no dependency on newer or evicted snapshots. Do not begin with a background worker queue, dictionaries shared between ticks or delta chains: each adds ordering, cancellation and memory-lifetime work to the hard limit.

Compression can reduce retained bytes. It does **not** eliminate full-world traversal, serialization or simulation work, and synchronous compression adds tick latency. Existing palettes already compress block-state storage. Independent compression also does not share unchanged data between consecutive snapshots. Real snapshot measurements must decide the default; no ratio or TPS gain is currently established. Leave `none` as the default until LZ4 passes correctness and performance acceptance.

If the contiguous raw/output workspaces become too large, a later bounded streaming implementation can encode independent fixed-size blocks within each snapshot. This reduces temporary memory but adds framing/decoder work. Keep it out of the first implementation unless measurements show it is necessary.

## Validation and measurements

Use tiny injectable byte limits in tests rather than allocating 2 GiB. Cover exact-boundary acceptance, one-byte-over rejection, metadata-only overflow, incompressible input, growing circuits, codec scratch peaks and two recording plots racing for the last reservation. Assert global reserved bytes never exceed the configured limit and return to zero after all sessions/workspaces are released.

Test oldest-entry eviction with ring wraparound, a shortened contiguous rewind window, re-enable failure preserving the old session, automatic stop on an oversized snapshot, and no recording gaps. Memory-full and decode failure must not panic or leave partial state. Admin tick-count bypass must not bypass the byte budget.

Run existing scheduler/piston/command-block/container rewind and replay checks against raw and compressed encodings. Include mutable entity payloads, moving carried inventories, edits, comparator/repeater delays, cross-plot reservation release, `/wsr 0`, reconnect and restart. Test corruption, size mismatches and unreasonable collection lengths before allocation.

Benchmark an empty plot, the supplied adder and Potados fixtures, a populated piston circuit, and container-heavy/incompressible data at 20, 100, 1,000 and unlimited TPS. Record raw/compressed stored bytes, actual reserved buffer peak, encoding/compression/decode p50/p95/p99 latency, and achieved simulation TPS against history disabled and current object snapshots. Rewind latency and memory during capture matter as much as retained size. Library benchmark results are not evidence for these plots.

The existing small protocol-smoke fixture projected approximately 765.25 MiB for 1,000 raw object snapshots, according to `TICK_REWIND.md`. This illustrates why a byte limit is useful, but it is neither a full-buffer measurement nor a compression estimate.

## Implementation order

1. **`Encode history snapshots`**: bounded byte representation, borrowed capture and scheduler adapter; raw round-trip/replay equivalence first.
2. **`Limit history memory`**: default 2 GiB admin configuration, shared reservations, scratch accounting, eviction/stop policy, status/help and concurrent-limit checks.
3. **`Compress history snapshots`**: independent LZ4 records, bounded encode/decode, raw fallback and corruption checks.
4. **`Measure history compression`**: representative measurements; choose whether to enable LZ4 by default. Prototype Zstd separately if its ratio/CPU tradeoff looks useful.

Expected files: `config.rs`, `plot/history.rs`, small history budget/codec modules and tests, `world/storage.rs` for borrowed serialization, `plot/mod.rs` for stop notifications, `core/Cargo.toml`/`Cargo.lock` for the chosen codec, and help/usage/benchmark tooling. Current container changes must be included in entity serialization and replay coverage. Do not change piston logic, movement rules, Redpiler internals, plot save schemas or deployed configuration as part of this scope document.
