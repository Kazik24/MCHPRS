# Redpiler timing and BUD support adjustment plan

## Implementation update — 2026-10-08

Production execution now preserves native piston timing through closed domains in
`redpiler/timed.rs`. Both the domain
and PlotWorld use the same `world/tick.rs`
phase driver and existing redstone functions. This retains scheduled priority/FIFO,
same-strength callbacks, piston acceptance, transient head/payload occupancy,
observer pulses, motion identities/progress and live reset continuation.

`--assume-instant` supplies the instant-construction proof. It keeps material,
geometry, ownership and independent BUD sampling checks, and preserves native
outputs and timing. Constant-response pistons with the matching saved pose are
proved static before BUD classification; their geometry and electrical effects
remain present. Extra pistons retain their native effects. Command blocks inside
domains execute synchronously through the existing host allowlist and support
conditional chains when driven with `tick_with_world`.

### Verification results

| Acceptance | Result |
| --- | --- |
| 18 TEST schematics, 222 native protocols × four flags | 888 attempts, 808 admitted replays, **zero output mismatches** |
| Default admission | 13/18; includes TEST_BUD_1, TEST_BUD_2, TEST_DEC2, TEST_WEIRD_3 and TEST_XOR2 |
| Assumption admission | 18/18; no waveform change caused by the flag |
| Older small IO pack | All 140 declared episodes × four rotations × four flags: **2,240 passes**, including ordered callbacks, cells, motion/work and handoff |
| ADDER_1BIT and ADDER_11BITS | Native waveform plus declared sum/carry arithmetic windows pass in all four flag modes |
| COUNTER | Native waveform and six-tick consumer count windows pass in all four flag modes |
| DIVIDER | All 11 declared protocols pass native full-state and piston-event comparisons in all four flag modes |
| FPU | All eight opcodes, operand changes and trigger cycles pass native full-state and piston-event comparisons in all four flag modes; no mathematical output oracle imposed |
| Revised Potados PC COUNTER | 4,096 output ticks match in eight option/budget combinations; peaks 1..682 and final 682; fixed piston `(6,15,4)` stays static |
| Lifecycle and guards | TEST compile/reset/recompile preserves pending motion/deadlines; invalid materials, moving support, mismatched heads and missing BUD samplers stay rejected transactionally |
| Command/container behavior | Conditional native command chains pass with and without `-i`; unchanged fixed containers retain external entity state on reset |
| Full workspace | **636 passed, zero failures, 28 ignored**; the PC counter and TEST output checks were additionally selected and passed explicitly |

The 80 rejected TEST attempts are default compilation of TEST_CODER_1, TEST_WEIRD,
TEST_WEIRD_INSTANT, TEST_WEIRD_INSTANT_2 and TEST_WEIRD_INSTANT_3, each across eight
protocols and two optimization settings. Their construction still needs the
assumption proof. All successful pulses, held runs and separated BUD actions match.

Evidence: [final ordered TEST outputs](output-timed-final.json),
[compact admission/mismatch counts](output-timed-summary.json),
[small trace test log](small-trace-test-log.txt),
[large/production native test log](production-native-test-log.txt), and
[Potados replay log](potados-replay-test-log.txt).
The [workspace log](workspace-test-log.txt) records `cargo test --workspace --
--test-threads=4` with `MCHPRS_CONFIG=F:/rustrepos/MCHPRS/Config.toml`. The older
per-crate config has a 4-million-block WorldEdit limit and falsely fails the
8.6-million-block PM1 paste test; the current repository config makes it pass.
A stale CPU capture example was also fixed to collect the existing `ram_trace`
field so the workspace builds. No new reference capture was executed.
The [repeated output verification](output-timed-verification.json) records the
SHA-256 of the final artifact; a second run with mismatch assertions enabled was
byte-identical. Formatting checks pass for every changed Rust file and
`git diff --check` passes. Global `cargo fmt --all -- --check` still reports
pre-existing compact arrays/one-line formatting in the untouched
`redstone/wire/mod.rs`; its [log](format-check-log.txt) is retained.
The original captures and frozen expectations were preserved. Legacy ideal-plan
tests explicitly select their model executor; the acceptance checks above select
the production compiler.

### Performance and scope remaining

