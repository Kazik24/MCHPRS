# Compiled netlist port: implementation plan for Luna6

## Decision and scope

Keep Redpiler's general architecture. Replace the experimental event engine's
physical representation and preparation passes with a compiled component/event
netlist based on the observed reference engine. Adapt the executor to consume
that representation. This is not a rewrite of the server, world, public API,
established Graph backend, or pass framework.

The implementation is independent Rust code using MROWW/MCHPRS mechanisms.
The local decompilation explains algorithms and data contracts; it is not code
to paste into the repository.

The deliverable is a working compiler **and** executor, initially demonstrated
by ADDER1BIT, then ADDER11BIT and COUNTER. An inventory, passing observation
diagnostic, constructible plan, or isolated helper is not that deliverable.

This plan supersedes the implementation sequence and stale checkpoint in
`REDPILER_EVENT_ENGINE_IMPLEMENTATION_PLAN.md`. Retain that document as history.

## Current evidence

- The normal `Compiler` has Graph/Event backend selection. Graph is the default.
  The opt-in event branch still prepares and lowers the physical CircuitProgram.
- The public event backend admits an exact, closed single-lane arrangement.
  Its checked wire/piston/lamp path executes without physical wire settling.
- Publication, selected-cell restoration, rejected early inputs and host-write
  rollback have integration coverage. Preserve these safeguards.
- The experimental physical fallback is not a correct general engine:
  ADDER1BIT and ADDER11BIT diverge during preparation. Do not keep repairing
  global settling as the implementation route to the reference algorithm.
- ADDER1BIT inventory: 32 wires, 13 pistons, 10 resources, 9 observers.
  ADDER11BIT inventory: 295 wires, 142 pistons, 109 resources, 98 observers.
- COUNTER_BASIC also needs an ordinary piston with an empty lane. A model in
  which every piston action must move a resource is insufficient.
- MAIR replay confirms active-low adder ports. For ADDER11BIT, 0+0 stays zero;
  1+0 and 1023+1 produce 1 and 1024 in post-trigger ticks 3–7, then return to
  zero at tick 8. Do not substitute global quiescence for a declared window.
- The checked 37-wire experiment demonstrated useful acceleration, but it is
  source-to-dust execution with physical notifications, not the complete port.
  Its historical rates are not FPU or server TPS.

These are checkpoints, not promises that all current dirty files build. Inspect
and rerun the appropriate baseline before editing. Preserve unrelated changes,
especially concurrent `crates/redgear-api` work. Do not deploy or reset the tree.

## Architecture to preserve

```text
Compiler::compile(world, selection, options, actual entry work)
    preparation / inventory
    ordered compilation passes with progress, budgets and cancellation
    validation and lowering
    ready backend + snapshot, installed only after all preparation succeeds
    controls / ticks / flush / stop
```

Keep `ActiveBackend::Graph` and `ActiveBackend::Event`, `BackendKind`, the
existing experimental opt-in, TaskMonitor, public fallible operations and
selected-region lifecycle. Keep Graph execution and its tests as fallback and
oracle. Building-time reuse of Graph discovery facts is allowed; running Graph
to calculate results for an admitted event region is not.

The event branch must eventually stop calling the complete physical callback
pipeline just to reconstruct a different program. Reuse useful geometry and
power helpers directly, or use a temporary inventory adapter. Do not make the
new runtime depend on CircuitProgram sites, physical routes or captured-wire
buffers merely because that adapter exists.

`GraphPass`, `PlanBuilder`, PreparedGraph and LoweredCircuit currently have
physical-graph-specific contracts. Leave those contracts intact for Graph.
Add a small, explicit event preparation sequence with the same orchestration
conventions. Plain stage functions are sufficient; do not introduce a generic
multi-IR pass framework. Each event stage produces inspectable data and validates
its own invariants before the next stage runs.

## Runtime representation

Use one immutable `EventProgram` and one mutable `EventState`. Names can follow
existing conventions; do not create duplicate concepts where a type already fits.

