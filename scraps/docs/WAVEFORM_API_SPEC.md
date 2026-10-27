# Waveform capture and public API specification

Status: implementation contract, 27 October 2026. The Rust recorder/API lives in
`crates/core/src/waveform/`; plot commands live in `plot/waveform.rs`.
This specification supersedes the older analyzer
design's editor-only browser credentials, public X/Z naming, single command
permission and uniform 64-signal/1,000-tick allowance. The existing React viewer
remains the presentation layer. Implementation should update its typed data
adapter rather than introduce another viewer.

## Scope

- One loaded plot is one isolated recording source. A viewer selects exactly one
  plot at a time; switching cancels pending reads and discards the previous live
  model. Different viewers may select different plots concurrently.
- Everyone can read retained waveforms and recorded command-block chat output,
  without Minecraft login, plot membership, presence or bearer credentials.
- Minecraft commands control probes and recording. Each subcommand has its own
  permission, in addition to the existing command-access and plot-edit checks.
- Record binary block/component transitions and command-block text events in one ordered,
  bounded, in-memory history. Recording does not depend on a browser being open.
- Save named probe definitions (names, order and absolute block positions) in
  `world/plots.db`, alongside plot metadata. Load them with recording off; never
  persist captures, command output or the recording-enabled state. Definition
  changes are committed before acknowledging the command; failed writes preserve
  the existing recorder. Rewind/Git restores keep the current definitions.
- Browser layout preferences live in local storage per plot; captures and event
  text stay out of local storage.
- Transport is a bounded HTTP JSON snapshot followed by Server-Sent Events (SSE),
  batched every **250 ms**. REST delta reads remain available for recovery.
  No WebSockets, custom binary format, VCD polling or extension host is needed.

Only already-loaded plots can be queried. HTTP requests must never load a plot,
read its saved files, reset its unload timer, pin it in memory or advance its
simulation. Existing player/always-running simulation behavior remains decisive.
An empty plot can remain loaded without ticking; the viewer must show that fact.

## Identity and time

Public plot identity is signed integer **x/y plot indices**, as requested. Public
`(x, y)` maps directly to internal `(PlotWorld.x, PlotWorld.z)`. Public plot `y`
is the second horizontal plot axis, never Minecraft elevation. APIs, URL paths
and plot-selector labels use x/y consistently. Probe and event-source positions
remain absolute Minecraft block `[x, y, z]`, with the usual vertical block y.
For example public plot `(3, -2)` is backend plot `(x=3, z=-2)`; no conversion to
block coordinates happens in the HTTP path.

Every loaded instance gets an opaque `source` generation that changes on unload
and reload, including process restart. Every capture gets an opaque `capture`
identity that changes on enabling, reset, active probe-set changes, restore and
other discontinuities. Never concatenate data across either identity change.
An old unload notification must unregister only its own source generation.

`tick` is the interpreter's logical game tick. `seq` is a strictly increasing
capture-local sequence, shared by signal changes and text events. Sequence order
preserves same-tick pulses and establishes whether a message happened before or
after an edge. It does not invent physical time inside a tick. Allocate sequence
numbers only for stored records, starting at 1; baseline position is sequence 0.
Record phases internally if needed for diagnostics, but v1 needs only tick/order
and `observed_tick`, `completed_tick`, `partial_tick` metadata.

Absolute ticks/sequences are decimal u64 strings in JSON. JavaScript parses them
as BigInt, subtracts an exact origin, then uses bounded relative numbers for
canvas math. Small signal IDs, page offsets, quotas and coordinates are JSON
numbers. Rotate the capture rather than wrap an exhausted counter. The nominal
timescale is 20 ticks/second, 50 ms/tick, regardless of actual plot TPS. VCD keeps
the approved `$timescale 1 ms $end` and multiplies ticks by 50.

## Commands and configurable allowances

The existing `Rank` enum has Player, Builder, Advanced, Expert, Engineer,
Moderator and Admin. [rank.rs](../../crates/core/src/permissions/rank.rs) explicitly
maps Builder to group `builder` and displays it with prefix B. Therefore B here
means **Builder**, not Player/default (whose prefix is G).

| Rank / existing group | Signals | Ticks |
| --- | ---: | ---: |
| Default / `default` | 16 | 100 |
| B / `builder` | 16 | 100 |
| Z / `advanced` | 64 | 1,000 |
| E / `expert` | 1,024 | 1,000 |
| I / `engineer` | 1,024 | 1,000 |
| M / `moderator` | 1,024 | 10,000 |
| A / `admin` | 1,024 | 10,000 |

