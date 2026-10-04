# Task progress

Source: [tasks.txt](tasks.txt). Started 2026-10-04. Movement rules and slime/honey attachment graphs remain deferred by user instruction. Major milestones use three-word commit messages.

| Task | Status | Evidence / next action |
| --- | --- | --- |
| 1. Finish piston update | Complete | [Timing implementation](PISTON_TIMING_IMPLEMENTATION.md); five matching Java circuit traces |
| 2. Review correctness and simplicity | Complete | [Review findings](PISTON_REVIEW.md); support notifications, NBT progress and malformed progress fixed |
| 3. Basic/edge piston unit tests | Complete | 34 focused cases, partial-step restart and migration tests; all 97 workspace unit tests pass |
| 4. Signed 16-bit adder test | Supplied fixture tested; full 16-bit coverage limited by asset | [Adder report](ADDER_TEST_REPORT.md): 11 interpolated cells fit; stored-input arithmetic and Java traces pass; changed-input circuit limitation matches Java |
| 5. Lifecycle / scheduling / completion issues | Complete | Expanded regressions cover source/payload replacement, removal support, pending observer work and repeated completion |
| 6. Compatible master features | Complete with documented interface exclusions | [Master audit](MASTER_FEATURE_AUDIT.md): commands, WorldEdit, components, compiler pending-state and monitor fixes; conflicting geometry/proxy/compiler replacements excluded |
| 7. Piston performance | Complete | [Performance report](PISTON_PERFORMANCE.md): same-workload before/after measurements, direct registry index, redundant source-write removal and lazy trackers |

Final verification: locked debug/release builds, full workspace tests (97 passed, no failures or ignored tests), formatting, generated-data reproducibility, independent protocol/restart and three-process plot-load smoke tests passed. Tests used temporary worlds. Graphical rendering and live-device acceptance remain observational work.

Task 4 cannot establish 16-bit coverage from the supplied fixture: its sign-derived stride permits 11 in-bounds cells. The tested stored inputs and captured Java traces match; one changed-input arithmetic case also fails on Java. A corrected layout/fixture is needed to extend that coverage. This is an asset/reference limit, not a weakened expected result. No further implementation is identified for that case without new layout evidence.

Commits: `7ad16b0` Fix piston ordering; `62857cf` Test signed adder; `e13d6c5` Repair piston lifecycle; `9e949f6` Port compatible fixes; `d9b5dcd` Preserve compiler states. `9e8947c` Optimize piston runtime. Final validation is recorded in the following documentation milestone.
