# Plot Git: feature scope

Status: scoped on 2026-10-06 and implemented in `crates/core/src/plot/git/`.
This document retains the design scope and acceptance checklist. See
[Plot Git usage](PLOT_GIT.md) for the implemented command interface, permissions,
limits and operational behavior. Client appearance/performance checks remain manual.

## 1. Purpose and first release

Give each plot persistent, named versions of its complete build and redstone
execution state. Players should be able to save meaningful milestones, search
their history, experiment on divergent branches, return to a branch, and inspect
changes between two versions through glowing world markers and optional inspection.

The first release includes all four requested capabilities and glowing visual
diffs. Implement them in stages so persistence and restoration are established
before visualization. Use a native snapshot repository; installing the Git CLI
or managing a conventional `.git` directory is not required.

### Included

- Manual commits with an ID, parent, author, UTC date, and message.
- Persistent history, pagination, and message searches.
- Branch creation from the current commit or an older commit.
- A single active branch or detached commit shared by everyone on the plot.
- Recoverable branch or commit checkout with simulation paused after restoration.
- Comparison of any two commits or branch tips in the same plot.
- Compact diff summaries, colored glowing markers, and optional sword-based inspection.
- Storage, memory, work, and marker limits.
- Plot membership and command permissions, command completion, and help.

### Deferred

- Merges, conflict resolution, rebases, cherry-picks, and partial commits.
- Remotes, GitHub integration, and interoperability with ordinary Git repositories.
- Personal working copies or simultaneous active branches on one plot.
- History rewriting, branch deletion, and automatic commit pruning.
- Automatic periodic commits, selective restoration, and cross-plot comparisons.
- Chunk deduplication and delta compression beyond whole-snapshot compression.
- A schematic-style before/after preview of an entire alternate plot.

## 2. Repository and snapshot semantics

A repository belongs to plot coordinates, not to a player. `HEAD` always names
an active branch. Each branch points to its latest immutable commit, and each
commit has at most one parent. Branches share their common ancestors.

Ordinary edits and simulation affect the working plot. A commit captures that
working plot and advances only the active branch. Creating a branch creates a
reference without copying the build or switching branches. A branch remains
available when another branch diverges.

The first successful commit creates the repository with branch `main` and a
root commit. No separate `/git init` command is necessary. Before that commit,
status reports an uninitialized repository and other history commands explain
how to create the initial commit.

### Snapshot contents

Capture the entire configured plot, including:

- Every block state, including powered states and other state properties.
- Block entities: container items and components, sign text, command-block data,
  and other supported saved block-entity fields.
- Scheduled block ticks, retaining their effective execution order.
- Piston events, moving-piston payloads, motion progress, and simulator state
  needed to continue the saved execution consistently.
- Format version, Minecraft data version, plot coordinates, and plot dimensions.

Do not version player positions or inventories, membership, ownership, permissions,
chat, ephemeral network queues, open menu sessions, or compiled Redpiler graphs.
Record the capture TPS as informational metadata. Checkout leaves TPS at zero;
current plot render settings and server configuration remain operational settings.

Snapshots must be internally consistent. Capture on the owning plot thread at a
game-tick boundary, after pending authoritative state has been materialized.
Export/materialize Redpiler blocks and scheduled ticks when it is active; taking
only the underlying world storage would miss compiled changes. If the world is
partway through a manually stepped tick, reject the capture with an actionable
message instead of silently advancing the circuit.

### Dirty status

Report two independent comparisons with the active branch tip:

1. **Build changes:** block states or block-entity data changed.
2. **Execution changes:** scheduled ticks or simulator state changed.

Running circuits can change both. A plot whose block arrangement looks unchanged
can still have execution changes. Recovery must preserve either kind of change.
An exactly identical snapshot is a no-op commit; explain that nothing changed
without adding another history entry.

## 3. Command interface

`/git` and `/git help` show usage. All commands operate on the player's current
plot. Arguments below use names rather than shell-style quoting: a commit message
or search query is the remaining text after the command and its flags.

