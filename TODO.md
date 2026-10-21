# Current Redpiler work

The v0.6.0 release is the production baseline. Current development scope and verification
gates are in [Redpiler development plan](docs/REDPILER_DEVELOPMENT.md), with the design
explained in [Redpiler architecture](docs/REDPILER_ARCHITECTURE.md) and shipped tiers in
[optimization levels](docs/REDPILER_OPTIMIZATION_LEVELS.md). These items are WIP; they do
not change release guarantees until implemented and validated.

- [ ] Complete COUNTER logical-state materialization and native interpreter handoff,
  including stop/restart. The accepted idle readout difference remains documented.
- [ ] Execute a small ordinary BUD slice without internal physical work. Preserve
  update-triggered same-value writes, acceptance rechecks, reset behavior, and publication
  timing; use Rilax as the small native comparison.
- [ ] Compile BubbleSort RAM state and its output/next-state logic into the logical event
  executor. Validate the full sort, controls, reset, required BUD effects, and native
  continuation before claiming whole-device contraction.
- [ ] Apply the same state/event path to ANPU and validate screen/state behavior, consequential
  BUD transactions, and native continuation. Keep redundant samples distinct from writes.
- [ ] Resolve FPU feedback state and output geometry; validate signal strength and output
  values at the original publication ticks against independent arithmetic/native evidence.
  A frame calculation alone is not production output validation.
- [ ] Expand beyond these devices only with source-derived coverage and explicit fallback
  witnesses. Replace the narrow clock-family executor with generic logical clock/event
  operations after the same lifecycle and handoff contracts pass.
- [ ] Profile complete release execution, then optimize only measured costs. Keep compiler
  CPU time, full plot simulation, visual updates, network/server throughput, warm-up, and
  end-to-end episode time separate.
