# Callback-link validation change

Measured 2026-10-19. `CircuitDraft::validate_links` now uses the existing `FxHashMap`
for its compiler-owned `(callback reference, source, receiver, delivery kind)` keys.
All reference, guard, route-source, exact-key and duplicate-count checks remain.
No validation calls were removed, and no pipeline or runtime behavior was changed.

## Isolated map-work comparison

A standalone `rustc --edition 2024 -O` harness compares standard `RandomState` with
`rustc_hash::FxHasher` using the same hash-derived key shapes and identical synthetic
inputs: 3,480,000 callback links, repeated exact keys, and reversed delivery order.
Each timed sample builds the multiset, consumes every delivery, checks remaining
counts and drops the map. Input construction is outside the timer. Small assertions
also reject missing and extra multiplicities for both hashers.

Ten pairs alternate which hasher runs first. The process was pinned to logical CPU 4
(`0x10`), matching the earlier compilation profile's affinity.

| Map | Median time |
| --- | ---: |
| Standard `HashMap` | 1.515 s |
| `FxHashMap` | 0.608 s |

This is 59.9% less time for the synthetic multiset operation; pair ratios ranged
from 1.87 to 2.92. A separate unpinned run gave 1.242 s versus 0.534 s (57.0% less).
These numbers are **not full compiler speedups or measurements of actual CHUNGUS
callback keys**. The bounded CHUNGUS test ran concurrently with the pinned comparison,
and other development activity was present, so timing variance remains.

## Correctness checks

- All 12 existing `program_graph::tests` passed, including callback direction and
  duplicates, invalid references, an unmatched duplicate delivery, and missing
  multiplicity returning an error without panicking.
- The existing ignored `fixed_classic_whole_chungus_compilation_and_execution` test
  passed with `MCHPRS_CHUNGUS_TICKS=64` and the independent interpreter oracle.
  The O2 general and O5 whole-fixed paths matched at all 65 checked boundaries.
  Their compile times were 18.481 s and 33.945 s in the optimized development test
  profile; these different compiler paths are not a before/after comparison.
- The focused test binary was built after the map change, before concurrent instant
  runtime integration edits. Later integration requires its own build/checks.

Ignored local evidence is in `target/compilation-fxhash/`: `callback_multiset.rs`,
`callback-multiset-pinned.log`, `pinned-summary.json`, `callback-multiset.log`,
`microbenchmark-summary.json`, `program-graph-tests.log` and `chungus-o5-64.log`.
The CHUNGUS test also writes `target/chungus-o5-native-64.json`.

No paired production compiler benchmark or full workspace suite was run for this
one-line change. The larger repeated-validation opportunities in
`COMPILATION_AND_RUNTIME_PROFILE.md` remain separate work.
