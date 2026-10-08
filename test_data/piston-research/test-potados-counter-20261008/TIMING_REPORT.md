# TEST_POTADOS_PC_COUNTER analysis

## Fixture

Downloaded `TEST_POTADOS_PC_COUNTER.schem` and verified its SHA-256:

`433add18a2a56a1c1cb3025b3c07ccc1370af4d4bca11bf5d546251feeeb7c4d`

It is a Sponge v2 schematic, 24 × 19 × 75 blocks (34,200 cells; 6,913 non-air). The selected structure contains 500 sticky pistons, 499 piston heads, 371 observers, 80 repeaters, 1,888 redstone-wire blocks, and 23 levers. The storage is repeater based, but the compiler also finds a BUD-sensitive piston arrangement in the circuit.

The specified switches were turned on in this order, using selection-local coordinates:

| Label | Lever coordinate |
| --- | --- |
| CIN | `(6, 9, 73)` |
| CH 3 FRW | `(11, 13, 65)` |
| READ CH 3 | `(14, 13, 67)` |
| WRITE | `(17, 12, 69)` |

The 16 input levers and the other three control levers (`READ CH R1`, `READ CH R2`, `WRITE BUS`) stayed in their saved OFF state. The labeled `OUT1`/`OUT2` row was read as 16 repeaters at `(13, 2, z)`, `z = 3, 7, …, 63`; all start powered. Raw bus strings below list repeater states in ascending `z` order.

## Redpiler result

All eight combinations of `--optimize`, `--assume-instant`, and analysis budget multiplier 1 or 8 fail at the same admission check, in 0.0147–0.0173 seconds. The failure happens before compiled execution:

```text
logical piston admission failed: BUD at BlockPos { x: 54, y: 36, z: 42 } has no independent sampling source represented by the instant runtime; piston base at BlockPos { x: 55, y: 36, z: 42 } can notify BlockPos { x: 54, y: 36, z: 42 }, but its ordered movement/reset callbacks and head eligibility need a timing certificate
```

Relative to the schematic selection, the target is `(14, 6, 2)` and the notifying piston is `(15, 6, 2)`. The target is a retracted north-facing sticky piston; the adjacent notifier is an extended downward-facing sticky piston. Redpiler treats this as a BUD sampling route whose notification timing and head eligibility its instant runtime cannot currently certify. This is not a repeater-memory classification failure, analysis-budget limit, or optimization interaction. Supporting the schematic requires modeling or proving that notification route; removing the admission check would risk changing observable sampling order.

## Native timing observations

After 12 idle game ticks, the output row is `1111111111111111` with no pending scheduled ticks, piston events, or motions.

With one game tick between turning on each requested switch, the following 4,096 native ticks produce 682 output-row transitions. Every transition is the repeater at `z=63` toggling; transitions alternate gaps of 5 and 7 ticks (341 and 340 gaps). The other 15 output repeaters stay powered. This run records 1,282,439 piston samples and 86,011 applied piston events. Its final state still has 24 pending scheduled ticks, one piston event, and 80 motions.

With 24 game ticks after each switch, the output after `WRITE` is `1111111111111100`, and there are three pending scheduled ticks. Over the next 4,096 ticks the output changes 2,040 times. Repeaters at `z=27, 31, 35, 39, 43, 47, 51, 55, 59, 63` toggle at least once; those at `z=3` through `z=23` remain powered. The final row is `1111111111111101`, with 123 pending scheduled ticks and 109 piston events.

The settled-switch trace therefore shows ongoing bus activity, but not a stable, monotonic 16-bit count: it revisits the all-powered state and leaves scheduled piston work outstanding. The interpreter result does not establish whether the saved circuit needs another control/input setting or whether this saved state or circuit behavior is the cause. All 16 data inputs and the three unrequested controls were deliberately left unchanged. Since compilation fails, there is no compiled-versus-native execution comparison.

The measured native capture durations were 3.27 seconds for the one-tick spacing and 4.16 seconds for the 24-tick spacing on this machine. These are diagnostic run times, not a TPS benchmark; the first capture also records per-tick piston and repeater changes.

## Artifacts and scope

- Schematic: [`TEST_POTADOS_PC_COUNTER.schem`](TEST_POTADOS_PC_COUNTER.schem)
- Full block/sign inspection: [`inspection/test_potados_pc_counter.json`](inspection/test_potados_pc_counter.json)
- Compile matrix and tick-by-tick native capture: [`analysis-final.json`](analysis-final.json)
- Opt-in capture test: [`potados_counter.rs`](../../../crates/core/src/redpiler/analysis/tests/research/potados_counter.rs)

The targeted ignored research test passed. No production compiler behavior or frozen expectations were changed. A full workspace test run and a corrected/working 16-bit count remain unverified.
