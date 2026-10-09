# Minimal PC counter admission: diagnosis and Redpiler support plan

## Summary

Redpiler rejects this six-piston counter because it cannot represent a QC-powered instant piston whose live power is reevaluated by a separate physical notification. Its power data and its activation event travel on different paths: the upper circuit changes quasi-connectivity (QC) power, while READA/READB callbacks cause the receiver to check that power.

The receiver pistons in this fixture are instant or QC-instant actors. They are not BUD memory cells. Their pose persists as ordinary physical actuator state, but the circuit does not store a logical memory bit in these pistons. The earlier report's “memory piston,” “BUD write,” and “sampled response” descriptions were inaccurate for this case. Redpiler should keep `MemoryCell` for actual storage circuits and add event-driven activation semantics for instant and QC-instant actors.

Removing the current rejection guard would also be wrong. It would let a power-source change update a receiver even when no piston notification was delivered. The fix must model the notification as an activation event and reevaluate the actor's current power at that event.

## Fixture and reproduction

The fixture is `PC_COUNTER_MINIMAL_FAIL.schem`, sourced from `urmom:/srv/mchprs/data/schems/PC_COUNTER_MINIMAL_FAIL.schem` and downloaded read-only on 2026-10-09. It is 884 bytes, 7 × 10 × 9 blocks, with SHA-256 `2b2df49a3bd2f9bf7049126f751a67e9d251a3ecd4e2234e867634d51887ea8b`. The READB, READA, and UPDATE signs identify the circuit. Coordinates below are selection-local; tests paste the selection minimum at `(40,30,40)`.

The ignored regression `saved_pc_counter_minimal_exposes_internally_driven_qc_admission` was run with:

```text
cargo test -p mchprs_core saved_pc_counter_minimal_exposes_internally_driven_qc_admission -- --ignored --nocapture
```

It passed by confirming the expected rejection in all four combinations of optimization on/off and `--assume-instant` on/off. Each diagnostic points to local actor `(1,3,2)` and data source `(1,6,4)`: “unsupported internally driven QC sampling interface … data source … does not notify the base; a movable writer delivers updates without an explicit sampled boundary.” This verifies the current rejection path; it does not show that the full counter output matches vanilla Minecraft.

The saved reductions report successful compilation when either the input instant mechanism or the READB mechanism is removed. Removing READB and its far redstone block causes an earlier rejection because the reset observer at `(1,4,4)` then has no qualifying activation path to the QC-instant receiver. These reductions help isolate the routes; passing a reduced case does not prove full counter behavior.

## Circuit behavior established by the fixture probes

The upper input instant moves the redstone block at `(2,7,5)`. This changes dust at `(1,7,5)`, which powers the rear of the ordinary repeater at `(1,6,4)` through its support. The repeater output powers `(1,5,4)` and dust `(1,5,3)`. That path supplies QC power to the two downward receiver pistons at `(1,3,2)` and `(1,3,4)`. The repeater's ordinary output notifications do not reach either receiver base.

READB moves the redstone block at `(0,4,3)`, changing dust at `(0,4,2)`. That dust delivers callbacks to the receiver even though the QC data path does not notify its base. The interpreter regression records three distinct callbacks in `PistonEvents`, from `(0,4,2)` with directions `None`, `West`, and `Top`, in that order. The receiver is powered at each callback. These are activation events for an instant/QC-instant actor, not writes to a memory cell; each callback must remain observable and ordered.

A longer interpreted probe records READA fanout from dust `(2,3,3)` to both receivers, including repeated callbacks and later piston-base/head callbacks. In the declared probe, the first fanout at tick 21 is: second base `None`, first base `None`, first base `East`, second base `East`, first base `South`, second base `North`. READB delivers its three callbacks at tick 41 in `PistonEvents`, and again at tick 82 in `MovingEntities`. These are interpreter timing observations. Redpiler admission must use its certified phase model rather than copying these absolute ticks into a circuit-specific rule.

## Why Redpiler rejects the circuit

### Architecture correction: current `redpiler/piston-1.21.5` checkout

The original reproduction below is historical evidence. The source review on
2026-10-10 did not rerun it. The report previously described the merged-domain
runtime; that dispatch path does not exist in this checkout.

The current backend selects `Runtime::Direct` or `Runtime::Native`. Instant
programs execute in Direct alongside the ordinary collapsed graph, without an
attached native executor. `PreparedInstant` carries sampling events and actual
memory cells, but no separate response activation policy or ordered activation
routes. `Runtime::advance` derives sampling delivery from a change in generator
or power strength, records a Boolean `delivered` flag, and processes a writer
group against an old memory snapshot. This cannot encode repeated unchanged
READA/READB callbacks. Source-driven response reevaluation also cannot express
a receiver whose available power changes without activating it.

Selective input capture is present in the reviewed worktree. It changes cache
maintenance, not activation semantics. Existing uncommitted runtime changes were
not modified by this documentation review.

Redpiler already records two different facts about a piston:

1. `recognition.inputs` and topology describe the electrical power function, including direct and QC routes.
2. `PistonPorts.updates` describes physical callbacks that can reach a base or head. `UpdateDependency` explicitly says this callback channel is separate from Boolean power dependencies and is not itself a sampling certificate.

