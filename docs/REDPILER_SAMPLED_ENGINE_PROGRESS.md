# Sampled Redpiler implementation status

The world-derived compiler admits multiple levers, lamps, repeaters and static
redstone blocks, and retains native replay mismatches as diagnostics. A
lever–repeater–lamp circuit executes a scheduled repeater transition. Quiet
source probes preserve static redstone-block contributions; arrangement-aware
ordering is not yet implemented. ADDER1BIT compiles and replays its eight
manifest cases with piston and token motion, but its outputs are incorrect.
Runtime input and tick calls return errors, and compiled instructions and
scheduled events are limited to one million semantic operations per game tick.
Compiled pistons now have sampled inputs and explicit callback instructions;
the runtime counts accepted actions, token departures and landings.

The private MAIR runtime and its lever–dust–lamp acceptance gate are implemented.
Compile-time inventory and preliminary token discovery are connected, and a
world-derived empty piston extension executes. The player-facing backend remains
Graph. The old `event_netlist` compiler, executor and executor tests have been
removed; their results below are historical only.

Repeater runtime mechanism is implemented and tested with manually constructed
lever → repeater → lamp programs and world-derived circuits. Full side-locking
coverage remains pending, and ADDER1BIT remains
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
  deliveries. Production admission allows multiple floor/wall levers, dust
  wires, lamps, repeaters and decorative signs; watcher installation and
  connected piston execution remain pending.
- `Program::new` validates component records, external sources, rise/fall
  programs indexed by component, electrical links, watcher records, repeater
  records and instruction references before building incoming indexes. Partial
  instruction sequences remain valid.
- Rise/fall programs contain ordered `Contribution`, `RefreshWire`, watcher
  check and explicit lamp delivery instructions. Explicit `UpdateLamp` always
  dispatches. Watcher delivery uses phase-scoped `UpdateOnce`; contribution
  changes can invalidate that marker. Production compilation installs watched
  wires for nearby pistons and observers; ordering and the full watcher graph
  remain incomplete.
- The runtime uses its own scheduler and lamp tick dispatch. It drains the
  current bucket, advances once, then drains the new current bucket. The lamp
  off delay remains four game ticks for `schedule_tick(..., 2, ...)`.
- The runtime supports rear-input repeater scheduling for loaded repeater state,
  delays 1–4, tick priorities, duplicate suppression, short pulses, and compiled
  rise/fall output programs. Repeater output contributions initialize from the
  loaded powered state. Pending work can remain queued across external input
  transitions. World compilation discovers repeater side inputs; broader
  side-locking coverage remains pending.
- Compile-time piston inventory records stable IDs, facing, sticky state,
  home/far cells, loaded extension/head state and transitive shared-cell groups.
  It rejects inconsistent, moving and out-of-capture contacts. Preliminary
  token discovery distinguishes static far-cell scenery and checks group limits.
  Scenery is retained separately from tokens. Individual redstone-token source
  alternatives and basic departure/arrival callbacks are probed; joint
  arrangement links and MAIR's ordered movement programs are incomplete.
  Fresh-scratch extraction reproduces the reference's 12 source maps at
  (48,33,40) and (54,33,42), including the masked conditional source. A local
  local input-arrangement tables switch varying source identities; both
  compiled reference wires reduce to the expected two tokens and 12 states.
  Piston and lamp arrangements have also been added. The conditional stationary
  source at (54,34,42) feeding the piston at (56,32,41) matches the reference
  in its three token states.
  This does not yet implement MAIR's full per-dependency group growth.

## Earlier acceptance boundaries

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

## Verification before dependency-growth port

- `cargo test -p mchprs_core redpiler::sampled::tests --lib`: **31 passed, 0
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
  ADDER1BIT's eight cases ran and reported 8 output-window mismatches, all in
  `prepared-1-1-1`. The runtime counted 339 accepted piston actions, 302 token
  movements, 336 completions and 28 repeater ticks. Interrupted motion slots
  are not included in the completion counter.

