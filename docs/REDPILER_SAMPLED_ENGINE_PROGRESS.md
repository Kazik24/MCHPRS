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
- Direct-program tests cover two independently addressed sources contributing
  13 and 15 to a wire, masking, restoration to 13, final shutoff with the existing
  four-game-tick lamp delay, and an electrical change without a lamp notification.
  These tests establish competing contributions and update separation, not
  watcher or source-ordering completeness. Structural Program validation is
  implemented; checkpoint 1 remains incomplete because watcher tables,
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

Explicit ordered-contribution increment verification:

- `cargo test -p mchprs_core redpiler::sampled --lib`: 12 passed, 0 failed,
  0 ignored, 1,035 filtered out. The new order test confirms that a receiver
  instruction sees the prior contribution if it precedes `Contribution(link)`;
  reversing the instructions lights the lamp. The electrical-change-without-
  notification test executes contribution instructions and contains no lamp
  update instruction. A malformed-program test covers record lengths, unique
  positions, supported block types and ranges, source bindings, rise/fall pair
  count, link endpoints and types, attenuation, and operation references.
  Empty/partial operation programs remain valid. The eleven previous tests,
  native comparison and compiler self-validation pass.
- `cargo check -p mchprs_core --tests`: passed with four existing dead-code
  warnings.
- `git diff --check`: passed.
- Watcher dispatch and MAIR's special reactive/BUD batching path remain
  subsequent work and are not implemented by this change.
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
