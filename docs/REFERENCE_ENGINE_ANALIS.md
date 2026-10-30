# Reference engine: algorithms and implementation notes

This is an independent functional description of the managed MAIR engine bundled
with RedGear, based on decompilation and bounded fixture replays. It is a guide
for a Rust implementation, not original source or a proof of equivalence for all
circuits. The original engine source was unavailable. Do not paste decompiled code.

Reference: `E:/codex-builds/RedGear-mair-decompiled`.
Inspected DLL SHA-256:
`DD963AA133042E70550E3C96D4AB0886D7DE460F49204B985FA34A01BF1A4DF1`.
Method names below refer to `MAIR.Core.Rsc.Compilation/Compiler.cs` and
`MAIR.Core.Rsc.Engine/Engine.cs`; search by name if line numbers differ.
Detailed investigation and replay limitations: [investigation](REDGEAR_ENGINE_INVESTIGATION.md).
See the [milestones](REDPILER_SAMPLED_ENGINE_PLAN.md),
[verified implementation status](REDPILER_SAMPLED_ENGINE_PROGRESS.md), and
[implementation assignment](MAIR_PORT_IMPLEMENTATION_PROMPT.md). The reference
algorithms below describe the inspected MAIR binary; they are not claims that
the current Rust prototypes implement those algorithms.

## 1. Architecture

The engine is a compiled event simulator. It is not a single simultaneous
Boolean expression and does not run a physical world interpreter on every tick.

```text
loaded world + supported block states + initial queued work
  -> inventory components, wires, pistons and movable tokens
  -> extract directional power links and dust sources
  -> probe token arrangements and extract conditional links
  -> sample source reaction order and movement updates in scratch worlds
  -> compile watcher lists, ordered operations and initial state
  -> runtime: scheduled components -> piston FIFO -> motion completion
  -> publish selected visible changes
```

Electrical contribution, a delivered update, a queued piston request and an
accepted movement are separate events. A source can change power without
delivering a piston update. A notification can matter even if a receiver's
final value is unchanged. Piston/BUD state cannot be reconstructed solely from
the latest power values.

The interpreter is used during compilation to observe selected update orders
and movement effects. It is not the only discovery mechanism: the reference
also extracts directional power, geometry, watcher relationships and resource
arrangements. A quiet-world sample is evidence for the sampled conditions,
not a proof for every combination of simultaneous updates or geometry.

## 2. Immutable program and mutable state

The reference keeps component nodes, directed input links, optional conditional
arrangements, watched wires, pistons, tokens, movement plans and scheduling
metadata. Nodes include initial state and component-specific information such
as repeater delay, side inputs and watcher lists. Positions support compile-time
geometry and world publication; dense IDs identify runtime objects.

Separate mutable state holds current outputs, active input levels, queued and
scheduled work, piston base/head motion, token position/motion, arrangement
indices and per-phase update markers. One physical token can be referenced by
several pistons. Empty piston actions can still produce updates.

Rust target: retain compact IDs, immutable flat arrays and ranges in the sealed
program; keep mutable arrays and reusable queues in runtime state. Compile-time
maps and scratch worlds are fine. Do not add per-tick world scans, interpreter
calls or Graph execution to the admitted runtime path. Explicitly represent
observable outputs and lifecycle ownership; the reference wrapper's reset
behavior does not prove native handoff.

## 3. Inventory and entry state

`Prepare` calls `ScanBlocks`, `DescribeNodes`, `BuildLinks`, `BuildWires`,
`OrderSources` and `TrackBudded`; details including token probes and watcher
construction happen within those stages. `AnalyzePistons` finds all pistons,
groups them by overlapping home/far cells and discovers movable tokens.

```text
scan selection and relevant readable surroundings
for each supported component:
    create node, retain loaded state and position
for each piston:
    record base, facing, sticky, home = base + facing, far = home + facing
group pistons transitively by overlapping home/far cells
discover movable tokens and possible cells per group
bind each piston to its group's token candidates and contact indices
build indexes for component, watched wire, token cell and piston home
```

