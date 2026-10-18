# Boundary-driven instant compilation

> Archive status, 2026-10-18: C/LLVM and finite/symbolic synthesis source has been removed.
> Recover the last combined source from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> Historical results and plans below remain preserved; see
> [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current behavior and restoration details.

> Archive status, 2026-10-18: C/LLVM and finite/symbolic synthesis source has been removed.
> Recover the last combined source from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> Historical results and plans below remain preserved; see
> [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current behavior and restoration details.

Status: stopped at the user's request on 2026-10-16 for a different experiment. This is an
incomplete compiler checkpoint, not a working whole-region replacement or a demonstrated
acceleration. Preserve the observations below when deciding what to reuse.

At the user's subsequent request, all three synthesis builders are disabled in the default
pipeline: `compile-symbolic-regions`, `compile-instant-machine`, and `compile-symbolic-kernels`.
They remain recorded as skipped even at O4/all tiers with stable inputs; custom developer
pipelines can explicitly select them. Existing O3 piston/reset/proxy optimizations remain.

Disablement validation passed: the default-pipeline regression across tiers/stable-input
settings, pass-order selection, and real ADDER1BIT JSON export (three focused debug tests).
`cargo check -p mchprs_core -p mroww --tests --benches --locked`, focused formatting checks,
and `git diff --check` passed. Compiler-specific tests explicitly select experimental
builders; the public benchmark instead checks that synthesis execution counters stay zero.
No full test suite or performance benchmark was rerun for this disablement.

All discovered edge cases and their evidence are maintained in the single register
[INSTANT_COMPILER_EDGE_CASES.md](INSTANT_COMPILER_EDGE_CASES.md).

## Stop checkpoint: 2026-10-16

The implementation snapshot is commit `ad1573f` (`Compiler`). The worker was stopped and
its long-running fixture test was interrupted. No more algorithm work or benchmarks were
authorized for this wrap-up; compilation verification was explicitly requested afterward.

### Present implementation

- The scoped event compiler in `compiler/event.rs` derives callback, actor-sample,
  action-acceptance, reset-tick and deferred-publication routines. It records ordered
  future effects and sparse owned state, including event-time views for outside callbacks.
- `redpiler_opt::Diagram` now supports a shared structural ITE DAG as well as canonical
  BDD construction. Reachable-input lowering removes unused runtime bindings. Bounded
  implication and local unit-propagation helpers prove some impossible paths; failure to
  prove means unknown. All 34 optimizer-library tests passed in both debug and release.
- Symbolic Turbo layer state coalesces rows with the same numeric layer. Wire-side reads
  use a legal three-state encoding. Specialized wire dispatch now checks regional ownership.
- Captured wire-power derivation now uses one canonical neighbor order. All four heading
  tables have identical horizontal/down/up neighbor groups; queue heading/order remains
  unchanged. A table-invariant and scalar comparison test was added but not executed in
  the final snapshot.
- The runtime evaluator and per-routine sparse revision-owner lists are drafted in
  `backend/graph/symbolic.rs`. **No callback/sample/accept/tick/publication dispatch calls
  `run_symbolic_region`. Its callback-effect branch is a no-op placeholder.** Do not hook
  this routine into execution without completing boundary delivery, captured operands,
  event-time overlays and reentrancy/admission handling.
- The benchmark reports full-episode execution cost as well as the historical active
  window, so untimed input preparation cannot masquerade as an optimization.

### Observed blocker

The first ADDER1BIT event entry is an uncaptured Update from Top at `SiteId(419)`,
receiver `Component(NodeId(89))`, in a region with 13 actors and nine reset nodes.
It still cannot produce an event Plan within 1,048,576 symbolic nodes.

After layer coalescing and local contradiction proofs, the failure moved from repeated
layer-row updates to `walk.visit-wire.power`, after 141,908 semantic steps. Compact wire
side encoding alone did not resolve it: the next run failed at `walk.identify.bias-write`
after 141,880 steps. Both reached 1,048,576 nodes / 1,048,575 charged logic operations.
These results precede the final heading-independent power change; it has not had a fixture
rerun. There is no successful new adder kernel, runtime-replacement result or speedup.

The remaining suspected costs are conditional walk/capture histories and the very broad
entry relation: every internal wire strength is arbitrary independent state at every
micro-event. Snapshot-variant coalescing and a local ready-state relation were discussed,
not implemented or proven. Neither increasing budgets nor falling back to general Graph
would establish the intended virtual-circuit compiler.

### Validation at shutdown

The parent reran these successfully after the final heading-power source edit:

```text
cargo check -p mchprs_core --tests --benches --locked
cargo check -p mroww --locked
cargo build -p mroww --locked
```

The complete server build succeeded, including code generation/linking, in 39.52 seconds;
its log is `target/instant-wrap-up-build.log`. These commands report dead-code warnings
for the unfinished runtime path and one unused import. No compile fix was needed.
The later validation sections in this document describe the **previous per-actor program
checkpoint**, not equivalence of this new event compiler. New full-transaction/all-history
adder tests remain acceptance gates; they do not currently pass the compilation gate.
No final full-suite run, new performance measurement, or broad fuzz campaign was completed.

## Goal

Build an AOT compiler that replaces an instant region with an equivalent optimized symbolic
virtual circuit and significantly accelerates execution. For ADDER1BIT and ADDER11BIT, almost
the entire internal computation should run through the virtual circuit instead of physical
graph propagation. Derive the circuit from graph semantics, never hand-written arithmetic,
global input-mask enumeration, runtime learning, or JIT.

Identify boundaries where instant logic meets ordinary devices through power/QC, Update/Shape,
and observable geometry dependencies. Any qualifying boundary event may activate the region;
no dedicated lever, common trigger, globally empty scheduler, or complete-device recurrence
is required. Keep normal components and BUD-switch memory in ordinary Graph execution: do not
classify logical BUD memory as an instant merely to include it in a kernel.

Stable external data during an admitted instant/reset transaction remains an explicit
assumption. The user additionally permits reordering **external callbacks within one logical
tick**. This does not permit dropping their payloads or multiplicity, moving them to another
tick, or reordering internal piston work in a way that changes circuit outputs or memory.

```text
boundary power/QC + incoming event + external normal/BUD memory
                    |
             compiled region
                    |
boundary outputs + same-tick effects + ordinary Graph memory/scheduled work
```

Replace covered internal wires, Turbo walks, reset computation and unnecessary notifications
in execution. Kernel phase/reset state and cold reconstruction data may remain where needed
for display, reset and interpreter handoff. Unrelated Graph devices, BUD memory and
scheduled ticks must continue normally. A private World or another physical simulator is not
an implementation of the virtual circuit.

The user additionally permits interpreter handoff to complete a few logical ticks after the
request. If necessary, finish the current compiled transaction and materialize at a certified
safe point. This relaxes immediate reconstruction of transient internal geometry, not output
timing, BUD memory or scheduled work. All ordinary Graph execution continues during the wait;
do not silently tick the region alone or jump the world clock. A deferred handoff needs an
explicit pending/completed lifecycle and a justified bound. The existing stronger immediate
handoff may remain where it is cheap; it is no longer a mandatory hot-path design constraint.

Success requires measured release acceleration, actual execution of compiled kernels and
substantial removal of internal work. Passing tests, recognizing actors, or translating
individual piston predicates without replacing the internal circuit is insufficient.

## Boundary contract

1. Classify connections and read dependencies. One ordinary component can supply an input port
   and receive an output port. Shared external fanout alone does not require shared ownership.
2. Keep power availability separate from activation. A QC data change can affect the next read
   while leaving the piston unchanged until a qualifying callback arrives.
3. Retain incoming/outgoing Update or Shape semantics: target, direction, guard, live/captured
   values, logical tick and meaningful duplicates. External same-tick ordering may differ under
   the explicit user assumption; required internal causal ordering remains. Unchanged power
   does not by itself justify deleting an update.
4. Account for external reads hidden in guards, contact variants, conducted power, wire shape
   and captured neighborhoods. An internal-to-internal edge can still depend on outside state.
5. Account for outside observations of owned geometry, including observers and Turbo wire
   readers. An unchanged final output does not make an intermediate observed change removable.
6. Close ownership over shared payloads and coupled piston footprints. Every mutable owner
   belongs to one execution state. Unrecognized mechanisms are reported as such.
7. Preserve reset deadlines, scheduled priority, required internal request causality, action
   revalidation and independently revision-checked payload publications. Keep ordinary/BUD
   memory outside instant ownership; evolving memory need not repeat its initial value.
8. External callback order within a tick may be canonicalized or batched. Preserve boundary
   values, occurrence semantics, logical deadlines and the supported memory behavior. This
   relaxation is not a certificate for unordered internal propagation; the stable-input adder
   counterexample still forbids changing internal request order when outputs change.
9. Unrelated devices and pending ticks continue normally. Regional execution must not replace
   the complete backend tick or require the entire circuit to be idle.
10. Unsupported dependencies decline explicitly. A general Graph fallback preserves behavior
    but does not count as successful regional compilation.

For the gate-extraction proposal clarified on 2026-10-18, one logical input edge supplies
one sampled input frame. Compiler traversal order is not circuit timing. Buffer/inhibition
dependencies can affect nanotick behavior and must be accounted for while deriving the
transfer function, even when runtime buffers and notifications are later eliminated.
An explicit causal relation or derived condition may replace those events; generic Turbo
replay is not a requirement. Atomic-edge agreement for the fixtures remains to be validated.

## Implementation checkpoints

1. Correct ownership/composition and derive a complete, deterministic, inspectable boundary.
   Validate dependencies and port ownership before lowering. Preserve JSON references and
   account for retained tables.
2. Compile and execute boundary-driven local event routines without a selected lever or a
   whole-selection periodic cursor. Reuse the existing ordered mutation/scheduler primitives.
   The existing single-actor compiler is a starting implementation, not completion of the
   multi-actor regional compiler.
3. Support multiple actors and shared memory in one regional program. Use shared structural
   Boolean operations for expressions and bounded proof helpers where needed. Derive functions
   from graph semantics; do not enumerate operand masks or insert fixture-specific arithmetic.
4. Contract certified internal wire/reset computation while retaining boundary observations,
   future work and reconstruction. Report actual removed execution work separately from
   compiled-plan counts.

## Acceptance evidence

- A fixture-derived mechanism is compiled and activated through ordinary redstone with no
  lever in the compiled selection; a compiled-routine counter proves the path actually ran.
- A separately scheduled device continues during regional execution, including imported work.
- Independent QC data and callback histories preserve BUD memory; multiple incoming ports and
  both directions of a component interface are covered.
- Guards, captured observations, duplicate callbacks and an external observer are exercised.
  An unsupported dependency has a tested, specific decline rather than a frozen value.
- A two-actor/shared-resource example demonstrates one owner and exact ordered revalidation.
- ADDER1BIT and then ADDER11BIT preserve output, owned memory, pending work, reset and native
  continuation. Divider coverage reports recognized, compiled and actually executed regions
  separately.
- Compare against general Graph and the interpreter. Functional success through fallback,
  graph-table reduction, or an unexecuted compiled artifact does not establish kernel success.
- Benchmark only after semantic execution checks pass. Separate preparation from runtime and
  include activation/effect costs in a transaction measurement.

### Work-replacement and measurement gate

`compiled_adder_regions_replace_internal_propagation_and_preserve_outputs` exercises a real
carry-producing transaction for each adder, compares projected state and future work with
the interpreter after each operation, checks arithmetic output windows and interpreter
continuation, and compares executed work with stable O3. Its initial work gate requires at
least 75% fewer Turbo walks and 50% fewer callbacks over preparation, activation and reset.
These are acceptance thresholds for the intended replacement, not measured results. A
per-actor predicate VM cannot pass by reporting more compiled steps while replaying all
internal propagation. Boundary-only work may remain; report it separately when judging the
remaining cost.

The benchmark keeps its original active-window `seconds` field for historical comparison.
Schema 6 additionally reports `preparation_execution_seconds`, `phase_stimulus_seconds` and
`episode_execution_seconds`, plus preparation work counters and episode medians. The episode
metric includes input/start operations, preparation ticks and the active ticks. It excludes
loading, compilation, display flush, observations, reset and interpreter continuation.
Report both time metrics: moving computation into an untimed activation is not a speedup.

External callback batching also needs a reentrancy check: an outside receiver may change
power and synchronously call back into the region before the original cascade finishes.
Either preserve that causal boundary with a continuation or decline the unsafe closure.
The allowed external ordering relaxation does not permit freezing an outside source or
substituting final owned state for an earlier captured observation.

### Event-program implementation contract

The existing Graph scheduler remains the sole clock. A callback, reset tick, piston request
acceptance or deferred publication selects a regional event routine. Compile the entire owned
propagation closure for that entry, reduce its symbolic expressions and retain reachable
reads, actual writes and boundary effects. No runtime Turbo walk is needed for the covered
closure. Do not replace the backend tick with a whole-selection cursor.

Emit future changes in one ordered stream: scheduled ticks, FIFO requests, publications,
accepted presentation actions and outside deliveries. Replay those changes incrementally
into the existing Graph future. An outside handler must see only the earlier emitted work;
it may append its own work normally. Pending membership and imported publication witnesses
are event inputs, not assumed-empty compiler queues.

An outside delivery also needs its event-time owned-state view. A repeater can observe a
transient high signal before the region finishes low, even without synchronous reentry.
A sparse temporary owned-state overlay may supply that view while the ordinary handler
runs, provided admission proves the handler cannot mutate/reenter owned execution. Restore
only the owned fields after delivery, retaining ordinary-component changes and newly queued
work. An overlay must not spuriously increment revisions or generate display changes.
Preserve actual captured operands separately from this live event-time view.

## Current baseline limitation

The existing AOT prefix/period compiler uses regions as a candidate hint and then traces the
complete selection from a lever edge. Its complete-state recurrence and global cursor are a
different proof/execution family. Preserve useful semantic tests during migration, but do not
extend those assumptions into the regional interface.

## Checkpoint 1 review evidence

Boundary derivation now lives in `analysis/instant_regions/boundary.rs`; graph data types retain
the compact references. The scanner uses dense ownership masks and memoized guard support.
Ordinary external component fanout no longer joins owners. Existing wire and passive-conductor
connectivity is retained.

Additional ports account for external guard uses (including uses on edges wholly outside the
region), dynamic wire neighborhoods, external wire readers, and callback target observations.
The exact original guard/route occurrence retains its view, direction and duplicate-row identity;
shared guard expressions are not copied into every port. Boundary validation checks reference
and ownership consistency. Export validation follows each encoding's own schema; the old finite
machine certificate has no required boundary field.

Before the fix, the independent two-adder discovery test observed two separate actor groups
becoming one after adding contributions from a common external source. The corrected regression
requires the original ownership partition to remain unchanged. Two further independent tests
cover an external-to-external power edge whose guard reads an owned actor, and an outside actor
guard that must not be classified as owned output state. All three pass in the checkpoint's
compiled debug test artifact (parent rerun: 3 passed, 0 failed).

Writer validation for checkpoint 1: core `cargo check --tests --locked` passed; the original
`boundary_contract_tests::` filter passed its three discovery tests; the existing two-adder
composition test passed and still asserts internal wire ownership; symbolic event export
validation passed. Those results established discovery and schema integration only. The grouped
runtime checkpoint below adds execution evidence. No benchmark ran.

## Checkpoint 2: grouped boundary event programs

O4 now builds one region program containing the shared boundary, every supported piston sample
and accept routine, and each unique reset update/tick routine. The graph plan binds the program to
its source region index. Lowering drops cold draft-indexed boundary metadata and keeps direct
actor/reset-node runtime bindings plus copied power links. This avoids stale table indices after
power sorting and proxy route lowering. The builder does not require a selected lever, complete
world-state recurrence, or globally empty pending work; unrelated Graph dispatch continues.

Boundary guard-use derivation now also records comparator far-override guards, which can read an
owned moving contact even when no power edge references that guard. The boundary JSON validator
checks this plan reference and its guard. Boundary inputs preserve outside values, while output
reads distinguish region-owned state from external actors. Callback occurrence metadata keeps
the original route and ordinal; runtime delivery remains at Graph's existing synchronous
dispatch point so it retains receiver, capture and reentry semantics.

Focused debug validation on the current source passed:

- `cargo test -p mchprs_core --lib boundary_contract_tests:: --locked`: 6 passed, including
  independent outside fanout, mixed guard dependencies, comparator geometry reads, and the BUD
  test that changes memory only after the independent qualifying update.
- `cargo test -p mchprs_core --lib symbolic_kernel_tests:: --locked`: 6 passed, including ordinary
  external component activation with no selected lever and continued dispatch of an unrelated
  pending device.
- `cargo test -p mchprs_core --lib stable_region_tests:: --locked`: 8 passed. Both ADDER1BIT and
  ADDER11BIT tests require complete actor/reset program coverage and actual positive regional
  execution, compare local state/future work against stable general Graph, and check native
  execution/interpreter handoff.
- Grouped region export, malformed reset-facing export, and O4 pass-name/tier checks each passed.
- The legacy whole-selection AOT projection tests explicitly disable the regional compiler and
  passed 3/3, retaining cursor, projection, revision-origin, clock-epoch and interpreter-handoff
  assertions (`cargo test -p mchprs_core --lib kernel_projection_tests:: --locked --
  --test-threads=1`).

These programs compile and execute local event predicates; they do not contract ordinary internal
wire propagation or callback delivery. No Divider kernel, whole-device acceleration, or wire-work
reduction is established.

## Parent integration validation

The final release Redpiler suite passed **395 tests, with 10 ignored and no failures**:

```text
cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1
```

The log is `target/boundary-regional-release-suite.log`; execution took 270.69 seconds.
The earlier broad debug run passed 392 tests and exposed three legacy projection tests that
still assumed the old default executor. Their setup now explicitly selects that AOT family;
all three passed a focused debug rerun, and the final release run includes the corrections.
The whole debug suite was not repeated after this test-only migration.

Core test/bench compilation (`cargo check -p mchprs_core --tests --benches --locked`), server
compilation (`cargo check -p mroww --locked`), focused formatting checks, and `git diff --check`
also passed. Full workspace tests and broad fuzz campaigns were not run. Performance is being
measured separately; compilation coverage and passing equivalence tests are not speedup evidence.
