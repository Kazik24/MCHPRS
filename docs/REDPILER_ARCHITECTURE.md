# Redpiler architecture

Redpiler executes a compiled selection while preserving the world’s redstone
state, controls, and scheduled work. Its physical graph and logical kernels
serve different roles: the graph models component connectivity and events;
logical kernels evaluate only subcircuits that compilation has certified.
The graph remains the general executor and fallback.

## Physical graph and logical programs

Compilation starts from a read-only world analysis, then prepares and lowers a
graph. The immutable [`CircuitProgram`](../crates/core/src/redpiler/program_graph.rs)
contains component nodes, spatially discovered power links, input channels,
guards, callback routes, piston/resource topology, and precomputed plans. Its
paired `CircuitState` contains mutable component state, piston state, and which
resource occupies each contact. [`GraphBackend`](../crates/core/src/redpiler/backend/graph.rs)
runs the general circuit:
control changes enter through component handlers, route notifications update
receivers, and its scheduler and piston phases process delayed actions.

A [`SettledCircuit`](../crates/core/src/redpiler/program_graph/settled_circuit.rs)
is a separate reduced logical program. It is a Boolean or strength-valued DAG
with explicit input and output bindings, optional state and event certificates,
and source-derived timing information. It is not a replacement graph and does
not rediscover physical topology at runtime. A specialized executor evaluates
the DAG and applies results at its certified boundaries; callbacks, piston
actions, reset effects, and retained outputs still belong to Graph unless a
specific runtime binding handles them. Unsupported roots or effects must stay
on the Graph path rather than being dropped.

Settled wire equations that start from a saved dot bind its native dot/cross
mode as a live component-state input. They combine that mode with the current
symbolic neighbor geometry, so a dot that connects and later disconnects keeps
the native cross fallback. Extracting an FPU's Boolean equations does not by
itself certify its clock, reset interval, or full runtime behavior.

## Compile, ownership, and handoff

[`Compiler::compile`](../crates/core/src/redpiler/mod.rs) analyzes the selected
world, runs the configured graph pass pipeline, lowers the result, then creates
the backend. Analysis is read-only. The backend takes ownership only after
lowering and runtime admission succeed. Logical plans are optional: each has to
bind its inputs, outputs, owned state, and any callbacks or events it consumes.
The original graph remains available to execute work outside that contract.

Finite-layer compilation first runs
[`regions::greedy`](../crates/core/src/redpiler/analysis/instant/regions/greedy.rs)
over the existing boundary catalog. It indexes exact input callback occurrences,
follows only the corresponding cell uses and their immediate effects, including
same-owner activations, then retains reached work that can affect a native
boundary. Deferred reset and publication effects remain boundaries. Power reads
participate in usefulness analysis but do not authorize ownership merges.

The partition joins selected owners only through witnessed immediate callback
edges, retaining complete shared-resource cell contracts and every original
perimeter read/effect row. Whether an owner can join another is separate from
whether it can compile alone. Excluded owners remain on Graph with a decline
reason; selected regions still require the existing arrival, boundary and
runtime-entry certificates. This discovery feeds the connected-layer compiler;
it does not itself create a clock scheduler or materializer for `SettledFrame`.

Compilation indexes power once and tries each independent selected region once.
Its lowering context retains the successful arrival proofs. Discovery has a
shared work and memory budget; exhaustion leaves its candidates on Graph.
Cancellation and invalid graphs still fail compilation. There is no
failure-guided repartition/recompile loop: the removed loop repeatedly rebuilt
global ownership and perimeters without demonstrating a useful throughput gain
on the saved plot.

An interaction island is a conservative envelope of mutable dependencies, not
proof that all its components belong to one computation. Computation-frame
ownership is selected positively from equation support and published targets;
unrelated components in the same island remain on Graph. Clocked Counter
devices retain their separate, complete device contract. The current candidate
selector still requires one recognized clock per interaction island; connecting
two otherwise supported clocked devices does not yet establish a supported
combined frame.

