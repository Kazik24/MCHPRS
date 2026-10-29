# World-backed tracer compiler: implementation scope

29 October 2026. Implementation handoff for Luna6. This refines the front-end
and notification work in `REDPILER_COMPILED_NETLIST_PORT_PLAN.md`; retain that
plan's executor, admission, publication and reset requirements. No runtime
implementation is delivered by this document.

## Decision

Build the experimental event compiler from an immutable world snapshot and
scratch interpreter probes. Keep EventProgram/EventState as the runtime target.
Keep ordinary Graph as fallback and comparison, with its existing pipeline.
The completed event compile path must not require rich::prepare, LoweredCircuit,
physical callback routes or Graph execution to compute a frame.

The goal is reference-style compatibility and useful speed under the approved
experimental contract: defined output boundaries, idealized intermediate timing,
and reset-only exit. This is not a whole-circuit fixed-point evaluator. Source
transitions, scheduled components, piston requests, resource movement and watcher
updates still determine results and must have explicit representations.

First acceptance is ADDER1BIT's complete vector suite and successive operations;
then ADDER11BIT, COUNTER, CPU and FPU_27. No circuit names or coordinates belong
in compiler rules. Large-plot speed is a later measurement, not inferred from
admission or small fixtures.

## What the reference actually does

The following is a functional reading of local decompiler output, not a claim
of exhaustive binary equivalence. Paths are under
`E:/codex-builds/RedGear-mair-decompiled`. Implement independently in Rust using
existing MCHPRS helpers. Preserve reference evidence outside build output.

| Reference stage | Observed behavior | Local source anchor |
| --- | --- | --- |
| Inventory | Scan blocks, components, pistons, shared movable tokens and their possible cells; describe loaded component states | `MAIR.Core.Rsc.Compilation/Compiler.cs`, Prepare at 1908; Build constructor and DescribeNodes |
| Electrical extraction | Extract source/reader links, diode sides and piston power. Walk dust with minimum attenuation and a 14-edge bound; retain constants | Compiler's extractor and AddLinks around 540 |
| Resource alternatives | Probe token positions in scratch worlds, compare reader inputs and wire source lists; grow interacting groups, then enumerate their arrangements | ProbeSingles 1205, ProbeJointly 1219, RecordInputs 1337 |
| Watcher construction | Nearby wires become hidden WireWatch nodes; each piston watches wires at Manhattan distance at most two, applicable component update footprints and adjacent movable cells. Observers watch their facing cell | BuildWatchLists 147, WatchAroundPiston 214, WatchWire 275 |
| Source ordering | Select sources feeding at least two racing receivers. Quiet dynamic sources, preserve constant-redstone-block dust power, switch source on then off; record first qualifying receiver callback separately for each direction | SourceOrdering 1356, BuildQuietPlot 1414, OrderRecorder 1570 |
| Movement ordering | For each token cell, probe departure and arrival separately; record first occurrences including watched wires, combine with cell watchers and observer placement rules | MoveRecorder 1646–1747 |
| Initially reactive components | Identify saved components whose state disagrees with their input and has no applicable pending work; build nearby-wire/source and component-neighbor indexes | TrackBudded 1753, Logic.CanReact in `MAIR.Core.Redstone/Logic.cs` at 500 |
| Runtime | Execute compact source programs, scheduled priority buckets, piston FIFO and motion landing work. Switch arrangement links, evaluate watched wires and deliver applicable updates | `MAIR.Core.Rsc.Engine/Engine.cs`, PropagateChange 2116, UpdateBudded 2187, WireWatch 2335, RunMoveSteps 2943 |

Important distinctions:

- This is not exclusively tracing. Electrical extraction, geometry-based
  watchers, sampled ordering and resource arrangement enumeration all contribute.
- OrderRecorder only records an indexed pending target's first qualifying
  non-wire callback. Observer handling is special; other targets must read the
  sampled transition's input polarity. Copying every callback into runtime is
  not the reference algorithm.
