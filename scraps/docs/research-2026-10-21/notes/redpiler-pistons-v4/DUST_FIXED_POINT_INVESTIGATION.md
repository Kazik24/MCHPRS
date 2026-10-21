# Unordered dust investigation and boundary-order plan

Date: 2026-10-14. Status: research completed for the first bounded episodes; the solver remains experimental and is not certified for production.

## What actually caused the CPU failure

The original experiment discarded every wire receiver at the notification boundary, including receivers outside the electrical component being settled. Electrical edges and update reach are different relations. Turbo's 24-site neighborhood can notify an electrically disconnected wire two cells away. Such a wire may need its stale strength recomputed even though it contributes no power to the triggering component.

A minimal regression uses a changed root wire at `(43,34,42)` and a separate stale wire at `(45,34,42)`. Exact Turbo brings the separate wire from 15 to 0; the old experiment left it at 15. The solver now suppresses only in-component wire notifications and retains external-wire callbacks. `fixed_point_preserves_wire_callbacks_outside_the_electrical_network` verifies the correction.

There was also a harness false positive: experimental comparisons used complete checkpoint dictionaries, whereas normal comparisons deliberately compare projected blocks, entities and chat. Native piston-inspection hashes include representation-specific physical state. The experimental comparison now uses the normal projected contract. The private reset fixture is a passing captured episode, not a failure.

## CPU evidence after repair

One release run of 100 active ticks, with the same approved in-memory sign removal on both inputs:

| Item | Normal O3 reference | Repaired experimental solver |
|---|---:|---:|
| Time | 1.079654 s | 0.9533848 s |
| Executed callbacks | 1,760,070 | 744,850 |
| Piston samples | 302,784 | 207,793 |
| Piston actions/publications | 27,884 | 27,884 |
| Scheduled ticks | 20,005 | 20,005 |
| Wire episodes | 32,505 | 43,947 |

The repaired run matches interpreter RAM at every sampled tick from 0 through 100. It cuts callbacks by about 57.7%, but performs more wire episodes because external-wire notifications can start another solve. The single-run time difference is about 11.7%; these are not paired samples, and machine conditions differ, so no reliable speed claim follows.

Pending scheduled ticks and piston events have the same counts but different order at reset. After 32 native continuation ticks, projected block/entity hashes differ although the RAM continuation matches. A short exact-Graph versus settled-Graph diagnostic also checks component states, piston states, resource state and ordered work separately. Matching displayed cells is insufficient to establish resource or continuation equivalence. In the short diagnostic, component and piston states match through tick 20; resource 1001 has different contact ownership at ticks 14–16 (ContactId 2290 versus 2325), then converges again. Scheduled/deferred order first differs at tick 8.

Artifacts: `target/cpu-fp-boundary-fixed.json`, `target/fp-cpu-first-divergence.json`, and `target/fp-investigation/`.

## Smaller examples

All 48 pinned arithmetic cases were replayed through all operations and 16 response ticks using exact Turbo and the settled solver on the same graph.

- Before the boundary fix, 41 cases had cell differences.
- After it, 16 cases still have cell differences; 32 preserve cells throughout the checked histories.
- Pending event/tick order can differ even when cells match. These are research observations, not 32 certified circuits.

The generic fixture harness additionally ran 64 round-robin control toggles and 512 ticks per device:

| Fixture | Repaired experiment |
|---|---|
| `TEST_PISTION_2` | Observations, projected state and future checks pass for this episode |
| `simple_and_gate` | Checks pass |
| `simple_or_gate` | Checks pass |
| `xor_and_1bit` | Checks pass |
| `ADDER1BIT` | First electrical trace difference at index 11 |
| `ADDER11BIT` | First electrical trace difference at index 179 |
| `rilax_adder_1bit` | Not measured: no existing lever protocol |

The arithmetic-manifest histories and the round-robin histories are different protocols; passing some prepared cases does not establish correctness for arbitrary inputs.

## Proven ordering counterexample

ADDER1BIT case `prepared-0-1-1` should produce sum zero at its first response tick, represented by a redstone block at `(58,32,41)`.

After the trigger, exact Turbo queues retractions in this order:

1. East-facing actor `(41,33,41)`, piston 6, resource 5.
2. North-facing actor `(49,33,45)`, piston 12, resource 9.

