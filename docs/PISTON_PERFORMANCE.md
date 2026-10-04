# Piston performance

Measured 2026-10-04 on this Windows workstation (AMD Ryzen 9 5950X, rustc 1.98.1) with the locked release build, Criterion 0.4, one-second warm-up, three-second sampling and 20 samples. These are local simulation measurements, not production throughput or full Java-equivalence claims.

## Workloads and reproduction

```text
cargo bench -p mchprs_core --bench piston -- --save-baseline before
cargo bench -p mchprs_core --bench piston -- --baseline before
```

The baseline uses the repaired event/progress engine and compatible master fixes from `9e949f6`, before the changes below. The same harness runs before and after. Each cycle powers every sticky piston, advances four game ticks, removes power, then advances four more ticks. Independent pistons carry one stone block and are separated on a grid. Chain workloads carry 8/32/128 stone blocks; after retraction two raw storage writes restore a contiguous chain for the next extension. This reset is included in both timings. No twelve-block movement limit is imposed, as instructed.

World construction, network transmission, client rendering, saves and compilation are excluded. Changes stay buffered, representing interpreted simulation between packet flushes. CPU-heavy builds/tests must finish before measurement; an intermediate run contaminated by concurrent compilation/tests was discarded.

## Changes

1. Replaced repeated registry-range searches with a generated state-to-block index (27,914 `u16` entries, about 55 KiB). Lookup is bounded and allocation-free. Exhaustive tests check state IDs, names and registry IDs against every generated range. A binary-search trial made the smallest workloads slower and was discarded.
2. Removed redundant source clearing during straight-chain extension. Destinations overwrite every source except the first, and the moving head overwrites that first source. This removes a quadratic overlap search and unnecessary air writes while retaining reverse movement and callback order.
3. Allocated a section's 4096-entry change tracker only after a write and released it after producing its packet. Saves retain pending changes. Untouched default-sized plots avoid approximately 31.97 MiB of initial tracker storage; active sections allocate on demand. This does not change the save format.

## Results

| Workload | Before mean | After mean | Mean change |
| --- | ---: | ---: | ---: |
| 1 independent | 5.60 us | 5.38 us | -3.9% |
| 64 independent | 421.44 us | 405.12 us | -3.9% |
| 256 independent | 2.31 ms | 2.27 ms | -1.7% |
| 1024 independent | 22.17 ms | 22.20 ms | +0.1% |
| 8 chain | 12.12 us | 12.08 us | -0.4% |
| 32 chain | 34.98 us | 34.22 us | -2.2% |
| 128 chain | 150.42 us | 143.25 us | -4.8% |

See the checked-in [measurement data](piston-repair/benchmark-results.json) for the final estimates and confidence intervals. Measurements vary with hardware, background work and the workload; small changes should not be generalized beyond this run.

## Remaining gaps

Active motions retain deterministic vector order and the compact save representation. Registration, identity lookup, event deduplication and removal still scan queued/active work, so many simultaneous pistons scale worse than a single piston. An additional index would need to preserve insertion order, stale identities, partial-step snapshots and restart behavior; it is a follow-up rather than a speculative rewrite in this repair.

Lazy packet trackers trade idle memory for an allocation on the first write after a packet flush. These benchmarks exclude flushing, so they do not establish the end-to-end cost of that trade-off. Server profiling with live circuits should measure it if allocation becomes significant.

The five recorded Java traces, signed-adder tests, lifecycle regressions, 97 workspace unit tests, save/restart cases and independently decoded packet smoke tests remain the correctness checks. Movement rules, adhesive graphs and unsupported compiled pistons are still deferred.
