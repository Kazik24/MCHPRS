# Redpiler graph passes

Redpiler prepares an electrical graph for supported piston-free selections.
The pass order lives in
[`passes::prepare`](../crates/core/src/redpiler/passes/mod.rs). Piston builds
are rejected before these passes run; there is no piston-program optimizer.

## Pass sequence

The pipeline identifies component nodes, searches for spatial input paths, and
clamps links whose attenuation makes them ineffective. With `--optimize`, it
also deduplicates links, folds eligible constants, prunes unreachable
comparator outputs, coalesces eligible constants and logic nodes, and—when
combined with `--io-only`—prunes nodes outside the required dependency closure.
Graph export runs when requested.

Each pass has local eligibility checks. Optimization is not a general
equivalence proof for every possible redstone circuit. In particular,
time-dependent component behavior and scheduled work constrain which graph
rewrites are safe.

## Verification

Classical redstone regression cases are in
[`legacy_regressions.rs`](../crates/core/src/redpiler/passes/legacy_regressions.rs).
Generated electrical-graph cases are in
[`redstone_fuzz.rs`](../crates/core/src/redpiler/analysis/tests/redstone_fuzz.rs).
These tests retain repeater timing and other ordinary redstone regressions.
The separate [`redpiler_opt`](../crates/redpiler_opt/src/lib.rs) Boolean
optimizer is not used by the server's graph-pass pipeline.
