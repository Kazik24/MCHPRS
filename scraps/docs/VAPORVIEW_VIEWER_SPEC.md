# VaporView-style standalone waveform viewer specification

Status: implementation specification, 27 October 2026. This describes a standalone HTML viewer for MCHPRS captures. It replaces the browser VS Code packaging direction; the visual reference is VaporView's viewer content, not the surrounding editor application. Recording and authorization remain defined by [the analyzer design](WAVEFORM_ANALYZER_DESIGN.md). This document changes no capture or server code.

## Current approved composition and interactions

Backend integration follows the [API specification](WAVEFORM_API_SPEC.md): one
already-loaded plot at a time, public x/y plot indices (y maps to backend z),
public read access, compact incremental signal changes and command-block text
events. The plot selector must use X/Y when the adapter ships. Add an Events lane
with tick-positioned dots and plain-text preview, expandable to source/output
rows; each message keeps its shared signal/event sequence order. These are planned
integration requirements, not capabilities of the current synthetic demo.

This section overrides historical screenshot composition and earlier proposals
below. The application has a compact topbar with editable plot X/Z inputs, Load,
Export VCD, plain search and compact waveform navigation controls; one netlist
at left; and the waveform at right. Remove application titles such as Waveform
Analyzer/Interpreter, desktop menus, filename tabs/dots/breadcrumbs, VS Code
activity bar and the entire Displayed Signals panel. Search has no `bin` or `t=`
adornment. These removed regions must not return for screenshot similarity.

The bottom bar stays, with only delta, A and B aligned at the right. It contains
no notices, selected-signal text or left content. Use focused tooltips or another
appropriate interaction for explanations rather than filling this bar.

Keep accessible draggable splitters for netlist width and waveform label/value
column width, with keyboard resizing, visible focus and useful minimum sizes.
Reorder through waveform labels and toggle visibility through netlist/selected-
row controls. Preserve the source-verified scrolling contract below. Plot X/Z
are plot indices; fixture loading must not imply an existing live server.

Plain Left/Right keyboard commands and plain toolbar arrow buttons move the main
marker exactly one Minecraft game tick backward/forward, respecting retained
bounds. They never jump to the next event. Previous/next transition navigation
remains a distinct explicitly labeled control. Preserve same-tick event ordering
for inspection without changing the one-tick stepping contract.

Context-menu choices persist in viewer state and show their current checked or
selected state when reopened, including applicable color, name, row-height and
edge-guide options. Persistence here means the active in-memory viewer session;
it does not authorize browser storage of credentials or capture data.

The approved default theme is red: dark surfaces, red waveform traces, red
active controls, red menu/selection accents and a subtle red downward gradient
beneath high plateaus. Keep the step line crisp and low state dark. Historical
green screenshots are reference evidence, not the approved color target. Shine
is visual emphasis only and must not suggest an analog strength measurement.

Export VCD downloads a standard VCD file from the current capture without an
extension or backend dependency. Use `$timescale 1 ms $end`; at default 20 TPS,
one game tick maps to 50 ms (timestamp = relative tick × 50 in those units).
Emit same-tick transitions in sequence order at the same timestamp, not invented
sub-tick timestamps. Include initial values and valid declarations/identifiers;
map unavailable values to `x`. Explain the default 20 TPS conversion in a comment
and keep the viewer ruler in game ticks. This offline time-unit mapping does not
claim the server actually ran at 20 TPS wall-clock speed.

## Evidence and interpretation

All three supplied images were inspected:

- `../image.png` (1701 × 738): complete editor, netlist and displayed-signals sidebar, toolbar, value column, waveform rows, two markers and footer.
- `../image copy.png` (726 × 721): signal-row context menu.
- `../image copy 2.png` (1746 × 843): viewer toolbar, group row, separator and blank-area context menu.

Dimensions and colors below are visual estimates in screenshot pixels, not recovered upstream CSS. Use the first image for the overall split and the third for the unobstructed waveform content. The screenshots have different scaling and versions; preserve their relationships rather than forcing incompatible pixel measurements into one layout.

