# Revised TEST_POTADOS_PC_COUNTER analysis

## Redpiler verification update — 2026-10-08

The static-piston classification fix is implemented. Production Redpiler now
compiles this fixture in all eight tested option/budget combinations and exactly
reproduces the interpreter's 4,096 output ticks: count peaks 1 through 682 and
final value 682. The fixed piston at local `(6,15,4)` remains unchanged throughout.
The regression imports the actual pending scheduler work after activation,
preserves native movement timing, and checks the bus again after reset.

See the [replay log](../test-prefix-20261008/potados-replay-test-log.txt) and
[implementation report](../test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md).
The original diagnosis below describes the compiler before this change.

## Fixture and activation

Downloaded the revised schematic and verified SHA-256 `641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7` (4,544 bytes; 24 × 19 × 75). Compared with the previous save, eight blocks changed. The important edit extends the sticky piston at local `(14, 6, 2)` and adds nearby support, so the previous BUD-notification error at that position is gone.

All 23 levers were switched on before the first interpreted game tick: the four requested controls, the other three control levers, and all 16 input levers. There were no idle ticks first. The activation tick leaves every lever on. Tick numbers below are relative to the capture immediately after that activation tick.

The labeled output row is 16 repeaters at `(13, 2, z)`, `z = 3, 7, …, 63`, next to `OUT1`/`OUT2`. Its stored repeater state is active-low and reversed spatially: `z=63` is bit 0, and `z=3` is bit 15. Thus the initially all-powered row decodes to zero.

## Interpreter result

Over 4,096 ticks after tick-0 activation, the output produces 682 zero-delimited count windows. Decoding each window’s peak yields exactly `1, 2, 3, …, 682`; the final output at tick 4,096 is `1111110101010101`, which decodes to `0x02AA` (682). For example, the first count peaks are 1 at tick 9, 2 at tick 15, and 3 at tick 22. The output has brief ripple and clear states between peaks, so sampling at an arbitrary tick can show an intermediate value.

This run records 3,075,187 piston samples and 220,323 applied piston events. The instrumented 4,096-tick capture took 10.94 seconds on this machine; this is not a TPS benchmark.

## Why Redpiler still rejects it

All eight compile combinations (both optimization states, both `--assume-instant` states, and budget multipliers 1 and 8) fail at the same admission check, in 0.0139–0.0169 seconds:

```text
logical piston admission failed: BUD at BlockPos { x: 46, y: 45, z: 44 } has no independent sampling source; data changes alone cannot update memory
```

That world position is selection-local `(6, 15, 4)`. Inspection of the analysis report confirms this is a false positive for the circuit’s intended role, rather than a BUD switch:

- It is an extended, downward-facing sticky piston. Its `native_should_extend` result is also `true`.
- Its only power dependency is the constant redstone block at local `(6, 17, 4)`, through quasi-connectivity. That source cannot change, and the piston stays in the same pose throughout the 4,096-tick count run.
- The analysis finds no reset path and no independent notification writer. Its only reported update port is its adjacent piston head at `(6, 14, 4)`.
- The payload group is the redstone block at `(6, 13, 4)` and the piston head at `(6, 14, 4)`.

The cause is the broad candidate rule in [`sampling.rs`](../../../crates/core/src/redpiler/instant/sampling.rs): every sticky piston that is not recognized as a reset mechanism, clocked cell, or control-coupled actuator falls through to independent-memory classification. If that pass cannot find a generator or independent wire notification, it reports “BUD … data changes alone cannot update memory.” Here the piston is powered by a fixed source and needs no sampled state, but the classifier has no static/constant-powered sticky-actuator role. Its “BUD” label is therefore not evidence that the schematic contains a BUD switch.

The next compiler change should recognize this constant-powered, pose-matching piston as static while preserving its payload and electrical effect, then add this schematic as a regression. The generic memory fallback should not infer stored state solely from an unmatched sticky piston.

## Artifacts

- [Revised schematic](TEST_POTADOS_PC_COUNTER.schem)
- [Block and sign inspection](inspection/test_potados_pc_counter.json)
- [All-levers-at-tick-0 compile and timing capture](analysis-compact.json)
- [Opt-in characterization test](../../../crates/core/src/redpiler/analysis/tests/research/potados_counter.rs)

The targeted ignored research test passed (1 test). No production compiler code or frozen expectations were changed.
