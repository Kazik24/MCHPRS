# Redpiler piston research and implementation notes

## Current proposal

The [layered instant compiler plan](layered-instant-compiler/README.md) is the current
implementation proposal from 2026-10-18. It includes architecture, algorithms, representation
shapes, eight reviewable implementation steps and a removal/replacement/reuse map.
It is not implemented or enabled by writing this plan.

Keep all edge cases and evidence in [INSTANT_COMPILER_EDGE_CASES.md](INSTANT_COMPILER_EDGE_CASES.md).
The authoritative physical semantics remain [PISTON_MODEL.md](../../PISTON_MODEL.md),
[REDSTONE_MODEL.md](../../REDSTONE_MODEL.md) and
[INTERPRETER_ARCHITECTURE.md](../../INTERPRETER_ARCHITECTURE.md).

## Existing code and historical evidence

- [INSTANT_REGIONS_AND_PROXIES.md](INSTANT_REGIONS_AND_PROXIES.md): the existing O3 region/proxy work.
- [CLASSIC_REDSTONE_PROGRESS.md](CLASSIC_REDSTONE_PROGRESS.md): separate ordinary-redstone work; inspect its current status.
- [BOUNDARY_DRIVEN_INSTANT_COMPILER.md](BOUNDARY_DRIVEN_INSTANT_COMPILER.md): the previous compiler stop checkpoint and lessons.
- [COMPACT_INSTANT_KERNEL.md](COMPACT_INSTANT_KERNEL.md), [SYMBOLIC_VIRTUAL_CIRCUIT.md](SYMBOLIC_VIRTUAL_CIRCUIT.md),
  [CLOSED_INSTANT_MACHINE.md](CLOSED_INSTANT_MACHINE.md) and [INSTANT_LOOP_COMPILER.md](INSTANT_LOOP_COMPILER.md):
  earlier experiments. Their source paths and successful test counts describe historical snapshots.

The old symbolic event compiler, finite/global kernels, `redpiler_opt` and native/C inference
experiment were removed during cleanup. Do not restore their architecture based on older
documents. Reuse counterexamples and existing fixture/reference infrastructure.
