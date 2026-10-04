# Signed adder fixture test

Tested 2026-10-04 on the repaired engine and the official Java 1.21.5 server.

The test loads `ADDER_GWIEZDNY_TEST.schem`, reads the front sign labels and uses the block below each sign. A1/A2, B1/B2 and O1/O2 define the stride; TICK defines the trigger. Inputs decode redstone blocks as zero and other solid blocks as one. Outputs decode redstone blocks as zero and air as one; moving outputs are explicitly unsettled.

| Marker pair | First local block | Next local block | Stride |
| --- | --- | --- | --- |
| A1/A2 | (3,5,42) | (3,5,38) | (0,0,-4) |
| B1/B2 | (3,5,40) | (3,5,36) | (0,0,-4) |
| O1/O2 | (20,2,40) | (20,2,36) | (0,0,-4) |
| TICK | (2,4,44) | — | Remove this redstone block |

The fixture measures 21×7×45. Eleven interpolated A/B/output positions fit its bounds; the twelfth already leaves the schematic. The requested sixteen-bit extrapolation is therefore unavailable in this file. The tests cover the supplied cells without inventing five additional stages or carry pins. Clarification of a different layout or a complete sixteen-bit asset is still needed for that coverage.

The stored inputs are A=63 and B=1. Removing TICK calculates 64 during the first game tick. The output is not a permanently settled register: the unmodified Java fixture subsequently cycles through `64,64,64,moving,moving,0` and repeats. Testing only the final settled state would incorrectly reject this working computation.

Four unit tests cover the marked inputs, first-tick arithmetic, the twelve-tick Java output/movement trace, low-bit arithmetic vectors, and a changed-input reference comparison. They all pass. The tests retain actual moving states and do not substitute zeros for them.

A further fixture limitation is recorded openly: setting A=0x555 and B=0x2aa, settling eight ticks with TICK held, then removing it produces **1087** on both Java and MCHPRS; the mathematical sum is **2047**. Matching this recording checks engine/reference behavior, not correct general eleven- or sixteen-bit arithmetic. A complete arithmetic validation requires resolving this circuit/layout issue.

Reference recordings: [stored-input trace](piston-repair/java-adder-traces.json) and [changed-input trace](piston-repair/java-adder-inputs.json). The SHA-pinned capture tool runs entirely in a temporary world and never modifies the live server.
