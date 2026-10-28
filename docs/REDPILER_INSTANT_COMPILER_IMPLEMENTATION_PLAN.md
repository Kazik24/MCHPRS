# Redpiler instant compiler replacement plan

**Later scope decision:** the user accepts output-state equivalence at settled
frame boundaries, frame-boundary input changes and settled-state handoff for
the proposed fast backend. Intermediate output states may differ. Native
neighbors and command effects still require a correct boundary; unsupported
regions fall back to Graph. The stricter per-tick and arbitrary-handoff goals
below describe the earlier plan and remain existing regression coverage, but
are not mandatory for a new backend under the revised contract. No such FPU
backend has been admitted or validated yet.

Status: implementation plan; no replacement has been accepted. The plan is for
one general compiler and runtime contract over the immutable physical graph. It
does not authorize layout edits, fixture-specific compiler rules, or loosening
existing ownership checks to force admission.

Implementation is assigned to Luna6 High, with root reviewing each stage's
design, changes and measured exit gates. Preserve unrelated uncommitted work;
limit edits and deletions to the audited compiler scope. The unvalidated shared
transition prototype has been archived outside the source tree. It is not the
implementation baseline.

## Required outcome

The replacement must execute supported regions while `GraphBackend` remains the
authority for clock scheduling, piston request FIFO and source, actor/resource
state, unsupported callbacks, commands, reset, and future work. A compiled
region is correct only if its state and effects match native execution at every
observable cut and its materialized state can continue natively at any tick
offset.

Acceptance targets are measured on the actual fixtures and protocols:

| Fixture | Correctness gate | Performance gate |
| --- | --- | --- |
| `counter_basic` | Per-tick state, outputs, reset/restart, queue/future, and native handoff | At least 1,000,000 game ticks/s on the actual counter fixture |
| `adder_1bit`, `adder_11bits` | Native trace and physical state; resolve the known ordered-handoff regression | Measure only after both correctness gates pass |
| Saved FPU plus neighboring Divider | Changing declared inputs; indicators/readouts and command history; actor/resource state; all six input-frame handoff offsets and native continuation | At least 10,000 game ticks/s; 40,000 is the stretch target |

Throughput means simulated game ticks divided by the fixture's measured replay
time, in the same release executable and input protocol as its native oracle.
Use repeated runs and report the median plus the exact revision, compiler
options, fixture hash, frames, ticks, callbacks, actor actions, compiled-region
entries, and materializations. Compilation time is reported separately.

Historical numbers are not baselines until reproduced at the implementation
revision. In particular, the old 4.7M counter and 43–48k standalone FPU results
are hypotheses to recheck. The saved FPU's current fallback is documented at
about 274 TPS with 9,838 event actors, zero PullWave actors, and 13.36M callbacks;
the later bounded route experiment measured about 373 TPS with 79 bypassed wire
walks. Neither is useful acceleration. Installed kernels, region counts, and
expression counts are diagnostics, not success metrics.

On 27 October, the pre-replacement counter baseline was reproduced for 1,000,002
ticks against an interpreted progression: three runs had wall throughputs of
3.96M, 3.52M and 3.65M ticks/s, each with 166,667 logical evaluations. The
median wall throughput was 3.65M ticks/s. This is the existing executor, and its
sampled progression check supplements rather than replaces the state and
handoff tests. Reports are `target/build-audit-tps/counter-native-1m.json` and
`counter-o6-1m.json`; the build used Rust 1.98.1, release LTO disabled and
incremental compilation disabled.

After removing the experimental single-cell firing runtime, the existing O6
counter device still ran: three 1,000,002-game-tick samples measured **5.38M,
5.48M, and 5.53M wall game TPS** (median **5.48M**). Each sample admitted and
executed the logical device, with 166,667 logical evaluations in its measured
window. The benchmark compared its sampled progression with the frozen native
reference. The separate `o6_counter_output_matches_native_each_frame_through_control_changes`
test compares every output repeater at settled publication boundaries through
starts, stops, and the 255-to-256 carry. Intermediate output values can differ
under the subsequently agreed contract; this is not a full per-tick equivalence
claim. System activity during the samples was not controlled.