| Command | Behavior |
| --- | --- |
| `/git status` | Show active branch, tip ID, build/execution changes, busy state, and storage usage. |
| `/git commit <message>` | Capture the working plot and advance its active branch. |
| `/git log [page]` | Show the active branch's ancestry, newest first. |
| `/git log --all [page]` | Show all user commits, including divergent branch history. |
| `/git search [--all] [--page <n>] <text>` | Search messages case-insensitively; default to active-branch history. |
| `/git branch` | List branches, highlight the active branch, and show tip IDs. |
| `/git branch <name> [ref]` | Create a branch at the current tip or a supplied reference; do not switch. |
| `/git checkout <branch\|commit>` | Restore a branch tip or commit after preserving unfinished work; commit IDs enter detached HEAD. |
| `/git diff <from> <to>` | Prepare a comparison and show a compact summary with clickable glow controls. |
| `/git diff show` | Show glowing markers for the player's prepared comparison. Normally invoked by clicking Show glow. |
| `/git diff inspect <x> <y> <z>` | Optional coordinate fallback for inspecting a position in the player's prepared comparison. Ordinary inspection uses a sword. |
| `/git diff hide` | Remove the player's markers and stop marker updates; retain the comparison until it expires or is replaced. |
| `/git recoveries [page]` | List automatic recovery snapshots separately from normal commit history. |
| `/git recover <recovery-id> <new-branch>` | Create and check out a branch containing recovered work. Preserve current unfinished work first. |

References accepted by branch creation and diffs are branch names, full commit
IDs, unique commit-ID prefixes of at least eight characters, and `HEAD`. Resolve
branch tips to immutable commit IDs when the operation starts. Reject ambiguous
prefixes, unknown references, and references from another plot.

Suggested input limits are 256 characters for messages, 128 for search text,
and 48 for branch names. Branch names use `[A-Za-z0-9][A-Za-z0-9_-]*`, are
case-sensitive, and cannot be `HEAD` or look like a commit-ID prefix. Repository
paths are derived exclusively from validated plot coordinates and generated IDs.

### Example workflow

```text
/git commit Working adder
/git branch experiment
/git checkout experiment
# Edit the circuit.
/git commit Smaller carry circuit
/git diff main experiment
# Click Show glow, then right-click a glowing change with a sword to inspect it.
/git checkout main
```

Logs show the short ID, UTC timestamp, author name, branch-tip labels where
applicable, and message. Store author UUID as the durable identity. Default to
10 results per page and provide clickable previous/next controls. A search result
also identifies its commit and date. Include a stable sequence as a secondary
ordering key so equal timestamps do not make pagination inconsistent.
Pagination applies to commit history, search, and recovery lists. Diff results
never enumerate changed blocks in chat and have no result pages.

## 4. Branch/commit checkout and recovery

Checkout replaces the shared working plot. Announce the destination to players
on that plot. A member committing work does not gain authority to replace it.

A commit ID or unique prefix enters detached HEAD without moving branch tips.
Detached commits advance HEAD and remain in `/git log --all`; `/git branch <name>`
names that history. The shared detached position survives restarts and recovery.

The required sequence is:

1. Authorize the operation and resolve the destination branch tip or commit.
2. Load and validate the destination snapshot before changing the live world.
   Validate plot identity, dimensions, format/data version, checksums, and state.
3. At a tick boundary, temporarily pause simulation and serialize plot-changing
   operations. Materialize compiled state and capture the current working plot.
4. If it differs from the current tip, durably save a recovery snapshot. Record
   its ID, author, date, source branch/tip, and destination branch. Abort checkout
   if recovery cannot be saved, including when storage is full.
5. Persist a checkout operation record sufficient to finish or roll back the
   operation after a crash.
6. Close player menus and reset Redpiler. Apply the validated world snapshot,
   scheduled ticks, and piston state together. Clear incompatible tick history,
   WorldEdit undo/redo, and stale interpreter/render caches.
7. Keep players connected. Refresh their authoritative chunks even if periodic
   world updates are disabled. Move players intersecting restored blocks to a
   validated safe position; define a safe fallback or reject checkout if none
   can be established.
8. Durably save the restored working plot and publish the active branch or detached HEAD.
   Complete the operation record only after the save and repository state agree.
9. Remove stale diff markers, announce completion and any recovery ID, and leave
   simulation paused. Players resume it with existing TPS/advance commands.

Recoveries are durable snapshots reachable through `/git recoveries`, not hidden
files that players cannot restore. They do not advance the source branch or
appear as user-authored milestones in ordinary logs. Recovery creates a new branch
and commits the recovered state there, retaining the source tip as its parent.

Before applying a destination, failures leave the working plot and active branch
unchanged and restore the previous simulation pacing. After application starts,
rollback uses the captured previous state, or startup completes the recorded
checkout. The implementation must never silently accept a saved plot/HEAD mismatch.
On startup, resolve pending operations before admitting players to the plot.

