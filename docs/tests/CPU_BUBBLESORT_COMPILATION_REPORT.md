# CPU_BubbleSort: compilation bug and runtime observations

Date: 2026-10-07. Status: implementation and regression validation in progress.

The unchanged CPU now compiles and completes its load/reset/sort protocol in the development checkout, both with and without `--assume-instant`. The top RAM bank finishes at `0,1,...,15`; the other three banks preserve their data. This is a passing comparison against the MCHPRS interpreter, not a completed release, a certification of arbitrary CPU programs, or independent Java conformance. Two broader regressions remain unresolved at the time of this report.

This report follows the [original interpreter/admission research](CPU_REFERENCES.md). That earlier report records the compiler before the sampled runtime work described here.

## Fixture and test protocol

The original [CPU_BubbleSort.schem](../../test_data/piston-research/cpu-bubblesort/CPU_BubbleSort.schem) was used without schematic repairs or precomputed sorting behavior.

| Item | Value |
| --- | --- |
| SHA-256 | `907e74d6c4882d4c7065d55e0f8ec4cca8885a34f7af90ca51c060953badaf2c` |
| Dimensions | 225 × 42 × 254 |
| Nonair blocks | 160,429 |
| Pistons | 16,561: 16,496 sticky and 65 ordinary |
| Observers | 11,229 |
| Dust blocks | 41,354 |
| Repeaters | 235 |
| Payload groups | 13,183; 3,346 shared groups, including 32 with three owners |
| Placement minimum | World `(2,8,2)` |
| Placement maximum | World `(226,49,255)` |
| Paste anchor | World `(226,50,2)` |

The test imports two identical worlds and compiles one. It switches the load lever at selection-local `(150,17,119)` ON and advances 400 ticks; clicks the PC-reset button at `(166,7,158)` and advances 100 ticks; then clicks Start at `(166,7,156)` and advances 11,000 ticks. The load lever stays ON. Buttons retain their ordinary twenty-game-tick release. Full-plot bounds supply the notification context around the imported build.

These fixed waits are the comparison harness's protocol, not the earlier research's quiet-detection deadlines. Consequently, the first failing tick described below is specific to this harness.

## Why the original compiler rejected the CPU

The existing certified counter path recognized one downward ordinary piston as a clock generator and a restricted bank of downward BUD memory cells. This CPU has 65 horizontal empty ordinary pistons: 64 memory-row update generators and one other generator. They deliver samples; they do not fit that one-clock protocol.

The CPU also starts with 171 retracted sticky pistons, uses shared payload ownership, and combines data, update and read paths within one coupled region. Its circuits cannot be handled as one combinational response followed by one global six-phase reset. Independent builds may share a plot, but the CPU itself cannot be split into independent clocks merely to make admission succeed.

The saved missing head at local base `(179,6,165)` is a separate issue. Native sorting succeeds with that defect present. The current runtime retains the actual saved absence of the head; it does not invent a head or unconditionally freeze the actor. Success of this one program does not prove that actor is inert under every possible input history.

## Runtime approach and unsuccessful simplifications

The new path compiles local electrical functions and the delivered notification relationships. It keeps piston state, shared payload position and owner, wire strengths and connectivity, observer pulse state, captured piston requests, and finite movement/completion deadlines. Ordinary timed components remain in the ordinary compiled graph.

Compiled ticks use these prepared relationships and retained values. They do not query world blocks, discover spatial wire paths, call interpreter piston callbacks, or recognize the fixture hash to supply a sorting result. The conductor update queue traverses a graph prepared at compile time.

An earlier attempt evaluated fully settled Boolean wiring after each geometry change. That erased intermediate wire values and the distinction between changing a BUD's power and actually sampling it. Load and reset were insufficient acceptance checks: sorting still lost data. Arbitrarily prioritizing one actor in a shared group also failed and was removed.

Allowing ideal timing does not permit memory to continuously follow its power input. It must hold its previous state until a qualifying update. Likewise, an update generator must still cause distinct samples, and a shared payload must retain one actual owner and position.

## First sorting divergence

After introducing retained wire strengths and the compiled conductor queue, both worlds matched through the first 28 completed ticks after Start. The first disagreement occurred while processing Start-loop tick index 28: logical tick 529 after the preceding 500 load/reset ticks.

Immediately before that tick, the comparison matched piston geometry, wire and observer strengths, and every extracted piston power read. The problem therefore arose within the reset wave, rather than from an earlier accumulated RAM error.

The first 440 accepted piston actions matched native order. Some adjacent actions then exchanged order; for example, world `(179,24,235)` and `(179,24,236)`. Later, the compiled runtime applied extra retractions to reset/read-path pistons. One traced storage actor, world `(180,13,179)`, received a compiled falling-power sample that the native interpreter never delivered during that wave.

The faulty run accepted **958** piston actions where native execution accepted **855**. The extra retractions lowered shared read/reset buses and ultimately erased stored values. The later wrong sort was a consequence of those samples, not evidence that the CPU's arithmetic formula had been extracted incorrectly.

## Concrete notification bug and correction

The native extension code distinguishes a destination shape change from an ordinary neighbor power notification:

