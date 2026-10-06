# Redpiler for Pistons

This file defines the piston feature scope. The [runtime documentation](INSTANT_PISTON_RUNTIME.md) describes the implemented compiler; the broader model and roadmap distinguish current support from proposed protocols.

**Implemented scope (2026-10-06):** normal `/rp compile` accepts supported ready instant networks, including the lever/repeater `ADDER_1BIT.schem`, `ADDER_11BITS.schem` and `COUNTER_BASIC.schem`. Ordinary output consumers can connect through moving redstone-block/wool/concrete/stone/sandstone/quartz/smooth-quartz geometry; they do not require an output piston. Conditional geometry becomes a bounded Boolean program; the counter adds stored-bit decisions and one owned observer clock. Ordinary repeaters retain their delays and reset pulses. Internal pistons and dust remain visually frozen while compiled. Unsupported owners or interfaces reject the whole compilation and preserve interpreter ownership. Standalone BUD adapters, additional reset families, clear/stop/restart and general multi-clock execution remain future work. See the [actual pipeline and manual tests](INSTANT_PISTON_RUNTIME.md) and [milestone roadmap](INSTANT_PISTON_IMPLEMENTATION_PLAN.md).

The [FPU and RILAX research](FPU_RILAX_REDPILER_RESEARCH.md) led to narrow admission fixes: quartz conductors, matching stationary furnace reset-support inventories, fixed comparator main overrides and aligned payload diagnostics. RILAX supplies independent data/update and sampled-read acceptance cases before ANPU; FPU still exposes joint reset ownership and scaling limits. Both whole builds currently reject compilation. The report lists exact locations for the missing RILAX decoder torch and author-confirmed broken FPU geometry. No FPU numerical output is certified.

Pistons combine electrical power, independently delivered updates and physical movement. Redpiler's computational abstraction must assign an owner and a validated protocol to each relevant mechanism.

Possible piston roles in a circuit are:

- Instant Piston
- Memory (BUD Switch)
- Other (non-Instant Piston)

