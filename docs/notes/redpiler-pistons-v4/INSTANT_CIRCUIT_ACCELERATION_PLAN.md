# Executable instant-circuit kernels

## First gate: CPU_BUBBLESORT interface

The 2026-10-20 source-graph audit finds one connected computation candidate:
15,200 instant actors, 41,353 wires, and an explicit physical perimeter. All
768 documented RAM cells have both read and write/notification references.
All 53 controls (52 buttons and one lever), including Start, are retained.
The merged perimeter contains every one of the 56,153 crossing power links
and 19,615 crossing callback occurrences; the audit finds zero omissions.

All 16,561 physical pistons and 13,118 resources remain represented. The
1,361 ordinary pistons include external memory/state machinery, whose effects
must be represented in the new kernel interface rather than discarded.
There are 41,354 physical wires in total; any wire outside the computation
candidate remains explicitly inventoried as peripheral transport.

This establishes a connected compilation candidate and retained source
dependencies, **not** an already accelerated executable. The current event
kernel owns zero wires. Its perimeter is not yet installed as a standalone
runtime interface, and memory transactions and scheduler effects are still
implicit in physical-node execution. The implementation below must close
those gaps.

Reproduction: ignored test
`kernel_interface::bubblesort_kernel_interface_is_complete_in_source_graph`.
Artifact: `target/instant-kernel-interface/cpu-bubblesort-interface.json`.
The artifact enumerates RAM cells, controls, peripheral nodes, typed perimeter
counts, resources, and crossing-edge checks against the source graph.

## Contract

Replace each admitted physical instant region with one executable stateful
graph, with original piston/resource identities and a complete typed interface.
Its inference must not need the original World or a physical block-neighborhood
walk. Keep logical state transitions and the causal relationships that affect
memory or exposed outputs. A connected group or renamed call into Turbo is
not this implementation.

Inputs include control actions and levels, clock advancement, memory/state
reads and readiness, external power/update events, and imported pending work.
Outputs include signal levels and falling events, delivered memory-write
samples/requests and accepted writes, reset/tick requests, deferred publications,
and exposed component effects. A level and its update notification are distinct.
The boundary must preserve route provenance, input sampling cuts, priorities,
deadlines, revision checks, and the ordering of dependent effects.

The new graph retains each piston as a stateful node, including both storage
and read pistons. Memory cannot be replaced by an unqualified Boolean bit while
its availability or in-flight writes matter. Stateful boundary primitives may
reuse existing piston/scheduler commit helpers; interior transport must execute
the new compiled representation.

## Algorithm

1. **Bind the complete region and interface.** Start from the validated
   BoundaryCatalog and merged RegionPartition/perimeter. Bind every owned
   actor, resource, contact selector, wire corridor, entry, and outside effect.
   Audit the interface against the original power/callback rows and stateful
   peripherals. No unexplained missing edge or state owner is admissible.

2. **Build two linked graphs.** A pure value graph describes power/guard
   expressions with attenuation and contact selection. An ordered event graph
   describes update delivery, piston sampling/acceptance, resource writes,
   reset, and future publication. The same value on two events does not make
   the events interchangeable. Keep original IDs in cold provenance tables;
   use dense local IDs in the executable arrays.

3. **Compile transport between piston nodes.** Trace fixed geometry once at
   compilation. Replace interior dust visits and passive block callbacks with
   transport programs whose leaves read typed live inputs and whose outputs
   deliver ordered receiver samples. Precompute reachability/attenuation where
   valid. A maximum-of-weighted-sources equation computes a settled value only;
   it is not, by itself, a replacement for update delivery or intermediate
   threshold crossings.

   For the first exact specialization, use bounded fixed-geometry corridors
   with queue-only receivers and a small source/entry-state family. Compile
   the exact reference transition into a compact event transducer: an entry
   guard, retained transport state, pure value operations, ordered sampling
   cuts, and next state. Preserve first-discovery observations and the first
   qualifying request/source route. Conditional entries must be checked before
   any optimized effect. Unsupported entries retain an explicit counted
   fallback; a region with such runtime fallback is not reported as fully
   replaced.

   A reference transition may be symbolically specialized or exhaustively
   compiled over a bounded finite family. Bound compile time, retained states,
   and memory; never assume that testing one driver vector certifies others.
   Inputs outside the certificate must fail entry before mutation. Any removed
   interior state needed for fallback or projection must be materialized from
   the transducer state, not read from a stale shadow circuit.

