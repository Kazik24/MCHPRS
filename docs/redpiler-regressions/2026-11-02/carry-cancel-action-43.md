# 8-bit Carry Cancel Adder — action 43

Status: reproduced on baseline `6ef1b165` and current `ea297c16`; no fix applied. Use the [common reproduction instructions](README.md).

Fixture: `2cdc24e68c78d7f0cdce7a141648385a`, [schematic](../../../test_data/redstonedb/schematics/2cdc24e68c78d7f0cdce7a141648385a-8_bit_Carry_Cancel_Adder.schem).
SHA-256: `e3743cd5b7754f51817d54b38c539824d43581a76d96607d2bb3b1ee518fe17b`.
Trace: 17 levers, 66 actions, 813 ticks.

## Exact disagreements

First observed pending FIFO difference: **action 18**, lever **`(8,24,21)`**, immediately after input, absolute tick **360**. Both entries have delay 2:

| Interpreter order | Sampled order |
| --- | --- |
| Comparator `(10,24,18)` | Wall torch `(13,24,16)` |
| Wall torch `(13,24,16)` | Comparator `(10,24,18)` |

First checked visible failure: action **43**, lever index **12**, **`(11,20,21)`**, wait **2**, starting after absolute tick **716**. At local tick **1** / absolute tick **717**, wire **`(11,15,9)`** is interpreter power **14**, sampled **0**.

At action43's input, repeater `(10,15,10)` is scheduled after 1 tick by the interpreter but after 2 by sampled Rust. After local tick 1, native has consumed that event while Rust still has it pending. The first queue defect and this later delay defect are distinct observations; their causal connection has not been established.

## Proven callback-filter defect

Native action18 callbacks reach comparator `(10,24,18)` before torch `(13,24,16)`. The comparator callback has `input_on=(true,true)` while the lever edge is false. The source-order filter rejects it because target input stays on, then fallback target order places the comparator after the torch.

The lowered off program on **both** revisions orders comparator `(9,24,18)`, torch `(13,24,16)`, then comparator `(10,24,18)`. The target already has a contribution in the source graph. This is not a missing graph edge or a scheduler FIFO implementation failure.

Relevant code: [source_order.rs](../../../crates/core/src/redpiler/sampled/passes/source_order.rs), observed callback filtering around lines 410–419 and fallback insertion around 424, at `ea297c16`.

The action43 source probe's repeated ON frames have the same blocks but different pending work: first sample queues comparator `(9,20,18)`; third additionally queues wall torch `(13,20,16)`. That warning is supplementary evidence, not proof of the action43 cause.

## MAIR, LLVM, and candidate status

MAIR reproduces the **same action43/tick1/position/power** disagreement with its own interpreter. LLVM semantic 19 matches Rust for all 813 ticks, including pending order and effects.

An isolated, **unbuilt and untested** candidate accepts observed callbacks despite `input_on` mismatch when the source record contains a `Contribution` or `ArrangementContribution`. It was not merged or used for the recorded replay. Such a change must preserve callback timing relative to contribution changes and be checked against the full prefix, ZCore and other circuits. Fixing action18's order has not been shown to fix action43.

## Evidence

- [Baseline replay](evidence/action43-baseline.log), [current replay](evidence/action43-current.log), [baseline diagnostic patch](evidence/action43-baseline-diagnostics.patch).
- [MAIR result](evidence/mair/2cdc24e68c78d7f0cdce7a141648385a.final.json).
- [LLVM/Rust full trace](evidence/llvm-four-fixtures-parity.log).

The source-only candidate remains in the original temporary `target/action43-investigation-20261102/current` worktree. The archived patch is the tested baseline instrumentation, not that proposed fix.

## Complete recorded action sequence

Toggle the listed lever, then execute the listed number of ticks. The final column is the absolute tick immediately before the toggle.