Upstream's [README](https://github.com/Lramseyer/vaporview#readme) confirms waveform navigation, two time markers, signal organization and searching. Its [API documentation](https://github.com/Lramseyer/vaporview/blob/main/API_DOCS.md) distinguishes source variables, displayed rows, groups and separators. Its [manifest](https://github.com/Lramseyer/vaporview/blob/main/package.json) defines viewer commands and settings. Those sources are behavioral references, not a requirement to run an extension host. The manifest inspected identifies version 1.6.0; the screenshots are not proven to show that exact release. Menu wording in this specification comes from the images; API documentation uses some different wording.

Everything marked **Proposed** is a concrete behavior for this standalone adaptation, not a claim about undocumented VaporView behavior. Unopened submenus and ambiguous toolbar glyphs must not be presented as recovered upstream facts.

## Historical visual composition and retained styling

Historical measurements below describe the supplied images; removed regions are evidence only, superseded by the current approved composition. Make the viewer occupy the browser viewport. No promotional header, dashboard cards, large margins, VS Code activity bar, command center, terminal, extension marketplace or browser editor is needed. The waveform should receive nearly all remaining width and height.

| Region | Reference and implementation target |
| --- | --- |
| Left sidebar | First image: x≈59–408, width≈349 px. Retained: standalone default 300–350 px, with accessible resize. Dark background; full height below a compact capture header. |
| Netlist panel | Upper sidebar, approximately 60% of its available height in image one. Uppercase `NETLIST` heading, disclosure chevron, expandable scope tree, checkboxes and type icons. |
| Displayed signals panel | Historical reference only: lower sidebar, `DISPLAYED SIGNALS` heading. Removed from the approved implementation. |
| Capture heading | Historical title/filename reference only. Removed; use a simple topbar with editable plot X/Z inputs. |
| Toolbar | One horizontal strip, approximately 48–66 px high. Controls align left with modest 12–18 px group gaps. A thin bottom border separates it from the ruler. |
| Ruler | Above waveforms only, approximately 45–52 px high including tick marks. It begins at the exact waveform origin, after name and value columns. |
| Name column | First image: x≈408–573, partially clipped long names. Third: x≈0–217. Default 220 px in standalone; draggable divider. Right-aligned monospaced names, ellipsis/clipping and full-name tooltip. |
| Value column | Approximately 72 px (third image x≈217–290). Right-aligned value at marker A. Thin vertical rules separate both columns. |
| Waveform canvas | Remaining width, no outside padding. Default row height 35–40 px, with signal high/low levels approximately 20 px apart. One row coordinate system spans label, value and waveform. |
| Footer | Retained compact bottom bar: only delta/A/B aligned right; no left content, notices or selected signal. |

**Historical screenshot color estimates** (superseded by the approved red theme): page/canvas `#1e1e1e`, sidebar `#181818`, toolbar `#202020`, rules `#383838`, primary text `#cccccc`, muted ruler `#858585`, names `#569cd6`, trace `#b5c9a5`, faint high-state fill `#b5c9a51a`, selected row `#094771`, selection outline `#007fd4`, checked control `#0e639c`, menu hover `#0078d4`. Check against the reference during implementation; these are starting values rather than sampled exact colors.

Use system UI/Segoe UI for panel headings and menus (13–15 px at normal scale); use a system monospace stack such as Consolas for names, values and ruler labels (14–16 px). Menus in the enlarged reference have larger apparent text. Keep strokes crisp, normally one CSS pixel, and account for device pixel ratio when rendering canvas. Thin muted vertical grid lines must remain subordinate to pale green traces. Avoid rounded cards, gradients, heavy shadows and oversized typography. Context menus alone have a subtle border, shadow and approximately 6 px corner radius.

## Signal model and the single netlist

The netlist lists every probe in the current capture, grouped by available scope, with stable probe identity and coordinates in tooltips. A scope can expand or collapse without changing recording or plotted rows. Binary signals use the small connected-node icon visible in the reference; bus brackets in the first image describe upstream buses, not redstone strengths.

