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

Production activation goes through `GraphBackend::from_lowered`: it imports
pending ticks, admits logical runtimes, and schedules initial command work.
`new_unactivated` deliberately constructs only the underlying Graph executor;
proof and comparison code can use it without activating a whole-device runtime.

Equation extraction explicitly selects settled resource semantics, recognized
feedback state, or one-shot fired state. Unsupported semantics, unresolved
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

The current original-checkout WIP adds a source-certified
[`LogicalFiringRuntime`](../crates/core/src/redpiler/backend/graph/logical_firing.rs).
It intercepts only an admitted activation, piston action, observer reset, and
retained lamp effects, and leaves other routes and actors on Graph. This event
runtime is experimental and is not part of the v0.6.0 release.

## Version boundary and limits

The v0.6.0 settled FPU frame and Counter runtimes are integrated with the newer
logical BUD and firing work. O6 implies stable inputs through reset; no additional
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

The production regression checks data outputs against the accepted computation
frame and independently checks native command power, execution, repeated pulses,
and stopping. These checks do not establish full native arithmetic, transient,
or interpreter-handoff equivalence. The older approximately 600 TPS measurements
used a different workload protocol and are not a matched speedup comparison.