Checkout of the already active branch is a no-op; it does not discard edits.
Branches cannot be checked out by commit ID in the first release. To work from
an old commit, create a branch at that commit and check out that branch.

## 5. Commit comparison

Compare logical content, not raw save-file bytes. Palette layout, compression,
hash-map ordering, and serialization order must not create false differences.

### Comparison pipeline

1. Resolve both references and load compatible immutable snapshots.
2. Compare canonical section hashes. Identical sections can skip block decoding.
   Compare block-entity content independently; identical blocks do not imply
   identical container contents or commands.
3. Decode differing sections and compare block states at matching coordinates.
   Within the same Minecraft data version, validated state IDs are sufficient
   for equality. Use names and properties for human-readable output.
4. Compare block entities by kind and canonical, typed contents. Sort map keys
   and inventory entries by slot; preserve meaningful list order, NBT types,
   and simulation queue order. Do not equate distinct types accidentally.
5. Build one change record per position, retaining before/after states and data.
6. Calculate separate execution-state counts and differences. Keep detailed
   execution information available on demand rather than listing it automatically.

Define `from -> to` categories as follows:

| Category | Definition | Marker |
| --- | --- | --- |
| Added | Air in `from`, non-air in `to`. | Green |
| Removed | Non-air in `from`, air in `to`. | Red |
| Changed state | Different non-air block types or state properties. | Yellow |
| Changed data | Same block state but different block-entity contents. | Yellow |

Air includes normal, cave, and void air. Each position contributes to exactly one
category. A record can still flag both state and data changes for its detail view.
Use the same definitions for hashes, equality, counts, inspection, and markers.

Block properties such as repeater delay, facing, and powered state count as
changes. An optional future filter may hide dynamic power changes, but the first
release compares the actual captured states without silently omitting them.

Compute exact summary counts even when only a limited set of nearby markers is
displayed. Stream the comparison and retain bounded section indexes and marker
records; do not build an unbounded vector or a chat list of every changed block.
Fetch a position's before/after details from the pinned snapshots on demand.

### Chat presentation

```text
main [a12b34cd] -> experiment [e56f78ab]
14 added, 3 removed, 8 changed

[Show glow] [Hide glow]
```

Keep this UI the same size whether ten blocks or ten million blocks changed.
Combine changed-state and changed-data counts into `changed` in the compact
summary; preserve the separate categories internally for inspection. If only
execution state changed, report that briefly instead of displaying an empty
block list. Otherwise expose execution details on demand.

The comparison labels are clickable to inspect each commit's author, date, and
message. Show glow enables the private markers; Hide glow removes them. When
markers are capped, show a concise nearby-marker count without adding block rows.
Unchanged world blocks stay visible as context.

When glow is enabled, display a single short hint: `Right-click a glowing change
with a sword to inspect it.` Any vanilla sword works, in either hand, without a
special item, inventory modification, or an additional tool-selection command.
Use a sword so this action does not conflict with the compass teleport tool or
the WorldEdit axe.

Right-click with a sword -> immediately show the diff of the pointed block.
Use the position under the crosshair in the active glow overlay and compare its
`from` and `to` snapshot contents. Show one short before/after result, for example:

```text
(122, 64, 80) Repeater
From: delay 1
To:   delay 4
```

Show changed saved data for both sides in this same result, including commands,
sign text, or container contents. Inspection never replaces blocks or switches
branches. Ordinary use does not require extra clicks, entering coordinates, or
memorizing extra commands. Coordinate inspection remains available as a fallback.

### Sword inspection behavior

- Enable sword inspection only while this player's glow overlay is active.
  Hide glow, expiry, or leaving the plot immediately disables the tool behavior.
- Each successful sword right-click directly displays the pointed block's states
  and changed saved data from both snapshots. No detail buttons are required.
- Trace the player's aim against their active changed-position markers, including
  removed positions now containing air. Select the nearest intersected marker
  within the configured overlay radius and loaded chunks. Display entities are
  not inherently clickable blocks; perform this lookup on the server.
- A glowing marker visible through other blocks can be inspected using the same
  bounded aim lookup. Inspection reads snapshots and never changes terrain.
- Handle right-click-on-block and right-click-in-air packets so a removed block
  is just as easy to inspect as an existing block. Validate the actual held sword
  and hand on the server; do not trust an arbitrary client-supplied target.
