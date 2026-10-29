# Redpiler event-engine implementation plan

## Goal and current checkpoint

Build an event-driven Redpiler engine following the observable design of the
RedGear reference: component and wire links, movable-resource arrangements,
source-qualified updates, piston requests, scheduled components and motion.
Implement it independently with MCHPRS's graph and redstone mechanisms.
The detailed local reference investigation is in
`scraps/docs/REDGEAR_ENGINE_INVESTIGATION.md` (that research directory is
ignored by Git). It is evidence about the binary, not a complete specification.

The user wants an actual `--optimize` plot executor for COUNTER,
CPU_BUBBLESORT and FPU_27. Target 15,000–20,000 changing-input FPU game TPS
initially, with 35,000 as a stretch goal. Measure game ticks on the actual
plot, including input changes and visible output projection. Offline evaluator
rates are not plot TPS.

The old instant/settled optimizer and logical-frame execution have been
removed from the current worktree. The physical Graph backend, scheduler,
piston actions and general wire/classic passes remain. This intentionally
removes the former fast COUNTER and FPU paths; their historical rates are not
the new engine's performance. The worktree is a large uncommitted rewrite, not a
deployable checkpoint.

Existing experimental pieces are **not active in plot compilation**:

- `backend/graph/wire_probe.rs` samples bounded physical Graph input cases.
- `backend/graph/piston_probe.rs` records source-qualified physical requests,
  resource positions and bounded clock windows.
- `event_netlist.rs` has a small event/resource runtime and a restricted
  physical-piston adapter. Its scope currently receives callback routes,
  power and output inputs from its caller.
- `event_repeater.rs` models repeater state and delayed work in a restricted
  adapter.

Focused tests passed for physical `TEST_PISTION` action and output traces,
the narrow piston scope, and repeater pulse/rearm behavior. These do **not**
show autonomous gate execution, Graph-work replacement or a complete FPU.
The last shared test build completed with seven warnings. Clean the warnings
and run the current checks before treating cleanup as finished.

## Execution contract

Start with an explicitly selected experimental device. Compile and run a
whole supported region or decline it before suppressing physical work. A
future partitioner can retain unsupported neighbors on Graph; it must preserve
their state and queued work. Do not silently acquire a shared resource.

The reference engine runs scheduled components, piston requests and moving
resources. Match that observable model first; do not assume its phase-deduping
is the user's additional once-per-resource rule. For the requested idealized
FPU mode:

- Sample declared controls and publish final visible results at the supported
  falling-clock boundary. Internal settling ticks and animation need not be
  displayed or match native timing.
- A movable resource can move at most once per computation cycle, even when
  several pistons can act on it. Mark it consumed only after a successful
  represented movement; rejected requests do not consume it. Rearm at the
  defined next cycle. Identify true persistent memory separately from resources
  restored by the cycle.
- A power level is not an update event. Unpowered BUDs do not fire without a
  represented callback/request. Revalidate queued actions against live power,
  geometry and resource state when they execute.
- Preserve required lamp, repeater and BUD-switch interface states. List any
  omitted command effects explicitly. Unsupported meaningful crossings must
  reject the candidate or remain on a correctly connected native boundary.
- On stop, write cancellation, replacement, error, save and unload, restore
  the selected region's captured starting state. Restore before applying an
  edit; preserve unrelated blocks, entities, scheduled work and players. A
  failed restore must retain the snapshot and avoid a partial teardown.

The reference's sampled ordering, `OutputsOnly` and hidden-piston modes do
not prove arbitrary-state correctness, once-per-resource behavior or native
handoff. The user accepts approximate internal behavior; report its effects and
do not invent correctness or throughput results.

## Passes and rough algorithms

Keep the passes small, independently testable and inspectable. Reuse the
physical Graph's sites, power links, guards, callback routes, `PistonId`,
`ContactId` and `ResourceId` where their contracts fit. Do not copy external
binary source or introduce fixture-specific compiler rules.

