# Redpiler for Pistons

This file describes the scope of adding the pistons to the Redpiler redstone compilator. The acual implementaion will depend and is free to chose.

Pistons are very complicated and have very complext behavior in game, they can be used in varity of scenarios, but basic usage for redpiler will be for computation.

Let piston be an entity that performs an task in circut. Possible tasks are:

- Instant Piston
- Memory (BUD Switch)
- Other (non-Instant Piston)

These tasks are very diffrent from eachother and all of them will eventually be required for redpiler to correctly parse and analyze complex builds like PM1 or ANPU. Initial acceptance focuses on gates, adders, counters, decoders and long wires; full CPU support is deferred.

- Instant Piston is defined as a piston with a reset circuit. Reset allows the mechanism to return to a reusable extended configuration after retracting. The first computation wave can propagate through multiple stages in the same game tick while physical reset takes subsequent ticks. There is an enormous number of possible reset constructions, but a basic one is:
  - Side-facing piston, observer with its red dot up (`facing=Down`, watching the base), solid block on the observer. After powering and depowering, this circuit can periodically retract and extend in a predictable way.
  - This behavior allows to define any combinatrony circuit by leverging the fact that we can attach en block on the end of the piston (let's name it output block). This block will generate an signal that can power next instants
  - We can define OR and AND gate:
    - OR gates are two instant pistons connected to same output block. If either piston is unlocked it the output will start the oscilations
    - AND gates are two instant pistons with output blocks connected to one output net.
    - NOT gate can be defined as output block that passes logical (1) to the output net (so by retracting two nets merge and cause the output net to become logical (1))
    - Logical one is a transition from nonzero power to zero. The low state enables downstream instants to retract and schedule their update cycles. A held zero is not a fresh external trigger; internal reset-generated cycles remain part of the triggered behavior.
  - PM_1 is fully instant CPU, it contains mostly Instant logic and is fully combinatonic.
  - ANPU is noninstant CPU, it contains BUD-switch memory
  - Shared-output OR groups may drop the output block, and either piston may recapture it; ownership can transfer. Recognition must validate the complete group's output and reset behavior. Other families require their own payload and reset rules rather than assuming that every candidate always resets properly.
- BUD-switch (Block Update Detector) is an circuit that can detect an update above the pistion and save it in the piston state (either retracted or extended).
  - BUD switch contains some kind of update, an input and optionaly output block.
  - BUD-switches can be observed in ANPU and PM1. Example of bud switch

```
R
B
A
PU
```

Where
R - redstone
B - Block
A - Air
P - Piston
U - Update wire
Note that update wire cannot face the pistion, if this happens it will power the pistion and this will be not valid instant
There are many ways of updating the pistion, by either other pistion, wire ect

- Non-Instant Pistons are pistons that are neither Instant nor BUD switches. They retain ordinary piston behavior. Moving only one block is an initial compiler restriction; the interpreter supports longer straight payload lines.
- Validated Instant Nodes can be lowered to combinational functions of the events and prepared conditions in a computation wave. Redpiler can minimize a certified network when its behavior at non-instant consumers and its interpreter handoff remain valid.

For future experiments take a look at schematics in test_data, like ADDER_GWIEZDNY_TEST.schem (fully instant adder), EDGECASE_PISTION.schem (Instant repeters), MCHPRS_REDSTONE_UPDATE_EDGECASE.schem
BUD-Switch: MemCellUnalignedNanoTicks.schem, UpdateTesterExtendInst.schem,UpdateTesterExtendNonInst.schem ect

## Clarified execution scope

The circuit author's [answers](ANSWERS.md) establish the following protocol:

- Computation is trigger-driven. The mechanism starts in a stable extended state, and removing a powering wire or circuit launches a falling transition. Each reset family needs an explicit ready condition before another computation.
- Discover outputs where instant-driven wires or update paths reach non-instant logic, including BUD switches, repeaters and lamps. A net may feed both another instant and an external consumer.
- Simplify internal movement when the externally observed behavior is preserved. Electrical strength, update callbacks and relevant timing at those consumers remain part of validation.
- New external inputs or a new computation during reset are undefined for the initial supported protocol. Internal propagation and reset work caused by the accepted trigger are still part of its behavior.
- Torch and dust resets activated by retraction are common families to prioritize alongside the observer reset. Their supplied examples must establish their geometry and update paths.
- Players should see infrequent useful output updates. Internal piston animation and exact internal wire-state rendering are unnecessary after minimization. Interpreter restoration still needs a validated reconstruction or phase-preserving handoff.
- Begin with small gates, adders, counters, decoders and long wires. PM1 and ANPU are later targets. The existing adder's changed-input discrepancy remains under investigation.

The full detection strategy, implementation stages, risks and requested fixtures are in [INSTANT_PISTON_IMPLEMENTATION_PLAN.md](INSTANT_PISTON_IMPLEMENTATION_PLAN.md). The physical interpreter semantics are specified separately in [REDSTONE_MODEL.md](REDSTONE_MODEL.md).
