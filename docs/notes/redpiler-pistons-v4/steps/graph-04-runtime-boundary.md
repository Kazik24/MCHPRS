# G4 — Graph construction boundary and ordinary coverage

Status (2026-10-13): partially implemented in batch 02 and awaiting human review. The constructor boundary and focused command/note behavior are present; ordinary coverage and G4 completion are not claimed. Read [the ownership sketches](../GRAPH_PIPELINE_MIGRATION.md).

## Goal

GraphBackend consumes the lowered general circuit program and prepared entry work. Keep its executor; fill ordinary coverage gaps deliberately. One shared facade around two unchanged executors is not completion.

## Scope

1. Inventory the current packed Direct component handlers, scheduling, observation, presentation and handoff against the general executor. Write a small coverage table before moving code. A handler missing from Graph is not necessarily absent from supported Direct circuits.
2. Construct GraphBackend from `LoweredCircuit`, without reading World to discover topology, imported tick targets or component behavior. Move remaining construction-time reads to preparation/lowering. Preserve original tick block type, current type, deadline, priority and duplicate membership, including stale imported ticks.
3. Retain the Graph scheduler, typed power queries, ordered route dispatch, generalized dust and piston/resource execution. Split methods into coherent modules only where this improves review/debugging. Keep one live state owner and one event phase implementation; do not relocate Graph into Direct.
4. Reconcile ordinary handlers against interpreter behavior. Graph's ordered notification semantics govern general execution; old Direct's publish-all-strengths-before-callbacks shortcut must not become a universal rule.
5. Keep sparse nested wire captures independent, stored wire strengths authoritative, electrical queries separate from Shape checks, and requests separate from acceptance. Preserve phase boundaries and event revision cancellation.
6. Preserve on-use, plate controls, inspect, presentation flush and full reset continuation. `io_only` cannot discard hidden state or future work on handoff.
7. Leave the existing packed path and selection available during review. Identify it explicitly as legacy execution; never hide Direct/Native inside Graph. Keep Native code untouched until the G6 retirement gate.
8. Unsupported Graph components have structured capability errors. Batch 02 requires command/note support in Graph; trapdoors may be explicitly rejected as a user-approved regression. Early refactoring preserves existing legacy entry paths; G6 retires them only after retained support passes the coverage gate. Follow the command/note host-boundary requirements in the batch prompt.

## References

`backend/direct/{mod,node,compile,update,tick}.rs`, `backend/graph.rs`, `backend/queue.rs`, `program_graph.rs`, `redstone/{comparator,repeater,observer,copper_bulb}.rs`, dust/piston source and model docs, `graph_tests.rs`, `graph_continuation_tests.rs`, `graph_test_family_tests.rs`.

## Required evidence

- Existing ordered callback, imported tick, nested dust, piston acceptance/cancellation, shared payload and continuation tests run through the reconstructed Graph backend, without Direct/Native execution.
- Prove no spatial runtime world reads using existing read-counting/test world helpers or a targeted instrumentation check; compile/flush/reset reads are separately identified.
- A normal component fixture and a supported TEST-family piston fixture execute through Graph with meaningful stimuli. Check intermediate observations and future work, not just final lamps.
- Validate constructor failures transactionally and state/node correspondence before publication.

## Batch 02 implementation notes

`prepare_graph_backend` lowers imported target type, remaining half-tick delay, priority, and compiled receiver binding before `GraphBackend::from_lowered`; construction itself has no World parameter. Graph now executes command and note behavior, captures note instrument/obstruction at the edge, and preserves presentation across reset. Piston state tracks head occupancy across transition publication phases. The pinned CPU_BubbleSort schematic now includes a matching extended sticky piston head at `(181,14,166)` for the piston at `(180,14,166)`; an in-memory Air-head variant still receives the structured missing-head error naming both positions. Focused tests cover matching and missing heads, stale/duplicate work, unloaded targets, command host preflight, synchronous command results, saved conditional state, missing command entities, note ticks, and reset handoff.

Hoppers and trapdoors now return explicit prepublication errors with block type, position, and reason, including when reached as callback dependencies. Other callback receiver coverage and ordinary runtime parity remain incomplete. The hostless command preflight conservatively requires a host for any due work in a graph containing a non-chain command block. See the batch coverage matrix in [the migration plan](../GRAPH_PIPELINE_MIGRATION.md); G5 and G6 remain gates.

## Non-goals

No packed-ID redesign, thread support, speed claim, truth-table replacement, legacy removal or production cohort switch. Keep capability additions separate from pure code movement; retain discovered divergences as explicit regressions and report them.
