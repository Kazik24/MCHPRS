# G4 — Graph construction boundary and ordinary coverage

Status (2026-10-13): implemented in this batch and awaiting human review. Graph construction consumes lowered data without World; command, note, ordinary, and piston paths now execute in the unified Graph runtime. Read [the ownership sketches](../GRAPH_PIPELINE_MIGRATION.md) for coverage limits.

## Goal

GraphBackend consumes the lowered general circuit program and prepared entry work. Keep its executor; fill ordinary coverage gaps deliberately. One shared facade around two unchanged executors is not completion.

## Scope

1. Inventory the current packed Direct component handlers, scheduling, observation, presentation and handoff against the general executor. Write a small coverage table before moving code. A handler missing from Graph is not necessarily absent from supported Direct circuits.
2. Construct GraphBackend from `LoweredCircuit`, without reading World to discover topology, imported tick targets or component behavior. Move remaining construction-time reads to preparation/lowering. Preserve original tick block type, current type, deadline, priority and duplicate membership, including stale imported ticks.
3. Retain the Graph scheduler, typed power queries, ordered route dispatch, generalized dust and piston/resource execution. Split methods into coherent modules only where this improves review/debugging. Keep one live state owner and one event phase implementation; do not relocate Graph into Direct.
4. Reconcile ordinary handlers against interpreter behavior. Graph's ordered notification semantics govern general execution; old Direct's publish-all-strengths-before-callbacks shortcut must not become a universal rule.
5. Keep sparse nested wire captures independent, stored wire strengths authoritative, electrical queries separate from Shape checks, and requests separate from acceptance. Preserve phase boundaries and event revision cancellation.
6. Preserve on-use, plate controls, inspect, presentation flush and full reset continuation. `io_only` cannot discard hidden state or future work on handoff.
7. During migration, keep any remaining legacy selection explicit; never hide Direct/Native inside Graph. The completed G6 work removed both legacy executors after replacing their retained supported component paths.
8. Unsupported components have structured capability errors. Hoppers and trapdoors remain explicit prepublication rejections; command/note support remains in Graph. Follow the command/note host-boundary requirements in the batch prompt.

## References

`backend/graph.rs`, `backend/queue.rs`, `program_graph.rs`, shared `components.rs`, `redstone/{comparator,repeater,observer,copper_bulb}.rs`, dust/piston source and model docs, `graph_tests.rs`, `graph_continuation_tests.rs`, `graph_test_family_tests.rs`. The former Direct sources are historical review material only.

## Required evidence

- Existing ordered callback, imported tick, nested dust, piston acceptance/cancellation, shared payload and continuation tests run through the reconstructed Graph backend, without Direct/Native execution.
- Prove no spatial runtime world reads using existing read-counting/test world helpers or a targeted instrumentation check; compile/flush/reset reads are separately identified.
- A normal component fixture and a supported TEST-family piston fixture execute through Graph with meaningful stimuli. Check intermediate observations and future work, not just final lamps.
- Validate constructor failures transactionally and state/node correspondence before publication.

## Batch 02 implementation notes

`prepare_graph_backend` lowers imported target type, remaining half-tick delay, priority, and compiled receiver binding before `GraphBackend::from_lowered`; construction itself has no World parameter. `tick_with_world` publishes the Graph logical clock at the host boundary without discovering topology. Graph executes command and note behavior, captures note instrument/obstruction at the edge, and preserves presentation across reset. Piston state tracks head occupancy across transition publication phases. The pinned CPU_BubbleSort schematic has a matching extended sticky piston head at `(181,14,166)` for the piston at `(180,14,166)`; an in-memory Air-head variant still receives the structured missing-head error naming both positions. Focused tests cover matching and missing heads, stale/duplicate work, unloaded targets, command host preflight, synchronous command results, saved conditional state, missing command entities, note ticks, and reset handoff.

Hoppers and trapdoors return explicit prepublication errors with block type, position, and reason, including when reached as callback dependencies. The hostless API conservatively asks for a host before any due work in a graph containing a non-chain command block; this prevents partial command consumption while leaving circuits without command blocks hostless. G4's exercised cases pass in debug and release; broad history coverage remains a review limit, not an alternate executor.

## Non-goals

No packed-ID redesign, thread support, truth-table replacement, or later circuit specialization. G6 legacy retirement is complete for the explicitly supported subset; retain discovered divergences as explicit regressions and report them.
