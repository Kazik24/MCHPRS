# Client updates and simulation throughput

The existing configuration controls general block-update cadence as well as
automatic piston presentation:

```toml
fast_render_threshold = 200
fast_render_send_rate = 5
```

This example caps visual block updates at 5 Hz when configured TPS exceeds 200,
or when TPS is unlimited. The existing defaults remain 100 and 10. The threshold
is based on configured TPS, not measured TPS. Setting the threshold to zero
disables automatic throttling. A lower plot `/wsr` limit still wins, and `/wsr 0`
still disables ordinary block-update flushing.

The cap applies even with `/piston_anim on`; piston animation and send cadence
are independent. `/piston_anim off` alone no longer throttles a low-TPS plot's
ordinary block updates. No duplicate visual-update configuration was added.
Simulation, command execution, and interactions retain their game-tick behavior.

Each connection has one background writer, shared by its player connection and
all plot packet senders. Visual section encoding, packet framing/compression,
and socket writes run there. Chunk snapshots and other ordinary packets enter
the same ordered queue. Compression mode is captured when enqueueing, preserving
the login compression transition.

Adjacent pending block batches merge by section and position, keeping the latest
state even when a client misses intermediate frames. Other packets form barriers:
updates cannot merge across chunk loads/unloads, entity data, chat, or interaction
responses. The worker releases the queue lock before encoding or writing. Server
world state is never replaced with the client presentation state.

Queues have a 128 MiB estimated-memory limit and a 16,384-item limit. A connection
that exceeds either limit is closed rather than blocking simulation or silently
discarding its latest state. Writes time out after five seconds. Normal close
drains the queue; send errors discard the disconnected client's backlog. Dropping
the last sender wakes and terminates its worker after draining.

## Measurements

`/rtps timings` reports cumulative plot simulation time and ticks, visual flushes,
sections/records, collection time, and enqueue time. It also sums the current
clients' connection-lifetime packet counts, bytes, visual encoding time,
framing/compression time, socket-write time, queue bytes, coalesced positions, and
send failures. Queue size excludes an in-progress write. These counters are
diagnostics rather than synchronized snapshots. Ordinary packet encoding, such
as chunk snapshot construction, still occurs at its call site and is outside
the worker's visual encoding counter.

Record counter differences over the same active program interval when comparing
live clients; connection totals can include earlier plots and login. The CPU
benchmarks time only interpreter calls and should not be compared directly with
these live results.

A repeatable synthetic sender benchmark uses identical 10,000-tick computation
in three modes and a sink with a one-millisecond delay per write. It generates
500 visual frames of 256 positions and asserts that the final frame arrives:

```powershell
cargo test -p mchprs_network --release --locked outbound::tests::compare_no_client_synchronous_and_background_sending -- --ignored --nocapture
```

One local run on the baseline machine:

| Mode | Producer time | Time through final delivery | Delivered frames |
| --- | ---: | ---: | ---: |
| No client | 1.488 ms | 1.490 ms | 0 |
| Synchronous sender | 778.922 ms | 778.922 ms | 500 |
| Background sender | 2.285 ms | 4.497 ms | 3 |

The background run coalesced 127,232 repeated position updates and delivered the
exact final state. This demonstrates separation from a slow writer; it does not
measure live Minecraft clients or predict the CPUs' live TPS. Network tests
also verify exact ordered protocol bytes, compression transitions, close/drain,
queue limits, and sender lifetime. The final ordinary workspace test run passed
194 tests; the synthetic benchmark passed separately.