| Immutable program | Mutable state |
| --- | --- |
| Components and main/side/quasi input definitions | Output strengths, powered/locked flags |
| Source-indexed electrical link ranges | Input contributions or level counts |
| Receiver-indexed contribution ranges | Active resource arrangement indices |
| Conditional link/group tables | Piston base/head/motion state |
| Rise/fall source event programs | Resource location and motion state |
| Departure/arrival event programs | Scheduled nodes and pending flags |
| Explicit BUD/observer watcher ranges | Piston FIFO and queued-action markers |
| Resource contacts and piston relationships | Motion completions and revisions |
| Output adapters and diagnostic positions | Dirty output IDs and reusable scratch |

Use contiguous Vec/Box arenas and offset/count ranges for the final tables.
Compilation may use maps and nested vectors, then flatten once. Use compact IDs
with checked conversion. Coordinates are compiler/host/diagnostic information,
not runtime identity or schematic-specific rules.

Separate electrical contributions from notifications. A link may adjust an input
without delivering a notification; an update may matter without changing final
strength. Represent the reference's ordered operations explicitly. Some paths
update contributions and notify in sequence; BUD handling has additional batching
and watcher rules. Do not replace all of this with one universal final-value flush.

Preserve multiple sources when one falls. A bounded scan of one receiver's
contributions is an acceptable first implementation; a whole-network scan is not.
Use per-level counts/masks if needed for the reference behavior and measured load.
Do not begin with unsafe pointers, packed bit tricks, SIMD or custom allocators.

The admitted execution path must not use:

- Global `settle_wires` or physical wire walks to calculate outputs.
- Recursive world/site/conducted-power discovery during ticks.
- Physical route interpretation as a substitute for compiled notification plans.
- Full immutable-table copies or construction of a second shadow executor per tick.
- Arithmetic shortcuts, fixture coordinates, fixed clock periods or output lookups.

Materialized wire values are permitted for selected watchers and presentation.
They must be derived from compiled dependencies and updated by represented events,
not normalized indiscriminately after unrelated controls change.

## Semantic baseline

First reproduce reference-style scheduled events, notifications and resource
motion. The old once-per-resource-per-computation-cycle latch is **not** the
reference's per-update-phase deduplication. Do not make it unconditional in the
new engine. Preserve it only as a separate, explicitly selected legacy/idealized
policy if an actual caller still needs it. Document any public transition; do
not silently turn `begin_computation_cycle` into a meaningless success.

Piston identity and resource identity are separate. Empty actions can succeed
without payload movement. Queued actions are deduplicated while queued by actor
and action, retain deterministic source identity, and are checked against live
power, availability and destination. Rejected actions consume no resource.

Model early retraction if the reference and target circuits require it. Until
represented, reject an unsupported control sequence before mutation. Never drop
the action or substitute an ordinary pull silently.

Compilation must preserve actual loaded state. A quiet scratch world used to
discover ordering is not permission to quiet the live world or normalize all
saved wire values. Source-to-receiver connectivity alone does not prove delivery.

## Step 0 — lock the executable reference contract

Read the existing fixtures and Graph/native replay code before writing a new
loader. Verify hashes, placement, orientations, initial raw states and ports.
Reuse `test_data/instant-pistons-io/fixtures/adder_1bit.json` and the corresponding
schematic, plus the existing one-bit adder tests. Resolve actual filenames with
`rg --files` if needed; do not invent a new copy of a fixture.

Reuse the isolated reference harnesses under `E:/codex-builds`. Record MAIR's
ADDER1BIT raw and decoded output for the same preparation and trigger sequence.
Include all declared operand/carry combinations and successive operations.
Record observation windows from the manifest and reference execution, rather
than assuming tick 24 or waiting for the whole circuit to stop.

Acceptance: one reproducible input/output contract, a failing new-engine test,
and a verified reference replay. Keep known failures visible and explicitly
runnable; a passing timeline diagnostic is not an equivalence test.

## Step 1 — introduce the event draft and inventory pass

Add the event data types and inventory builder to the existing Redpiler module
tree. Suggested ownership: `event_netlist` owns draft/program/state and runtime;
`analysis` owns event discovery; `passes` owns the short ordered event preparation
sequence; `event_backend` remains the host-facing wrapper. File splits should
follow real responsibilities, not create empty scaffolding for every step.