This implementation completes the requested admission and observable-timing
corrections using existing native transition code. A conservative notification
closure includes nearby solids, so it can merge large circuits into one domain.
Only disconnected ordinary circuits currently receive graph optimization. Faster
compiled timed actors and mixed graph/domain notification interfaces remain future
performance work; this report does not claim those optimization stages complete.
The ownership scan is bounded and cancellable. Timed-domain graph export is
rejected, and worldless `Compiler::tick` has no host command interface.

No new Java run or 50,000-tick PM1/ANPU/BubbleSort execution was performed. Finite
protocol matches do not prove equivalence for every possible input history.

## Original investigation and proposed sequence

The interpreter and Redpiler produce different output waveforms for all eleven admitted TEST schematics when their inputs are exercised. In TEST_PISTION the difference also changes the final copper bulb latch values. The required adjustment is to preserve observable piston reset and movement episodes, while retaining cached Boolean evaluation where those episodes can safely be hidden. Admission changes must then represent the two BUD switches under default compilation and the weird mechanisms under `--assume-instant`.

## Comparison evidence

The [comparison harness](../../../crates/core/src/redpiler/analysis/tests/research/test_prefix_timing.rs) produced [ordered output traces](output-comparison.json) and a [compact summary](output-summary.json) at commit `d39175768898db0a8c2ecff9cd691a78e95bd680`, with the existing working tree changes present. It ran 222 interpreter protocols: 54 input masks, 162 pulses of widths 1, 2, 3, 4, 6 and 8 game ticks, and six additional BUD protocols separating data and updates. Each protocol was attempted with default flags, `-O`, `--assume-instant`, and both flags, at the default budget.

Of 888 attempts, 528 compiled and ran; 412 of those differed from the interpreter in at least one output frame. Eight retained an output difference at the final observation. Those eight are the held input and eight-tick pulse in TEST_PISTION, each under all four flag combinations. No admitted protocol changed its output waveform between flag combinations. These counts describe protocols and flag combinations, not independent schematics.

Each input mask is held for 96 game ticks and then restored for 32 ticks. Pulses begin after four idle ticks and settle for 32 ticks after the return edge. Outputs include repeaters, both bulb properties, comparator powered state and live analog strength, and any lamps. Compiled comparator strength is read from backend nodes: its world entity is deliberately materialized on reset and is otherwise stale. Comparing that entity directly would falsely report additional persistent errors.

Output observations are taken after complete game ticks and at input boundaries. Native piston operations also retain tick and phase. Exact compiled callback-order equivalence is an acceptance target below, rather than a claim established by these output snapshots.

The first twelve ticks of each original input mask reproduce the output block properties in [timing-full.json](timing-full.json). All 27 tested one-tick pulses, including the temporary controls described below, produced no interpreter piston sample or accepted event. Longer pulses expose behavior that this short-pulse result cannot establish.

### Oscillator and bulb outputs

With TEST_PISTION input mask 1 applied at tick 0, the sticky piston accepts Retract at ticks 2, 8, 14 and Extend at ticks 5, 11, 17. Its output repeater at local `(0,2,12)` behaves as follows:

| Observation | Interpreter | Redpiler under every admitted flag combination |
| --- | --- | --- |
| Initial output | Powered | Powered |
| First falling edge | Tick 4 | Tick 4 |
| Subsequent rising edges while held | 9, 15, 21, 27, continuing every 6 ticks | None |
| Subsequent falling edges while held | 11, 17, 23, 29, continuing every 6 ticks | None |
| Rising edge after restoring input at tick 96 | Tick 99 | Tick 100 |

The native repeater pulses are two game ticks wide. Its directly connected bulb at `(0,2,13)` toggles at ticks 9, 15, 21 and so on. The next bulb toggles at ticks 11, 23, 35; the final bulb at ticks 13, 37, 61. The comparator delays and successive bulb latches expose the missing edge history.

At tick 128, the three bulbs at z = 13, 15, 17 have these `(lit,powered)` states:

| Engine | First bulb | Second bulb | Third bulb |
| --- | --- | --- | --- |
| Interpreter | `(false,true)` | `(false,false)` | `(true,false)` |
| Redpiler | `(true,true)` | `(true,true)` | `(false,true)` |

TEST_PISTION_2 makes the same loss visible without bulbs. Its six delay-1 repeaters have native falling edges at ticks 4, 6, 8, 10, 12, 14, then rising edges at 9, 11, 13, 15, 17, 19. Their later pulses recur every six ticks. Redpiler propagates the initial falling edge and the eventual restored level, but omits the held-input pulse train. These repeaters preserve and delay the native pulses; they are not filtering the oscillator away.

