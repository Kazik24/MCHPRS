# Layered instant compiler: implementation plan

Status: implementation in progress; no new compiler enabled yet. Started 2026-10-18
from checkout `ba9bfb6`, following the plan written against `d917e56`.
Concurrent source/report changes belong to other work; inspect the worktree before editing.
This plan does not enable a compiler, modify a fixture, or report a speedup.

The Step 01 corpus inventory and executed-versus-pending coverage are maintained in
[FIXTURE_COVERAGE.md](FIXTURE_COVERAGE.md). Oracle port counts are kept separate from
the later boundary pass's discoveries; physical observation sites may share one signal.

## Objective

Compile instant circuitry into a small virtual circuit derived from graph connections,
logical cell contracts and causal activation layers. Eliminate covered internal wires,
Turbo walks, reset propagation and unnecessary notifications from execution. Preserve
boundary signals, ordinary/BUD memory, required updates and scheduled consequences.

First cover all computation actors in ADDER1BIT; then ADDER11BIT, Divider and FPU.
Use Counter to check BUD isolation and CPU_BubbleSort to check mixed-domain performance.
ANPU is a control for ordinary non-instant execution, not a target to misclassify as instant.

The change in abstraction is deliberate:

```text
prepared Graph
  -> identify instant/classic boundaries             compiler pass 1
  -> extract regions and logical cells
  -> derive guarded causal layers                    compiler pass 2
  -> reduce to output/state/effect expressions
  -> lower into the existing Graph program
```

Layers are construction iterations inside the second pass. Do not register a separate
optimization pass for every layer. Do not symbolically interpret the generic Turbo algorithm.

## Read in this order

1. [Architecture and algorithms](ARCHITECTURE.md): contract, representations and invariants.
2. [Removal/replacement/reuse map](MIGRATION.md): exact responsibilities and retirement gates.
3. The next approved implementation step below. Each is an agent handoff and a human review gate.
4. [Central edge-case register](../INSTANT_COMPILER_EDGE_CASES.md): extend this single register.

| Step | Reviewable result | Status |
| --- | --- | --- |
| [01 Contract and oracle](steps/01-contract-and-oracle.md) | Atomic-edge protocol, projected observations and a measured single-use census. | Expanded corpus tests passed; awaiting human review |
| [02 Local cells](steps/02-local-cell-contracts.md) | Graph-derived buffer/inverter/AND/OR equations and resource/reset witnesses. | In progress |
| [03 Boundaries](steps/03-boundary-pass.md) | Complete, typed instant/classic interfaces from one discovery implementation. | Planned |
| [04 Guarded layers](steps/04-regions-and-layers.md) | Input-to-output tracing over cells, with cycles and causal ordering explicit. | Planned |
| [05 Reduction and lowering](steps/05-reduction-and-lowering.md) | Compact immutable programs with dead internal work absent. | Planned |
| [06 Runtime replacement](steps/06-runtime-and-boundary-effects.md) | Real kernel execution, exclusive ownership and boundary effect adapters. | Planned |
| [07 Reset and handoff](steps/07-reset-and-handoff.md) | Small reset continuations, recurring waves and safe interpreter continuation. | Planned |
| [08 Scale and retirement](steps/08-validation-performance-and-retirement.md) | Larger-fixture correctness, paired performance and removal of superseded paths. | Planned |

Do not batch implementation of steps 01-04 before their contracts have been reviewed.
The user's end-to-end implementation instruction authorizes continuing after each internal
review and its tests; the checkpoints do not require asking for permission again.
The old project failed partly because runtime scaffolding grew before a useful region
could compile. Each step must produce a concrete artifact and stop at its review gate.

## Required semantic contract

- One declared logical input edge samples one input frame. A shared game-tick number alone
  does not make independent lever operations simultaneous. Native references use the same protocol.
- External data is stable through the admitted computation/reset transaction. Any supported
  boundary event can activate a region; no dedicated lever, common trigger or global-idle gate.
- A recognized actor has at most one accepted **computation use per wave**. Failed samples
  do not consume it. Reset/restoration is a separate role, and another wave can reuse it.
- Stable inputs and single-use do not prove order independence. Compiler DFS/BFS order must
  not become circuit timing. Preserve relevant nanotick dependencies or decline the candidate.
- An instant feedback edge cannot refire a consumed computation use in the same wave. A
  not-yet-consumed actor may become eligible after another input arrives.
