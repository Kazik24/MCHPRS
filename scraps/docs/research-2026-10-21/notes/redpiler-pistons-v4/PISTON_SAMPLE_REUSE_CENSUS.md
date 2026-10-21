# BubbleSort piston-sample reuse census, 2026-10-20

The full O5 + stable-input BubbleSort run checked initial RAM and every tick
through tick 10,036 against the frozen interpreter trace. All 10,037 samples
matched; the final output words were 0 through 15. All 27 execution counters
matched the retained minimized-program workload, including 33,492,510 piston
samples and 89,455,682 callbacks. This instrumented run is not a TPS measurement.

The test-only recorder brackets exact transport, learned replay, signal-channel
replay and stable logic. It admits only nets without ordered receivers, clears
its memo on every actual wire write, and isolates nested episodes. It records
the action after the ordinary live sample; it skips no callback or power read.
Base-pending pistons, which return before a power sample, are not counted here.

| Scope | Samples in eligible intervals | Repeated qualifying action | Repeated no action | All repeats |
|---|---:|---:|---:|---:|
| Exact transport | 120,128 | 426 | 3,309 | 3,735 |
| Learned replay | 1,707,431 | 25,774 | 162,009 | 187,783 |
| Signal channel | 148,687 | 16,081 | 484 | 16,565 |
| Stable logic | 2,663,792 | 326,429 | 50,231 | 376,660 |
| Total | 4,640,038 | 368,710 | 216,033 | 584,743 |

These 584,743 repeats are 1.75% of all piston samples. This is a count of
opportunities inside the measured conservative scope, not a global upper bound
on future dependency-aware optimizations and not a predicted runtime speedup.
No conflicting outcomes occurred. The original recorder counted differing
qualifying actions directly; each row also satisfies
`repeated_any == repeated_qualifying_same_action + repeated_no_action`,
which excludes mixed no-action/qualifying outcomes in this run. The retained
recorder checks the complete optional action, including both mixed directions.

The same run counted raw recursive guard evaluations:

| Kind | Calls |
|---|---:|
| State | 327,804,374 |
| All | 206,058,550 |
| Not | 95,494,516 |
| Any | 48,500,095 |
| Captured profile identity | 39,550,778 |
| Other site-test kinds | 0 |

These are nested evaluator calls, not distinct guards or CPU-time percentages.
Direct Always/State returns in the outer wrapper are outside this census.

Episode-local piston caching is therefore deferred. The next measured candidate
compares captured profile IDs directly instead of resolving a whole block for
those 39.55 million identity checks. It changes neither capture timing nor
callback/request order, and needs no additional runtime representation.

Evidence: `target/piston-sample-census/baseline-census.json`, `cpu.log`,
`unit-tests.log`, `build.log`, and frozen `core-tests.exe`, SHA-256
`3a9d307d38fa2a066a7eb2f35e6a6d515abb5a429704c6027392ec39d37db7b4`.
The initial recorder had five passing unit tests. Later optional-action conflict
checks and their additional unit test are verified with the candidate batch.
