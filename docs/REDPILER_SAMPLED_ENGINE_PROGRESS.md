# Sampled Redpiler implementation status

The private MAIR runtime and its lever–dust–lamp acceptance gate are implemented.
Compile-time inventory of bounded empty piston actors is also implemented;
piston execution remains the next unmet gate. The player-facing backend remains
Graph. The old `event_netlist` compiler, executor and executor tests have been
removed; their results below are historical only.

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
- Production admission remains one straight lever/dust/lamp chain. The lamp
  dependency is still constructed specifically for that chain. Broader discovery
  fixtures do not establish multiple-source executable compilation, watcher
  ordering completeness or piston execution.
- `Program::new` validates component records, external sources, rise/fall
  programs, electrical links, watcher records and instruction references before
  building incoming indexes. Partial instruction sequences remain valid.
- Rise/fall programs contain ordered `Contribution`, `RefreshWire`, watcher
  check and explicit lamp delivery instructions. Explicit `UpdateLamp` always
  dispatches. Watcher delivery uses phase-scoped `UpdateOnce`; contribution
  changes can invalidate that marker. Production compilation still installs an
  empty watcher table.
- The runtime uses its own scheduler and lamp tick dispatch. It drains the
  current bucket, advances once, then drains the new current bucket. The lamp
  off delay remains four game ticks for `schedule_tick(..., 2, ...)`.
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

- `cargo test -p mchprs_core redpiler::sampled --lib`: 17 passed, 0 failed,
  0 ignored (1,003 filtered out). Includes explicit repeated delivery versus
  watcher suppression, discovered-link execution with a nonzero link index,
  native rise/fall self-validation, boundary rejection and host preservation.
- `cargo test -p mchprs_core redpiler::scratch --lib`: 15 passed, 0 failed,
  0 ignored (1,005 filtered out). New wire-only checks cover competing path
  minimum attenuation and directional strong conduction without pistons.
  Retained tests cover competing initially-off sources, distances 14/15,
  directional emitters, stepped dust and scratch isolation.

See the [milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md) and
[architecture](REDPILER_ARCHITECTURE.md).
