# Redpiler architecture

The player-facing Redpiler uses the physical Graph backend. An independent
EventProgram/EventState executor and its offline compiler are experimental
research paths; they are not selectable through the player-facing compiler.
World-derived discovery feeds narrow adapters to that executor. A second private
prototype in `sampled.rs` compiles one lever–dust–lamp chain directly from the
world. Neither experimental path is ready for player activation. Compilation
and runtime are separate: discovering dependencies does not establish that
compiled work executes.

## Current execution paths

Graph is the player-facing backend. It compiles a selected region into
CircuitProgram and CircuitState:

```text
World selection
  -> geometry and component discovery
  -> power links and ordered callback routes
  -> general graph rewrites and execution plans
  -> CircuitProgram + CircuitState
  -> physical GraphBackend
```

The graph records components, wires, contacts, pistons, resources, signal
dependencies, and callback order. `GraphBackend` owns circuit state, scheduled
ticks, piston requests, callbacks, and reset behavior. Physical effects continue
through the existing world handlers. Unsupported or unoptimized work remains on
the general graph executor.

The active optimization pipeline contains general guard and callback rewrites,
wire plans, and the existing classic signal compilers. The former instant-region,
transport-kernel, stable-logic, logical-device, and settled-frame compiler paths
have been removed.
The independent event core has immutable EventProgram tables and mutable
EventState: component strengths, scheduled work, piston requests, shared
resources and movement completions. Its offline compiler still accepts
LoweredCircuit. `compile_with_wire_discovery` replaces selected wire dependencies
and watchers with world-derived facts for static geometry and one piston.
`compile_with_resource_discovery` additionally accepts three arrangements of one
resource and sampled movement notifications. These adapters still use Graph
inventory and other Graph-derived dependencies; they are not the final compiler.

The offline event executor uses immutable EventProgram tables and mutable
EventState for its own runs and tests. It does not activate on player plots and
has no public backend selector. Offline real-circuit compilation does not
establish that the independent core can replace Graph for player-facing use.

`sampled.rs` has its own immutable Program and mutable Runtime, with separate
rise/fall receiver instructions and a TickScheduler. Its runtime refreshes the
wire from the live lever and the lamp from the live wire; it does not replay
stored output frames. It admits only a straight floor-lever, single-dust,
single-lamp chain with stone supports and settled input changes. Every runtime
tick drains the current scheduler bucket, advances the ring once, then drains
the new current bucket through the same lamp dispatch, matching native scheduler
phases. `schedule_tick(..., 2, ...)` retains its four-game-tick lamp-off delay.
The sampled acceptance test passes for initially-off and initially-on entry,
including host preservation, 12 alternating transitions per entry state,
pending-work comparisons through scheduler wrap, and lamp-off timing. The
compiler's sampled-runtime self-check remains enabled. This narrow test does not
establish piston, observer, resource or general-circuit support. Latest
verification with `cargo test -p mchprs_core redpiler::sampled --lib`: 8 passed,
0 failed, 0 ignored (1,035 filtered out), including boundary-read rejection.

The same private module has a separate compile-time inventory pass for empty
piston actors, with stable scan-order IDs, home/far cells, loaded base/head
state and transitive shared-cell groups. It uses captured scratch-world reads
and rejects inconsistent, occupied, moving or out-of-capture contacts. This
inventory is not connected to lamp compiler admission or a piston runtime; the
unmet piston gate is native-equivalent extension, retraction and rearm with
accepted actions and pending work checked.

## Target event architecture

```text
read-only world + selection + actual entry work
  -> captured blocks/entities and supported boundary
  -> world-derived components, electrical sources and wire watchers
  -> scratch-interpreter resource probes and ordered update observations
  -> validated, flat EventProgram + starting EventState
  -> exclusive event execution
  -> explicit output publication and snapshot-based reset
```

Compilation uses the existing interpreter in private ScratchWorld instances.
The host world is read-only during probes. The executor retains compiled arrays
and its own state, without an interpreter or Graph running behind it to calculate
results. The final compiler does not require rich::prepare or LoweredCircuit.
A temporary adapter may reuse Graph-derived descriptors while replacing specific
dependencies with world-derived tables; that boundary must remain explicit.

Electrical contributions and update delivery are distinct. A piston can lose
power without receiving a callback. A delivered update samples main and quasi
power; a queued action revalidates live power when it executes. Watched wires
retain raw strengths, including positive-to-positive changes and source masking.
Source rise/fall order and movement departure/arrival order need separate plans.
The general EventProgram currently has one operation range per source; separate
edge programs exist only in the narrow sampled prototype. Do not treat the
general compiler's contribution-plus-notification construction as sampled order.

The proposed offline executor contract permits different intermediate timing
and reset-only exit, but requires correct declared boundary outputs and
represented BUD, observer, clock and shared-resource behavior needed to produce
those outputs. Unsupported crossings or entry work must decline before native
execution is disabled.

