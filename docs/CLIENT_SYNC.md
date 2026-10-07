# Client synchronization

The world is authoritative. Presentation throttling, lamp-screen projection,
packet coalescing, and neighboring snapshots do not change interpreter or
compiled simulation state. Their implementation is in
[player.rs](../crates/core/src/player.rs),
[packet_handlers.rs](../crates/core/src/plot/packet_handlers.rs),
[screen_updates.rs](../crates/core/src/plot/screen_updates.rs), and
[outbound.rs](../crates/network/src/outbound.rs).

## Interactions and position changes

Predicted block interactions use a block-action acknowledgement. Accepted
external edits and authoritative corrections are sent before that acknowledgement,
including when ordinary visual flushing is disabled or lamp-only mode is on.
These ordered packets close the prediction sequence; a later presentation
frame cannot substitute for an omitted correction.

Teleportation establishes a pending ID in
[TeleportState](../crates/core/src/player/client_sync.rs). Movement is fenced
until the matching confirmation arrives. Old or duplicate confirmations cannot
release a newer teleport; retries use the same ID at one-second intervals.
Changing plot/view context also updates the chunk states delivered to the player.
Neighboring plots are snapshots rather than independently simulated client worlds.

## Presentation cadence

`fast_render_threshold` and `fast_render_send_rate` control visual cadence.
The defaults are 200 configured TPS and a 10 Hz cap. Unlimited TPS also selects
the cap; a zero threshold disables automatic throttling. A smaller plot `/wsr`
limit still wins, and `/wsr 0` disables ordinary delta flushing. Piston animation
selection is separate from this cadence.

`/screenonly on` sends lamp-screen changes and the replacement states needed to
clear removed pixels. It suppresses piston animation and retains other changes
in authoritative storage overlays. Disabling it sends current suppressed states.
Fresh chunk snapshots, saves, and world reads use authoritative storage. Retained
overlay memory is the cost of skipping ordinary presentation deltas.

## Ordered outgoing queue

Each connection has a background writer shared by its packet senders. Adjacent
visual batches merge by section/position, retaining the latest state. Other
packets are ordering barriers, so updates do not merge across chunk changes,
interaction acknowledgements, chat, or other ordinary packets. Compression mode
is captured when enqueueing to preserve login transitions. Encoding visual
sections, framing/compression, and socket writes happen in the writer.

The estimated queue-memory cap is 128 MiB and the item cap is 16,384. Exceeding
either closes the connection rather than leaving a silently stale client. Writes
time out after five seconds. Normal close drains pending packets; write failure
discards that disconnected client's backlog. The last sender wakes the worker
so it can finish and stop.

`/tps timings` exposes simulation, collection/enqueue, writer, byte, and queue
counters. They are diagnostic counters with different lifetimes, not a single
atomic snapshot. Compare deltas over the same interval. Interpreter-only CPU
benchmarks exclude connected-client costs unless explicitly configured otherwise.

## Regression checks

```sh
cargo test -p mchprs_core --lib --locked client_sync
cargo test -p mchprs_core --lib --locked screen_updates
cargo test -p mchprs_network --locked outbound::tests::
```

[Player](../crates/core/src/player/client_sync_tests.rs) and
[plot](../crates/core/src/plot/client_sync_tests.rs) tests decode real protocol
bytes for teleport, chunk, correction, and acknowledgement order. Network tests
also check queue coalescing/barriers, compression transitions, drain, and limits.
See [test suites](tests/README.md) for wider checks. These establish the tested
server packet behavior, not every client rendering or proxy topology.