An actor need not have a token. A sticky actor can pull a token; an ordinary
actor can still move its head and notify surrounding components. Multiple
actors can share a token. The reference checks geometry and refuses some
configurations; observed limits include six cells and two tokens in specific
piston clusters, plus restrictions on permanently displaced non-sticky payloads,
moving electrical components and slime/honey. These are reference limits, not
fundamental Minecraft rules. `FindPistons`, `GroupBySharedCells`, `FindTokens`
and `BindPistons` show the cases.

Preserve loaded values. Sampling worlds may be quieted for an experiment, but
that must not overwrite the execution entry state. MCHPRS admission must account
for its actual scheduler, piston event and motion inventory; reject a state that
cannot be represented before replacing native work.

## 4. Power extraction and dust

`Extractor::CollectSignal`, `CollectWeak`, `CollectStrong`, `CollectDiodeRear`
and `GetWireSources` extract links. Source orientation, conductor behavior,
attenuation and receiver port matter. A piston must account for both ordinary
and quasi power; repeaters/comparators have distinct rear and side semantics.

```text
inputs(receiver, arrangement):
    query applicable faces / ports using block-specific power rules
    for a conductive block, inspect its strong-power sources
    for dust, extract its reachable sources with attenuation
    retain source identity, distance, receiver port and condition

wire_sources(start_wire):
    queue = [(start_wire, distance 0)]
    best_distance[wire] = 0
    while queue not empty:
        wire, d = pop_front(queue)
        record direct sources, keeping minimum d + source_distance
        if d == 14: continue
        enqueue reachable same-level and step-up/step-down wires at d + 1
    return shortest source distances
```

A direct source to its adjacent first dust has zero dust-edge attenuation;
each subsequent wire edge contributes one. Keep all competing sources: wire
strength is the maximum of their active attenuated strengths. Source masking
and positive-to-positive strength changes both matter. `GetWireNode` uses
horizontal neighbors and conditional above/below geometry, not merely six
axis-adjacent wires. Constants, including redstone blocks, remain sources even
when dynamic sources are set off for an ordering probe.

The supported bound observed in the reference is 14 dust edges. The reference
selects the shortest path to each source. Check MCHPRS query-direction and wire
shape conventions against its interpreter rather than assuming C# face enums
have the same orientation. Represent initially-off supported sources; reject
unrepresented potential sources even if they happen to be off on entry.

## 5. Token arrangements and conditional links

`GroupProbe::ProbeSingles` probes alternative positions for each token and
compares affected receiver inputs and wire source lists. `ProbeJointly` grows
interacting dependency groups and probes combinations. `RecordInputs` stores
the links for each arrangement. The observed group cap is 4,096 arrangements.

```text
baseline = extract relevant inputs and wire sources in captured arrangement
for each token and candidate position, including absent/in-motion:
    scratch = isolated copy of basis
    place represented arrangement
    changed_dependencies = compare extract(scratch) with baseline
    associate changes with token
repeat joint probing until no groups grow or budget is exceeded
for each interacting group:
    enumerate supported token-position combinations
    extract conditional receiver links and wire sources for each combination
    build arrangement index and token -> group index deltas
```

The reference's token radix includes an extra absent/moving state. Resource
position and availability both affect links: a redstone block carried in a
moving-piston cell does not emit as a stationary redstone block. A receiver's
input may change even when every dust source list is unchanged.

Local probing and its candidate search are not a completeness proof. Reject a
crossing or unsupported interaction instead of inventing an unconditional
link. Use budgets and report the affected position and reason. Static geometry
and resource alternatives need their own focused tests.

## 6. Watchers are not power links

`BuildWatchLists` constructs update relationships separately from electrical
connections. `WatchAroundPiston` watches nearby dust within Manhattan distance
two, applicable component update footprints and neighboring movable cells.
Observers watch the cell in their facing direction. Token cells get
`CellWatchers`; piston head/home cells get `HomeWatchers`. Hidden `WireWatch`
nodes track selected dust strengths and deliver their watchers on a strength
change. `OrderedWatchersOf` orders watchers using reference update footprints.