| Extension step | Native behavior | Faulty compiled behavior |
| --- | --- | --- |
| Write a moving payload at its destination | Deliver shape changes | Deliver shape changes and power callbacks |
| Vacate its source / install the moving head | Notify source and head neighbors | Notify head neighbors |
| Mark the piston base extended | Notify base neighbors | Notify base neighbors |

In addition, native `notify` completes the shape-change pass and then delivers power callbacks in neighbor order. The faulty compiled implementation recalculated adjacent wires during the shape pass and sampled nearby pistons afterward. This regrouping could change the power seen by a BUD when its sample arrived.

The correction separates `sequential_shape_changed` from `sequential_notify`. Extension destinations receive only the shape-change operation. Full notifications then dispatch precompiled wire/base/head callbacks in the native neighbor order, instead of grouping wire recalculations ahead of piston samples.

Both corrections were applied together. The evidence confirms that the corrected notification contract removes the failure; it does not independently quantify each defect's contribution to the original CPU trace.

After the correction, the failing wave accepted **855** actions in both runtimes and its completed state matched. Some independent adjacent actions still exchanged order, so identical action counts are not a claim of an identical internal event trace. The subsequent full state and sorting comparisons are the stronger evidence.

A small regression, `sampled_extension_destination_does_not_resample_quasi_powered_memory`, isolates the destination rule: an extending source temporarily removes the power of an adjacent quasi-powered memory piston. A destination shape change must not prematurely sample and retract that memory. It passes with both flag settings.

Relevant implementation:

- [Native extension and notification rules](../../crates/core/src/redstone/piston.rs).
- [Compiled sampled runtime](../../crates/core/src/redpiler/backend/direct/instant/sequential.rs).
- [Preparation of notification and conductor relationships](../../crates/core/src/redpiler/instant/sequential.rs).
- [Local electrical extraction](../../crates/core/src/redpiler/instant/logic/sequential.rs).
- [Small destination-notification regression](../../crates/core/src/redpiler/analysis/tests/regions.rs).

## Validation completed

The opt-in `bubblesort_compiled_sampled_protocol` comparison passed for both `assume_instant=false` and `assume_instant=true`.

With `MCHPRS_CPU_COMPARE_GEOMETRY=1`, every tick after Start checks retained wire/observer strengths, extracted piston power against native power reads, and virtual geometry against the interpreted world. The geometry check treats two moving-piston blocks as equivalent without comparing their facing; it is not a complete motion-entity or client-animation comparison. Loading and PC reset also compare piston state. Final assertions check the complete 64-word RAM projection.

| Result | Without flag | With `--assume-instant` |
| --- | --- | --- |
| Compile unchanged CPU | Passed | Passed |
| Load descending top bank | Passed | Passed |
| Reset PC | Passed | Passed |
| Full 11,000-tick Start comparison | Passed | Passed |
| Top bank ends at 0–15 | Passed | Passed |
| Other 48 RAM words unchanged | Passed | Passed |

The combined test took 540.06 seconds in the instrumented development build. This is test duration, not a runtime speedup measurement. These CPU runs used default optimization/I/O settings. The sampled CPU path currently retains finite movement and notification boundaries under both flag settings; the result is not evidence of zero-delay CPU execution under `--assume-instant`.

Run the comparison from the repository root in PowerShell:

```powershell
$env:MCHPRS_CPU_COMPARE_GEOMETRY = '1'
cargo test -p mchprs_core --lib bubblesort_compiled_sampled_protocol --locked -- --ignored --nocapture
Remove-Item Env:MCHPRS_CPU_COMPARE_GEOMETRY
```

The [test harness](../../crates/core/src/redpiler/analysis/tests/research.rs) keeps fixture hash verification and compares actual compiled execution with a separately interpreted import.

## Broader observations and remaining work

The latest Redpiler-filtered run completed with **86 passed, two failed and seven ignored**. Existing ADDER, COUNTER, material/consumer, rotation and mixed-adder/counter regressions passed in that run. This is not a fully green regression result.

The two failures are:

1. **Saved-geometry conductor handoff.** The newly admitted missing-head/retracted conductor cases match during compiled execution, but the missing-head case's interpreter continuation comparison fails after reset. Reconstruction and test-fixture copying need investigation before this path can be considered validated.
2. **RILAX graph ownership.** Compilation tries to project an output for a blocked note block at world `(40,36,50)`. Consumer discovery includes the note block, while ordinary node identification omits it because a redstone block covers it. The resulting “electrical source ... has no graph owner” error is an admission/graph disagreement, not evidence of a memory transaction failure. The new positive transaction comparison cannot yet run.

An exhaustive COUNTER carry/wrap comparison is running separately. Its result must be recorded after completion; it is not counted as passed here.

The known XOR tick-8 reset waveform discrepancy was reproduced separately. The user requested that it be ignored for this task, so it remains excluded rather than reported as fixed.

Still pending: CPU plus other builds in one mixed plot, sampled-runtime handoff at active phases, the optimization/I/O matrix for the CPU path, the final full-core/workspace checks, and release compilation. Existing mixed-adder/counter tests do not establish arbitrary mixed-CPU correctness.

The changes are in the development checkout and have not been released. Successful CPU sorting does not by itself prove every supported input history, every instruction/program, every saved malformed entry, or independent Java behavior.