Reproduce with Rust 1.98.1, release LTO and incremental compilation disabled:

```powershell
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --tier 6 --ticks 1000002 --iterations 3 --reference target/build-audit-tps/counter-native-1m.json --output target/build-audit-tps/counter-simplification-1m.json
```

The tested executable SHA-256 was
`72322D42C7D2C524D6582DECBF8B9C022E3DC16B77BD35D7460C160F865AFACA`;
the frozen reference SHA-256 was
`0A128ADA3869CEEF97F94EBF62214FEFBB29C11EE2372ABB52A63868E6DC2CBA`.
This measurement predates deletion of unreachable one-shot compiler code; the
counter differential test passed after that deletion, but timing was not rerun.

## Replacement contract

Keep the physical circuit immutable during compilation and retain the existing
Graph implementation as the exact fallback. Replace the competing settled
frame, logical-island, and wave-ownership policies with a single execution
region model. A region program has typed inputs, outputs, ordered effects and
exclusive mutable-state ownership:

* **Inputs** are read-only and are sampled at a named source occurrence and
  phase. Each input is explicitly either fresh at every cut or latched from the
  exact native capture that created the queued callback/request. Reading an
  actor or wire never transfers ownership of it.
* **Outputs** are typed value/event ports with an exact native publication cut
  and consumers. A value output does not grant the region authority to mutate
  the consumer's actor or resource; every resulting callback remains an
  ordered effect or a native boundary.
* **Owned state** names exact mutable actor, component, and shared-resource
  fields. Every field has one executor. Its transition contract covers native
  selection/guards, acceptance, ordered writes, reset and deferred state. All
  actors that can mutate a shared piston lane/resource belong to one atomic
  ownership group.
* **Effects** are ordered occurrences, not a final-state diff: source route and
  occurrence, captured receiver/site data, callback kind, queue insertion and
  request source, scheduled work, command output, and nested callbacks. Commit
  effects in native order, publishing writes before any callback that can
  observe them.

The runtime invokes the region only at a proved native event/acceptance cut. It
reads live inputs from authoritative Graph state there, applies its transition,
and returns to Graph at every unowned callback/effect. After that effect it may
resume only a continuation proved for the new state and cut. Otherwise Graph
executes the remaining callbacks in original order. An unexpected FIFO pop,
source, acceptance, actor availability, resource/lane state, capture, or guard
invalidates the predicted suffix before it performs another effect. Requests
already enqueued remain in Graph's real FIFO. At reset and handoff, the actual
actor/resource fields and scheduler/FIFO future must be materialized exactly;
display-equivalent output is insufficient.

Divide the algorithm into independently testable compiler passes: classify
instant capabilities; discover typed fans and dependencies; grow candidate
regions with the worklist; certify state ownership and ordered effects; then
lower certified regions. Each pass consumes and produces an inspectable result
with stable source identities and explicit decline reasons. Keep each pass's
invariants and focused tests alongside it, and reuse the existing boundary
catalog and graph representation. Runtime execution consumes the certified
result; it does not repeat discovery or certification. Each boundary must be
inspectable by region and source occurrence: show its typed live or captured
inputs, owned fields, outgoing effects and the first declined or invalidated
obligation. Runtime diagnostics identify the native cut and fallback reason,
with counters for actual entries and fallback; avoid a separate diagnostic
model or logging on every hot-path event. Tests should report the first
divergent cut and effect so a failed proof or replay can be traced directly.

### Region discovery and stopping rule