Native callback closure can stop at a wire Shape occurrence only when the
shared geometry proof shows that every possible handler leaves the full wire
state unchanged. This proof covers all inferred contact variants, callback
directions and power levels. Update and Turbo callbacks remain separate
obligations. In the saved FPU plus Divider, the remaining native closure still
overlaps 22,626 of 23,042 equation-support nodes. Removing its one optional
terminal-wire output cannot recover that frame; execution boundaries need an
ordered transition contract, not weaker ownership validation.

Retaining a physical cell does not transfer its lifecycle to the frame. Its
piston, resource, reset observer, guards, and callback order remain Graph-owned.
The frame supplies only the certified data queries. Exact retained native
routes bypass incompatible route shortcuts, while callbacks represented by
compiled equation inputs are consumed only at their certified occurrences.
Resource ownership must cover a complete physical owner group; a next-state
equation alone is insufficient. Retained same-wave feedback additionally checks
its canonical resource position and ready piston pose before each computation.
It is not an interface for arbitrary persistent native memory.

The computation frame does not currently own general inlined piston actors.
It has no transition or handoff materializer for their poses, moved resources,
or pending publications. Their native callbacks therefore keep those actors
and their power dependencies on Graph. Masking their nodes would lose state;
relaxing a scope check cannot replace the missing transition contract.
Acceleration across that boundary needs an ordered state/effect transaction,
with native callbacks observing preceding writes and a continuation that uses
the actual state left by any nested callback. The archived symbolic compiler's
final-write-then-effect runtime did not satisfy this requirement.

The saved FPU exposed a concrete missing cut: route 87788 emits a live Shape
callback from piston 2168 to wire 45540. That wire's Main query reads resource
1378 at contact 6058 and neighboring wire node 43278 (wire-table index 3144).
A later ownership diagnostic establishes that both wires remain on Graph,
outside the candidate's corridors. Their callbacks reach foreign piston 697,
the currently committing actor 2168, and a reset. These handlers update outside
wires and queue requests; they do not synchronously accept piston actions or
move resources. Native retraction already publishes its resource pose before
the far-contact callback. This witness therefore does not establish a missing
owned-wire materializer. At the FIFO foundation checkpoint, connected composition
and PullWave lowering still decline it: they lack a transitive effect/read
certificate for that outside callback and cannot yet preserve a continuation
through independent foreign work.

The region already records causal request/acceptance layers and conditional
resource transfers, but `Composition::wires` is only a set of substituted wire
nodes; it has no per-cut strengths. `Arrival::expected_wire_levels` describes
the stationary entry only. Lowering now emits ordered `KernelRouteStep`s at
original callback ordinals: clear a certified corridor, enqueue an internal
request, or deliver a retained native callback. Native acceptance still updates
each actor's pose, resource and deferred state. The runtime leaves Graph's FIFO
intact and commits one matching popped request at a time. Evaluated roots live
in a shared immutable snapshot, separate from Event/CPU scratch. A foreign or
unexpected action cancels the suffix; draining the queue also clears it.

The FIFO foundation passes the 760-test release Redpiler suite. A shared-wire
fixture checks the actual first-tick request order and sources, compiled child
execution in a closed wave, and native child fallback after an intervening
foreign action. Both cases match Graph/native state for 24 ticks. This does
not establish saved-plot admission or throughput. The fixture exposed another
distinction: the native request source is the final Turbo
callback, but its enqueue cut is the producer's original callback. Two producers
can reach the same finish, so lowering cannot reconstruct that cut from the
finish alone. Composition now retains it separately in `UseCause.enqueue_triggers`;
lowering checks it against the corresponding arrival certificate. The JSON
export omits this field when it equals the existing source occurrences.

The foundation's closed-wave rules exclude that external callback. Wire arrivals
require distinct producer and consumer actors, so the singleton exception for
outside handlers cannot coexist with a wire clear. Retained reset-observer
notifications only inspect their own state and schedule a later tick; their
guards do not inspect wire strength. No reachable reordering defect was found
under those rules. Broader eligibility additionally needs the exterior
callback's effect/read proof; ordered steps alone do not supply it.