These are allowances, not automatic command grants. A rank with no waveform
command permission cannot activate or mutate a recorder. Server configuration
must expose these defaults and hard safety ceilings; deployment permission data
assigns the numeric grants to groups. Reuse
`Player::numeric_permission_limit` / `PlayerPermissionsCache::numeric_limit`,
which already select the largest effective positive numeric node and respect
denials/expiry. Do not implement a separate hardcoded rank switch in every
waveform handler.

Proposed numeric nodes are `waveform.signals.limit.<n>` and
`waveform.ticks.limit.<n>`; dedicated mode uses
`mchprs.waveform.signals.limit.<n>` and
`mchprs.waveform.ticks.limit.<n>`. The deployment grants the numeric values in the table above. Explicit operator
configuration can change these limits;
there is no implied unlimited/admin bypass. Resolve the acting player's primary
rank once in a shared admission helper and apply the configured rank-profile
ceiling. Effective limits are the minimum of that profile, the actor's numeric
grants and configured server safety ceilings. Builder can inherit Player/default
grants of 1024/10000; the Builder profile must still clamp them to 16/100.
Do not rely on inheritance order, the largest numeric grant or repeated handler
rank switches to enforce the lower B allowance. Existing rank profiles supply
known primary-rank identity when enabled; deployments without that integration
must configure an explicit primary-group-to-profile mapping. Unknown/missing
external rank identity denies new allocation rather than selecting the larger
"other rank" profile. In dedicated mode a missing numeric grant means zero.
Standalone without a permission provider uses
the configured standalone policy, default 1024/10000, plus normal plot editing
rules. Unknown/unavailable external permissions deny new allocation, rather
than falling back to the more generous standalone policy. Wildcard command
permission does not synthesize numeric grants.

| Command | Required subcommand node | Result |
| --- | --- | --- |
| `/waveform add <name> [x y z]` | `commands.waveform.add` | Register a pointed/explicit non-air block; if enabled, start a new capture with changed watches. |
| `/waveform addbulk <name> <bits>` | `commands.waveform.addbulk` | Stage low-to-high left-click selection; register `name_0` through `name_<bits-1>` together. |
| `/waveform cancel` | Existing command access | Discard only the caller's unfinished selection, leaving watches/history untouched. |
| `/waveform remove <name>` | `commands.waveform.remove` | Remove probe; if enabled, start a new capture for remaining probes. |
| `/waveform list` | `commands.waveform.list` | List probes and current values, with clickable `/hl` coordinate links. |
| `/waveform clear` | `commands.waveform.clear` | Erase history/events in place; keep watches, recording state, tick window and reserved buffer. |
| `/waveform reset` | `commands.waveform.reset` | Warn without changing anything. |
| `/waveform reset confirm` | `commands.waveform.reset` | Disable, remove all watches/events/history, cancel selections and release storage. |
| `/waveform on [ticks]` | `commands.waveform.enable` | Enable a fresh capture, including zero-probe text-only recording; default 1,000 ticks capped by the actor's allowance; optional ticks selects another permitted window. |
| `/waveform off` | `commands.waveform.disable` | Disable instrumentation, discard history/events, release capture buffer; keep small watchlist. |
| `/waveform [status]` | `commands.waveform.status` | State, limits, usage, retained range and reason for stop. |
| `/waveform web` | `commands.waveform.web` | Show public viewer URL with this plot selected; no credential. |

All commands also require `commands.waveform` and `can_use_commands()`. Every
command in this family, including list/status, requires current
`can_edit_plot(owner, coords)` as originally requested. Dedicated resolver adds
the `mchprs.` namespace to command nodes. Register canonical/alias permission
checks, help and completion together. An HTTP reader needs none of these
permissions. `/waveform web` replaces the earlier credential-issuing proposal;
the public viewer already selects a plot by coordinates. Canonical on/off verbs
and enable/disable permission suffixes follow `/rhistory`; do not also add
redundant start/stop/ticks verbs. Omitted `on` ticks defaults to the acting
player's admitted tick allowance. Calling `on` while enabled starts fresh after
successful admission. Help and success messages explicitly state that `on` and
`off` discard existing history; download before disabling if it must be kept.