4. **Execute causal events on the compiled graph.** An input activates its
   compiled entry. Follow precompiled dependency/transport edges, enqueue
   target piston work, and commit through the original stateful semantics.
   FIFO successors remain behind already queued peers where that order matters.
   A chain must not run recursively to completion ahead of a sibling chain.
   No universal simultaneous-layer rule is assumed: shared resources and
   acceptance reads retain their explicit dependencies and arbitration.
   Graph construction is cold; runtime does not rediscover physical layers.

5. **Minimize only under the transition contract.** Fold constants, remove
   proven passive work, hash-cons pure expressions at the same sampling cut,
   remove dead reads/edges, and fuse equivalent typed reads. Do not remove a
   piston/buffer merely because its settled Boolean value is an identity;
   it can contribute a causal step. Do not merge observations across a write
   or capture boundary. An event may be removed only if its reads, effects,
   and generated pending work are redundant under the certificate.

6. **Pack for CPU execution.** Store opcodes, operands, successor ranges,
   state slots, guards, and output bindings in contiguous arrays. Reuse local
   scratch with nested-invocation isolation. Avoid hot-path block reconstruction,
   physical-neighbor discovery, hash maps, allocation, and indirect dispatch
   per dust block. Start with efficient scalar operations; add SIMD only to
   measured batches of independent operations, preserving event dependencies.

7. **Keep entry, exit, and fallback honest.** Import control, piston/resource,
   and pending state. Export interface effects and continuation state. Keep
   original physical projection cold and separate from inference. Count every
   general transport fallback and expose the uncovered region; installed
   kernel count is not a coverage or performance claim.

## Why this preserves NANOTICK_EXAMPLE

The pinned supplied example has five piston actors. Its measured first-wave
order is `delay-1, ordinary, delay-2, downstream, inhibit`: the ordinary branch
queues downstream before delay-2 queues inhibit. Downstream therefore retracts
before the inhibitor becomes effective. The replacement must reproduce that
behavior, including reset/reuse; it must not repair the designer's race by
running the entire delayed branch first or evaluating only settled Boolean
values. Keep the unequal actor chains and their activation/acceptance edges.

## Implementation and acceptance order

1. Add a **separate pass**, typed plan output, independent validation, lowering,
   runtime, interface export, coverage and operation counters. Reuse current
   state/scheduler owners. Keep it experimental until its certificates pass.
2. Deliver an executable small region first, with actual dust traversal removed:
   single-wire controls, NANOTICK_EXAMPLE, ADDER1BIT and its prepared-0-1-1
   ordering counterexample. Cover rotations, rising/falling inputs, shared
   resources, interrupted/reset entries, and native continuation.
3. Grow through ADDER11 and Counter, including Counter's sixteen labeled BUD
   cells. Preserve whole input/output histories, not only final settled bits.
4. Lower BubbleSort's one candidate with the audited full interface. Report
   exactly which transport is compiled, which state and effects remain, and
   whether any general walks/callbacks still execute. Validate its entire
   10,036-tick four-bank RAM trace, availability, final sort, external controls,
   reset/pending work and continuation. Extend input histories before promotion.
5. Validate FPU_MROWW with the green-wool trigger and seeded random histories.
   Do not claim arithmetic equivalence from a native output bank that stays
   0xffff, or call ordinary-piston probes a certified FPU memory map.
