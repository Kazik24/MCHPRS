# Sampled Redpiler implementation status

Verified 30 October 2026 against the working tree. The sampled lever–dust–lamp
gate passes, and compile-time inventory of bounded empty piston actors is
implemented. Piston execution remains unimplemented; the player-facing backend
remains Graph.

## Current implementation

- The indexed-runtime increment of checkpoint 1 addresses inputs with
  `set_input(source_index, powered)`, selecting the matching source rise/fall
  program. Runtime retains each attenuated contribution and computes an
  electrical input as the maximum of its incoming links. `Contribution(link)`
  recalculates one link from its source's live output; `RefreshWire` stores the
  wire's current input power without implicitly refreshing outgoing links.
  Source changes update the source output, then run the selected ordered
  program. Contribution changes alone do not execute receiver instructions.
- The compiler places `Contribution(0)` (lever → wire) before sampled delivery
  instructions and inserts `Contribution(1)` (wire → lamp) immediately after
  every sampled `RefreshWire`. Repeated sampled deliveries and their order are
  retained. This lowering is valid only for the currently admitted single-chain
  topology; it is not generalized to undiscovered geometry.
- `WatchedWire` records keep a wire index and ordered lamp receiver indexes.
  Runtime stores each observed raw wire strength separately from physical wire
  state and link contributions. `CheckWatchedWire` recomputes the maximum input
  contribution, updates the observation only on a raw strength change, and
  dispatches watchers in order. Duplicate recipients normalize to their first
  occurrence. Lamp delivery uses `UpdateOnce` within explicit external-input
  phases; an effective link activation change invalidates its target's marker.
  Production compilation currently installs an empty watcher table.
- Direct-program tests cover two independently addressed sources contributing
  13 and 15 to a wire, masking, restoration to 13, final shutoff with the existing
  four-game-tick lamp delay, and an electrical change without a lamp notification.
  These tests establish competing contributions and update separation, not
  watcher discovery or source-ordering completeness. Structural Program
  validation and the lamp-only watched-wire mechanism are implemented;
  checkpoint 1 remains incomplete because world-derived watcher discovery,
  receiver-port representation and semantic program validation remain.
  World-derived multi-source extraction and piston execution follow afterward;
  production admission remains the single-chain contract below.
- `crates/core/src/redpiler/sampled.rs` contains a private compiler, immutable
  Program and separate Runtime. It records native callbacks in an isolated
  ScratchWorld, separately for rising and falling source changes.
- Runtime instructions are `Contribution`, `RefreshWire` and `UpdateLamp`. They
  read live runtime state; delayed lamp off uses TickScheduler and rechecks
  power. Earlier state-delta and frame-replay implementations have been
  superseded.
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

Ordered contributions and lamp-watcher mechanism verification:

- `cargo test -p mchprs_core redpiler::sampled --lib`: 16 passed, 0 failed,
  0 ignored, 1,035 filtered out. Tests cover raw-strength masking and 15→13
  changes, unchanged repeated checks, independent lamp power, duplicate watcher
  suppression, contribution-driven UpdateOnce invalidation, phase reset,
  recipient normalization and malformed watcher records/instructions. Delivery
  counts are asserted. The prior twelve tests, native comparison and compiler
  self-validation pass.
- Explicit-contribution order and malformed-Program tests remain passing;
  empty and partial operation programs remain valid.
- `cargo check -p mchprs_core --tests`: passed with four existing dead-code
  warnings.
- `git diff --check`: passed.
- No world-derived watcher discovery is connected to production compilation.
  Observer filtering, movement phases, receiver ports and MAIR's reactive/BUD
  batching path remain subsequent work and are not implemented here.
- The event-netlist suite was not rerun for this increment; its separately
  recorded ADDER1BIT failure below remains unresolved.

Earlier run before the two boundary-read cases, `cargo test -p mchprs_core
redpiler::sampled --lib`:

- 4 passed, 0 failed, 0 ignored; 1,035 filtered out.
- `native_edges_and_host_preservation` covers initially-off and initially-on
  entry. The initially-on fixture is activated natively and settled for four
  game ticks before compilation. Both cases check host blocks, entities,
  scheduled work and piston state across compilation, then compare loaded
  component state and pending ticks immediately and after each of four game
  ticks for 12 alternating transitions. This crosses the 32-bucket scheduler
  wrap. Each falling edge keeps the lamp lit through ticks 1–3, extinguishes it
  at tick 4, and leaves no pending work at the settled boundary. Compiled
  operations are asserted to have executed.
- The compiler's sampled-runtime self-check and its block/pending-tick
  comparisons remain enabled.
- Tight-capture and excluded redstone-block cases both reject with the
  positioned `scratch probe read outside captured boundary` error. Each test
  accepts any reported position outside its capture and verifies that host
  blocks, block entities, scheduled work and piston state are unchanged.

Latest sampled-suite rerun after adding those boundary cases:

- 8 passed, 0 failed, 0 ignored; 1,035 filtered out, including empty-piston
  inventory, malformed contact rejection, escaped-contact rejection and
  transitive actor grouping.

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

## Scheduler correction

Runtime now drains all due work in the current bucket, advances once, and drains
all due work in the new current bucket, using the same lamp dispatch each time.
This matches native MCHPRS's per-game-tick scheduler phase order. The repeated
native comparison verifies the wraparound and delay boundary.

`schedule_tick(..., 2, ...)` means two redstone ticks, hence four game ticks.
The delay remains unchanged; the lamp remains lit through tick 3 and turns off
at tick 4.

## Remaining work

1. Pass the piston execution gate: extension, retraction and rearm against native
   execution with actual accepted actions and pending work checked.
2. Feed world-derived source and watcher tables into one independent runtime;
   preserve the old event experiment as a reference until replacement gates pass.
3. Add observer updates, movable resources and BUD behavior after the piston
   execution gate; inventory alone does not establish piston execution support.
4. Establish interacting arrangements and real-circuit correctness before public
   activation, lifecycle integration or performance claims.

See the [milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md),
[architecture](REDPILER_ARCHITECTURE.md) and
[implementation assignment](MAIR_PORT_IMPLEMENTATION_PROMPT.md).
