# Implemented instant-piston pipeline

This document describes the Rust implementation in the working tree on 2026-10-06. The first executable acceptance target is the lever/repeater revision of `ADDER_11BITS.schem`. The broader [implementation plan](INSTANT_PISTON_IMPLEMENTATION_PLAN.md) remains a roadmap; completing this adder does not complete BUD storage, counters or every reset family.

The tested I/O schematic is [ADDER_11BITS.schem](../test_data/instant-pistons-io/ADDER_11BITS.schem), dimensions 23 × 7 × 46, SHA-256 `335ab94a3ba7f89cd0a497a2ac606372ff2743cfcd549f4863ead310af4138b9`. Its [manifest](../test_data/instant-pistons-io/fixtures/adder_11bits.json) defines the controls and observation windows. The original 21 × 7 × 46 schematic also passes graph preparation. Both contain 142 sticky pistons: 98 carry redstone blocks and 44 carry white wool. Multiple pistons can share one payload, so those actuator counts are not counts of independent blocks.

## Actual compilation stages

1. [Compiler::compile](../crates/core/src/redpiler/mod.rs) performs live analysis before graph construction. It checks entry phase, actual blocks and entities, scheduled work, piston events and motions. Analysis is bounded and cancellable. Ordinary plots retain the ordinary path; piston plots require a prepared instant program.
2. [program::prepare](../crates/core/src/redpiler/instant/program.rs) validates ready extended sticky mechanisms, supported payloads, update context, observer ownership and output context. It rejects command blocks, unowned observers, active motion, independent BUD sampling and unsupported consumers. An observer reset needs a downward-facing observer above the base, a fixed conducting cap, and a verified return path without another reset writer.
3. [logic::extract](../crates/core/src/redpiler/instant/logic.rs) reads conditional electrical geometry. A provisional Boolean variable describes each actuator's first retraction. Shared far occupancy disappears if any owner retracts. Moving near payloads provide neither power nor conduction during the response. Wool can conduct a primitive strong source or alter dust climbing and connection sides; it is not a redstone emitter. The wire-side rules are shared with [wire/mod.rs](../crates/core/src/redstone/wire/mod.rs), and emission rules with [power.rs](../crates/core/src/redstone/power.rs).
4. Each possible power contribution becomes a guarded threshold, `source_strength > wire_distance`. Contributions join by OR; an actuator response is absence of effective power. Own-payload and observer feedback belong to reset rather than the first computation wave. A canonical [Boolean decision arena](../crates/core/src/redpiler/instant/boolean.rs) removes redundant conditions before dependency analysis. It does not infer an adder from names, signs, bit coordinates or schematic hashes.
5. Actuator dependencies must be acyclic. Topological substitution replaces provisional actuator variables with ordinary-source functions. A mechanism without an observer must be a payload follower: substituting ready occupancy for every other actuator must make its response identically false, independently of external input strengths. It cannot introduce an independent external root. Provisional decisions and construction caches are discarded after extraction.
6. [Boundaries](../crates/core/src/redpiler/instant/boundary.rs) excludes owned pistons, mobile positions, observers and internal wire nodes from ordinary identification. It retains ordinary program sources and actual consumer interfaces. Mobile aliases remain mutable `MobileSource` nodes. The executable graph uses the program's retained sources directly; `InstantInput` sinks remain part of the older read-only electrical candidate representation.
7. The ordinary pass order remains IdentifyNodes → InputSearch → ClampWeights → DedupLinks → ConstantFold → UnreachableOutput → ConstantCoalesce → Coalesce → PruneOrphans → ExportGraph. Protected source identities, consumers and mobile aliases survive relevant optimization and I/O pruning. Instant binary export is rejected explicitly.
8. [Direct lowering](../crates/core/src/redpiler/backend/direct/compile.rs) validates strengths and fan-in, binds ordinary sources and mobile aliases to retained backend node IDs, and lowers mobile aliases as `InstantSource` nodes. Backend preparation requires the paired `PreparedInstant`; passing an unowned candidate graph to the ordinary compile entry still fails. The compiler publishes a fresh backend only after preparation succeeds. The plot clears interpreter work only after successful activation.

An analysis report's piston/observer ownership requirements are not the final executable verdict. Local reset matching is also insufficient for a conducting or shared network. `/rp analyze --graph` uses the executable preparation path for wool networks and the older structural candidate path for the redstone-only reset fixtures. Analysis does not activate either artifact.

## Wave execution and output timing

[The Direct runtime](../crates/core/src/redpiler/backend/direct/instant.rs) evaluates source thresholds through backend node IDs. It does not read blocks, walk wires, run piston callbacks, or simulate internal nanoticks during compiled ticks. There is one synchronous wave coordinator for the selected network. Its prepared-data and trigger protocol must be respected; independent overlapping computations are later work.

Entry functions must indicate that no actuator needs to retract. This prevents compilation itself from creating a trigger. After ordinary scheduled input work has run, the runtime evaluates the ready network. In the accepted episode, data remains stable and the trigger's nonzero-to-zero change removes power. Restoring power before the first tick cancels that launch. Internal stages compose within the same response rather than adding ticks per piston.

For the supported observer adapter, a firing group's far supply is zero in phases 1–5 and restored in phase 6. The next internally generated response can start in phase 1 of the following cycle while inhibition remains removed. This recurrence belongs to one accepted episode; it is not a new external falling edge. Changes during response/reset remain outside initial conformance.

For a sum bit equal to one, the saved-orientation adder has this observable sequence:

| Game tick after trigger | Virtual far supply | Output repeater power |
| --- | --- | --- |
| 0 | 15 | 15 |
| 1–2 | 0 | 15 |
| 3–5 | 0 | 0 |
| 6 | 15 | 0 |
| 7 | 0 | 0 |
| 8–9 | 0 | 15 |
| 10–11 | 0 | 0 |
| 12 | 15 | 0 |