The world-derived empty-piston test records one accepted action and completion;
the manually queued token test records one token departure and completion.
Retraction, observer handling and individual-token source probes are preliminary.
Full dependency-group growth, reference-ordered movement programs and complete
notification behavior remain absent. The manifest diagnostics show incorrect sum ones
and incorrect carry zeroes in several observation windows. COUNTER_BASIC's
saved-idle case fails compilation at (47, 38, 42), a redstone wall torch; it
has not executed.

Against the supplied MAIR replay, `prepared-0-1-0` matches the recorded visible
states. For `prepared-0-1-1` and `prepared-1-1-0`, the first visible difference
is response tick 10 at the carry repeater, outside its declared observation
window. For `prepared-1-1-1`, the first visible difference is response tick 1:
Rust retains the sum payload while MAIR reports air. The Rust trace shows the
piston at (56,32,41) receiving an unpowered retraction request after token 2
departs; live-power and event ordering around that request need comparison with
instrumented MAIR. The saved wrapper trace has no internal event sequence.

See the [milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md) and
[architecture](REDPILER_ARCHITECTURE.md).

## Full-port baseline recheck

Continuation: receiver strength and movement diagnostics account for all
arrangement groups at a target. `group_probe.rs` discovers touched wire
networks and nearby readers, compares source/port maps for single-token
alternatives, grows influencing-token sets through joint probes, then extracts
groups with the reference arrangement limit. Group inputs retain port and silent
classification; groups with the same token axes share a runtime arrangement.
The previous proximity Cartesian-product compiler loop has been removed.

The masked conductor test and a conditional repeater-side input test pass.
The existing ADDER arithmetic and joint-arrangement tests passed in a sampled
run after the GroupProbe change. COUNTER compiled and executed and its memory,
output dust and repeater bank matched the SHA-checked reference timeline for
all 24 saved-idle and 102 release ticks, including moving markers. Token
redstone sources now use the
reference's stable cell-to-virtual-constant index. Further equivalence work
remains for unsupported component and entry-state paths, overlapping-token
arrangements, and the remaining movement/piston ordering differences.

Reference mapping: `TouchedWires`/`CloseNetwork` and reader proximity are in
`group_probe::run`; `InputsOf` is `inputs_of`; `FindChangedLinks` is
`compiler::find_changed_links`; single and joint probe rounds and extraction
are in `group_probe::run`; `compile_movement_flips` combines equal token axes
and constructs runtime flips. The extractor covers the node families admitted
by this private engine, not all reference component roles.

The pinned counter timeline was copied unchanged from
`E:/codex-builds/luna-counter-test-investigation-20261030/counter-mair-timeline.csv`
into `test_data/instant-pistons-io/reference/`; its SHA-256 is
`929a9c6a49aaf381780a3177bca97264a135019588cfe5061d08c85c8bec5f37`.
The corresponding capture and decoder provenance are in that investigation's
`REPORT.md`.

Compiled movement now uses arrangement-indexed flip programs, stored conditional
link contributions, and ordered per-cell `MoveStep` records. Remaining recipients
retain flip-discovery order. Watched-wire output changes occur at their movement
steps, followed by remaining switched recipients and changed-wire notifications.
Runtime dependency-signature scans no longer make movement decisions (the helper
remains for trace diagnostics). Own groups precede shared groups. Piston flips
retain the reference's special non-delivery treatment. Added validation checks
group dimensions, source identities, movement references and token axes.

Source-order probes now capture immediate qualifying reactions, including the
observed rear/side input at callback time; they no longer require settling.
Ordinary output programs interleave per-target link application and direct
updates. Quiet probes retain active constant/token contributions. Conditional
inputs use stored contributions rather than reading live source outputs before
their ordered instruction. Source and piston-home watcher lists are compiled
from reference update-footprint rules, with shape/update filtering at delivery.
This is not complete notification parity: wire-watcher order, reactive-neighbor
inventory and native movement callback capture still differ from the reference.

Floor/wall torches now have support-input discovery, inversion, two-game-tick
scheduling and revalidation, output programs and watcher delivery. The world-derived
floor/wall torch test covers delay, repeated rearm and withdrawn-input rechecks.
Piston requests now retain extension, normal-retract and early-retract identities,
with per-kind deduplication. Landing-phase early classification and independent
motion-part completion remain incomplete.

