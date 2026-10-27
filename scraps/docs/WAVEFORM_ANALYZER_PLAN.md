# Waveform analyzer implementation plan

Implement the [design](WAVEFORM_ANALYZER_DESIGN.md) in this order. The standalone
[Vite/React/TypeScript viewer](../../waveform-viewer/README.md) supports the
public API and explicit mock data. The recorder and Hyper HTTP/SSE service are
implemented; the gates below remain the review and rollout checklist. Its detailed UI contract is the [viewer specification](VAPORVIEW_VIEWER_SPEC.md).
The [API specification](WAVEFORM_API_SPEC.md) owns public plot reads, compact
records, command-block output events, permissions and rank-based recording limits.
The former Docker/code-server prototype was stopped and deleted.

## 1. Review the standalone UI prototype

Use React DOM for the single netlist, plot X/Y topbar and context menus, and
canvas for dense traces. Keep `trace.ts` math and `state.ts` state understandable
and testable. Complete local reorder, enable/disable, markers, tick delta,
fit/zoom/alignment, groups, separators and the screenshot menu interactions.
Actions must work or be visibly disabled; no synthetic data may be called live.

Recreate the verified scrolling contract: mouse wheel pans time; Shift scrolls
rows; touchpad uses X for time and Y for rows; Ctrl/pinch zooms around the pointer.
Nonzero X routes both axes even in Mouse mode; Shift has priority over Ctrl;
wheel over labels/values scrolls rows. Offer Mouse/Touchpad/Auto overrides and
retain the source's 100 ms detection heuristic without invented momentum.

Gate: focused math/state/input tests and build pass; compare browser screenshots
at reference sizes, review both context menus, keyboard access and physical mouse
and touchpad behavior. Do not claim pixel-exact conformance without comparison.
The prototype's numeric tick range is a known limit; API integration must parse
u64 decimal strings as BigInt and convert only safe relative offsets to canvas
numbers. No extension installation, WCP or VCD live adapter is required.

## 2. Add a bounded recorder

Add `crates/core/src/plot/waveform.rs` and focused tests. Keep its state in
`PlotWorld`; make serialization operate on owned snapshots outside the hot path.
Implement immutable per-capture probe schemas, binary normalization, initial
values, ordered events, tick indexes, baseline advancement, reset/stop reasons
and byte/event admission. Copy the existing history budget pattern where useful;
do not expose history's private codec as a new shared abstraction.

Add bounded plain-text command-block events sharing signal tick/sequence order.
Capture only successful command-block output, once at accepted enqueue, using
source block positions and the existing accepted text component structure. Update
the world hook and every caller/mock listed in the API spec. Capture event-only
sessions with zero dust probes when enabled; text recording never blocks or
changes Minecraft chat delivery.

Add shared limits under a small `crates/core/src/waveform/` module only for API
ownership, request/response types and recorder/work reservations. Register it in
`lib.rs`. Use existing serde, random generation and channel dependencies; review
the additional HTTP server dependency in step 5.

Gate: small unit tests cover 0/nonzero normalization, nonzero-to-nonzero silence,
duplicate rejection, missing/replaced dust, On/Off/On ordering, tick eviction and
baseline correctness, empty/partial ticks, overflow while paused, allocation
failure and release of global reservations. Admission failure preserves old data.

## 3. Connect all interpreter paths

Observe changed watched positions in `PlotWorld::set_block_raw`. Put tick hooks
in shared `prepare_operation` transitions, not packet flushes or only the game-tick
wrapper. Keep watched lookup behind an empty-recorder check. Audit direct chunk
writes and bulk replacements so they either use the shared hook or deliberately
begin a new capture.

Update successful history rewind and Git checkout/restore application. Update
interpreter idle detection, empty-plot handling, unload, compiler command admission
and automatic compilation eligibility. Treat pending compilation as incompatible
with starting a recording. Preserve existing history and stepping restrictions.

Gate: interpreter tests compare exact blocks, queues, piston state and ticks with
recording disabled/enabled. Equivalent game/nano/pico stepping produces the same
trace. Include immediate interactions, a nested same-tick pulse, WorldEdit,
rewind success/failure, Git restore, quiet plots, compiler guards and unload.

## 4. Add commands and configuration

Update `plot/commands.rs`: dispatch, completion nodes, `native_command_permission`,
`changes_plot` policy and help. Put authorization in the shared command handler
so every action, including status/list/web, requires plot edit access. Use existing
ray tracing and coordinate validation; reuse membership and permission resolution.
Add user-facing text through the existing messages module.

