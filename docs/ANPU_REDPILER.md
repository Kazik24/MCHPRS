# ANPU BUD behavior and compiled acceptance

The target is `Q2CK@Q2CK_Anpu1_Pong_KBTV.schem`, confirmed by the author on 2026-10-06. ANPU does not yet compile: it needs persistent BUD storage and independent, ordered update channels in the graph. `/rp compile` uses the Direct/Boolean pipeline and rejects unsupported piston programs before activation. There is no interpreter-backed Redpiler execution mode.

The interpreter remains the reference for the required memory-write pattern and screen behavior. Its frozen BUD trace, CPU checkpoints and screen frames are retained as acceptance data for actual compiled support. Existing supported adders and counters use their compiled programs.

Manual compilation and analysis use the initiating player's effective rank budget:
Player/Builder 1×, Advanced 2×, Expert 4×, Engineer/Moderator/Admin 8×.
The multiplier increases inspected cells, piston inventory, dependency traversal,
instant actor/source counts, extraction steps, Boolean decisions and conjunction
cache limits. It is bounded at 8× and cannot be selected by command flags.
Stale permissions and expired rank membership fall back to 1×; automatic compilation
also retains 1×. Rank resolution requires `redstonefun_ranks=true`.
These are work/count limits, not a RAM reservation or a guarantee that a build can
enter the compiler. ANPU still requires BUD/update-channel graph support.

## Fixture and important differences from the counter

| Property | ANPU Pong |
| --- | --- |
| SHA-256 | `f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec` |
| Selection dimensions | 184 × 95 × 122 |
| Pistons | 2,898: 2,113 sticky and 785 ordinary |
| Observers | 64 |
| Identified note-block BUD bank | 896 downward sticky cells, initially extended |
| These cells' payload | Black concrete, rather than a redstone block |
| Bank base Y layers | 32: 256 cells; 41: 128; 47: 256; 56: 256 |
| Start button | Selection-local `(122,68,56)` |
| Screen | Selection-local `x=140`, `y=58..89`, `z=38..69` |

The 896-cell signature is a **test projection**, not a general parser rule or a claim that these are all storage elements in ANPU. Mapping these cells to instruction/register/framebuffer addresses remains separate work.

ANPU also starts with retracted pistons and uses multiple physical update paths. Consequently the counter's ready-extended, redstone-block storage bank and fixed six-phase clock cannot be generalized to this CPU by removing admission checks. The conducting payload can change a downstream route without directly emitting power. Note-block state changes and independent piston/wire notifications matter even when the data strength does not change.

## Observable contract

For the supplied start-button episode, compare three related observations:

1. **Sampling:** game tick, interpreter phase, storage position, previous extension state and sampled desired power. Include samples that retain the same value; unchanged electrical strength does not imply absence of an update.
2. **Accepted movement:** game tick, phase, cell position, extension/retraction/drop action, facing and sticky identity, in execution order. A queued request can be cancelled by the physical event's power recheck; it must not be counted as an accepted write in advance.
3. **Screen:** all 1,024 lamp pixels at every completed game-tick boundary. Record every changed frame and its game tick, including the initial frame.

The interpreter baseline for the no-paddle Pong episode produces 327,585 piston samples and 10,682 accepted piston operations over ticks 1–50,000. These totals include ordinary pistons; they must not be described as 10,682 BUD memory writes. It produces the original 14 screen states, with the final change at tick 3,422.

Within the identified 896-cell bank, the frozen episode contains 245,453 samples and 2,768 accepted movements across 1,552 nonempty sample ticks. Its first bank sample is at tick 18 and its last at tick 3,414.

The [supplementary BUD reference](../test_data/cpu-references/anpu_bud_updates.json) freezes the 896-cell projection, its sorted coordinates, and each nonempty game tick's ordered-trace hash, sample count and accepted-event count. Coordinates include the harness placement offset `(8,8,8)`. Hash input is JSON serialization of the ordered filtered `trace::Entry` array, including tick and phase. Empty ticks are omitted, so a newly introduced sample/write also changes the reference comparison.

The original [CPU checkpoints](../test_data/cpu-references/anpu_pong.json), [screen frames](../test_data/cpu-references/anpu_screen.json) and authorized [piston-shape migration](../test_data/cpu-references/piston-shape-checkpoints.json) remain unchanged. The new BUD capture uses the interpreter and requires every existing checkpoint and screen frame to pass before writing a new file. This is a physical regression oracle, not an independent proof of ANPU's instruction set or of Java behavior for this CPU.

