# Task progress

Source: [tasks.txt](tasks.txt). Started 2026-10-04. Movement rules and slime/honey attachment graphs remain deferred by user instruction. Major milestones use three-word commit messages.

| Task | Status | Evidence / next action |
| --- | --- | --- |
| 1. Finish piston update | Complete | [Timing implementation](PISTON_TIMING_IMPLEMENTATION.md); five matching Java circuit traces |
| 2. Review correctness and simplicity | Complete | [Review findings](PISTON_REVIEW.md); support notifications, NBT progress and malformed progress fixed |
| 3. Basic/edge piston unit tests | Complete | 34 focused cases, partial-step restart and migration tests; all 85 workspace unit tests pass |
| 4. Signed 16-bit adder test | Supplied fixture tested; full 16-bit coverage limited by asset | [Adder report](ADDER_TEST_REPORT.md): 11 interpolated cells fit; stored-input arithmetic and Java traces pass; changed-input circuit limitation matches Java |
| 5. Lifecycle / scheduling / completion issues | Complete | Expanded regressions cover source/payload replacement, removal support, pending observer work and repeated completion |
| 6. Compatible master features | Complete with documented interface exclusions | [Master audit](MASTER_FEATURE_AUDIT.md): commands, WorldEdit, components, compiler pending-state and monitor fixes; conflicting geometry/proxy/compiler replacements excluded |
| 7. Piston performance | Pending | Benchmark before changes; optimize measured gaps while retaining reference tests |