6. Measure fresh alternating production-release pairs after correctness passes.
   Compare interpreter, current correct maximum compilation, and the new pass
   on identical inputs. Report inference TPS separately from preparation and
   projection; target a material BubbleSort gain (at least 10% lower median
   inference time), and explain work removed. A slower prototype or uncovered
   whole region is unfinished, even if it groups into one kernel.

Each transport certificate must give a state mapping and a transition argument:
at every exposed sampling/commit boundary the same relevant values are read and
the same dependent output/future effects are produced. Internal queues need not
be byte-identical if a different representation has that correspondence. The
memory-only strict/relaxed experiments are useful evidence, not the certificate.

## Stable-input combinational compilation (2026-10-20)

The user clarified the optional `--stable-instant-inputs` contract: external
inputs stay fixed from the triggering input change through reset. Under that
contract the target is a combinational function of the input snapshot and
starting memory, with memory writes, reset and exposed output effects at its
boundary. The internal physical callback sequence is not the observation
contract for this mode. This does not freeze internal resource state across
successive accepted piston requests.

`compile-stable-instant-logic` now derives weighted source expressions from
the existing fixed-geometry, queue-only transport certificates. Distinct inputs
need not share a predicate. For a source at distance `d`, the contribution is
`source.saturating_sub(d)`; equal source/guard pairs share one input slot, and
parallel paths retain the smallest attenuation. Paths losing fifteen levels
cannot contribute. These expressions require no inference-time neighborhood
discovery, wire queues, or relaxation rounds.

The CPU product covers 2,138 nets and 17,426 wires with 8,772 shared typed inputs,
18,925 terms and 26,780 possible output sites. Execution evaluates the entry
expression first, samples other inputs lazily, and only evaluates the remaining
net if the entry changes. Distinct boundary sites retain live admission and
existing memory/reset commit helpers. An incompatible captured entry takes a
counted exact fallback before optimized effects. Physical wire values are still
materialized for remaining graph consumers and interpreter handoff.

The separate `compile-signal-channels` pass keeps exact ordered transitions
without the stable-input promise. Its complete 10,036-tick CPU run preserved
every graph state/future boundary and all 768 native RAM/readiness bits, removed
1,889,156 interior visits, and recorded no channel entry fallback. It covers
3,699 wires, so it is not whole-circuit replacement.

The stable-input path has passed all forty pinned Adder11 output windows and
the complete 10,036-tick CPU trace: all 768 RAM bits/readiness, command output,
final sorted values and sixteen ticks of interpreter continuation. FIFO-local
event-kernel tests keep their original exact
future assertions with the separate combinational pass excluded from those
focused pipelines. The current regression suite has 574 passing tests.

Three production fat-LTO samples with O5 and stable inputs measured 326.77,
326.07 and 325.06 inference TPS (median 326.07). This is 8.69% above the
user-accepted 300 TPS baseline; these are not alternating pass-ablation pairs.
Preparation and World projection are excluded. See
[the measured report](STABLE_INSTANT_LOGIC_RESULTS.md) for scope and costs.

Both products remain partial: general propagation, physical actor notification
routes, and state outside their owned nets still execute. The whole-circuit
goal is open. After measuring this implementation, the remaining work is to
fold signal expressions into actor predicates and replace internal physical
notification routes with compiled state dependencies, retaining the complete
memory/control/reset interface.

### Incremental execution and the next admission extension

Input-to-expression and wire-to-output inverse indices are now implemented.
The complete native CPU trace passes again; wire-expression evaluations fell
from 109,252,352 to 31,205,330, while all other execution counts stayed equal.
Five alternating production pairs measured 320.21 versus 321.27 median TPS;
the median paired time reduction was 0.62%, with one slower pair. This is not a
material acceleration. The whole-circuit goal still requires removing the
2,009,418 remaining general walks and most of the 174,026,524 callbacks.