Classify graph elements by instant capability before admitting them: pure
value, wire/event transition, actor transition, resource arbitration, command
or other observable effect, or unsupported/native-only. Start the directed
worklist from declared input ports, scheduled clock ticks, event/source
callbacks, stateful boundaries and observable effect sinks. Traverse read
dependencies, callback receivers, effect dependencies, and state transitions.
Classify each encountered edge by its semantics. Membership depends on supported
operations and source-derived proofs over legal inputs, not saved power values,
coordinates, sampled histories or successful benchmark vectors. Runtime input
changes do not rebuild the partition. A native input is a valid represented
input; growth does not require its producer to become compiled first.

1. Add pure expressions as read dependencies, without acquiring their source
   state.
2. Add an actor only when a complete local transition contract exists for the
   relevant phase, including captures, guards, acceptance, writes and effects.
   If the contract is incomplete, retain that transition on Graph and cut there.
3. Group all actors that can write a shared resource into one atomic ownership
   group; keep outside readers as read ports. Do not allow two regions to race
   over the resource.
4. Keep independent clocks independently scheduled. Represent feedback as
   explicit state transitions/SCCs with native update order; do not merge a
   whole connected or read/callback envelope merely because it has a cycle.
5. Join regions only when the exact interface between them can be internalized:
   same source/capture/order semantics, a single owner for every mutable field,
   and a continuation valid at the join. Otherwise leave the interface native.

Continue until no eligible transition can be added. This monotone fixed point
is computed once: a decline does not repartition/recompile the whole plot using
progressively weaker checks. Unsupported transitions stay native; other closed
regions may still run compiled.

Index directed value, effect and callback edges once and propagate only newly
learned port/capability facts. Track readiness incrementally rather than rescanning
all fan-in every time a neighbor changes. Discovery is linear in nodes, edges
and emitted ports, with the existing near-linear union operation for safe joins.
Expression construction and semantic certification have separate bounded work
and memory budgets; they are not assumed to be linear. Cancellation stops the
build; a local proof-budget exhaustion retains that cut on Graph. A failed
candidate does not rescan or regenerate the full graph.

### Cycles, event captures and phase correctness

Every feedback cycle must identify its state-bearing fields and the native
phase at which they update. A cycle with no identified state transition is not
compiled as a combinational shortcut. Captured callback data is distinct from
current live state: queued callbacks use their recorded capture, while fresh
queries at action acceptance sample current Graph state. The implementation
must never evaluate a wave-entry expression against a post-effect state as a
substitute for rebuilding a residual continuation.

The first runtime integration should use the ordinary `tick_phases` ordering in
`backend/graph.rs`: drain due callbacks, advance the scheduler, drain due
callbacks again, run the pre-piston hook, drain actual FIFO actions, publish
deferred connections, and return to `BetweenTicks`. Input-triggered piston
sampling retains its original callback occurrence. If the existing
logical-device tick path cannot preserve this exact sequence and native future,
it must be retired in favor of the ordinary Graph scheduler rather than
duplicated as a second clock authority.

## Files and ownership of the change

Primary runtime seam:

* `crates/core/src/redpiler/backend/graph.rs`: `tick_with_host`,
  `tick_phases_with_hook`, `deliver_route`, and `deliver_route_inner`. Add the
  unified region dispatch at native cuts while preserving ordinary Graph
  fallback and exact nested callback order.
* `crates/core/src/redpiler/backend/graph/pistons.rs`:
  `sample_piston_inner`, `drain_piston_actions`, and `accept_action_using`.
  Keep sampling, the actual FIFO pop/source, and native acceptance authoritative;
  a region may replace work only when its prediction matches that live cut.
* `crates/core/src/redpiler/backend/graph/projection.rs`: use the current state
  and future projection as the handoff contract; extend only where a newly
  owned field is absent. Do not reconstruct physical history from final wire
  power.
* `crates/core/src/redpiler/backend/graph/instant_kernel.rs`: first migration
  point for a cut-specific residual/region continuation. Current PullWave
  continuations use wave-entry expression roots; do not broaden their ownership
  or resume a frozen suffix after foreign effects.

Compiler seam:

