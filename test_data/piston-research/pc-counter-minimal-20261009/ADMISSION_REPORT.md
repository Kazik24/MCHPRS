# Minimal PC counter admission investigation

The schematic is a legal combination of instant responses, ordinary timed data,
and notification-driven reads. Current admission rejects the full example because
the receiving reset actors have no prepared sampled-response boundary. Removing
the guard would let electrical data changes act as writes.

Source: `urmom:/srv/mchprs/data/schems/PC_COUNTER_MINIMAL_FAIL.schem`, downloaded
read-only on 2026-10-09. The 884-byte schematic is 7 × 10 × 9; SHA-256 is
`2b2df49a3bd2f9bf7049126f751a67e9d251a3ecd4e2234e867634d51887ea8b`.
The READB, READA, and UPDATE signs identify this exact fixture. All coordinates
below are selection-local; the tests paste the selection minimum at `(40,30,40)`.

## Reproduction

Each result below holds with optimization on/off and `--assume-instant` on/off.

| Variant | Result |
| --- | --- |
| Original six pistons | Rejected at memory `(1,3,2)` for unsupported internally driven QC sampling; data source `(1,6,4)` does not notify the base |
| Remove input instant base `(4,7,5)`, head `(3,7,5)`, and reset observer `(4,8,5)`; retain far redstone block | Compiles |
| Remove READB base `(0,4,5)`, head `(0,4,4)`, and reset observer `(0,5,5)`; retain far redstone block `(0,4,3)` | Compiles |
| Remove READB mechanism and its far redstone block | Rejected earlier: reset observer `(1,4,4)` independently samples memory `(1,3,2)` without a coupled data update |

The two successful reductions have positive compile regressions. The full
example's rejection is retained only as an ignored reproduction; accepting it
with correct deliveries remains the required behavior.

## Causal paths

The upper input instant moves redstone block `(2,7,5)`, affecting dust `(1,7,5)`.
That dust supplies the rear of ordinary repeater `(1,6,4)`, through its support.
Its output supplies target `(1,5,4)` and dust `(1,5,3)`. This provides QC power to
the downward memory pistons `(1,3,2)` and `(1,3,4)`. The repeater's normal output
notifications do not reach either memory base.

READB moves its redstone block at `(0,4,3)`, affecting dust `(0,4,2)`. The dust's
callbacks reach memory `(1,3,2)` even when QC data did not itself notify it. An
independent interpreter regression records three distinct callbacks during
`PistonEvents`, with source `(0,4,2)` and directions `None`, `West`, `Top` in that
order. All three read the currently powered piston. They must not be deduplicated.

A longer interpreted probe also records READA fanout from dust `(2,3,3)` to both
memory bases, with repeated deliveries and later piston-base/head callbacks.
At tick 21 of that declared probe, the first fanout is: second base `None`, first
base `None`, first base `East`, second base `East`, first base `South`, second base
`North`. READB delivers its three callbacks at tick 41 in `PistonEvents`, and again
at tick 82 in `MovingEntities`. These physical timings are supporting evidence;
an instant acceptance test must use the certified phase model.

## Why admission fails

`instant/sampling.rs::recognize` excludes reset candidates from independent
memory classification. Both downward receivers watch their own reset observers,
so neither receives prepared memory sampling events. They remain response actors.

`instant/program.rs::prepare` walks ordinary graph descendants of each program's
output ports into `internally_driven`. With the upper instant present, repeater
`(1,6,4)` is one of these descendants. `sampling::validate_feedback` identifies
its QC contribution, establishes that it does not notify the receiving base, and
finds a movable writer feeding the receiving dust callbacks. It rejects the
missing sampled boundary at `sampling.rs:519`.

This is a model gap, not evidence of illegal ordinary feedback. The runtime's
`notification_targets` accepts only prepared external power samplers. Ownership
dispatch consumes other callbacks to assembly-owned bases without capturing the
response. Meanwhile `Runtime::advance` recalculates response actors whenever an
ordinary source changes. Removing the check would therefore update the receiver
on an unnotified QC data change.

Removing the upper instant makes the same data repeater an external ordinary
input rather than an output descendant. Removing the READB mechanism leaves its
redstone block constant, and also splits the lower actors from the upper instant.
The current per-program descendant/target check then does not connect that
cross-assembly route. Neither successful admission alone proves timing equivalence.

## Smallest sound completion

Represent these receivers as actors whose response is captured by qualifying
callbacks, while retaining their certified reset geometry and phase work. Route
actual READB/READA dust, base/head, and relevant reset deliveries through the
coordinator's ordered batches. A data-source change updates the available input;
it must not independently commit the receiver's sampled response. Each complete
fanout captures before commits, and later distinct deliveries see earlier commits.

Apply boundary validation across assembly cuts as well as within a merged region.
Retain diagnostics for unresolved routes, including the cross-reset case exposed
by removing the READB payload. Do not bypass the contract with `--assume-instant`.

No runtime admission guard was removed and no implementation fix was made in this
investigation. The opt-in probes and two positive regressions passed; no Java
recording or complete counter output comparison was run.