- MoveRecorder uses a different capture rule: first indexed component or
  watched-wire occurrence, combined with explicit cell watchers. It does not
  reuse OrderRecorder's polarity filter.
- TrackBudded's inspected CanReact predicate includes repeaters, comparators,
  torches, lamps, trapdoors and note blocks; it does not include pistons.
  Piston wire watchers and initially reactive component handling are separate.
- Runtime wire watchers recalculate max source strength with attenuation and
  notify on a changed wire strength, including positive-to-positive changes.
  A Boolean-only wire cache loses such updates.
- In RunMoveSteps, changed-link marks, explicit watcher flags and changed-wire
  relationships control reactions. Contribution refresh and notification are
  different instructions. There is additional post-step handling of switched
  nodes and changed-wire watchers; one blanket final-value flush is insufficient.
- Initial wire/component values come from the loaded world. A quiet probe is
  not permission to quiet or recompute the running circuit at entry.
- Observed limits include 4,096 arrangements per group, six cells and two tokens
  for certain clusters. These are compatibility/budget limits, not validity proofs.
- A sampled quiet-world order is not exhaustive over simultaneous sources or
  alternate geometry. Record and test the supported abstraction explicitly.

## Fit to the current compiler

`Compiler::compile` already receives `&W: World`, bounds, tick entries, options
and TaskMonitor. CompilerInput already contains world, bounds and ticks.
PassContext currently contains monitor/options/EntryState, but no world.
The event public branch still invokes rich preparation and lowering. The
independent EventProgram compiler currently translates those physical facts.

Use a borrowed compile-scoped world reference. Do not store it in EventProgram,
EventState or a backend after activation. Do not let passes mutate the live world.
All probes operate on private scratch state built from the same captured entry.

Suggested minimum API shape (illustrative, not a required new trait framework):

```rust
fn prepare_event<W: World>(
    input: &CompilerInput<'_, W>,
    options: &CompilerOptions,
    monitor: &TaskMonitor,
    entry: &EntryWork,
) -> Result<PreparedEvent, EventCompileError>;
```

Pass ordinary references into plain stage functions. Share one captured snapshot,
entry inventory and progress/budget context rather than recapturing per stage.
Existing Graph passes can gain read-only world access if a specific caller needs
it, but do not parameterize every GraphPass/PlanBuilder solely for this project.
World includes chunk mutation, queues, sounds and commands; a narrow read facade
is preferable if object-safe shared pass access is needed. Probe mutation belongs
to a separate scratch owner, not a second `&mut` reference to the host world.

Before activation, the host must still provide the complete actual pending-work
inventory. A snapshot assembled from blocks alone cannot establish quiescent
entry. Account for scheduler, piston events/motions, entities and any active
backend's private work. An active Graph must stop through a validated lifecycle
before compiling its world; native queues alone are insufficient evidence.

## Fit to our interpreter

Use the existing redstone interpreter for probe execution:

- `redstone::update` dispatches ordinary power updates, including Wire Turbo's
  non-wire deliveries. It receives the supplied Block and direction; record
  these as delivered rather than rereading and replacing captured arguments.
- Observer shape handling follows separate paths. Record shape callbacks at
  their actual dispatch point as well as ordinary updates.
- `redstone::piston` already implements power sampling, request acceptance,
  movement and notifications. Its detailed trace is test-only and insufficient
  as a production compiler dependency; reuse its semantics, not its TLS collector.
- Existing World storage, TickScheduler and piston state can back a private
  scratch world. Reuse a suitable existing headless/test world storage owner
  after inspecting its scheduling and side effects; do not clone players or
  networking and do not reimplement redstone behavior.

Add one optional, inert-by-default compilation observation hook to World, with
events for delivered power callbacks and observer/shape callbacks. Default
worlds do nothing; the scratch implementation records bounded occurrences.
Place hooks at actual dispatch boundaries, including Wire Turbo, without double
recording callbacks routed through redstone::update. Inspect dispatch paths
before choosing sites. Block-write logging alone cannot observe unchanged updates.
Use ordinary normal-build code: no functional cfg(test) hook guards.

