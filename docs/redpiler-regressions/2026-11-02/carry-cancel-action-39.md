# Eight-bit carry-cancel adder — action 39

Status: reproduced on baseline `6ef1b165` and current `ea297c16`; no fix applied. Use the [common reproduction instructions](README.md).

Fixture: `8111b000090d8f0b3e1370448bcb64a3`, [schematic](../../../test_data/redstonedb/schematics/8111b000090d8f0b3e1370448bcb64a3-8_bit_Carry_Cancel_Adder.schem).
SHA-256: `19b2b16bb84371f320e9c7ac5d6fdac38fe6a0e248f1ba0f03eb12f623f9175d`.
Trace: 17 levers, 66 actions, 831 ticks; no initial settling.

## Exact disagreement

Action **39**, lever index **1**, position **`(14,10,28)`**, wait **5**. The action starts after absolute tick **694**. At local tick **5**, absolute tick **699**, wire **`(14,9,8)`** has interpreter power **1**, sampled Rust power **0**. The unchanged full prefix is required.

## Proven causal evidence

| Local tick within action 39 | Observation |
| --- | --- |
| 2 | Comparator source `(10,10,11)` outputs 7. It feeds comparator `(13,9,9)` on rear/side paths with attenuation 4/3, giving rear 3 and side 4. |
| 3 | Source output falls 7→0. Native callbacks expose rear/side on/on → on/off → off/off. The intermediate rear 3, side 0 schedules comparator `(13,9,9)` for local tick 5. |
| 3 | Sampled execution updates side input and batches rear contributions before `UpdateComparator`; it sees settled 0/0 and does not schedule that tick. |
| 4 | Rear becomes 1. Sampled execution now schedules the comparator for tick 6; native already has its earlier pending tick. |
| 5 | Native comparator output/powered state becomes 1/true, feeding wire power 1. Sampled output/power remains 0. |

The comparator at `(14,9,9)` is false in both engines at the failure. The differing direct source is `(13,9,9)`, which has no rear or far override. Another comparator `(11,11,12)` supplies stable strength 7 and faces a redstone-stocked barrel at `(11,11,13)`; container state influences the surrounding circuit, but the failing target itself has no override.

Relevant existing code: [executor.rs](../../../crates/core/src/redpiler/sampled/executor.rs), `set_component` (line 217), `update_side_inputs` (780), `execute_output_program` (1383), `update_comparator` (1531), and `tick_comparator` (1550), as recorded at `ea297c16`.

An earlier pending-order difference was observed at action 18, but the investigation identified action39/tick1 as the first additional semantic scheduling difference relevant to this trace: a native comparator tick at `(9,10,11)` is absent in Rust. The proven target-level explanation is the lost local-tick-3 pulse above. Do not equate every earlier FIFO difference with the eventual visible failure.

## MAIR and LLVM

MAIR's own interpreter and compiled engine agree throughout all 831 ticks. At this exact failing checkpoint they both produce power **0**, while MCHPRS's interpreter produces **1**. Before action 39, both MAIR engines show target power 1; it becomes 0 on tick 5. Thus MAIR does not reproduce the same internal compiler disagreement, and its reference execution cannot decide which interpreter is correct here.

LLVM semantic 19 matches sampled Rust throughout the complete trace, including projected states, effects and ordered pending entries. This is a shared MCHPRS interpreter/compiler gap, not a demonstrated LLVM-only defect.

## Evidence and next step

- [Baseline trace](evidence/action39-baseline.log), [current trace](evidence/action39-current.log), [diagnostic patch](evidence/action39-baseline-diagnostics.patch).
- [MAIR full result](evidence/mair/8111b000090d8f0b3e1370448bcb64a3.final.json), [source/pending checkpoint](evidence/mair/8111b000090d8f0b3e1370448bcb64a3.source-result.json).
- [LLVM/Rust full-trace result](evidence/llvm-four-fixtures-parity.log).

Candidate to validate: preserve the comparator callback between side-link and rear-link changes when one source supplies both paths. Use existing indexed graph operations and scheduling. No candidate was implemented or validated against other dual-path circuits; do not generalize this ordering to every comparator without replay evidence.

## Complete recorded action sequence

Toggle the listed lever, then execute the listed number of ticks. The final column is the absolute tick immediately before the toggle.