Keep existing design validation: valid name, unique name/position, non-air target,
bounded ray trace and block height/plot bounds. Reject invalid targets before
changing any existing recording. Names are text, not HTML. Signal slots are
stable within one capture and numbered from zero; enabled additions/removals start
a new capture with a new immutable schema. Dust/comparator output >0 is On.
Components with a powered state report that state; torches and lamps report lit.
Other blocks report incoming power using the interpreter's existing redstone
query. Missing/air blocks are unavailable (`null`); values remain binary.

Bulk selection consumes left-click digging packets without breaking blocks and
blocks ordinary right-click use while active. It cannot toggle a selected
lever/button or dispatch an item's powertool action. Every click rechecks current
permissions, bounds/reach, duplicate names/positions and the complete resulting
quota. Selected positions are rechecked against concurrent registrations before
commit. Existing watches/history remain unchanged until the final successful
click; failed admission or saving keeps the prior capture. Enabled capture
rotates once for the whole batch. Leaving the plot or cancelling drops selection.

The recorder is shared plot state, not a private allocation per viewer, member
or signal. At activation or any allocation-changing command, validate the entire
resulting probe count and requested tick window against the **acting player's**
current allowances. Do not add members' quotas together or use the most generous
viewer. Registering probes while off checks the probe allowance but does not
allocate capture/tick storage. A B player cannot append probe 17, enable a
1,024-probe capture or select 10,000 ticks because another member once did so.
They may disable/clear/reset it or remove
probes to reduce its footprint; reduction retains or reduces the previous
reservation and does not grant additional storage. Clear reuses the existing
capture reservation; it does not allocate a replacement or enlarge quotas. A later on/enabled add
must pass full current-actor admission. A user upgrading an existing capture
must choose an `on [ticks]` count within their grant before allocating it.

Command audit spans include the acting UUID; retain admitted limits in the
recorder. No actor identity goes in public API metadata. Admission is an explicit
allocation grant: it survives departure
and later permission changes until the next mutating command or configuration
enforcement. Commands always recheck current permissions; a downgrade does not
silently delete history or probes. This avoids needing an offline permission
service just to sustain capture. Server ceiling reductions stop any now-invalid
recorders with `limits_changed` and preserve the valid retained prefix; a current
editor can reconfigure/enable. No automatic ownership transfer or owner-rank
lookup is needed in v1.

Validate quotas, target and memory reservations before committing changes or
discarding a previous capture. Insufficient permission or global capacity leaves
the old recording intact. Successful discontinuities report that old history
was discarded. Tick count is a **maximum rolling simulation window**, not a
guarantee that all 1,024 probes can toggle for 10,000 ticks under the byte budget.

## Command-block text events

While recording is active, record successful command-block chat-output emissions
throughout the plot, regardless of the probe set. Explicit `/waveform on`
supports zero-signal text-only captures; adding a probe while off only registers
the watch. Off discards both channels and releases recording instrumentation;
clear empties both channels while keeping watches and recording state;
reset confirm also removes watches and stops recording. Bare reset only warns.
Removing the last probe keeps an
already-enabled recorder running for events. This replaces the older design's
automatic enable-on-first-probe and storage release on last probe removal.
No per-command-block watch registration or separate event entitlement is required
in v1.

Current supported commands are `/say` and `/tellraw`; this is not a promise to
execute arbitrary commands. Capture the **normalized output text**, once per
successful command-block emission, not the literal command or JSON input. `/say`
includes its actual `[source] ` prefix. Flatten normalized text components in
display order, concatenating `text` and nested `extra`, discarding formatting
and stripping recognized Minecraft legacy color/format codes. Keep Unicode and
newlines. Do not strip arbitrary ampersands or execute HTML/terminal escapes.
Future dynamic commands must record the already-evaluated output, never attempt
to evaluate the raw command again in the waveform subsystem.

The hook belongs in
`PlotWorld::execute_command_block`, after parsing and existing output-limit
checks succeed and at the authoritative `command_messages` enqueue boundary.
The delayed `Plot::update` drain may deliver messages after multiple ticks and
fan them out to many players, so it must **not** generate waveform events.
Allocate event tick and shared sequence at enqueue time. Never hook ordinary
player chat or the separate player `/say` and `/tellraw` dispatch path.
Malformed commands, unsupported commands, conditional blocks that do not run
and emissions rejected by existing output limits generate no text event.
Command-block `last_output` success/error diagnostics are not chat emissions;
`track_output=false` must not suppress valid captured chat text.