Inventory components, direct controls, fixed sources, movable resources, all
legal contacts, actor relationships, outputs and the complete entry-work scope.
Record ordinary and sticky pistons and empty actions. A resource shared by
several actors has one identity. Do not confuse read dependencies, notifications
and exclusive mutation ownership.

Use existing geometry facts where sound. If physical geometry preparation has
a narrower model than the reference, extend or replace that discovery stage
instead of treating its limitation as the final compatibility specification.

Validate stable IDs, contact consistency, entry state and selection crossings.
Use existing budgets and cancellation within expensive loops. Errors identify
the pass, reason and block/resource position. Compilation changes no world state.

Acceptance: ADDER1BIT produces an inventory that covers its 13 actors, 10 shared
resources and 9 observers without schematic-specific rules. This is an interim
checkpoint only; proceed immediately to the electrical/notification passes.

## Step 2 — compile electrical links

Implement the reference's bounded electrical extraction: direct, weak, strong,
conducted, diode rear/side and quasi inputs remain distinguishable. Trace dust
back to independent component/constant/resource sources, retaining the best
attenuation per relevant source. The reference's dust search reaches 14 edges.
Use existing power and wire-side rules; do not reinterpret a stored dot as a
universal disconnected source or mutate the loaded state to make extraction fit.

Produce source-indexed outgoing links and receiver-indexed incoming links.
Do not leave dust sites as unresolved physical input reads. Include stationary
inventory/override sources when required by the target; explicitly decline a
component whose analog/side behavior is not represented.

Validate zero/rise/fall, attenuation, masking sources and analog changes on
small supported cases. Compare extracted inputs with scratch Graph/native reads.
Electrical equality alone is not sufficient admission for effectful receivers;
Step 4 supplies their notifications.

Acceptance: complete unconditional ADDER1BIT electrical extraction, with any
resource-dependent connections identified for Step 3. No runtime wire evaluator
is added as a shortcut around missing links.

## Step 3 — compile resource arrangements and connection changes

Follow the behavior of the reference's ProbeSingles/ProbeJointly:

1. Extract dependencies for candidate resource positions in scratch state.
2. Compare changed connections and identify which resources influence them.
3. Grow local interacting groups when combinations change those dependencies.
4. Enumerate supported arrangements and lower their link changes.

Group by actual electrical/geometric interaction, not by arbitrary callback
connectivity or membership in one plot. Avoid a product over the entire circuit.
Use a documented, configurable compilation budget through existing mechanisms;
the inspected 4,096-arrangement bound is reference evidence, not a proof that
all larger circuits are invalid. Report which local group exceeded a bound.

Include absent/in-motion resource states where they affect power and geometry.
Lower arrangement changes to ranges of link additions/removals or equivalent
indexed operations. Moving a resource updates its groups and withdraws/adds
contributions using current source outputs; no fresh topology search is needed.

Acceptance: ADDER1BIT's resources are represented across relevant positions,
including shared contention and a dependency influenced by multiple resources.
Validate arrangement-index transitions and contributions in both directions.

## Step 4 — compile ordered notifications and watchers

This pass is essential, not optional cleanup after electrical compilation.
Port the reference behavior of source ordering, MoveRecorder, CellWatchers,
HomeWatchers, WireWatch and BUD relationships.

- For relevant sources, obtain rise/fall receiver ordering from a quiet scratch
  replay, retaining constants and the reference's qualification rules.
- Retain other applicable targets in the source program; observing one reaction
  is not permission to omit remaining targets.
- Record first-occurrence departure/arrival ordering and distinguish shape
  events from ordinary updates. Do not indiscriminately notify nearby observers.
- Compile the source lists for watched wires, and index those watchers by the
  sources/resources that can affect them.
- Preserve BUD checks that matter when the final source output is unchanged.
- Keep per-phase notification deduplication distinct from queued piston-action
  deduplication and from any optional computation-cycle policy.

Scratch observations establish the chosen reference-compatible model, not a
universal proof of native equivalence. Document its assumptions and reject
unrepresented dependencies. Do not require preservation of the prototype's
global sweep history or copy its duplicate physical-route events blindly.

Acceptance: every ADDER1BIT observer/reset dependency has an executable compiled
description. A resource departure/arrival triggers a checked observer pulse and
connected reaction without physical settling or physical-route traversal.

