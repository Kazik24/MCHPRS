# 8-bit Adder — action 63

Status: reproduced on baseline `6ef1b165` and current `ea297c16`; root cause of the visible failure is not fully resolved. Use the [common reproduction instructions](README.md).

Fixture: `2735cc14b1051cab4197df60265878b2`, [schematic](../../../test_data/redstonedb/schematics/2735cc14b1051cab4197df60265878b2-8_bit_Adder.schem).
SHA-256: `1f58889c7eb3394b2dfd74398d475005269074092c1a20dc0fbffa15db4fa2c7`.
Trace: 17 levers, 66 actions, 834 ticks.

## Exact disagreements

First observed live pending-membership difference: **action 3**, lever **`(13,13,18)`**, local tick **4** / absolute tick **64**. The interpreter queues comparator **`(12,14,13)`** for two ticks later, Normal priority, block type 441. Rust omits it. Checked block states still match. This occurs on both revisions.

First checked block-state difference: **action 63**, lever index **8**, **`(15,17,18)`**, wait **8**, starting after absolute tick **819**. There is no projected block mismatch immediately after input. At local tick **1** / absolute tick **820**, wire **`(11,17,14)`** is interpreter power **13**, Rust **0**.

The failing interpreter replay stops there; actions64–65 were not reached in that failing run. The independent Rust/LLVM parity test and MAIR trace used the complete saved action sequence.

## What the trace proves

Compiled execution **does invoke** `UpdateComparator` for `(12,14,13)` at the earlier disagreement. It is nonreactive, has three incoming links, rear 12, side 13, stored/calculated output 0 and powered false, so it schedules no tick. The interpreter emits a callback and schedules a tick, but its block-entity output at that exact callback was not captured. A missing source edge is therefore not established as the explanation.

At action63, the wire's compiled inputs are comparators `(13,17,15)` and `(15,17,15)`. The latter's contribution drops from 9 to 0 in the failing tick. The relationship between the earlier omitted comparator tick and this wire mismatch remains **unproven**.

The failing lever's first and third ON probe frames have identical blocks and identical full callback lists (45 events). Their pending queues differ: the later frame additionally includes comparator `(15,17,15)` and torches `(17,17,16)` and `(11,17,16)`. The `repeated transition differs` warning is about pending work here, not a changed callback order.

Relevant code: [executor.rs](../../../crates/core/src/redpiler/sampled/executor.rs), `update_comparator` / `comparator_result` / `tick_comparator`; [source_order.rs](../../../crates/core/src/redpiler/sampled/passes/source_order.rs), `Frame` and repeated-edge comparison. Next diagnostic: capture native stored output and numeric rear/side inputs at the action3 callback, then follow the causal path to action63. Broadening source-order edges is not justified by current evidence.

## MAIR, LLVM, and saved evidence

MAIR reproduces the **same action63/tick1/position/power** failure. LLVM semantic 19 matches Rust across all 834 ticks, including projected states, effects and ordered pending entries. No fix or unverified candidate was merged.

- [Baseline trace](evidence/action63-baseline.log), [current causal trace](evidence/action63-current.log), [baseline diagnostic patch](evidence/action63-baseline-diagnostics.patch).
- [MAIR result](evidence/mair/2735cc14b1051cab4197df60265878b2.final.json).
- [LLVM/Rust full trace](evidence/llvm-four-fixtures-parity.log).

## Complete recorded action sequence

Toggle the listed lever, then execute the listed number of ticks. The final column is the absolute tick immediately before the toggle.

