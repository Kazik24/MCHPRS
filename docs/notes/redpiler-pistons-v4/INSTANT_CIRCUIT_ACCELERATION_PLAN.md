# O6 logical circuit compiler and runtime plan

Status: implementation in progress, 2026-10-21. This replaces the earlier
physical transport and stable-input implementation plan at this path. Existing
O0-O5 optimizations remain supported. Implementation progress belongs in
[TODO.md](../../../TODO.md); measurements and their limits remain in
[SETTLED_RUNTIME_VERIFICATION.md](SETTLED_RUNTIME_VERIFICATION.md) and
[STABLE_INSTANT_LOGIC_RESULTS.md](STABLE_INSTANT_LOGIC_RESULTS.md).

## Execution contract

Target: O6 automatically compiles valid builds to a reduced logical representation.
Preserve stable observable outputs at their original game ticks, controls,
persistent state, consequential BUD samples/writes, and required notification
and publication timing. Internal glitches, previews, propagation, piston pose
and reset geometry may disappear. Preserve strength wherever a consumer needs
it; otherwise infer Boolean signals from their uses. Shared payloads with up to
five admitted pull owners use one pulled/unpulled value; the winning direction
is irrelevant under the accepted O6 contract.

An update event is distinct from a value change. A BUD may sample unchanged QC
power on an update. Conversely, a QC value change alone need not write memory.
Keep ordering only where it can affect a retained state read/write or output.
An instant can fire once before its reset and remains retracted in that interval
(user clarification, 2026-10-21). Feedback therefore needs reset-scoped fired
state, rather than unrestricted algebraic reevaluation. Bind rearming to each
actor's proven reset event; merging actors into one reset frame requires evidence
that their reset and next-computation cuts coincide.
Do not require identical internal callback counts, queues, revision numbers or
physical snapshots. Required delays and temporarily unreadable memory may be
logical state/events rather than moving blocks.

No new user flag, input-combination enumeration, whole-state case cache, or
per-build name/coordinate special case. Unknown semantics retain an explicit
fallback and a concrete diagnostic; discuss a newly required semantic assumption
before implementing it. Partial coverage is not whole-device compilation.

## Starting point

- `LayerExpr`, `ReducedLayerProgram`, reduction and `LogicalValues` already give
  shared Boolean expressions and incremental evaluation of changed dependencies.
  Keep the existing fifteen-threshold strength representation where required.
- O6 compiles live Main/QC queries for every BubbleSort and ANPU piston. This
  work is complete, but native action/reset/event execution still runs.
- COUNTER has a qualified logical executor with zero physical work, measured
  at 4.24M whole-loop game TPS. Its clock recognizer is a narrow family.
- The FPU computation-frame kernel is experimental; its measured calculation
  rate does not establish production game TPS or a publication adapter.
- Production settled extraction now uses source equations, global constant
  propagation and output liveness. Tests cover a 513-equation chain, a cycle
  removed by constants, dead feedback and an explicit live-feedback decline.
  Local electrical tracing and repeater inlining retain separate safety limits.
- BubbleSort's selected RAM-query inventory traverses 36,116 typed sources and
  44,237 edges. Extraction gets past the old 256-source recursive limit, but a
  113-edge cycle survives global constant propagation. The full containing
  component has 3,146 payload signals and 4,295 edges, including 144 negative
  edges. It is a reset-scoped one-shot network, not 113 inferred RAM registers.
  The test-only one-shot compiler now handles the selected RAM Main/QC cone:
  9,308 instant uses, 7,248 resources, 24,937 inputs and 84,404 Boolean expressions,
  with no declines. Normal BubbleSort execution still uses Graph.
- Ordinary single-owner BUD discovery and logical sample/accept/publication
  dispatch are implemented independently of the strict memory-reader pattern.
  Rilax busy-phase and same-level-update differential checks pass. ANPU activates
  967 owners and matches 5,000 screen/state ticks plus all 1,552 ordered BUD update
  ticks, with 2,090 logical accepted transitions/publications. BubbleSort matches
  all 10,036 frozen RAM ticks and 32 interpreter continuation ticks. This slice
  retains callback occurrences at the Graph boundary and does not establish
  whole-device event contraction or a speedup.
- The older candidate audit inventories 15,200 computation actors and 768
  external RAM pistons. Its thousands of physical crossing edges describe that
  partition, not the final logical boundary or a requirement to retain them.

## One compiled product, two linked parts

```text
validated source graph
    -> typed source equations + state/event dependencies
    -> global simplification, state boundaries, cycle classification
    -> reduced value DAG + logical state/events + boundary/projection bindings
    -> checked lowering and exclusive ownership
    -> existing tick/phase dispatcher + incremental evaluator
```

Extend the existing settled-plan path instead of adding a parallel compiler or
another device simulator. The executable product needs:

1. The existing reduced value DAG, with typed control, state and boundary inputs.
2. Dense retained state slots and event bindings: sample a BUD, validate/commit a
   write, schedule a required delay/publication, and publish an outside effect.