```text
on source transition:
    apply represented electrical contribution changes
    deliver represented component/watch updates in their program order
    for each affected watched wire:
        new = max(active source strength - attenuation)
        if new != old:
            store new
            deliver that wire's watchers
```

This is a conceptual summary; the reference uses separate paths for ordinary
sources and initially reactive nodes. A changed input link does not automatically
mean every receiver was notified. In particular, quasi power may change while a
piston remains in its prior state until a qualifying update. MAIR's
`TrackBudded` / `Logic.CanReact` is a distinct mechanism for certain initially
reactive electrical components; its inspected predicate does not include
pistons. Do not conflate that name with all piston BUD behavior.

## 7. Sampled source order

`SourceOrdering` selects sources feeding at least two potentially racing
targets. `BuildQuietPlot` switches dynamic sources off in scratch while
retaining dust power from applicable constant redstone blocks. `OrderRecorder`
switches the chosen source on, then off, and records each eligible target's
first qualifying interpreter reaction separately for the two directions.

```text
quiet = scratch copy with dynamic sources off and constant dust power retained
for each selected racing source:
    mark its candidate receivers waiting
    switch source on in scratch
    record first qualifying callback per receiver in observed order
    switch source off in scratch
    record first qualifying callback per receiver in observed order
    revert scratch mutations
compile rise_order and fall_order into source programs
```

The reference filters non-wire callbacks by candidate identity and expected
input polarity, with special handling for observers. This is not a recording
of every callback. It also keeps additional applicable receivers; an unobserved
callback is not automatically an absent dependency. Do not assume a quiet
probe covers arbitrary live arrangements. If a represented arrangement changes
order, use an appropriate separate plan or explicitly decline it.

## 8. Movement sampling and notifications

`MoveRecorder` samples each token cell's departure and arrival separately.
It collects first occurrences of indexed components and watched wires, combines
them with explicit cell watchers and observer rules, and derives relationships
between changed wires and their watchers. This differs from `OrderRecorder`.

```text
for each token and candidate cell:
    prepare departure arrangement in scratch
    remove token and perform reference neighbor / shape updates
    record first indexed recipients and watched wires
    combine with applicable explicit cell watchers in defined order
    revert
    prepare arrival arrangement in scratch
    place token with reference updates
    record and combine recipients, including observer shape handling
    revert
```

At runtime, `SetTokenPos` changes arrangement-dependent contributions and
`RunMoveSteps` processes compiled steps. It distinguishes changed link marks,
wire strength changes, explicit watchers and observer shapes; `UpdateOnce`
deduplicates relevant recipients within an update phase. It also handles
remaining switched nodes and changed-wire watchers after ordered steps.

Piston base/head notifications are separate from token-cell events. MAIR's
`TryExtend`, `Retract`, `NotifyHome`, `NotifyCell` and `FinishPistonPart`
execute applicable notifications even when no token moves. Motion records
retain piston/token identity and direction through completion. `FinishMotion`
uses different token/piston completion ordering depending on the moving part.

The reference remove/place probes are an abstraction of movement. Compare them
with MCHPRS's actual piston mutation and completion phases before assigning a
sampled callback to a runtime phase. Scheduled component callbacks following a
movement remain scheduler work; they are not extra arrival instructions.

## 9. Runtime stepping

The reference `Engine.Tick` advances scheduled work in priority buckets, then
consumes piston events, then processes motion landings. Source transitions
update link contributions and deliver ordered updates. Queue deduplication is
per piston and event kind while queued; an event is revalidated against current
power and movement state when consumed.

