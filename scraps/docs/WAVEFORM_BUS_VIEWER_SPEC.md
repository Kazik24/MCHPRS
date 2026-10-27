# MROWW Waveform Viewer: grouped buses

Status: implemented in the frontend, 27 October 2026. This document records the
design and the implementation checks. Deployment is separate.
The product name is **MROWW Waveform Viewer**, including the browser title.

## Goal and reference

A group of watched binary signals can appear as one variable, with a combined
waveform and value in Decimal, Hexadecimal, Octal or Binary. Each format also
supports inverted logic: an unpowered bit means one. Inverting four-bit `0000`
therefore gives `0xF`; it never reverses bit order.

VaporView's documented reference behavior is a right-click **Format Values**
submenu, shaded multibit segments, a value column at the marker, draggable rows
and collapsible organization. Its Binary/Hex/Oct formats support unknown states;
Decimal falls back to binary for those states. Its bit splitting starts at bit
zero. See the official [value formatting](https://github.com/Lramseyer/vaporview#value-formatting),
[multibit renderer](https://github.com/Lramseyer/vaporview#multi-bit-display-type)
and [custom signals](https://github.com/Lramseyer/vaporview#custom-signals)
documentation. Combining independent Minecraft probes into a bus and the
inverted variants below are **our proposed adaptation**, not verified upstream
features. Reimplement the behavior using our existing components.

The current React viewer already has flat groups, row reordering, binary trace
rendering, menus, session files and per-plot local storage. Extend these rather
than adding an extension host, renderer framework or second trace model.
The [capture API](WAVEFORM_API_SPEC.md) continues sending physical binary probes.

## Interaction and bit mapping

- Keep existing folder groups. A group's context menu gains **Show as Bus**,
  checked when active. Enabling it combines its direct signal children, including
  children hidden from the viewer. An empty group cannot enable bus mode.
- First child is bit zero, next child bit one, and so on. A group label shows
  `name [N-1:0]`; expanded child labels show their bit indices. This matches
  `/waveform addbulk name N`, where the player clicks `name_0` first, low to high.
  Preserve numeric creation order: `name_10` must not sort before `name_2`.
  Names do not override an explicitly arranged group's order.
- Collapsing hides child rows and leaves the combined bus row. Expanding shows
  the same bus row above its individual physical bit rows. One-bit buses work;
  the group's display remains a bus rather than changing renderer unexpectedly.
- Moving a whole group preserves its bit mapping. Moving a child within a bus
  deliberately changes significance to match the new top-to-bottom order.
  Moving a child into/out of the group changes membership and width. Bit indices
  update immediately. Separators never consume bits; nested groups remain
  unsupported, matching the current session validator.
- A checkbox/visibility toggle only changes display. It cannot remove a bit
  from a bus, change width or alter its value. Removing a viewer row is an
  explicit membership edit; none of these operations removes a Minecraft watch.
- Make group membership visible: indent children beneath their group header,
  draw a subtle continuous group guide/boundary, and end it before the next
  ungrouped row. A color accent may reinforce this, but hierarchy must remain
  clear without color. Bus children additionally show their bit indices.
- Every grouped signal has **Remove from Group** in its context menu. This
  keeps the signal visible as an ungrouped row just after the group's remaining
  children, recomputes the bus membership and leaves the Minecraft watch intact.
  Dragging outside the group must likewise have a clear insertion boundary.
  If the last member is removed, keep the empty folder and disable bus mode.
- Default proposed format is **Hexadecimal**, unsigned. A bus defaults to the
  existing red trace color; retain the existing Color and Row Height menus.
  Changing the bus color does not recolor its physical children.

**Format Values** contains ten checked choices: Binary, Octal, Decimal,
Decimal U2 and Hexadecimal, then their five **Inverted** variants. Store radix,
signed-decimal mode and inversion separately. Reopening the menu must
show the active choice. For mixed selections apply only to bus rows; single-bit
rows keep their current binary display. No new global toolbar is needed.

The bus value column, **Copy Value at Marker**, immediate hover preview and
segment text use one formatter. Hover shows the full value and bit mapping;
it must not rely on delayed native `title` tooltips. The current marker A chooses
the settled value at its tick. Markers A/B, one-tick arrows, zoom and scrolling
retain their existing behavior. Explicit previous/next transition navigation
uses combined changes; rising/falling guides remain single-bit controls.

## Values and transitions

Use `BigInt` for packed values and known-bit masks, supporting up to 1024 bits.
Never use JavaScript Number or 32-bit bitwise operators for a combined value.
For width N, `widthMask = (1n << BigInt(N)) - 1n`. Unknown bits are tracked
separately; inversion is `(value ^ widthMask) & knownMask`. An unknown bit stays
unknown. Neither formatting nor inversion modifies recorded values or API data.

| Format | Proposed output |
| --- | --- |
| Binary | Exactly N digits, most significant first, with `0b` prefix; unknown bit is `x`. |
| Hexadecimal | `0x` plus ceil(N/4) digits, uppercase A-F; a digit containing an unknown real bit is `x`. |
| Octal | `0o` plus ceil(N/3) digits; a digit containing an unknown real bit is `x`. |
| Decimal | Unsigned integer without padding; if any bit is unknown, show the full binary representation. |
| Decimal U2 | Signed two's-complement integer at the bus width; if any bit is unknown, show the full binary representation. |

Pad Binary/Hex/Oct to the declared width. Unused positions in a partial leading
hex/octal digit are zero, never invented inverted bits. For example an inverted
five-bit zero is `0x1F`, not `0xFF`. Decimal U2 interprets the sign bit after
logic inversion, using the declared bus width; normal Decimal stays unsigned.
For four bits, `1111` is -1 in U2; inverted `0000` is also -1. Label a decimal
unknown fallback as `0b...` so it is unambiguous. Older saved formats without
the optional signed flag stay unsigned.

Build combined transitions from the union of member changes ordered by tick,
then existing exact `seq` comparison. Start with member initial states, apply
each record and update only that bit. Preserve ordered intermediate bus values;
the plateau after a tick is its final settled value. Changes that leave the
combined known/value pair identical do not introduce a new plateau.

All changes within a tick share the same horizontal coordinate. Multiple
ordered bus changes get the existing diamond indicator and an immediate preview
listing the ordered values. Do not invent sub-tick durations. A transient that
returns to its original value still gets an indicator. Carry the value from
before the viewport into its left edge; an ongoing bus segment must remain
visible when its beginning is offscreen. Continue the final segment to the
viewport/capture end.

## Canvas appearance and cost

Draw a filled band with short angled/crossed ends at changes, matching the
VaporView multibit appearance in the supplied screenshot. Use the row color,
readable monospaced value text and existing dark/red theme. This is a multibit
band, not a high/low digital trace or analog graph. Unknown values remain
distinguishable through their text, without treating them as zero.

Clip fills, angles and labels to the waveform area. Center the value in the
visible part of a long plateau. Shorten labels to fit with an ellipsis; at very
narrow widths omit text and retain boundaries. Hover/copy always expose the
full value, including 1024-bit values. Reuse existing canvas row coordinates,
DOM name/value cells, scrolling mechanics and marker drawing.

Compute bus data from changes, not one sample per bit per recorded tick. Reuse
the merged transitions across marker moves and redraws; invalidate when member
data, membership or capture identity changes. Formatting changes can reuse
packed transitions. Start with a merge/sort of actual member records; introduce
incremental merging or a worker only after measured need. Do not compute bus
waveforms for collapsed-away or otherwise invisible bus rows unnecessarily.

## Persistence and exports

Add optional bus settings on the existing group row. Persist format, inversion,
membership/order, color and collapse state through both local storage and viewer
session save/load. Reuse the current name-plus-position stable signal identity;
never persist ephemeral API IDs. A saved bus must retain its ordered stable
member keys so a temporarily absent probe becomes `x` in its original slot,
rather than silently shrinking the bus or moving higher bits down. An explicit
membership edit replaces this list from the current child order. Derive all bus
values from those references; do not save packed captures or command output.

Old groups without bus settings stay folders. Extend existing validators with
defaults and bounds (1-1024 unique member keys, supported radix, boolean
inversion); malformed settings must follow the current safe layout fallback.
Session membership resolves against the current capture and plot exactly as
existing sessions do. Capture replacement invalidates derived bus transitions.

VCD download continues exporting the original physical probes and their ordered
changes at 50 ms/tick. A `command_output` string signal carries command-block
messages grouped by tick using the string extension supported by VaporView/Wellen
and GTKWave. Browser grouping, number formats and inverted display
must not rewrite physical truth or add synthetic bus declarations in this first
version. Viewer-session files preserve bus presentation separately. Existing
WaveDrom export should include a visible bus once, with formatted settled values,
and include child rows only when actually visible; keep its existing bounded
range and state that same-tick detail remains available in VCD.

## Implementation plan and checks

1. Add optional group bus state, validation and stable-member persistence in
   `state.ts` and `layout.ts`. Keep old saved layouts usable and preserve missing
   member slots. Extend grouping/drag/remove operations in `App.tsx` to update
   membership in one place; hide/collapse operations must never touch it.
2. Add a small pure `bus.ts` module for assembly, merge and formatting, using
   existing `trace.ts` ordering helpers. Keep rendering out of this module.
3. Add checked bus/format menu items through `ContextMenu.tsx`'s existing menu
   model and `App.tsx`; use the same bus-value helper for copy and marker cells.
4. Extend `components/Waveform.tsx` with the filled bus renderer, clipped labels,
   combined transition navigation and immediate value/ordered-change preview.
   Preserve physical-bit high-state shine and existing scroll behavior.
5. Extend existing session/layout tests and add focused bus tests. Update
   WaveDrom handling; verify VCD is unchanged by bus format/inversion. Run the
   existing frontend tests, type check and production build.

Meaningful cases: low-first `0001`/`1000`; 4/5-bit inversions; unsigned 64-bit and
1024-bit maxima; partial leading hex/octal digits; unknowns in every format;
two bits changing in one tick, including a return pulse; a marker exactly on a
transition; segments starting left of the screen; hidden child still contributing;
collapse/expand and child reorder; stable membership after IDs change or a probe
is absent; old-layout defaults; menu checked state after reload; WaveDrom without
duplicate hidden bits; unmodified VCD physical values. Manually inspect narrow
segments, long decimal labels, immediate hover and mouse/touchpad scrolling.

Proposed defaults needing no backend decision: opt-in bus mode, Hexadecimal,
unsigned decimal, first child as bit zero and independent bus color. Automatic
grouping of bulk probes, arbitrary expression signals and synthetic
VCD buses are deferred. This scope changes no commands, permissions or deployment.