The measured coverage blocker is actual conditional wire transfer: 113 rejected
corridors contain 17,736 wires, including one corridor with 13,340 wires. No
corridor is rejected solely by the blanket geometry check. Another 106 corridors
and 3,725 wires first fail receiver admission; 2,466 singleton corridors contain
2,466 wires. The detailed report is
`target/instant-drives/cpu-transport-rejections-before-guarded.json`.

The next compiler extension should use guarded weighted edges:

1. Lower each transfer slot into a directed source-to-target edge. Direct slots
   use a true guard. Descent uses `!solid(horizontal)`; Turbo ascent uses
   `solid(horizontal) && !transparent(horizontal) && !solid(up)`. Reuse typed,
   ordered contact-variant selection from the drive compiler. Resolve every
   possible source to a retained wire or an explicit typed carried-wire input;
   a geometry predicate alone does not establish source identity.
2. For each contributing driver, propagate guarded reachability for distances
   zero through fourteen. For distance `d`, OR predecessor reachability AND the
   edge guard. Merge equal formulas in the canonical Boolean DAG. The resulting
   wire value is the maximum driver value minus its source/path attenuation,
   gated by reachability. Positive attenuation bounds cycles without treating
   memory feedback as combinational logic. Keep explicit work/memory limits;
   do not publish a partially compiled net when a limit is exceeded.
3. Intern source and topology predicates, then compile inverse predicate-to-
   expression dependencies. A changed input or geometry guard should update
   only affected expressions and output sites, preserving the existing
   admission/commit cuts. Do not solve every wire in the large corridor for
   every entry. Internal resource mutations still require fresh predicate
   samples even when external controls obey the stable-input promise.
4. Check the entire enlarged corridor for receiver effects and notification
   obligations. The first rejection witness is not proof that all other
   restrictions pass. Memory storage/read pistons, writes, reset deadlines and
   outside observers remain explicit stateful boundaries. In particular,
   repeater lock updates need a stronger proof than Torch's queue-only update.
5. Verify guarded topology on small rising/falling, contact-selection and reset
   cases, then run the complete CPU/native RAM/readiness and continuation check.
   Report how many original walks/callbacks disappear before repeating production
   timing. Pure formula minimization alone has not delivered the requested gain.

### Ordered guarded transport implementation

The separate O5 `compile-guarded-instant-transport` pass now extends the
transport certificate to conditional connections, singleton corridors and
delayed-device boundaries. It runs before drive/channel/stable-logic
compilation, replacing the original transport table with a freshly derived
table. Publication independently rederives the guarded certificate.

Conditional input masks use the original captured horizontal/up geometry;
source identities must still be fixed member wires. The dense executor keeps
the native heading, shared first captures, rotating queues, receiver admission
cuts and FIFO calls. Torch/comparator scheduling and repeater lock changes
remain at those cuts. A pre-entry guard checks repeater receiver strengths
against their powered state, since a lock callback can normalize that strength.
Canonical imports and writes maintain a conservative O(1) admission hint;
otherwise the net's repeater receivers are checked before optimized effects.

This stage removes general neighborhood discovery from newly admitted wiring.
It does not yet replace its ordered transport with guarded weighted equations
or remove its boundary callbacks. The settled stable-logic builder continues
to decline conditional connections and delayed-device receiver nets. Wider
admission is therefore a basis for the planned dependency program, not evidence
that the whole circuit has been replaced. The first complete CPU checks pass:
39,144 wires are admitted and the sort executes zero general walks, with exact
state/future equivalence without the stable assumption and native RAM/final
sort/continuation checks in both modes. Production timing is collected
separately; see `STABLE_INSTANT_LOGIC_RESULTS.md` for the retained work counts.

### Electrical value regions inside ordered transport

The fresh guarded profile puts 47.17% of inference CPU in dense transport and
16.18% in stable logic. The full-sort attribution identifies the real hot
region: transport net 15 executes 68,221,728 visits (71.52% of all dense visits).
Its 13,340 wires split into **2,150 weak components of possible Turbo power
transfer**, including conditional edges; median component size is two and the
largest is 56. Net 76 adds 11,659,899 visits across 79 such components, none
larger than fifteen. Notification connectivity has grouped many small electrical
calculations into large transport corridors. These component counts describe
wire-strength edges; source/state and notification dependencies remain explicit.

