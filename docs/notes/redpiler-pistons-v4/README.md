# Redpiler pistons, attempt four

Status (2026-10-13): Steps 01-09 implemented; Steps 10-11 implemented as reported by the user; Step 12 (adders) implemented and differentially verified in this turn, awaiting human review. Side projects remain scoped on 2026-10-12.

The first deliverable is a readable, executable graph representing supported piston circuits without physical or interpreted redstone execution. The interpreter is the differential-test oracle. Generalized dust remains explicit initially; a later pass may eliminate it when power, updates, timing, and continuation are preserved. Pistons select compiled connections and publish events: the runtime has no physical Motion objects or progress simulation.

## Read first

Use [the initial Luna6 prompt](LAUNCH_PROMPT.md) to start Step 01 in a new session.

1. [Findings and agreed requirements](FINDINGS.md).
2. [Architecture, memory ownership, and proposed Rust shapes](ARCHITECTURE.md).
3. [Agent instructions and review contract](AGENT_GUIDE.md).
4. The single assigned step below, plus its referenced source files.

These notes are agent prompts as well as human review material. Names in struct sketches are proposed internal names; semantic responsibilities and ownership rules are requirements. The sketches are not instructions to introduce every type immediately.

## Implementation sequence

Implement and review one step at a time. A passed test does not authorize starting the next step. Steps 01-14 establish the general model and target coverage. Step 15 begins optimization only after that model is reviewed.

| Step | Small intermediate goal | Review evidence |
| --- | --- | --- |
| [01](steps/01-reference-contract.md) | Establish an independent comparison harness and pinned fixtures | Existing interpreter episodes and meaningful comparison failures |
| [02](steps/02-graph-and-memory.md) | Introduce the graph vocabulary and ownership model | Validated IDs, typed links, explicit state ownership |
| [03](steps/03-geometry-and-payload-parser.md) | Parse stationary geometry and bounded payload footprints | Read-only deterministic results and specific rejection witnesses |
| [04](steps/04-electrical-parser.md) | Parse guarded electrical connections | Directional, analog, conduction, and QC checks |
| [05](steps/05-callback-parser.md) | Parse ordered neighbor and shape routes | Exact directions, order, duplicates, and prospective heads |
| [06](steps/06-scheduler-and-components.md) | Execute graph callbacks and scheduled fixed components | FIFO/priority/deadline and observer parity |
| [07](steps/07-generalized-dust.md) | Execute ordered dust entirely over graph references | Seed/Turbo distinctions, snapshots, heading, and nested walks |
| [08](steps/08-piston-transitions.md) | Execute empty and single-payload piston transitions | Acceptance cancellation, contact switching, and timed publication |
| [09](steps/09-shared-payloads.md) | Execute shared payload transfer and recapture | One shared resource, both action orders, obsolete-event cancellation |
| [10](steps/10-activation-and-handoff.md) | Activate safely and return exact pending state on reset | No fallback, transactional compilation, uninterrupted continuation |
| [11](steps/11-test-family.md) | Cover the supported TEST examples | Admitted graph replays under meaningful stimuli |
| [12](steps/12-adders.md) | Validate ADDER1BIT, then ADDER_11BITS | Shared geometry, inhibition order, arithmetic windows |
| [13](steps/13-divider-and-anpu.md) | Validate Divider and noninstant ANPU behavior | Trigger/rearm episodes and actual noninstant graph execution |
| [14](steps/14-counters-and-fpu.md) | Validate counters, then FPU | Ordered data/update behavior, stop/restart, opcode histories |
| [15](steps/15-dust-optimization.md) | Remove provably unnecessary dust while retaining fast ordinary execution | Interpreter parity, preserved observables, paired performance evidence |

An unexpectedly large step should be divided during review into smaller, complete slices. Report the proposed split before implementing dependent work. Do not hide missing behavior behind TODO handlers, assumed outputs, or native execution.

## Completion of the first deliverable

- Supported TEST, adders, Divider, counters, FPU, and ANPU cases actually execute through the graph.
- Power, update delivery, event acceptance, selected topology, and future work remain distinguishable.
- Runtime redstone evaluation performs no spatial world reads and calls no physical piston, redstone, or dust executor. No progress, carried-block, or physical movement model is stored.
- Compile failure preserves the input world; reset reconstructs exact current state and pending work.
- Parser stages and graph operations explain themselves through stable IDs, source positions, named inputs, guards, and ordered traces.

Behavioral optimization and multithreading remain subsequent work. The user has additionally requested the inspection export/viewer and basic-pass research below; these do not change the core execution gates.

## Side projects