A probe record needs sequence, target position, delivered raw state, event kind,
direction and probe identity. Optional parent occurrence/depth helps explain
causality. Source/piston request identity is retained where needed. Keep raw
occurrences in diagnostic probe output; each pass separately applies its
reference-derived first-occurrence and filtering rules.

Scratch worlds suppress networking, sounds and external commands, but not
electrical updates, scheduler effects or block entities needed for power.
Reject command effects that would affect execution until represented; explicitly
report omitted diagnostic effects under the experimental contract.

Each probe starts from a defined immutable basis. Initially a fresh scratch
copy per probe is acceptable. Once correct, reuse scratch via a write journal
that restores blocks, entities, scheduler, piston queues/motions, logical time,
cache invalidation and recording state. Test journal reset against fresh copies.
Do not optimize snapshot copying in the executor as part of this front-end work.

## Passes and deliverables

### 0. Lock the baseline

Preserve the current dirty tree and active work. Run existing O0 Graph/native
adder acceptance and the event arithmetic gate; retain raw active-low values,
manifest hashes and declared windows. Do not turn a failing gate into an
observation-only passing test. Define how successive input histories rearm.

### 1. Capture and inventory

Capture selected starting blocks/entities and actual entry work, with a bounded
read halo. Inventory components, wires, pistons, resources, possible contacts
and crossing dependencies directly from the world. Reuse block/power/geometry
helpers. Reads reaching past the captured halo either grow it within budget or
produce a positioned admission error; never interpret unknown outside as air.
Build compact IDs and diagnostic position maps. No Graph route IDs required.

### 2. Scratch interpreter and observation

Implement the optional hooks and one scratch owner. Verify recording is passive:
traced and untraced native execution have identical blocks, pending work and
piston state. Verify nested callbacks, unchanged updates, observer shape events
and Wire Turbo delivery. Probe failure/cancellation leaves the host unchanged.

### 3. World-derived electrical links and wire watchers

Extract directional source dependencies directly with separate main/quasi and
diode-side inputs. Dust attenuation uses a bounded min-distance walk. Constants
remain sources. Create watched-wire readers and explicit watcher ranges around
pistons/observers. Saved wire strengths initialize reader state; do not globally
settle the circuit. Runtime updates a watched wire only from indexed affected
sources/arrangements, never by a physical dust walk.

First vertical acceptance: prepared-0-1-0's trigger changes the watched wire
corresponding to Graph node 89 and causes the piston corresponding to Piston 6
to sample main/quasi power and request retraction. Coordinates/Graph IDs are
test diagnostics only. Also require a masked source change to leave wire
strength unchanged and a 15-to-14 wire change to remain an observable change.
Do not automatically notify on every quasi power dependency change.

### 4. Resource arrangements

Probe each shared movable resource's alternate contacts in scratch worlds and
compare reader links plus wire source sets. Grow jointly interacting dependency
groups until no new dependencies appear or budget is exceeded; enumerate each
group's represented arrangements. Retain absent/in-motion alternatives and shared
resource identity. Reuse EventCondition where it fits; flatten or intern shared
conditions at sealing rather than proliferating recursive duplicates.

An arrangement index alone does not order updates. This stage changes which
links contribute; later programs determine which receivers are notified.

### 5. Source rise/fall notification programs

Construct a quiet scratch basis preserving geometry, constant-source dust power
and the chosen resource arrangement. For racing sources, record qualifying
reaction order on rise and fall separately using native update procedures.
Retain applicable receivers not observed by the order probe with an explicit
reference-compatible policy; never silently remove them. Combine sampled order
with explicit watcher relationships and electrical contribution instructions.
Test multiple/masked sources and timing through scheduled components.

