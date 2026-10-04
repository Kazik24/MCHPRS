# Piston implementation review

Reviewed 2026-10-04 against the current timing repair, the pinned Java source and live fixture traces.

The straightforward design is retained: one ordinary tick scheduler, one FIFO of piston events and an ordered vector of moving identities/progress. These share a phase machine for game/nano/pico advancement. The rules table and adhesive resolver were removed as requested. Correctness takes priority over adding a general simulation framework.

## Findings fixed

1. Event writes incorrectly rechecked the source itself. That generated an immediate second event in the memory cell. Notifications now target neighbors; completion runs the restored component's placement/recheck once. The memory cell matches Java.
2. Destination shape changes were missing for moved blocks. Observers on the pushed side missed their pulse. Destination notifications now match all four existing update-tester traces.
3. Removing an owned counterpart did not notify its neighbors after both cells were cleared. A torch above a vertical head could survive breaking the base. Removal now returns the affected counterpart and the caller notifies it after the ordinary destruction path. A regression covers that support loss and preserves valid support beforehand.
4. Network/chunk NBT used current progress, whereas Java serializes previous/interpolated progress. Exact current/previous values remain in runtime/save metadata; the legacy byte now follows the Java NBT convention. The half-step and first two NBT progress values have regression coverage.
5. Invalid imported motion progress was silently clamped. NaN, infinity and values outside 0..1 now return a malformed-entity error.
6. Failed survival on restoration could leave a carried block entity attached to air. Completion now removes that entity before clearing the cell.

## Coverage and retained choices

Regression coverage includes extension-time base flags, no future base lock, captured drop actions, event cancellation/duplicate suppression, source/head removal, source and payload replacement, repeated stale completion, six directions, storage boundaries, current/previous progress, old observer ticks, destination-pending observer ticks, normal/interrupted waterlogging, invalid support, partial-step restart and frozen format-3 migration.

A scheduled block tick identifies a block type, as Java does. Changing properties does not invalidate it; replacing it with another kind does. Moving-entity work separately uses an incarnation identity. Keeping those concepts separate is necessary.

Neighbor iteration order stays local to piston callbacks. The repair does not globally reorder every redstone component. Compiler handoffs reject piston/observer storage or outstanding piston work before extracting ordinary ticks; unsupported piston nodes are not compiled.

The supported evidence is five circuit traces plus the signed-adder recordings. It does not establish every callback order or all vanilla gameplay. The supplied adder has missing sixteen-bit coverage and a changed-input arithmetic limitation on Java itself; [the adder report](ADDER_TEST_REPORT.md) records those limits.

## Performance candidates

The runtime vector keeps deterministic insertion order and a compact save representation. Its position/identity lookups and removals scan active motions; benchmark this before introducing another index. Straight-chain source clearing also repeatedly scans overlapping destinations even though all sources except the first are overwritten by the preceding move. That can be simplified without changing movement order. Registry lookup is currently linear and ordinary typed tick checks exercise it frequently. Measure these gaps in task 7 before choosing changes.
