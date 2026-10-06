1. **What represents logical one?** At an instant’s input and output, is it positive wire power, no power, enabled oscillation, or absence of the output block? Does the convention differ between circuits?
   Logical one is an signal that changes from non-zero value to zero. This low state can enable next instants to trigger the same bahavior by schleduling the update cycles. 
2. **What exactly is the computational output?** Should we read the output block’s position during the first retraction wave, sample dust at a particular point, or determine whether the output keeps oscillating?
   It is hard to answer. We will need to detect where wire that is driven by instant circuit is connected to non-instant logic, in these cases we can assume it is the output (for example output to BUD-switch, to repeater, lamp or any other)
3. **How much physical timing must compilation preserve?** Must exposed signals match every game tick, including reset pulses, or can we simplify internal movements provided computation results and their availability remain correct?
   We can simplify if possible and it will retain the same behavior.
4. **Are inputs continuously variable or trigger-driven?** Can data change at any time, or must inputs be prepared before releasing a clock/inhibit signal? What role does `TICK` play in the adder?
   Impulses are always trigger-driven - Instant circuit starts in stable, extended state and it gets triggred by removing an wire/circuit in given places. 
5. **What should happen during reset?** If inputs change or another computation starts before reset finishes, is that valid operation? Is there a minimum interval between computations?
   Let's assume it is undefined behavior. 
6. **Where are the smallest OR, AND and NOT examples?** Existing schematic names plus selection-local coordinates would be ideal. For each, identify its inputs, output block or net, and expected truth table.
   There are not yet created, you want me to create few? 
7. **For shared-block OR gates, what block movement is allowed?** Can either piston temporarily drop the payload? Where must it return, and can ownership transfer between the pistons?
   Exacly, any pistion can drop the block and it does not matter. ownership can be transfered. 
8. **Which alternate reset constructions matter most?** Besides the observer-above design, which families are common in PM1? Small examples of downward-facing instants would be especially useful.
   There is enormus amount of reset circuits. Most common are with redstone torch, redstone dust that get's triggered when piston is retracted. I will provide an examples.
9. **Can you label the ambiguous fixtures?** In the four `UpdateTester` schematics and `MemCellUnalignedNanoTicks`, which pistons are intended as instants, BUD memory, or ordinary pistons? Which wire delivers the update independently of power?
   I will provide an example for you
10. **What is the first acceptance target?** Individual gates, the existing adder, a PM1 subsystem, full PM1, or ANPU?
   I think that PM1 and ANPU are too ambitions projects for now, too complicated. We will first focus on simpler builds: Adders, counters, gate examples, decoders, long wires ect, later we will get into bigger projects like ANPU.
11. **What should players see while compiled?** Must internal pistons animate and output blocks move visibly, or is displaying computational outputs sufficient? When compilation stops, must the exact physical reset phase be restored?
   It should probably show updates infrequently, for sure not animated pistions or even wire states. After optimalization it will be hard to find what wire is what (circuit can be minimized and lost some connections)
12. **Is the adder’s changed-input limitation expected?** The historical Java reference produced `1087` for the `0x555 + 0x2aa` setup. Was that an intended operating restriction, an initialization problem, or a circuit defect?
   I will invesigate.

   Refreshed replacement fixture verification (2026-10-06): [ADDER_11BITS.schem](../test_data/instant-pistons/ADDER_11BITS.schem) produces the correct `2047` for `0x555 + 0x2aa` in both Java and MCHPRS. It also fixes the preceding replacement's high-bit cases: saved `2047 + 2047` inputs produce `2046`, and prepared `1024 + 1024` inputs produce `0`, both modulo 2048. The replacement Java traces and arithmetic tests use the refreshed binary.