The netlist is the only left-side tree. Its checkboxes toggle local visibility.
Checking a probe adds/reveals it; unchecking hides it while retaining its last
position and presentation settings. A selected waveform row also exposes an
accessible visibility action. Hiding never changes recording.

Dragging a waveform name reorders the shared presentation row list. Show a thin
insertion line and permit insertion before/after another row or into an expanded
group. Dragging from netlist adds/reveals a signal at the drop position. Keyboard
move controls provide the same operation. Preserve selection and markers while
rearranging. The initial implementation needs only one displayed instance of each
probe; no Displayed Signals panel or duplicate-row feature is required.

`Remove Variable` removes a displayed row and its viewer settings, without deleting the source probe or retained events; it can be added again from netlist. Hiding preserves row settings. Deleting a Minecraft watch changes the recorder schema and potentially resets capture; that remains an explicit game command, never a side effect of a checkbox, Delete key, menu removal or local session load. The browser is a read-only capture client.

## Binary traces, values and time

Each dust probe is `1`/On for power greater than zero and `0`/Off otherwise. Strength changes 15→7 do not produce another level. Missing/unavailable dust can render `X` with a distinct patterned trace and explanation; it is validity metadata, not a third measured strength. Never draw analog ramps, 0–15 axes, voltage plots or fabricated bus values.

Draw binary step traces with horizontal high/low plateaus and vertical transition edges. A quiet signal still draws across the retained range using its baseline. Use a subtle horizontal selection background across all three row sections. Cursor A's value appears in the value column; transition-hit details may show before→after, as visible in image one. Do not silently substitute the latest live value for the value at A.

The ruler's primary unit is **Minecraft game ticks**, labeled `0`, `10`, `20` etc. with an explicit `ticks` label; never `ns`, `ps` or a wall-clock duration. It is independent of server TPS and must not call game ticks redstone ticks. Major spacing follows a readable 1/2/5 progression; minor ticks do not imply sub-tick timing precision. Zooming far out aggregates drawing only, not stored event order.

The underlying timestamp remains `(capture_id, tick, seq, phase)`. Multiple events within one tick have order but no measured physical duration. **Proposed:** show a compact multiple-transition/glitch indicator at that tick; clicking it opens an ordered event list with sequence and phase. Coincident edges must not erase an On/Off/On pulse. Do not spread these changes evenly across a tick and label the resulting fractional positions as measured time. Marker details can select a particular sequence at the tick; the compact value then states the selected event boundary. At a plain tick boundary use the last completed state at or before that boundary and state this rule in help.

Cursor A is the main vertical dashed/dotted marker; cursor B is the alternate. Both extend through ruler and rows. Right-aligned bottom-bar details only: `Δ: +17 ticks`, `A: 120 ticks`, `B: 137 ticks`; no notices, selected signal or left content. The signed delta is B−A. If both are at the same tick but different sequences, show `Δ: 0 ticks` and the sequence difference separately, without claiming a fractional tick duration. A partial tick and a gap/expired range receive explicit labels. Changing captures clears stale marker event identities and prevents traces from different generations being joined.

## Toolbar and navigation

The images show magnifiers, edge/alignment glyphs, a search field, a `t=`/format area, previous/next arrows, marker-related toggles and (in image three) `Auto Reload`. Exact semantics of every unlabeled glyph cannot be recovered from the images. Retain their compact visual rhythm, with tooltips and accessible names; implement the following explicit **proposed** command set rather than guessing hidden labels:

| Control | Required behavior |
| --- | --- |
| Fit capture | Show the complete retained tick interval with a small edge margin. Empty/single-tick captures remain usable. |
| Zoom out / Zoom in | Scale around the pointer if over the graph, otherwise marker A when visible, otherwise viewport center. Never move the selected timestamp. |
| Jump start / Jump end | Align the left/right retained boundary to its respective viewport edge. |
| Previous / next transition | Navigate the selected signal relative to A using ordered `(tick, seq)` events, including same-tick events. Disable without a selected signal or available transition. |
| Align A left / center / right | Position A at the requested viewport location without changing A; edge clamping may prevent exact alignment and must not move the marker. |
| Search | Search signal names in the sidebar; keep the visible input plain, without bin or t= decorations. No radix selector is necessary. |
| Marker A / B controls | Choose which marker a subsequent click places, and provide numeric tick entry and clear-B. Keep the reference's compact toggle style. |
| Refresh / Auto Reload | Explicit consistent capture refresh first; opt-in follow polling only when the API exists. Keep viewport/selection for the same capture; fit a new capture once. Show unavailable state in a fixture-only prototype. |