3. An ownership table and cold source mapping for diagnostics, activation and
   interpreter reconstruction. Physical identities need not be hot state.

Keep Graph's game clock, phase ordering and scheduling interfaces as the single
scheduler. Logical events target retained state and boundary operations, not
all original internal pistons. A closed kernel must execute without calling
`extend_connections`, `retract_connections`, Turbo or physical reset handlers
for its internals. External classic redstone may remain on Graph through typed
ports. Preserve a fast empty-phase path so generic scheduling does not erase
COUNTER's achieved throughput.

Within each phase, logical and Graph events must share the relevant sequence/FIFO
order, unless their cross-owner effects are shown to commute. Draining a separate
logical queue before or after an entire physical action batch is insufficient.

## Implementation slices and completion gates

### 1. Produce scalable equations and classify feedback

Refactor `logical.rs` source expansion into one-level typed equation templates
and an explicit worklist. Retain `SignalTracer` for local electrical queries,
its memoization and `LayerBuilder` simplification. Do not promote the diagnostic
string/report format to a production artifact. Use bounded work, graph size and
memory rather than the 256-source recursive substitution limit. The tracer's
separate local query/guard limits remain distinct diagnostics.

Propagate constants across templates to a fixed point, fold aliases and gates,
remove dead branches, and slice from actual value/state/event roots. Then find
strongly connected components and topologically lower the acyclic parts. Known
state reads terminate combinational traversal. Classify remaining feedback as
removable reset/control work, an explicit state transition with an established
update contract, or unresolved. A cycle alone never creates a register; the
current automatic retained-resource cut is a diagnostic, not that contract.
Only invariant constants may propagate; snapshot values are runtime inputs/state.
Demand only the thresholds each consumer needs, keeping full strength at the
ports where it matters rather than carrying it through every internal gate.

Gate: >256-deep acyclic examples compile; cross-equation constants eliminate
otherwise apparent cycles; real state remains state; unresolved cycles give a
bounded source/owner witness. Revisit the actual BubbleSort cycle after global
simplification and record the resulting classification before runtime elision.
Include `A = B && C; B = A; C = false`: the apparent cycle must disappear after
substitution, while a nonconstant unresolved cycle must remain a decline.

### 2. Compile ordinary state and consequential events

Discover state candidates from resource owners, readers/writers, Main/QC links,
callback occurrences, observer/reset provenance and delayed effects. The strict
upper-storage/lower-reader `BudMemoryRead` optimization remains useful evidence
but cannot be the admission requirement for ordinary memory. Production must
not use the fixtures' RAM coordinate lists to recognize state.

For each retained cell, bind its readable value, accepted writes, update
triggers, and any visibility/ready phase that a later retained sample can read.
Retain generation/cancellation state only as needed for scheduled effects.
Represent required clocks, repeater/observer delays and reset rearming as small
logical state/event operations; do not assume one global clock or period.
Private reset notifications disappear when their only job is preparing the
next logical computation and any retained rearming, sample, due-time or ordering
effect has a logical replacement. Do not globally fold all observers OFF as in
the FPU-only experiment.

Compile request sampling, acceptance-time rechecks and delayed publication as
separate cuts where a relevant intervening change is possible. Fuse them only
when the retained dependency graph permits it. Collapse internal event chains
and redundant deliveries to their consequential boundary events. Backward
slicing must include events that cause writes even when signal values stay
unchanged. Do not keep millions of callbacks under new opcode names.

Gate: every retained reader, writer, trigger and publication has one typed
binding; each removed internal effect is either dead to these roots or replaced
by a logical consequence. Rilax establishes ordinary memory behavior, including
same-value updates, enable edges, latched reads and delayed availability.

### 3. Execute one complete BUD slice without physical internals

Extend `SettledCircuit`/its existing plan ownership with the state/event tables;
use `SettledEvaluation` and `LogicalValues` for values. Extend the existing
`LogicalDevice` integration at Graph's tick/phase cuts. `SettledMachine::step`
currently writes every register simultaneously; it is not a generic CPU tick.
Add event-selected commits and preserve simultaneous groups only where the
source contract establishes them.

A state/input change marks dependent expressions dirty. Evaluate them before a
retained sample or publication needs them. An update can execute even when the
dirty value frontier is empty. Preallocate runtime storage; use dense IDs and
arrays in the hot path. Keep physical projection and source descriptions cold.

Gate: a small Rilax/one-BUD circuit preserves observable tick history, meaningful
writes and output strength while executing zero internal physical callbacks,
wire visits, piston actions and reset publications. Exercise canceled requests,
changes between request and acceptance, repeated updates, pending writes and
same-tick competing events. Compare rejected as well as accepted actions during
shadow validation; accepted-only probes can miss spurious writes.
Zero callbacks includes owned route delivery, not just the piston commit calls;
required external deliveries still execute at their logical boundary cuts.

### 4. Install the complete BubbleSort kernel