- A successful inspection consumes that click before ordinary block/container
  interaction or Redpiler handling. Do not open a chest, toggle a lever, change
  a repeater, place/break a block, or reset compilation as a side effect.
- Deduplicate the packets Java can send for one click and suppress any secondary
  hand fallback for a consumed inspection. Rate-limit repeated details without
  letting throttled inspection clicks mutate the selected block.
- If no highlighted position is aimed at, preserve the ordinary right-click
  behavior. Do not emit a chat error on every miss. Other items and left-clicks
  retain their normal behavior.
- Require the player's diff read/visual permissions, rather than build access,
  for this read-only operation, and recheck access before returning details.

The explicit coordinate command remains a fallback, including while glow is
hidden. The ordinary flow uses Show glow and a single sword right-click; there
is no chat Inspect button, additional detail click, or toggle mode.

Long commands, NBT, and item contents are abbreviated in the inspection output.
Bound the output, escape user-controlled text, and explain truncation; do not
introduce pagination of changed-block results. Execution details describe changed
scheduled ticks or piston motion without implying they all have visible markers.

The comparison is always between the two requested commits, even when the current
working plot differs from both. Show both IDs and explain that world markers refer
to snapshot coordinates, not necessarily the current world's block contents.

## 6. Glowing visual diffs

Use Minecraft Java's entity glowing outline to mark changed positions. Placed
blocks do not have an entity glowing flag. The proposed mechanism is temporary
`block_display` entities sent only to the player viewing the diff, with the
glowing flag and a `glow_color_override` for green, red, or yellow.

Display entities support custom glow colors and have no collisions or physics;
see Mojang's [display-entity documentation](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-19-4).
The repository targets Minecraft 1.21.5 and contains the `block_display` entity
in its checked-in registry data.

### Appearance and prototype gate

Use consistent cube markers for changed coordinates, including now-empty removed
positions and blocks whose models are small or do not render as block displays.
The color identifies the change category; chat/details identify the actual block.

Prototype the marker appearance before committing to the rendering method:

- Check visibility beside, inside, and behind existing blocks.
- Check removed positions, slabs, wire, containers, and dense adjacent changes.
- Verify color, view distance, resource-pack behavior, and overlap artifacts.
- Do not assume making a block display invisible produces an outline-only cube.
  Block displays can render their block model as well as an outline.
- If display cubes obscure the build, prototype another glowing entity marker
  or a border made from transformed display elements. Choose the simplest
  vanilla-client option with acceptable readability.

The first visual milestone must establish this appearance with a real 1.21.5
client. The compact summary and coordinate inspection remain available if the
visual session cannot be displayed.
No client mod or resource pack is required by the planned feature.

### Session behavior

- Markers are a per-player packet overlay. Do not create real world blocks,
  simulate marker entities, or put them in commits or ordinary plot saves.
- Allocate entity IDs through a shared allocator that cannot collide with players
  or other entities. Add dedicated spawn/metadata/destruction helpers as needed.
- Keep one prepared comparison per player, with a separately enabled/disabled
  marker overlay. A new comparison replaces the old one.
- Show only changes in loaded chunks within a bounded radius. Update the marker
  set when the player moves, with bounded per-tick spawn/destruction work.
- Prefer nearest changes when a marker cap is reached. Show `shown/total` counts;
  marker limits never change the exact comparison result.
- Remove markers on `hide`, plot exit, disconnect, checkout, expiry, or loss of
  permission. Clean up replaced sessions and never reuse live marker IDs.
- Player interaction with a marker must not edit the plot or affect simulation.
- Keep the comparison pinned to its resolved commit IDs while the player moves
  or someone advances a branch.
- Hide disables only the overlay; Show glow can re-enable the prepared comparison
  without making the player re-enter references. Sword inspection is disabled
  while hidden; explicit coordinate inspection remains available.

Proposed starting limits: 128 active markers per viewer, a 64-block radius,
32 marker operations per update, and a five-minute session timeout. Make these
configurable and measure client FPS, packet volume, and server cost before fixing
production defaults. Markers follow the player through the plot so large diffs can
be explored spatially. Coordinate inspection remains available for positions
outside the active marker radius; do not fall back to a large chat block list.

## 7. Persistence and crash behavior

Store each repository separately from the ordinary plot save, for example:

```text
world/plot-git/p<X>,<Z>/
  repository.sqlite
```