Implement a separate `compile-instant-value-regions` product after typed drives:

1. Partition each certified transport net by its direct **and possible
   conditional** wire-strength edges. Keep the net-local wire IDs and the
   original notification graph so boundary ordering can still be executed.
   Do not split transport execution merely because electrical components differ.
2. Within each region, lower unconditional edges to unit-loss arcs. Group
   structurally equal typed drive expressions, omitting constant-zero queries.
   Each group is a logical input rather than another physical source site.
   Compile a bounded BFS distance row from every drive group and conditional
   edge target to every region output; distances saturate at fifteen. Store
   rows contiguously as bytes and intern equal rows. Reuse the existing drive
   conditions and query IDs rather than rebuilding block/property reads.
3. Retain conditional arcs as `(source value, target distance row, selector)`.
   Intern each horizontal/up geometry selector. The value kernel starts from
   the max of drive amplitudes minus their distance rows, then applies enabled
   conditional arcs until stable, with at most fifteen rounds because every
   such arc loses one signal level. This computes a unique max-plus fixed point
   without walking physical wire neighborhoods. Bound compilation work/memory
   and decline a complete region before publication when its certificate fails.
4. Use the value kernel as an exact **interval certificate**, before replacing
   any ordered boundary effect. An entry must match a previously certified
   settled profile. Sample current drives/selectors coherently for that walk.
   If drives and conditional edge changes all move the region's monotone
   operator in the same direction, every captured/written strength stays between
   the old and new fixed points. A wire with equal old/new values is therefore
   invariant throughout the episode. Check the already-written native seed
   against these bounds; a seed/Turbo equation difference is not automatically
   safe. Arbitrary/unsettled entries, mixed-direction changes and unproved
   source/topology effects retain exact transport before any uncertified skip.
5. Replace certified invariant wire evaluations by the necessary marker effects
   at the original FIFO cut. Preserve first-capture obligations, visited state,
   heading/bias and layer suppression; never mark a visit early at enqueue time.
   Geometry is stable within an admitted walk, but captured repeater locking
   can still matter to boundary scheduling. Preserve all ordered piston samples,
   their first-request provenance, and delayed-device calls until each removed
   operation has its own proof. Later compress marker/capture footprints and
   compile per-source boundary programs to remove the remaining physical events.

Compiler/value-kernel tests should compare the compiled rows with ordinary
bounded relaxation on random small directed/conditional graphs, including
cycles, multiple drives, attenuation and disconnected outputs. Runtime gates
must cover both polarities, unchanged outputs, dirty neighboring components,
captured values, mixed changes and invalid profiles. Then repeat exact full CPU
state/future, native RAM and continuation checks before production timing.
Keep Nanotick and reversed-request counterexamples as ordering constraints.
This is a route toward whole-circuit replacement; publishing region metadata
alone does not satisfy the goal.

The first runtime stage uses a narrower certificate than step 4: on the first
member capture of a region in each walk, solve its current fixed point and
require every live member strength to match. No region wire has been captured
or written yet, and other electrical components cannot change its power inputs.
The existing transport certificate holds source emissions and geometry stable
for the episode. Such a region remains unchanged throughout the walk. Cache its
typed wire observations and skip subsequent value queries, while preserving
neighborhood discovery, visited/heading state, FIFO markers and boundary calls.
Classify unmatched or declined regions as exact for the entire walk. Recheck
all certificates in the next walk; no persistent profile or monotone-transition
assumption is used in this first stage. The native seed write and root
propagation remain unconditional, including a seed whose region is now settled.
Transition intervals and marker removal remain later work. The implementation
passes focused and full CPU exact/native/continuation checks in both modes.
Five production pairs regress from 334.31 to 295.48 median TPS, with 12.21%
more time in the median paired comparison. It removes 32.10 million calculations
in stable mode but retains all 95.39 million dense visits and solves 9.16 million
regions. Profile the new runtime before further optimization. The next narrow
candidate is persistent per-region certificates: reuse unchanged driver/selector
snapshots and full typed wire values, invalidate only the owning region on a
member `set_wire`, and guard publication with a region revision. Shared query
or selector IDs need separate per-region snapshots. This stage does not yet
provide acceleration or satisfy whole-circuit replacement.