| Action | Lever index | World position | Wait ticks | Tick before input |
| ---: | ---: | --- | ---: | ---: |
| 0 | 0 | (13, 9, 14) | 20 | 0 |
| 1 | 1 | (8, 10, 21) | 20 | 20 |
| 2 | 2 | (11, 10, 21) | 20 | 40 |
| 3 | 3 | (8, 12, 21) | 20 | 60 |
| 4 | 4 | (11, 12, 21) | 20 | 80 |
| 5 | 5 | (8, 14, 21) | 20 | 100 |
| 6 | 6 | (11, 14, 21) | 20 | 120 |
| 7 | 7 | (8, 16, 21) | 20 | 140 |
| 8 | 8 | (11, 16, 21) | 20 | 160 |
| 9 | 9 | (8, 18, 21) | 20 | 180 |
| 10 | 10 | (11, 18, 21) | 20 | 200 |
| 11 | 11 | (8, 20, 21) | 20 | 220 |
| 12 | 12 | (11, 20, 21) | 20 | 240 |
| 13 | 13 | (8, 22, 21) | 20 | 260 |
| 14 | 14 | (11, 22, 21) | 20 | 280 |
| 15 | 15 | (8, 24, 21) | 20 | 300 |
| 16 | 16 | (11, 24, 21) | 20 | 320 |
| 17 | 16 | (11, 24, 21) | 20 | 340 |
| 18 | 15 | (8, 24, 21) | 20 | 360 |
| 19 | 14 | (11, 22, 21) | 20 | 380 |
| 20 | 13 | (8, 22, 21) | 20 | 400 |
| 21 | 12 | (11, 20, 21) | 20 | 420 |
| 22 | 11 | (8, 20, 21) | 20 | 440 |
| 23 | 10 | (11, 18, 21) | 20 | 460 |
| 24 | 9 | (8, 18, 21) | 20 | 480 |
| 25 | 8 | (11, 16, 21) | 20 | 500 |
| 26 | 7 | (8, 16, 21) | 20 | 520 |
| 27 | 6 | (11, 14, 21) | 20 | 540 |
| 28 | 5 | (8, 14, 21) | 20 | 560 |
| 29 | 4 | (11, 12, 21) | 20 | 580 |
| 30 | 3 | (8, 12, 21) | 20 | 600 |
| 31 | 2 | (11, 10, 21) | 20 | 620 |
| 32 | 1 | (8, 10, 21) | 20 | 640 |
| 33 | 0 | (13, 9, 14) | 20 | 660 |
| 34 | 7 | (8, 16, 21) | 7 | 680 |
| 35 | 6 | (11, 14, 21) | 1 | 687 |
| 36 | 15 | (8, 24, 21) | 7 | 688 |
| 37 | 4 | (11, 12, 21) | 2 | 695 |
| 38 | 16 | (11, 24, 21) | 7 | 697 |
| 39 | 14 | (11, 22, 21) | 2 | 704 |
| 40 | 13 | (8, 22, 21) | 2 | 706 |
| 41 | 3 | (8, 12, 21) | 1 | 708 |
| 42 | 7 | (8, 16, 21) | 7 | 709 |
| 43 | 12 | (11, 20, 21) | 2 | 716 |
| 44 | 1 | (8, 10, 21) | 1 | 718 |
| 45 | 3 | (8, 12, 21) | 4 | 719 |
| 46 | 5 | (8, 14, 21) | 5 | 723 |
| 47 | 0 | (13, 9, 14) | 3 | 728 |
| 48 | 4 | (11, 12, 21) | 7 | 731 |
| 49 | 13 | (8, 22, 21) | 3 | 738 |
| 50 | 4 | (11, 12, 21) | 4 | 741 |
| 51 | 2 | (11, 10, 21) | 1 | 745 |
| 52 | 1 | (8, 10, 21) | 7 | 746 |
| 53 | 10 | (11, 18, 21) | 5 | 753 |
| 54 | 4 | (11, 12, 21) | 1 | 758 |
| 55 | 3 | (8, 12, 21) | 7 | 759 |
| 56 | 1 | (8, 10, 21) | 8 | 766 |
| 57 | 9 | (8, 18, 21) | 5 | 774 |
| 58 | 12 | (11, 20, 21) | 4 | 779 |
| 59 | 1 | (8, 10, 21) | 7 | 783 |
| 60 | 5 | (8, 14, 21) | 3 | 790 |
| 61 | 16 | (11, 24, 21) | 4 | 793 |
| 62 | 11 | (8, 20, 21) | 1 | 797 |
| 63 | 1 | (8, 10, 21) | 5 | 798 |
| 64 | 2 | (11, 10, 21) | 4 | 803 |
| 65 | 11 | (8, 20, 21) | 6 | 807 |