* `crates/core/src/redpiler/analysis/instant/layers/`: retain pure extraction
  and `reduction.rs::reduce`/`reduce_roots`; replace the competing ownership
  construction in `composition.rs`, `kernels.rs`,
  `logical/islands.rs::partition`, and
  `logical/settled.rs::compile_scoped_devices_with_plan` with one source-derived
  region plan and typed interfaces. Do not add a second whole-graph partition
  layer: build on `program_graph/instant_boundaries.rs::BoundaryCatalog`,
  `analysis/instant/layers/partition.rs::partition`, and
  `analysis/instant/regions.rs::partition_with_cuts` and
  `analysis/instant/boundaries.rs::scan_boundaries`. The current layer
  partition's one-wire/cell-specific `computation_dependency` admission policy
  is the narrow candidate policy to generalize; retain its typed directed port
  rows and single catalog scan. Separate read footprints from effect/write
  footprints so a live read can cross a native ownership edge without claiming
  its state, while a callback/write still requires an exact owner/effect proof.
  Build adjacency indexes once and visit only newly exposed edges; never
  repartition/recompile the whole graph after a failed candidate.
* `crates/core/src/redpiler/program_graph/`: reuse the physical program,
  boundary/cell discovery, and native state/future representations where they
  carry exact semantics (`instant_regions.rs`, `instant_boundaries.rs`,
  `instant_cells.rs`, `settled_circuit.rs`,
  `settled_machine.rs`, `logical_bud_events.rs`). Inspect the in-progress
  shared-transition/reducer design as an optional implementation choice if it
  reduces duplicated validation or runtime work; no prototype API is active or
  assumed to exist. Any such design must pass the same cut and performance
  gates before adoption.
* `crates/core/src/redpiler/analysis/instant/layers/logical/settled.rs` and
  `backend/graph/logical_device.rs` are the old settled-frame compiler/runtime.
  Keep them until each caller has migrated and parity passes, then delete their
  alternate tick/ownership path. Likewise retire `logical/islands.rs` ownership
  partition and PullWave-specific `Composition`/`KernelProgram::PullWave` code
  only after the unified region path covers their valid cases. Preserve reusable
  pure reducers, source-route/capture data, local cell contracts, and Graph
  fallback. Remove failed architecture instead of leaving an untested second
  owner model alive.
* `crates/core/benches/instant.rs` and its `support/` modules: add a release,
  non-test saved-plot replay entry using the same snapshot and input protocol as
  the oracle. Share the fixture protocol rather than maintaining two histories.
  Keep coordinate/manifest interpretation in the fixture harness. The runtime
  compiler receives ordinary graph ports and events.

Tests and fixtures to extend (no coordinates or circuit names in compiler
selection logic):

* `backend/graph/tests/logical_device_counter.rs`, especially
  `o6_counter_matches_native_at_stable_storage_and_publication_cuts`,
  off-phase/reset cases and pending-import fallback. Its physical fixture is
  `test_data/instant-pistons-io/fixtures/counter_basic.json`.
* `backend/graph/tests/logical_device_islands.rs` for independently clocked
  counters and shared-resource separation.
* `backend/graph/tests/adder.rs` and its subtests/fixture manifests for both
  `adder_1bit` and `adder_11bits`; preserve the ordered source/FIFO handoff
  regression.
* `backend/graph/tests/instant_layer_entry/region_boundary.rs` for the six
  handoff offsets, native continuation, physical state and nonzero command
  effects.
* `backend/graph/tests/logical_device_saved_plot.rs` for the pinned scale-5
  saved FPU + Divider protocol, manifest-defined input positions, changing
  controls, command witness, output mapping, all six handoffs and native
  continuation. Keep this as a fixture-driven oracle, not a compiler special
  case.

## Implementation stages and gates

### 0. Establish reproducible baselines