**Proposed marker contract:** click graph places A, Shift+click places B; drag a marker moves it. Wheel/pinch routing must follow the source-verified scrolling contract below. Clicking and dragging a row label reorders rows, not time. Dragging empty graph space pans horizontally after a small movement threshold. Prevent browser page zoom only when the graph handles its own zoom gesture. The ruler stays visible while rows scroll; names, values and traces share vertical scrolling. Time scrolling moves only the waveforms/ruler, never labels. The single netlist scrolls independently. All three splitters have minimum sizes and keyboard support.

**Proposed keyboard contract:** Tab reaches controls; arrow keys navigate tree rows; Left/Right collapse/expand tree nodes; Space toggles the focused visibility checkbox; Enter selects a row; Delete invokes local `Remove Variable`; Ctrl+Up/Down moves a selected row; `+`/`-` zoom; plain Left/Right move A by exactly one game tick; Home/End jump retained bounds; Escape closes a menu/drag. `G` and `Ctrl+R` use the screenshot meanings below only when viewer focus is outside a text input; prevent browser reload for Ctrl+R only while that viewer command is active. Do not register global document shortcuts that break browser text editing.

## Exact context menus

Open the signal menu when right-clicking its name, value or waveform. Select the hit row before opening, preserving an existing multi-selection when the hit row belongs to it. Open the blank menu below the last row or on empty plot space. Position within viewport bounds; keyboard context-menu key/Shift+F10 opens the corresponding menu. Menu text, order, dividers, right-side shortcut labels and submenu chevrons below match the screenshots. Submenu contents were not shown and are not specified as screenshot facts.

### Signal menu (image copy.png)

| Visible text | Behavior for the standalone adaptation |
| --- | --- |
| `Edge Guides` › | Proposed submenu toggles vertical guides from the selected signal's transitions. Guides are a visual aid only. |
| `Color` › | Proposed compact palette changes the selected trace color and icon accent. |
| `Row Height` › | Proposed compact row-height choices; label/value/waveform heights always change together. |
| `Name` › | Proposed display-name choice/alias without changing stable probe ID or server watch name. |
| — divider — | |
| `Remove Variable` | Remove selected local displayed row(s); never delete a recording probe. |
| `Show in Netlist View` — `Ctrl+R` | Reveal, expand ancestors, select and scroll to the original netlist probe. |
| `Copy Variable Name` | Copy the full source name, not its clipped visible substring. |
| `Copy Value at Marker` | Copy selected row's value at A; disable if no defined marker value. |
| — divider — | |
| `New Group` | Create an empty local group near the selected row and edit its name. |
| `New Group from Selection` — `G` | Put selected displayed rows in a new group, preserving their order. |
| `New Separator` | Insert a non-signal separator near selection. |
| — divider — | |
| `Save Vaporview Session...` | Proposed local JSON download of viewer layout/settings, with no credential or trace payload. |
| `Load Vaporview Session...` | Proposed local file picker; validate bounded input, resolve known probe IDs, report missing probes and do not mutate the server. |
| — divider — | |
| `WaveDrom` › | Upstream export-related submenu; deferred in binary MVP. Keep the entry disabled with an explanatory tooltip in the visual replica. No unseen submenu labels are claimed. |

### Blank-area menu (image copy 2.png)

Exact sequence: `New Group`; `New Group from Selection` with right-aligned `G`; `New Separator`; divider; `Save Vaporview Session...`; `Load Vaporview Session...`; divider; `WaveDrom` ›. Apply the same behaviors as above; group-from-selection is disabled without a selection. `New Group` is blue when hovered/focused as in the image. Do not insert row-only actions, browser-native menu items, or invented screenshot entries here.

