# Settled circuit sample results

The ignored O6 inventory ran against all 19 `TEST_*.schem` files under `test_data/piston-research`. It restored the archived control levers and bottom supports for the prefix fixtures, checked listed download-manifest hashes, prepared each graph, ran the O6 pipeline, and compiled settled output equations. These counts include feed-forward repeater inlining and canonicalized input leaves. They describe the compiler artifact; they do not claim that replacing the physical graph with these equations is correct.

`outputs` counts published power/register roots. `effects` counts explicit boundary effects retained for the physical executor. `recorded outputs/samples` comes from the archived `output-summary.json`; those historic observations are not revalidated by this inventory.

| Sample | Cells | Inputs | Exprs | Outputs | Registers | Effects | Declines | Recorded outputs / samples |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `TEST_POTADOS_PC_COUNTER` | 340 | 306 | 2,772 | 135 | 0 | 1,073 | 0 | — |
| `TEST_BUD_1` | 0 | 8 | 10 | 18 | 0 | 0 | 0 | 1 / 19 |
| `TEST_BUD_2` | 0 | 7 | 9 | 18 | 0 | 0 | 0 | 1 / 19 |
| `TEST_CODER_1` | 2 | 7 | 12 | 11 | 0 | 44 | 0 | 6 / 48 |
| `TEST_DEC` | 4 | 4 | 42 | 4 | 0 | 16 | 0 | 2 / 32 |
| `TEST_DEC2` | 9 | 7 | 67 | 10 | 0 | 54 | 0 | 4 / 64 |
| `TEST_INSTANT_3` | 1 | 2 | 3 | 3 | 0 | 14 | 0 | 2 / 16 |
| `TEST_OR1` | 1 | 4 | 5 | 3 | 0 | 13 | 0 | 1 / 16 |
| `TEST_OR2` | 1 | 4 | 5 | 3 | 0 | 13 | 0 | 1 / 16 |
| `TEST_OR_3` | 1 | 5 | 9 | 3 | 0 | 22 | 0 | 1 / 16 |
| `TEST_PISTION` | 1 | 4 | 4 | 5 | 0 | 11 | 0 | 6 / 48 |
| `TEST_PISTION_2` | 1 | 2 | 2 | 7 | 0 | 11 | 0 | 6 / 48 |
| `TEST_WEIRD` | 0 | 9 | 23 | 13 | 0 | 0 | 1 | 1 / 8 |
| `TEST_WEIRD_3` | 1 | 2 | 2 | 3 | 0 | 14 | 0 | 2 / 16 |
| `TEST_WEIRD_INSTANT` | 0 | 5 | 7 | 11 | 0 | 0 | 0 | 1 / 8 |
| `TEST_WEIRD_INSTANT_2` | 0 | 4 | 6 | 9 | 0 | 0 | 1 | 1 / 8 |
| `TEST_WEIRD_INSTANT_3` | 0 | 4 | 6 | 12 | 0 | 0 | 1 | 1 / 8 |
| `TEST_XOR` | 2 | 4 | 41 | 3 | 0 | 13 | 0 | 1 / 16 |
| `TEST_XOR2` | 1 | 4 | 33 | 3 | 0 | 9 | 0 | 1 / 16 |

Sixteen of the nineteen graphs produced equations for every requested settled output. These are supported candidate graphs, not evidence that all sixteen are safe to optimize or that graph execution can be dropped. Thirteen prefix samples are covered by the existing active-control native replay suite, which compares the native interpreter and graph backend. The full Redpiler regression with repeater inlining passed 631 tests with 74 ignored (`target/logical-inference/settled-redpiler-repeater.log`). This validates the existing graph fallback on those histories, not the new settled equations. The inventory itself did not rerun those histories. The remaining prefix samples have archived native-output samples but no corresponding active-control replay case in that suite.

`TEST_WEIRD`, `TEST_WEIRD_INSTANT_2`, and `TEST_WEIRD_INSTANT_3` each have one expected exclusion from the instant model: a Quasi-input corridor is sourced by an electrically active contact component. The schematics parse and their graphs prepare; only that instant input query is outside the instant model, with ordinary graph execution retained. `TEST_WEIRD_INSTANT` has no decline, but also has zero recognized cells, so it is ordinary classic equation coverage rather than a compiled instant-piston cell.

No sample produced a settled register output. The BUD-named fixtures have zero recognized cells, and the larger `TEST_POTADOS_PC_COUNTER` has 340 recognized cells but still zero logical register roots. Sequential storage therefore remains outside this artifact. The 1,073 retained effects in that larger sample also show why its zero decline count cannot be read as a self-contained device kernel: the classical and external notification boundary is substantial. Across the smaller recognized-cell samples, 9–54 effects remain explicit as well.

The separate 11-bit adder extraction now reports 1,083 graph nodes, 23 inputs, 731 expressions, 22 outputs, 132 retained effects, and zero declines after canonicalizing repeated source-threshold inputs and inlining feed-forward repeaters. Those 23 inputs are the 22 operand bits and the trigger. Locked and feedback repeaters remain state boundaries when present, as checked by a separate focused test. Its settled DAG matched all 7 native carry/overflow vectors again. A counter graph with the external observer boundary retained reports 1,738 nodes, 20 inputs, 609 expressions, 35 outputs, 16 register outputs, 107 effects, and zero declines. These native vector checks support the sampled settled equations; they do not establish full stateful handoff correctness.
