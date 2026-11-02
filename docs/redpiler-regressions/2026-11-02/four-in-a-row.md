# Four In A Row Detector — stale wire rail

Status: reproduced on baseline `6ef1b165` and in the current `ea297c16` suite; no fix applied. Use the [common reproduction instructions](README.md).

Fixture: `b6a6e870229ace7eb2afd8cb387a514e`, [schematic](../../../test_data/redstonedb/schematics/b6a6e870229ace7eb2afd8cb387a514e-Four_In_A_Row_Detector.schem).
SHA-256: `7cc18d21ad5c43873330a4196a5c19ea6efc7bdcf95d2f24068a7464ce8621bf`.
Trace: 84 levers, 200 actions, 3,496 ticks. The failure is in the fixed initial sweep, before the seeded tail.

## Exact disagreement

Toggle action0, lever index0, then run20 ticks. Toggle **action1**, lever index **1**, **`(10,9,27)`**, then run20 ticks. Immediately after input and at tick1, checked states agree. At local tick **2**, absolute tick **22**:

| Position | Interpreter power | Sampled power |
| --- | ---: | ---: |
| `(12,9,10)` | 0 | 1 |
| `(12,9,11)` | 0 | 2 |
| `(12,9,12)` | 0 | 3 |
| `(12,9,13)` | 0 | 4 |

This retains the schematic's initial powers; clearing them or settling first would change the reproduction.

## Trigger and compiled graph gap

At tick2, wall torch **`(10,9,23)`** changes off. Its notifications reach wire **`(10,9,22)`**, reported with old power15 and direction South. This is the first observed wire callback entering native wire handling. Subsequent native turbo work clears the rail above; sampled execution leaves its captured powers unchanged.

At the failing target `(12,9,10)`, sampled `input_strength=0` and `has_input=false`. Its only incoming electrical dependency is repeater `(12,9,23)`, attenuation12, contribution0. Thus stored wire power1 is stale even though effective input is already zero. The source output program has no electrical refresh dependency for that far rail. `RefreshWire` updates its named wire only.

Earlier speculation that the lever's immediate callback caused the divergence was disproved: its callback at `(10,9,25)` has old power0 and post-use state still matches. Tick2's torch transition is the observed trigger.

Relevant code:

- [redstone/mod.rs](../../../crates/core/src/redstone/mod.rs): `on_torch_state_change`, `surrounding_notifications`.
- [wire/mod.rs](../../../crates/core/src/redstone/wire/mod.rs): `on_neighbor_updated` starts turbo work only after a power change.
- [wire/turbo.rs](../../../crates/core/src/redstone/wire/turbo.rs): `update_surrounding_neighbors` and `update_node`.
- [sampled/executor.rs](../../../crates/core/src/redpiler/sampled/executor.rs): `refresh_wire_power` and `execute_output_program`.
- [source_order.rs](../../../crates/core/src/redpiler/sampled/passes/source_order.rs): wire deliveries are skipped; the quiet sampling basis clears the stale powers before probing.

The outer callback was captured; internal turbo-node traversal and the first seed's exact old→new power were not independently instrumented. Do not infer their complete ordered trace from the outer observation API.

## Candidate and comparisons

A candidate is a fixed, ordered wire-update plan compiled from the existing graph, keyed to the callback seed and gated on that wire's power change. It must preserve meaningful repetitions, watcher checks, FIFO, operation charging and failure handling. An unconditional component-wide refresh could clear the rail too early and is not validated. No runtime world-wire walk, interpreter tick, or general event engine is proposed. This candidate remains unimplemented and untested.

MAIR reproduces the same rail discrepancy at action1/tick2; its post-use and tick1 checks also agree. LLVM semantic19 matches Rust through the complete 3,496-tick trace. Both sampled backends still share this interpreter disagreement.

Evidence: [baseline callbacks](evidence/four-in-a-row-baseline.log), [current suite failure](evidence/redpiler-suite.log), [baseline diagnostic patch](evidence/four-in-a-row-baseline-diagnostics.patch), [MAIR result](evidence/mair/b6a6e870229ace7eb2afd8cb387a514e.final.json), [LLVM/Rust parity](evidence/llvm-four-fixtures-parity.log).

## Complete recorded action sequence

Toggle the listed lever, then execute the listed number of ticks. The final column is the absolute tick immediately before the toggle.

