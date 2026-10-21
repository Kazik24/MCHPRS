# Pure circuit optimization

> Archived on 2026-10-18: this synthesis-only crate and its workspace/dependency wiring
> were removed. Source is preserved in commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> See [algorithms and recovery](../../scraps/docs/research-2026-10-21/notes/redpiler-pistons-v4/ALGORITHMS_AND_RECOVERY.md).
> Commands and integration descriptions below apply to that historical checkout.

This crate optimizes acyclic Boolean expressions. It has no World access, timing,
callback delivery, piston ownership or reset semantics. The caller must first prove
that a fragment is a pure expression and declare every required output and state-write
root. Root order and duplicate occurrences are preserved.

`reference` validates all nodes, including unused nodes, and produces a topological
execution plan. `optimize` folds local identities, shares identical operations and
prunes nodes unreachable from the declared roots. Evaluation uses caller-owned buffers
and does not allocate.

`specialize` additionally accepts `Option<bool>` bindings in **declared input order**:
`Some(value)` fixes that input, while `None` leaves it variable. The returned plan keeps
the original input layout; supplied values for fixed inputs are ignored. Its equivalence
contract applies only to assignments consistent with those bindings. Budget exhaustion
returns an exact **specialized** reference plan; cancellation returns an error.
Invalid references and cycles are rejected before binding or cancellation.

## Redpiler integration

O4 instant-kernel lowering uses this crate to specialize occurrence predicates for each
already-certified held-input assignment. The lowered program retains only active
Electrical writes, dirty owners and ordered presentation effects. Equal phase effect
lists share storage. Scalar values and logical work counts are evaluated exactly during
lowering. Runtime ticks follow these unconditional lists; private state and future work
remain in the kernel's cold reconstruction representation.

This is bounded specialization of the existing small kernel family, not proof that a
physical update can be removed from an arbitrary circuit. The kernel compiler establishes
the closed boundary, input protocol and causal trajectory before Boolean optimization.
The JSON export remains the post-pass semantic kernel definition; the lowered effect
table is derived from it, and its retained storage is reported separately.

## Checks

```powershell
cargo test -p mchprs_redpiler_opt --locked
cargo test -p mchprs_core --lib instant_machine_tests --locked
cargo test -p mchprs_core --lib kernel_projection_tests --locked
cargo run -p mchprs_redpiler_opt --example benchmark --release --locked -- --check
```

The synthetic example checks Boolean execution only. Use the existing core `cpus`
benchmark for server Graph timings; synthetic DAG throughput is not server TPS.
