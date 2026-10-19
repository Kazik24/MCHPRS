# Closed instant networks and logical circuit reduction

2026-10-19. This extends the single-wire experiment into whole multi-wire
corridors. `unsafe-abstract-instant-nets` is a separate read-only pass and Cargo
feature, selected only at O4+ with `--stable-instant-inputs`. It is a hypothesis
test, **not an equivalent optimization or a production setting**. The normal
pipeline and the exact single-wire experiment do not enable it.

## Why the current compiler fails to compose the circuit

There are several separate restrictions, not a single connectivity bug:

1. `layers/partition.rs` joins private one-resource, one-use cells with their own
   base-watching observer. Shared memory uses and other reset families are cut.
2. A joinable cable must be flat, straight, at most 15 wires, and isolated from
   every other wire in its canonical neighborhood. Callback/electrical topology
   can therefore be connected while the executable partition remains split.
3. `composition/corridors.rs` requires a unique private producer and consumer,
   reset-only Quasi input, closed callback recipients and a particular settled
   imported power gradient. Those are proof-family restrictions, not definitions
   of an instant circuit.
4. `composition/connected.rs` declines multi-actor regions whose retained
   callbacks touch outside wires. It cannot substitute a final settled value for
   an unproved request-time arrival cut.
5. `layers/kernels.rs` refuses other synchronous outside handlers for multi-actor
   waves. The runtime then requires all actors' stationary entry predicates and
   a queue entirely explained by one admitted entry route. A large pipelined
   CPU can have connected instant logic without ever satisfying that atomic-wave
   entry condition.

A whole logical region should therefore be an event-driven stateful graph,
with original FIFO requests and reset clocks, rather than one giant Boolean
wave requiring the whole CPU to become idle. The existing `CircuitState` and
`GraphFuture` remain authoritative.

## A stronger structural boundary

The experiment starts from complete boundary-catalog corridors, includes all
admitted cell uses (including shared memory), and checks every possible contact
recipient. Wire members are internal. Actual possible non-wire recipients must
be admitted instant actors, their reset observers, or inert controls. Conducted
power is followed to its terminal consumers; classic consumers and unknown
pistons keep the complete net on general Graph. Incoming electrical sources must
also be internal wires, admitted cell resources/reset sources, or declared
controls/constants. Existing execution owners are excluded. Statically false
callback slots and power guards can be removed from this closure check; variable
guards remain possible regardless of the saved pose.

Only wire-interior propagation is removed. Original geometry, component/resource
state and reset clocks remain. Each selected boundary sample retains its original
wire route, but the experiment does not preserve the first qualifying occurrence
of that sample. The prototype reuses the existing experimental fixed-point
executor; it adds no second simulator.
Its validator checks structural closure, **not causal/FIFO equivalence**.

The final census, after distinguishing possible electrical edges from mere
geometric neighbors, found:

| Circuit | Closed multi-wire nets | Member wires | Largest structural logical group |
| --- | ---: | ---: | ---: |
| BubbleSort | 1,408 | 19,362 | 10,228 actors / 18,094 wires |
| FPU | 1,006 | 10,918 | 5,747 actors / 10,983 wires |

This is much broader than the singleton kernels installed by the current
executor. These are candidate groups, not executable kernel coverage. Recognizing
an island does not prove that its transient request sequence can be erased.
The largest groups include shared/reset cells and singleton corridors as well as
the contracted multi-wire nets. The census costs were 138 ms for CPU and 68 ms
for FPU, excluding validation, in a test build without LTO. These are preparation
diagnostics, not production compile-time benchmarks.

## Test boundary order separately from state

Strict comparison finds a reset-deadline ordering difference at CPU tick 8 and
FPU tick 1. The first differing entries are built-in reset clocks with no output
route. Counts match at those cuts, but pending entries are permuted. A clock
permutation alone does not establish whether the transitions commute, so the
probe also continues comparing projected block/entity state independently:

| Circuit | First ordered-future mismatch | Block/entity comparison |
| --- | ---: | --- |
| BubbleSort | Tick 8 | First mismatch at tick 26; 25 preceding ticks match |
| FPU | Tick 1 | 128 ticks match, including a control toggle at tick 64 |