Before implementation changes, record `git rev-parse HEAD`, working-tree diff,
fixture hashes, `rustc -Vv`, release options, and existing artifacts. Run the
counter and both adder native-oracle tests. Re-run the counter TPS benchmark and
the saved-FPU replay on that exact source/options before using old numbers.
For FPU, use the production snapshot's `PLOT_SCALE=5`, release LTO disabled,
incremental disabled; restore the source constant after pinning the executable.
Use the existing documented profile protocol (`MCHPRS_PROFILE_FRAMES=192`,
one test thread) and save output separately from source. Derive input words from
the manifest's declared bit roles, then verify each role-to-position mapping
against the standalone witness before changing the saved profile protocol. In
particular, the first 16-frame saved-plot warmup packed the opcode ports in the
wrong order: it exercised selector 110 instead of the known selector 011
command witness. Its zero command history is therefore not evidence about plot
health. Compare the exact saved entry/reset and native command history against
the standalone/fixed-schematic witness without changing the saved layout, and
track command entities and exact output history separately during warmup and
measurement. The corrected selector-011 saved-plot witness is now confirmed:
24 warmup ticks emit eight tellraw outputs and update two command entities.
The 16-frame changing-input oracle then passes every-tick comparisons, all six
handoffs and native continuation, retaining those eight outputs at every phase.
See `target/build-audit-tps/prod/fpu-command-selector3-profile-16.json` and its log;
the pinned executable is `fpu-command-selector3-scale5.exe`, SHA-256
`EBE3207D397AC2F999463477BD61774E2650906C5F0C5CD1ADDA8CC0E4A83924`.
There is no demonstrated physical FPU defect from these checks; arithmetic
correctness remains outside this native-equivalence result.

**Exit:** exact protocol and expected native state/output history are
reproducible; historical speed claims are either reproduced or explicitly
discarded; the port mapping and native command witness are validated. Preserve
the existing Counter path until Stage 0 proves its correctness/performance
status and the replacement has passed the Counter gate; do not trade a
known-good fast path for an unmeasured candidate.

### 1. Introduce the single region plan and native-cut oracle

Implement the typed input/owned-state/effect interface and one immutable
region-discovery worklist. Initially execute only a tiny region whose full actor
transition/effect semantics already have a native reference. Emit a per-cut
trace of sampled input identity/value, owner fields, ordered writes/effects,
captured source, and fallback decision. Compare the trace's architectural state
and effect history against Graph at each cut. Do not optimize a final readout or
bulk-publish outputs yet.

Add negative tests proving a read does not claim ownership, a foreign callback
invalidates/resumes only a valid suffix, a second writer prevents exclusive
ownership, independent clocks remain independent, and state at a cut is
materializable. Exercise duplicate routes and same-actor requests with distinct
source routes; route deduplication is allowed only when native queue semantics
prove it equivalent.

**Exit:** per-cut differential tests pass through callbacks, nested callbacks,
queue insertion/pop, resource acceptance, reset, all six handoff offsets and
native continuation. The existing production path remains in use until an
actually useful replacement passes its gate; exercise initial region programs
explicitly in differential tests without adding a production feature flag.

### 2. Migrate Counter without losing its valid speed

Run the COUNTER fixture through the unified owner and transition contract. The
region must cover the actual mutable memory actors/resources and their timed
publication; read inputs stay live and Graph handles unrelated scheduler work.
Compare every tick, including input toggles, reset/restart, a pending imported
tick, pending piston requests, physical actor state, and future after handoff.
Measure old and new paths under the same fixture and options. Enable the new path
only when it is at least 1M game TPS and remains equivalent. Keep the old known
fast path as fallback until this gate is met.

**Exit:** all Counter oracles pass on the unified path and >=1M TPS is reproduced
in repeated runs; benchmark records executable/hash/options and actual region
entries. Otherwise retain old execution and narrow the next experiment.

### 3. Add reusable physical transitions and effect contracts

Add local contracts for wire state/power, piston sample and action acceptance,
resource/lane arbitration, observer pulse/reset behavior, and command effects
only as each needs exact phase/capture/order. Begin with small physical fixtures
from `instant_cells.rs`, `instant_events.rs`, `guarded_transport.rs`, and
`ordered_wire.rs`. Add a shared-resource group fixture and separate-clock fixture
before composing them. Validate physical block/entity state, scheduled callbacks,
real FIFO source, and outputs at every native cut. An unsupported actor or
effect remains a Graph cut; supported neighboring regions may run around it.

