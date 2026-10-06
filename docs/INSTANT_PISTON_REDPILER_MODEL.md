# Instant pistons and BUD switches: a Redpiler model

This is a compact mathematical contract for recognizing and simplifying instant circuits. Physical power, callbacks and piston execution follow [REDSTONE_MODEL.md](REDSTONE_MODEL.md). Geometry and measured episodes belong in the [schematic catalog](INSTANT_PISTON_SCHEMATICS.md). This document defines the abstraction between them and Redpiler.

The intended interface uses ordinary Redpiler nodes: levers, buttons, torches and wires supply stable signals. An instant circuit becomes a subassembly or node in the larger graph, with its own input and output ports. Source destruction in the characterization fixtures is an experimental stimulus. A compiled circuit should accept signal changes through its existing input nodes.

**Status (2026-10-06):** this contract is partially implemented. Redpiler executes the lever/repeater 1-bit and 11-bit adders, conditional electrical ports for moving conductors, and a shared-clock BUD network supporting `COUNTER_BASIC`, through Boolean programs paired with ordinary graph nodes. Counter next-state expressions use stored bits; an owned observer protocol supplies sampling and output timing. Standalone BUD adapters, multiple clocks and general reset/consumer protocols remain proposed. The [implemented pipeline](INSTANT_PISTON_RUNTIME.md) defines current admission, output timing, handoff and validation; the wider mathematical model below does not imply that every described family can compile.

ANPU is the next [compiled acceptance target](ANPU_REDPILER.md). Its interpreter sampling/write projection and screen trace are retained as reference data; the general BUD graph abstraction remains pending. Its 896 identified note-block BUD cells carry conducting black concrete and require independent ordered update channels. Unsupported ANPU programs reject compilation; there is no physical compatibility backend.