The admission path does not yet combine those facts into an activation model for an instant/QC-instant actor:

1. `instant/sampling.rs::recognize` classifies fixed actors, empty generators, and independent-memory/BUD candidates. It excludes actors watched by reset observers from the independent-memory class. That exclusion is sensible for real storage BUDs, but this fixture's receivers are instant/QC-instant actors and need a separate classification path.
2. `instant/program.rs::prepare` walks the ordinary graph descendants of each instant output. The upper input instant makes repeater `(1,6,4)` internally driven. It then calls `sampling::validate_feedback` for response actors.
3. `sampling::validate_feedback` sees the repeater as a QC data source that does not notify the receiving base. It also finds a movable writer feeding wire-notification routes to that receiver. Since there is no prepared activation model for those deliveries, it rejects the case at the diagnostic above.
4. On the current branch, runtime sampling detects changes in prepared source values and groups deliveries by writer. It has no ordered callback-triggered response entry for ordinary instant/QC-instant actors. The merged branch's `dispatch_owned_update` and `notification_targets` are historical implementation references, not available hooks to extend here.
5. Conversely, `Runtime::advance` reevaluates dirty logical responses when ordinary sources change. If the admission guard were simply removed, a repeater/QC power change could change the actor response without a callback to its base. That would accept the fixture by changing its semantics.

The guard therefore exposes a real model gap, even though the circuit's callback and power routes are legal. The correct response is not to remove the guard or classify these pistons as memory. Redpiler must represent when an instant actor checks its current power.

## Redpiler change needed

Add an activation policy for piston responses that is separate from storage classification. An ordinary instant actor may be activated by its supported ordinary update paths. A QC-instant actor may read its current direct/QC power when a qualifying notification arrives. Neither policy implies a `MemoryCell`. Existing storage BUDs keep their explicit stored bit and sampling protocol.

The compiler should derive each actor's activation routes from the physical update ports and reset/phase certification, alongside—not from—the electrical dependency graph. For this circuit, that means resolving READA/READB dust callbacks and relevant piston base/head deliveries to the receiver actors, including routes that cross assembly boundaries. The repeater remains a live power dependency; its output notification is not required to be the receiver's activation event.

At runtime, dispatch each qualifying callback to the affected instant actor in delivery order. Refresh the actor's electrical inputs, evaluate its response at that callback, and advance its existing piston/reset phase state. A source-strength change may refresh cached inputs, but it must not activate a notification-gated actor on its own. Keep repeated callbacks as separate events. For a shared fanout, preserve the engine's established callback order and phase semantics rather than deduplicating by source or actor.

The feedback validator should accept an internally driven QC data path when it can prove a valid activation route for the receiver and the route's delivery semantics are represented. It should continue rejecting unresolved routes, including the reduced case where READB's moving payload is removed and the receiver has no independent activation. Run this validation across the union of connected assembly routes; a per-program descendant check cannot establish a trigger path that crosses a program cut.

For this branch, compile activation-connected assemblies into one region where
ownership and reset certificates permit it. Otherwise reject an unresolved cut
until explicit inter-region event delivery is supported. Ordinary repeaters
remain delayed graph components. Internal dust stays compiled; an activation
route must generate its ordered notifications without restoring physical dust
execution. External decoration shape callbacks are outside this interface.

Implementation requires a targeted notification/event adapter for the collapsed
graph and compiled assembly routes. Copying the merged backend wholesale is not
required. Current computed electrical responses and committed actor responses
must become distinct: downstream geometry reads use committed actor state, not
an unactivated candidate. Do not emulate this distinction by adding a memory
cell or merely suppressing assignment to `fired`.

The proposed formal extension and current implementation limits are documented
in [the execution model](../../../docs/REDPILER_MODEL.md#13-proposed-notification-gated-instant-responses).
The code scope is in [the implementation plan](../../../scraps/PC_COUNTER_ACTIVATION_PLAN.md).

This is a model-level change, not a fixture exception: it applies to any instant/QC-instant actor with separated data and notification paths. It should retain reset geometry and phase certificates, use the ordinary response function for current power, and reserve logical stored state for actual memory circuits. `--assume-instant` must not bypass route or phase validation.

## Acceptance checks

The support change should be considered complete when:

- The six-piston fixture compiles with optimization on/off and `--assume-instant` on/off.
- A runtime regression proves a QC data-source change without an activation callback does not actuate the receiver.
- The recorded READB fanout is delivered in order, including the three distinct callbacks; READA fanout and base/head callbacks also reach the expected actors.
- A receiver evaluates current QC power at each qualifying callback and its physical response follows the certified reset/phase model.
- The READB-removed reduction remains rejected with a route-specific diagnostic.
- Existing true BUD/storage regressions continue to use stored-memory semantics and remain unchanged.
- The full counter's externally visible outputs are compared against a trusted reference over reset, input update, and read sequences. The current report and probes do not provide this end-to-end comparison.

## Scope and status

No Redpiler implementation change was made as part of this report. The admission reproduction was run and confirmed the current diagnostic. The interpreter probes provide callback-order evidence, but no Java recording or complete counter-output comparison was run. Several Redpiler source files already had unrelated or in-progress worktree changes before this report was edited; those files were left untouched.