| Action | Lever index | World position | Wait ticks | Tick before input |
| ---: | ---: | --- | ---: | ---: |
| 0 | 0 | (14, 9, 9) | 20 | 0 |
| 1 | 1 | (13, 11, 18) | 20 | 20 |
| 2 | 2 | (15, 11, 18) | 20 | 40 |
| 3 | 3 | (13, 13, 18) | 20 | 60 |
| 4 | 4 | (15, 13, 18) | 20 | 80 |
| 5 | 5 | (13, 15, 18) | 20 | 100 |
| 6 | 6 | (15, 15, 18) | 20 | 120 |
| 7 | 7 | (13, 17, 18) | 20 | 140 |
| 8 | 8 | (15, 17, 18) | 20 | 160 |
| 9 | 9 | (13, 19, 18) | 20 | 180 |
| 10 | 10 | (15, 19, 18) | 20 | 200 |
| 11 | 11 | (13, 21, 18) | 20 | 220 |
| 12 | 12 | (15, 21, 18) | 20 | 240 |
| 13 | 13 | (13, 23, 18) | 20 | 260 |
| 14 | 14 | (15, 23, 18) | 20 | 280 |
| 15 | 15 | (13, 25, 18) | 20 | 300 |
| 16 | 16 | (15, 25, 18) | 20 | 320 |
| 17 | 16 | (15, 25, 18) | 20 | 340 |
| 18 | 15 | (13, 25, 18) | 20 | 360 |
| 19 | 14 | (15, 23, 18) | 20 | 380 |
| 20 | 13 | (13, 23, 18) | 20 | 400 |
| 21 | 12 | (15, 21, 18) | 20 | 420 |
| 22 | 11 | (13, 21, 18) | 20 | 440 |
| 23 | 10 | (15, 19, 18) | 20 | 460 |
| 24 | 9 | (13, 19, 18) | 20 | 480 |
| 25 | 8 | (15, 17, 18) | 20 | 500 |
| 26 | 7 | (13, 17, 18) | 20 | 520 |
| 27 | 6 | (15, 15, 18) | 20 | 540 |
| 28 | 5 | (13, 15, 18) | 20 | 560 |
| 29 | 4 | (15, 13, 18) | 20 | 580 |
| 30 | 3 | (13, 13, 18) | 20 | 600 |
| 31 | 2 | (15, 11, 18) | 20 | 620 |
| 32 | 1 | (13, 11, 18) | 20 | 640 |
| 33 | 0 | (14, 9, 9) | 20 | 660 |
| 34 | 5 | (13, 15, 18) | 7 | 680 |
| 35 | 14 | (15, 23, 18) | 2 | 687 |
| 36 | 13 | (13, 23, 18) | 6 | 689 |
| 37 | 1 | (13, 11, 18) | 3 | 695 |
| 38 | 3 | (13, 13, 18) | 8 | 698 |
| 39 | 8 | (15, 17, 18) | 6 | 706 |
| 40 | 6 | (15, 15, 18) | 6 | 712 |
| 41 | 9 | (13, 19, 18) | 6 | 718 |
| 42 | 10 | (15, 19, 18) | 6 | 724 |
| 43 | 3 | (13, 13, 18) | 3 | 730 |
| 44 | 12 | (15, 21, 18) | 2 | 733 |
| 45 | 11 | (13, 21, 18) | 2 | 735 |
| 46 | 14 | (15, 23, 18) | 3 | 737 |
| 47 | 15 | (13, 25, 18) | 8 | 740 |
| 48 | 16 | (15, 25, 18) | 6 | 748 |
| 49 | 0 | (14, 9, 9) | 3 | 754 |
| 50 | 12 | (15, 21, 18) | 8 | 757 |
| 51 | 8 | (15, 17, 18) | 5 | 765 |
| 52 | 5 | (13, 15, 18) | 1 | 770 |
| 53 | 11 | (13, 21, 18) | 5 | 771 |
| 54 | 9 | (13, 19, 18) | 1 | 776 |
| 55 | 1 | (13, 11, 18) | 1 | 777 |
| 56 | 3 | (13, 13, 18) | 8 | 778 |
| 57 | 5 | (13, 15, 18) | 8 | 786 |
| 58 | 15 | (13, 25, 18) | 7 | 794 |
| 59 | 6 | (15, 15, 18) | 5 | 801 |
| 60 | 5 | (13, 15, 18) | 7 | 806 |
| 61 | 7 | (13, 17, 18) | 5 | 813 |
| 62 | 7 | (13, 17, 18) | 1 | 818 |
| 63 | 8 | (15, 17, 18) | 8 | 819 |
| 64 | 2 | (15, 11, 18) | 5 | 827 |
| 65 | 11 | (13, 21, 18) | 2 | 832 |
