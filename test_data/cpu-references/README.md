# Mixed piston/redstone CPU references

These fixtures freeze interpreted execution of the two original, unmodified
schematics. They are regression references for interpreter optimizations, not
independent proofs that the CPUs implement their instruction sets correctly.

| CPU | Schematic | Start button (selection-local) | Stop button |
| --- | --- | --- | --- |
| PM1 SORT | `../PM1_SORT.schem` | `(187, 35, 72)` | `(187, 32, 72)` |
| ANPU Pong | `../Q2CK@Q2CK_Anpu1_Pong_KBTV.schem` | `(122, 68, 56)` | — |

The selection minimum is placed at `(8, 8, 8)` in an empty plot, ignoring the
saved WorldEdit player origin. The harness presses the actual stone buttons with
the same support notifications and release scheduling as player interaction.
All tick counts are **game ticks**, not redstone ticks. No compilation, manual
paddle inputs, or initial whole-build settling is performed.

Both runs advance 50,000 game ticks from the start press. PM1 then receives a
manual stop press and another 100 ticks. PM1 emits 1,534 messages during the
program, including `shut` at tick 12,051; the manual stop adds a message at
tick 50,007. ANPU finishes quickly without paddle input. Its 32×32 lamp display
is selection-local `x = 140`, `y = 58..89`, `z = 38..69`.

## What is compared

`pm1_sort.json` stores the complete, ordered, tick-stamped tellraw trace.
`anpu_screen.json` stores every change in ANPU's visible display at game-tick
boundaries, including the initial frame. Each bitmap contains 1,024 literal
`0`/`1` pixels: descending Y rows, ascending Z columns. The baseline has 14
distinct consecutive display states, with the final change at tick 3,422.
Comparing only the final frame would miss animation regressions.

Both CPU JSON files also contain stricter SHA-256 checkpoints at ticks 0, 10,
100, 1,000, 5,000, 10,000, 20,000, 30,000, and 50,000. PM1 includes 50,100 too.
These cover:

- All block states and section coordinates, including empty sections.
- All block entities in sorted coordinate order.
- Pending scheduled ticks in execution order, including expected block type.
- Piston events, motion progress, identities, and stepping phase.
- Chat contents and their execution ticks.

Item NBT compound keys are recursively sorted before hashing because import
reserializes hash maps with arbitrary key order. Tag types, numeric bit patterns,
list order, and all other values remain significant. Palette storage layout,
network dirty flags, and packet caches are excluded. No redstone power state,
piston timing, queued callback, or output difference is silently tolerated.
An optimization that intentionally changes internal states should introduce a
specific, reviewed normalization while retaining the chat and screen checks;
it should not replace these frozen outputs with newly generated expectations.

The schematic SHA-256 values are asserted before each run. Registry ID changes
also invalidate the block snapshots; review those as a version migration.

## Interpreter changes

Wire walks retain every cached neighbor for signal strength and direction
calculation, but omit callbacks for blocks whose redstone dispatcher is inert.
Live block snapshots are cached within a single walk; immutable block-state facts
are shared by registry ID. An exhaustive registry-state test calls
every omitted handler against a world that panics on reads and side effects.
Active callbacks retain their existing relative order.

Neighbor discovery uses section-local generation stamps for plot cells, retaining
a hash-table fallback outside the plot. Canonical neighbor addresses persist in
the plot, and each walk builds fresh oriented and direct neighbor lists in a
contiguous array arena. The current queue
layer is processed in place instead of cloning it. Propagation appends only to
the next two layers.

Wire walks reuse thread-local node, neighbor, hash-table, and queue allocations.
Every walk clears all block states, oriented neighbors, eligibility, and direction
data. Spatial entries expire by generation; immutable canonical addresses remain
in the plot cache. The scratch space is removed from its thread-local slot
before calling handlers, so recursive walks obtain independent scratch space.
One returned scratch space is retained per interpreter thread. This trades the
memory of a previous large walk for fewer allocations on subsequent walks.

Block-state decoding uses a shared immutable table initialized once from the
original decoder. Every world read indexes this table instead of reconstructing
properties and verifying the reverse ID mapping. Its memory cost is one `Block`
per registry state; invalid IDs still produce the original opaque `Unknown` state.
An exhaustive registry test checks equality with the original decoder and ID
round trips. Modeled enum blocks also skip command-block name lookups; the same
test verifies classification against registry names for every state.

