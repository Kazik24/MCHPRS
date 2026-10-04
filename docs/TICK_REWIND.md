# Tick history

`/rhistory on [ticks]` records interpreter state before each whole game tick.
It keeps up to 100 ticks by default. `/rback [ticks]` restores the selected tick,
consumes that tick and its future, and pauses the plot. Rewind also restores
later edits and clears WorldEdit undo/redo. History is off by default and clears
on compilation, unload or restart. Nano/pico stepping requires recording to be off.

`/rhistory` shows available ticks, uncompressed snapshot bytes, compressed payload
bytes, and server history usage/limit. `/rhistory off` releases the plot's buffer.
Enablement reports an estimated full-buffer size based on the current plot;
the memory limit can shorten the retained window.

## Memory settings

```toml
rhistory_memory_limit_mib = 2048
rhistory_work_memory_limit_mib = 256
```

The default stored limit is **2 GiB across all plots**, applied after compression.
It includes actual payload capacities, ring slots and dictionaries. Raw capture
and codec buffers use a separate, shared 256 MiB workspace limit. Reservations
happen before allocation and release on eviction, rewind, disable, errors or unload.
These limits cover history buffers, not process RSS, allocator overhead or the
live world reconstructed during rewind. Zero disables the corresponding budget.

`/rhistory limit` shows server usage. Admins with the explicit LuckPerms permission
`plots.admin.rewind.memory` can use `/rhistory limit <MiB>`; changes take effect
immediately and persist in `Config.toml`. Lowering below current usage is rejected;
free buffers first. Without LuckPerms, edit configuration and restart.
`plots.admin.rewind.unlimited` bypasses the ordinary 1,000-tick restriction only.

## Compression and failure handling

Snapshots use LZ4 with one immutable dictionary, seeded from the last 65,535
bytes of the initial snapshot and shared across that plot's recording session.
Each tick decodes independently, even after the seed tick is evicted.
Incompressible snapshots retain raw bytes. CRC32 and bounded decoding guard rewind.
Capture borrows chunk/entity data; the scheduler stores ordered typed entries
with relative delays. No save format or compiler internals change.

When stored memory fills, recording evicts only that plot's oldest ticks.
If one tick cannot fit, recording stops, frees its session and sends one notice;
simulation continues. Failed re-enablement or rewind preserves existing state.

## Verification

Sixteen focused tests cover rewind/replay, entities, scheduler ordering, palettes
and direct block storage, post-compression admission, dictionary reuse, raw
fallback, eviction, concurrent reservations, corruption and workspace exhaustion.
The isolated two-client protocol/restart smoke checks command messages, memory
permission rejection, piston/container rewind, disabled sends and persistence.

The smoke fixture recorded four ticks as 2.07 MiB uncompressed and 18.69 KiB
compressed (82.91 KiB including slots/dictionary). This is one fixture's result,
not a throughput benchmark. Full-world serialization and compression still cost
CPU; representative TPS/latency benchmarks remain future work.

```text
cargo test -p mchprs_core --lib plot::history --locked --offline
cargo test --workspace --all-targets --locked --offline
cargo build --locked --offline
py tools/run_protocol_smoke.py
```
