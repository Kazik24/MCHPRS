# Redpiler implementation and research

Start with [the current state](CURRENT_STATE.md): supported execution, measured speed,
remaining compiler work, validation limits and the document retirement map.

For the current compilation cleanup and the ten-second target, use the
[compilation simplification plan](COMPILATION_SIMPLIFICATION_PLAN.md). It separates
implemented improvements from the next proposed validation and ownership changes.

Graph and the O2-O5 optimizations work today. O6 has live piston power plans and a
qualified COUNTER logical executor. Whole BubbleSort, ANPU and FPU logical execution
remains unfinished. Recognition, reduced artifacts and executed replacement are
different coverage claims.

## Current proposal

Use the [O6 logical compiler plan](INSTANT_CIRCUIT_ACCELERATION_PLAN.md), updated
2026-10-21: iterative equations, global simplification, retained state/BUD events,
exclusive logical execution and validation gates. It replaces the earlier
physical transport proposal under the current observable-output contract.

The [earlier layered plan](layered-instant-compiler/README.md) remains design history
and a source of reusable boundary/effect analysis, not the current O6 scope.

Keep all edge cases and evidence in [INSTANT_COMPILER_EDGE_CASES.md](INSTANT_COMPILER_EDGE_CASES.md).
The authoritative physical semantics remain [PISTON_MODEL.md](../../../../../docs/PISTON_MODEL.md),
[REDSTONE_MODEL.md](../../../../../docs/REDSTONE_MODEL.md) and
[INTERPRETER_ARCHITECTURE.md](../../../../../docs/INTERPRETER_ARCHITECTURE.md).

## Existing code and historical evidence

- [INSTANT_REGIONS_AND_PROXIES.md](INSTANT_REGIONS_AND_PROXIES.md): the existing O3 region/proxy work.
- [CLASSIC_REDSTONE_PROGRESS.md](CLASSIC_REDSTONE_PROGRESS.md): separate ordinary-redstone work; inspect its current status.
- [BOUNDARY_DRIVEN_INSTANT_COMPILER.md](../../../redpiler-pistons-v4/BOUNDARY_DRIVEN_INSTANT_COMPILER.md): the previous compiler stop checkpoint and lessons.
- [COMPACT_INSTANT_KERNEL.md](../../../redpiler-pistons-v4/COMPACT_INSTANT_KERNEL.md), [SYMBOLIC_VIRTUAL_CIRCUIT.md](../../../redpiler-pistons-v4/SYMBOLIC_VIRTUAL_CIRCUIT.md),
  [CLOSED_INSTANT_MACHINE.md](../../../redpiler-pistons-v4/CLOSED_INSTANT_MACHINE.md) and [INSTANT_LOOP_COMPILER.md](../../../redpiler-pistons-v4/INSTANT_LOOP_COMPILER.md):
  earlier experiments, now archived under `scraps/docs/redpiler-pistons-v4/`. Their source
  paths and successful test counts describe historical snapshots.

The old symbolic event compiler, finite/global kernels, `redpiler_opt` and native/C inference
experiment were removed during cleanup. Do not restore their architecture based on older
documents. Reuse counterexamples and existing fixture/reference infrastructure.
