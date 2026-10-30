# Sampled Redpiler implementation status

The world-derived compiler admits multiple levers, lamps, repeaters and static
redstone blocks, and retains native replay mismatches as diagnostics. A
lever–repeater–lamp circuit executes a scheduled repeater transition. Quiet
source probes preserve static redstone-block contributions; arrangement-aware
ordering is not yet implemented. ADDER1BIT fails admission at the extended
sticky piston at (49, 31, 42), before any fixture actions or token motions.
Runtime input and tick calls return errors, and compiled instructions and
scheduled events are limited to one million semantic operations per game tick.
Piston and motion events are not yet implemented or counted.

The private MAIR runtime and its lever–dust–lamp acceptance gate are implemented.
Compile-time inventory of bounded empty piston actors is also implemented;
piston execution remains the next unmet gate. The player-facing backend remains
Graph. The old `event_netlist` compiler, executor and executor tests have been
removed; their results below are historical only.

Repeater runtime mechanism is implemented and tested with manually constructed
lever → repeater → lamp programs. World-derived repeater compilation remains
pending, full side-locking behavior remains pending, and ADDER1BIT remains
incomplete.

## Current implementation

- The MAIR module is split into `sampled/compiler.rs`, `sampled/program.rs`,
  `sampled/executor.rs` and `sampled/tests.rs`.
- `scratch.rs` owns isolated native-interpreter worlds, entry-state validation,
  boundary detection, and native electrical-discovery helpers whose watcher
  integration remains pending.
- Shared `discover_wire_sources` performs electrical extraction without pistons
  or watcher records. The retained watcher-discovery entry point calls the same
  search. It preserves source identity (including initially-off sources), minimum
  attenuation, the 14-edge bound, directional strong conduction and MCHPRS stepped
  dust geometry. The compiler runs it against captured scratch state, checks
  escaped reads, maps positions to deterministic component indexes and resolves
  contribution instructions by their actual link indexes.
- Lamp inputs come from all six directional neighbors. The compiler collects
  weak sources, expands eligible dust with `discover_wire_sources`, and collects
  strong sources through one solid conductor hop. It deduplicates source
  identities by minimum attenuation, checks escaped scratch reads, and resolves
  source positions to deterministic component indexes. The admitted chain now
  uses a discovered lever-to-lamp contribution; it no longer synthesizes a
  wire-to-lamp link. Electrical contribution updates remain separate from lamp
  deliveries. Production admission allows one floor/wall lever, multiple dust
  wires, one lamp and decorative signs; multiple-source
  compilation, watcher installation and piston execution remain pending.
- `Program::new` validates component records, external sources, rise/fall
  programs indexed by component, electrical links, watcher records, repeater
  records and instruction references before building incoming indexes. Partial
  instruction sequences remain valid.
- Rise/fall programs contain ordered `Contribution`, `RefreshWire`, watcher
  check and explicit lamp delivery instructions. Explicit `UpdateLamp` always
  dispatches. Watcher delivery uses phase-scoped `UpdateOnce`; contribution
  changes can invalidate that marker. Production compilation still installs an
  empty watcher table.
- The runtime uses its own scheduler and lamp tick dispatch. It drains the
  current bucket, advances once, then drains the new current bucket. The lamp
  off delay remains four game ticks for `schedule_tick(..., 2, ...)`.
- The runtime supports rear-input repeater scheduling for loaded repeater state,
  delays 1–4, tick priorities, duplicate suppression, short pulses, and compiled
  rise/fall output programs. Repeater output contributions initialize from the
  loaded powered state. Pending work can remain queued across external input
  transitions. World compilation still rejects repeaters; side-input discovery
  and changing lock state are unsupported.
- Compile-time piston inventory records stable IDs, facing, sticky state,
  home/far cells, loaded extension/head state and transitive shared-cell groups.
  It rejects inconsistent, occupied, moving and out-of-capture contacts.
  Inventory does not change lamp-only production admission.

## Acceptance boundaries

The native comparison covers initially-off and initially-on entry. The initially
on fixture is activated and settled for four native game ticks before compile.
Both cases preserve host blocks, entities, scheduled work and piston state;
compare loaded component state and pending ticks immediately and after each of
four game ticks across 12 alternating transitions; cross the 32-bucket wrap;
and check lamp-off at tick 4 with no pending work at settled boundaries.
The compiler self-check remains enabled and compiled operations are asserted to
have executed.

Tight capture and excluded redstone-block-source cases reject with a positioned
boundary error. The reported position may be any recorded escaped read. Both
checks preserve the host snapshot, including the outside redstone block where
applicable.

The piston execution gate is still unmet: extension, retraction and rearm must
match native execution with accepted actions and pending work checked. Other
remaining MAIR gaps are world-derived watcher installation, receiver ports,
observer filtering, resource movement, reactive/BUD batching, interacting
circuits and player-facing activation.

## Historical event-engine result

The retired `event_netlist` suite last recorded 45 passed, 1 failed and 1
ignored. ADDER1BIT `prepared-0-1-0` had actual sum output 0 versus expected 1
at tick 3. COUNTER_BASIC remained ignored after a recorded piston divergence at
`(56,36,44)`. Static-piston and selected one-resource BUD adapter comparisons
passed. None of those results are current MAIR coverage or an active test suite.
The fixture manifests and their expected outputs and observation boundaries
remain the migration contract for future MAIR acceptance tests.

## Current verification

- `cargo test -p mchprs_core redpiler::sampled::tests --lib`: **28 passed, 0
  failed, 0 ignored**, 1,004 filtered out. This includes repeater delay 1–4
  boundaries, short-pulse replay, pending-tick deduplication, all three priority
  cases, loaded powered and locked state, repeated lever–repeater–lamp propagation
  and rearm, plus structural rejection. These mechanism tests use the sampled
  runtime directly and do not use the native interpreter to calculate results.
- `cargo test -p mchprs_core redpiler::scratch --lib`: **15 passed, 0 failed,
  0 ignored**, 1,017 filtered out.
- `cargo check -p mchprs_core --tests`: passed with five existing dead-code
  warning groups.
- `git diff --check`: passed.
- `cargo test -p mchprs_core
  redpiler::sampled::adder_tests::adder1bit_mair_manifest_arithmetic --lib`:
  **0 passed, 1 failed**. The active case `prepared-0-0-0` is declined at
  `(51,31,40)` because production world compilation still reports the repeater
  as an unsupported component. Its manifest replay and output assertions are not
  reached. This is an admission failure; ADDER1BIT remains incomplete.

The native lamp chain still uses isolated sampling, component-state and pending
tick self-checks, and host-preservation checks. The new repeater tests establish
only the manually assembled rear-input mechanism. They do not establish
world-derived repeater compilation, side-input discovery, changing lock state,
overlapping circuit admission, or ADDER1BIT execution. Full side-locking behavior
and piston execution remain pending.

See the [milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md) and
[architecture](REDPILER_ARCHITECTURE.md).
