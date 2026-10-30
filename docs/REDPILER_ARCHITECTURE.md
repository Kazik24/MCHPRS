# Redpiler architecture

The player-facing Redpiler uses the physical Graph backend. An independent
EventProgram/EventState executor and its offline compiler are experimental
research paths; they are not selectable through the player-facing compiler.
World-derived discovery and scratch probes are still being integrated with that
executor. Compilation and runtime are separate: discovering dependencies does
not establish that compiled work executes.

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
LoweredCircuit and builds electrical dependencies and wire watchers from Graph
facts. World-derived WireDiscovery is not an input to this compiler yet.

The offline event executor uses immutable EventProgram tables and mutable
EventState for its own runs and tests. It does not activate on player plots and
has no public backend selector. Offline real-circuit compilation does not
establish that the independent core can replace Graph for player-facing use.

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
Source rise/fall order and movement departure/arrival order have separate plans.

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
watched-wire extractor exist. The reported focused suite covers direct sources,
directionality, stepped dust, attenuation limits, initially-off sources, masking
and boundary isolation. These results do not establish runtime integration.

The latest reported ADDER1BIT event gate fails prepared-0-1-0 at sum tick 3.
Fresh O0 Graph/native replay previously passed all eight one-bit and forty
eleven-bit manifest cases. The scratch discovery diagnostic now matches native
watched-wire strengths for the failing one-bit history.

The immediate next gate is a private, single-receiver static fixture executing
world-derived source/wire/watcher tables through EventState, with actual piston
movement and native-equivalent rearm. Resource arrangements and recipient
ordering follow that bridge. Public activation and FPU performance remain later
gates. See the [implementation scope and current plan](REDPILER_TRACER_COMPILER_SCOPE.md).

Historical FPU measurements describe the retired settled-frame implementation.
They do not establish current offline event-executor behavior or throughput.
