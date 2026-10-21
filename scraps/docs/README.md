# REDPILER review documents

## Latest archive

- [2026-10-21 research and plan snapshots](research-2026-10-21/README.md): the
  54 former `docs/notes/` files, previous TODO, benchmark plan, and tier guide.
- [2026-10-21 Redpiler review](reports/REDPILER_REVIEW_2026-10-21.md): additions,
  removed paths, cleanup priorities, and architecture limits.
- [Maintained general references](../../docs/README.md).


[Failed attempts summary](REDPILER_PISTON_ATTEMPTS_SUMMARY.md) describes what was tried, the encountered problems and their architectural causes.

[Parser behavior and generalization barriers](REDPILER_PARSER_GENERALIZATION.md) compares the actual preparation paths and the unusual geometry, notification, reset, material and entry-state cases they had to handle.

[Regression cases and unit tests](REDPILER_REGRESSION_TESTS.md) explains what the tests asserted, which bugs required them, inherited versus added coverage, oracle limitations, and test modules omitted by the final branch.

## Documents moved from the current checkout

Source revision: `b2aa1a19161fd514bf9f6444a57b87d4729641dc`. Relative links were updated for the new locations.

| Original location | Archived document |
| --- | --- |
| `docs/REDPILER_ARCHITECTURE.md` | [REDPILER_ARCHITECTURE.md](REDPILER_ARCHITECTURE.md) |
| `docs/REDPILER_MODEL.md` | [REDPILER_MODEL.md](REDPILER_MODEL.md) |
| `docs/REDPILER_OPTIMIZER.md` | [REDPILER_OPTIMIZER.md](REDPILER_OPTIMIZER.md) |
| `docs/REDPILER_PARSER.md` | [REDPILER_PARSER.md](REDPILER_PARSER.md) |
| `docs/notes/COMPILED_GRAPH_OBSERVABILITY_PLAN.md` | [COMPILED_GRAPH_OBSERVABILITY_PLAN.md](notes/COMPILED_GRAPH_OBSERVABILITY_PLAN.md) |
| `docs/notes/NOTIFICATION_GATED_RESPONSES.md` | [NOTIFICATION_GATED_RESPONSES.md](notes/NOTIFICATION_GATED_RESPONSES.md) |
| `test_data/piston-research/pc-counter-measurement-points-20261010/TIMING_REPORT.md` | [TIMING_REPORT.md](research/pc-counter-measurement-points-20261010/TIMING_REPORT.md) |
| `test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md` | [ADMISSION_REPORT.md](research/pc-counter-minimal-20261009/ADMISSION_REPORT.md) |
| `test_data/piston-research/test-potados-counter-20261008/TIMING_REPORT.md` | [TIMING_REPORT.md](research/test-potados-counter-20261008/TIMING_REPORT.md) |
| `test_data/piston-research/test-potados-counter-revised-20261008/TIMING_REPORT.md` | [TIMING_REPORT.md](research/test-potados-counter-revised-20261008/TIMING_REPORT.md) |
| `test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md` | [REDPILER_ADJUSTMENT_PLAN.md](research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md) |
| `test_data/piston-research/test-prefix-20261008/TIMING_REPORT.md` | [TIMING_REPORT.md](research/test-prefix-20261008/TIMING_REPORT.md) |

## Historical branch snapshots

These files are exact copies from the commits below. Their contents and historical claims were preserved. Links inside them describe their original source tree; the snapshots are a selected evidence collection, rather than complete checkouts.

Passing results inherited by a branch do not establish correctness of a later replacement executor. Consult the summary for those distinctions.

### redpiler/generalization-attempt

Commit: `117e302fb2359b59b54371b6cc508e337820f606`.

