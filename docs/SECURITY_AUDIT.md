# MCHPRS permission and input audit

Date: 2026-10-05. Scope: the current Rust backend, dedicated LuckPerms policy,
commands and aliases, incoming play packets, WorldEdit, plot ownership/database,
containers/signs, command blocks, history and deployment isolation.

## Findings and fixes

- **Command dispatch could switch actors.** A teleport removed a player from
  the plot, but their remaining queued commands could execute using the next
  player at that index. Dispatch now stops when the original player leaves.
- **Temporary group grants could outlive membership.** Permission expiry now
  includes every membership/inheritance edge. Alternate permanent paths remain
  valid, and inheritance cycles terminate.
- **Permission revocations required reconnecting.** Background reads refresh
  permissions during sessions. Caches stop granting access after 30 seconds if
  a refresh fails or stalls. LuckPerms remains read-only from MCHPRS.
- **WorldEdit could panic or allocate excessive work.** Invalid numeric IDs,
  zero/infinite pattern weights, excessive patterns, out-of-height selections,
  oversized numeric arguments and overflowing clipboard geometry are rejected.
  Move/stack/paste destinations are checked before undo capture or mutation;
  operations cannot silently cut blocks and paste outside the plot.
- **Repeated undo snapshots could grow indefinitely.** Retained undo/redo is
  bounded by configured block counts, including redstone stack operations.
- **Client input could store invalid positions.** Movement and teleport reject
  nonfinite or extreme positions; rotations must be finite. Saved positions,
  rotation, hotbar slots, inventory and speeds are validated before login.
- **Forged build packets lacked consistent spatial checks.** Placement and
  digging check plot bounds, height and reach before world access. Placement
  also checks hand/cursor values. Creative inventory packets require creative
  mode and the dedicated inventory permission.
- **Flooding could grow incoming queues indefinitely.** Per-connection queues
  and receive batches are bounded; a full queue disconnects the flooding client.
  Chat/commands have length, character, rate and queue limits.
- **Plot claims were not atomic.** Plot and owner writes now share one SQLite
  transaction. Duplicate claims cannot change ownership, failed writes roll
  back, and coordinates have a unique index. User/claim/read errors are handled
  instead of panicking command dispatch. Automatic plot search is bounded.
- **Command-block chat could flood the whole server.** Only the existing
  `say`/`tellraw` whitelist executes. Automated output is limited to the current
  plot, 64 messages and 64 KiB per second, with a bounded pending queue. Imported
  looping chains retain the existing 256-link execution bound.
- **Schematic completion followed directory links.** Completion skips links,
  requires load/plot permission, caps results and tolerates non-UTF-8 filenames.
  Existing schematic load/save containment and read-only `rf` protections remain.
- **Client prediction acknowledgements preceded block updates.** Placement and
  digging now acknowledge after authoritative updates enter the ordered writer,
  including rejected actions. This fixes the packet order that can make predicted
  placements disappear and reappear. A socket test checks the order with and
  without compression; a visual game-client check has not been performed here.

## Current rank policy

See [every rank and its permissions](MCHPRS_RANKS.md).

- Admin/Moderator: all MCHPRS permissions; no permission-based history tick cap.
- Engineer: own-plot features; history up to 1,000 game ticks.
- Expert/Advanced: own-plot features; history up to 200 game ticks.
- Builder: own-plot features and claiming; `/rhistory` and `/rback` disabled.
- Player: join/chat, spectator access; edits and backend commands denied.
- `/curse` and `/bless`: Admin/Moderator only.
- Expert/Engineer have explicit history grants because their parallel inheritance
  from Builder would otherwise tie against Builder's new denials.

Dedicated policy changes use `server=mchprs`; Paper permissions and inheritance
were preserved. The read-only database account cannot assign ranks. Individual
user overrides can deliberately change the baseline.

## Resource configuration

```toml
max_command_ticks = 10000
command_work_time_ms = 250
worldedit_max_blocks = 4194304
worldedit_history_blocks = 8388608
```

`/radvance` validates the requested count and yields after the time budget,
reporting how many ticks completed. A single expensive tick can still exceed
that budget. WorldEdit limits include stack work; retained undo/redo limits are
block counts, not exact byte accounting for block-entity metadata. Operators can
adjust these settings in the existing configuration file.

Production Compose limits MCHPRS to 6 GiB RAM, no extra swap, and 4 CPU cores.
These limits reduce its ability to exhaust Paper's resources. Official-account
authentication remains on Velocity; the backend has no published host port.

## Verification and limits

Regression coverage includes actor changes, relative-coordinate overflow,
aliases, group/path expiry, stale-cache denial, rank/history inheritance,
atomic/duplicate claims, malformed geometry/patterns, command-block privilege
rejection/output limits, and compressed/uncompressed block-update/ACK ordering.
Core and network library tests are run before deployment. The separate live
PostgreSQL test and synthetic sender benchmark are normally ignored.

This is a source audit with targeted regression tests, not a proof that arbitrary
redstone circuits cannot exhaust the backend. Per-plot simulation memory/CPU
accounting is still incomplete. Large block-entity data and expensive ticks can
consume the container budget; an OOM kill can lose unsaved progress. Atomic
saves/backups preserve earlier on-disk data, but cannot recover unsaved edits.
The reader supports static server/world contexts, not every LuckPerms feature.
Cached chat rank display/command suggestions fully refresh on reconnect.

Database backup before these policy changes:
`/srv/mchprs/deploy-backups/security-20261005/rf.dump`.

Verified: **189 core tests and 21 network tests passed**, including the real
socket block-update/ACK order test in both compression modes. Two optional tests
were ignored (live PostgreSQL integration and the synthetic sender benchmark).