### BUD behavior

The two labelled inputs in both BUD files are update first and data second in the saved lever order. Holding data alone changes neither BUD's accepted movement nor its output level. Preparing data before enabling updates does change storage:

| Protocol | TEST_BUD_1 | TEST_BUD_2 |
| --- | --- | --- |
| Both inputs applied at tick 0 | Generator retracts at 2; memory retracts at 4; output falls at 6 | Generator and memory retract at 2; output falls at 4 |
| Data applied at 0, update at 8 | Generator retracts at 10; memory retracts at 12; output falls at 14 | Generator and memory retract at 10; output falls at 12 |
| Update applied at 0, data at 8 | Memory retracts at 12; output falls at 14 | Memory retracts at 11; output falls at 13 |

These are real notification and storage mechanisms. TEST_BUD_1 needs the observer-delivered sample; TEST_BUD_2 can deliver the adjacent generator notification in the same piston-event phase. A single generic delay attached to a settled generator edge would change one of these protocols.

### Schematics without saved controls

Temporary wall levers were attached to the fixed supports of existing input wall torches. Their positions are recorded as `added_controls`; schematic bytes and decoded sidecars were preserved. These are explicit inferred input protocols, rather than claims about an unsaved original control arrangement.

| Schematic | Added control positions |
| --- | --- |
| TEST_DEC2 | `(2,5,-1)`, `(4,5,-1)` |
| TEST_OR_3 | `(0,2,-1)`, `(0,5,-1)` |
| TEST_XOR | `(0,1,-1)`, `(2,1,-1)` |

TEST_OR_3 and TEST_XOR compile and show the same missing pulses under these controls. TEST_DEC2 has substantial native activity: masks 1, 2, 3 accept respectively 160, 192, 352 piston events during the full hold/restore protocol. Its rejection does not identify a physically missing sampler: preparation first tries to treat its downward ordinary piston as a specialized shared bank clock, then fails that specialization. The intended actor roles still need extraction from the actual power and notification routes.

## Why the current model loses the waveform

The [current execution model](../../../docs/REDPILER_MODEL.md) explicitly gives both default compilation and `--assume-instant` the same cached logical runtime. The flag relaxes construction proofs. Unclocked regions evaluate on source changes, project settled geometry, and have no physical reset or movement waveform.

In [Runtime::advance](../../../crates/core/src/redpiler/backend/direct/instant.rs), an initialized unclocked region returns immediately when no ordinary source or sampling state changes. [Geometry evaluation](../../../crates/core/src/redpiler/backend/direct/instant.rs) sets `MovingBase` to false and selects near/far occupancy from the logical response. [Reset certification](../../../crates/core/src/redpiler/instant/observer.rs) can own and omit the returning observer or dust pulse. Thus the native cycle can disappear while the input stays low. An ordinary output connected to the moved payload still observes that cycle in the interpreter, even when the reset observer has no direct electrical path to that output.

The bulb transition rule exists in both engines and toggles on rising power. The evidence locates the first divergence upstream at the repeater's missing tick-9 rise. Changing the bulb rule, adding an output-only oscillator, or applying a constant timestamp offset would not repair the causal model.

Two independent admission restrictions also apply. [clocked.rs](../../../crates/core/src/redpiler/instant/clocked.rs) requires downward storage and an independent horizontal sampling observer. [sampling.rs](../../../crates/core/src/redpiler/instant/sampling.rs) rejects an ordinary generator watched at its base or head, before the assumption flag can help. Raising the budget does not address these restrictions.

## Required behavioral contract

Default compilation must preserve the interpreter's electrical output history for these supported mechanisms, including startup, sustained operation, stop/restart and pulse cancellation. Ordinary repeaters and copper bulbs are observable throughout execution. A logical result window cannot exclude their intermediate pulses.

Keep `--assume-instant` as a relaxation of construction certification. It must still represent notification delivery, memory retention, supported geometry and output timing. Granting trust to a construction must not authorize erasing a pulse consumed by another component.

Power gives a desired piston response; an actual qualifying callback samples it; the queued event validates it against live state; accepted movement changes geometry; movement completion delivers further callbacks. Preserve these distinct stages. Memory commits follow the accepted transition and the timing of electrically usable payload occupancy, rather than an immediate source-change evaluation.

## Implementation sequence

### 1 Make output history a regression requirement