### 1. Inventory and selection

Prepare a physical Graph from the world without activating an optimizer.
Enumerate components, wire positions, pistons, movable resources, possible
resource contacts, initial states, scheduled work and possible controllers.
Identify the caller's selection and its external control, output and update
fans. A candidate is not an ownership proof.

Report a compact summary and block positions for unsupported elements and
crossings. Classify power reads, callbacks, mutable resources, pending work and
visible effects separately. Include clock candidates, every lamp, terminal
repeater and any exposed BUD state; do not infer a public BUD merely from a
quasi input. Name command omissions. Check for out-of-region destinations,
ambiguous resource ownership and unknown callback/guard dependencies.

Tests: small `TEST_PISTION` and `TEST_OR1`, two independent devices, a shared
block, and an unsupported neighboring region. Verify the report and rejection
occur before live state or callbacks are changed. A large interaction island
or an empty list of graph crossings is not by itself proof of isolation.

### 2. Power and wire links

For each source and receiver, derive the existing physical input relationship
and attenuation. Treat direct, weak, strong, conducted, repeater rear and side
inputs separately. A practical wire algorithm is a bounded breadth-first walk
that retains the best attenuation from each source; the inspected reference
uses a 14-edge bound. Preserve constant sources when quieting dynamic inputs.

Use fresh physical Graph states to probe source transitions. For bounded local
groups, compare independent and simultaneous inputs; do not claim untested
combinations from one all-off sample. The present eight-input exhaustive
truth-table prototype is a small fixture, **not** a global limit for an FPU.
Validate representative samples against the interpreter and decline or keep
Graph for unsupported dependencies.

Tests: attenuation, two masked sources, simultaneous inputs, rise/fall,
constant power, unsupported baseline and non-monotone input behavior.

### 3. Movable resources and conditional links

Identify each movable block as one resource shared by all possible actors.
Enumerate its legal contacts and affected power/callback readers. Probe or
derive links before and after candidate positions in isolated Graph states.
Group overlapping resources and grow a group when a combination changes a
dependency; represent each supported arrangement as a set of active link and
watcher changes. Bound enumeration and report the offending positions when a
group exceeds the budget. The reference's observed 4,096-arrangement limit
is a starting comparison, not an instruction to reject every larger valid
circuit.

At runtime, a successful move changes the resource contact, updates only its
affected arrangement groups, and queues represented departure/arrival
notifications. A failed or stale request leaves both contact and consumed flag
unchanged. Avoid treating the old 15 feedback bits as the complete set of FPU
resources: equation reduction can inline other resource behavior.

Tests: one sticky piston, `TEST_OR1` shared-resource contention in both
request orders, blocked/missing payload, two independent resources, canceled
request, rearm, and a dependency visible only after more than one resource
changes. Include a same-final-power notification where an event still matters.

### 4. Source and callback ordering

Separate power relationships from delivered updates. For relevant source
transitions and token departure/arrival, collect ordered, source-qualified
callback routes in scratch physical Graph execution; cross-check representative
cases with the interpreter. Where practical, select racing destinations
instead of tracing every source. Preserve an event even when the sampled power
at the start and end is equal.

The reference samples first reactions for both rising and falling sources in a
quiet world. Treat this as an implementation technique, not a universal order
proof. If a state or interaction cannot be represented under the experimental
contract, report it and decline rather than silently erase the effect.

Tests: request identity and source, changing sources, same-level pulse,
feedback that attempts to reuse a consumed resource, and simultaneous
competing requests. No unpowered-only activation.

### 5. Lower compact program and execute events

Lower the validated selection into indexed arrays: components, source fanout,
input/side links, resource contacts and owners, active arrangement groups,
callback destinations, scheduled-node metadata and output adapters. Validate
indices, strength ranges, guards and supported actions before installation.
Avoid a scan over every link for each event; pre-index affected targets and
resource watchers. Keep current state separate from immutable compiled data.

