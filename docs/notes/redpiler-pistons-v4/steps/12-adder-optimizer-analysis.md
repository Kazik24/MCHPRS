# Step 12: Adder structure and optimization analysis

This note records measured runtime graph structure from the pinned adder fixtures and relates it to the Step 11 TEST family. Counts come from `GraphBackend::compile` and its executable `CircuitProgram`; the older `adder-1bit.graph.json` callback draft is not used as a runtime inventory.

## Runtime inventory

| Structure | ADDER_1BIT | ADDER_11BITS |
| --- | ---: | ---: |
| Nodes | 107 | 1,083 |
| Profiles | 34 | 37 |
| Guards | 3,760 | 36,700 |
| Sites | 660 | 5,191 |
| Contacts / pistons / resources | 36 / 13 / 10 | 393 / 142 / 109 |
| Wire nodes / power links | 32 / 205 | 295 / 2,047 |
| Routes / ordered deliveries | 184 / 4,847 | 1,869 / 48,923 |
| Ordered deliveries per route (mean) | 26.3 | 26.2 |

The 11-bit fixture has 142 pistons arranged as one 12-piston first slice and ten 13-piston slices. The first slice has 6, 3 and 3 pistons at local Z rows 1, 2 and 3. Each later slice repeats rows of 7, 3 and 3 pistons at offsets 0, 1 and 2 from every fourth Z cell. The remaining row in each four-cell interval is empty.

Node, route, power-link and ordered-delivery counts grow close to tenfold from one bit to eleven bits. Contacts, pistons and resources grow by about 10.9 times. The profile count stays nearly constant at 34 versus 37. The layout therefore repeats a mechanical and dataflow slice, while using a small vocabulary of block profiles. Both fixtures share resources across two or three candidate contacts; the shared token is part of the circuit's behavior, not merely a block value that can be copied or merged by material.

Neither circuit lowers to an adder or gate node. The executable model contains electrical, observer, wire, piston and contact nodes, with explicit sites, guards, shared resources, power links and ordered callback routes. The arithmetic is produced by the ordinary event and resource network.

## Relation to Step 11

Step 11 showed the same runtime vocabulary across thirteen TEST circuits and exercised more varied control histories and rotations. Step 12 adds a repeated bit-slice pattern and arithmetic output windows, but does not change the semantic center: shared payload ownership, callback direction/order, piston requests and acceptance, cancellation, and future work remain observable.

The adder counts suggest where to profile next: guard evaluation and ordered route dispatch have large compiled tables, and the route/delivery counts scale with bit width. The stable mean delivery count per route also suggests that a per-route indexing or representation change could be measured independently of logic rewriting. The inventory does not count duplicate semantic guards or prove dispatch is the runtime bottleneck; measure both before implementing a pass. Preserve route order and duplicate deliveries. Any power-link deduplication must prove identical max-input semantics, including source, channel, target, guard, face and attenuation. Do not merge shared resources solely because their material profiles match.

The first optimizer experiment should therefore be a measured sharing/index audit over the complete runtime program. Candidate low-risk work is exact guard-expression interning or route lookup indexing only where complete records and ordered delivery behavior are preserved. The graph remains the reference until a before/after differential replay proves the change.

## How a near-total adder optimization could be safe

The measured outputs establish a useful candidate contract: operands are prepared before one falling trigger, the declared output bus is sampled in fixed windows, and the 11-bit fixture exposes only the low eleven sum bits (there is no separate carry output port). Under an explicit host-controlled contract that keeps operands stable, permits no second trigger or external edit during the operation, and proves no observer or outside callback can see internal stages, a timed arithmetic transition could replace most internal bit slices while retaining the same publication windows.

Those assumptions are not true of the general executor. The tests also change an input after an accepted action, reverse operand order, cancel within one tick, hold controls, rearm and issue repeated triggers. A schematic-name or topology match alone cannot select the arithmetic shortcut. Enforce the restricted contract at the host boundary and use the specialized path only while it holds; otherwise execute the general graph. The specialized path would still need to reproduce acceptance, exposed callbacks, reset continuation and output timing that cross its certified boundary.

No optimization was implemented or timed in Step 12. The next code change should follow the reviewed, legality-focused sequence in [Side B](./side-b-basic-passes.md); any adder-specific transition needs a separate reviewed step and a checked runtime contract.
