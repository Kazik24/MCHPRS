# Instant compiler: edge cases and correctness obligations

> Archive status, 2026-10-18: C/LLVM and finite/symbolic synthesis source has been removed.
> Recover the last combined source from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> Historical results and plans below remain preserved; see
> [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current behavior and restoration details.

This is the single running register for edge cases discovered while developing the
boundary-driven instant compiler. Add new cases here instead of scattering them across
agent reports. Keep the smallest example, its observable consequence, the compiler rule and
the evidence. A plausible risk is not a confirmed fixture failure; label it accordingly.

The active design and acceptance criteria are in
[BOUNDARY_DRIVEN_INSTANT_COMPILER.md](BOUNDARY_DRIVEN_INSTANT_COMPILER.md).

## Current assumptions

- External data remains stable during the admitted instant/reset transaction.
- External callback order within the same logical tick may change. Their observable values,
  directions, meaningful multiplicity, deadlines and memory effects must remain correct.
- Internal causality may not be discarded when it changes actual outputs.
- Ordinary components and logical BUD memory remain outside instant ownership.
- Interpreter handoff may wait a few logical ticks for a certified safe point. Ordinary
  devices and the shared clock continue normally while the handoff is pending.

An assumption relaxes a particular contract; it does not establish that every other
observable intermediate effect is irrelevant.

## Confirmed semantics and counterexamples

### E01. A repeater remembers a pulse that disappears before its scheduled tick

**Situation:** input goes `0 → 15 → 0`, with updates delivered and no game tick between
the changes. The first update schedules the repeater's response. Removing the input does
not cancel that work. An initially off repeater subsequently turns on and schedules its
later off transition even if its input is already zero.

**Consequence:** a zero final input does not imply that no output pulse occurs. A compiler
which only publishes the final wire level can produce the wrong output.

**Required rule:** preserve the resulting scheduled transition. The compiler may derive a
guarded scheduling effect directly; it need not replay all internal wire callbacks.

**Evidence:** [native repeater](../../../crates/core/src/redstone/repeater.rs),
`repeater_short_pulse_ends_after_selected_delay` in
[master_tests.rs](../../../crates/core/src/redstone/master_tests.rs), and
`repeater_keeps_short_pulse_and_delayed_off_transition` in
[Graph component tests](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).
These existing tests were inspected, not rerun for this entry.

### E02. QC power and activation are separate inputs

**Situation:** changing a BUD's data/QC input changes available power but delivers no
qualifying activation update. A separate update later samples the current data.

**Consequence:** updating memory whenever a power expression changes destroys BUD behavior.
Conversely, deleting an update because power stayed equal can suppress a real activation.

**Required rule:** keep power availability and event activation distinct. Leave logical BUD
memory outside instant ownership; route qualifying effects to its normal Graph handler.

**Evidence:** `regional_bud_memory_changes_only_after_the_independent_update` in
[boundary_contract.rs](../../../crates/core/src/redpiler/backend/graph/tests/boundary_contract.rs).
Passed in release during this work, including the assertion that the memory actor is not
owned by an instant program. This validates the existing path, not the unfinished event kernel.

### E03. Stable inputs do not make internal piston requests order independent

**Situation:** the pinned one-bit adder case `prepared-0-1-1` is run with stable inputs.
Replacing ordered propagation with the fixed-point experiment changes the initial request
FIFO. The sum payload changes from a redstone block to air. Restoring just that initial FIFO
repairs this particular counterexample.

**Consequence:** this is not merely a difference in invisible internal state. The unordered
experiment also produces the wrong real sum repeater response in ticks 3 through 7.

**Required rule:** preserve required internal request causality, or prove an equivalent
transfer function. The same-tick outside-order assumption does not authorize unordered
internal propagation.

**Evidence:** `fixed_point_reordered_adder_requests_change_the_sum_payload` in
[fixed_point_adder.rs](../../../crates/core/src/redpiler/backend/graph/tests/fixed_point_adder.rs).
Inspected existing counterexample; not rerun during this entry.

**Step 04 recheck (2026-10-18):** reran
`fixed_point_reordered_adder_requests_change_the_sum_payload`; it passed and reproduced
the difference. The exact initial request queue is actor 6 then actor 12; the fixed-point
experiment swaps them. In the exact execution actor 3 is sampled unpowered, then powered
before its queued action is checked, so that action does not accept. In the reordered
experiment it accepts and changes the real Sum response. A layer must retain the
callback-time request separately from live acceptance and prove the relevant arrival
precedence. Final settled power alone cannot justify either observation point.

**Input-edge clarification (2026-10-18):** the proposed gate compiler treats inputs applied
on one logical edge as one sampled input frame. Its DFS/BFS traversal order must not select
the answer. The counterexample demonstrates sensitivity to changing physical propagation
order; it does not require reproducing that traversal at runtime or invalidate gate extraction.
Buffer chains can affect the circuit's internal nanotick dependencies. Derive those effects
before removing the buffers, retaining only the resulting output/activation conditions where
possible. Sequential lever API calls are not automatically an atomic input frame; compare
the same declared stimulus protocol before attributing a difference to the compiler.
This is a design clarification, not a newly executed atomic-edge regression.

### E04. A shared external source does not own both devices it powers

**Situation:** two independent instant devices receive power from the same external component.

**Consequence:** treating every power source as an internal connection merges unrelated
regions, enlarges the symbolic problem and creates false ownership dependencies.

**Required rule:** an external component is a port. Its common fanout alone must not merge
the receiving ownership groups.

**Evidence:** `shared_external_power_does_not_merge_independent_instant_devices` in
[boundary_contract.rs](../../../crates/core/src/redpiler/backend/graph/tests/boundary_contract.rs).
Existing regression; not rerun in this entry.

### E05. Endpoint classification misses dependencies hidden in guards

**Situation:** an otherwise internal power edge reads an external actor in its guard, or an
external-to-external edge is conditioned on an actor/contact owned by the region.

**Consequence:** an endpoint-only boundary loses an input or an externally observable output.

**Required rule:** classify the support of guards as well as source/target ownership. A
mixed condition can contribute both input and output dependencies. External actor atoms
must not be mislabeled as owned state.

**Evidence:** `internal_power_edges_keep_their_external_guard_dependencies`,
`external_power_edges_expose_their_reads_of_region_owned_guards`, and
`external_actor_guard_atoms_are_not_misclassified_as_owned_outputs` in
[boundary_contract.rs](../../../crates/core/src/redpiler/backend/graph/tests/boundary_contract.rs).

### E06. A comparator can observe owned geometry without a power-link guard

**Situation:** a comparator's far-override eligibility reads whether an owned contact is
solid. That read can exist in the comparator plan rather than on a power edge.

**Consequence:** scanning only power links misses an observable change in comparator input.

**Required rule:** inspect comparator override plans, contact alternatives and geometry
predicates in addition to ordinary power and callback edges.

**Evidence:** `comparator_overrides_expose_owned_geometry_reads_without_a_power_edge` in
[boundary_contract.rs](../../../crates/core/src/redpiler/backend/graph/tests/boundary_contract.rs).

### E07. A region can activate without a selected lever or common trigger

**Situation:** an ordinary component delivers a callback into an instant region; the lever
which originally caused it is outside the compiled selection. Multiple incoming ports may
activate the same region.

**Consequence:** a lever-selected compiler can reject a valid region or silently leave its
internal propagation active while claiming kernel coverage.

**Required rule:** bind semantic boundary events. A dedicated lever, unique common trigger
or whole-device input protocol is not a required entry shape.

**Evidence:** `external_component_callback_runs_region_without_selected_lever_or_global_tick_gate`
in [symbolic_kernel.rs](../../../crates/core/src/redpiler/backend/graph/tests/symbolic_kernel.rs).
This existing test exercises event entry; it does not by itself prove propagation removal.

### E08. Unrelated scheduled work must survive regional execution

**Situation:** an ordinary repeater has pending work when an instant region activates.

**Consequence:** a whole-backend cursor, an idle-queue gate or replacement of the entire
future can stall that repeater or consume unrelated work.

**Required rule:** retain the normal Graph scheduler as the sole clock. Dispatch only the
selected region entry and merge its future changes into the existing queues.

**Evidence:** the no-lever test above also schedules an unrelated repeater and verifies its
dispatch. A positive region-step counter alone is insufficient evidence.

### E09. Lazy external reads must not become preparation constants

**Situation:** a component outside the region changes after compilation. A fast
`Stored`/`Output` power read bypasses site observation and accidentally uses initial state.

**Consequence:** the compiled region responds to the schematic's saved value instead of its
current input, including analog strengths between zero and fifteen.

**Required rule:** bind every reachable mutable read dynamically, including fast power
paths, comparator overrides, state guards and contact-selected receivers.

**Evidence:** `regional_external_power_reads_stay_dynamic_without_rebinding_internal_writes`
in [boundary_contract.rs](../../../crates/core/src/redpiler/backend/graph/tests/boundary_contract.rs)
passed in debug during this work; it checks all sixteen stored strengths.

### E10. Rebinding an already-written field destroys sequential semantics

**Situation:** a compiler lazily symbolizes a wire, writes strength seven, then symbolizes
the wire again during a later query.

**Consequence:** replacing the current expression with the original input forgets the
earlier write. The generated circuit no longer describes the ordered computation.

**Required rule:** symbolize each field owner once per entry. Subsequent reads use the
current SSA value. Reset the entry baseline explicitly when compiling another routine.

**Evidence:** the same dynamic-read regression checks that the strength-seven write survives
resymbolization. Passed in debug during this work.

### E11. Shared payloads are conserved resources, not one permanent block per piston

**Situation:** several sticky actors capture the same payload at different times. An actor
can temporarily have no payload; a payload can be dropped and later recaptured.

**Consequence:** a permanent actor-to-block assumption rejects real adders or duplicates and
loses memory when ownership transfers.

**Required rule:** retain one resource identity, its permitted contacts and its revision.
Close ownership over coupled footprints and conserve the token across valid transfers.

**Evidence:** `three_sticky_actors_share_one_conserved_resource`,
`shared_payload_overlapping_repeated_triggers_keep_fifo_and_conserve_one_token`, and
`shared_resource_drop_and_recapture_ignore_the_obsolete_actor_event` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E12. Actor and resource publication witnesses are independent

**Situation:** an actor publication becomes stale after another action, while its resource
publication is still valid, or a recaptured resource invalidates the payload witness while
the actor restoration remains valid.

**Consequence:** testing one revision for the complete event suppresses a valid restoration
or restores an obsolete payload at the wrong contact.

**Required rule:** validate actor and resource witnesses independently. Preserve conditional
revision increments and event-time stamps through symbolic writes and lowering.

**New implementation review:** seeding `Publication.when` with actor-validity is incorrect
when `publish_resource` also gates itself on that field. Use an unconditional occurrence
guard and independent actor/resource validity predicates. This draft issue was identified
in source review before the new path executed.

**Evidence:** `stale_resource_publication_does_not_cancel_a_valid_actor_publication` and
the drop/recapture regression in [components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

**Fresh corpus evidence:** the revised PC counter's Step 01 128-tick native/Graph/frozen
episode passes output and full-state checkpoints while accepting 19 `RetractWithoutPull`
actions, separately from 2,957 ordinary retractions. Thus rejecting or reinterpreting every
early drop as an ordinary pull would exclude a real target history. These observations do
not certify the new stationary-cell contracts for early-drop entries.

### E13. A publication due this tick can still count as an early extension

**Situation:** retraction is sampled during scheduled work or piston actions, before a due
extension is published in the deferred-connections phase.

**Consequence:** deciding solely from `due_tick > now` chooses normal pull instead of early
retraction without pull. Opposite-facing shared actors also cannot share an undirected flag.

**Required rule:** preserve phase, facing, resource/contact and revision-qualified pending
extension predicates. Include existing queued publications as inputs to a regional entry.

**Evidence:** `shared_payload_deadline_reversal_executes_notified_actor_in_same_tick`,
`shared_payload_opposite_direction_does_not_request_early_retraction`, and
`piston_early_reversal_matches_interpreter_and_invalidates_old_restoration` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E14. Requests must be revalidated at acceptance

**Situation:** a callback samples power and enqueues an extension. Before that request is
consumed, reset power or contact availability changes. Repeated callbacks can also request
the same actor/action before acceptance.

**Consequence:** accepting the original sample unconditionally moves a piston that the
interpreter cancels; failing to deduplicate can create extra actions.

**Required rule:** keep sampling, FIFO request insertion and live acceptance separate.
Preserve action-specific deduplication and admission conditions.

**Evidence:** `symbolic_requests_are_deduplicated_and_revalidated_against_live_reset_power`
in [symbolic_kernel.rs](../../../crates/core/src/redpiler/backend/graph/tests/symbolic_kernel.rs),
and `piston_acceptance_cancellation_matches_interpreter` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E15. Pending work is typed, ordered and can intentionally be stale

**Situation:** imported ticks have duplicate occurrences, different priorities, stale block
types or deadlines. A stale tick must remain pending until its normal dispatch point.
An untyped imported occurrence need not suppress a new typed callback tick.

**Consequence:** dropping stale work at import changes continuation; treating the queue as
a set changes order and multiplicity; using the wrong pending key suppresses valid work.

**Required rule:** preserve typed target identity, duplicates, priority, FIFO and half-tick
deadline. Seed regional pending reads from matching live membership, not an empty queue.

**Evidence:** `imported_ticks_keep_duplicates_priority_fifo_and_reject_stale_types`,
`untyped_imported_work_does_not_suppress_a_typed_callback_tick`, and
`stale_tick_at_uncompiled_air_position_remains_until_dispatch` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E16. Dispatch removes pending membership before the handler runs

**Situation:** a reset observer's scheduled handler rearms itself.

**Consequence:** retaining its old membership while applying the handler suppresses the
follow-up tick. Replacing the whole future instead can erase unrelated work.

**Required rule:** consume only the dispatched occurrence, update membership, then execute
the regional routine and append its guarded schedule effect through the normal helper.

**Evidence:** existing observer/reset behavior in [Graph components](../../../crates/core/src/redpiler/backend/graph/tests/components.rs)
and [symbolic work semantics](../../../crates/core/src/redpiler/analysis/instant_regions/compiler/work.rs).

### E17. Shape notifications are not electrical updates

**Situation:** a Shape delivery reaches an observer or torch. An ordinary Update may carry
a direction, no direction, or captured powered state.

**Consequence:** treating both delivery kinds identically can trigger an observer or sample
electrical power at the wrong point.

**Required rule:** retain delivery kind, direction and captured/live operand meaning. Use
the receiver's real qualification rules; equal final power does not replace this check.

**Evidence:** `shape_delivery_does_not_sample_observers`,
`observer_shape_then_explicit_update_matches_native_for_every_orientation`, and
`torch_shape_does_not_sample_power_before_the_update_phase` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E18. Nested Turbo walks have independent captured observations

**Situation:** a bulb callback starts a nested wire walk while an outer walk is paused.
The inner walk observes and writes intermediate strengths; afterward the outer walk resumes.

**Consequence:** one shared mutable capture or an unordered final-strength solver loses
event-time properties, heading order or the outer queue position.

**Required rule:** trace the ordered semantics at compile time or prove an equivalent
transfer. Keep captured and live reads distinct; do not silently substitute current state
for an incoming captured target.
Lowering a captured piston property must use its frozen extension value, not the actor's
final state after tracing. A captured contact alternative also includes receiver identity:
if two alternatives share a profile but have different receivers, a profile-only selector
is insufficient. Preserve that distinction or explicitly prove it cannot occur in admission.

**Evidence:** `bulb_callback_starts_a_nested_wire_walk_and_outer_order_resumes` in
[components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E19. Stored dust strength and emitted power are different channels

**Situation:** diode fallback reads stored dust strength irrespective of its shape, while
weak/strong emission depends on regulated sides. Conducted reads can include or exclude
dust and must read the appropriate live neighbors.

**Consequence:** one generic power bit can change comparator/repeater inputs or feed dust
back into a calculation which deliberately excludes it.

**Required rule:** retain port/channel meaning and four-bit attenuation. Reduce to Boolean
only where the receiving operation proves that zero/nonzero is sufficient.

**Evidence:** [power semantics](../../../crates/core/src/redpiler/analysis/instant_regions/compiler/power.rs),
[electrical parser tests](../../../crates/core/src/redpiler/analysis/tests.rs), and
the external-wire/channel tests in [components.rs](../../../crates/core/src/redpiler/backend/graph/tests/components.rs).

### E20. Lowering can reorder IDs referenced by a certificate

**Situation:** power links are sorted by destination and proxy lowering changes route-row
ordinals after draft boundary analysis.

**Consequence:** retaining an unremapped numeric index can make a correct formula read the
wrong source or bind the wrong callback occurrence.

**Required rule:** use stable semantic bindings or remap references at lowering. Keep cold
draft witnesses separate from hot entry keys. Validate ownership and reference ranges.
Canonical entry ordering also needs a complete key: adding direction to a delivery-kind
number causes Update/Shape collisions, and truncating contact IDs to eight bits causes
collisions in large graphs. Keep distinct key fields and the full ID width.

**Evidence:** `regional_adder_execution_matches_general_graph_and_interpreter_handoff` in
[stable_region.rs](../../../crates/core/src/redpiler/backend/graph/tests/stable_region.rs)
explicitly exercises a nontrivial power-link permutation.

### E21. Host-owned ordinary blocks may exist outside an instant region

**Situation:** an unrelated command block carries an entity, or a note block carries
instrument state. A whole-selection symbolic initializer rejects host-owned state.

**Consequence:** otherwise valid instant regions cannot compile just because ordinary
devices coexist in the selection. Freezing those ordinary devices is equally incorrect.

**Required rule:** initialize and trace the scoped region; model only reachable outside
reads as boundary inputs. Host execution remains normal Graph behavior.

**Evidence:** scoped initialization now exists in
[compiler/state.rs](../../../crates/core/src/redpiler/analysis/instant_regions/compiler/state.rs).
Dedicated mixed-host/event-kernel validation remains required.

### E22. A correct private-state difference can still hide a wrong output

**Situation:** an optimization changes payload/contact state and initially appears to differ
only internally. An ordinary downstream repeater samples that difference later.

**Consequence:** comparing only a final idle output, or dismissing a payload discrepancy as
invisible, can miss the actual timed arithmetic error.

**Required rule:** compare declared real output windows, logical memory and pending work,
and continue long enough for ordinary consumers to respond.

**Evidence:** E03 checks the real repeater output, not just the sum payload. The independent
`compiled_adders_preserve_every_pinned_arithmetic_history_and_safe_handoff` regression now
checks all 48 pinned histories; it is added but not yet run on the new event kernel.

## Regional event-kernel risks requiring explicit validation

### E23. An outside callback can synchronously reenter the owned region

**Situation:** an outgoing callback changes a bulb or starts an external Turbo walk, which
delivers a callback back into owned execution before the original cascade returns.

**Consequence:** computing the region to completion and replaying all outside effects
afterward can use the wrong state and power.

**Required rule:** prove no such path exists, or compile a continuation/barrier preserving
the dependency. A conservative decline is valid but must be visible in coverage reports.

**Status:** identified by semantic review; the new regional admission proof and dedicated
regression are not complete. Existing nested-bulb tests establish that synchronous nesting
is a real supported Graph behavior, not a purely hypothetical API concern.

### E24. An outside write can change a later outgoing guard or capture

**Situation:** the first buffered callback changes an ordinary component; a later emitted
effect reads that component's powered/locked state or tests it in a guard.

**Consequence:** freezing all outside reads at routine entry can incorrectly add, remove or
alter the later effect even if no callback reenters owned handlers.

**Required rule:** account for outside read/write dependencies, emit an appropriate ordered
transfer, or decline the unsafe closure. No-reentry alone is insufficient.

**Status:** review risk; dedicated event-kernel regression remains required.

### E25. A live outgoing read must see its event-time state, not the final state

**Situation:** owned state changes again after an outgoing callback would have run. Its
receiver reads region-derived power or geometry. Capturing the receiver's own properties
does not capture the upstream input it samples.

**Consequence:** replaying that callback against final owned state can lose E01's pulse.

**Required rule:** lower the observed effect directly, or provide its sparse event-time
owned-state view while the normal outside handler runs. Preserve incoming captured operands
separately. A temporary overlay may not generate dirty flags, revisions or callbacks itself.
An overlay is relative to the routine's **final** state, not its entry state. A field that
is unchanged at the callback but written later still needs restoration to its entry value
during that callback. Include the union of final writes and snapshot changes, then prune
equal values. Revision snapshots require the event-time origin plus selected write prefix;
do not increment an already-final revision again while overlaying it.

**Status:** the repeater behavior is confirmed; the new overlay/effect path needs its own
integration regression. This is not a claim that every adder produces such a transient.

### E26. An outside handler can observe future-queue prefixes

**Situation:** an earlier owned action creates a publication or request, an outside callback
samples it, and a later owned action appends different work within the same cascade.

**Consequence:** appending all future work before callbacks exposes work too early; appending
it afterward hides work that already existed. Swapping queues can lose outside-generated work.

**Required rule:** use one ordered effect stream for schedules, requests, publications and
outside callbacks, replayed incrementally into Graph's existing future. Preserve initial
pending/witness inputs and independently revision-qualified publications.

**Status:** compiler effect logging is implemented; end-to-end event-kernel replay is unfinished.

### E27. Outside logical memory need not return to its starting value

**Situation:** a Counter's BUD memory advances while its instant/reset machinery repeats.

**Consequence:** requiring complete-device state recurrence rejects a valid instant region,
or incorrectly compiles ordinary memory into an instant cycle.

**Required rule:** compile a regional transfer over boundary inputs. Leave normal memory
outside; do not require global periodicity or a globally empty future.

**Status:** ordinary BUD behavior is tested by E02. Larger Counter validation with genuine
internal-work removal remains required.

### E28. Deferred interpreter handoff is not a private time advance

**Situation:** the user requests interpreter mode while a compiled instant/reset transaction
is still active and internal geometry is not materialized.

**Consequence:** ticking only that region to finish its reset would desynchronize unrelated
devices. Switching immediately with partial state can lose pending effects.

**Required rule:** handoff may wait a justified few logical ticks at the user's permission,
but the whole Graph continues normally. Use an explicit pending/completed lifecycle and a
certified safe point; otherwise retain the stronger immediate reconstruction.

**Status:** newly permitted contract, not an implemented deferred-handoff API.

### E29. Unsupported geometry must fail compilation without altering the input

**Situation:** moving entry, malformed heads, moved electrical components, unsafe support
loss or footprint escape cannot be represented by the supported contact model.

**Consequence:** silently accepting them yields missing blocks or incorrect callbacks;
mutating the input to make compilation succeed destroys user data.

**Required rule:** structured rejection with a position/actor/material witness and no partial
activation. Stationary signs on piston bases are permitted by the agreed model; do not reject
them merely because another attachment position would lose support.

**Evidence:** [geometry parser](../../../crates/core/src/redpiler/analysis/geometry.rs) and its
tests. For CPU benchmarks the sign normalization is an explicit, identical in-memory change
to both inputs, not a schematic modification or a general acceptance proof.

### E30. Budget exhaustion, cancellation and stale plans are not valid kernels

**Situation:** symbolic construction exceeds its budget, a user cancels, or another pass
changes the graph after a plan was derived.

**Consequence:** a partially emitted or stale plan can reference invalid owners or describe
different semantics from the graph being executed.

**Required rule:** publish complete validated plans atomically. Report unsupported regions
and cancellation explicitly; invalidate or rederive affected metadata. Do not silently count
general Graph or the old whole-selection machine as regional compilation.
Do not catch a cancelled trace as an ordinary unsupported-region decline. Large semantic
trace budgets also need cancellation checks inside tracing, not only before and after it.

**Evidence:** `symbolic_region_build_budgets_cancellation_and_stale_plans_are_checked` in
[symbolic_kernel.rs](../../../crates/core/src/redpiler/backend/graph/tests/symbolic_kernel.rs).
New event-program validation remains to be completed.

## Performance traps that can disguise failure

### E31. More compiled steps can mean more work, not replacement

**Situation:** per-piston symbolic predicates run, but every original wire walk and callback
still executes afterward.

**Consequence:** compile/step counters look positive while runtime gets slower.

**Required rule:** report actual eliminated propagation and measure release speed. The new
work-replacement regression requires a large reduction, not merely a positive VM counter.

**Evidence:** an intentional failing baseline run of
`compiled_adder_regions_replace_internal_propagation_and_preserve_outputs` recorded identical
one-bit work for O3 and the old O4: 1,877 callbacks, 45 walks and 830 piston samples over
56 ticks. O4 added 1,077 symbolic steps. Functional checks before the work gate passed;
the work gate correctly failed. This is baseline evidence, not a new-kernel result.

### E32. Untimed activation can hide the actual cost

**Situation:** the compiler moves computation from timed ticks into the trigger operation,
which the benchmark performs before starting the tick timer.

**Consequence:** a faster tick number can conceal equal or worse total execution cost.

**Required rule:** report activation/preparation execution separately and include it in an
episode metric. Keep load, compilation, flush and correctness inspection outside both timers.

**Evidence:** benchmark schema 6 adds preparation, phase-stimulus and full-episode execution
times and work counters. O4 stable runs include O3 stable in the same alternating sample
group. The revised harness passes compilation checks; new-kernel timings are not yet available.

### E33. Dead input declarations can remain on the hot path

**Situation:** a shared symbolic arena contains many region fields, but one event uses only
a small cone. Lowering prunes operations while retaining all declared input bindings.

**Consequence:** evaluating a small routine can still read or allocate storage for the whole
region, reproducing the eager-read overhead of the old per-actor VM.

**Required rule:** keep only reachable runtime bindings and actual writes. Original input IDs
and root order must remain meaningful after compaction.

**Evidence:** `Diagram::circuit_live_inputs` preserves the existing declared-input API and
offers compact bindings for event kernels. All 26 `mchprs_redpiler_opt` tests passed in debug and release during
this work, including sparse-ID equivalence, duplicate roots, constant/empty roots and invalid
input validation. Runtime use by the new kernel remains to be verified.

### E34. Enumerating impossible event metadata creates a quadratic compiler

**Situation:** publication entries are generated for every actor crossed with every resource
and every contact in the complete region, even when an actor's lane can never publish most
of those resources or contacts.

**Consequence:** a large adder pays for many impossible symbolic traces and retained plans.
This can exhaust budgets or memory before the valid arithmetic width becomes relevant.

**Required rule:** derive publication keys from the actor's actual lane/resource relation.
An extension can publish its own lane's far contact, a retraction its near contact, and a
payload-less action has no resource witness. Shared resource identity does not mean every
actor can publish it at every contact. Keep the catalogue structural and deduplicated.

**Status:** the Cartesian loop was identified in the new draft during source review, before
larger fixtures were run. This is not an explanation of the historical 42 GiB report.

### E35. Canonical BDD construction can explode before a single event plan exists

**Situation:** the first ADDER1BIT incoming entry, `SiteId(419) → Component(NodeId(89))`,
uncaptured Update from Top, enters the symbolic wire/Turbo closure. It reaches exactly
1,048,576 decision nodes after 6,894,601 logic-work charges but only 329 semantic steps.
The region has 13 actors and nine reset nodes. No event Plan is produced.

**Consequence:** a large semantic-step budget or successful parser coverage cannot fix this
particular failure. Increasing the node ceiling alone risks excessive compiler memory.

**Required rule:** avoid requiring a canonical truth relation merely to record a computation.
The expression IR can retain a shared structural conditional DAG and lower that to the
existing optimized Plan. Use canonical BDD reasoning only where its proof is needed and
bounded. Structural identity must not be advertised as complete functional equivalence.

**Status:** the node/work failure and call path through `seed_wire`/`walk` are observed.
Arbitrary independent stale wire/geometry inputs are a likely cause, not an isolated proof
of the exact exponential dependency. `Diagram::new_structural` now exists; its Boolean and
four-bit operations are independently tested against canonical/scalar results. The first
structural attempt also reached 1,048,576 nodes, after 6,838 semantic visits. A bounded
syntactic implication simplifier moved the failure to 7,715 visits but did not resolve it.
The exact failing site had 560 guarded layer alternatives; retaining alternative 293 at
target layer four exhausted the node budget. This is observed path-partition growth,
not evidence that all those paths are feasible.

**Current mitigation:** `Diagram::prove_false` expands a bounded nearby ITE cone into
local clauses and performs unit propagation under the assumption that the guard is true.
Unexpanded expressions are independent variables, so omitted constraints only relax the
problem: a contradiction remains a sound proof, while failure or exhausted work means
unknown. It never guesses that a path is impossible. Three targeted tests cover a hidden
partition contradiction, invalid roots, and generated three-input expressions across
truncated node/work budgets; all 34 optimizer-library tests passed in debug and release.
The helper is integrated into walk-layer control points. Layer coalescing plus those proofs
reached 141,908 semantic steps before exhausting the same node budget at wire-power
derivation. Legal wire-side encoding alone then reached 141,880 steps and failed at
`walk.identify.bias-write`. Both attempts still stopped before the first event Plan existed.
The final canonical-heading power change has not had a fixture rerun.

### E36. Repeated layer occurrences must not become separate symbolic state alternatives

**Situation:** `advance_layer` appends a `(layer, guard)` row for each admitted occurrence.
Several rows can describe the same numeric layer. The observed failure in E35 had 560
rows at target layer four; their exact distinct-layer count was not measured in that run.

**Consequence:** each new occurrence rewrites all prior rows even though the state being
represented is only the site's last-enqueued numeric layer. Occurrence history belongs in
the ordered FIFO queues; duplicating it in the layer-state representation multiplies guard
construction without adding state information.

**Required rule:** retain one row per distinct numeric layer and OR its selecting guards.
This is an exact quotient: all valuations selecting that layer have the same value in every
later `old_layer >= target` test. Do not merge or reorder the actual queued occurrences.
There is also a local invariant: `active = when AND NOT OR(guards with old_layer >= target)`
is disjoint from each such guard. Only rows below the target need the `AND NOT active`
rewrite; rewriting the other rows constructs an identity the generic simplifier then has
to rediscover.

**Status:** implemented in `advance_layer`; the former 560-row failure no longer stops
the first entry at that point. Successful Graph fixture compilation and an independently
executed partition-equivalence regression are not established. It does not authorize
unordered wire propagation or removal of callbacks.

### E37. Independent predicates can describe impossible enum and geometry states

**Situation:** the scoped compiler currently imports five independent Boolean predicates
for an actor's availability. Runtime availability is one enum value, so exactly one of
those predicates can be true. Resource contact selections similarly share one location;
a stored wire side cannot simultaneously be both Up and Side.

**Consequence:** treating every predicate combination as a feasible symbolic entry may
expand branches which no valid runtime circuit can reach. A sound overapproximation is
safe for output correctness but can prevent termination or useful simplification during
tracing. Stable external inputs do not make invalid enum combinations valid.

**Required rule:** retain the invariants at the input representation or prove them explicitly.
A compact shared enum encoding can derive all equality predicates from the same bits;
any unused codes need a stated interpretation or an enforced admission check. Do not
substitute the fixture's initial state for live memory merely to shrink the formula.

**Status:** wire-side input encoding is now None=00, Side=01, Up=10, with 11 also selecting
Up; captured wire sides use the same encoding. Independent availability/contact predicates
remain. The side change alone did not avoid the ADDER1BIT node limit (E35); do not attribute
the entire failure to invalid side combinations. Runtime bindings still need complete
valid-value coverage; dynamic SSA writes must survive subsequent reads.

### E38. A specialized wire visitor can bypass the regional ownership boundary

**Situation:** `Compiler::visit_site` dispatches a wire directly to `visit_wire`, while other
receivers pass through `Execution::deliver`. The regional ownership check is in `deliver`.
If a captured Turbo neighborhood contains a wire outside `region.wires`, that fast path
can symbolically write the outside component instead of emitting a boundary delivery.

**Consequence:** final sparse writes contain only owned state, so an outside write could
be discarded together with the missing boundary callback. Tracing can also expand into
an unrelated net. Checking ordinary receiver dispatch alone does not establish closure.

**Required rule:** check ownership before specialized wire execution as well. An outside
wire receives an explicit captured boundary callback; owned wires retain their original
walk semantics. Do not quietly enlarge ownership to include a neighboring ordinary device.

**Status:** the specialized wire path now checks regional ownership and sends outside wire
receivers through the boundary receiver path. No targeted executed fixture replay yet
establishes the correction's observable behavior. The remaining regression must check emitted
delivery, unchanged outside SSA state, and retained captured values.

### E39. An unsuccessful early AND sample cannot consume the computation use

**Situation:** A reaches an AND-like cell before B. The early sample does not qualify an
accepted computation. B then arrives in the same wave and makes the cell eligible.

**Consequence:** a visited/fired bit set on first traversal or first request loses the later
result. Marking an entire cable visited can similarly discard another contributor.

**Required rule:** consumption means accepted computation under its live validity predicate.
Retain value availability and activation separately; a failed sample does not consume either
the actor or every contributor to its input net.

**Status:** logical counterexample motivating the proposed layer algorithm, not a newly
executed native fixture regression. Step 01/02 must establish the physical decoder and
step 04 must test both arrival orders before claiming this case is handled.

### E40. Single-use per wave does not mean single-use per tick or held-input episode

**Situation:** reset/restoration can cause another response while external power remains
held. A physical actor can therefore perform reset work and later computation again.

**Consequence:** a permanent fired bit swallows later responses; clearing it on every
notification allows feedback to refire the current computation. One shared game-tick number
does not distinguish computation, reset and a newly admitted wave.

**Required rule:** define a certified wave cut and actor role. Consumption is scoped to
one computation wave. Reset clock wakeups can start another wave through the existing
scheduler; no new external lever edge is universally required.

**Evidence/status:** PISTON_MODEL section 5 distinguishes held zero, internal observer/
restoration callbacks and subsequent response cycles. The executed Step 01 ADDER1BIT
census records the prepared-0-1-1 history retracting the same seven actors at tick 33,
extending them at 36, retracting at 39, and repeating in later drains. Across 48 prepared
histories it observes 1,008 ordinary retractions and 1,008 extensions. The 40 ADDER11BIT
histories likewise reuse actors across separate drains. No actor had a second ordinary
retraction within one drain in these histories. This supports a drain-local bound, not a
permanent consumed bit or a universal reset period. The layered runtime's computation/
reset classification and safe continuation still require implementation and verification.

### E41. A finite-use bound does not prove a small symbolic compiler artifact

**Situation:** N actors may each be consumed at most once for one concrete input frame,
while different input/activation conditions place candidates in different causal orders.

**Consequence:** a compiler can still enumerate exponentially many conditional histories.
N accepted uses is not a bound of N candidate records, layers or Boolean expressions.

**Required rule:** share logical pin/owner expressions, analyze SCCs locally, preserve only
necessary causal blocks and budget construction work/bytes. Decline a candidate rather than
truncate a trace or increase limits repeatedly. Compiler DFS order cannot choose its result.

**Status:** general construction risk supported by the historical E35/E36 failures, not a
measured failure of the proposed gate-level compiler, which does not exist yet.

### E42. Deferred handoff cannot depend on global reset-clock alignment

**Situation:** independent reset clocks can keep future work pending and need not reach a
globally idle or identical combined phase within the allowed handoff interval.

**Consequence:** waiting for global quiescence can stall indefinitely even though each local
mechanism has a usable safe cut. Advancing only one region changes its relation to ordinary
devices and deadlines.

**Required rule:** use a certified local reconstruction cut with a justified wait bound;
continue the ordinary global clock, preserve unrelated work, and transfer future obligations
once. If no supported bound/recipe exists, decline kernel admission before it owns execution.

**Status:** lifecycle review obligation, not an executed failing handoff test. The new reset
and handoff step must cover staggered clocks and repeated requests explicitly.

### E43. A known Boolean result cannot commit before its acceptance phase

**Situation:** a boundary callback samples power before `PistonActions`. Another queued
ordinary action can change an inhibit or shared-resource condition before acceptance.

**Consequence:** immediately publishing the kernel's eventual output from that callback
moves the computation across the scheduler/FIFO boundary and can expose a result that the
physical action would cancel. Stable external data does not remove ordinary queued work.

**Required rule:** separate sampled requests from accepted effects. Use the existing action
queue and phase; batch internal acceptances only when intervening work cannot matter. Yield
at a supported continuation cut or decline before taking ownership. No private region clock.

**Evidence/status:** source requirement from `backend/graph/pistons.rs::enqueue_piston`,
`drain_piston_actions` and `accept_action`, which rechecks live power and occupancy. The
layered runtime has not yet been implemented or tested against this obligation.

### E44. Reset provenance may watch a different shared owner

**Situation:** the observer providing reset power to an actor watches another base in the
same shared ownership group. The actor receiving power is not necessarily the actor whose
base-change route arms that observer.

**Consequence:** requiring the receiving actor's own base route to notify the reset rejects
valid adder cells. Conversely, attaching reset effects to the wrong base changes pulse timing.

**Required rule:** retain the recognizer's `watched_actor`, close that ownership dependency,
and bind the reset's exact directional callback occurrences to the watched base's change
and restoration routes. Preserve the existing observer state and scheduler owner.

**Evidence/status:** source-derived ADDER1BIT and ADDER11BIT cell tests compare those route
occurrences to the prepared graph. This does not yet prove a contracted reset lifecycle.

### E45. A valid instant can reset without an observer

**Situation:** `INSTANT_RESET_REDSTONE` contains a sticky piston, redstone blocks and wires,
with no observer reset source. Current local-cell discovery is seeded by observer provenance.

**Consequence:** no local cell is emitted for this fixture even though native reference
execution is available. Treating the fixture name as recognition, or inventing a reset-node
binding, would hide a missing reset family.

**Required rule:** retain ordinary Graph execution and report this admission limit until
feedback-reset dependencies, activation and future work have their own derived contract.
Do not claim full instant-fixture compilation from all-actor adder coverage.

**Status:** explicit recognition limit; neither an output mismatch nor an activated kernel.

### E46. A control can enter through a wire query without a PowerLink

**Situation:** ADDER1BIT's trigger lever has no outgoing `PowerLink`, but its callbacks and
canonical wire-neighborhood reads cross into the candidate instant circuitry. Wire power
queries can read a neighboring component directly.

**Consequence:** counting only electrical-link rows misses a real control. The three data
controls plus trigger must not become a claimed three-input device by losing activation.

**Required rule:** collect value/query and activation bindings independently. Retain the
original wire/neighbor pair and route occurrence; do not synthesize a power edge merely to
make a port-count test pass. Pin the four physical control positions in the adder test.

**Evidence/status:** observed in fresh ADDER1BIT and ADDER11BIT boundary exports. This was
an incomplete test assumption, not an executed Graph/native output mismatch.

### E47. An ordinary BUD update path is not an instant-region output

**Situation:** in `bud_instantmemorycellobserverupdate`, the admitted data cell supplies QC
power, while the memory's sampling callbacks originate in ordinary update devices outside
that cell and its connecting corridors.

**Consequence:** requiring the instant catalog to emit the sampling update would invent an
effect or incorrectly absorb ordinary state. The two paths remain independent.

**Required rule:** retain the outgoing QC dependency, leave BUD memory and ordinary update
devices outside, and preserve their original routes. Expose an update output only when its
actual source belongs to the candidate instant circuitry.

**Evidence/status:** all 18 original memory-targeting route rows in the inspected fixture
originate outside the admitted owners. Existing native/Graph BUD tests check that changing
QC data alone holds memory until a qualifying ordinary update.

### E48. A synthetic empty-actor compiler family can be unreachable after real admission

**Situation:** the first Step 04 draft compiled a single actor only when its resource list
was empty and every power contribution had an outside boundary port. The actual mechanism
recognizer requires a sticky actor with a resource at both Near and Far and power supplied
by its recognized reset observer. That reset belongs to the cell; its power need not be
an outside input. Synthetic cell equations alone did not exercise these admission facts.

**Consequence:** a Boolean evaluator can pass a large exhaustive grid while its accepted
family contains no real parsed mechanism. That is not circuit compiler coverage.

**Required rule:** test through normal preparation and cell discovery, retain local reset
clock reads under the scheduler's existing owner, and derive the private payload's
source-absence predicate from accepted transfer. Do not fabricate outside ports or treat
internal wire values as independent inputs. Shared or unproved families still decline.

**Evidence/status:** source review of `discover_indexed_mechanisms` in
`passes/utils/instant_mechanisms.rs` establishes the required nonempty sticky resource
lane and reset provenance. The first 22-fixture Step 04 census compiled zero actors.
A real parser-backed narrow-cell regression is being added; no successful execution
or acceleration is claimed by this entry.

## Updating this register

For each newly found case, add the smallest scenario and distinguish:

- **Observed failure:** name the fixture/history and the actual output, memory or deadline
  difference. A private-state mismatch alone does not establish an output failure.
- **Established semantic requirement:** link interpreter code and an existing regression.
- **Review risk:** explain the failing mechanism and add a targeted regression before claiming
  the new implementation handles it.

Record tests actually executed separately from inspected source and historical reports.
Do not weaken a semantic test just to make the new kernel pass; document any intentional
contract relaxation and retain checks for the observations that still matter.
