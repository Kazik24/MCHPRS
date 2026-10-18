# Step 03: one authoritative boundary-identification pass

Status: implemented and validated; awaiting human review.
Runtime replacement remains off.

## Goal and integration

Implement `passes/identify_instant_boundaries.rs` as a read-only `PlanBuilder` returning
one typed boundary catalog through `PlanOutput`. Follow the existing cancellation/atomic
publication API. Every pass stays in its own file under `passes/`.

In the explicit experimental pipeline, replace old `identify-instant-regions` and proxy
construction with the new flow. Do not run two discovery engines and compare whichever
answer looks more useful. Public O3 remains unchanged during this stage.

## Algorithm

1. Reuse step 02 cells and indexed potential dependencies. Mark every ordinary component,
   unrecognized actor, BUD owner and classic-owned wire as an outside endpoint.
2. Determine which wire/conductor corridors can belong to instant logic. Classify across
   every reachable contact alternative, not just the entry pose. A support or geometry
   dependency can require a port even when no electrical edge crosses the boundary.
3. Collect inputs and outputs separately for power/QC, Update/Shape, geometry and external
   memory. Keep original route occurrence, direction, guard support and live/captured meaning.
4. Check guard atoms, comparator overrides, conducted paths, regulated dust emission,
   captured wire neighborhoods and observer watches. Common fanout does not imply ownership.
5. If a local cell or corridor is demoted, revisit adjacent classification and ports with
   a worklist until stable. Deduplicate identical read bindings; preserve separate callback
   occurrences when their multiplicity/directions can affect behavior.
6. Validate exclusive actor/resource/reset ownership and complete outside read coverage.
   A shared resource crossing ordinary memory is not duplicated or silently internalized.
7. Publish a deterministic catalog and structured declines. No graph rewrite or runtime
   activation occurs. The second pass will form connected instant regions from this catalog.

## Reuse/replacement work

Move useful boundary/ownership scans from `analysis/instant_regions.rs` into the new shared
analysis. Keep one implementation and migrate callers. Keep the old public pass adapter
temporarily only if necessary for unchanged O3 behavior; remove it at the final rollout.
Do not keep old `InstantRegion` and a new catalog as separately authoritative owner maps.

Use the existing JSON adapter to add an optional cold boundary catalog. Older exports
without the table must remain readable. Update validator/loader/viewer only for the actual
new table: do not build a separate simulator or redesign the viewer.

## Required tests

- Two independent mechanisms with one common external source remain separate candidates.
- A component supplying data and receiving updates has distinct input/output bindings.
- External QC data without activation leaves BUD memory unchanged.
- Internal power edges depending on external guard state keep that input.
- External readers depending on an internal guard/contact keep that output.
- Comparator far override, conductor, head observer and captured outside-wire reads.
- Every potential shared contact pose is covered; late geometry changes cannot cross an
  unrecorded boundary. Ordinary Shape checks are not treated as electrical updates.
- Classic and instant ownership never overlap; cancellation publishes no partial catalog.

## Gate

Readable per-fixture boundary exports, deterministic ownership and explicit rejection
witnesses. Reuse the current boundary-contract tests, adding the missing interfaces.
No change in ordinary execution and no claim of runtime work removal yet.

## Implemented structure

`passes/identify_instant_boundaries.rs` is a read-only builder. The explicit
`experimental_boundary_pipeline` replaces the old region/proxy builders; it is selected
by the fixture tests, not by public O0–O4 compilation. No new flag or executor is added.
Publication clears the old ownership plans, and graph validation rejects coexisting owner
maps or a catalog that differs from deterministic graph-derived inference.

The small `analysis/instant/boundaries.rs` coordinator binds admitted owners and collects
the result. `boundaries/corridors.rs` discovers connected non-classic wire/conductor groups;
`boundaries/scan.rs` classifies original dependency rows. Legacy region discovery uses the
same basic scans with its existing admission map, without expanding the experimental guard,
contact or comparator catalog. `cells.rs` returns dense owner maps from its existing closure
instead of making boundary analysis repeat that closure.

`program_graph/instant_boundaries.rs` defines the immutable `BoundaryCatalog`, separate
cell/corridor ownership, port direction and typed source witnesses. It does not own mutable
state or publish an electrical result. Cells and wire corridors remain separate; forming
whole instant regions belongs to Step 04.

### Port meaning

Port rows describe **owner-to-owner interfaces**, including connections between candidate
cells and corridors. Their row count is not the number of outside data bits. Step 04 will
connect compatible internal interfaces and retain the actual region perimeter. Update and
Shape occurrences retain different original ordinals even when recipients repeat.