Use TEST_PISTION and TEST_PISTION_2 as the smallest waveform regressions. Require the exact initial edges, the sustained period and width, the repeater-chain offsets, and the terminal bulb states shown above. Keep the existing interpreter captures as evidence; promote a small set of reviewed timelines into normal assertions. Add the eight-tick pulse because it catches persistent state loss in a shorter episode.

Separate explicitly ideal Boolean extraction tests from interpreter waveform tests. Existing truth tables and ideal six-step bank tests remain useful for their declared contracts, but cannot certify physical output equivalence. Update [REDPILER_MODEL.md](../../../docs/REDPILER_MODEL.md), [REDPILER_ARCHITECTURE.md](../../../docs/REDPILER_ARCHITECTURE.md) and [PISTON_MODEL.md](../../../docs/PISTON_MODEL.md) when the implemented contract changes.

### 2 Retain observable reset and movement work

Extend the existing prepared region and runtime with bounded timed state for the supported base/head/near/far mechanisms. Reuse the existing geometry descriptors, guarded power extraction, observer scheduler and Boolean response cache. Start with empty ordinary generators and sticky pistons moving one supported redstone block or fixed conductor, matching these fixtures.

Track transient occupancy, request/acceptance state, motion identity and completion, and pending observer/reset work. Use the interpreter's rules in [piston.rs](../../../crates/core/src/redstone/piston.rs) and [PistonMotion](../../../crates/world/src/lib.rs) as the transition authority. Share small transition calculations where practical. Moving payloads must not emit or conduct as their stationary material. Early retraction, interrupted motion and head forwarding require the same live validation as the interpreter.

Reset elimination must prove that the complete episode is unobservable through moved payloads, conditional conductors, observer watches and storage callbacks. A proof considering only direct observer-to-output power paths is insufficient. Otherwise retain the reset work. Cached Boolean evaluation can still supply power predicates and internal computation; physical boundaries read the timed occupancy.

Reach: `instant/program.rs`, `instant/observer.rs`, `instant/boolean.rs`, `instant/logic.rs`, `instant/outputs.rs`, and `backend/direct/instant.rs`. Do not revive the retired wave/sequential executors or hardcode a schematic period.

### 3 Deliver notifications on one ordered timeline

Extend [the existing phase contract](../../../crates/core/src/plot/mod.rs) to compiled region work: scheduled callbacks, ordered piston events, then movement work, including callbacks generated during each operation. Preserve scheduler priority/FIFO and the native shape and power neighbor orders.

Replace settled-edge delivery and Boolean `delivered` coalescing at these boundaries with ordered notifications carrying source, target, phase and head eligibility. Deliver an unchanged-power callback when native execution does; deduplicate only where the native queue or observer pending rule does. A head-only route qualifies when the live head exists, including transient absence. Events created by a callback must become visible at the corresponding native checkpoint.

The existing Direct fanout groups targets by node type, and `set_node` suppresses equal-strength link updates. Neither establishes native notification order. Introduce ordered routes only for the affected timed/sampling boundaries, keeping ordinary maximum-strength propagation for stable purely electrical graph paths.

Reach: `analysis/ports.rs`, `instant/sampling.rs`, `instant/regions.rs`, `backend/direct/compile.rs`, `backend/direct/mod.rs`, `backend/direct/update.rs`, `backend/direct/tick.rs`, and the shared queue. Retain the proposed ownership and boundary principles in [REDPILER_PARTIAL_COMPILATION.md](../../../docs/notes/REDPILER_PARTIAL_COMPILATION.md); a full partial-compilation framework is not a prerequisite for these bounded cases.

### 4 Admit the BUD switches with default flags

For TEST_BUD_1, remove the downward-only storage assumption. Derive near and far cells from the piston facing, using the geometry already available. Preserve sticky ownership, supported payload, head matching, independent update routes and entry-state checks. Vertical QC and gravity remain world-oriented: generalizing storage coordinates must not rotate those electrical rules.

For TEST_BUD_2, recognize the west-facing empty generator and its downward observer as timed notification machinery. Record both direct adjacent callbacks and observer delivery, including their different deadlines. Replace the blanket watched-generator rejection only when the mechanism has an executable timed contract. The BUD timing table is the acceptance requirement.

Make shared-clock recognition an optional specialization after role classification. A shape resembling its generator is insufficient to force the entire region into that specialization. A failure to prove an independent shared sampler should select supported general timed preparation; it must not invent a memory write or silently discard a genuine invalid-context error.