| Action | Lever index | World position | Wait ticks | Tick before input |
| ---: | ---: | --- | ---: | ---: |
| 0 | 0 | (8, 10, 27) | 20 | 0 |
| 1 | 1 | (14, 10, 28) | 20 | 20 |
| 2 | 2 | (10, 11, 28) | 20 | 40 |
| 3 | 3 | (8, 12, 27) | 20 | 60 |
| 4 | 4 | (10, 13, 28) | 20 | 80 |
| 5 | 5 | (8, 14, 27) | 20 | 100 |
| 6 | 6 | (10, 15, 28) | 20 | 120 |
| 7 | 7 | (8, 16, 27) | 20 | 140 |
| 8 | 8 | (10, 17, 28) | 20 | 160 |
| 9 | 9 | (8, 18, 27) | 20 | 180 |
| 10 | 10 | (10, 19, 28) | 20 | 200 |
| 11 | 11 | (8, 20, 27) | 20 | 220 |
| 12 | 12 | (10, 21, 28) | 20 | 240 |
| 13 | 13 | (8, 22, 27) | 20 | 260 |
| 14 | 14 | (10, 23, 28) | 20 | 280 |
| 15 | 15 | (8, 24, 27) | 20 | 300 |
| 16 | 16 | (10, 25, 28) | 20 | 320 |
| 17 | 16 | (10, 25, 28) | 20 | 340 |
| 18 | 15 | (8, 24, 27) | 20 | 360 |
| 19 | 14 | (10, 23, 28) | 20 | 380 |
| 20 | 13 | (8, 22, 27) | 20 | 400 |
| 21 | 12 | (10, 21, 28) | 20 | 420 |
| 22 | 11 | (8, 20, 27) | 20 | 440 |
| 23 | 10 | (10, 19, 28) | 20 | 460 |
| 24 | 9 | (8, 18, 27) | 20 | 480 |
| 25 | 8 | (10, 17, 28) | 20 | 500 |
| 26 | 7 | (8, 16, 27) | 20 | 520 |
| 27 | 6 | (10, 15, 28) | 20 | 540 |
| 28 | 5 | (8, 14, 27) | 20 | 560 |
| 29 | 4 | (10, 13, 28) | 20 | 580 |
| 30 | 3 | (8, 12, 27) | 20 | 600 |
| 31 | 2 | (10, 11, 28) | 20 | 620 |
| 32 | 1 | (14, 10, 28) | 20 | 640 |
| 33 | 0 | (8, 10, 27) | 20 | 660 |
| 34 | 13 | (8, 22, 27) | 1 | 680 |
| 35 | 9 | (8, 18, 27) | 6 | 681 |
| 36 | 0 | (8, 10, 27) | 2 | 687 |
| 37 | 6 | (10, 15, 28) | 4 | 689 |
| 38 | 0 | (8, 10, 27) | 1 | 693 |
| 39 | 1 | (14, 10, 28) | 5 | 694 |
| 40 | 6 | (10, 15, 28) | 6 | 699 |
| 41 | 15 | (8, 24, 27) | 8 | 705 |
| 42 | 0 | (8, 10, 27) | 7 | 713 |
| 43 | 1 | (14, 10, 28) | 7 | 720 |
| 44 | 1 | (14, 10, 28) | 3 | 727 |
| 45 | 12 | (10, 21, 28) | 1 | 730 |
| 46 | 1 | (14, 10, 28) | 5 | 731 |
| 47 | 0 | (8, 10, 27) | 6 | 736 |
| 48 | 14 | (10, 23, 28) | 8 | 742 |
| 49 | 1 | (14, 10, 28) | 5 | 750 |
| 50 | 2 | (10, 11, 28) | 5 | 755 |
| 51 | 13 | (8, 22, 27) | 1 | 760 |
| 52 | 9 | (8, 18, 27) | 4 | 761 |
| 53 | 5 | (8, 14, 27) | 8 | 765 |
| 54 | 3 | (8, 12, 27) | 3 | 773 |
| 55 | 1 | (14, 10, 28) | 7 | 776 |
| 56 | 6 | (10, 15, 28) | 2 | 783 |
| 57 | 6 | (10, 15, 28) | 2 | 785 |
| 58 | 2 | (10, 11, 28) | 7 | 787 |
| 59 | 13 | (8, 22, 27) | 7 | 794 |
| 60 | 9 | (8, 18, 27) | 5 | 801 |
| 61 | 4 | (10, 13, 28) | 3 | 806 |
| 62 | 11 | (8, 20, 27) | 3 | 809 |
| 63 | 9 | (8, 18, 27) | 7 | 812 |
| 64 | 1 | (14, 10, 28) | 6 | 819 |
| 65 | 7 | (8, 16, 27) | 6 | 825 |