## Ownership and pass boundaries

- The host owns the selected starting world and outside work.
- Compilation owns the snapshot, scratch instances and bounded observations.
- Each pass produces inspectable tables; validation precedes installation.
- EventProgram owns immutable execution facts; EventState owns runtime queues,
  component states, resource positions and clocks.
- The backend owns output projection and the reset snapshot. Existing checked
  restoration and reset-before-edit safeguards remain required.

Share a compile-scoped read-only world input across the event preparation stages.
Pass mutable scratch access only to probe stages. Keep ordinary Graph passes
intact; a generic framework rewrite is unnecessary.

## Current evidence and next gate

The native observation hook, isolated scratch execution and world-derived
watched-wire extractor exist. Native comparison fixtures exercise the discovery
adapters through EventState for static piston and selected one-resource BUD
histories. This establishes narrow runtime integration, not general admission.

Verification on 30 October 2026:

- `cargo test -p mchprs_core redpiler::sampled --lib`: latest run, 8 passed,
  0 failed, 0 ignored (1,035 filtered out). Native rise/fall execution passes
  for initially-off and initially-on entry, including 12 alternating edges,
  settled pending-work comparisons through scheduler wrap, host preservation,
  and the four-game-tick lamp-off delay. Tight captures and an excluded
  redstone-block source reject with a positioned boundary error, and preserve
  host blocks, entities, scheduled work and piston state.
- `cargo test -p mchprs_core redpiler::event_netlist --lib`: 45 passed,
  1 failed, 1 ignored. ADDER1BIT fails as described below; COUNTER is ignored.

Sampled ticks drain the current bucket, advance the ring once, and drain the new
current bucket through the same lamp dispatch. This matches native scheduled
processing. Lamp off remains four game ticks (`schedule_tick(..., 2, ...)`), not
two. The acceptance test crosses the 32-bucket wrap and checks the native
pending work at each boundary.

The latest reported ADDER1BIT event gate fails prepared-0-1-0 at sum tick 3.
Fresh O0 Graph/native replay previously passed all eight one-bit and forty
eleven-bit manifest cases. The scratch discovery diagnostic now matches native
watched-wire strengths for the failing one-bit history.

The sampled lamp gate and compile-time empty-piston inventory pass. The unmet
sampled milestone is the separate piston execution gate (extension, retraction
and rearm against native execution, with accepted actions and pending work
checked). The event compiler still has the independent ADDER1BIT gate failure
described above. Continue with
edge-specific delivery order, repeater pulse retention/locking/priorities and
supported piston motion rules. General interacting arrangements, public
activation and FPU performance remain later gates. See the
[milestone plan](REDPILER_SAMPLED_ENGINE_PLAN.md) and
[implementation assignment](MAIR_PORT_IMPLEMENTATION_PROMPT.md).

Historical FPU measurements describe the retired settled-frame implementation.
They do not establish current offline event-executor behavior or throughput.

## Event-program acceptance and deferred diagnostics

Run `cargo test -p mchprs_core redpiler::event_netlist --lib` for the active
mechanism and fixture suite. It covers scratch isolation and admission, source
masking and attenuation, event notification behavior, resources, scheduling,
BUD fixtures, and ADDER1BIT arithmetic. Run
`cargo test -p mchprs_core redpiler::event_netlist::tests::real_circuits::counter_basic_manifest_runs_through_event_program --lib -- --ignored`
to reproduce the retained COUNTER_BASIC failure: execution diverges at piston
(56,36,44). Keep the manifest outputs and observation window unchanged while
investigating it. The ADDER1BIT gate is active in the suite and currently fails
for `prepared-0-1-0`: raw sum repeater output differs at tick 3 (event 0,
expected 1). Reproduce it with
`cargo test -p mchprs_core redpiler::event_netlist::tests::real_circuits::offline_adder1_compiled_event_program_arithmetic --lib`.
Keep this gate and its declared output checks intact; use native traces to
diagnose intermediate differences. `cargo check -p mchprs_core --tests` checks
the retained suite.

The ignored ADDER1BIT Graph/EventProgram lockstep diagnostics were removed.
They asserted piston and sum state at intermediate Graph ticks, a retired
timing contract that is not the approved output-boundary acceptance rule. The
manifest arithmetic gate remains active. Reintroduce a diagnostic only when it
compares a named manifest case at its declared output boundary and uses native
traces solely to explain intermediate differences. The prepared
`0-1-0` scratch-discovery diagnostic remains because it tests native-derived
source discovery for a known failing event history.

ADDER11BIT, CPU_BUBBLESORT, FPU_27 and TEST_ fixtures remain established
Graph/interpreter fixture coverage, outside the event-program acceptance suite
until this compiler supports their required mechanisms. Do not infer event
parity from those other backends' results.