The settled solver reverses them. It produces Air at the sum payload position, representing the wrong bit. A third execution changes only the initial FIFO back to exact order, while retaining settled propagation. That restores the correct redstone block and matches the native response window. `fixed_point_reordered_adder_requests_change_the_sum_payload` captures this causal intervention.

The actors own different payloads. Their effects converge downstream at the output mechanism. Therefore neither different resource IDs nor absence of immediate shared ownership proves that deliveries commute.

## What an order-preserving optimization should compile

Keep two distinct artifacts:

1. An electrical plan: guarded attenuation edges and external power roots, used to compute wire strengths without repeated Turbo neighborhood queries.
2. A boundary-event plan: ordered notifications, including external wire receivers, required captures, directions, guards and multiplicity.

Internal wire-to-wire visits need not exist merely to carry power. Boundary notifications must not be sorted globally by position or node ID. Their order depends on the entry root, heading/visit history, topology and nested delivery. The first useful family should retain all necessary boundary order rather than immediately trying to prove deliveries independent.

### First implementation family

Start with a static private line or tree whose entry points and boundary receivers are explicit. Pure gate fixtures are good examples. Certify the following before replacing an episode:

- Electrical topology stays fixed in the certified state family; contacts that affect power only are distinguished from contacts that affect connectivity.
- Every external power root is known, and input sampling stays valid until the solver's publication boundary.
- Internal wire states are not observed through required transient values, updates, shape changes or synchronous reentry.
- Out-of-network wire notifications remain represented, even without an electrical edge.
- Boundary behavior is equivalent for all admitted strength/history cases, including imported work and reset continuation.
- Order and captures are retained for receivers whose effects can interact. Independence includes downstream electrical/update paths and resource ownership, not just immediate receiver identities.

Do not treat the presence of a delay as automatic independence: callbacks can enqueue work with the same deadline and priority, and FIFO can become observable later.

### Inferring an ordered boundary plan

For each supported entry/topology family, symbolically replay the existing Turbo queue rules during compilation. Remove internal computation from the resulting execution representation, retaining the boundary event sequence and its required operands. Where changing strengths alter which wires change, use a checked small family of guarded plans rather than one universal sequence.

Use the existing executor as an oracle, not as a production fallback hidden inside the plan. Exhaustively check small strength/state families and compare full relevant state, notifications, requests, acceptance, pending deadlines/order and continuation. A broader pass needs a mathematical invariant or bounded equivalence checker for regions larger than exhaustive enumeration permits. Unsupported roots, dynamic topology or reentrant episodes retain normal Turbo.

Only after ordered boundary equivalence is established should a further pass remove or commute boundary deliveries. A dependency/conflict graph must include downstream interaction closure. Unknown dependence keeps the original order.

## Coverage and expected benefit

A conservative census of 11 baseline graph exports contains 1,470 wires. 859 (58.4%) are locally free of contact/piston/observer hazards, but only 149 (10.1%) belong to potential wire components entirely free of those hazards. This is not a CPU census or a proof: torch/comparator feedback and update boundaries still need checking.

That conservative rule is intentionally coarse. A more useful proof should inspect meaningful electrical/update dependencies rather than reject every contact in a 24-site radius. Large gains remain plausible through finer region boundaries and complete instant proxies; a 90% coverage claim is not supported yet.

Current repaired timing suggests an incremental gain, not the stalled experiment's dramatic reduction. An engineering target of 10â€“30% lower runtime for the first ordered fast path is a hypothesis to measure. The previous profile attributes about two-thirds of tick samples to inclusive Turbo work; an idealized complete removal would cap that isolated opportunity around threefold throughput if all other costs stayed fixed. Actual settling, sampling and boundary delivery remain, and inclusive categories overlap.

## Validation limits

Focused tests cover attenuation/decay over 32 stale-strength/source cases, external-wire delivery, pass retention/validation/lowering, the adder FIFO counterexample, 48 arithmetic histories, and a bounded CPU diagnostic. The experimental CPU benchmark is one sample. Ordinary builds remain on exact Turbo.

Final focused debug verification: five fixed-point tests (including the 48-case research sweep and the short CPU probe), the pass test and three normal isolated-wire tests pass: nine tests total. Diff checks pass.

Not checked: complete CPU sorting, paired timing campaigns, broad fuzzing, the full suite, all legal protocols, or a production-ready boundary-order certificate. No approximate mode is enabled in normal optimization tiers.