| Scope | Reviewable slices |
| --- | --- |
| [Side A: export and viewer](steps/side-a-export-and-viewer.md) | A1: read-only JSON export; A2: offline HTML inspection and recorded trace replay; true execution is a later A3. |
| [JSON format contract](GRAPH_JSON.md) | Versioned typed records, stage/capability declarations, provenance, logical snapshots, and ordered routes. |
| [Side B: basic passes](steps/side-b-basic-passes.md) | B1: source research and legality table; B2: reviewed local simplification/interning, before later runtime-sensitive rewrites. |
| [Side C: graph differential fuzzing](steps/side-c-differential-fuzzing.md) | C0: isolated piston replay and comparison; C1-C4: local environment sweeps, seeded histories, and failure reduction; C5-C6: shared resources and continuation after their runtime prerequisites. |

Each slice stops for human review. Side agents preserve the active core agent's files and must not implement unreviewed parser/runtime changes to satisfy an export or optimizer interface.

## Parser audit gate

After Step 05, use [the complete parser review checklist](STEP_05_REVIEW.md) to review Steps 02-05 together, run focused regressions and real fixture preparation, and compare query/route plans with interpreter procedures before starting Step 06.

The [2026-10-12 deeper audit](PARSER_AUDIT_2026-10-12.md) records counterexamples, fixes, and passing follow-up checks. Its supported static parser gate is approved; runtime parity remains for Steps 06-10.

## Later implementation direction

The [common compiler graph, extensible pass pipeline and single Graph runtime migration](GRAPH_PIPELINE_MIGRATION.md) has seven review steps. It replaces the earlier generalized-Direct proposal. G1–G3 remain implemented and await their first human review; the four review findings were fixed and checked in batch 02. G4 has a partial world-independent construction boundary plus focused command and note behavior. G5 has focused differential cases but remains incomplete. G6 has not activated ordinary circuits and has not retired Direct/Native. G7 has not started. The current capability matrix and limits are in the migration plan.

The user authorized [batch 01 for Luna6: G1–G3](BATCH_01_LUNA_PROMPT.md), with internal checks and one human review at batch completion. This does not claim prior human approval of G1–G3.

The user authorized batch 02 for Luna6: G4–G6. It closes the G1–G3 findings, adds world-independent Graph construction and focused command/note execution, and gives Hoppers and trapdoors explicit prepublication rejections with type, position, and reason. The pinned CPU_BubbleSort schematic now has the matching sticky piston head; its interpreter oracle sorts the selected RAM bank to `0..15` in 50,000 ticks. An earlier explicit Graph replay also sorted after Graph and interpreter received the same in-memory removal of the saved sign, with RAM matching through 10,035 active ticks. The stored schematic and sign remain intact, and public compilation with the sign still returns a structured support-loss error. The existing release CPU harness now has a short Graph mode: five fresh input pairs, 100 active ticks each, with compilation and RAM checks outside timing. It confirmed `BackendKind::Graph`, equal final RAM, and the same counters as the `cfg(test)` episode. Graph median was 4.396806 s (range 4.245987–4.582137 s); interpreter median was 0.634134 s (0.621264–0.646845 s). Graph processed 2,059,909 callbacks, 302,784 piston samples, and 27,884 piston actions in each run. CPU retained-table estimates peak at 411 MiB after callback construction; the old ~42 GiB preparation report remains unexplained. A separate unbounded test-trace run grew to 12.45 GiB during execution, while clearing trace vectors each tick kept sampled RSS near 0.54 GiB. Ordinary public compilation still selects Direct/Native for ordinary selections and broader G5 coverage remains incomplete, so G6 is not complete. Batch 02 stops at human review; see the migration plan for the capability matrix and validation record.

These are follow-up projects, not instructions to add speculative node variants during the steps above. Write and review their individual implementation prompts once the general graph provides concrete evidence.

| Work | Required justification |
| --- | --- |
| Piston/reset specialization | Preserve sampling, acceptance cancellation, response, reset, rearm, and shared-resource interactions; expose readiness as logical state when needed. |
| BUD memory specialization | Preserve independent data/update channels and acceptance-time behavior; missing normal input in one saved pose is insufficient. |
| Instant circuit grouping | Prove joint boundary behavior, event order, observer visibility, and resource dependencies; actor count or proximity is insufficient. |
| Export and visualization | Describe the typed graph and state with stable provenance and a reviewed versioned format. |
| Parallel execution | Prove independent event work and preserve observable ordering before introducing threads or synchronization. |

## Source authority

The current [redstone model](../../REDSTONE_MODEL.md), [piston model](../../PISTON_MODEL.md), [interpreter architecture](../../INTERPRETER_ARCHITECTURE.md), and Rust sources define reference behavior. Historical failed-attempt reports are evidence about those attempts, not reusable claims that a replacement executor passes.