The persistent-certificate and compact-capture rewrite is now implemented and
passes focused tests, the normal and stable full CPU gates, native RAM and
interpreted continuation. In stable execution it reduces solves to 2,929,286,
with 6,234,805 cache hits and 42,603,086 byte captures. It still retains all
95,393,263 visits and 174,026,524 callbacks. Five clean alternating production
pairs measure 342.61 TPS for guarded transport versus 305.26 TPS for this
rewrite, with 12.18% more paired execution time. This is evidence against
continuing to add classification work without removing events.

The full-sort opt-in census attributes every retained visit to a value region.
Forty of the hundred hottest regions are single-drive, unconditional regions
in net 15, totaling 11,127,067 visits. Before splicing a transition program,
count foreign-region wire notifications and shared capture sites; electrical
closure alone does not make visited/heading/layer markers private. The current
`SignalTranscript` represents whole episodes and does not encode the marker
interface needed to splice an arbitrary subregion safely.

Also measure complete episodes where every visited wire is inert. In such an
episode only the native seed propagates: all subsequent wire visits perform
capture/marker work without enqueueing another wave. This may admit a smaller
whole-episode program containing only required external captures, live
enqueue-time selections and ordered calls. Its proof must establish inertness
before optimized effects, preserve the earliest capture of each used external
site, retain both delayed root delivery layers and alias suppression, and
decline before effects if any reached wire is not certified. This is a candidate
for actual visit removal, not an implemented or measured acceleration.

The follow-up census rejects both as sufficient standalone routes. Across the
full sort, single-drive/unconditional regions account for 62,858,499 visits,
but none survive both zero foreign mapped-wire slots and zero shared non-wire
capture sites. Entirely inert episodes account for only 1,231,414 visits
(1.29%) across 502,423 invocations. Region 90 itself has 31 foreign-wire slots
and one shared capture site. These are conservative structural dependencies;
they do not prove every marker is observable in final memory. The next program
must therefore compose cross-region marker/capture interfaces before minimizing
them, rather than require each electrical region to be private or settle it
atomically. Preserve interleaved queue insertion and boundary sample cuts;
precomputed final strength profiles alone are insufficient. Do not prioritize
the 1.29% whole-inert-episode shortcut as the main performance pass.

`compile-instant-transition-programs` now specializes the complete transport
episode, composing the coupled regions before eliminating their interior
markers. It records first-entry wire strengths, typed drive values and used
conditional input masks, then guards every replay before effects. Within that
certificate, the original captures/writes reproduce all internal control, so a
contiguous `SignalOp` stream can retain only ordered external captures,
admission, wire writes and calls. External acceptance and pending membership
remain live. This avoids assuming each electrical region is private, and
supports arbitrary input combinations through exact fallback on a mismatch.
Full normal and stable CPU checks pass, including native RAM, sorted output
and interpreted continuation. It removes 95.00 million visits in normal mode
and 73.68 million in stable mode (80.02% and 77.23%). See
`INSTANT_TRANSITION_PROGRAMS.md` for the contract, implementation and evidence.

The first baseline is collected before further minimization. The next concrete
rewrite is to reuse channel admission minimization: fold only source slots
whose validated `known` bit is set, remove known-false calls, use unconditional
sentinels for known-true calls, compact dynamic selection IDs and prune unused
captures. Further piston-sample fusion must establish equal *live* power
predicates at the relevant cuts. A learned Boolean is not a valid replacement
for unguarded external state reads. First qualifying request provenance,
resource arbitration, reset scheduling and interpreter continuation remain
correctness obligations. The remaining exact fallback and boundary execution
still prevent claiming whole-circuit replacement.