## Current compilation pipeline

1. [Compiler::compile](../crates/core/src/redpiler/mod.rs) performs live analysis using the server-selected budget. Analysis inventories physical state and outstanding work before graph preparation.
2. Piston builds require [program::prepare](../crates/core/src/redpiler/instant/program.rs) to produce a supported instant program and ordinary graph. Admission guards retain ownership, payload, entry-state and boundary requirements.
3. Successful preparation lowers into the Direct backend. The staged backend becomes active only after graph/program compilation succeeds; unsupported builds retain their interpreted state and scheduled work.
4. ANPU's arbitrary BUD notifications, initially retracted pistons and conducting black-concrete storage are not covered by the current counter protocol. Increasing work budgets does not add these semantics or bypass admission.

## Validation and manual reference

The [interpreter BUD regression](../crates/core/src/redstone/piston/tests/bud_reference.rs) runs the saved start-button episode for 50,000 game ticks, verifies all original/migrated physical CPU checkpoints and the complete screen trace, and compares the ordered 896-cell sampling/write projection against its frozen reference. Trace recording exists only in test builds. An enabled regression also verifies that ANPU cannot activate compilation through the removed compatibility flag and that rejection leaves physical state and scheduled work intact.

Run the admission regression and full frozen episode with:

```powershell
cargo test -p mchprs_core --lib redstone::piston::tests::bud_reference::anpu_cannot_bypass_compiled_graph_admission --locked -- --exact --test-threads=1
cargo test -p mchprs_core --lib redstone::piston::tests::bud_reference::anpu_interpreter_preserves_frozen_bud_updates_and_screen --locked -- --ignored --exact --nocapture --test-threads=1
```

For a manual reference, import the saved schematic without an initial whole-build settling pass and pause with `/tps 0`. Keep the build interpreted, press its start button, then advance with `/adv` using recorded game-tick counts and no paddle input. Continue through at least tick 5,000 to include the final screen change and settling. `/rp analyze` reports current graph blockers; `/rp compile` must reject this unsupported CPU rather than activate a compatibility executor.

To establish a separately reviewed BUD capture, choose a **new** destination:

```powershell
$env:MCHPRS_ANPU_BUD_CAPTURE = 'F:\rustrepos\MCHPRS\target\new-anpu-bud-reference.json'
cargo test -p mchprs_core --lib redstone::piston::tests::bud_reference::capture_anpu_bud_reference_to_new_file --locked -- --ignored --exact --nocapture --test-threads=1
```

The capture refuses replacement. Do not regenerate the committed reference to accommodate a regression. Warm entry, paddle-input comparisons and active-phase compiled handoff require new compiled regressions once the graph supports these operations.

## Sequential implementation milestones

| Milestone | Work and exit condition | Status |
| --- | --- | --- |
| A1: memory/screen oracle | Freeze ordered samples and accepted movements for the identified bank; retain original per-tick screen and physical checkpoints | Interpreter oracle implemented for the no-paddle episode |
| A2: BUD cells in the graph | Separate data and notification channels; both saved storage states; conducting-payload occupancy; event acceptance and live power recheck; stable ordered sampling | Pending |
| A3: compiled update propagation | Carry wire/source/head/observer notifications with their causal order; preserve same-strength updates and deduplication; unify deadlines and same-tick work across graph/storage owners | Pending |
| A4: CPU graph lowering | Lower ordinary CPU logic, updater pistons, note-block interfaces and certified storage together; account for every update owner; pass the frozen memory and screen oracle | Pending |
| A5: optimization and speed | Retain sampling side effects through folding/coalescing; preserve position/value/order/tick of memory transactions; test reset at active phases and benchmark active execution | Pending |

A2 should first pass `BUD_NonInstantInputs` and `BUD_PistonUpdate` with data-only changes, repeated samples, both bit values and retracted entry. Then use a minimal ANPU note-block/black-concrete cell and two cells sharing an update bus. Compare compiled behavior directly against interpreted fixtures while implementing A3–A5.

Useful additional fixtures are a small cropped ANPU cell with labeled data/update/read ports, two cells whose data and update arrive in one game tick in both orders, and a short paddle-input episode with a stated expected outcome. These improve graph recognition and the CPU's semantic address map; they do not block testing the existing Pong fixture.
