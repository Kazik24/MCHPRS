# Sampled Redpiler implementation status

Verified 30 October 2026 against the working tree. Milestones 1–2 are not
complete: the supported lever–dust–lamp acceptance fixture currently fails.
The player-facing backend remains Graph.

## Current implementation

- `crates/core/src/redpiler/sampled.rs` contains a private compiler, immutable
  Program and separate Runtime. It records native callbacks in an isolated
  ScratchWorld, separately for rising and falling source changes.
- Runtime instructions are `RefreshWire` and `UpdateLamp`. They read live
  runtime state; delayed lamp off uses TickScheduler and rechecks power. Earlier
  state-delta and frame-replay implementations have been superseded.
- Compilation repeats the first edge, rejects divergent samples, checks return
  to the loaded settled state, and executes both edge programs against recorded
  blocks and pending ticks before accepting a program.
- Runtime has no world handle, Graph executor or interpreter call. Frames and
  callback records are compile-time evidence, not runtime answers.
- `redpiler/mod.rs` declares the private module. PlotWorld's existing redstone
  update observation accessors are available to compile-time probes. Piston
  notification observation accessors remain test-only.

## Admission contract

Empty selections, or one floor lever, one dust and one lamp in a straight
horizontal chain with stone supports, are represented. Other components, block
entities and additional unrepresented fixed connections are rejected.

ScratchWorld rejects scheduled entry work, piston events, active motions,
pending movement work and mid-tick entry. Reads outside the capture are reported
by `finish_probe`. Inputs must settle before another edge. Each sample includes
the immediate boundary and four native game-tick boundaries. Loaded block states
are retained and checked rather than normalized to a convenient probe state.

## Verified results

`cargo test -p mchprs_core redpiler::sampled --lib`:

- 3 passed, 1 failed, 0 ignored.
- Empty admission and the two rejection tests passed.
- `native_edges_and_host_preservation` failed because compile returned
  `ScratchError` at (15,15,15), with reason
  `sampled behavior is not represented by runtime`.

The fixture contains assertions comparing host blocks, entities, scheduler and
piston state before/after compilation; compilation fails before those assertions
complete. Their presence is not a passing preservation result for this revision.

`cargo test -p mchprs_core redpiler::event_netlist --lib`:

- 45 passed, 1 failed, 1 ignored.
- ADDER1BIT `prepared-0-1-0` failed its raw sum repeater check at tick 3:
  actual 0, expected 1.
- COUNTER_BASIC remains explicitly ignored for its recorded divergence.
- Static wire/piston and selected discovered-resource BUD comparisons passed.
  Those adapters still depend on LoweredCircuit and do not establish direct
  world-derived sampled-engine support for pistons or resources.

Earlier passing tests and cargo check/fmt reports apply to earlier revisions.
They do not supersede these execution failures.

## Scheduler defect and required correction

The sampled Runtime drains the old current bucket and then advances once, but
does not drain the new current bucket. Native MCHPRS drains both, with exactly
one advance per game tick. A request scheduled at cursor 0 with game delay 4
must execute on game tick 4; this runtime leaves it until tick 5.

`schedule_tick(..., 2, ...)` means two redstone ticks, hence four game ticks.
`schedule_half_tick(..., 4, ...)` expresses the same delay. Preserve that delay;
fix the phase order instead of changing the fixture or shortening the request.
Retain pending-tick comparisons at every boundary when verifying the fix.

## Remaining work

1. Pass the existing sampled acceptance fixture and add focused coverage for
   initially-on entry, repeated settled edges and scheduler boundary timing.
2. Add explicit outside-dependency and tight-capture rejection coverage. Complete
   passing host-preservation assertions; avoid a source-preservation claim based
   solely on a failed compile.
3. Feed world-derived source and watcher tables into one independent runtime;
   preserve the old event experiment as a reference until replacement gates pass.
4. Add generic component behavior and source-specific rise/fall programs, then
   empty pistons, observers, resource movement and BUD gates in order.
5. Establish interacting arrangements and real-circuit correctness before public
   activation, lifecycle integration or performance claims.

See the [milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md),
[architecture](REDPILER_ARCHITECTURE.md) and
[implementation assignment](MAIR_PORT_IMPLEMENTATION_PROMPT.md).