For staged delivery, an action must work or be visibly disabled with a specific reason; a menu that closes without performing the named action fails acceptance. The exact upstream session file format is not established here. The preserved screenshot wording does not promise cross-compatibility with VaporView session files; document the standalone format and reject unsupported inputs clearly.

## Groups and buses

Image three shows a full-width selected `Group 1` row, left disclosure chevron and a separator labeled `---` beneath it. Reproduce these as organization rows: groups expand/collapse their children, can be renamed and moved, and separators consume a normal row without a signal value. Group selection is not an aggregate signal measurement. Children retain individual visibility state when a group collapses.

Image one shows vector names such as `[31:0]` and filled pale-green polygons with hexadecimal labels. That is an upstream multi-bit bus rendering reference. MCHPRS binary probes remain individual one-bit rows. Bus construction, bus bit ordering, radix formatting, custom bit slices, analog rendering and protocol decoding are later features; do not simulate them by exposing redstone power as a 4-bit measurement. A later explicit user-defined bus may combine binary probes only after its ordering and unavailable-bit rules are designed.

## Accessibility and data boundaries

Expose trees as navigable trees, splitters as separators, menus as menus and checkboxes with names/states. Preserve a visible keyboard focus ring independent of selected-row fill. Tooltips explain glyphs; color is never the only indication of On/Off, unavailable state, visibility or selection. Provide a DOM-readable selected-signal event table/value summary alongside any canvas renderer. Support reduced motion, browser zoom and high contrast. Focus returns to the invoking row when a menu closes; dialogs trap focus only while open.

Treat capture names, probe names, aliases and loaded session fields as untrusted text: render text nodes, not HTML. Bound session file size and validate its schema. Clipboard failures receive a small accessible error. Session storage/export excludes bearer credentials, private API URLs with credentials and any Minecraft write authority. Handle unauthorized/expired access, disconnected server, empty capture, expired cursor and memory-limit stop as explicit states. A disconnected trace must not appear to continue recording.

## Delivery scope and acceptance

Stage 1 is the standalone binary viewer using the current approved simplified composition: three coordinated columns, the single netlist, tick ruler, binary fixture pulses, selection, reorder, visibility, A/B markers, fit/zoom/pan/alignment and both context menus. Implement grouping/separators and local presentation actions required by those menus; WaveDrom export entries remain deferred; Export VCD works from the topbar. It must run without VS Code, code-server or an extension installation. The fixture is labeled synthetic and must not imply live Minecraft capture.

Stage 2 connects the authorized read-only capture API, handles generation/retention/partial ticks and refreshes consistently. Auto Reload is an opt-in presentation feature, not recorder start/stop. Local session save/load should use a versioned standalone schema and stable capture probe IDs; source identity mismatches require a clear report.

Stage 3 may add user-defined buses, richer export such as WaveDrom, value-search refinements and other upstream options if specifically requested. These do not block the binary capture viewer.

Acceptance checklist:

- [ ] At 1701 × 738 compact typography, dark surfaces and dominant graph match retained reference styling with the approved red theme; only topbar plot X/Z, netlist and waveform remain, with no editor chrome, filename strip or second tree; the bottom bar contains only right-aligned delta/A/B.
- [ ] At 1746 × 843 the top ruler, label/value dividers, group selection and empty space retain useful reference proportions; netlist and label/value splitters work by pointer and keyboard, and high plateaus have subtle red downward shine with red traces and active accents.
- [ ] Every row's name, marker value and trace remain aligned through resize, vertical scrolling and row-height changes.
- [ ] The single netlist scrolls and expands/collapses; no Displayed Signals panel is present; the bottom bar contains only right-aligned delta/A/B.
- [ ] Reordering changes no capture events; hiding/restoring retains position; Remove Variable affects local display only.
- [ ] All visible context-menu labels, dividers and shortcut annotations match the two images; no unobserved submenu labels are claimed as exact.
- [ ] Fit, zoom, pan, start/end and A alignment work on empty, one-tick and long captures without negative ranges or zero-width division.
- [ ] Ruler and delta use game ticks at every TPS; no nanosecond or strength axis appears.
- [ ] A/B values and delta are consistent; multiple transitions in one tick remain inspectable in sequence order.
- [ ] An On/Off/On pulse within one tick, a constant signal, missing dust, a partial tick and an expired range all have honest distinct rendering.
- [ ] Groups and separators reorder and collapse predictably; binary probes do not acquire fabricated bus or analog semantics.
- [ ] Keyboard-only users can select, reorder, toggle visibility, navigate markers and operate/close menus; names containing HTML render as text.
- [ ] Unsupported/deferred actions are visibly disabled; session import/export never deletes probes or exposes access credentials.
- [ ] A fixture-only preview is explicitly synthetic; integration states cannot claim successful live capture before the recorder/API exists.

