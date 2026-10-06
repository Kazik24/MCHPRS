# ANPU BUD execution and Redpiler acceptance

The current target is `Q2CK@Q2CK_Anpu1_Pong_KBTV.schem`, confirmed by the author on 2026-10-06. Redpiler now provides an explicit **physical compatibility mode** for it:

```text
/rp compile --piston-events --io-only
```

This mode executes the shared physical redstone engine in a private plot and publishes I/O. It does **not** lower ANPU into an optimized electrical netlist. Normal `/rp compile` continues to use the existing Direct/Boolean pipeline for supported ordinary builds, adders and counters. ANPU graph lowering remains the next implementation milestone.

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

The 896-cell signature is a **test projection**, not a general parser rule or a claim that these are all storage elements in ANPU. The paired regression additionally compares samples and accepted events for every piston. Mapping these cells to instruction/register/framebuffer addresses remains separate work.

ANPU also starts with retracted pistons and uses multiple physical update paths. Consequently the counter's ready-extended, redstone-block storage bank and fixed six-phase clock cannot be generalized to this CPU by removing admission checks. The conducting payload can change a downstream route without directly emitting power. Note-block state changes and independent piston/wire notifications matter even when the data strength does not change.

## Observable contract

For the supplied start-button episode, compare three related observations:

1. **Sampling:** game tick, interpreter phase, storage position, previous extension state and sampled desired power. Include samples that retain the same value; unchanged electrical strength does not imply absence of an update.
2. **Accepted movement:** game tick, phase, cell position, extension/retraction/drop action, facing and sticky identity, in execution order. A queued request can be cancelled by the physical event's power recheck; it must not be counted as an accepted write in advance.
3. **Screen:** all 1,024 lamp pixels at every completed game-tick boundary. Record every changed frame and its game tick, including the initial frame.

The event mode preserves all three. The no-paddle Pong episode matches 327,585 piston samples and 10,682 accepted piston operations over ticks 1–50,000. These totals include ordinary pistons; they must not be described as 10,682 BUD memory writes. It also matches the original 14 screen states, with the final change at tick 3,422.

Within the identified 896-cell bank, the frozen episode contains 245,453 samples and 2,768 accepted movements across 1,552 nonempty sample ticks. Its first bank sample is at tick 18 and its last at tick 3,414.

The [supplementary BUD reference](../test_data/cpu-references/anpu_bud_updates.json) freezes the 896-cell projection, its sorted coordinates, and each nonempty game tick's ordered-trace hash, sample count and accepted-event count. Coordinates include the harness placement offset `(8,8,8)`. Hash input is JSON serialization of the ordered filtered `trace::Entry` array, including tick and phase. Empty ticks are omitted, so a newly introduced sample/write also changes the reference comparison.

The original [CPU checkpoints](../test_data/cpu-references/anpu_pong.json), [screen frames](../test_data/cpu-references/anpu_screen.json) and authorized [piston-shape migration](../test_data/cpu-references/piston-shape-checkpoints.json) remain unchanged. The new BUD capture uses the interpreter and requires every existing checkpoint and screen frame to pass before writing a new file. This is a physical regression oracle, not an independent proof of ANPU's instruction set or of Java behavior for this CPU.

## Actual execution pipeline

1. `Compiler::compile` checks `--piston-events` before ordinary instant-family analysis. No unsupported instant is silently converted into Boolean logic.
2. [EventBackend::prepare](../crates/core/src/redpiler/backend/events.rs) requires one complete loaded plot, a completed game-tick boundary and exact scheduled-work transfer. Command blocks, cursed-world support semantics and incompatible graph/export/update flags reject transactionally. No live state or scheduled work is changed during preparation.
3. [Chunk::snapshot](../crates/core/src/world/storage.rs) copies authoritative block storage, including pending unflushed changes and entities, with independent cache identities. The private plot receives the exact piston state and typed scheduled queue. Active motions and queued events at a completed boundary can be retained.
4. The shared `PlotWorld::tick_interpreted` runs scheduled callbacks, piston-event FIFO and ordered motion completion. A single execution owner retains every notification and power recheck. There is no physical/graph scheduling boundary inside the CPU.
5. Lever/button actions and pressure-plate input enter this same private world with the physical support notifications and button release scheduling. Repeated same-strength plate notifications are preserved.
6. Private block deltas update bounded sets/maps of modified and visible positions. I/O publication includes lamps, controls, trapdoors and note-block state; wire and piston presentation remains frozen. Generated sounds are forwarded when outputs flush. Neither mode performs network piston animation.
7. Reset writes changed physical geometry, restores entities including entity-only comparator/motion changes, and transfers exact piston state and scheduled work. It retains expected block types, FIFO order and remaining deadlines. It does not replay the CPU's historical instruction stream. The compiler then drops its private world.