- [docs/PISTON_MODEL.md](generalization-attempt/docs/PISTON_MODEL.md)
- [docs/REDPILER_ARCHITECTURE.md](generalization-attempt/docs/REDPILER_ARCHITECTURE.md)
- [docs/REDPILER_MODEL.md](generalization-attempt/docs/REDPILER_MODEL.md)
- [docs/REDPILER_OPTIMIZER.md](generalization-attempt/docs/REDPILER_OPTIMIZER.md)
- [docs/REDPILER_PARSER.md](generalization-attempt/docs/REDPILER_PARSER.md)
- [docs/REDPILER_PARTIAL_COMPILATION.md](generalization-attempt/docs/REDPILER_PARTIAL_COMPILATION.md)
- [docs/REDSTONE_MODEL.md](generalization-attempt/docs/REDSTONE_MODEL.md)
- [docs/Redpiler.md](generalization-attempt/docs/Redpiler.md)
- [docs/tests/ANPU_PHYSICAL_DOMAIN_MODEL.md](generalization-attempt/docs/tests/ANPU_PHYSICAL_DOMAIN_MODEL.md)
- [docs/tests/ANPU_PISTON_DOMAINS.md](generalization-attempt/docs/tests/ANPU_PISTON_DOMAINS.md)
- [docs/tests/CPU_REFERENCES.md](generalization-attempt/docs/tests/CPU_REFERENCES.md)
- [docs/tests/FPU_DIVIDER.md](generalization-attempt/docs/tests/FPU_DIVIDER.md)
- [docs/tests/INSTANT_PISTONS.md](generalization-attempt/docs/tests/INSTANT_PISTONS.md)
- [test_data/cpu-references/pm1-sort-correction-results.json](generalization-attempt/test_data/cpu-references/pm1-sort-correction-results.json)
- [test_data/cpu-references/pm1-sort-generalization-results.json](generalization-attempt/test_data/cpu-references/pm1-sort-generalization-results.json)

### redpiler/generalization-2

Commit: `c9a2afb968505b052837dedb4c4259fc5f0f6604`.

- [docs/PISTON_MODEL.md](generalization-2/docs/PISTON_MODEL.md)
- [docs/REDPILER_ARCHITECTURE.md](generalization-2/docs/REDPILER_ARCHITECTURE.md)
- [docs/REDPILER_MODEL.md](generalization-2/docs/REDPILER_MODEL.md)
- [docs/REDPILER_OPTIMIZER.md](generalization-2/docs/REDPILER_OPTIMIZER.md)
- [docs/REDPILER_PARSER.md](generalization-2/docs/REDPILER_PARSER.md)
- [docs/REDPILER_PARTIAL_COMPILATION.md](generalization-2/docs/REDPILER_PARTIAL_COMPILATION.md)
- [docs/REDSTONE_MODEL.md](generalization-2/docs/REDSTONE_MODEL.md)
- [docs/Redpiler.md](generalization-2/docs/Redpiler.md)
- [docs/tests/ANPU_PHYSICAL_DOMAIN_MODEL.md](generalization-2/docs/tests/ANPU_PHYSICAL_DOMAIN_MODEL.md)
- [docs/tests/ANPU_PISTON_DOMAINS.md](generalization-2/docs/tests/ANPU_PISTON_DOMAINS.md)
- [docs/tests/CPU_REFERENCES.md](generalization-2/docs/tests/CPU_REFERENCES.md)
- [docs/tests/FPU_DIVIDER.md](generalization-2/docs/tests/FPU_DIVIDER.md)
- [docs/tests/INSTANT_PISTONS.md](generalization-2/docs/tests/INSTANT_PISTONS.md)
- [test_data/cpu-references/pm1-sort-correction-results.json](generalization-2/test_data/cpu-references/pm1-sort-correction-results.json)
- [test_data/cpu-references/pm1-sort-generalization-results.json](generalization-2/test_data/cpu-references/pm1-sort-generalization-results.json)
- [test_data/piston-research/test-potados-counter-20261008/TIMING_REPORT.md](generalization-2/test_data/piston-research/test-potados-counter-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-potados-counter-revised-20261008/TIMING_REPORT.md](generalization-2/test_data/piston-research/test-potados-counter-revised-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md](generalization-2/test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md)
- [test_data/piston-research/test-prefix-20261008/TIMING_REPORT.md](generalization-2/test_data/piston-research/test-prefix-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-prefix-20261008/output-summary.json](generalization-2/test_data/piston-research/test-prefix-20261008/output-summary.json)
- [test_data/piston-research/test-prefix-20261008/output-timed-summary.json](generalization-2/test_data/piston-research/test-prefix-20261008/output-timed-summary.json)
- [test_data/piston-research/test-prefix-20261008/output-timed-verification.json](generalization-2/test_data/piston-research/test-prefix-20261008/output-timed-verification.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-before.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-counter-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-indexed.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-counter-indexed.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-native.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-counter-native.json)
- [test_data/piston-research/test-prefix-20261008/perf-divider-before.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-divider-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-fpu-before.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-fpu-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-fpu-indexed.json](generalization-2/test_data/piston-research/test-prefix-20261008/perf-fpu-indexed.json)
- [test_data/piston-research/test-prefix-20261008/production-native-test-log.txt](generalization-2/test_data/piston-research/test-prefix-20261008/production-native-test-log.txt)
- [test_data/piston-research/test-prefix-20261008/workspace-test-log.txt](generalization-2/test_data/piston-research/test-prefix-20261008/workspace-test-log.txt)