## Step 5 — validate and lower the compiled program

Consume the event draft and emit immutable flat tables plus initial mutable state
and supported entry work. Validate all ranges, IDs, strengths, arrangement indices,
resource owners, receiver operations, event bounds and output adapters.

No incomplete plan may acquire ownership. All effects reachable inside the
selected region must be represented or reported unsupported. Initial support can
require a closed quiescent selection; inspect actual entry scheduler/piston work,
not just component flags. Keep unrelated work outside ownership intact.

Do not validate by requiring exact component counts or an exact relative lane
layout. Those checks describe the current public demonstration, not the new
general representation. Replace them only when the new plan validates the effects
they previously excluded.

Acceptance: a sealed ADDER1BIT program with inspectable component, link, condition,
watcher and movement tables. No physical CircuitProgram is needed to execute it.

## Step 6 — execute the plan and pass ADDER1BIT

Reuse TickScheduler, but match the reference's priority, insertion, phase and
rescheduling behavior. Use one explicit mutable state, with deterministic queues:

```text
control/source event -> compiled contribution and notification operations
game tick -> due scheduled operations in priority/FIFO order
          -> piston FIFO, accepting against live state
          -> motion/arrival stage and its compiled notifications
```

Preserve the reference's distinctions about callbacks created during each phase.
Do not force every movement-generated event into the same immediate recursion.
Track request identity and action; distinguish accepted piston actions from
payload moves. Handle empty motion and supported early retraction. Rearm physical
clock/reset mechanisms through their actual state, without a hard-coded period
or host-side frame call on every tick.

Use persistent queues and reusable scratch. Share immutable plans. Preserve
atomic runtime failure using prevalidation and bounded rollback/journaling as
needed; do not remove safety merely to avoid copies. Avoid nested snapshots and
full-plan clones. Measure before introducing a more elaborate state journal.

Acceptance: all declared ADDER1BIT input combinations and repeated operations
pass at the specified output windows through this executor. Record compiled
operations and assert zero physical settling and zero shadow Graph ticks.
Keep a small regression for each semantic defect found along the way.

Steps 1–6 form ONE vertical deliverable. Do not stop at a passing extraction,
observer helper, inventory or observation diagnostic and label the port complete.

## Step 7 — connect the existing public backend and lifecycle

Replace the experimental backend's dependency on PhysicalSignalNetwork and
CompiledPistonActors with the validated event program/state. Preserve the public
Compiler API, explicit opt-in and established Graph default. Change the event
preparation branch in `redpiler/mod.rs`; do not refactor unrelated Graph passes.

Adapt existing projection/snapshot helpers to the new output/contact state.
Validate and stage writes, handle refusal after earlier writes, retain recoverable
state, and freeze execution if restoration/rollback cannot complete. Reset-only
exit remains allowed. Restore before applying an edit; preserve outside work.

Keep compile failure, cancellation and replacement atomic. Active Graph must be
reset first; do not migrate its private queues implicitly. Keep the normal-library
integration test and add ADDER1BIT through the same public path. Session/headless
support must use this path, not an alternate testing constructor. Coordinate
`redgear-api` changes with their owner instead of overwriting concurrent work.

Before player activation, cover stop, edit, error, replacement, save and unload.
Do not silently save an approximated internal state as native-continuable state.
Keep the selected snapshot until any fallible restoration has succeeded.

Acceptance: public/headless ADDER1BIT execution, world output checks, all host
write/rollback tests and lifecycle behavior. No player deployment in this plan.

## Step 8 — scale the same algorithm to the real fixtures

1. ADDER11BIT: replay prepared 0+0, carry chains, overflow and successive operands.
   Check active-low raw and decoded outputs in the declared window. Use the
   existing preparation divergence as a regression, not as an expected result.
2. COUNTER_BASIC: admit its ordinary empty-lane piston and supported controller.
   Verify changing counts, clock continuity, reset and restart without host
   computation-cycle declarations artificially driving each operation.
3. Divider and CPU_BUBBLESORT: verify connected resources, sustained updates and
   meaningful results using the pinned fixtures/reference histories.
