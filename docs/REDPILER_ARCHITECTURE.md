# Redpiler architecture

Redpiler compiles a selected world region into an immutable circuit program and
mutable execution state. The compiler reads the world during preparation; it
activates only after validation and lowering succeed.

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
have been removed. Optimization tiers and their current additions are listed in
the [tier guide](REDPILER_OPTIMIZATION_LEVELS.md).

`--stable-instant-inputs` is no longer accepted. The current compiler does not
replace physical piston actions or callback sequences with settled-frame
execution. Native reset, piston scheduling, and world projection continue to
use the physical Graph path.

Historical FPU measurements and their execution contract describe the retired
settled-frame implementation. They are retained in the [build report](REDPILER_BUILD_REPORT_2026-10-25.md)
as historical evidence, not as current behavior or a throughput claim.