Callback eligibility, solidity, transparency, and directional wire connections
use an immutable registry-state table. Inert nodes still receive the original
layer metadata updates, preserving deduplication. Cached connection facts describe
a block state, not its current surroundings. Power and traversal headings are
recomputed on every walk.

Moving-piston entity installation no longer removes the same previous motion
twice: `register_motion` already performs that removal. This eliminates a
redundant linear scan while preserving motion identity and ordering.
Motion lookups and event membership now use derived indexes; ordered motion and
event collections remain authoritative and keep their original save format.

Scheduled priority queues use FIFO deques instead of shifting a vector on every
front removal. Tick timing, priority, insertion order, and save formats stay the
same. The frozen scheduler snapshots check their execution order alongside the
Java piston traces and existing compiler/interpreter handoff tests.

## Run

```powershell
cargo test -p mchprs_core --test cpu_references --release -- --include-ignored --test-threads=1
cargo bench -p mchprs_core --bench cpus
cargo bench -p mchprs_core --bench cpus -- --cpu anpu_pong --iterations 3
cargo bench -p mchprs_core --bench cpus -- --iterations 3 --label current --output target/cpu-performance.json
```

Full CPU regression tests are explicitly ignored in ordinary `cargo test`
because they simulate large builds. The ordinary input test checks both frozen
schematics and initial worlds. The benchmark always runs all correctness
assertions, and defaults to one fresh run per CPU. Each sample excludes loading,
button activation, checkpoint hashing, chat/screen comparison, and teardown;
only `tick_interpreted()` calls contribute to the reported time. The 100 manual
stop ticks are validated but excluded from the timed 50,000 ticks. Per-tick timer
overhead matters for idle ticks but is negligible next to the active execution.

TPS is game ticks divided by timed interpreter seconds. The 50,000-tick average
includes the long idle tail. The benchmark additionally reports an active window:
PM1 ticks 1 through 12,051 (startup through its `shut` output), and ANPU ticks 1
through 5,000 (startup, screen animation, and settling). These are fixed windows,
not adaptive early exits, and all remaining ticks and outputs are still checked.

`baseline-performance.json` records repeated pre-optimization samples.
`optimized-performance.json` records the same benchmark after optimization.
`first-optimization-performance.json` preserves the earlier queue/wire-only results.
Machine and source provenance are in `environment.json`.
Relative benchmark report paths resolve from the workspace root, including when
Cargo launches the benchmark with the core crate as its working directory.

Measured on the Ryzen 9 5950X in `environment.json`, release/bench with fat LTO,
three samples per CPU:

| CPU | Baseline seconds / 50,000 ticks | Optimized seconds | Baseline average TPS | Optimized average TPS | Optimized active-window TPS |
| --- | ---: | ---: | ---: | ---: | ---: |
| PM1 SORT | 149.272 | 93.226 | 335 | 536 | 129 |
| ANPU Pong | 10.439 | 6.278 | 4,790 | 7,965 | 797 |

Median simulation time decreased by 37.5% for PM1 and 39.9% for ANPU versus the
recorded baseline. The six optimized full runs passed every frozen assertion.
These are local simulation-throughput measurements, excluding networking and
rendering. Active windows include startup; their fixed lengths are defined above.
Samples vary (PM1 88.915–97.442 seconds, ANPU 6.179–6.534 seconds), so the medians
describe these runs rather than a guarantee for other machines or CPU workloads.
All 188 ordinary workspace tests also passed with the final runtime changes.

Later client-update work reuses the existing render settings and moves visual
encoding/compression and writes to an ordered background sender. Its diagnostics
and separate validation are described in [client-updates.md](client-updates.md).
The CPU timings above were recorded before that client-path change and measure
interpreter throughput without connected clients.

The subsequent three interpreter optimizations, memory costs, command controls,
and newer measurements are described in [heavy-interpreter.md](heavy-interpreter.md).
`heavy-optimization-performance.json` preserves their three-sample CPU results;
the original references and earlier performance reports remain unchanged.

## Capture deliberately

The capture tool refuses to replace an existing CPU JSON file. Use a new output
directory and the unoptimized interpreter when establishing a new reference:

```powershell
cargo run -p mchprs_core --example cpu_references --release -- target/new-cpu-references
# Optional second argument: pm1_sort or anpu_pong
```

Do not regenerate references simply to make an optimization pass. The current
references were captured before the queue and neighbor-cache changes.