| Action | Lever index | World position | Wait ticks | Tick before input |
| ---: | ---: | --- | ---: | ---: |
| 0 | 0 | (8, 9, 27) | 20 | 0 |
| 1 | 1 | (10, 9, 27) | 20 | 20 |
| 2 | 2 | (12, 9, 27) | 20 | 40 |
| 3 | 3 | (14, 9, 27) | 20 | 60 |
| 4 | 4 | (16, 9, 27) | 20 | 80 |
| 5 | 5 | (18, 9, 27) | 20 | 100 |
| 6 | 6 | (20, 9, 27) | 20 | 120 |
| 7 | 7 | (22, 9, 27) | 20 | 140 |
| 8 | 8 | (24, 9, 27) | 20 | 160 |
| 9 | 9 | (26, 9, 27) | 20 | 180 |
| 10 | 10 | (28, 9, 27) | 20 | 200 |
| 11 | 11 | (30, 9, 27) | 20 | 220 |
| 12 | 12 | (32, 9, 27) | 20 | 240 |
| 13 | 13 | (34, 9, 27) | 20 | 260 |
| 14 | 14 | (8, 13, 27) | 20 | 280 |
| 15 | 15 | (10, 13, 27) | 20 | 300 |
| 16 | 16 | (12, 13, 27) | 20 | 320 |
| 17 | 17 | (14, 13, 27) | 20 | 340 |
| 18 | 18 | (16, 13, 27) | 20 | 360 |
| 19 | 19 | (18, 13, 27) | 20 | 380 |
| 20 | 20 | (20, 13, 27) | 20 | 400 |
| 21 | 21 | (22, 13, 27) | 20 | 420 |
| 22 | 22 | (24, 13, 27) | 20 | 440 |
| 23 | 23 | (26, 13, 27) | 20 | 460 |
| 24 | 24 | (28, 13, 27) | 20 | 480 |
| 25 | 25 | (30, 13, 27) | 20 | 500 |
| 26 | 26 | (32, 13, 27) | 20 | 520 |
| 27 | 27 | (34, 13, 27) | 20 | 540 |
| 28 | 28 | (8, 17, 27) | 20 | 560 |
| 29 | 29 | (10, 17, 27) | 20 | 580 |
| 30 | 30 | (12, 17, 27) | 20 | 600 |
| 31 | 31 | (14, 17, 27) | 20 | 620 |
| 32 | 32 | (16, 17, 27) | 20 | 640 |
| 33 | 33 | (18, 17, 27) | 20 | 660 |
| 34 | 34 | (20, 17, 27) | 20 | 680 |
| 35 | 35 | (22, 17, 27) | 20 | 700 |
| 36 | 36 | (24, 17, 27) | 20 | 720 |
| 37 | 37 | (26, 17, 27) | 20 | 740 |
| 38 | 38 | (28, 17, 27) | 20 | 760 |
| 39 | 39 | (30, 17, 27) | 20 | 780 |
| 40 | 40 | (32, 17, 27) | 20 | 800 |
| 41 | 41 | (34, 17, 27) | 20 | 820 |
| 42 | 42 | (8, 21, 27) | 20 | 840 |
| 43 | 43 | (10, 21, 27) | 20 | 860 |
| 44 | 44 | (12, 21, 27) | 20 | 880 |
| 45 | 45 | (14, 21, 27) | 20 | 900 |
| 46 | 46 | (16, 21, 27) | 20 | 920 |
| 47 | 47 | (18, 21, 27) | 20 | 940 |
| 48 | 48 | (20, 21, 27) | 20 | 960 |
| 49 | 49 | (22, 21, 27) | 20 | 980 |
| 50 | 50 | (24, 21, 27) | 20 | 1000 |
| 51 | 51 | (26, 21, 27) | 20 | 1020 |
| 52 | 52 | (28, 21, 27) | 20 | 1040 |
| 53 | 53 | (30, 21, 27) | 20 | 1060 |
| 54 | 54 | (32, 21, 27) | 20 | 1080 |
| 55 | 55 | (34, 21, 27) | 20 | 1100 |
| 56 | 56 | (8, 25, 27) | 20 | 1120 |
| 57 | 57 | (10, 25, 27) | 20 | 1140 |
| 58 | 58 | (12, 25, 27) | 20 | 1160 |
| 59 | 59 | (14, 25, 27) | 20 | 1180 |
| 60 | 60 | (16, 25, 27) | 20 | 1200 |
| 61 | 61 | (18, 25, 27) | 20 | 1220 |
| 62 | 62 | (20, 25, 27) | 20 | 1240 |
| 63 | 63 | (22, 25, 27) | 20 | 1260 |
| 64 | 64 | (24, 25, 27) | 20 | 1280 |
| 65 | 65 | (26, 25, 27) | 20 | 1300 |
| 66 | 66 | (28, 25, 27) | 20 | 1320 |
| 67 | 67 | (30, 25, 27) | 20 | 1340 |
| 68 | 68 | (32, 25, 27) | 20 | 1360 |
| 69 | 69 | (34, 25, 27) | 20 | 1380 |
| 70 | 70 | (8, 29, 27) | 20 | 1400 |
| 71 | 71 | (10, 29, 27) | 20 | 1420 |
| 72 | 72 | (12, 29, 27) | 20 | 1440 |
| 73 | 73 | (14, 29, 27) | 20 | 1460 |
| 74 | 74 | (16, 29, 27) | 20 | 1480 |
| 75 | 75 | (18, 29, 27) | 20 | 1500 |
| 76 | 76 | (20, 29, 27) | 20 | 1520 |
| 77 | 77 | (22, 29, 27) | 20 | 1540 |
| 78 | 78 | (24, 29, 27) | 20 | 1560 |
| 79 | 79 | (26, 29, 27) | 20 | 1580 |
| 80 | 80 | (28, 29, 27) | 20 | 1600 |
| 81 | 81 | (30, 29, 27) | 20 | 1620 |
| 82 | 82 | (32, 29, 27) | 20 | 1640 |
| 83 | 83 | (34, 29, 27) | 20 | 1660 |
| 84 | 83 | (34, 29, 27) | 20 | 1680 |
| 85 | 82 | (32, 29, 27) | 20 | 1700 |
| 86 | 81 | (30, 29, 27) | 20 | 1720 |
| 87 | 80 | (28, 29, 27) | 20 | 1740 |
| 88 | 79 | (26, 29, 27) | 20 | 1760 |
| 89 | 78 | (24, 29, 27) | 20 | 1780 |
| 90 | 77 | (22, 29, 27) | 20 | 1800 |
| 91 | 76 | (20, 29, 27) | 20 | 1820 |
| 92 | 75 | (18, 29, 27) | 20 | 1840 |
| 93 | 74 | (16, 29, 27) | 20 | 1860 |
| 94 | 73 | (14, 29, 27) | 20 | 1880 |
| 95 | 72 | (12, 29, 27) | 20 | 1900 |
| 96 | 71 | (10, 29, 27) | 20 | 1920 |
| 97 | 70 | (8, 29, 27) | 20 | 1940 |
| 98 | 69 | (34, 25, 27) | 20 | 1960 |
| 99 | 68 | (32, 25, 27) | 20 | 1980 |
| 100 | 67 | (30, 25, 27) | 20 | 2000 |
| 101 | 66 | (28, 25, 27) | 20 | 2020 |
| 102 | 65 | (26, 25, 27) | 20 | 2040 |
| 103 | 64 | (24, 25, 27) | 20 | 2060 |
| 104 | 63 | (22, 25, 27) | 20 | 2080 |
| 105 | 62 | (20, 25, 27) | 20 | 2100 |
| 106 | 61 | (18, 25, 27) | 20 | 2120 |
| 107 | 60 | (16, 25, 27) | 20 | 2140 |
| 108 | 59 | (14, 25, 27) | 20 | 2160 |
| 109 | 58 | (12, 25, 27) | 20 | 2180 |
| 110 | 57 | (10, 25, 27) | 20 | 2200 |
| 111 | 56 | (8, 25, 27) | 20 | 2220 |
| 112 | 55 | (34, 21, 27) | 20 | 2240 |
| 113 | 54 | (32, 21, 27) | 20 | 2260 |
| 114 | 53 | (30, 21, 27) | 20 | 2280 |
| 115 | 52 | (28, 21, 27) | 20 | 2300 |
| 116 | 51 | (26, 21, 27) | 20 | 2320 |
| 117 | 50 | (24, 21, 27) | 20 | 2340 |
| 118 | 49 | (22, 21, 27) | 20 | 2360 |
| 119 | 48 | (20, 21, 27) | 20 | 2380 |
| 120 | 47 | (18, 21, 27) | 20 | 2400 |
| 121 | 46 | (16, 21, 27) | 20 | 2420 |
| 122 | 45 | (14, 21, 27) | 20 | 2440 |
| 123 | 44 | (12, 21, 27) | 20 | 2460 |
| 124 | 43 | (10, 21, 27) | 20 | 2480 |
| 125 | 42 | (8, 21, 27) | 20 | 2500 |
| 126 | 41 | (34, 17, 27) | 20 | 2520 |
| 127 | 40 | (32, 17, 27) | 20 | 2540 |
| 128 | 39 | (30, 17, 27) | 20 | 2560 |
| 129 | 38 | (28, 17, 27) | 20 | 2580 |
| 130 | 37 | (26, 17, 27) | 20 | 2600 |
| 131 | 36 | (24, 17, 27) | 20 | 2620 |
| 132 | 35 | (22, 17, 27) | 20 | 2640 |
| 133 | 34 | (20, 17, 27) | 20 | 2660 |
| 134 | 33 | (18, 17, 27) | 20 | 2680 |
| 135 | 32 | (16, 17, 27) | 20 | 2700 |
| 136 | 31 | (14, 17, 27) | 20 | 2720 |
| 137 | 30 | (12, 17, 27) | 20 | 2740 |
| 138 | 29 | (10, 17, 27) | 20 | 2760 |
| 139 | 28 | (8, 17, 27) | 20 | 2780 |
| 140 | 27 | (34, 13, 27) | 20 | 2800 |
| 141 | 26 | (32, 13, 27) | 20 | 2820 |
| 142 | 25 | (30, 13, 27) | 20 | 2840 |
| 143 | 24 | (28, 13, 27) | 20 | 2860 |
| 144 | 23 | (26, 13, 27) | 20 | 2880 |
| 145 | 22 | (24, 13, 27) | 20 | 2900 |
| 146 | 21 | (22, 13, 27) | 20 | 2920 |
| 147 | 20 | (20, 13, 27) | 20 | 2940 |
| 148 | 19 | (18, 13, 27) | 20 | 2960 |
| 149 | 18 | (16, 13, 27) | 20 | 2980 |
| 150 | 17 | (14, 13, 27) | 20 | 3000 |
| 151 | 16 | (12, 13, 27) | 20 | 3020 |
| 152 | 15 | (10, 13, 27) | 20 | 3040 |
| 153 | 14 | (8, 13, 27) | 20 | 3060 |
| 154 | 13 | (34, 9, 27) | 20 | 3080 |
| 155 | 12 | (32, 9, 27) | 20 | 3100 |
| 156 | 11 | (30, 9, 27) | 20 | 3120 |
| 157 | 10 | (28, 9, 27) | 20 | 3140 |
| 158 | 9 | (26, 9, 27) | 20 | 3160 |
| 159 | 8 | (24, 9, 27) | 20 | 3180 |
| 160 | 7 | (22, 9, 27) | 20 | 3200 |
| 161 | 6 | (20, 9, 27) | 20 | 3220 |
| 162 | 5 | (18, 9, 27) | 20 | 3240 |
| 163 | 4 | (16, 9, 27) | 20 | 3260 |
| 164 | 3 | (14, 9, 27) | 20 | 3280 |
| 165 | 2 | (12, 9, 27) | 20 | 3300 |
| 166 | 1 | (10, 9, 27) | 20 | 3320 |
| 167 | 0 | (8, 9, 27) | 20 | 3340 |
| 168 | 55 | (34, 21, 27) | 4 | 3360 |
| 169 | 21 | (22, 13, 27) | 3 | 3364 |
| 170 | 50 | (24, 21, 27) | 6 | 3367 |
| 171 | 46 | (16, 21, 27) | 5 | 3373 |
| 172 | 33 | (18, 17, 27) | 1 | 3378 |
| 173 | 56 | (8, 25, 27) | 1 | 3379 |
| 174 | 16 | (12, 13, 27) | 6 | 3380 |
| 175 | 64 | (24, 25, 27) | 7 | 3386 |
| 176 | 38 | (28, 17, 27) | 2 | 3393 |
| 177 | 30 | (12, 17, 27) | 3 | 3395 |
| 178 | 65 | (26, 25, 27) | 4 | 3398 |
| 179 | 8 | (24, 9, 27) | 6 | 3402 |
| 180 | 28 | (8, 17, 27) | 4 | 3408 |
| 181 | 80 | (28, 29, 27) | 2 | 3412 |
| 182 | 37 | (26, 17, 27) | 1 | 3414 |
| 183 | 27 | (34, 13, 27) | 3 | 3415 |
| 184 | 51 | (26, 21, 27) | 5 | 3418 |
| 185 | 65 | (26, 25, 27) | 4 | 3423 |
| 186 | 17 | (14, 13, 27) | 7 | 3427 |
| 187 | 82 | (32, 29, 27) | 4 | 3434 |
| 188 | 51 | (26, 21, 27) | 3 | 3438 |
| 189 | 3 | (14, 9, 27) | 2 | 3441 |
| 190 | 51 | (26, 21, 27) | 7 | 3443 |
| 191 | 63 | (22, 25, 27) | 5 | 3450 |
| 192 | 52 | (28, 21, 27) | 8 | 3455 |
| 193 | 6 | (20, 9, 27) | 6 | 3463 |
| 194 | 55 | (34, 21, 27) | 6 | 3469 |
| 195 | 42 | (8, 21, 27) | 4 | 3475 |
| 196 | 58 | (12, 25, 27) | 6 | 3479 |
| 197 | 71 | (10, 29, 27) | 2 | 3485 |
| 198 | 38 | (28, 17, 27) | 6 | 3487 |
| 199 | 15 | (10, 13, 27) | 3 | 3493 |
