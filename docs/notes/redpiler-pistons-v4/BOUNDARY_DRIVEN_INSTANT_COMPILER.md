# Boundary-driven instant compilation

Status: grouped regional event programs are implemented for supported regions; full internal
wire contraction is still incomplete. This document records the contract, current evidence and
remaining work rather than claiming completion of the full virtual-circuit target.

## Goal

Compile an owned instant region through its actual connections to the surrounding circuit.
No dedicated lever, common trigger, globally empty scheduler, or repeated complete device
state is required. Stable external data during an admitted instant/reset transaction remains
an explicit supported assumption. Callback order remains observable.

```text
boundary power + incoming event + owned memory
                    |
             compiled region
                    |
boundary power + ordered effects + next memory + scheduled work
```

The parser, graph, CircuitState and GraphFuture remain the existing owners. A compiled region
does not acquire a private World, another scheduler, or a second payload state.

## Boundary contract

1. Classify connections and read dependencies. One ordinary component can supply an input port
   and receive an output port. Shared external fanout alone does not require shared ownership.
2. Keep power availability separate from activation. A QC data change can affect the next read
   while leaving the piston unchanged until a qualifying callback arrives.
3. Retain each incoming/outgoing Update or Shape occurrence, including target, direction,
   guard, live/captured view, order and meaningful duplicates. A callback with unchanged power
   can still matter.
4. Account for external reads hidden in guards, contact variants, conducted power, wire shape
   and captured neighborhoods. An internal-to-internal edge can still depend on outside state.
5. Account for outside observations of owned geometry, including observers and Turbo wire
   readers. An unchanged final output does not make an intermediate observed change removable.
6. Close ownership over shared payloads and coupled piston footprints. Every mutable owner
   belongs to one execution state. Unrecognized mechanisms are reported as such.
7. Preserve reset deadlines, priority, FIFO request order, action revalidation and independently
   revision-checked payload publications. Evolving memory is an output of a transition; it does
   not have to repeat its original value.
8. Deliver effects at their original causal positions. If an outside callback immediately
   reenters the region, its reads must see the correct preceding writes. End-of-routine batching
   needs an independent proof.
9. Unrelated devices and pending ticks continue normally. Regional execution must not replace
   the complete backend tick or require the entire circuit to be idle.
10. Unsupported dependencies decline explicitly. A general Graph fallback preserves behavior
    but does not count as successful regional compilation.

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
reduction is established. The release Redpiler suite is a separate validation step and no
benchmark has run for this checkpoint.
