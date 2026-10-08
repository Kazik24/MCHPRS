# Wire pen

Hold any carrot on a stick and right-click to draw redstone. `/wire` is a
convenience command: it uses a held carrot on a stick or puts a named pen into an
empty hotbar slot, and resets the current route.

Leaving the plot, disconnecting, changing the held item, or `/wire off` cancels
the current route and clears its preview. Hold the pen and right-click to start
again; another `/wire` command is unnecessary.

## Controls

| Action | Result |
| --- | --- |
| Right-click a block | Use that block as the first support, with dust directly above it. |
| Right-click existing dust | Reuse that dust cell as the starting endpoint. |
| Aim at a destination | Preview a safe route with its endpoint on the drawing plane. |
| Aim into air | Move the endpoint where the view ray meets the selected plane. |
| Right-click a fully displayed green route | Build the segment and continue from its endpoint. |
| F, the default offhand-swap key | Cycle horizontal, vertical X, and vertical Z drawing planes. |
| Sneak + F | Change the preferred bend order. |
| Sneak + right-click | Cancel the current segment. |
| Change held item or `/wire off` | Cancel the route and remove its preview. |
| Hold the pen again and right-click | Start a fresh route. |
| `/wire` | Obtain a named pen or reset the current route. |
| `//undo` | Restore the previous segment's construction geometry. |

Use an intermediate point to guide the route around machinery or to split a
long connection. A preferred bend orders search choices; it is not permission
to build through an obstruction. Green means the current candidate passed the
supported checks. Pending or stale previews cannot be placed. The action bar
shows the active plane and latest routing status, including a missing route,
exhausted resources, or unsupported circuit context. Automatic status changes
are deduplicated and
limited to one update per second; they do not fill chat with search, budget,
stale-preview, or placement messages. Explicit `/wire` and `/wire off` commands
can send one chat confirmation.

The clicked block itself is the first support; dust goes directly above it.
Existing dust endpoints keep their positions and supports. The pen adds missing
supports and their dust together: glass for flat runs, white wool for ascending
and descending steps where an opaque support is required for dust to connect in
both directions.

Drawing starts on the horizontal plane through the selected start. F
cycles the planes:

| Plane | Endpoint movement |
| --- | --- |
| Horizontal (X/Z) | Move along X and Z at the current start's height. |
| Vertical X (X/Y) | Move along X and Y at the current start's Z. |
| Vertical Z (Y/Z) | Move along Y and Z at the current start's X. |

Each plane passes through the current start, including the endpoint of the last
built segment. To create a route through several planes, build an intermediate
point and switch planes there. The selected plane constrains the aimed endpoint;
the router can take safe detours in three dimensions. The route follows legal
dust steps: vertical connections need staircases with horizontal space, since
dust stacked directly above dust cannot form a continuous wire.

The tool neither checks signal range nor places repeaters or other tick-delay
components. A long dust route can attenuate to zero; manage signal restoration
yourself.

## Safety contract

Connecting the selected endpoints changes the circuit intentionally. The tool
checks placement geometry and rejects unintended contacts outside those
endpoints. The safety checks inspect potential interactions, including sources
that are currently off, rather than treating the current powered state as proof
of isolation.

Dust connectivity, support conduction, strong-power sources, component input
faces, and notification-sensitive context matter. Nearby moving geometry,
quasi-connectivity, BUD behavior, observers, or other interactions that cannot
be validated conservatively cause refusal. This is a guarded construction tool,
not a proof of arbitrary circuit equivalence or transient timing. The redstone
power and dust-geometry helpers remain the authority for supported predicates;
the compiler parser is reused only where its model applies.

Search and validation use a private snapshot and proposed-world view. Preview
entities are sent only to the viewer and do not affect simulation. The live
world is checked again before placement. A changed world refreshes the preview
instead of placing an obsolete plan. Plot ownership, permissions, build bounds,
and Git checkout locks remain in force.

Placement uses normal world updates and one WorldEdit history entry. `//undo`
and `//redo` restore geometry; they do not roll back ticks, pulses, moving blocks,
or any other simulation history. Use the tick-history commands for that separate
operation.

## Work and latency limits

Each routing request stops when the first applicable limit is reached:

| Resource | Initial limit |
| --- | ---: |
| Expanded search states | 8,192 |
| Geometry/dependency reads | 131,072 |
| Worker search and validation | 50 ms |
| Captured geometry | 131,072 cells (512 KiB of block states) |
| Snapshot and scratch memory per request | 16 MiB |
| Shared retained snapshot allocation | 64 MiB |
| Planned dust/support placements | 512 |

These are construction and resource limits, not signal-range limits. Reaching a
budget does not prove that no safe path exists. Try a closer intermediate point
or another route.

The search corridor surrounds the endpoint bounding box with four blocks of
horizontal padding and two blocks of vertical padding, clipped to plot bounds.
The captured safety context extends another 13 blocks horizontally and 14
blocks vertically. A route outside that corridor is outside this request's
search, even if it would be possible elsewhere in the plot. Closer intermediate
points change the corridor and reduce snapshot work.

Shared reservations cover retained snapshots and in-progress captures; they are
released when the last owner drops them. The worker queue and node allocation
limits also bound concurrent searches and scratch work.

Aim and relevant geometry changes schedule previews at up to 20 Hz. Each player
has one active request and a replaceable latest target, so cursor movement does
not build a queue of obsolete searches. Cancellation is cooperative; generation
checks reject old results. Geometry changes also reconsider a stationary target.

The tool shares Git diff's viewer-only marker renderer, with its own update
cadence and marker ownership. Unchanged marker IDs are retained. Marker work is
bounded per update, and placement waits for the current overlay to finish.
Simulation batches yield after completed ticks at approximately 5 ms while the
tool is active; compiled changes are flushed before reading the world. A single
expensive tick or flush can still exceed this interval.

## Checks

Run the targeted routing checks and documentation validation from the repository
root:

```sh
cargo test -p mchprs_core wire --locked
python tools/validate_docs.py
```

Exercise live use with rapid aiming, stationary obstacle edits, paused and high
TPS, and more than one player. Check stale-result rejection, budget feedback,
marker cleanup after leaving or changing items, Git preview coexistence, and
segment undo. Automated checks do not establish live client latency.