Update `config.rs`, deployment examples and `docs/MCHPRS_PERMISSIONS.md`. Validate
the tick, signal, byte and event ceilings before allocation; enabling a listener
requires valid bind/public URL settings. Keep defaults disabled. Save probe names,
order and positions in `world/plots.db` using the existing plot metadata pattern;
recording restarts off. Captures/events are never persisted. No binary plot-save
migration is needed. Existing command-block fixtures and
trait mocks must follow any source-position signature change.

Use `/waveform on [ticks]`, `/waveform off` and `/waveform [status]` like history.
Every action has its own permission plus the existing command/edit gates. Off
discards captured data and releases its budget while keeping probe definitions;
clear empties history in place and keeps definitions/recording; reset only warns
until `/waveform reset confirm` removes everything. Enforce configurable Builder (B) caps of 16 signals
and 100 ticks; every other rank gets caps of 1,024 signals and 10,000 ticks.
Use the API spec's shared-plot and numeric permission rules, not browser identity.

Gate: command tests cover owner/member/admin/visitor, dedicated permissions and
standalone behavior, unowned plots, negative/out-of-plot positions, malformed names,
duplicates, all subcommands and autocomplete. Configuration tests reject overflow
and invalid bounds. A successful schema change announces trace replacement.

## 5. Add the one global API worker

Add startup/shutdown wiring in `server.rs` and generation-tagged registration in
plot lifecycle, following `neighbors.rs`. Keep request/reply mailboxes bounded.
Use Hyper with a current-thread Tokio runtime on one named OS worker. Reads are public and require no UUID, credential or membership. Route only
to existing loaded sources; the HTTP handler does not access live world storage,
load plots or extend their lifetime.

Implement the API spec's compact baseline and incremental records, shared signal/
text ordering, capture-bound cursors, bounded work reservations, timeouts and public
read rate control. Public x/y plot indices map to backend x/z. Keep cold metadata
out of each edge record; use standard JSON/compression before considering binary
protocols. Make API startup failure explicit and join the worker during shutdown.
VCD download remains separate from live event integration.

Gate: unauthenticated HTTP reads succeed for loaded plots, unloaded plots remain
unloaded, and browser reads do not pin a plot. Cover stale generations, evicted
cursors, resets, u64 identities, compact record decoding, bounded pagination,
partial ticks, text-event limits, overloaded mailboxes, slow clients and concurrent
unload. One slow request must not hold the worker or block a plot. Command tests
separately enforce denied actions, numeric caps and permission changes.

## 6. Connect the data adapter and production Docker routing

Replace the fixture with public metadata/baseline/delta reads. Keep exactly one
selected x/y plot, abort older requests when it changes, and never combine plots
or generations. The plot must be loaded; show an explicit unloaded state. Parse
absolute tick/sequence identities as BigInt and render safe relative offsets.
Follow the API spec's HTTP snapshot, 250 ms SSE batching and reconnect contract while
preserving stable local selection/viewport. Add an event lane with dots at output
ticks, plain-text preview and expansion into one shared output timeline row. Use literal text, not
HTML or styling, and keep one output represented once despite recipient fanout.

Deploy static frontend assets with production Docker/ingress routing at the same
HTTPS origin as the internally routed Rust API. Both app assets and loaded-plot
capture reads are public; only Minecraft commands require permissions. There is
no account token, per-user IDE container, terminal, extension host or WCP port.
Bound public traffic without using the recording budget as a response cache.

Gate: API fixtures and real HTTP integration verify initial values, binary/X,
same-tick pulses, partial ticks, resets, retention gaps and u64 values above
Number.MAX_SAFE_INTEGER. Test two anonymous browsers selecting different plots,
rapid plot switches, command-block text dots/previews/shared expanded timeline, no formatting
execution, and disconnected/unloaded/expired states.
Browser review must prove that displayed traces originate from interpreter data
before claiming live integration. Required offline VCD download has its own checks;
it is not a transport dependency.

## 7. Measure and qualify production behavior

Measure disabled hooks, 1/16/64/1,024 probes, up to 10,000 retained ticks, quiet and
clock-heavy plots, text bursts, rollover, concurrent plots and public API traffic.
Use existing interpreter
benchmark/fixture support and record baseline and changed timings, peak recorder
and work memory, and observed overflow. Agree an acceptable overhead from those
measurements rather than promising an unmeasured percentage.

Gate: required Rust checks and focused tests pass, the deployed browser receives
real interpreter traces, recorder/work budgets stay bounded under load, plot saves
remain compatible, and disabling waveform returns the original execution path.

## Later map phase

Scope the Three.js map after waveform capture ships. Reuse the single public plot
selector and probe coordinates, then design bounded section snapshots, chunk
versioning, bounded mesh updates and block assets. Decide live versus retained
world views explicitly: waveform traces alone cannot reconstruct a historical
3D world. Compiled capture and scripting each need separate designs and checks.