- Ordinary memory, including BUD memory, remains outside the kernel. Power availability and
  permission to sample are separate channels. Same-value qualifying updates can matter.
- Outside callback order within a logical tick may change only under the existing user
  relaxation. Preserve recipients, directions, meaningful multiplicity, pulse consequences,
  deadlines and memory. Internal order which changes results is not relaxed.
- Runtime motion/progress objects are unnecessary. Keep logical phases and required future
  work. Reconstruction may wait a bounded number of ticks for a certified safe cut.
- Unsupported cases retain ordinary Graph execution. That fallback is not compiler coverage.

The single-use rule is an admission obligation, not an interpreter theorem. If a required
fixture violates it, show the smallest history and request a design decision; do not erase
the second use or silently redefine the wave until every event fits.

## Agent instructions

Use the repository's Ponytail and pass hygiene rules. Every new pass is one `.rs` file under
`crates/core/src/redpiler/passes/`. Pass files orchestrate focused analysis functions. Shared
helpers belong in `passes/utils/` only when multiple consumers actually need them. Do not
import one pass implementation from another.

Keep Graph as the sole executor. Use `PlanBuilder`, `PlanOutput`, existing typed graph IDs,
`CircuitProgram`, `CircuitState` and the existing scheduler. Do not create another World,
interpreter, runtime backend, general symbolic physics engine or profiling framework.

Code must have a straight flow and named intermediate values. Every struct field and enum
payload needs a short explanation. Explain why a complex operation is correct. Avoid one
large handler with flags selecting unrelated modes. Keep tests in the existing tests folder.

No fixture-name/coordinate/hash dispatch, hand-written adder equations, global input-mask
enumeration, JIT, runtime training, full-state transcript cache or complete-device recurrence.
Small exhaustive tests of a cell contract are verification, not the production compiler.

Reuse installed dependencies. The old `redpiler_opt` crate and generic symbolic compiler
were removed: do not restore them. Start with a small Boolean expression arena owned by this
compiler. Add a solver dependency only after a measured, specific proof need and review.

Only publish complete, validated plans after cancellation checks. Analysis budgets limit
work/storage, not arbitrary operand width. Return a witness on decline. Do not increase a
budget repeatedly to conceal exponential tracing.

## Existing sources to inspect

Full paths below are relative to the repository root. Short `passes/`, `analysis/`,
`program_graph/` and `backend/` paths are relative to `crates/core/src/redpiler/`.
These sources existed at the scoped checkout.

- `docs/PISTON_MODEL.md`, especially sections 2, 4 and 5.
- `docs/REDSTONE_MODEL.md`, `docs/INTERPRETER_ARCHITECTURE.md`.
- `crates/core/src/redpiler/passes/README.md`, `mod.rs`, `planning.rs`, `rich.rs`.
- `passes/utils/instant_mechanisms.rs`: current narrow reset-mechanism recognition.
- `analysis/instant_regions.rs` and `analysis/instant_regions/composition.rs`: existing ownership/boundary discovery.
- `program_graph.rs`, `program_graph/instant_regions.rs`, `execution_plans.rs`, `validation.rs`, `lowering.rs`.
- `backend/graph.rs`, `backend/graph/pistons.rs`, `sites.rs`, `power.rs`, `reset.rs`, `projection.rs`.
- `backend/graph/instant_proxy.rs` and `instant.rs`: existing route fusion and exact reset-wire specialization.
- `passes/compile_classic_redstone.rs`, `program_graph/classic_redstone.rs`, `backend/graph/classic.rs`.
- `backend/graph/tests/adder.rs`, `boundary_contract.rs`, `fixed_point_adder.rs`, `stable_region.rs`, `continuation.rs`.
- `crates/core/src/redstone/instant_piston_tests/reference_contract.rs`: native ordered observations and projections.
- `crates/core/benches/support/graph_short.rs` and `cpus.rs`: fixture loading and existing paired measurements.

Historical symbolic/kernel documents describe removed experiments. Reuse their counterexamples
and lessons, not their architecture or old validation counts. Check current paths before coding.

## Delivery format for each implementation agent

Report the implemented scope, files, exact discovered/executed tests, and unsupported cases.
For accepted regions report recognized actors, contracted actors, executed kernels, remaining
general callbacks/walks and real retained memory separately. Show the smallest failing witness.
Do not call a positive kernel counter acceleration. Do not commit or modify unrelated work
unless asked. End at the review gate; broad benchmarks belong to step 08.