MCHPRS adaptation: native scheduled processing drains the old current bucket,
advances the ring exactly once, and drains the new current bucket. Do not copy
the reference's time/bucket indexing literally. `schedule_tick` takes redstone
ticks (twice as many game ticks); `schedule_half_tick` takes game ticks. A lamp
off request is four game ticks. Repeater activations retain a short input pulse
after the input disappears; side locking and diode-dependent priorities also
affect subsequent event order. See REDSTONE_MODEL sections 5 and 9.

```text
tick:
    advance time and run due scheduled component work in priority/order
    while piston FIFO not empty:
        request = pop_front; clear its queued marker
        if live piston state, power and geometry permit request:
            execute action, switch arrangements and run movement programs
    process due motion landings in reference order
    retain newly scheduled work for its proper future phase

on piston callback:
    sample current main OR quasi power
    if power differs from extension state and action is viable:
        enqueue the appropriate action unless already queued
```

The inspected engine distinguishes extension, retraction and early retraction;
`IsEarlyRetract` and `FinishMotion` handle in-flight behavior. Empty actions
still update base/head state. Per-phase `UpdateOnce` is not evidence of a
once-per-resource-per-computation-cycle latch. The user's one-move rule, if
required by a later idealized mode, needs its own explicit and tested contract.

Projection is separate from simulation. Retain only outputs whose semantics
are supported; unsupported meaningful boundary crossings must be rejected.
For the intended experimental mode, defined settled/output boundaries matter;
intermediate visual equality is not automatically required. Reset-only exit
still needs a complete, isolated restoration operation and safe edit ordering.

## 10. MCHPRS integration and implementation sequence

Keep the normal Graph backend. Build a separate sampled compiler and runtime;
do not make its sealed program depend on `LoweredCircuit` or run Graph behind
the event executor. `World` supplies reads and interpreter callbacks; private
scratch state supplies writes, scheduler and piston motion during probing.
Existing `ScratchWorld` and observation hooks can be reused if their boundary,
entry-work and callback coverage match the new compiler's requirements.

Implement in small, executable gates:

1. Empty engine; lever → dust → lamp sampled rise/fall and runtime execution.
2. Empty piston requests, completion and rearm.
3. Piston base/head → observer shape → second receiver.
4. One movable redstone-block resource, conditional receiver and movement plan.
5. BUD store/hold/sample/resample and repeated operations.
6. Interacting resource groups and all ADDER1BIT vectors.
7. ADDER11BIT, COUNTER, CPU_BUBBLESORT and FPU_27 with meaningful inputs.
8. Public lifecycle integration and release measurements only after correctness.

For each gate assert compiled work actually ran, compare declared outputs and
relevant state/pending work, and ensure probes do not mutate the host. For
performance use flat ranges, direct actor/resource indexes and reusable runtime
storage; profile checked real workloads before packing or unsafe optimizations.

## 11. Evidence and open questions

- The pinned reference executed COUNTER_BASIC outputs 1–16 at sampled ticks
  `6n+5`; see `E:/codex-builds/counter-boundary-audit/mair-counter.log`.
  The event prototype read zero at those same boundaries in a separate replay.
- A native counter trace identified a piston-base shape update to an observer
  before the first memory-piston activation. MAIR's static watcher relationships
  support investigation but do not by themselves establish its firing order;
  see `E:/codex-builds/counter-mair-relationships-20261030/REPORT.md`.
- FPU schematic performance and output samples in the investigation report do
  not establish saved-plot correctness or current server TPS.
- Full binary parity, all update orders, arbitrary resource interactions,
  command-block effects and seamless native continuation are not established.
  Inspect reference call sites and test each supported behavior rather than
  interpreting this pseudocode as an exhaustive specification.
- On 30 October 2026 the Rust sampled suite passed 3 tests and failed its chain
  acceptance test; the event suite passed 45, failed ADDER1BIT and ignored
  COUNTER. Wire/resource discovery adapters exist, but still use LoweredCircuit.
  One operation range per source, simplified repeaters and rejection of
  in-flight piston actions remain port gaps. Passing mechanism tests are not
  evidence of general MAIR compatibility.