This specification does not copy or audit upstream implementation code, verify every upstream shortcut, or establish session-file interoperability. Visual measurements need browser comparison during implementation; unknown submenu details remain deliberately unspecified.


## Source-verified VaporView scrolling contract

The user's requested scrolling feel is a core requirement. The following was verified directly in upstream [vaporview.ts](https://github.com/Lramseyer/vaporview/blob/main/src/webview/vaporview.ts) (`VaporviewWebview.scrollHandler`, `isTouchpad`, constructor) and [viewport.ts](https://github.com/Lramseyer/vaporview/blob/main/src/webview/viewport.ts) (`handleScrollEvent`, `handleZoom`, `applyZoom`). These source files were inspected on 27 October 2026; links track main rather than a pinned release. This section supersedes any generic wheel convention: ordinary mouse-wheel motion over the graph pans time, not rows.

| Input over graph | Upstream mapping to reproduce |
| --- | --- |
| Shift + wheel | Vertical signal scrolling by `deltaY || deltaX`. This branch has priority even when Ctrl is also down. |
| Ctrl + wheel/pinch | Pointer-anchored zoom. On macOS Meta is also accepted. Negative Y zooms in; positive Y zooms out. |
| Mouse mode, ordinary wheel, zero X | Add deltaY to horizontal/time offset; leave row offset unchanged. |
| Touchpad mode, ordinary gesture | Add deltaX to time offset and deltaY to vertical row offset in the same event. |
| Either mode with nonzero deltaX | Route both axes independently, as for touchpad. |
| Wheel over names or values | Vertical row scrolling by deltaY; synchronize all waveform rows and leave time unchanged. |

The extension exposes Mouse, Touchpad and Auto modes; its manifest default is Mouse. **Proposed standalone default:** Auto with a compact mode selector/fallback so hardware misclassification can be corrected. This is an explicit adaptation of the default, not an upstream fact.

Auto detection: reuse the last classification during a 100 ms gesture window. Otherwise, if the nonstandard `wheelDeltaY` is truthy, classify as touchpad only when it equals `-3 * deltaY`; if it is absent/falsy, pixel-mode `deltaMode === 0` is touchpad; other cases are mouse. Each unmodified gesture event extends the 100 ms window. This is a heuristic and cannot reliably identify all hardware; explicit modes are necessary. Do not reinterpret every pixel-mode event as a mouse without the user override.

Zoom amount in explicit Mouse mode is +1 or −1 based on Y's sign, giving a halving/doubling of pixels per tick. In Auto/Touchpad use `divider = 18 / sensitivity`, clamp deltaY to ±divider, then pass `amount = clampedDelta / divider`. Upstream default sensitivity is 1. Apply `newPixelsPerTick = oldPixelsPerTick * 2 ** (-amount)`, bounded by the viewer's supported zoom range. For pointer x relative to graph, compute the timestamp under x before changing scale, then set new horizontal offset to `timestamp * newPixelsPerTick - x`. Clamp at retained bounds; only this boundary clamp may break perfect pointer anchoring. Tick labels remain integral/readable even when the viewport transform uses fractional coordinates.