Saved logs under `E:/codex-builds/`: `mair-movement-adder.log` preserves the first
movement run's eight ADDER mismatches; `mair-compiled-flips-sampled.log` records
33 passing mechanisms and the same eight mismatches. The later ordinary-source
and watcher changes record 34 passing mechanisms but **11 ADDER mismatches** in
`mair-watchers-sampled.log`, including additional window differences. This is a
known regression, not a parity success. `mair-torch-sampled.log` demonstrates that
COUNTER passes torch discovery but declines at (56,36,46) with too many local input
arrangements. Dependency growth remains required; raising that cap is not a fix.
The obsolete unsupported-torch rejection test was changed to an unsupported
comparator. Scratch tests passed (15); the Graph/native family diagnostic passed
(1 test covering its existing 13 samples), logged in `mair-torch-graph.log`.

Departure-record comparison against the pinned `mair-compiled-records.txt`:
Rust's token (53,32,41) program now contains (52,33,41), but places it after the
output piston (56,32,41); MAIR places it before. Token (54,33,41)'s departure
still lacks that recipient. These callback/order differences remain unresolved;
no internal MAIR runtime event sequence is inferred from these records.

Latest check: `mair-groupprobe-anchor.log` records **42 passing sampled tests**,
including ADDER and the counter reference timeline. `mair-groupprobe-scratch.log`
records 15 passing scratch tests; `mair-groupprobe-final-check.log` records a
passing `cargo check -p mchprs_core --tests`. `git diff --check` passes. The
full 18-sample matrix, comparator/reactive cases and supported pending/moving
entries have not been verified and remain required. No fixture, action, decoder,
expected output or observation-window changes were made.

After property-based passive geometry admission and its glass/sandstone movement
test, `mair-groupprobe-passive.log` records 43 passing sampled tests. Three
focused TEST samples (`TEST_PISTION_2`, `TEST_BUD_1`, `TEST_XOR`) also passed
saved-idle, rise/fall, and 1/2/3-tick pulse output comparisons in
`mair-focused-comparison.log`. Their unmodified reference replay is stored in
`test_data/instant-pistons-io/reference/test-mair-focused-replays.json`, SHA-256
`5829ce8d437d028c7ae93e53766912e50b20edc21ec82fcc64c7cfbb28f650a8`, copied
from the Luna investigation. `TEST_PISTION` is still excluded because comparator
and reactive semantics remain unimplemented; the other 14 samples have not
been compared by this focused test.

The final combined run `mair-groupprobe-final-suite.log` passed all 44 sampled
tests, including those focused comparisons; the corresponding `cargo check`
passed. A separate ADDER diagnostic run produced no reported differences on
the three selected observed ports where reference frames were available. This
does not compare every internal state or establish full MAIR compliance.

| Reference algorithm | Rust implementation | Behavior check | Unresolved gap |
| --- | --- | --- | --- |
| Extractor and CollectInputs | scratch source discovery and sampled input tables | scratch directional and dust tests | General ports, conditional paths and passive families incomplete |
| GroupProbe and ExtractGroups | `sampled/group_probe.rs` and compiled arrangement groups | masked conductor and ADDER joint tables | Side/control extraction and overlapping token identity need broader reference comparisons |
| SourceOrdering and PropagateChange | immediate qualified probes and interleaved target programs | ordered conditional-contribution test | Reactive branch, complete ports and watcher parity remain |
| MoveRecorder and SetTokenPos | compiled arrangement flips and ordered move steps | masking, discovery-order and rearm tests | Callback records and wire-watcher ordering differ |
| QueueEvent and motion completion | three event identities and combined motion records | per-kind queue identity and two-tick movement tests | Landing-phase classification, interruption and paired motion incomplete |
| DescribeNodes, Update and TickNode | sampled component handlers including torches | floor/wall torch delay, recheck and rearm test | Comparator and other reference roles incomplete |
| TrackBudded and entry imports | stationary-entry admission | pending-work rejection test | Reactive state and supported pending or moving entries absent |
