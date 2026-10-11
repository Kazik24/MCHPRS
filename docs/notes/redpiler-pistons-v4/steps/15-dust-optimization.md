# Step 15: Certified dust elimination and preserved ordinary speed

Status: not started. [Step 14](./14-counters-and-fpu.md) must be reviewed.

## Agent prompt

Implement only this step using [the agent guide](../AGENT_GUIDE.md), [findings](../FINDINGS.md), and [architecture](../ARCHITECTURE.md). Follow Ponytail: the smallest complete, readable change with meaningful checks. Preserve user changes. Stop with review evidence before continuing.

## Intermediate goal

Begin optimization with a small, explainable pass that removes safe dust detail while preserving the reviewed execution contract.

## Read these sources

- [pass pipeline](../../../../crates/core/src/redpiler/passes/mod.rs), [existing identification](../../../../crates/core/src/redpiler/passes/identify_nodes.rs), and [input search](../../../../crates/core/src/redpiler/passes/input_search.rs).
- [direct lowering](../../../../crates/core/src/redpiler/backend/direct/mod.rs), [packed links](../../../../crates/core/src/redpiler/backend/direct/node.rs), and architecture section 13.
- [all earlier step checks](../README.md#implementation-sequence), [Turbo tests](../../../../crates/core/src/redstone/wire/turbo_tests.rs), and the existing benchmark support under [core benches](../../../../crates/core/benches/support/cpus.rs).

## Scope and implementation

Optional optimization milestone after the graph representation is approved. Start with static callback-insensitive dust cones. Required generalized construction must remain independent of -O, and uncertain dynamic regions retain their original graph.

1. Define the exact eligibility certificate before implementing the rewrite: preserve every consumed directional/analog channel, update source/delivery/order, observer visibility, scheduler history, and reset representation.
2. Analyze callback and geometry liveness beyond immediate electrical neighbors. Turbo's 24-site stencil, prospective heads, shape delivery, unchanged-value notifications, and shared contact switching can make a distant wire relevant.
3. Rewrite only certified regions. Use the existing attenuation/ordinary lowering machinery where semantically equivalent; do not force new callback families through packed Default/Side links.
4. Preserve the existing fast direct path for unchanged ordinary circuits and provide a reviewed fast lowering for equivalent generalized ordinary graphs. Avoid implementing a second native domain or a whole mixed-backend framework to optimize a small cone.
5. Account for imported stale dust and materialization. A removed wire needs an exact justified reconstruction or retained state when its history is observable; blindly solving equilibrium on reset is incorrect.
6. Keep optimization disabled/on comparisons explicit. A failure of certification retains the original region; it is not an admission failure for an otherwise supported circuit.
7. Measure operation/node reductions and paired timings on active ordinary and selected piston episodes using identical fixtures, protocols, build mode, observation handling, and fresh initial states. Run repeated paired samples; include correctness checks in the measurements.
8. Record why each removal is safe and explain any speed change. Do not claim Counter/FPU acceleration from old native-domain reports or imply a universal speed ratio.

## Acceptance checks

Compare unoptimized graph, optimized graph, and interpreter on the certified static cases, observer-sensitive negative cases, stale imported wire states, analog consumers, same-value callbacks, and mid-execution reset. Rerun the relevant reviewed target matrix after rewriting shared infrastructure. Collect repeatable ordinary-path timings and active selected-circuit timings rather than idle admission benchmarks.

Verify new tests are registered and actually run. Use the targeted release/locked workflow in the agent guide. Preserve frozen fixture/reference data.

## Human review gate

Review the certificate and one before/after graph with preserved boundary traces. Review correctness before interpreting paired timings; no removed region may rely on an unenforced input protocol.

One narrowly scoped dust rewrite is proven against its contract and measured, while uncertified graph regions and ordinary existing execution remain correct.

## Leave out

No instant-group, BUD-memory, or adder specialization; no export product, graph UI, multithreading, broad cache framework, or guaranteed speedup for every circuit.

## Handoff

Report changed files, invariants, exact test commands/counts/results, and remaining risks. Further instant/BUD specialization, export, visualization, and threading require separately reviewed plans. Stop here.