Rough runtime loop:

```text
on supported input or clock event:
    update source state and enqueue its represented notifications
    while ready work remains within the bounded wave:
        pop the next request in the selected ordering
        revalidate actor power, callback guard, resource and destination
        if movement succeeds and resource is not consumed in this cycle:
            mark resource consumed; update its contact
            switch affected conditional links and enqueue move notifications
        update affected component inputs and schedule delayed work
    at the declared publication boundary, project the final interface states
```

Use explicit scheduler buckets/priorities for repeaters and other delayed
components, and retain state across frames. `TEST_PISTION` demonstrated why:
its repeater input was zero at two falling actions while its output differed
because opposite delayed ticks were pending. Do not replace that state with the
current input. Keep event identity and counters for entries, movements,
scheduled work, work elided and fallbacks.

First implement **one autonomous TEST_PISTION cycle and its next rearmed
cycle**, including repeater output, without feeding it observed Graph power or
callbacks during execution. Then handle `TEST_OR1`, TEST XOR/other small
gates and a mixed circuit. Matching a recorded transition alone is not a
compiled gate.

### 6. Install, output and restore

Admission must prepare the executable plan, explicit outside adapters and a
restorable selected snapshot together. Install only if all three succeed.
After installation, internal work is executed exactly once by the new engine;
unowned Graph work continues and supported crossings are delivered via
adapters. Verify this with runtime counters and independent neighbor progress.

Connect one selected-region restoration operation to every lifecycle exit.
Include blocks, entities, piston/resource positions, and owned scheduler and
event work. Protect user edits by restoring before the write. A snapshot helper
that refuses reset whenever *any* outside piston is moving is only a
conservative prototype: either prove that restriction at admission or implement
ownership-aware preservation. Do not destroy the active backend or snapshot
before a fallible restoration has succeeded.

Tests: stop, edit, execution error, compile replacement, save, unload,
unsupported crossing, shared-resource refusal, outside ticking/moving work,
and rejected restoration without partial mutation. Keep ordinary Graph reset
unchanged for circuits that did not activate this experimental mode.

## Validation and measurement gates

1. **Build and cleanup:** complete pass/model deletion, remove dead callers and
   warnings, run formatting and relevant physical Graph regressions. Preserve
   the user's existing work and fixtures; no wholesale reset.
2. **Piston mechanics:** the once-per-resource tests above pass, then an actual
   compiled small piston fixture performs consecutive cycles and publishes the
   expected output while replacing its internal Graph work.
3. **Small circuits and mixed execution:** exercise `TEST_` gates, independent
   clocks, connected devices and a neighboring unsupported region. Record
   failures and exact rejected positions/reasons.
4. **COUNTER and CPU_BUBBLESORT:** verify real executor entries, meaningful
   changing outputs and repeated operation. Compare native/known outputs at
   appropriate boundaries; record omitted command effects and compatibility
   assumptions. Do not count compilation alone as acceleration.
5. **FPU_27:** use `test_data/piston-research/FPU_27.schem` and separately
   validate the current saved plot snapshot before using it as an oracle. Raw
   input `0x412345678` must produce the user-confirmed `0x84f3`; input
   `0x558e1d715` must not produce `0xffff`. The alternative `0x871e` is not
   independently confirmed. Exercise varied operands/opcodes, repeated values,
   history and restart, 35 lamp states and 16 result outputs at the defined
   falling edge. The user reports that native FPU behavior is wrong for at
   least the first vector, so native parity is not a universal arithmetic oracle.
6. **Performance:** only after real activation and the relevant output checks,
   run at least three comparable changing-input measurements and report median
   game TPS, wall time, options/budget, executor entries and physical work
   replaced. Compare the same scenario with physical Graph; distinguish an
   isolated schematic from the saved plot and server/client overhead.

No step is complete merely because a plan was generated or the plot compiled.
The reference engine's successful compilation of a fixture is a useful design
clue, not proof of this engine's output or throughput.