Emit once for an accepted message even if there is currently no matching
recipient; capture records output produced by the circuit, not packet delivery.
Thus command-block `tellraw @s` currently has Recipient::Nobody but can still
produce a successful recorded emission. Existing chat routing is unchanged.
Targeted `/tellraw Player ...` output will be publicly visible through this API,
even though in-game delivery targets one player. Public probes expose circuit
behavior too. This is a direct consequence of public reads; communicate it in
waveform help and deployment documentation. Do not expose recipient identities,
player chat, raw command strings or unrelated stored command-block data.

Include source block coordinates for stable event grouping. The current
`World::execute_command_block_at(pos, command, source)` supplies the executing
`BlockPos`. Its default forwards to the existing positionless hook, preserving
existing mocks and graph forwarders. Physical interpreter execution uses the new
hook; PlotWorld captures only after successful parsing and output admission.
Compiled recording remains unsupported; compilation is blocked while recording.

The viewer renders one Events lane with dots on ticks and a count where events
overlap. Hover/focus/click previews plain text. Expansion shows all command-block outputs on one shared timeline row, aligned
with their ticks. Same-tick messages share one label and full-text preview,
ordered by `(tick, seq)`. Multiple emissions in one tick remain separate,
including identical text; search or display folding must not delete records.
Event visibility/expansion are browser presentation state, not recording controls.

## Bounded recording and publication

Use per-plot compact transition/event storage and tick indexes. Never sample
every probe on every tick or send a full 1,024-by-10,000 boolean matrix. Append
only normalized changes; preserve within-tick transitions. Tick completion must
be observed in the shared interpreter stepping dispatcher, including nano/pico
steps and quiet ticks. If an interpreter idle shortcut skips tick advancement,
do not synthesize time that the simulation did not advance.

Evict whole old ticks beyond the requested window. Before eviction advance a
baseline vector to values immediately before the new first retained tick. Keep
that vector even for quiet probes, so their old transition may be evicted safely.
Events evict with their tick; they do not consume signal slots. Record all
ordered mutations in a partial tick and mark its completion separately. Restore,
load or backend discontinuities reset identities rather than fabricate continuity.

Tick quotas alone do not bound memory: one tick or paused between-tick edits can
produce many records. Proposed configurable initial safety budgets, to validate
with measurements rather than treat as performance claims:

| Resource | Initial default |
| --- | ---: |
| Recorder bytes per active plot | 32 MiB |
| Recorder bytes globally | 256 MiB |
| Combined transition/text records per plot | 500,000 |
| Text events per plot | 10,000 |
| Retained UTF-8 event text per plot | 4 MiB, inside plot byte budget |
| Text per event | 64 KiB UTF-8 |
| API serialized page | 512 KiB before compression |
| API records per page | 4,096 default; 8,192 maximum |
| API concurrent work | 16 requests, 16 MiB global copied/serialized work |
| Plot request mailbox | 8 requests, served with a per-update work budget |

Count allocated capacities, indexes, strings and baselines, not just logical
payload sizes. Reserve capacity before activating, bounded by the global budget;
release it on off/reset confirm/unload. There is no per-browser recorder copy. Overlarge
text is truncated on a UTF-8 boundary with explicit original-byte count in the
event; never silently report it as complete. Other quota exhaustion stops the
**whole capture** with `overflow`, preserving an ordered valid prefix. Do not
drop arbitrary edges/events or block simulation on a consumer. Reserve a final
stop marker/status allowance before accepting ordinary records. A partial final
tick remains marked partial. Existing Minecraft output still succeeds if the
recorder stops; waveform limits must not turn a successful command into failure.

One global HTTP worker routes bounded requests to owning plot threads using
generation-tagged registrations, following the existing neighbor-source pattern
but without its disk fallback. No references to mutable worlds cross threads.
Plot threads copy bounded pages at a consistent observation boundary; JSON,
compression and network writes happen outside the simulation callback. Process
bounded request work so anonymous readers cannot starve simulation. Requests
expire after two seconds; expired queued work is discarded before allocation.
A full mailbox/work pool returns retryable 503. Bound network write duration and
output bytes, including responses to slow readers. Never pin an entire capture
for a slow or abandoned pagination session.

## HTTP v1