Reach: `instant/clocked.rs`, `instant/sampling.rs`, `instant/program.rs`, and memory/notification tests. Add rotations for horizontal storage and both saved poses. The six-step shared-bank optimization is valid only where its first sample, delivery order and output behavior satisfy the declared reference protocol.

### 5 Extend the weird cases and decoder preparation

TEST_WEIRD watches an upward generator head; TEST_WEIRD_INSTANT watches a west-facing generator head; TEST_WEIRD_INSTANT_2 and _3 watch north-facing generator heads. Represent those watches through transient head occupancy and accepted movement/completion notifications. In TEST_WEIRD_INSTANT a generator can extend and retract in the same tick; replacing this history with one final pose can lose an observer pulse.

Use the timed contract under both flag modes. Default admission can require a proven feedback/reset construction; `--assume-instant` may accept the supported construction without that proof. It still needs closed ownership, valid power/notification routes and executable timing. The saved signs in TEST_WEIRD_INSTANT and _2 explicitly request assumption-flag support; signs remain test intent, never compiler recognition input.

For TEST_DEC2, classify the ordinary piston by its actual control, watched cells and notification recipients. Its two input cones drive several reset actors. Allow that graph through general timed preparation instead of requiring a horizontal observer belonging to a shared memory bank. Decide storage boundaries from uncoupled data plus delivered update routes. Native activity alone does not prove which actors are storage or justify an artificial sampling clock.

### 6 Preserve optimization and interpreter handoff

Mark observer watches and notification routes as semantic uses so `-O` cannot remove a relevant wire, identity or delivery checkpoint. Preserve all contributors to conditional output strengths and comparator main/side channels. Check the existing passes in `redpiler/passes/` against those uses.

Reset must export current timed occupancy, stored state, remaining observer/component deadlines, event order and motions. The current materializer writes settled geometry and dormant owned observers; that is insufficient for a running oscillator. Support continuation at represented checkpoints, or explicitly require a proven quiescent handoff until phase transfer exists. Never discard work to manufacture quiescence.

Reach: `backend/direct/instant.rs::materialize`, `backend/direct/mod.rs::reset`, `backend/queue.rs`, `plot/mod.rs`, and continuation/presentation tests. Compile rejection must leave interpreter state intact. This affects existing users of default compilation and reset and therefore needs explicit regression coverage.

## Admission and acceptance targets

| Fixtures | Required target |
| --- | --- |
| TEST_BUD_1, TEST_BUD_2 | Compile with no flags and reproduce independent data/update timing and retained state |
| TEST_DEC, TEST_INSTANT_3, TEST_OR1, TEST_OR2, TEST_OR_3, TEST_PISTION, TEST_PISTION_2, TEST_WEIRD_3, TEST_XOR, TEST_XOR2 | Preserve default admission and repair exercised output histories |
| TEST_CODER_1 | Preserve assumption-flag admission and repair its output pulse history |
| TEST_WEIRD, TEST_WEIRD_INSTANT, TEST_WEIRD_INSTANT_2, TEST_WEIRD_INSTANT_3 | Must compile and preserve output timing under `--assume-instant`; admit by default where construction certification succeeds |
| TEST_DEC2 | Support the extracted timed graph, targeting default admission when its construction is certified; do not reject solely because shared-bank specialization fails |

Acceptance requires the four flag combinations where applicable and both budget levels, original masks plus explicit controls for missing inputs, 96-tick held runs, short and qualifying pulses, both BUD action orders, and stop/restart. Add same-tick ordering, observer pending suppression, head absence, equal-strength delivery and early retract cases. Repeat independent episodes on the same world to detect lost memory and pending work. Compare native and compiled outputs and sampling order, then exercise compile/reset/recompile continuation at represented phases. Keep unknown payload, invalid attachment, boundary escape and ambiguous ownership rejection checks.

The research test passed, including hash checks, original twelve-tick output reproduction and paired frame counts. Only the research harness and these analysis artifacts were changed for this investigation. Production implementation is the work proposed above. Java comparison, runtime replay at the 8x budget, and compiled reset/continuation equivalence remain outside the measured evidence.

Reproduce with a new output filename:

```powershell
$env:MCHPRS_TEST_PREFIX_COMPARISON_OUTPUT = 'F:/rustrepos/MCHPRS/test_data/piston-research/test-prefix-20261008/new-output-comparison.json'
cargo test -p mchprs_core test_prefixed_schematics_compare_interpreter_outputs -- --ignored --nocapture
```