Subsequent reset pulses recur with period six. A zero sum bit keeps its repeater powered. The initial arithmetic result is valid at ticks 3–7. Read logical bits as **unpowered repeater = one** in that window. A held trigger therefore produces visible reset pulses rather than a permanent displayed sum.

Repeater delay, locking and pending short-pulse behavior remain in the ordinary backend. In particular, the reset rise at tick 6 can produce a queued activation at tick 8 even though the input fell again at tick 7. Publishing a constant arithmetic result would lose this behavior. Internal pistons and dust remain at their entry presentation while compiled; they are not an output probe.

## Returning to the interpreter

`Compiler::reset`, including `/rp reset`, first flushes current ordinary node values. It then reconstructs the owned region using a private interpreter world built from a saved region snapshot. The snapshot includes an electrical neighborhood around the inspected context because settled near payloads can energize reset paths absent from the first-wave graph.

The replay prepares the last ready source levels, applies the accepted launch, and advances a bounded response phase. The first cycle uses at most six response ticks; a repeated phase uses one additional six-tick cycle. Preparation has a 32-tick bound. Ordinary scheduled work is excluded from the replay: its authoritative deadlines come from the live backend scheduler. Only owned blocks, entities, piston work and reset requests are transferred back. Motion timestamps are shifted to the compiled elapsed clock.

This is handoff work, not a runtime simulation fallback. It reconstructs supported geometry and continuation; it does not promise identical historical callback logs, motion identities, or internal compiled rendering. Tests check continued output behavior after handoff at every phase, including a trigger changed before any compiled tick. Broader save/load, edit and lifecycle combinations remain part of the roadmap rather than a completed general materialization proof.

## Admission limits and next work

- Ready, powered, extended sticky mechanisms only; redstone-block and wool payloads; no carried entities, upward mechanisms or active movement.
- A selected network must remain within one plot and include required electrical and qualifying-update context.
- Observable moving context must be a far redstone supply whose owners have verified observer resets. Exposed near payloads, moving-wool conduction and unverified reset context require another adapter. Reset sources visible directly to ordinary consumers are rejected.
- The 1-bit adder's Boolean sum passes all eight arithmetic cases, but its separate carry repeater sees conducting-payload context and is not yet an executable acceptance target.
- The supplied standalone BUD examples and illegal OR are rejected transactionally. Counter storage, AND_3 sampling history, torch/dust execution protocols and XOR's context-dependent later reset waveform need additional work. Structural recognition of those families remains available.
- Current extraction budgets are 1,024 actuators, 64 ordinary source positions, eight actors per local occupancy enumeration, 8,388,608 traversal checks, 1,048,576 Boolean decisions and 2,097,152 conjunction-cache entries. Budget exhaustion fails preparation; a partial function is never activated. The separate analysis defaults are 16,777,216 inspected cells, 65,536 pistons and 4,194,304 dependency steps.

Priority next examples are a small moving-wool-to-repeater carry interface with reset traces, legal ownership transfer with exposed near outputs, and independent BUD data/update circuits attached to this output adapter. Complete clock/rearm histories are needed before changing data during an active recurring episode can become a supported operation. Keep counters and general multi-clock composition as explicit stateful milestones.

## Manual test

Use a clean plot and the lever/repeater schematic revision. Disable automatic ticking while observing exact windows:

```text
/rtps 0
//load ADDER_11BITS.schem
//paste
/rp analyze --graph
/rp compile --optimize --io-only
```

Leave the trigger lever powered while preparing data. In selection-local coordinates, bit `i` uses A at `(4,5,43-4i)`, B at `(4,5,41-4i)`, and its output repeater at `(21,2,41-4i)`. Powered data levers encode zero; unpowered data levers encode one. The trigger is `(2,5,45)`. These coordinates are test instructions, not recognition rules.

Prepare `31 + 1`, advance a few ticks with the trigger still powered, then switch the trigger off and run `/radvance 3`. Only bit 5 should be unpowered: the decoded sum is 32. Other useful cases are `1365 + 682 = 2047` and `2047 + 2047 = 2046` modulo 2048. At tick 8, reset pulses can power previously unpowered repeaters again. Internal payload positions remain visually frozen while compiled.

Use `/rp reset` at a response or reset phase, then `/radvance 12` to observe interpreted continuation. Use fresh imports for independent arithmetic experiments; changing prepared data during reset is undefined in this milestone. Repeat with `/rp compile` to compare unoptimized behavior. `/rp compile --export` rejects piston artifacts explicitly. Nano/pico stepping remains unavailable while compilation is active.

## Regression evidence

[Rust tests](../crates/core/src/redpiler/analysis/tests.rs) verify:

- 40 fresh prepared 11-bit cases × four optimize/I/O flag combinations: arithmetic at ticks 3–7, exact repeater states through tick 24, then 12 interpreted continuation ticks.
- Handoff at ticks 0–12 for six prepared cases, followed by 18 ticks of output comparison each.
- The 40 arithmetic cases after 90°, 180° and 270° rotation plus translation, with optimization and I/O pruning enabled. These prove compiled geometry independence; the full physical waveform comparison is for the saved orientation.
- First-wave arithmetic for all eight 1-bit cases, unchanged imports, unsupported-payload diagnostics, rejection of unsupported storage/consumer contexts, ordinary circuits in piston plots, Boolean composition and retained graph bindings.

The independent Java evidence remains the separately versioned [I/O validation](INSTANT_PISTON_IO_VALIDATION.md). Compiled execution is compared directly with MCHPRS here; this implementation did not produce a new Java capture set. No measured runtime speedup is claimed yet.
