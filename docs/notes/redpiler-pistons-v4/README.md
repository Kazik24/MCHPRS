# Redpiler pistons, attempt four

Status: Steps 01-05 implemented and reviewed; the supported static parser audit is approved. Step 06 is next. Side projects scoped on 2026-10-12.

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

Each slice stops for human review. Side agents preserve the active core agent's files and must not implement unreviewed parser/runtime changes to satisfy an export or optimizer interface.

## Parser audit gate

After Step 05, use [the complete parser review checklist](STEP_05_REVIEW.md) to review Steps 02-05 together, run focused regressions and real fixture preparation, and compare query/route plans with interpreter procedures before starting Step 06.

The [2026-10-12 deeper audit](PARSER_AUDIT_2026-10-12.md) records counterexamples, fixes, and passing follow-up checks. Its supported static parser gate is approved; runtime parity remains for Steps 06-10.

## Later implementation direction

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
