# Redpiler execution model

Redpiler compiles supported, piston-free redstone selections into an electrical
graph. Piston builds are rejected and continue to run through the interpreter.
This document describes the compiled graph and direct backend; the physical
rules remain in [REDSTONE_MODEL.md](REDSTONE_MODEL.md) and
[PISTON_MODEL.md](PISTON_MODEL.md). For the implementation and tests, see
[REDPILER_ARCHITECTURE.md](REDPILER_ARCHITECTURE.md).

## Electrical graph

Each graph node represents a redstone component and its saved state. A directed
edge carries a source's signal to a receiver on a main or side input channel,
with attenuation. For a source strength `s` and edge attenuation `a`, the
received strength is `max(0, s - a)`. Each input channel takes the maximum of
its incoming strengths. Strengths range from 0 to 15.

The graph records spatially discovered power paths so runtime propagation does
not need to search neighboring blocks for every change. The graph passes may
deduplicate links, fold eligible constants, prune unreachable comparator
outputs, coalesce eligible nodes, and prune unneeded nodes when their options
allow it. These passes preserve supported graph behavior under their guards;
they are not a proof of equivalence for arbitrary circuits.

## State and scheduled transitions

Nodes begin with the state read from the world. Scheduled work is transferred
with its remaining deadline and priority, after entries for blocks that have
since changed are discarded. The direct backend propagates input changes over
graph edges and schedules component transitions such as repeater delays,
comparator updates, observer pulses, and lamp turn-off.

The direct scheduler uses half-tick slots and priority queues. It models the
compiled graph's scheduled transitions; it does not model piston movement.
While compilation is active, the backend owns the compiled selection. Reset
exports the resulting state and remaining scheduled work back to the
interpreter.

## Piston behavior and scope

`Compiler::compile` rejects any selection containing a piston before graph
preparation. The command reports `build contains pistion, pistions are not
supported` and identifies the piston in the build error. Piston heads, moving
pistons, and piston timing remain part of the physical interpreter model.
Read-only analysis can inventory piston geometry and explain admission issues,
but it does not produce an executable piston representation.

The compiler supports ordinary electrical graph execution only. No piston
response functions, region composition, sampled piston memory, or instant
execution mode are part of the runtime contract.
