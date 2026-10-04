# Task progress

Source: [tasks.txt](tasks.txt). Started 2026-10-04. Movement rules and slime/honey attachment graphs remain deferred by user instruction. Major milestones use three-word commit messages.

| Task | Status | Evidence / next action |
| --- | --- | --- |
| 1. Finish piston update | Complete | [Timing implementation](PISTON_TIMING_IMPLEMENTATION.md); five matching Java circuit traces |
| 2. Review correctness and simplicity | In progress | Review event, ownership, scheduled work and completion paths |
| 3. Basic/edge piston unit tests | Implemented; extend during review | 28 focused cases, partial-step restart and migration tests; all 75 workspace unit tests pass |
| 4. Signed 16-bit adder test | Supplied fixture tested; full 16-bit coverage limited by asset | [Adder report](ADDER_TEST_REPORT.md): 11 interpolated cells fit; stored-input arithmetic and Java traces pass; changed-input circuit limitation matches Java |
| 5. Lifecycle / scheduling / completion issues | Implemented; review remaining acceptance cases | Regression tests cover the reported failures |
| 6. Compatible master features | Pending | Audit CURRENT_VS_MASTER.md against current code; port compatible omissions and document exclusions |
| 7. Piston performance | Pending | Benchmark before changes; optimize measured gaps while retaining reference tests |
