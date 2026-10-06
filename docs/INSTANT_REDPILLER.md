# Redpiler for Pistons

This file describes the scope of adding the pistons to the Redpiler redstone compilator. The acual implementaion will depend and is free to chose.

Pistons are very complicated and have very complext behavior in game, they can be used in varity of scenarios, but basic usage for redpiler will be for computation.

Let piston be an entity that performs an task in circut. Possible tasks are:

- Instant Piston
- Memory (BUD Switch)
- Other (non-Instant Piston)

These tasks are very diffrent from eachother and all of them are required for redpiler to corretly parse and analize complex builds like PM1 or ANPU.

- Instant Pistion is defined as an piston with an reset circuit. Reset allows the piston to return to the extended state immidietly after retracting. There is enormus amount of possible ways of building one but basic one is
  - Side facing piston, Observer facing up (red dot up), solid block on observer. This circuit after poweing and depowering will periodily retract and extend in predictable way.
  - This behavior allows to define any combinatrony circuit by leverging the fact that we can attach en block on the end of the piston (let's name it output block). This block will generate an signal that can power next instants
  - We can define OR and AND gate:
    - OR gates are two instant pistons connected to same output block. If either piston is unlocked it the output will start the oscilations
    - AND gates are two instant pistons with output blocks connected to one output net.
    - NOT gate can be defined as output block that passes logical (1) to the output net (so by retracting two nets merge and cause the output net to become logical (1))
    - Note that Instant Reapeters are an negative logic, so signal ==0 is an zero, an >0 is an one.
  - PM_1 is fully instant CPU, it contains mostly Instant logic and is fully combinatonic.
  - ANPU is noninstant CPU, it contains BUD-switch memory
  - Instant Pistions cannot drop the block except in or gate, but they will always reset properly. Instant pistion is defined as Pistion with proper reset circuit. Reset circuit is non-trival.
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

- Non-Instant Pistions are pistions that are not Instant Nor BUD switch, they behave normally and obey standard redstone logic, except we can simplify that Pistions can move only one block.
- Parsed Instant Nodes are combinatroic, so chaning the input will cause an output note to retract in same amount of time. In Redpiller we can replace the whole Instant network with the Combinatrionic - replacment circuit, mimimized.

For future experiments take a look at schematics in test_data, like ADDER_GWIEZDNY_TEST.schem (fully instant adder), EDGECASE_PISTION.schem (Instant repeters), MCHPRS_REDSTONE_UPDATE_EDGECASE.schem
BUD-Switch: MemCellUnalignedNanoTicks.schem, UpdateTesterExtendInst.schem,UpdateTesterExtendNonInst.schem ect