The experimental transitive exterior callback certificate was removed during
cleanup. It added compilation work while removing only about 0.05% of callbacks
in the saved-plot replay. Multi-actor regions with uncertified exterior wire
callbacks now remain on Graph. The existing singleton boundary, closed-wave
arrival certificates, FIFO continuation and constant-false request pruning stay.
Entry-state specialization and the queue's additional membership index were
also removed. Request deduplication uses the native FIFO's `(piston, action)`
pairs and preserves the first source.

A cycle's `InstantFired` latch is not a physical resource-position input at
every callback cut. Retraction sets the latch before the base-change callbacks
and before moving the resource; restoration publishes the far contact before
clearing the latch. Another actor reading `!InstantFired` as source presence
can therefore observe a future or stale pose. Connected actor execution must
preserve live resource reads at these cuts or prove that no dependent query
can run there. The same requirement applies to intermediate wire strengths.

Stable inputs do not make output callbacks order-independent. Publishing all
final wire values before notifying a piston can omit a transient sample that
would have queued a request; sorting or deduplicating deliveries can also change
request provenance and FIFO order. Bulk wire equations therefore need a separate
boundary-effect proof. A queue-only receiver is still an observable effect.
The current max-plus compiler therefore declines nets with external effectful
callbacks before equation expansion. Only statically inert Update handlers can
cross that bulk boundary; pistons, observers, delayed components, command blocks,
and unknown receivers keep the exact ordered transport path.

Shared payloads use the finite event-layer compiler, whose accepted uses retain
FIFO request order and one-use-per-wave state. Direct observer-reset groups can
join neighboring cells when their resource owners exactly match their uses.
Isolated single-wire corridors can join certified cells, including complete
shared groups. The arrival proof preserves native Turbo visitation and first
matching receiver rows, with each source occurrence bound to its actual emitting
actor. A reset notification can be omitted only when proven inert; captured
directionless notifications to a cell's built-in reset require an observer-off
entry guard. Other observer effects remain on Graph.

Wire entry levels come from the certified source and attenuation, rather than
the schematic's saved power. Runtime requires both the stationary cell contract
and those wire levels before execution. A cold saved input vector can therefore
compile without permitting a kernel to execute against stale wire state.
Other shared wire corridors remain boundaries until their arrival semantics are
certified. Whole-frame
next-state equations inferred from lane geometry remain candidates and cannot
claim those shared resources without a complete lifecycle contract.

A PullWave follows Graph's actual popped requests, committing one action at a
time through the ordinary action handler. Entry checks every actor for a
conflicting BUD, native clock, or retained-frame owner before evaluating or
changing state. It validates the queued region requests and their source order;
unrelated requests remain in the FIFO. A foreign or unexpected pop discards the
predicted suffix. Live acceptance is checked again before each action, and a
route with a native callback cut hands back to Graph before running compiled
steps. Independent islands do not justify reordering scheduled ticks or
command effects.

Production activation goes through `GraphBackend::from_lowered`: it imports
pending ticks, admits logical runtimes, and schedules initial command work.
`new_unactivated` deliberately constructs only the underlying Graph executor;
proof and comparison code can use it without activating a whole-device runtime.

Equation extraction explicitly selects settled resource semantics or recognized
feedback state. Unsupported semantics, unresolved
feedback, exhausted budgets, and malformed programs have typed internal errors;
diagnostic wording does not decide whether compilation declines. Physical
boundary certification and runtime entry checks remain separate obligations.

While active, the backend owns the selection’s component state and pending
work. User lever, button, and pressure-plate changes enter through the graph
backend; `flush` projects visible state to the world without ending execution.
Reset writes the backend’s current state and remaining scheduled work back to
the interpreter. Kernel-private state is not a saved-world format. Runtime
admission also requires a valid starting boundary; pending entry work can keep
the general graph executor selected.

## Timing and controls