All endpoints are public read-only GET under
`/api/v1/plots/{x}/{y}/waveform`. Validate canonical signed i32 plot indices and
all query sizes/ranges before dispatch; reject unknown query parameters. No
global plot listing or multi-plot batch endpoint is needed for this release.

| Endpoint | Purpose |
| --- | --- |
| `GET /api/v1/plots/{x}/{y}/waveform` | Current status, source/capture identities, signal schema, limits and window; no trace body. |
| `GET /api/v1/plots/{x}/{y}/waveform/data?limit=4096` | Begin consistent snapshot: baseline, signal schema and first records page. |
| `GET /api/v1/plots/{x}/{y}/waveform/data?cursor=<opaque>&limit=4096` | Continue fixed snapshot pagination or read incremental records after its end. |
| `GET /api/v1/plots/{x}/{y}/waveform/stream?cursor=<opaque>&limit=4096` | Stream batches after a fully assembled snapshot. |

For example, a snapshot request for plot `(3, -2)` is
`GET /api/v1/plots/3/-2/waveform/data?limit=4096`.
There are no API mutations. VCD download remains client-side using a successfully
assembled capture; a second server-side export path is unnecessary initially.
Text events are not VCD signals; keep event previews in the viewer rather than
misrepresent chat as a binary wire.

All metadata and data pages include `owner`: the plot owner's cached display
name, or `null` for an unclaimed plot. Unavailable names use `"Unknown owner"`.
Owner identity is cached per loaded source and refreshed when ownership changes.
The API does not expose an owner UUID.

Metadata/status includes
`recording|disabled|stopped|overflow|unsupported_backend|empty`,
stop reason, requested/admitted limits, used bytes/record counts, retained first
tick, observed/completed tick and partial-tick status. Plot-loaded and
recorder-active are separate facts: loaded with no recorder returns 200 empty,
never synthetic demo data. `disabled` may retain a watchlist but no trace;
`empty` has neither. Their data response has `ticks: []`, no snapshot/cursor and
no active capture identity. A previous active cursor first receives 409, then
the fresh read returns the empty/disabled state. `stopped` preserves a prefix
after configuration enforcement, with reason `limits_changed`. Backend
`unsupported_backend` is explicit.

Changed blocks and comparator-entity writes resample watched positions within
two Manhattan steps, covering power through an adjacent solid block. The existing
position index filters candidates before reading power. No full watchlist scan
is performed each tick; recording-off writes return immediately.

### Compact page representation

This illustrative response has one signal transition, one chat emission and
another transition in tick 100. It is a snapshot page with no further historical
pages pending; subsequent reads use its returned live cursor.

```json
{
  "source": "s-example-7",
  "capture": "c-example-2",
  "state": "recording",
  "window": {
    "first_tick": "80",
    "observed_tick": "100",
    "completed_tick": "99",
    "partial_tick": true
  },
  "snapshot": {
    "signals": [["clock", [770, 64, -510]], ["ready", [771, 64, -510]]],
    "baseline": [0, 1],
    "baseline_tick": "80",
    "baseline_seq": "400",
    "through_seq": "403"
  },
  "baseline_seq": "400",
  "seq_base": "400",
  "ticks": [["100", [[1, 0, 0, 1], [2, 1, 775, 64, -509, "[@] ready", 0], [3, 0, 1, 0]]]],
  "more": false,
  "cursor": "opaque-continuation"
}
```

- `signals` is an array indexed by signal ID; each entry is `[name, blockPos]`.
  Send schema and baseline only on the first snapshot page, not each poll.
- `baseline` values are `0`, `1`, `null` for Off, On, unavailable, immediately
  before `baseline_tick`; `baseline_seq` is the last evicted record or zero.
- Each tick group is `[absoluteTickString, records]`. A tick may span pages.
  Preserve append order rather than sorting records after capture.
- `seq_base` is an exact decimal sequence origin for this page. Each record's
  first field is a safe numeric offset; absolute sequence is base + offset.
- Signal record: `[seqOffset, 0, signalId, value]`.
- Text event: `[seqOffset, 1, blockX, blockY, blockZ, text, originalBytes]`.
  `originalBytes=0` means untruncated; a positive value means original UTF-8 byte
  length before clipping. This small source position cost avoids maintaining a
  second dynamic dictionary while enabling source rows.
