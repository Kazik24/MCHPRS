# Instant pistons and BUD switches: a Redpiler model

This is a compact mathematical contract for recognizing and simplifying instant circuits. Physical power, callbacks and piston execution follow [REDSTONE_MODEL.md](REDSTONE_MODEL.md). Geometry and measured episodes belong in the [schematic catalog](INSTANT_PISTON_SCHEMATICS.md). This document defines the abstraction between them and Redpiler.

The intended interface uses ordinary Redpiler nodes: levers, buttons, torches and wires supply stable signals. An instant circuit becomes a subassembly or node in the larger graph, with its own input and output ports. Source destruction in the characterization fixtures is an experimental stimulus. A compiled circuit should accept signal changes through its existing input nodes.

**Status:** proposed compiler contract, grounded in the [author's clarifications](ANSWERS.md) and fixture evidence. Current Redpiler has no executable instant or BUD node. Source inspection used revision `8b661d8987375f9b5cf23a8445a8c4f79d1321b6` on 2026-10-06.

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

Keep four quantities distinct:

| Symbol | Meaning |
| --- | --- |
| $s_i(\tau)\in\{0,\ldots,15\}$ | Electrical strength at an input connection |
| $d_i\in\{0,1\}$ | Prepared data, decoded from a stable input condition |
| $e_i\in\{0,1\}$ | A falling event in the declared computation wave |
| $q_j\in\{0,1\}$ | A stored BUD bit, read at a valid observation point |

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
\mathbf y_k=F(\mathbf d_k,\mathbf e_k,\mathbf q_k).
$$

$\mathbf d_k$ is the prepared data, $\mathbf e_k$ identifies the accepted triggering events, and $\mathbf q_k$ is the memory visible to this computation. An instant-only region has no persistent data memory. Its reset or clock activity can still require execution state.

The formula applies under the region's protocol, synchronization assumption and output decoder. Wave grouping belongs to the protocol; arbitrary changes are not combined just because they occur near each other. Examples from the characterized first wave are:

| Mechanism | Logical projection | Evidence |
| --- | --- | --- |
| Shared-payload OR | $y=e_A\lor e_B$ | [OR_1](INSTANT_PISTON_SCHEMATICS.md#or-1) |
| Independent sources on one output net | $y=e_A\land e_B$ | [AND_1](INSTANT_PISTON_SCHEMATICS.md#and-1) |
| Activation with effective inhibition | $y=e_A\land\neg e_B$ | [NOT_1](INSTANT_PISTON_SCHEMATICS.md#not-1), under its synchronized consumer contract |
| Prepared one-bit addition | $S=A\oplus B\oplus C$, $C_{out}=AB\lor AC\lor BC$ | [ADDER_1BIT](INSTANT_PISTON_SCHEMATICS.md#adder-1bit), after its separate trigger |

The AND interpretation follows the event encoding: if both initially positive contributors feed an electrical maximum, the output becomes zero only when both contributors are removed. An electrical join itself remains a maximum of strengths. An internal event edge carries $y_k$ once for wave $k$; repeatedly reading a held value does not emit new events.

These formulas describe the declared result projection. They do not certify every intermediate wire transition or the subsequent reset response. In particular, [XOR_Simple](INSTANT_PISTON_SCHEMATICS.md#xor-simple) has a first-wave XOR projection and additional reset effects.

Shared-output OR is one region with a shared payload, rather than two independently owned output blocks. Ownership may transfer or a payload may be dropped and recaptured. The region must preserve its permitted payload states and have a valid reset for every supported ownership outcome. [OR_Interpreter_illigal](INSTANT_PISTON_SCHEMATICS.md#or-interpreter-illigal) fails that reset requirement and is author-excluded from compiler scope.

## 4. Inhibition and the synchronization assumption

Negation is inhibition of another activation. Let $i_k$ be the effective inhibit bit for a supported wave. Then

$$
y_k=e_{T,k}\land\neg i_k.
$$

**Assume supported instant circuits are correctly synchronized in Java/interpreter execution. Redpiler does not model their internal nanoticks.** It evaluates the logical function from the wave's inputs, without reproducing actuator delays, callback traversal, event FIFO or internal piston movement order.

Physically, this assumption means inhibition becomes effective before a receiving operation accepts an activation that it should suppress. Equal-game-tick firing is allowed. This is a condition on the supported physical construction, not a runtime ordering graph for Redpiler.

The [measured order relations](INSTANT_PISTON_SCHEMATICS.md#negation-partial-orders-and-compiler-boundary) justify this assumption for specific episodes. A correct final Boolean value still needs the right consumer contract: NOT_1 has a raw-wire transient that its tested repeater probe filters. A BUD or observer cannot automatically be assumed to filter it.

[NANOTICK_EXAMPLE](INSTANT_PISTON_SCHEMATICS.md#nanotick-example) violates the synchronization assumption and remains outside this logical model. Preserve it as a physical diagnostic; supporting it is not a reason to add a nanotick simulator to Redpiler.

## 5. BUD memory samples on an update

A BUD has two semantic inputs: a live data/power condition and a qualifying update. Changing the data need not deliver that update. This separation is the source of storage.

For a recognized BUD cell, let $u_j$ mean a logical sampling update and let $d_j$ be the stable input decoded for that update. The storage rule is

$$
q_j^+=
\begin{cases}
d_j,&u_j=1,\\
q_j,&u_j=0.
\end{cases}
$$

Equivalently, $q_j^+=(u_j\land d_j)\lor(\neg u_j\land q_j)$. The cell can store either bit on an update; this is not restricted to falling data events. Repeated samples of an unchanged value retain that value.

This is a target contract for certified BUD families. The physical power and update connections must implement the declared sample under the synchronization assumption. Redpiler can then perform the storage update directly, without replaying callbacks or movement. A moving physical cell is not a valid stationary bit observation. Multiple logical updates are separate transactions; their data dependencies remain explicit.

The decoder establishes polarity. For a cell with stationary extended state $m$, a possible convention is $q=\neg m$; then sampling physical power $b$ stores $d=\neg b$. COUNTER_BASIC uses retracted=one, extended=zero. Other recognized families must declare their own decoder.

[AND_3](INSTANT_PISTON_SCHEMATICS.md#and-3) demonstrates why both inputs matter: a remote QC power change can leave the base unrechecked. Removing remote B before callback-generating A permits retraction; reversing those actions leaves the base extended. The final removed-source set is identical, but the sampled history differs.

Memory remains explicit when simplifying the surrounding instant logic. A region that reads and writes BUD state must define which stored state a computation reads; $\mathbf q_k$ in $F$ means that state. Feedback through stored state is permitted. Feedback without a memory or clock owner requires a separate temporal model. These are logical dependencies, not an internal nanotick schedule.

## 6. Protocol, validity and reset

Attach a protocol $\mathcal P$ to each recognized region. It specifies the synchronization assumption, ready condition, preparation, accepted triggers, observation windows, reset/clock response and next-use condition.

For an externally launched computation, the ready condition includes the stable extended mechanism, permitted payload arrangement, reset state and relevant queued work. Prepared inputs remain stable through the supported computation/reset episode. New external data or another computation during reset is outside initial conformance. The input adapter must make the promised protocol achievable.

A logical result is published with validity:

$$
(v(\tau),y(\tau)),\qquad
y(\tau)=F(\mathbf d,\mathbf e,\mathbf q)\text{ when }v(\tau)=1.
$$

An invalid observation is $\bot$, not Boolean zero. The corrected [11-bit adder](INSTANT_PISTON_SCHEMATICS.md#adder-11bits) illustrates this: its first result decodes at response boundaries 1..3; moving outputs at 4..5 are invalid; the physical reset at 6 is not a new arithmetic result.

Reset can return to a reusable ready state, require external rearm, or maintain a recurring episode. Therefore there is no universal reset delay or rule that returning to extended means ready. Held-zero observer examples continue cycling; internally generated events remain part of the original accepted response. A compiler must retain their effects at exposed consumers even though the external falling event occurred only once.

A counter consequently has memory **and** generator/reset state. [COUNTER_BASIC](INSTANT_PISTON_SCHEMATICS.md#counter-basic) has one external release followed by internal updates, with stored counts 1..16 measured at boundaries $6n$. This supports a bounded increment sequence, not a verified 16-bit wrap, clear or restart contract.

## 7. Minimal compiler contract

Represent a recognized region by

$$
\mathcal R=(F,H,\mathcal P,\mathcal A).
$$

| Component | Responsibility |
| --- | --- |
| $F$ | Logical computation within a certified wave |
| $H$ | Accepted BUD transactions and any reset/clock state transitions |
| $\mathcal P$ | Supported synchronized input histories, readiness and observation validity |
| $\mathcal A$ | Port adapters: internal event/value connections and electrical strengths, transitions, updates and relevant timing at ordinary consumers |

With retained state $z=(\mathbf q,c)$, where $c$ is any required reset/clock state, the transition is

$$
z_{k+1}=H(z_k,\mathbf d_k,\mathbf e_k,\mathbf u_k).
$$

Logical steps are accepted waves, memory updates or required clock deadlines. They are not individual piston callbacks. For a pure combinational region, $z$ is empty.

$F$ is eligible for Boolean minimization and composition into a larger graph. $H$ can be omitted only when boundary behavior needs no retained memory, reset or generator state. It preserves logical state dependencies and the external timing required by $\mathcal P$, with no internal nanotick model. A successful simplification preserves the consumer behavior:

$$
\forall h\in\mathcal P:\quad
\operatorname{Obs}_{boundary}(\operatorname{Interpret}(h))
=\operatorname{Obs}_{boundary}(\operatorname{Compile}(\mathcal R,h)).
$$

$h$ is an ordered input history from an equivalent ready state. The observation includes consumer states, consequential callbacks and relevant timing throughout the accepted episode, including reset. Internal animation and internal wire display can be omitted when this equality holds. Rendering frequency does not weaken simulation correctness.

The parser must establish the port decoders, reset/payload closure, supported synchronized protocol and consumer adapters before activation. Recognition can rely on validated families; no detailed nanotick analysis or simulation is required in the compiled evaluator. If the contract is unknown, report the candidate and keep interpreted execution. Restoring an interpreter world also requires an equivalent materialized state with appropriate pending work; a final output bit is insufficient.

Existing [graph node types](../crates/core/src/redpiler/compile_graph.rs) provide ordinary input/output components, but no instant/BUD execution. Existing [node discovery](../crates/core/src/redpiler/passes/identify_nodes.rs) also does not discover these region boundaries. Power links cannot stand in for independent BUD updates, and region ports must not be confused with existing world-I/O flags. These are requirements for the future parser/backend, with implementation stages in the [plan](INSTANT_PISTON_IMPLEMENTATION_PLAN.md).

Current fixtures establish useful projections and physical protocols. Dedicated standalone BUD acceptance examples, ordinary-node input adapters, general non-instant consumer adapters and compiled/interpreter handoff still need validation. The supplied nanotick and illegal-reset counterexamples remain rejection cases. CPU analysis stays deferred.
