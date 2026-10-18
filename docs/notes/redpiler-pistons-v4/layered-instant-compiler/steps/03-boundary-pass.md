# Step 03: one authoritative boundary-identification pass

Status: planned; requires approved local contracts. Stop after boundary review.

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