- `more=true` means historical pages through the fixed `through_seq` remain.
  `more=false` changes the returned cursor to tail mode. An empty tail response
  still updates status/window/tick completion and current `baseline_seq`; it is
  not evidence time stopped. `baseline_seq` is the server's last evicted record
  sequence, supplied as a top-level field on every data page. The value inside
  `snapshot` describes its fixed initial baseline; the top-level value describes
  the current server retention boundary for tail-gap detection.

Send only meaningful records, not a tick group for every quiet tick. Bounds and
completed-tick metadata carry quiet time. `seq_base` offsets are bounded by the
configured retained-record limit; if numeric safety would be exceeded use a new
base/page, never round. Record kind codes are fixed for v1; unknown codes require
a clear incompatible-version error rather than silently skipping observations.

### Cursor, retention and reset rules

Cursors are opaque bounded strings encoding plot/source/capture, last consumed
sequence and, during snapshot pagination, reconstruction anchor tick, fixed
high-water sequence and observation metadata. They are not credentials. Validate
their complete contents and membership/ranges server-side; malformed or forged
values cannot request unbounded scans/allocations. A cursor for another plot is
invalid. No unbounded server-side cursor/session registry is required.

The initial page atomically chooses baseline/schema and a high-water sequence.
Later snapshot pages stop at that high-water, even while capture continues.
They do not pin old data; if required baseline/history is evicted before the
client finishes, return 409 and start again. Once tailing, the client already
owns its baseline and consumed records: advancing the window alone must **not**
force another full snapshot. Each successful tail response supplies the current
`first_tick` and `baseline_seq`. Before trimming locally retained records older
than `first_tick`, apply their signal changes to the client's baseline; discard
their text events. Quiet rollover leaves values unchanged and needs no new
baseline vector. Resnapshot only if `last_consumed_seq < baseline_seq`, meaning
an actual unconsumed record was evicted, or source/capture changed. Equality is
safe: the reader already consumed the last evicted record. This also detects
lost text events rather than quietly hiding event-history gaps. If the capture
has no records, a moving tick window remains an empty cheap tail response.

Only after applying an entire page may the client replace its cursor. Deduplicate
an accidentally repeated page by source/capture/absolute seq. Keep at most one
in-flight request per selected plot. Cancel and reject late responses after a
plot switch. A source/capture change, rewind or expired baseline requires a fresh
snapshot; clear incompatible trace rows rather than join discontinuities.

| HTTP status | Stable error code / client action |
| --- | --- |
| 400 | `invalid_coordinates`, `invalid_limit`, `invalid_cursor`; correct input. |
| 404 | `plot_not_loaded`; show unavailable and retry manually/back off. |
| 409 | `source_changed`, `capture_changed`, `history_expired`; resnapshot. |
| 429 | `rate_limited`; obey `Retry-After`. |
| 503 | `busy` or `source_timeout`; keep frozen data labeled stale and retry with backoff. |

Errors are bounded objects such as
`{"error":"history_expired","resnapshot":true}`. An unloaded route is 404 even
with an old cursor; if it has already reloaded, that cursor receives 409 for the
new generation. No 401/403 reader flow exists. Check current source liveness
before serving a cached response so unloading does not leave a live public route.

## Efficient public reads

First drain the fixed HTTP snapshot with one request in flight. Open an
EventSource on `/stream?cursor=...` only after `more=false`. Each `batch` event
contains the same compact data page as REST and its `id` is the resulting cursor.
Apply the whole page before saving that cursor. The stream reads at 250 ms
intervals; a quiet stream emits a comment heartbeat every 15 seconds. Window,
completion and state changes are sent even without edges. Catch-up remains
bounded by the requested page limit, without unbounded bursts.

`Last-Event-ID` takes precedence over the query cursor on automatic reconnect.
After an application-level failure, close the EventSource and explicitly reopen
with the last **applied** cursor; receipt alone does not mean successful apply.
A `reset` event contains a bounded error object and closes the stream. Resnapshot
on changed/expired capture, back off on overload, and show unloaded plots clearly.
Errors before stream headers use normal HTTP status codes. Close streams on plot
switch/unmount; ignore late callbacks. High-TPS plots may outrun retention; show
the gap rather than silently joining histories. No SSE connection pins a plot.

