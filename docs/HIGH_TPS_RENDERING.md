# High-TPS rendering

The recovered 1.18.2 fork suppressed piston action packets above 200 TPS and at unlimited TPS. Intermediate moving-piston states still reached clients.

The current implementation automatically uses static piston rendering **above 100 TPS**, including unlimited TPS. At 100 TPS and below, normal animation packets remain enabled. Clients see carried blocks and piston heads directly instead of moving-piston placeholders; moving block entities and piston action packets are omitted. This applies to both incremental changes and initial chunk snapshots. Note-block sound packets are also suppressed in this mode.

`Config.toml` adds:

```toml
fast_render_threshold = 100
fast_render_send_rate = 10
```

Set `fast_render_threshold = 0` to disable automatic fast rendering. The fast send rate is a cap in Hz, clamped to 1–1000. A lower `/wsr` remains effective, and `/wsr 0` still disables periodic changes. `/wsr` reports both the configured and effective rate. Switching modes refreshes visible chunks with active piston movement so paused movements also display correctly.

These changes affect client presentation. Logical movement, scheduled ticks, piston events, neighbor updates, nano/pico stepping and saved state keep their existing behavior. Chunk snapshots now include pending block changes without consuming the next incremental update.

`/piston_anim [auto|on|off]` controls the current plot. `/bisdon_anim` is an alias. With no argument it reports the saved preference and effective state. `auto` follows the threshold; `on` restores normal animations and sending even above it; `off` forces static performance rendering at any TPS. The preference survives reconnect/restart in plot save format 5. Permission: `commands.piston_anim`.

Validation covers the strict threshold, unlimited mode, disabled sends, snapshot palettes/entities/heights, identical movement state and tick queues in both modes, and independent 1.21.5 clients receiving static heads without animation actions or moving-piston updates. A default 60 Hz send rate becomes at most 10 Hz during fast rendering; the actual byte reduction depends on the circuit and other packets.