Upstream uses clamped pseudo-scroll coordinates and translates label/value positions together. Its wheel path updates the viewport directly; the inspected code does not add a velocity model, friction or synthetic momentum. Preserve native trackpad event momentum; do not invent an inertia animation. Separate animated toolbar zoom helpers exist, so their existence must not be confused with wheel inertia. React may coalesce painting into requestAnimationFrame, but must accumulate every input delta and not discard gestures between frames. Use a native non-passive graph wheel listener so preventDefault works; stop browser/page scrolling only within the handled region. Keep the DOM and canvas on the same single vertical offset.

Scrolling acceptance checks (pure input-routing/viewport tests plus browser hardware review):

- [ ] In explicit Mouse mode deltaY=100 with deltaX=0 increases time offset by 100 px and leaves row offset fixed; Shift routes that same input to rows.
- [ ] A diagonal touchpad event changes both offsets by their respective deltas with no axis jumping; a nonzero X event does so even in Mouse mode.
- [ ] Ctrl+Shift follows Shift's row-scroll priority; wheel over labels/values always scrolls rows.
- [ ] Auto classification remains stable for a burst under 100 ms, can change after the gap and can be overridden explicitly.
- [ ] Mouse Ctrl+wheel zooms by powers of two; touchpad/pinch uses the clamped continuous amount; the tick beneath a pointer at 25% width stays at that position except at bounds.
- [ ] Repeated gestures neither scroll the browser page nor desynchronize labels, values, grid, markers and trace rows.
- [ ] Horizontal and vertical offsets clamp at both ends; fit/single-tick/empty data have no NaN or divide-by-zero behavior.
- [ ] High-resolution input deltas accumulate between paints; no synthetic momentum continues after input stops.
- [ ] Review on a physical mouse and touchpad. Synthetic WheelEvents verify routing, not actual device detection or native gesture feel.

## React and TypeScript implementation shape

Use React + TypeScript under `waveform-viewer/`, with canvas for dense traces/ruler and React DOM for the netlist, menus, compact topbar, minimal delta/A/B bottom bar and accessible controls. Keep this a small application, with no extension host and no parallel frontend framework.

A top-level viewer owns capture identity, immutable capture data, ordered presentation rows, selection, visibility, marker A/B, viewport transform and menu state. A sidebar renders only the netlist; a toolbar invokes named viewport actions; a waveform viewport renders coordinated labels/values and the canvas. A context-menu component uses the exact row/blank command lists above. These are responsibility boundaries, not a requirement to create a file or abstraction for every noun. Keep state local unless more than one component needs it; a reducer is useful only where coordinated changes actually need atomic updates. No global state dependency is required.

Keep tick/value lookup, row flattening and viewport/zoom/input-routing calculations in small pure TypeScript functions with focused tests. Canvas receives data, flattened visible rows, dimensions and a viewport transform; it must not independently own selection, duplicate the time axis or mutate capture records. Maintain stable IDs across reorder/hide and use them as React keys. ResizeObserver drives viewport size; device-pixel-ratio scaling keeps strokes clear. One requestAnimationFrame painting boundary may batch input-driven draws, using refs for transient input accumulation while React state remains the reviewable source of presentation truth. Clean up listeners, observers and animation frames on unmount.

Do not build a plugin system, renderer framework, generic event bus or speculative transport adapters. Start with a labeled typed fixture and replace its data source with the read-only API in stage 2. Checks must distinguish working preview interactions from real capture integration.

Additional final acceptance checks:

- [ ] Plain toolbar arrows and Left/Right keys move A by exactly one game tick even when no event exists there; explicit edge-navigation buttons retain separate transition behavior.
- [ ] Context-menu option state remains checked/selected after reopening and visibly matches current rendering.
- [ ] Export VCD downloads a parseable standard file with 1 ms timescale, 50 ms per default game tick and sequence-ordered same-tick transitions sharing one timestamp.
- [ ] Red traces/high-state gradients and active/menu/selection accents are the defaults; green remains historical reference only.
- [ ] Topbar has plot X/Z Load and Export VCD with plain search/navigation, no app title/filename/bin/t= chrome; bottom bar has only right-aligned delta/A/B.