The unchanged baseline matches native throughout each tested prefix. FPU's
bounded state agreement does not establish future equivalence or arithmetic
correctness. These interleaved diagnostic timings are not clean performance
measurements; no speedup is claimed for the broader reduction.

### A single-net memory counterexample

Delta debugging reduces BubbleSort's failure from 1,408 enabled nets to just
**corridor 18: 16 wires**, with every other net using its original execution.
This net connects paired/shared-reset memory cells and endpoint actors. At tick
26, both versions have identical piston poses and identical work totals:

- 5,029 accepted actions, 3,075 scheduled ticks and 4,176 deferred publications;
- 945 existing kernel invocations, with original selected wire routes retained.

Nevertheless, resources 9,455 and 9,456 occupy different contacts. Accepted-action
order first differs at zero-based trace index 202: endpoint retractions move
earlier relative to other cell operations. The full-net probe also exposes a
visible Air/MovingPiston mismatch at `(196, 30, 191)` on that tick. The evidence
rules out treating this as only a harmless permutation of pending reset ticks.
It does not attribute the later failure to the first clock permutation alone.

Thus even a completely instant-only net cannot generally be replaced with its
settled truth value and an arbitrarily ordered consumer list. Shared resource
transfers make the original first-request order observable.

First-census artifacts are in `target/instant-wire-nodes/closed-nets-v1/`;
final evidence is in `target/instant-wire-nodes/closed-nets/`, including
`cpu-minimal-counterexample.json`. `abstract-tests-4.exe` reproduces both final
probes and the minimization; `provenance-v4.json`, `source-v4.diff` and
`sources-v4/` preserve their source snapshot. An ignored
probe's successful exit means its diagnostic completed, not that the unsafe
reduction passed. Read `runtime_correctness`, the first divergence fields and
the tested-prefix lengths.

## Stronger semantic boundary and next pass

The useful boundary is **queue-only effects during a synchronous wire episode**.
Admitted piston callbacks sample live Main/Quasi inputs and enqueue a request;
observer/reset ingress schedules a future clock edge. Neither executes a piston
action or changes a cell resource before the episode returns. Consequently the
cell/source state is frozen while the net propagates, even if other cells are
already queued or resetting. This is a local scheduling property, not a claim
that the whole CPU is combinational.

That supports a proposed ordered boundary transducer:

```text
T_net(entry root, old net state, frozen cell inputs, pending membership)
    -> (new net state, ordered first requests, ordered reset events)
```

Its eligibility certificate must prove all reachable synchronous recipients have
only those effects, and all incoming sources remain frozen during the episode.
The structural census is a starting point for that certificate, not its proof.
Wire strength/threshold crossings are internal logical operations; boundary
samples retain the power at their original causal cuts. Source route witnesses,
duplicate suppression and first-request insertion order remain explicit. Paired
memory cells retain their shared resource state and their separate read actor.
There is no requirement for every cell in the connected region to be idle.

Compile each closed net into abstract signal operations with an ordered list of
qualifying consumer samples. Keep live Main/Quasi sampling, resource sharing and
first-request insertion order. Lower reset into logical clocks retaining the
required deadlines and order; a permutation can be internal only after proving
those transitions commute with all intervening readers and writes. Classical
ports retain their original observations and callbacks.

That representation can compose a large region while allowing individual cells
to be resetting or already queued. It avoids both repeated dust discovery and
the current all-ready/whole-queue wave restriction. Whole-region final-state
settling remains an experiment until these obligations are established. Use the
existing wire query rules when deriving operations: seed and Turbo ascent rules
are not interchangeable. Reset callback removal can follow where it proves a
redundant schedule; retain the logical clock transition, rearm and deadline.

The next implementation is this ordered local transfer compiler, starting with
the isolated 16-wire counterexample. It is not implemented by the unsafe
fixed-point experiment. Keep one authoritative `CircuitState`/`GraphFuture` and
the existing general fallback for uncertified nets.

## Validation of the experimental code

The final default-feature Redpiler suite passes 553 tests (40 ignored). Workspace
all-target checks and the core all-target check with both experimental features
pass; Python graph-loader tests pass 28 tests and the viewer passes 34 tests.
These checks include the safe single-wire transfer, classic/variable-source
rejections, original route retention and plan validation. They do not override
the explicit native counterexample for the unsafe reduction. Logs are under
`target/instant-wire-nodes/`; the final suite uses `final-tests.exe`.