Power rows retain Main/Quasi/conducted channels, attenuation and guard provenance. Canonical
wire-neighborhood reads are also inputs: ADDER1BIT's trigger has no outgoing PowerLink.
Guard bindings retain the leaf atom ID; the source link/route/relation retains its enclosing
condition. Dependencies of both sides of a conditional transfer are included.

Static constant-site geometry is a compile fact, not an external dynamic input. Referenced
changing contacts retain every possible profile/receiver alternative. A cursor processes
newly exposed contact/guard dependencies to a fixed point; it does not rescan all contacts
against all ports. Observer watches, captured Turbo operands, comparator rear/far reads and
note-block support/obstruction reads remain explicit.

Every actor is either admitted once or listed in a decline. Unseeded observer-free reset
mechanisms and ordinary BUD owners are explicitly outside. A QC output into BUD memory does
not imply ownership of that memory or its independent ordinary sampling-update path.

### Inspection

The normal JSON adapter adds optional `instant_cells`, `instant_corridors`,
`instant_boundary_ports` and `instant_declines` tables, with typed references to the existing
graph. They are absent from ordinary exports. The Rust validator, Python loader and HTML
viewer understand these tables, reject ownership conflicts and malformed references, and
continue accepting older exports without them. The viewer uses its existing record browser;
there is no second visualizer or simulator.

The fixture census writes review artifacts to `target/layered-instant-boundaries/`. These
are derived inspection artifacts; source schematics and checked-in graph fixtures are not
rewritten by this work.

## Fixture review snapshot, 2026-10-18

| Circuit | Local cells | Admitted / total actors | Wire corridors | Input / output interface rows |
| --- | ---: | ---: | ---: | ---: |
| ADDER1BIT | 6 | 13 / 13 | 4 | 1,149 / 1,032 |
| ADDER11BIT | 65 | 142 / 142 | 15 | 12,537 / 11,606 |
| Counter basic | 74 | 106 / 163 | 4 | 10,763 / 8,811 |
| Revised PC counter | 243 | 307 / 500 | 200 | 46,469 / 33,082 |

These are potential owner-local interfaces, **not** that many external inputs or outputs.
The adder tests verify the exact four/23 physical controls (data plus trigger), and all
declared timed repeater output positions. Payload and repeater observations remain separate
in the native differential check. The PC has three physical controls directly referenced
at this perimeter and 831 direct component output targets; other control paths can pass
through ordinary components. No roles are guessed from nearby signs. Every one of its 500
actors is admitted or included in one of the 145 outside-group witnesses.

All 22 IO fixture catalogs are exported, including explicit zero-admission results.
The PC export is compact JSON (about 37.65 MiB) to fit the existing 64 MiB tool limit;
the two adders' pretty exports are about 1.60 and 16.77 MiB. The default limits were not raised.
Both Python and viewer validators accepted all 23 real exports, in addition to their
20 and 26 focused unit tests.

New checks cover independent external fanout, both endpoint contexts of nested guards,
comparator rear/far/live-output dependencies, head observers, note-only contact reads,
all referenced contact alternatives, static geometry versus variable conducted power,
duplicate callback ordinals, Shape without piston activation, cancellation and corrupted
catalog rejection. Both adders also execute their eight/forty prepared histories with
full native/Graph state and pending-work checks and eight ticks of interpreter handoff.
These probes are separate from the isolated metadata tests that inject graph dependencies
without lowering the modified draft.

The construction uses iterative guard/contact worklists and bounded work/payload estimates.
Those estimates are not a peak-RSS measurement. No runtime benchmark belongs to this step:
the catalog does not remove wires, callbacks or scheduled execution yet.

## Completed verification

The final integrated debug Redpiler suite passed **411 tests, with 18 ignored**
(429 discovered). This includes the new catalog checks, both adder differential histories,
the PC boundary test and existing parser, scheduler, component, ordering and continuation
regressions. `cargo check -p mchprs_core --tests --benches --locked` also passed.

```text
cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1
cargo check -p mchprs_core --tests --benches --locked
py -3 -m unittest tools.redpiler_graph.test_loader
node --test tools/redpiler-graph-viewer/viewer.test.cjs
```

The suite log is `target/instant-boundaries-redpiler-suite.log`. Tool unit tests passed
(Python: 20; viewer: 26), and both validators loaded all 23 generated fixture exports.
That checks schema/validation behavior, not manual browser layout. Focused Rust formatting
and diff checks passed. Release tests, full-workspace tests, broad fuzz campaigns and runtime
benchmarks were not rerun for this gate. Candidate admission still does not mean kernel
execution; Step 04 must connect interfaces and derive causal layers before reduction.