**Exit:** one atomic resource has one owner, independent clocks stay separate,
and composing two regions preserves each isolated trace and their joint native
trace. No region-wide retry or fixture coordinates are used.

Before widening further, profile executed work on the actual saved plot. Show
that the new interior replaces a substantial part of its private propagation;
79 skipped walks among 13.36M callbacks is a failed performance experiment.
Record the first expensive blocked boundary with its source dependency and
missing contract. Do not spend another stage on predicate-only micro-optimizations
while the surrounding private callback/reset workload remains intact.

### 4. Validate and enable adders

Run the exact 1-bit and 11-bit fixtures and inputs against native on every tick,
including current known ordered-handoff regression, state materialization,
scheduled future and native continuation. The 11-bit case must exercise carry
and changing operands, not only a stable output. Record command/effect coverage
if the fixtures emit commands. Do not mark an adder working from settled output
agreement alone.

**Exit:** both fixtures pass all native state/effect/FIFO/future checks on the
region executor. If not, keep them on Graph and fix the exact cut/state contract
before performance tuning.

### 5. Compose the saved FPU and Divider

Compile the saved plot from manifest-derived physical ports and the reusable
contracts. Use the recorded changing-input sequence plus a native-confirmed
deterministic command witness, and compare command text/source/tick history, all
35 indicators, readout mapping, Divider outputs, physical actor/resource state,
and scheduled work. Run frame-aligned and all five non-aligned handoff offsets,
then continue in native Graph. Verify neighboring control inputs and unsupported
callbacks remain live. Measure the complete saved profile, including the actual
Graph/region runtime, not a standalone schematic or analysis-only evaluator.
For the throughput gate, drive all declared operand and control inputs with a
reproducible pseudorandom sequence under load at the input-frame rate; record
the seed, distribution and input-change frequency. Include stimulus, required
observations and neighboring Divider activity in the timed replay. A static or
idle-input result cannot satisfy the FPU performance target.
Compare command history against the confirmed native selector-011 witness at
the same cuts and require nonzero command output and entity execution. A zero
history cannot satisfy this gate.

**Exit:** native parity at every tick/cut and all six handoff continuations, with
command history parity against the native run with nonzero witness execution,
plus >=10k TPS on the same changing
input profile. Report distance to the 40k stretch target. If only portions run,
report exact region entries and work eliminated; do not label the profile
accelerated from installed-plan counts.

### 6. Retire competing execution owners

After Counter, adder, physical-event and FPU callers have migrated, remove the
alternate settled-frame tick loop and its ownership adapters, the undirected
single-clock island admission model, and only those stale wave-entry PullWave
ownership continuations that the new path replaces. Keep the already validated
route-boundary handoff behavior and its six-offset regression until equivalent
new coverage passes; correctness of that narrow slice does not establish useful
bulk acceleration. Delete dead adapters, configuration, tests, and metrics tied
only to retired paths. Retain Graph fallback, pure reduction, source/capture and
boundary metadata, local native parity fixtures, and the region's explicit
unsupported path. Run the full Redpiler test suite, production release build,
and all four performance/correctness suites after deletion.

**Exit:** no production path can create two executors for the same mutable field;
all previously supported valid fast Counter behavior is retained or improved;
unsupported circuits execute through Graph.

## Verification commands and reporting

Use the existing Rust test filters and benchmark harness (confirm exact test
names at the pinned revision). Run the tests from the repository root:

```powershell
cargo test -p mchprs_core --locked redpiler::backend::graph::logical_device_counter_tests -- --nocapture
cargo test -p mchprs_core --locked redpiler::backend::graph::adder_tests -- --nocapture
cargo test -p mchprs_core --locked redpiler:: -- --nocapture
```

