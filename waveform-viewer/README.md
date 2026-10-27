# MROWW Waveform Viewer

Standalone React + TypeScript viewer based on the supplied VaporView screenshots.
It reads binary Minecraft captures and command-block output from the public
waveform API.

Grouped values: [bus viewer design](../scraps/docs/WAVEFORM_BUS_VIEWER_SPEC.md).

Requires Node 22.22 or later. From this directory:

```powershell
npm ci
npm run dev
```

Open [http://localhost:8443](http://localhost:8443). `npm run build` type-checks
and produces static assets in `dist/`. `npm test` runs the trace, scrolling and
session and API record checks using Node's test runner.

Enter plot XY as `0 0` or `0, 0` and click **Load**. The plot must be loaded in Minecraft. You can
also open `http://localhost:8443/?x=3&y=-2`. Y is the plot index corresponding to
Minecraft Z, not block height. The viewer always uses the live API.

Select signals and press G to group them, then right-click the group and enable
**Show as Bus**. **Format Values** offers Binary, Octal, Decimal, Decimal U2 (signed two's complement) and Hexadecimal,
plus inverted-logic versions. The first child is bit 0. Collapse the group to
show only the combined waveform; hiding a child does not remove its bit.
**Remove from Group** removes that bit from the bus without removing the watch.
Formats, bit order and layout are saved per plot in the browser and in viewer
session files. Missing probes keep their bit slots as X. VCD exports physical
signals and command output; WaveDrom uses visible rows and selected bus formats.

The bottom-left **Zero tick** field sets the absolute tick displayed as zero.
Apply with Enter or by leaving the field. Ruler, cursors, event previews and
go-to-tick use this reference. Empty the field or use **Reset** to return to zero;
earlier ticks display as negative. The reference
is saved per plot and in viewer sessions. VCD timing and recordings stay physical.

In Minecraft, `/waveform clear` empties captured history/events and keeps watches
and recording. `/waveform reset` only warns; `/waveform reset confirm` removes
all watches and history and stops recording.
`/waveform on` defaults to 1,000 ticks, capped by your allowance; pass an explicit
tick count to select a different permitted window.

The Vite development server proxies `/api` to `http://127.0.0.1:8091`. Set
`WAVEFORM_API_URL` before starting Vite if the Rust server uses another address.
Production should serve the static build separately and reverse proxy `/api` to
the Rust service on the same origin. Disable proxy buffering on the SSE route;
the backend sends batches every 250 ms. No cross-origin permissions are needed.

The included Dockerfile builds a separate Nginx frontend with this proxy already
configured. Build from this directory with `docker build -t mchprs-waveform .`.
Run it on the Minecraft container's Docker network, publishing port 8080 and setting
`WAVEFORM_BACKEND` to the backend container and port (default `mchprs:8091`). The Rust
service must enable waveform serving and bind to `0.0.0.0:8091` inside its container.
Keep that API port private to the Docker network; expose the frontend through your
usual HTTPS proxy. Container creation and the production Compose file are separate
from this frontend build.

Static assets use relative URLs, so a reverse proxy may also mount the viewer at
`/waveform/` by stripping that prefix. Redirect `/waveform` to `/waveform/` to keep
asset resolution correct. The public API remains at the origin's absolute
`/api/v1/plots/` path; route that path to this container or directly to Rust.

## Code map

| File                               | Responsibility                                                                           |
| ---------------------------------- | ---------------------------------------------------------------------------------------- |
| `src/trace.ts`                     | Binary sampling, tick ranges, pointer-anchored zoom and edge navigation.                 |
| `src/bus.ts`                       | Exact packed bus values, formats, membership and ordered transitions.                    |
| `src/components/drawBus.ts`        | Clipped shaded bus segments and same-tick change indicators.                             |
| `src/scroll.ts`                    | Mouse/trackpad classification and scrolling mappings, independent of React.              |
| `src/state.ts`                     | Display rows, grouping, ordering and validated session loading.                          |
| `src/layout.ts`                    | Safe per-plot browser preferences using stable probe names and positions.                |
| `src/demo.ts`                      | Synthetic test fixtures; excluded from the production app.                                         |
| `src/api.ts`                       | Paginated snapshot loading, SSE, exact u64 decoding, deduplication and window retention. |
| `src/App.tsx`                      | Viewer state, commands and menu actions.                                                 |
| `src/components/Netlist.tsx`       | Single netlist with scopes and visibility checkboxes.                                    |
| `src/components/ResizeHandle.tsx`  | Pointer and keyboard resizing for both column dividers.                                  |
| `src/components/Toolbar.tsx`       | Navigation controls and scroll-mode selection.                                           |
| `src/components/Waveform.tsx`      | Canvas ruler/traces, high-level gradients, cursors and pointer input.                    |
| `src/components/CommandEvents.tsx` | Event dots, plain-text previews and a shared expandable output timeline.                 |
| `src/components/ContextMenu.tsx`   | Row/blank-area menus, submenus and keyboard dismissal.                                   |
| `src/vcd.ts`                       | Standard waveform-file export at 50 milliseconds per tick.                               |
| `src/style.css`                    | Reference colors and layout.                                                             |

React owns viewer state and DOM controls; the canvas draws the dense waveform area.
Capture data stays separate from display rows, so hiding/removing a displayed row
does not remove a watched Minecraft probe. This prototype does not mutate a server.

## Interactions

Mouse mode uses the wheel to pan time. Trackpad mode sends horizontal motion to
time and vertical motion to rows. Auto mode follows VaporView's device heuristic;
select a mode explicitly if a device is misclassified. Shift-wheel and wheel over
names/values always scroll rows. Ctrl/Command-wheel or pinch zooms under the pointer.
Native device momentum is preserved without an additional inertia animation.

The topbar contains plot X/Y inputs, Load, the owner, and Export VCD.
The selected plot's owner appears next to Load; unclaimed plots are labeled
explicitly. When no capture is available, the waveform area shows a short
Minecraft setup guide and distinguishes recording being off, unloaded plots,
connection failures and a recorder waiting for command output. Quiet wires and
event-only captures stay visible, including during reconnects. A replacement
snapshot only replaces displayed data after all its pages arrive successfully.
Loading coordinates replaces the current plot; failed loads clear the old capture.
The two draggable
dividers resize the netlist and waveform label columns; focused dividers also
support left/right keyboard resizing. The bottom bar shows only delta and A/B.

Click places cursor A; Alt-click or middle-click places B. Drag waveforms to pan;
drag signal labels to reorder. Checkboxes enable/disable rows. Plain left/right
buttons and keyboard keys move cursor A by exactly one game tick; separate edge
buttons navigate transitions. Search has no radix or time-mode decoration.
Right-click a row or empty area for the corresponding menu. Use the `?` button for
keyboard controls. Session JSON stores this viewer's layout; it is not an upstream
VaporView session format. WaveDrom export samples at most 32 settled ticks.

The browser automatically remembers each plot's presentation in localStorage,
signal order and visibility, groups and separators,
colors and row heights, edge guides and names, both column widths, collapsed
netlist scopes, command-output expansion, snapping and scrolling mode. Probe
names and block positions identify saved rows, so capture resets and server
reloads preserve the layout even when numeric probe IDs change. New probes append
to the saved layout; deleted probes disappear. Waveform samples, command output,
time cursors and capture IDs are never stored. Blocked, full or malformed browser
storage falls back safely to the default layout. Clearing browser site data
removes these preferences.

The default theme uses red traces and interface accents on a dark background.
High segments have a translucent gradient fading toward the low level, using the
row's selected color. Diamonds mark multiple events in one tick, without assigning
them invented sub-tick durations.

Register a block or component with `/waveform add clock` while looking at it
(or provide absolute block X/Y/Z). Dust/comparator output is On above zero;
repeaters, levers and buttons report their powered state. Ordinary blocks report
incoming redstone power; missing blocks show X.

For a multi-bit signal, use `/waveform addbulk data 8`, then **left-click low to
high**. This creates `data_0` through `data_7` together after the last click.
Clicks select without breaking/toggling the blocks; `/waveform cancel` abandons
the pending batch. Every bit counts toward the normal probe limit. An active
capture starts fresh once the entire batch is committed.

Command-block output appears as dots in a fixed lane below the signals. Hover or
focus a dot for an immediate plain-text tooltip; click to preview all output at
that tick and move cursor A. Expand **Command output** to see all messages on one
shared timeline row, with dots retained above it. Labels remain visible when
their start scrolls off-screen, until the next output replaces them. Newlines are
preserved in tooltips and previews; text renders literally without HTML or
Minecraft colors. Multiple outputs at one tick remain ordered. Command events
are separate from binary VCD signals.

Live mode first downloads all snapshot pages, then subscribes to the completed
cursor through SSE. Absolute u64 ticks and sequence IDs stay as strings/BigInt;
only offsets within the retained window enter the canvas. Duplicate records are
ignored, sequence gaps trigger a fresh snapshot, and expired records are pruned
with their last signal value retained as the new baseline. Disconnects, reset
events and capture changes reload the snapshot after one second. Changing plots
or modes aborts pending requests and closes the old stream. Live view follows the
latest tick while positioned at the end; panning away keeps the inspected range.

Export VCD downloads all captured probes, including hidden rows, with a `1 ms`
timescale. Each tick advances the file by 50 ms (20 ticks/second). Command-block
output appears in a `command_output` string signal, with block coordinates and
all messages from the same tick combined in order. The string signal uses a VCD
extension supported by VaporView/Wellen and GTKWave; some VCD readers may not
support it. Session JSON remains a separate presentation-only export.

## Limits

The API is public and read-only; capture commands and permissions belong to the
Minecraft server. Production Docker hosting remains separate. Bus decoding and
the upstream extension's complete feature set are outside this binary viewer.
The expanded event list covers the visible time range rather than the entire
retained buffer. Mouse/trackpad hardware feel still needs
human review even though the scroll mappings have automated checks.

See the [viewer specification](../scraps/docs/VAPORVIEW_VIEWER_SPEC.md),
[capture design](../scraps/docs/WAVEFORM_ANALYZER_DESIGN.md) and
[implementation plan](../scraps/docs/WAVEFORM_ANALYZER_PLAN.md).
The [API specification](../scraps/docs/WAVEFORM_API_SPEC.md) defines public
loaded-plot reads, X/Y plot naming and command-block event dots and a shared expanded text timeline.

Material SVG icons come from [Google Material Design Icons](https://github.com/google/material-design-icons). Attribution and Apache-2.0 license are included in `public/` and copied into production builds.