These roles can coexist in complex builds such as PM1 or ANPU. Initial graph acceptance focuses on gates, adders and counters, with decoders and long wires as additional targets. ANPU Pong is now the next compiled target, with a frozen interpreter memory/screen oracle; BUD/update-channel CPU graph lowering remains pending. After the FPU/RILAX admission fixes, ANPU still rejects at all tested rank budgets: 1× exhausts analysis, while 2×/4×/8× reach an ordinary piston outside the owned generator protocol. Its 50,000-tick interpreted memory/screen replay remains unchanged. See [the measured failures](ANPU_REDPILER.md#admission-after-the-fpurilax-legalization).

- Instant Piston is defined as a piston with a reset circuit. Reset allows the mechanism to return to a reusable extended configuration after retracting. The first computation wave can propagate through multiple stages in the same game tick while physical reset takes subsequent ticks. There is an enormous number of possible reset constructions, but a basic one is:
  - Side-facing piston, observer with its red dot up (`facing=Down`, watching the base), solid block on the observer. After powering and depowering, this circuit can periodically retract and extend in a predictable way.
  - A payload can emit power, conduct another source or change a connection to the next instant. Composing certified first-wave functions permits combinational computation, while reset and external adapters can remain stateful. This does not imply that arbitrary physical constructions already compile.
  - We can define OR and AND gate:
    - Shared-payload OR removes the initial far output supply when either owner fires: `y = e_A OR e_B`. Subsequent oscillation or rearming depends on the reset protocol.
    - Independent supplies on one output net yield falling-event AND: the initially powered net falls to zero only when both supplies disappear in the declared wave. Electrical contributions still join by maximum strength.
    - Negation is an inhibition circuit that blocks another piston when its input is active. It is not a typical standalone NOT gate. Its required update order must be established from the circuit; the author specifies that the "negated piston must fire first, update-wise."
    - Logical one is a transition from nonzero power to zero. The low state enables downstream instants to retract and schedule their update cycles. A held zero is not a fresh external trigger; internal reset-generated cycles remain part of the triggered behavior.
  - PM1 is a CPU research fixture with instant logic, storage and timed control. Its whole execution is not a pure combinational function. The [CPU references](../test_data/cpu-references/README.md) freeze interpreted behavior without certifying a complete compiled model.
  - ANPU also combines ordinary logic and piston mechanisms with BUD memory. It is the next compiled acceptance target: interpreter memory/screen references are frozen; BUD/update-channel graph lowering remains pending and unsupported compilation rejects. See [ANPU milestones](ANPU_REDPILER.md).
  - Shared-output OR groups may drop the output block, and either piston may recapture it; ownership can transfer. Recognition must validate the complete group's output and reset behavior. Other families require their own payload and reset rules rather than assuming that every candidate always resets properly.
- A BUD switch stores a settled piston/payload state because its live data power and qualifying recheck can arrive through different paths. A data change alone need not update the stored bit. A notification samples live power; any requested movement still needs acceptance against the live base, power and payload. Same-value samples can occur without a write.
  - The parser must identify the data decoder, independent update path, accepted storage operation and read interface. Notifications can reach a base directly or through head/neighbor changes; they are not restricted to the cell above the piston.
  - BUD switches occur in ANPU and PM1. A schematic illustration of remote data above a base is:

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
Wire direction alone does not establish separate data and update inputs. Trace actual receiving faces, quasi-connectivity and notifications: the illustrated update wire must not unintentionally replace the intended data condition. Other constructions use piston heads, wire updates or note-block changes. The [BUD model](INSTANT_PISTON_REDPILER_MODEL.md#5-bud-memory-samples-on-an-update) and [RILAX characterization](FPU_RILAX_REDPILER_RESEARCH.md) distinguish stored data, accepted movement and retained read-gate state.

- Non-Instant Pistons are pistons that are neither Instant nor BUD switches. They retain ordinary piston behavior. Moving only one block is an initial compiler restriction; the interpreter supports longer straight payload lines.
- Validated Instant Nodes can be lowered to combinational functions of the events and prepared conditions in a computation wave. Redpiler can minimize a certified network when its behavior at non-instant consumers and its interpreter handoff remain valid.

For future experiments take a look at schematics in test_data, like [ADDER_11BITS.schem](../test_data/instant-pistons/ADDER_11BITS.schem) (instant adder), EDGECASE_PISTION.schem (Instant repeters), MCHPRS_REDSTONE_UPDATE_EDGECASE.schem
The new basic pack is in [test_data/instant-pistons](../test_data/instant-pistons). `ADDER_11BITS.schem` is the canonical corrected adder; `ADDER_11BIT.schem` is an alias of the same build. The refreshed version has six sign labels and dimensions 21 x 7 x 46. Its port maps and Java references have been updated and verified against that binary.
BUD-Switch: MemCellUnalignedNanoTicks.schem, UpdateTesterExtendInst.schem,UpdateTesterExtendNonInst.schem ect

## Clarified execution scope

The circuit author's [answers](ANSWERS.md) establish the following protocol:

- Computation is trigger-driven. The mechanism starts in a stable extended state, and removing a powering wire or circuit launches a falling transition. Each reset family needs an explicit ready condition before another computation.
- Discover outputs where instant-driven wires or update paths reach non-instant logic, including BUD switches, repeaters and lamps. A net may feed both another instant and an external consumer.
- Simplify internal movement while preserving the declared logical consumer contract. Physically synchronized episodes retain their interpreter/Java comparisons; internal nanotick misalignment may be normalized. Electrical strength, qualifying updates and declared consumer timing remain part of validation.
- New external inputs or a new computation during reset are undefined for the initial supported protocol. Internal propagation and reset work caused by the accepted trigger are still part of its behavior.
- Torch and dust resets activated by retraction are common families to prioritize alongside the observer reset. Their supplied examples must establish their geometry and update paths.
- Players should see infrequent useful output updates. Internal piston animation and exact internal wire-state rendering are unnecessary after minimization. Interpreter restoration still needs a validated reconstruction or phase-preserving handoff.
- Begin with small gates, adders, counters, decoders and long wires. PM1 and ANPU are later targets. The latest replacement adder passes the previous changed-input, saved maximum-input and high-bit overflow cases. Its Java traces verify `1365 + 682 = 2047` and `2047 + 2047 = 2046` modulo 2048.
- Physical negation requires synchronization and relative update ordering: extra actuator depth on the negating input can make inhibition arrive too late. Under the latest compiler clarification, both full-net and optimized compiled plans use ideal internal synchronization, so this physical failure alone does not exclude recognized logic. NANOTICK_EXAMPLE remains a physical diagnostic and becomes a candidate intentional-divergence test once its logical interface is certified. Independent BUD data/update semantics remain explicit; the [lever/repeater revision](INSTANT_PISTON_IO_SCHEMATICS.md) supplies standalone examples. Using fine-stepping APIs alone does not create a circuit misalignment example.

The [schematic catalog](INSTANT_PISTON_SCHEMATICS.md) and [validation report](INSTANT_PISTON_VALIDATION.md) establish current-binary characterization before recognition. OR_Interpreter_illigal is author-excluded undefined behavior: another actuator can take the powered payload needed for a dust reset, leaving a member non-instant. Its exact state and diagnostics are preserved, with no proposed compiler acceptance. The [analysis-agent prompt](INSTANT_PISTON_ANALYSIS_AGENT_PROMPT.md) defines the evidence requirements.

The full detection strategy, implementation stages, risks and requested fixtures are in [INSTANT_PISTON_IMPLEMENTATION_PLAN.md](INSTANT_PISTON_IMPLEMENTATION_PLAN.md). The physical interpreter semantics are specified separately in [REDSTONE_MODEL.md](REDSTONE_MODEL.md).