The general Graph scheduler advances scheduled component ticks, then piston
actions and deferred connections in defined phases. A logical device instead
uses a source-qualified [`LogicalClock`](../crates/core/src/redpiler/backend/graph/logical_clock.rs)
when its controller matches a supported topology. The clock supplies compute,
publish, and commit edges; external data
controls are sampled from their live component state at those boundaries. A
clock certificate is deliberately narrower than arbitrary piston timing. The
Counter kernel uses a qualified torch/observer clock; broader control, timer,
or reset behavior remains with Graph unless another contract is added.

Command-block power is an explicit frame output. The existing command scheduler
and host execute its effects, including `tellraw`, and retain command entity
results. A command result that cannot be bound into the logical input cone keeps
the circuit on Graph; commands are never discarded to admit a frame.
For certified single-owner payload cells feeding commands, Graph retains the
local piston, reset observer, and deferred publication work while the frame
supplies the data inputs. Near-contact outputs retain their transfer delay and
repeated command pulses. Unsupported output boundaries keep Graph execution.

Terminal readout repeaters are native frame boundaries too. The frame computes
their Main inputs and retains the held values between evaluations; Graph owns
their scheduled transitions and delays. Their source wires remain public outputs.
Repeaters with downstream consumers or side inputs require a broader boundary
proof and are not admitted by this terminal-output adapter.

Single untimed observer-reset cells execute on Graph. The experimental
single-cell firing runtime and its one-shot equation probes have been removed;
the ordinary event-boundary diagnostics remain available for research.

The requested replacement contract compares output states at settled frame
boundaries; intermediate output states may differ. Inputs may be restricted to
frame boundaries and handoff to a settled state. Native circuitry that observes
intermediate effects cannot safely share such a boundary without a separate
proof, and command effects must remain correct. Unsupported circuits use Graph.
The saved FPU does not yet have an admitted implementation of this contract.

## Version boundary and limits

The v0.6.0 settled FPU frame and Counter runtimes are integrated with the newer
logical BUD work. O6 implies stable inputs through reset; no additional
flag is needed. Lower tiers request that promise explicitly with
[`CompilerOptions`](../crates/core/src/redpiler/options.rs)'s
`--stable-instant-inputs` option.

Neither runtime is a universal circuit compiler. Specialized paths are
qualified by input history, physical timing, state ownership, and output
effects. Their admission limits and ordinary Graph fallback are part of the
execution contract. See the [optimization tiers](REDPILER_OPTIMIZATION_LEVELS.md),
[redstone model](REDSTONE_MODEL.md), and [piston model](PISTON_MODEL.md) for
user-facing behavior and physical rules.

The October 25, 2026 release benchmark after the architecture cleanup measured
median **48,476 game TPS** for `FPU_MROWW` and **48,373 game TPS** for
`FPU_FIXED_INSTANT` on a Ryzen 9 5950X.
Each of three samples ran 4,096 changing input frames held for six game ticks,
including input updates, projection, and real command execution. Each sample
recorded 4,096 logical evaluations, zero wire walks, 512 callbacks, 64 piston
actions, and 32 command outputs. Compilation took approximately 22 seconds and
was excluded from TPS. Run `cargo bench -p mchprs_core --bench instant --locked
-- --component fpu_legal --device-frame --tier 6 --episodes 4096 --iterations 3`
to repeat the original workload. Reports are in
`target/architecture-cleanup-fpu-{original,fixed}.json`; the executable SHA-256 is
`94abca940aab260bcd235868f63728dea1434dd038ce0be474c40bf80e8bff89`.

These are historical measurements, not validated throughput for complete FPU behavior. They did not establish preservation of the input indicator lamps and observer pulses; stricter current admission declines the frame while its native boundaries are completed. See the [completeness correction and live server evidence](REDPILER_BUILD_REPORT_2026-10-25.md#independent-logical-devices-26-october).

The production regression checks data outputs against the accepted computation
frame and independently checks native command power, execution, repeated pulses,
and stopping. These checks do not establish full native arithmetic, transient,
or interpreter-handoff equivalence. The older approximately 600 TPS measurements
used a different workload protocol and are not a matched speedup comparison.