Metadata records include commits, branch tips, active branch, snapshot objects,
recovery records, and pending checkout operations. A commit records its parent,
snapshot ID, author UUID/name, UTC date, message, and sequence. Snapshot objects
record format/data versions, dimensions, compressed/decompressed sizes, checksums,
and canonical content/section hashes.

Use SHA-256 over versioned canonical snapshot content for snapshot IDs. Derive
commit IDs from canonical commit metadata, including parent and snapshot ID.
Never use incidental compressed bytes or hash-map iteration order as identity.
Unchanged identical snapshots can share an existing object; different builds
initially store full compressed snapshots rather than chains of deltas.

The implementation stores compressed snapshot objects as SQLite blobs alongside
metadata. Object insertion and reference advancement share one fully synchronized
transaction. A failed write must not advance a branch or publish an orphan object;
reachable commits and recoveries stay.

Checkout also changes the ordinary plot save, so a SQLite transaction alone is
insufficient. Use a durable operation record and an atomic plot-save replacement
to reconcile filesystem and HEAD state after interruption. Define and test each
failure point. Do not overwrite original snapshot objects during migration.

Existing plots require no migration until the feature is used. Incompatible future
format/data versions must produce an actionable error; never reinterpret old
numeric IDs as a new registry. Version migration/export is a subsequent feature.
Repository and ordinary plot data must both be included in deployment backups.

## 8. Permissions and shared work

Dedicated permission nodes:

| Node | Access |
| --- | --- |
| `mchprs.commands.git` | Single allow/deny gate for the complete feature: commands, completion, glow and sword inspection. |
| `mchprs.plots.admin.git` | Admin override for repository operations on another plot. |

Read access also requires plot ownership or membership unless the admin override
is granted. Commit and branch creation must follow existing plot-edit checks.
Checkout and recovery require ownership or the explicit admin override, in addition
to `mchprs.commands.git` and backend command access. An explicit Git denial
continues to apply to admins. Do not grant permissions by editing a live
LuckPerms database as part of implementation; document grants for operators.

Repositories share the plot's lifecycle. Ownership transfers update authorization
without rewriting commit authors. A future plot-reset/delete operation must state
what happens to its repository; do not silently attach old history to an unrelated
replacement build. Add explicit lifecycle handling before integrating such actions.

Serialize repository mutations per plot. Two concurrent commits cannot both
advance a branch from the same assumed tip. Pin references for read operations.
Authorize mutations when requested; accepted writes finish durably even if their
requester leaves. Recheck access before delivering read results or maintaining
overlays. Only one operation per plot can advance references at a time. Messages
are user text, not shell commands or filesystem paths.

## 9. Work limits and threading

Capture consistent state on the owning plot thread. Send owned immutable data to
a bounded background worker for compression, hashing, file/SQLite work, and diffs.
Workers must not read or mutate a live `PlotWorld` or access a player's connection
as mutable plot state. Deliver completion messages to the owning plot thread.

Allow only one repository mutation in flight per plot and limit outstanding read
jobs per player and globally. Changes may continue after a commit's capture;
report that the commit represents the captured instant. Serialize branch mutation
until that commit finishes. Checkout acquires a short exclusive plot-operation
window after destination preparation, without blocking packet/network processing.

Capture itself is not free: avoid cloning/serializing an entire large plot without
a memory reservation and timing measurement. Prefer existing snapshot machinery
and a bounded capture strategy. Never yield through an inconsistent capture while
allowing world mutations. Fail or briefly suspend edits/ticks if a consistent
bounded capture cannot otherwise be obtained.

Configure storage quotas per plot and globally, including recovery objects and
staging reservations. Configure maximum decompressed snapshot size, work memory,
concurrent jobs, query output, and visual sessions. Reject work that exceeds a
limit with an actionable message. Do not evict committed history automatically.

A plot currently contains roughly 16.8 million positions at the default
256 x 256 x 256 dimensions. Section hashes and bounded spatial diff queries are essential;
do not allocate one entity or one permanent in-memory record for every change.

## 10. Repository integration points