From `crates/core`, record a frozen interpreted counter progression, then run
the same protocol through O6. The harness requires a tick count divisible by
six; 1,000,002 ticks also exercises multiple 16-bit wraps:

```powershell
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --interpreted --ticks 1000002 --iterations 1 --output target/build-audit-tps/counter-native-1m.json
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --tier 6 --ticks 1000002 --iterations 3 --reference target/build-audit-tps/counter-native-1m.json --output target/build-audit-tps/counter-o6-1m.json
```

This benchmark checks sampled `word_progression` under its stopped preparation
and running warmup protocol. It does not replace the per-tick state/future and
handoff oracles above.

The existing adder benchmark can reproduce the current gap:

```powershell
cargo bench -p mchprs_core --locked --bench instant -- --graph-short --all-devices --component io/adder_11bits --graph-tier 6 --stable-instant-inputs --iterations 3 --output target/build-audit-tps/adder-current.json
```

`--graph-short` is diagnostic: it logs some active-state checkpoint differences
instead of failing them and does not cover all six handoff phases. Its TPS is
not accepted until a strict 11-bit oracle asserts full physical state, ordered
future and native continuation, including the known 1+0 regression.

For saved FPU, follow the checked-in ignored test and pinned fixture instructions
in `logical_device_saved_plot.rs` and
`REDPILER_REGION_ARCHITECTURE_FAILURE.md`; run sequentially with
`MCHPRS_PROFILE_FRAMES=192`, `--ignored --exact --test-threads=1`, and capture
the JSON report. The test must fail if the built snapshot scale/hash or manifest
ports do not match. Run correctness first; do not derive TPS from test wall time
or the instrumented trace build. The production replay benchmark must use the
same changing inputs, clock protocol, commands, observations and required
projection work, with the native oracle outside its timed interval. Record at
least three quiet samples and report all samples plus their median.

The budgets are 1 microsecond per counter tick, 100 microseconds per FPU tick at
10k TPS, and 25 microseconds at 40k TPS. Measure mandatory native boundary and
projection costs as well as private execution; do not change the timing scope
to reach the target. Record at minimum:

* native-vs-region mismatches per tick for outputs, physical state, FIFO/source,
  scheduler/future and command text/timestamp;
* successful region entries, fallbacks by reason, live vs latched input reads,
  actors/resources owned, callbacks/wire walks/action work actually eliminated;
* commands executed during both warmup and measured window, separately;
* elapsed replay ticks and median TPS, separated from compilation time.

Any correctness mismatch, stale cut, missing command coverage, or handoff
mismatch is a no-go regardless of TPS. Any speed result below a fixture's gate
keeps that fixture on its prior exact path and triggers focused profiling before
more compiler scope is added.

## Main risks and decisions

* **State authority is the hard problem.** A correct equation is not a complete
  actor transition. Do not admit a piston/resource until selection, acceptance,
  update order, deferred work and reset are specified.
* **Residuals must be cut-specific.** After a foreign callback, either derive a
  residual against current authoritative state or resume native callbacks. Never
  reuse wave-entry roots as post-callback values.
* **Callback identity and captures are observable.** Preserve exact original
  `RouteId`, occurrence, source request and captured-site snapshot. Any deduping
  requires a proof against actual FIFO behavior.
* **A complete fallback may be correct but slow.** Measure executed work and
  actual TPS; shrinking code, expressions or callbacks on paper is not evidence
  of saved-plot acceleration.
* **Fixture protocol can hide behavior.** The FPU's changing operands, 35 control
  positions, readout descriptor mapping, command output and handoff phases must
  be independently checked against the native run before blaming the compiler
  or the circuit.
* **Compilation cost may dominate iteration.** Record pass time separately and
  profile only after runtime correctness; do not weaken proofs or add global
  retry loops to improve admission counts.

The plan intentionally favors a small exact execution region with native cuts
over claiming one broad settled frame. Region breadth may grow only when a
complete, testable transition contract makes the larger region independently
sound and measurably useful.
