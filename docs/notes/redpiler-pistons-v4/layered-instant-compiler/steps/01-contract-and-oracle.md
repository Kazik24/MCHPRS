# Step 01: establish the wave contract and independent oracle

Status: planned. Read [architecture](../ARCHITECTURE.md) and [migration](../MIGRATION.md).
Implement this step only, then stop for human review. No compiler/pipeline activation.

## Goal

Establish exactly what one atomic input frame, one computation use, one reset and one safe
observation mean for the actual fixtures. The single-use rule must be evidence-backed
before it becomes the algorithm's termination argument.

## Work

1. Reuse `backend/graph/tests/adder.rs`, current manifest loaders and native reference-contract
   recorders. Do not create another schematic loader or gate simulator as the oracle.
2. Add `backend/graph/tests/instant_contract.rs`, registered from the existing graph test tree.
   Represent a frame with old/new boundary data, the qualifying input event(s), and their
   declared order/provenance. Explain every field. Keep the adapter test-only initially.
3. For real adder cases, use physical data preparation followed by the fixture activation
   edge. Sample all data at that edge. Keep sequential preparation events visible to both
   executors; do not label them a simultaneous atomic frame merely because no tick elapsed.
4. Add an explicit synthetic multi-source frame protocol if needed: apply all source values
   first and release declared notifications afterward in the same way for native and Graph.
   Label this as a new contract-specific input protocol, not ordinary Minecraft interaction.
5. Decode native observations into accepted computation uses, reset/restoration uses and
   boundary effects. Annotate actors by their mechanism role, not by an arbitrary event count.
   A no-change sample or cancelled request does not consume an actor.
6. Census repeat uses, shared transfers, reset-generated responses and observation windows.
   Define the earliest supported local cut for a wave. Do not use a whole-world idle test.
7. Compare the known `prepared-0-1-1` adder case under the declared stimulus protocol. Record
   whether a mismatch concerns source preparation, internal causality, or an actual output.
8. Establish old/new data, output levels/pulses, external BUD memory and scheduled deadlines
   as the comparison projection. Raw internal wires/heads during an admitted wave are not
   mandatory equivalence roots; reconstruct and compare them at the safe cut where promised.

## Fixtures and required checks

- `INSTANT_BLOCKED`: failed requests do not count as computation use.
- `INSTANT_CHAIN`: causal progression is not one game tick per piston.
- `INSTANT_RESET_REDSTONE` and `_2`: distinguish reset from computation and repeated waves.
- Shared OR and AND fixtures: accepted uses, not samples, determine the logical result.
- Independent BUD data/update fixture: data alone cannot write memory.
- ADDER1BIT all pinned cases; census ADDER11BIT without synthesizing it.
- Include a sub-tick repeater pulse and an external observer of a head/contact.

Reuse the `instant-pistons-io/fixtures/*.json` manifests and hashes. Use the equivalent
`test-prefix-20261008` family to expose different geometries; do not change saved schematics.
The historical actor expectations are 13 and 142 for the pinned adders; verify them from
the current loaded fixture rather than accepting an old report as fresh evidence.

## Review artifact

Provide per-fixture wave boundaries, accepted computation counts by actor, reset roles,
boundary observations and the smallest counterexample to any proposed assumption.
Append edge cases to the single central register. A short status note may link the table;
do not create a second counterexample catalog.

## Gate

The input protocol and consumed-use definition are unambiguous and executable in tests.
If a required actor fires twice in one intended wave or has unmodelled retained memory,
report that fact before step 02. Do not erase it or split waves without a justified cut.
No speed result is expected from this step.