Hyper runs on one named OS thread with a current-thread Tokio runtime. Bounded
plot mailboxes provide pages; JSON encoding, compression and socket I/O stay on
that API thread. There are at most 64 HTTP/1 connections and 64 active requests/
streams. Page responses are capped at 512 KiB, backed by a shared work budget.
Plot reads and blocked SSE writes time out after two seconds. Gzip streams flush
after each frame; `X-Accel-Buffering: no` prevents nginx buffering.

Support ordinary HTTP gzip at the API or reverse proxy. Compact transition tuples
and schema-once pages address volume first; benchmark before adding binary
encoding, delta-bit packing or shared per-cursor response caches.
Send `Cache-Control: no-store` to browsers/proxies for live endpoints; no
long-lived shared CDN cache or conditional-304 protocol is needed initially.
Static hashed JS/CSS assets can retain normal long-lived caching.

Configure public request/byte/concurrency limits at the reverse proxy and bound
work again in the Rust service. Recommended proxy per-client ceiling: 16 requests/s
with burst 32, covering snapshots and catch-up; byte and global limits remain
independent. Trust forwarded IP only from the configured proxy, and account for
shared-NAT readers when tuning. Rate control is resource protection, not access
authorization. Serve frontend and API at one HTTPS origin, keep the Rust listener
private in Docker, and do not enable broad CORS just because data is public.

## Implementation reach and acceptance checks

The implementation must reach recorder/config structures, shared interpreter
tick boundaries and block-write hook, restore/discontinuity paths, command
dispatch/aliases/help/completion, permission grants/documentation, command-block
World trait and all callers/mocks/forwarders, source registration/unload, global
worker/deployment, and the existing viewer's adapter/types/event lane. Do not
implement a parallel recording path in network packets or the rendering flush.

Required focused checks:

1. Public unauthenticated read works without plot membership/player presence;
   an unloaded plot is never loaded by reads. Unload/reload invalidates old
   sources and queued work. Loaded idle/empty/unsupported are explicit states.
2. Canonical/alias/subcommand permissions and plot editing gates are independent.
   Test dedicated and standalone paths, wildcard/denial/expiry, missing grants,
   B admission at 16 versus 17 probes and 100 versus 101 ticks, other ranks at
   1024/10000 and one above, zero/overflow config, multi-member shared capture,
   reduced-permission next commands and no destruction on failed admission.
   Explicitly test Builder inheriting default's 1024/10000 positive grants:
   central profile ceilings must still reject probe 17 and tick 101; a known
   non-Builder primary rank retains its configured 1024/10000 ceiling. Unknown
   external primary-rank mappings fail closed for new allocation.
3. Quiet baseline survives eviction. Same-tick On/Off/On and text ordering agree
   under normal/nano/pico stepping; partial completion is explicit. Ticks above
   2^53 remain exact. Enable/active probe changes/clear/restore rotate capture
   identity; off releases capture state but retains watches; reset confirm removes both.
   Bare reset changes nothing, including pending bulk selections.
4. `say` and nested `tellraw` produce plain evaluated text at execution tick,
   once despite many/no recipients; player chat does not. Check targeted output,
   conditional/chain/repeating blocks, failures, output limits, `track_output`,
   UTF-8 clipping metadata, missing dust and text-only zero-probe recording.
5. Snapshot pagination reconstructs the same state as a single recorder view;
   live writes do not change fixed high-water. Repeated steady-state tail polls
   across quiet and active window rollover return only bounded deltas/status,
   without full resnapshots when all evicted records were already consumed.
   Lost unconsumed records, snapshot eviction,
   reset, forged/cross-plot cursors and duplicate/late pages cannot silently
   combine histories or create unbounded work. Page byte/count ceilings hold.
6. Full queues, slow clients, many public readers and record/event/global memory
   exhaustion never block simulation or silently drop accepted records. Frozen
   overflow data remains a valid prefix; existing chat success is preserved.
7. Viewer handles one plot, dots/preview/source expansion, all event messages in
   order, plain-text rendering, stale/error states, configured limits and VCD
   50 ms/tick without claiming chat events are one-bit signals.

Measure disabled hooks, 64 and 1024 probes, quiet and edge-heavy captures,
command-block event bursts, full-window rollover and concurrent readers. Record
actual interpreter overhead, recording bytes and serialized bandwidth before
adjusting defaults. Do not promise a throughput target without these measurements.

Implementation deliberately excludes capture persistence, the 3D map,
compiled capture, browser editing, custom binary transport and formatted events.
Public targeted command-block text and quota-based early overflow are the
material behavior operators and users need to understand.
