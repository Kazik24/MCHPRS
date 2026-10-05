# Indexed pistons and persistent wire addresses

Three changes reduce interpreted work without changing game-tick timing:

1. Piston motion lookup uses a position index, validating motion identities and
   repairing shifted vector entries on demand. Event membership uses counted
   keys while execution still consumes the original ordered deque. Installing or
   deleting an entity without a motion avoids scanning the motion vector.
   Banks and event queues of at most eight entries retain linear lookup to avoid
   hashing overhead; larger collections build indexes on demand.
   Movement work reuses its vector allocation. Public mutable state access
   invalidates the indexes; save/load and rewind preserve the authoritative
   collections and rebuild derived state.
2. Wire nodes use section-local generation stamps instead of hashing every plot
   coordinate. A new walk advances the generation, so old node IDs cannot refer
   to new block snapshots. Out-of-plot positions and other World implementations
   retain the hash-table fallback. Six direct neighbor indices also avoid repeated
   hash lookups during signal calculation.
3. Each plot retains canonical addresses for the 24 wire neighbors. A shared
   immutable registry-state table supplies solidity, transparency, callback
   eligibility, and directional connection facts. Cached addresses remain valid
   when pistons move blocks or players edit the plot. Live block snapshots, power,
   visited flags, biases, headings, and oriented callback order are freshly built
   for every walk. Resolved live connections are never retained across walks.

Recursive wire walks take independent scratch space. Plot-local addresses cannot
reuse another plot's canonical world coordinates. Generation wrap explicitly
clears old stamps. Legacy duplicate motions and events retain their original
membership semantics. None of these derived caches enters save files or frozen
world hashes.

## Memory

Spatial stamps allocate 32 KiB per touched 16³ section and retain those allocations
per interpreter thread (128 MiB for all sections of a plot). Canonical address
tables allocate 32 KiB per populated section, plus 480 bytes of neighbor data and
Arc/allocation overhead for each cached center. Admissions stop at 1,048,576
centers per plot; existing entries remain usable and uncached addresses are
computed normally. This cap bounds retained neighbor data to 480 MiB, plus table
and allocation overhead. Ordinary builds allocate only their visited sections
and centers. Registry fact tables are shared between plots.

## Commands

- `//update` sends the normal redstone update to blocks in the current selection.
  The handler already existed; it now has tab completion, validates plot and Y
  bounds, and correctly crosses unaligned section boundaries. Cascading redstone
  effects may extend beyond the selected region.
- `//update -p` updates the entire plot. It does not require a selection.
- `//invalidatecaches` clears this plot's motion/event indexes, canonical address
  cache, and the current interpreter thread's retained wire scratch allocation.
  Blocks, scheduled ticks, motion progress, queued events, and compiled mode
  remain unchanged. The caches rebuild on demand. Immutable registry tables stay
  shared because they do not contain mutable world state.

Both commands use the existing WorldEdit plot ownership/bypass checks. Their
permission nodes are `mchprs.we.update` and `mchprs.we.invalidatecaches`. Help is
available through `/help we` and `//help <command>`.

## CPU measurements

Ryzen 9 5950X, Windows/MSVC, release with fat LTO, three fresh-world runs per CPU.
The preceding optimized version is `optimized-performance.json`; this version
is `heavy-optimization-performance.json`.

| CPU | Previous seconds / 50,000 ticks | New seconds | Previous average TPS | New average TPS | Previous active TPS | New active TPS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| PM1 SORT | 93.226 | 64.001 | 536 | 781 | 129 | 188 |
| ANPU Pong | 6.278 | 5.544 | 7,965 | 9,019 | 797 | 902 |

Median simulation time fell by 31.3% for PM1 and 11.7% for ANPU. Throughput rose
by 45.7% and 13.2%, respectively. Against the original pre-optimization baseline,
simulation time is down 57.1% and 46.9%. Sample ranges were 63.956–64.502 seconds
for PM1 and 5.278–5.588 seconds for ANPU. Every sample passed the unchanged
whole-world checkpoints, ordered tick-stamped PM1 chat, and per-tick ANPU screen
trace. The full 50,000-tick average includes an idle tail; active windows remain
PM1 ticks 1–12,051 and ANPU ticks 1–5,000. Networking and rendering are excluded.

## Piston scaling

The independent-piston benchmark runs a full eight-game-tick extension/retraction
cycle in a preconstructed world. `piston-scaling-performance.json` records
Criterion slope estimates and the new confidence intervals. The previous point
estimates are transcribed from the earlier benchmark's console output.

| Independent pistons | Previous cycle time | New cycle time |
| ---: | ---: | ---: |
| 1 | 3.899 µs | 3.964 µs |
| 64 | 305.760 µs | 292.336 µs |
| 256 | 1.993 ms | 1.290 ms |
| 1,024 | 20.954 ms | 8.139 ms |

The 1,024-piston case uses 61.2% less cycle time (2.57× throughput). Increasing
from 256 to 1,024 pistons now costs 6.31× rather than 10.51×: ordered vector
removal still moves entries, so this is not fully linear scaling. The single
piston remains about 1.7% slower in these measurements. Large collections are the
target workload; the small-bank fallback avoids the initial larger regression.

The workspace suite passed 203 ordinary tests, including exhaustive registry
facts, generation wrap, shifted and duplicate motion/event indexes, plot address
namespaces, cache clearing with pending work, and selection-boundary updates.

```powershell
cargo bench -p mchprs_core --bench cpus -- --iterations 3 --label indexed_pistons_spatial_wire --output target/heavy-cpu-performance.json
cargo bench -p mchprs_core --bench piston -- piston-cycle/independent --noplot
cargo test --workspace --release --locked --no-fail-fast -- --test-threads=1
```