Initially disable order caching and parallel probes. They add no compatibility.
If a sampled order varies across relevant represented arrangements, key the
program by arrangement or reject that interaction explicitly; do not silently
apply one trace universally.

### 6. Departure/arrival programs and reactive saved state

Record resource departure and arrival independently for every supported contact.
Apply distinct MoveRecorder-style filtering. Combine observer cell watchers,
watched-wire changes and other delivered receivers in explicit operation order.
Preserve source/base/head mutations required by that plan. Compare each scratch
movement with the corresponding native piston movement; a remove/place probe
is an abstraction and must not be assumed equivalent without checks.

Implement initially reactive component handling separately from piston watchers.
Port the tested purpose of CanReact using our actual component semantics and
pending inventory. Use per-update-phase deduplication where specified, not a
global once-per-resource-per-computation latch.

### 7. Seal, validate and integrate

Validate IDs, ranges, conditions, source coverage, watcher reachability,
arrangements and owned boundary adapters before installation. Flatten into
EventProgram's immutable arrays. Give each rejected stage a short positioned
reason and inspectable dependency/probe witness. Report budget exhaustion
separately from unsupported geometry or inconsistent state.

Switch the opt-in public event compile branch to prepare_event directly only
after ADDER1BIT passes. Keep the old Graph-to-Event adapter available only for
comparison during migration; remove it once its useful fixtures use the new
path. Preserve publication/reset rollback and reset-before-edit ordering.
The existing narrow public corridor can remain until its replacement passes
the same public API tests; avoid accidentally broadening admission by removing it.

### 8. Acceptance and scaling

1. ADDER1BIT: all eight vectors, declared payload/sum/carry windows, repeated
   vectors, consecutive changing inputs, reset/restart; actual piston work across
   the suite. Compare native and O0 Graph, with the pinned reference as additional
   compatibility evidence. Do not require movement on a valid no-op vector.
2. ADDER11BIT: all 40 existing cases, carry chains, overflow and successive
   operations; sum uses the manifest's active-low decoding/window.
3. COUNTER: internal clock, repeated counts, empty ordinary-piston action,
   reset/restart; do not manually rearm each game tick to force progress.
4. CPU and FPU_27: changing inputs and meaningful output transitions, explicit
   supported boundaries, measured executor entries and work removed.
5. Public path: supported classical components, independent and connected
   devices, multiple clocks, outside work preservation, all reset exit paths.

For each gate report actual operation counts, output checks, compile time/peak
memory and at least three sequential release timings after correctness. Separate
fixture throughput from server game TPS. The target remains useful acceleration
on the actual FPU plot; none of the preceding counts establish it alone.

## Ownership and stop conditions

Compilation owns scratch state; the host owns the immutable input world and
outside pending work. EventProgram owns compiled facts. EventState owns runtime
queues, resources and clocks. The backend owns projection and the reset snapshot.
No runtime world reference and no hidden Graph dependency are retained.

On probe/arrangement budget exhaustion, cancellation or unknown boundary work,
decline activation before suppressing native work. Start with one closed region;
general mixed-region partitioning is a later extension. Allowing broader geometry
does not license silently dropping observer, clock or resource interactions.

Implementation should proceed one pass at a time with inspectable output and
focused tests. Do not continue repairing global settle_wires. Do not replace the
interpreter, delete the Graph backend, introduce a generic compiler framework,
or optimize runtime packing before the real adder gates pass.

## Evidence and remaining uncertainty

- Fresh O0 Graph/native acceptance passed all 8/40 adder cases on this date.
- The quasi-port correction removes the earlier false Piston 11 retraction in
  zero-plus-zero. The zero-plus-one event failure is a missing pre-movement
  update while power agrees; it motivates the first watcher acceptance case.
- Reference watchers and sampled-order mechanisms are confirmed by decompiler
  inspection. The exact MAIR compiled table/runtime watcher responsible for this
  particular ADDER1BIT occurrence has not been captured independently.
- This document is a scoped implementation plan. No new engine behavior or
  performance result was produced while writing it.