Compile all required RAM/state and control dependencies, then merge the
computation and event regions so physical cell boundaries cease to be runtime
interfaces. Ordinary retained state is part of the logical program, not an
excuse to keep every storage piston physical. Retain only actual outside ports.
Do not re-enable the old stable-input flag path as the automatic O6 solution.

Assign exactly one owner to every actor/resource/route effect: Graph or logical.
A removed internal callback cannot also run through legacy event, packed-query
or transport paths. Compile/entry failure before activation leaves the complete
unsupported ownership region on Graph. After a logical effect commits, do not
fall back by replaying that action physically.

Gate: all 10,036 native BubbleSort RAM ticks, final sorting, required external
outputs and controls agree; internal physical work for the claimed core is
zero. Start/stop/reset and changing controls also pass. Power-query coverage
alone cannot satisfy this gate. Complete event coverage is required before
calling the whole build compiled.

### 5. Close activation, continuation and generality

Adopt current state rather than schematic defaults. Import retained pending
work and deadlines with their existing game tick/phase, or continue Graph until
a supported adoption cut; checking or waiting must be automatic and reported.
Discard pending private work only when its logical consequences are represented.
Snapshot validation and activation remain atomic.

Project public outputs at the original cadence. For interpreter handoff,
materialize compatible state and pending retained effects at a supported cut;
if reconstruction requires settling, advance the authoritative logical program
to that cut before switching. Never resume from a stale physical shadow.
Test handoff at supported busy phases as well as quiescent checkpoints.
The atomic handoff includes public electrical levels/strengths, retained state,
pending external callbacks/source ticks, deadlines and cancellation generations.
Every queued effect transfers once; already delivered effects must not replay.

Run ANPU with the same compiler/runtime: 5,000 ticks of state and screen
behavior, all 14 changed screen frames, meaningful BUD transaction timing and
native continuation. Reuse the frozen 1,552-row BUD trace as a reference, but
separate consequential transactions from redundant samples before comparing
an intentionally reduced executor. Preserve the old exact trace tests for the
unchanged physical/query path. Validate ADDERS11BIT, parseable TEST_ samples,
COUNTER and a divider as cross-build checks; keep the previously accepted
unsupported samples explicit. Apply the same core to FPU's publication adapter
and validate outputs at the required original publication ticks. Keep the
documented user-accepted native glitch/result exceptions explicit; a frame-only
calculation result does not establish publication correctness or production TPS.

Gate: automatic O6 activation, no double execution, observable continuation and
O0-O5 regressions pass. Retire the narrow standalone clock bypass only when the
generic path preserves COUNTER behavior and its speed target.

### 6. Measure and remove superseded paths

Run serialized release benchmarks on the same inputs and active-tick windows.
Report whole-device game TPS, compile/adoption/projection costs and logical
expression/event work separately. Count remaining physical callbacks, samples,
actions, transport visits and fallbacks by ownership. No speed claim follows
from a cold netlist or a kernel-only evaluation rate.

Keep the existing COUNTER >=1MTPS and production FPU >=1kTPS targets. BubbleSort
and ANPU require measured whole-device improvement and complete claimed-core
elision; no CPU TPS target has yet been specified. Profile the closed kernel
before considering JIT, SIMD or additional caching. Retain live power plans and
O0-O5 transport as useful fallback code; retire duplicate experimental O6
execution paths and probes superseded by permanent regressions.

## Code scope and review ownership

| Area | Required change |
| --- | --- |
| `analysis/instant/layers/logical.rs`, `logical/settled.rs` | Iterative equations, global simplification, state/event roots and closed-region admission |
| `analysis/instant/layers` and `program_graph/bud_memory_reads.rs` | Reuse typed boundary/reset evidence; discover general retained state and event dependencies |
| `program_graph/settled_*`, `logical_values.rs` | Extend the existing checked plan with state/event bindings and selected commits; reuse incremental inference |
| `passes/compile_settled_circuit.rs`, `planning.rs`, validation and execution-plan dependencies | Automatic O6 publication, source/ownership checks and invalidation of affected products |
| `program_graph/lowering.rs`, `backend/graph/logical_device.rs`, scheduler and piston dispatch seams | Dense instructions, exclusive event dispatch, activation, output projection and handoff |
| Coverage/export, `tools/redpiler_graph/loader.py`, tests and optimization-level documentation | Validate new references, distinguish candidate/lowered/activated/fallback coverage and expose retained work |

Use the existing validated-product ownership flow; build once and check the
relevant source dependencies rather than repeatedly recompiling every plan.
Structural checks must reject invalid roots, writers, event targets, time/order
bindings and ownership conflicts. Native/interpreter differentials validate
semantics; repeating the same compiler is not an independent semantic proof.
Export only the bindings/counters needed to assess this work; a viewer redesign
and a new pass framework are outside scope.

Luna agents can implement equation analysis, event/state discovery and fixture
validation as separate assignments. Root reviews the semantic boundary and
integration. Serialize shared Cargo work and all performance runs; use separate
build directories only when concurrency is actually useful. No tests are
required merely for editing this plan; each implementation slice has the gates
above and must leave its relevant regressions passing.