### redpiler/merge-domains

Commit: `70f6fcdba037875cfdd23a10897723f092bd3521`.

- [docs/PISTON_MODEL.md](merge-domains/docs/PISTON_MODEL.md)
- [docs/REDPILER_ARCHITECTURE.md](merge-domains/docs/REDPILER_ARCHITECTURE.md)
- [docs/REDPILER_COMPOSITION.md](merge-domains/docs/REDPILER_COMPOSITION.md)
- [docs/REDPILER_MODEL.md](merge-domains/docs/REDPILER_MODEL.md)
- [docs/REDPILER_OPTIMIZER.md](merge-domains/docs/REDPILER_OPTIMIZER.md)
- [docs/REDPILER_PARSER.md](merge-domains/docs/REDPILER_PARSER.md)
- [docs/REDSTONE_MODEL.md](merge-domains/docs/REDSTONE_MODEL.md)
- [test_data/cpu-references/pm1-sort-correction-results.json](merge-domains/test_data/cpu-references/pm1-sort-correction-results.json)
- [test_data/cpu-references/pm1-sort-generalization-results.json](merge-domains/test_data/cpu-references/pm1-sort-generalization-results.json)
- [test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md](merge-domains/test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md)
- [test_data/piston-research/test-potados-counter-20261008/TIMING_REPORT.md](merge-domains/test_data/piston-research/test-potados-counter-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-potados-counter-revised-20261008/TIMING_REPORT.md](merge-domains/test_data/piston-research/test-potados-counter-revised-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md](merge-domains/test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md)
- [test_data/piston-research/test-prefix-20261008/TIMING_REPORT.md](merge-domains/test_data/piston-research/test-prefix-20261008/TIMING_REPORT.md)
- [test_data/piston-research/test-prefix-20261008/output-summary.json](merge-domains/test_data/piston-research/test-prefix-20261008/output-summary.json)
- [test_data/piston-research/test-prefix-20261008/output-timed-summary.json](merge-domains/test_data/piston-research/test-prefix-20261008/output-timed-summary.json)
- [test_data/piston-research/test-prefix-20261008/output-timed-verification.json](merge-domains/test_data/piston-research/test-prefix-20261008/output-timed-verification.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-before.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-counter-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-indexed.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-counter-indexed.json)
- [test_data/piston-research/test-prefix-20261008/perf-counter-native.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-counter-native.json)
- [test_data/piston-research/test-prefix-20261008/perf-divider-before.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-divider-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-fpu-before.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-fpu-before.json)
- [test_data/piston-research/test-prefix-20261008/perf-fpu-indexed.json](merge-domains/test_data/piston-research/test-prefix-20261008/perf-fpu-indexed.json)
- [test_data/piston-research/test-prefix-20261008/production-native-test-log.txt](merge-domains/test_data/piston-research/test-prefix-20261008/production-native-test-log.txt)
- [test_data/piston-research/test-prefix-20261008/workspace-test-log.txt](merge-domains/test_data/piston-research/test-prefix-20261008/workspace-test-log.txt)

## Storage

`scraps/` is ignored by Git in this repository. The collection exists locally; it will need explicit inclusion if it is intended to be versioned.