**Known conformance gap:** `XOR_Simple` can currently pass compilation, but releasing both inputs exposes a reset-wave repeater mismatch at tick 8. Its first-wave formula does not certify the complete adapter response. The [runtime](INSTANT_PISTON_RUNTIME.md#conditional-output-contract) retains the explicit ignored regression; the preservation equation below is a required contract, not a claim that this admitted case already satisfies it.

[RILAX bank characterization](FPU_RILAX_REDPILER_RESEARCH.md) now supplies a smaller precursor: distinct data power and head-notification channels, samples on both ordinary-piston movement edges, unchanged samples without writes, and read-gate state that retains an earlier read across a later write. A direct combinational read of stored bits would lose this sampled behavior. Its physical data/output levels use powered = one; that fixture decoder does not redefine the instant logical-one falling event. The same report separates FPU compiler limitations from author-confirmed broken saved geometry and leaves its custom arithmetic outside acceptance.

The compiler now admits quartz/smooth-quartz conductors and matching stationary furnace reset-support inventories. A comparator's direct fixed rear override stays an ordinary analog input even when conditional geometry powers that support; comparator sides retain their separate electrical rules. This does not admit transported entities, dynamic rear override reads, unknown reset writers or broken geometry. The report lists the exact manual repairs and remaining FPU/RILAX protocol boundaries.

## 1. Circuit boundaries

The parser identifies a region of interacting pistons, their payloads, reset circuits and internal connections. Its interface is derived from connections to the surrounding graph:

```mermaid
flowchart LR
  S[Lever or button] --> N[Ordinary torch / wire nodes]
  N --> I[Instant subassembly A]
  I --> J[Instant subassembly B]
  J --> C[Lamp / repeater / other consumer]
  I --> U[BUD update input]
  N --> D[BUD data input]
  D --> M[BUD memory]
  U --> M
  M --> J
```

An input port is a connection into the region, with a declared role: prepared data, trigger, inhibit, or update. It may come from an ordinary node or another instant subassembly. One connection can have several roles. An output port is a connection from the region to another graph node; it can feed another instant, a BUD, or an ordinary consumer. Follow the actual paths; sign labels help characterize fixtures but are not the parser interface.

**An instant output port is not Redpiler's world-output flag.** Intermediate event/value connections remain internal graph edges. Discovering a lamp or other non-instant consumer establishes a boundary where the logical result must be converted into that consumer's input semantics. Adjacent instant subassemblies can instead be connected or merged and simplified together.

Region ports refer to node identities and retained connection semantics. A torch or wire can be a region input without being a player-operated input. Wire elimination must preserve required port identities or aliases. A non-instant piston needs an execution owner before compiled execution can be enabled.

## 2. Levels, events and stored bits

Keep these quantities distinct:

| Symbol | Meaning |
| --- | --- |
| $s_i(\tau)\in\{0,\ldots,15\}$ | Electrical strength at an input connection |
| $d_i\in\{0,1\}$ | Prepared data, decoded from a stable input condition |
| $e_i\in\{0,1\}$ | A falling event in the declared computation wave |
| $q_j\in\{0,1\}$ | A stored BUD bit, read at a valid observation point |
| $r_j\in\{0,1\}$ | Retained state of a sampled read gate or other stateful output adapter |

Here $\tau$ identifies a boundary transition or observation point, not an internal nanotick. Define a falling event by

$$
e_i(\tau)=[s_i(\tau^-) > 0\ \land\ s_i(\tau)=0].
$$

Logical one at an instant event port means this transition. Held zero produces no new external event; positive-to-positive changes also produce no falling event. Those changes can still affect electrical strength or callbacks and therefore cannot be discarded without checking their consumers. A zero in an event truth table means no event in that wave, whose observation point must be specified.

Prepared data has a separate decoder $d_i=D_i(s_i,\text{connection state})$. Its polarity comes from the recognized circuit. Do not automatically identify a prepared bit with a falling event.

For example, let $x$ be a stable lever/button state and $\ell=[s>0]$ the output of its negating torch. At settled levels $\ell=\neg x$, so activating the control produces

$$
e=\ell^-\land\neg\ell^+=\neg x^-\land x^+.
$$

The event occurs at the actual torch output transition, with normal torch timing. A button's release and pulse duration also belong to the input protocol. Replacing a removed source with this input adapter requires checking power and notification paths; matching a final level alone does not establish equivalence.

## 3. Instant computation

For a supported computation wave $k$, a certified instant region has the logical projection

$$
\mathbf y_k=F(\mathbf d_k,\mathbf e_k,\mathbf q_k,\mathbf r_k).
$$

$\mathbf d_k$ is the prepared data, $\mathbf e_k$ identifies the accepted triggering events, $\mathbf q_k$ is the data memory visible to this computation, and $\mathbf r_k$ is any retained read-adapter state. The latter can instead be included in a single larger storage vector; separating it prevents a mistaken combinational read assumption. An instant-only computation has no persistent data memory, but reset/clock and consumer adapters may still require state. A stateless region omits both storage arguments.

The formula applies under the region's protocol, ideal internal synchronization and output decoder. Physical synchronization is a compatibility property, not a prerequisite for evaluating the logical function. Wave grouping belongs to the protocol; arbitrary changes are not combined just because they occur near each other. Examples from the characterized first wave are:

| Mechanism | Logical projection | Evidence |
| --- | --- | --- |
| Shared-payload OR | $y=e_A\lor e_B$ | [OR_1](INSTANT_PISTON_SCHEMATICS.md#or-1) |
| Independent sources on one output net | $y=e_A\land e_B$ | [AND_1](INSTANT_PISTON_SCHEMATICS.md#and-1) |
| Activation with effective inhibition | $y=e_A\land\neg e_B$ | [NOT_1](INSTANT_PISTON_SCHEMATICS.md#not-1), under its synchronized consumer contract |
| Prepared one-bit addition | $S=A\oplus B\oplus C$, $C_{out}=AB\lor AC\lor BC$ | [ADDER_1BIT](INSTANT_PISTON_SCHEMATICS.md#adder-1bit), after its separate trigger |

The AND interpretation follows the event encoding: if both initially positive contributors feed an electrical maximum, the output becomes zero only when both contributors are removed. An electrical join itself remains a maximum of strengths. An internal event edge carries $y_k$ once for wave $k$; repeatedly reading a held value does not emit new events.

These formulas describe the declared result projection. They do not certify every intermediate wire transition or the subsequent reset response. In particular, [XOR_Simple](INSTANT_PISTON_SCHEMATICS.md#xor-simple) has a first-wave XOR projection and additional reset effects.

Shared-output OR is one region with a shared payload, rather than two independently owned output blocks. Ownership may transfer or a payload may be dropped and recaptured. The region must preserve its permitted payload states and have a valid reset for every supported ownership outcome. [OR_Interpreter_illigal](INSTANT_PISTON_SCHEMATICS.md#or-interpreter-illigal) fails that reset requirement and is author-excluded from compiler scope.

## 4. Inhibition and ideal internal synchronization

Negation is inhibition of another activation. Let $i_k$ be the effective inhibit bit for a supported wave. Then

$$
y_k=e_{T,k}\land\neg i_k.
$$

**Redpiler does not preserve internal nanoticks. It evaluates recognized instant logic with ideal internal synchronization, even when the physical circuit is misaligned in Java/interpreter execution.** It evaluates the logical function from the finalized wave's inputs, without reproducing actuator delays, callback traversal, event FIFO or internal piston movement order. This applies to both the full-net reference plan and the optimized plan; minimization must preserve the same logical semantics.

In physical execution, correct synchronization makes inhibition effective before a receiving operation accepts an activation that it should suppress; equal-game-tick firing is allowed. In compiled execution, the inhibit bit participates directly in the same logical evaluation, so extra internal actuator depth cannot make it arrive too late. Delays in ordinary components and distinct declared sampling transactions remain meaningful boundaries.

The [measured order relations](INSTANT_PISTON_SCHEMATICS.md#negation-partial-orders-and-compiler-boundary) document physical compatibility for specific episodes. The compiled result still needs a declared consumer contract: NOT_1 has a physical raw-wire transient that its tested repeater probe filters. A BUD or observer cannot automatically be assigned that same adapter. Physical transients caused solely by internal misalignment may be omitted under the compiled contract.

[NANOTICK_EXAMPLE](INSTANT_PISTON_SCHEMATICS.md#nanotick-example) demonstrates delayed physical inhibition. Under the author's revised compiler contract, misalignment alone does not exclude its recognized logical function: compilation should suppress the activation that the intended inhibit expression forbids. Preserve the physical diagnostic and add a separately established logical expectation when its ports/protocol are certified. This is an intentional difference from physical execution, not a claim of implemented support. Invalid reset or payload ownership remains a separate exclusion.

## 5. BUD memory samples on an update

A BUD has two semantic inputs: a live data/power condition and a qualifying update. Changing the data need not deliver that update. This separation is the source of storage.

Distinguish a delivered sampling notification $\nu_j$, a requested movement and an accepted storage transaction $u_j$. A notification samples current power even if no movement is needed. A request can later be rejected by the live power, base state or payload check. The physical rules are [sections 13.2–13.3](REDSTONE_MODEL.md#132-requests-are-not-scheduled-block-ticks); the [ANPU oracle](ANPU_REDPILER.md#observable-contract) retains samples separately from accepted movements.

For a certified logical transaction, let $u_j=1$ mean that its update has passed the family's acceptance rules, and let $d_j$ be the value decoded at that accepted sample boundary. The settled storage rule is

$$
q_j^+=
\begin{cases}
d_j,&u_j=1,\\
q_j,&u_j=0.
\end{cases}
$$

Equivalently, $q_j^+=(u_j\land d_j)\lor(\neg u_j\land q_j)$. The cell can store either bit; this is not restricted to falling data events. A same-value accepted sample gives $q_j^+=q_j$ and need not produce a movement. Its sampling record remains significant. Conversely, $\nu_j=1$ does not justify setting $u_j=1$ for an unvalidated queued movement. A rejected request leaves storage unchanged. Under a proven stable-data, unblocked protocol the sampling notification and logical storage update can be collapsed; that is a family-specific proof.

This is a target contract for certified BUD families, implemented only for the current owned Counter bank. Recognition must establish the data decoder, qualifying update, event acceptance and state visibility. A future general adapter must perform validated transactions directly without replaying callbacks or movement. A moving physical cell is not a valid stationary bit observation. Multiple logical updates remain separate transactions unless the protocol proves an atomic batch. Their ordering and live data dependencies are not erased by ignoring internal nanoticks.

The decoder establishes polarity. For a cell with stationary extended state $m$, a possible convention is $q=\neg m$; an accepted, unblocked transaction that settles extension to physical power $b$ then stores $d=\neg b$. A notification alone does not promise that settled state. COUNTER_BASIC uses retracted=one, extended=zero. Other recognized families must declare their own decoder.

[AND_3](INSTANT_PISTON_SCHEMATICS.md#and-3) demonstrates why both inputs matter: a remote QC power change can leave the base unrechecked. Removing remote B before callback-generating A permits retraction; reversing those actions leaves the base extended. The final removed-source set is identical, but the sampled history differs.

Memory remains explicit when simplifying the surrounding instant logic. A region that reads and writes BUD state must define which stored state a computation reads; $\mathbf q_k$ in $F$ means that state. Feedback through stored state is permitted. Feedback without a memory or clock owner requires a separate temporal model. These are logical dependencies, not an internal nanotick schedule.

### Sampled read state

RILAX's lower concrete read gates retain their own geometry. Its output therefore has the form $O=\mathcal A(\mathbf q,\mathbf r,c,\mathbf s)$, where $c$ includes read enable/closure timing; it is not generally `data_bank[selected_address]` whenever read enable is high. In the measured held-read case, an initial zero read remains zero after the selected data word becomes `0xff`; a fresh read cycle publishes `0xff`. Read closure also has its measured delay before outputs return to zero. This establishes a need for retained read state, not a complete specification for every overlapping read/write history.

The retained variable describes gate state, not a proven independent latch for an arbitrary eight-bit output word: $\mathbf q$ and ordinary strengths remain arguments of the read adapter. Reverse/mixed-bit writes during held reads, held-read address changes and shorter preparation intervals require additional cases before choosing a general read transition function.

Both movement edges of RILAX's ordinary update pistons can sample the upper bank. A held enable is not continuous sampling, and releasing it can sample data prepared while it was held. The decoder determines which pulses reach those edges. These measured operations are distinct from a universal one-write-per-rising-edge RAM model and remain future compiled-family work.

In the measured prepared-`0xff` protocol, enable widths one and two game ticks cause no updater movement or write; widths four, six, eight and twelve do write. Width three is unmeasured. Ordinary pulse filtering is part of the adapter contract; a raw enable edge cannot be substituted for the resulting sampling notification.

### Analog electrical adapters

For a supported electrical consumer channel, let $g(\tau)$ be certified geometry state, $\gamma_t(g)$ the guard for contribution $t$, $a_t$ its attenuation and $s_t(\tau)$ its ordinary source strength. Then

$$
s_{\mathrm{consumer}}(\tau)=\max\left(\{s_t(\tau)\mathbin{\dotminus}a_t:\gamma_t(g(\tau))=1\}\cup\{0\}\right).
$$

Here $x\mathbin{\dotminus}a=\max(0,x-a)$. A redstone-block contribution has $s_t=15$; wool, concrete, stone, sandstone, quartz and smooth quartz conduct permitted sources but do not supply their own fifteen. A fixed contributor can keep the output powered when a mobile contribution disappears.

A comparator's direct stationary rear override is the separate input rule $I=V(\text{rear inventory/state})$, irrespective of electrical power through that rear block. Its side channel accepts dust, diodes and redstone blocks, not general solid conduction. Neither channel is universally Boolean. Fixed furnace inventory strengths remain ordinary graph inputs; changing rear geometry or transporting an override requires a different protocol. The [physical comparator equations](REDSTONE_MODEL.md#103-rear-input-and-far-override) also distinguish a far override and its strength-fifteen exception.

## 6. Protocol, validity and reset

Attach a protocol $\mathcal P$ to each recognized region. It specifies logical wave grouping, ready condition, preparation, accepted triggers, observation windows, reset/clock response and next-use condition. Record physical synchronization/compatibility separately; internal misalignment is normalized by compiled evaluation.

For the currently admitted instant-wave adapter, a launch requires powered, extended mechanisms, permitted payload arrangements, reset state and relevant queued work. Its prepared inputs remain stable through the supported computation/reset episode; new external data or another computation during that reset is outside initial conformance. This is not a universal initialization rule for memory: a BUD protocol may admit either settled storage state, including a state that differs from live power. RILAX and ANPU preserve that history. Each family's protocol determines which inputs must stay stable through which sampling/acceptance interval.

A logical result is published with validity:

$$
(v(\tau),y(\tau)),\qquad
y(\tau)=F(\mathbf d,\mathbf e,\mathbf q,\mathbf r)\text{ when }v(\tau)=1.
$$

An invalid observation is $\bot$, not Boolean zero. The corrected original [11-bit adder](INSTANT_PISTON_SCHEMATICS.md#adder-11bits) illustrates this at its raw payload/net ports: its first result decodes at response boundaries 1..3, moving payload observations at 4..5 are invalid, and the physical reset at 6 is not a new arithmetic result. This is not the repeater output window: the [lever/repeater revision](INSTANT_PISTON_RUNTIME.md#wave-execution-and-output-timing) exposes sum at ticks 3..7 and one-bit carry at 5..9. Validity is per declared port; internal movement does not make every ordinary electrical consumer invalid.

Reset can return to a reusable ready state, require external rearm, or maintain a recurring episode. Therefore there is no universal reset delay or rule that returning to extended means ready. Held-zero observer examples continue cycling; internally generated events remain part of the original accepted response. A compiler must retain their effects at exposed consumers even though the external falling event occurred only once.

A counter consequently has memory **and** generator/reset state. The original [COUNTER_BASIC characterization](INSTANT_PISTON_SCHEMATICS.md#counter-basic) measured counts 1..16 at boundaries $6n$. The [implemented I/O revision](INSTANT_PISTON_RUNTIME.md#clocked-storage-and-the-counter) now also has exhaustive next-state checks and a full 65,536-increment compiled/interpreter comparison, including high carries and wrap. This additional Rust evidence does not extend the existing Java captures or establish clear and restart protocols.

## 7. Minimal compiler contract

Represent a recognized region by

$$
\mathcal R=(F,H,\mathcal P,\mathcal A).
$$

| Component | Responsibility |
| --- | --- |
| $F$ | Logical computation within a certified wave |
| $H$ | Validated storage transactions and retained read/reset/clock/acceptance state transitions |
| $\mathcal P$ | Supported logical input histories, wave grouping, readiness and observation validity |
| $\mathcal A$ | Port adapters: internal event/value connections and electrical strengths, transitions, updates and relevant timing at ordinary consumers |

With retained state $z=(\mathbf q,\mathbf r,c)$, where $c$ includes required reset/clock/read lifecycle and acceptance state, let $\sigma_k$ be the **ordered** protocol actions for step $k$. It retains qualifying sample boundaries and their data, acceptance/cancellation decisions, required adapter deadlines and declared atomic-batch membership. Same-value samples remain observable when the interface requires them. These are logical records owned by the adapter, not a replay of every physical callback. The transition is

$$
z_{k+1}=H(z_k,\mathbf d_k,\mathbf e_k,\sigma_k).
$$

Logical steps are accepted waves, memory transactions or required adapter deadlines. They are not individual piston callbacks. Multiple transactions on the same cell must not become an unordered update bit. For the admitted Counter, all next-state expressions read the same old bank and then commit together in phase 3; evaluating one bit after overwriting a preceding bit would change its function. A general BUD family needs its own ordered or atomic protocol. For a pure combinational region with stateless adapters, $z$ is empty.

$F$ is eligible for Boolean minimization and composition into a larger graph. $H$ can be omitted only when boundary behavior needs no retained storage, read-adapter, reset, acceptance or generator state. It preserves logical state dependencies and the external timing required by $\mathcal P$, with no internal nanotick model. Let $\operatorname{Logical}(\mathcal R,h)$ be execution of the extracted logical circuit with ideal internal synchronization. A successful simplification preserves that reference semantics:

$$
\forall h\in\mathcal P:\quad
\operatorname{Obs}_{boundary}(\operatorname{Logical}(\mathcal R,h))
=\operatorname{Obs}_{boundary}(\operatorname{Compile}(\mathcal R,h)).
$$

$h$ is an ordered input history from an equivalent protocol entry state, including decoded storage and retained adapter state. The observation includes declared consumer states, required sample records, accepted writes and relevant timing throughout the accepted episode, including reset. ANPU's proposed interface explicitly retains same-value samples and accepted-movement order as well as every game-tick screen state. A no-op logical sample must not be counted as an accepted physical movement. For physically synchronized compatible histories, the physical interpreter must agree with this projection as well. For a circuit failing solely because of internal nanotick misalignment, physical and compiled outputs may intentionally differ; the compiled result follows the established logical function. Internal animation and internal wire display can be omitted. Rendering frequency does not weaken the logical contract.

The parser must establish the port decoders, reset/payload closure, logical protocol and consumer adapters before activation. Recognition can rely on validated families; physical synchronization is not an admission requirement and no nanotick analysis or simulation is required in the compiled evaluator. If the logical contract is unknown, report the candidate and keep interpreted execution. Restoring an interpreter world also requires an equivalent materialized state with appropriate pending work; a final output bit is insufficient. A physically misaligned circuit resumes its physical semantics after handoff, so continuing agreement with compiled logical execution is not promised.

Existing [graph node types](../crates/core/src/redpiler/compile_graph.rs) provide ordinary input/output components and mobile aliases. [Node discovery](../crates/core/src/redpiler/passes/identify_nodes.rs) uses ownership boundaries supplied by prepared instant programs; the executable Boolean program and clock/storage state live beside the graph. Power links do not stand in for independent BUD updates: the shared-clock adapter explicitly supplies sampling. Region ports remain distinct from world-I/O flags. Broader independent sampling and clock composition remain requirements in the [plan](INSTANT_PISTON_IMPLEMENTATION_PLAN.md).

The [separate lever/repeater revision](INSTANT_PISTON_IO_SCHEMATICS.md) establishes physical ordinary input adapters and standalone BUD data/update sampling, retained state and quiet resampling for two families. Observer-generated updates also demonstrate data-before-sample synchronization. One BUD selection still lacks its saved update control, and XOR reset response depends on the recorded spatial/order context. Selected adder/counter handoff passes continuation tests; general consumer adapters, unrestricted reuse and broader lifecycle equivalence remain unverified. The supplied nanotick example is a candidate logical normalization case; the illegal-reset counterexample remains excluded. Earlier catalog admission statements record the prior policy; frozen physical observations remain unchanged. [ANPU's current admission and 50,000-tick memory/screen checks](ANPU_REDPILER.md) are now measured; general CPU graph lowering remains pending.