The normal render cadence can coalesce several game ticks into one client update. The regression flushes once per game tick to verify exact logical frames. Increasing the visual update rate changes how many frames a player receives, not the simulation's memory or screen state.

The compatibility mode retains a second plot snapshot and interpreter caches. Its state remains bounded by the plot and pending work; it does not accumulate a production trace. Typed trace instrumentation exists only in test builds. This path does not promise a simulation speedup, and it is deliberately not selected by automatic compilation.

## Validation and manual test

The enabled regressions cover independent BUD data/hold/resample, compilation and handoff around live motion, rejection without mutation, and ANPU warm compilation at tick 125. The warm ANPU episode exercises paddle inputs, duplicate same-strength notifications and complete handoffs/recompilation at ticks 225, 350, 512 and 800. Full physical checkpoints and future sampled events agree.

Run the focused checks and full frozen episode with:

```powershell
cargo test -p mchprs_core --lib redpiler::backend::events::tests --locked -- --test-threads=1
cargo test -p mchprs_core --lib redpiler::backend::events::tests::anpu_event_execution_matches_ordered_writes_and_frozen_screen --locked -- --ignored --exact --nocapture --test-threads=1
```

For an in-game comparison, use two fresh imports with the saved inputs, no initial whole-build settling and no paddle input. Pause with `/tps 0`. Leave the first interpreted; activate the event mode in the second. Press the schematic's start button, then use `/adv` with matching game-tick counts. The ball/display transitions must agree; the internal compiled-view pistons and wires stay visually frozen. Continue through at least tick 5,000 to include the final screen change and settling. `/rp reset` exposes the current physical memory state and allows ordinary/nano/pico continuation. `/rp inspect` additionally logs private state in debug builds with debug logging enabled.

`--io-only` is accepted for familiar command usage; event mode always publishes I/O. `--optimize`, `--export`, `--export-dot` and `--update` are rejected in this mode. `/rp analyze` describes eligibility for the existing graph pipeline, so its instant-runtime blockers do not describe event-mode eligibility. `/rp analyze --graph --piston-events` explicitly explains that this mode has no electrical candidate graph.

To establish a separately reviewed BUD capture, choose a **new** destination:

```powershell
$env:MCHPRS_ANPU_BUD_CAPTURE = 'F:\rustrepos\MCHPRS\target\new-anpu-bud-reference.json'
cargo test -p mchprs_core --lib redpiler::backend::events::tests::bud_reference::capture_anpu_bud_reference_to_new_file --locked -- --ignored --exact --nocapture --test-threads=1
```

The capture refuses replacement. Do not regenerate the committed reference to accommodate a regression.

## Sequential implementation milestones

| Milestone | Work and exit condition | Status |
| --- | --- | --- |
| A1: physical compatibility owner | Complete-plot snapshot, exact queue/motion ownership, physical controls, I/O publication and transactional activation | Implemented |
| A2: memory/screen oracle | Ordered samples and accepted movements, frozen 896-cell trace, original per-tick screen and physical checkpoints, warm/paddle/motion handoffs | Implemented for the declared episodes |
| A3: BUD cells in the graph | Separate data and notification channels; both saved storage states; conducting-payload occupancy; event acceptance and live power recheck; stable ordered sampling | Pending |
| A4: compiled update propagation | Carry wire/source/head/observer notifications with their causal order; preserve same-strength updates and deduplication; unify deadlines and same-tick work across graph/storage owners | Pending |
| A5: CPU graph lowering | Lower ordinary CPU logic, updater pistons, note-block interfaces and certified storage together; account for every update owner; pass the frozen memory and screen oracle | Pending |
| A6: optimization and speed | Retain sampling side effects through folding/coalescing; preserve address/value/order/tick of memory transactions; test reset at active phases and benchmark active execution | Pending |

A3 should first pass `BUD_NonInstantInputs` and `BUD_PistonUpdate` with data-only changes, repeated samples, both bit values and retracted entry. Then use a minimal ANPU note-block/black-concrete cell and two cells sharing an update bus. Keep the event backend available as the differential owner while implementing A4–A6.

Useful additional fixtures are a small cropped ANPU cell with labeled data/update/read ports, two cells whose data and update arrive in one game tick in both orders, and a short paddle-input episode with a stated expected outcome. These improve graph recognition and the CPU's semantic address map; they do not block testing the existing Pong fixture.