| Existing area | Planned integration |
| --- | --- |
| `crates/core/src/plot/commands.rs` | `/git` dispatch, command declarations, completion, and mutation authorization. |
| `crates/core/src/plot/packet_handlers.rs` | Sword right-click interception for active diff overlays, aim lookup, hand validation, and duplicate-click suppression. |
| `crates/core/src/plot/help.rs`, `messages.rs` | Usage, help topic, bounded status/error text, and chat results. |
| `crates/core/src/plot/mod.rs` | Capture/restore lifecycle, plot-thread completion handling, client refresh, and save coordination. |
| `crates/core/src/plot/history/codec.rs` and `history.rs` | Reuse snapshot semantics and restoration lessons; keep persistent repositories separate from ephemeral tick buffers. |
| `crates/save_data/src/plot_data.rs` | Existing validation/versioning and world-state fields; introduce an explicitly versioned repository snapshot envelope. |
| `crates/core/src/player.rs` | Diff session ownership and shared entity ID allocation. |
| `crates/network/src/packets/clientbound.rs` | Display-entity spawn, metadata, and destruction helpers using checked-in 1.21.5 protocol data. |
| `crates/core/src/plot/neighbors.rs` | Preserve normal neighbor refresh after checkout; never use stale viewer snapshots as commit input. |
| `docs/MCHPRS_PERMISSIONS.md` | New nodes and suggested grants, following the existing deployment conventions. |

Suggested new modules are `plot/git/` for command/repository coordination,
snapshot codecs, comparison, recovery, and tests, plus a small visual-session
module. Keep storage logic testable independently of networking and live plots.

## 11. Delivery stages and acceptance criteria

### Stage 0: snapshot contract and glowing prototype

- Confirm interpreted/compiled state export and tick-boundary behavior.
- Define canonical snapshots, equality, hashes, and version compatibility.
- Prototype colored markers with a real vanilla 1.21.5 client, including removal
  markers and dense changes. Record the selected rendering approach.

### Stage 1: persistent commits and history

- Implement repository storage, initial `main`, commit, status, log, and search.
- Add command completion/help, author/date display, permissions, and quotas.
- Demonstrate that commits survive server restart and retain container/command
  data, scheduled ticks, and moving-piston state.
- Failed or interrupted commits never advance a branch or corrupt an object.

This stage is the smaller useful release if the complete feature needs to ship
incrementally. It preserves whole-plot milestones but does not claim to provide
branch checkout or visual comparison yet.

### Stage 2: branches, checkout, and recovery

- Create divergent branches at current or historical commits.
- Restore one branch without changing another branch's tip or ancestry.
- Preserve unfinished work, expose recoveries, and recover onto a new branch.
- Restore runtime state consistently, leave simulation paused, refresh clients,
  and maintain safe player positions and existing plot access controls.
- Exercise restart/failure points around recovery, plot save, and HEAD publication.

### Stage 3: comparison engine and compact inspection UI

- Compare commits in either direction with exact category counts and a compact summary.
- Cover state-only, data-only, combined, and execution-only changes.
- Produce no false differences for reordered map keys or equivalent palette layouts.
- Keep unchanged sections cheap and dense diffs bounded in memory and output.
- Show before/after states and changed saved data immediately for an explicitly
  inspected position. Make commit metadata clickable; never list changed blocks automatically.
- Verify that chat output stays compact for millions of changes, with no block
  result pagination or coordinate-navigation workflow required.

### Stage 4: glowing visual comparisons

- Add private green/red/yellow markers, including removed positions.
- Bound active markers and work; track loaded chunks and player movement.
- Clean up on hide, replacement, expiry, checkout, exit, and disconnect.
- Verify marker privacy, unique entity IDs, unchanged world state, client FPS,
  and readable outlines in dense builds.
- Verify Show/Hide controls, re-enabling a hidden comparison, and single-click
  sword inspection of changed and removed positions with immediate From/To data.
- Cover either hand, clicks in air, clicks through glow-visible obstructions,
  duplicate packets, repeated clicks, read-only viewers, and misses. Inspection
  must not interact with containers/redstone or reset Redpiler; hiding glow must
  immediately restore ordinary sword right-click behavior.

### Verification across stages

Use focused storage/codec tests, integration tests for plot capture/restore and
permissions, fault injection for interrupted persistence, and actual client checks
for visuals. Include concurrent builders, compiled circuits, moving pistons,
containers, `/wsr 0`, ambiguous IDs, invalid names, full quotas, corrupt objects,
and incompatible versions. Compare resumed redstone behavior with the captured
state, not only the visible block layout.

## 12. Decisions to validate during implementation

The feature behavior above is the proposed default. The remaining engineering
decisions are the final glowing marker geometry, measured production quotas,
the snapshot capture budget for large plots, and the exact crash-recovery protocol
for coordinating plot saves with repository metadata. Resolve these with prototypes
and tests without expanding into merges or Git interoperability.