4. FPU_27: changing operands/opcodes, successive frames and reset/restart.
   Respect the user-confirmed 0x412345678 -> 0x84f3 oracle and the invalid
   0xffff result for 0x558e1d715. Native parity alone is insufficient for the
   known broken physical vector; report disagreements without inventing answers.

Use the same representation and passes throughout. Expand missing general
features based on concrete graph dependencies, never schematic-specific rules.
Where semantics differ between reference and native, record both and declare the
supported contract. A completed reference port does not imply universal Minecraft
compatibility or make an incorrect arithmetic result acceptable.

## Step 9 — performance, unsupported regions and retirement

Measure compile time separately from execution. Warm up the reference and run
at least three sequential uninstrumented Rust repetitions with identical controls,
sampling and publication costs stated. Measure wall time, ticks, component/link
work, notifications, piston actions, resource moves, allocations and copied bytes.
Count actual work, not only one subloop. Keep output assertions in timed workloads.

Reproduce the small reference comparisons, then benchmark the real circuits.
Report isolated fixtures separately from saved plots/server throughput. The
historical FPU 15–20k initial target/35k stretch remains unproven until changing
inputs and required outputs pass on the actual workload. No speed claim follows
from recognizing actors or compiling successfully.

Initial fallback can choose Graph for an entire rejected selection. After the
complete executor is sound, support independent closed regions without merging
their clocks or resources. Mixed execution requires explicit read, notification
and mutable-resource adapters; an unsupported neighbor must not silently suppress
supported work or share a resource with two executors. Defer arbitrary partitioning
rather than reintroducing the former whole-plot ownership-closure problem.

Delete replaced experimental physical settling, test-only selectors, duplicate
actor runtimes and obsolete plan types once no retained caller uses them. Keep
the established Graph backend and native tests. Archive failure explanations in
documentation; do not retain unused runtime abstractions as a history mechanism.
If high-quality but incomplete work must be retained, leave it explicitly inactive
with a concise reason. Do not restore deleted legacy instant passes wholesale.

## Working and reporting rules for Luna6

- Inspect `git status` and applicable repository instructions before editing.
- Implement one coherent slice at a time, carrying it into the real adder gate.
  Use the listed stages for debugging, not as excuses for disconnected scaffolding.
- Read callers and reuse the fixture, scheduler, power and lifecycle mechanisms.
- Do not replace implementation work with repeated inventories or passing
  diagnostics. If blocked, show the exact first failing operation and attempted
  patch; make the next experiment narrow enough to execute.
- No coordinates, schematic names or arithmetic functions in runtime rules.
- No arbitrary increases to waits, normalization of loaded worlds, silent policy
  changes or loosened ownership checks merely to obtain admission.
- Keep source stable during a build and coordinate benchmark CPU use.
- Run meaningful focused checks, then relevant Graph/classical/public regressions.
  Format touched files and included test fragments; do not reformat unrelated work.
- Report real circuit, chosen path, output window, expected/observed outputs,
  execution counts, tests and exact remaining unsupported feature.
- Save durable fixtures outside `target`; preserve untracked files in checkpoints.
- No deployment, merge or live-world activation during implementation.

## Reference and reproduction material

- `scraps/docs/REDGEAR_ENGINE_INVESTIGATION.md`: detailed local binary analysis.
- `E:/codex-builds/RedGear-mair-decompiled`: compiler/runtime behavior reference.
- `E:/codex-builds/mair-adder-semantics-20261029/REPORT.md`: checked ADDER11BIT
  polarity and output timelines, plus reproducible harness.
- `E:/codex-builds/reference-fixture-bench-20261029/REPORT.md`: small checked
  reference benchmark; projection costs differ from the historical Rust timings.
- `crates/core/src/redpiler/backend/graph/tests/adder.rs`: pinned loading and
  Graph/native adder histories, including boundary/ordering tests.
- `crates/core/src/redpiler/event_netlist/tests/real_circuits.rs`: current failing
  real-circuit gates and causal observations; retain useful failures.
- `crates/core/tests/event_execution.rs`: normal-library execution and safety gates.

Reproduction reports must include fixture/DLL identities, input order, raw and
decoded observations, tick units and any importer omissions. Keep local E: paths
as research aids; repository tests must use checked repository fixtures and must
not require that particular machine's decompilation directory to run.