The pre-minimization production baseline is now measured: five alternating
frozen fat-LTO pairs give 327.12 TPS guarded reference and 365.83 TPS transition
program medians, with 7.80% less time in the median paired comparison. All
output/checkpoint/semantic-count gates pass; candidate counters also match the
full stable correctness run. This is a verified gain, but the remaining
21.72 million visits and 85.16 million effect instructions leave the original
whole-circuit replacement objective open. Proceed with instruction minimization
against this frozen baseline before extending semantics or adding wider SIMD.

### Minimized instructions and consumed-wire guards

That first minimization is implemented. Channel and adaptive programs share
known-admission folding, selection compaction and dead-capture pruning. Adaptive
entry guards now retain only first-captured powers consumed by transport
control; boundary predicates remain live at their original cuts. Seven focused
checks, 609 broader checks and both full CPU/native/continuation gates pass.

Stable fallback visits fall 21.72M -> 10.82M (88.65% removed versus guarded
transport), misses fall 334,641 -> 160,580, and replay instructions fall
85.16M -> 79.21M. Five alternating production pairs against the frozen initial
transition program give 7.09% less median paired execution time. Current host
medians are 211.29 -> 227.45 TPS; the identical reference executable is slower
than in the preceding session under background desktop load. Compare within
the current pairs, not across historical absolute TPS. See
`INSTANT_TRANSITION_PROGRAMS.md` and `target/instant-transition-minimized/`.

Next collect a fresh inference-only profile. The remaining 79.21M replay
instructions and 33.49M live piston samples require substantially more
elimination to meet the whole-circuit goal. A candidate is a typed piston-call
operation for proved receiver identities, pruning captures that serve only
generic dispatch while preserving live power/resource/pending sampling and
request provenance. Do not freeze learned powered values or remove arbitrary
repeated samples without proving equal live predicates. Full physical-graph
replacement and new FPU timings remain unfinished.

The new profile is collected and production-gate matched; see
`INSTANT_TRANSITION_PROFILE.md`. It attributes 44.42% exclusively to remaining
graph execution outside compiled effects/stable logic/transport entry and exact
walks. General guard evaluation is 18.96% inclusive across paths. The separate
`compile-guard-predicates` experiment reuses the existing compact Boolean compiler
with live/captured context preserved. Full normal/stable CPU correctness passes,
but five alternating production pairs show 8.38% more inference time; the builder
is excluded from the default pipeline. See `COMPILED_GUARD_PREDICATES.md`.

The unchanged minimized reference measures 428.70 median / 437.97 best TPS in
these pairs. Host conditions changed from the preceding session; use paired
comparisons to attribute gains. The next larger candidate is a compiled route
effect view that omits ordinary component-shape callbacks with no effect while
retaining the original rows used by Turbo admission. It must preserve live guard
sampling, classic-entry ordinal substitution, piston callbacks and error paths.
No such route-effect pass is implemented yet. The whole-kernel objective remains
unfinished.

Fresh production FPU evidence now covers the same shared inference code:
`FPU_CURRENT_INFERENCE.md`. Five rotated random-input rounds measure 387.01 TPS
native, 1,281.73 TPS O5 and 1,347.51 TPS O5+stable medians. All final physical
checkpoints, semantic pending work and 16-tick native continuations pass. Raw
piston allocator IDs are retained for diagnosis and excluded from the semantic
future comparison, following existing continuation tests. Native output remains
constant, so this is switching-cost evidence. O5 still executes 19.69M callbacks
and 8.05M piston samples over initialization, warm-up and the measured vectors;
stable mode reduces these to 18.80M and 7.51M. These counts identify remaining
work, not profile percentages or a finished standalone logical circuit.
