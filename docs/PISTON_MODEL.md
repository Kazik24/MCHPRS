# Piston, BUD and instant ordering model

This document specifies piston behavior implemented by the world interpreter. It includes quasi-connectivity, delivered updates, event acceptance, payload ownership, movement and the causal order often called nanoticks. The rules apply to geometry and ordered operations, independently of schematic names or particular truth tables.

[REDSTONE_MODEL.md](REDSTONE_MODEL.md) defines positions, directional power, support checks, notifications and the shared scheduler. [REDPILER_MODEL.md](REDPILER_MODEL.md) defines the compiled abstraction; [Redpiler.md](Redpiler.md) describes its implementation. A derived logical gate or storage cell is valid only under the decoder and protocol stated for it. The physical interpreter continues to use the operational rules below for every input history.

## 1. State, notation and observables

Positions are integer triples. $F=(U,D,N,S,E,W)$ is the ordinary face order, $F_{\mathrm{piston}}=(W,E,D,U,N,S)$ the piston power/neighbor order, and $F_{\mathrm{shape}}=(W,E,N,S,D,U)$ the piston shape order. Facing $f$ points from a base toward its head/payload. Write $\bar f$ for the opposite face. $R_1(b,p,g;\Sigma)$ is the directional redstone query from the redstone model, and $[P]$ is the indicator of proposition $P$.

The shared state is

$$
\Sigma=(B,E,Q,c,t,\phi,a,A,M,I,L,j,K,O).
$$

$B$ and $E$ are block and entity maps; $Q,c$ the scheduled ring and cursor; $t$ the logical game tick; $\phi$ the phase; $a$ whether the cursor has advanced during this tick; $A$ the ordered event queue; $M$ the ordered exact motion records; $I$ the identity counter; $L,j$ the movement snapshot/cursor; $K$ metadata and world hooks; and $O$ observable actions/sound/command output. The complete state during a synchronous callback additionally includes its stack and active dust-walk snapshots.

From `BetweenTicks`, `tick_interpreted` completes one game tick with one scheduler half-tick bucket advance; from a partially stepped tick, it completes the remaining phases. `schedule_tick(d)` requests $2d$ game ticks, whereas `schedule_half_tick(d)` requests $d$. A movement operation occurs in the movement phase, and a piston event in the event phase. None is a separate physical time unit.

Four observations must remain distinct:

| Observation | Meaning |
| --- | --- |
| Sample | A delivered callback evaluated live power, including a no-change sample |
| Request | A particular Extend/Retract/RetractWithoutPull tuple was queued |
| Accepted event | Its live validation succeeded and its movement mutation ran |
| Settled value | A declared decoder can read a stationary base/payload state |

Block state alone is insufficient: identical geometry and power with different queues, motion identities or phases can have different futures. A moving payload occupies its destination as `MovingPiston` and emits none of its carried block's electrical power. A screen or static-animation rendering mode changes the client projection, not these simulation states.

### Ordered notification operators

Let $U(b,p,d)$ be a power callback, and $G(b,p,d)$ a shape/support callback, with $d=\bot$ meaning no supplied direction. Each nested callback completes before the caller proceeds. Reads below occur immediately before each call; duplicate positions are retained.

`Shape(p)` visits $g\in F_{\mathrm{shape}}$:

```text
q := p + g
G(B(q), q, g)
if live B(q) is an observer:
    U(B(q), q, opposite(g))
```

`Notify(p)` performs `Shape(p)` and then visits $g\in F_{\mathrm{piston}}$, calling $U(B(p+g),p+g,\bar g)$ when the live neighbor is not an observer. The shape callback direction points from the changed cell toward the receiver; the observer callback uses its opposite. Observers receive shape/state callbacks, while the subsequent loop supplies ordinary power rechecks.

The nonpiston notification operators are specified in the [redstone notification rules](REDSTONE_MODEL.md#4-immediate-callbacks-and-ordered-notification-procedures). In particular, surrounding updates can skip diagonal piston bases, and indirect power loops can skip observers. A cell being in the power neighborhood does not imply that every change in that neighborhood delivers a callback to it.

## 2. Piston power, requests, and event validation

### 2.1 Power predicate and quasi-connectivity

For piston facing $f$, define the extension predicate

$$
\begin{aligned}
X(p,f)={}&\bigvee_{g\in F_{\mathrm{piston}},\ g\ne f}
[R_1(B(p+g),p+g,g)>0]\\
&\lor[R_1(B(p),p,D)>0]\\
&\lor\bigvee_{g\in F_{\mathrm{piston}},\ g\ne D}
[R_1(B(p+U+g),p+U+g,g)>0].
\end{aligned}
$$

The first term excludes direct power from the piston-front cell. The second is the literal query from the above-cell position toward its bottom neighbor; for an ordinary piston base it returns zero. The final term checks the five non-bottom neighbors around the cell above the piston, giving the implemented quasi-connectivity neighborhood.

This predicate is evaluated when a callback rechecks the piston. The world does not globally poll every quasi-connected piston after every remote power change. Whether a recheck is delivered depends on the notification procedures, including their diagonal-piston exclusions.

### 2.2 Requests are not scheduled block ticks

A piston update receives a supplied base state $(f,s,e)$, calculates $x=X(p,f)$, and returns if $x=e$.

Calculation of $x$ is a power sample even when $x=e$ and no movement is requested. The test-only BUD trace records this sample before the early return. A delivered notification, a queued event and an accepted movement are distinct observations; an unchanged bit does not prove that no update occurred. Neither a held power level nor idle game ticks cause a global BUD resample.

If $x=1,e=0$, validate the forward payload line in section 3.1. If valid, request an Extend event. If invalid, request nothing.

If $x=0,e=1$, inspect the cell $r=p+2f$. An early-retraction condition holds only when

1. $E(r)$ is a moving-piston entity extending along $f$;
2. a motion record at $r$ exists; and
3. that record satisfies

$$
\mathrm{previous\_progress}<\tfrac12
\quad\lor\quad\mathrm{last\_tick}=t
\quad\lor\quad
\phi\in\{\mathrm{ScheduledTicks},\mathrm{PistonEvents}\}.
$$

Request RetractWithoutPull if this condition holds; otherwise request Retract.

An event is

$$
\epsilon=(p,s,f,\alpha),\qquad
\alpha\in\{\mathrm{Extend},\mathrm{Retract},\mathrm{RetractWithoutPull}\}.
$$

Append it to $A$ unless an identical full tuple is already pending. Equality includes action and captured facing. Different actions at one base can therefore coexist in the event queue. Requests do not use `pending_tick_at` and do not wait on an unrelated future base tick.

### 2.3 Event validation

When $\epsilon$ is popped, read the live block at $p$:

- It must be a piston base.
- Its sticky flag must equal the captured sticky flag.
- Recompute $X$ using the **live facing**.
- Extend executes only when currently powered and unextended.
- Either retract action executes only when currently unpowered and extended.
- Extension also rescans the live payload line and can fail.

No additional guard requires the live facing to equal the captured event facing. Extension geometry uses the live facing. Retraction geometry also uses the live facing, but the retracted carried base's facing and the emitted block-action direction use the captured facing. This is the literal behavior for an intervening state mutation.

If validation rejects the event, it has still been consumed. Successful execution emits a piston block action carrying the event action and captured direction.

The abstract storage equation `q_next = accepted_transaction ? decoded_data : q` is a settled, certified-family projection of these rules, not a replacement for validation. A no-op sample can be a logical transaction without an accepted physical movement; a cancelled or blocked movement cannot be counted as a successful state-changing write. Valid BUD history can leave an extended or retracted cell disagreeing with current power until a qualifying recheck. Section 4 defines the settled decoder and separates these records.

### 2.4 Legacy ticks and heads

A scheduled callback on a piston base simply calls the state-request procedure above. It is not a movement-completion callback or a cooldown.

A neighbor update on a piston head looks one cell opposite its facing; if that cell is a piston base, it rechecks that base. Ordinary moving-piston states have no redstone tick or update action. Their progress is advanced through the movement phase.

## 3. Piston payload transport and completion

### 3.1 Forward payload line

For extension, scan the ordered cells

$$
p+f,p+2f,p+3f,\ldots
$$

until one of the following cases occurs:

| Encounter | Result |
| --- | --- |
| Out-of-height or unavailable chunk | Reject extension |
| Recognized container block | Reject extension |
| Air | Accept; air terminates the line and is not a payload |
| Moving piston or piston head | Reject extension |
| Any other block | Append it to the line and continue |

Let the accepted payload positions be $(q_1,\ldots,q_n)$ from nearest to farthest, ending before the terminating air cell.

The scan has **no twelve-block limit** in this checkout. It also does not consult a general Minecraft push-reaction table. Bedrock, obsidian, unsupported variants, and extended bases are not rejected merely by their names or hardness. Slime/honey adhesion and branching payload sets are not implemented here. The model is the literal straight-line scan above.

### 3.2 Creating a moving cell

Define $\operatorname{MoveCell}(r,b,f,s,e,u,E_b)$, where $b$ is a carried state, $e$ indicates extension, $u$ indicates a source head/base, and $E_b$ is an optional carried entity. For `PlotWorld`, it performs, in order:

1. Delete the destination's existing block entity and motion record.
2. Write a moving-piston block at $r$, with facing $f$ and sticky flag $s$.
3. Install a moving-piston entity storing $b$'s raw state ID, direction $f$, extending flag $e$, source flag $u$, and serialized progress $0$.
4. Register a new exact motion record at $r$ with a fresh identity.
5. Attach a clone of $E_b$, if present, to that motion record's carried-entity field.

Registration removes any previous motion at $r$, increments $I$, and appends to $M$. The new record is

$$
\mu=(r,I,0,0,t,E_b),
$$

with fields $(\mathrm{pos},\mathrm{identity},\mathrm{progress},\mathrm{previous\_progress},\mathrm{last\_tick},\mathrm{carried\_entity})$.

The carried state does not contribute its original redstone power while the cell is moving. Queries see a non-solid, transparent moving-piston block with zero primitive weak/strong power.

### 3.3 Extension mutation order

A successful extension snapshots every $(q_i,B(q_i),E(q_i))$ **before writing overlapping cells**. Then:

1. For $i=n,n-1,\ldots,1$, create an extending, nonsource moving payload at $q_i+f$.
2. Create an extending source moving head at $p+f$, carrying a nonshort head matching the current base's direction and sticky flag.
3. For $i=n,\ldots,1$, run $\operatorname{Shape}(q_i+f)$.
4. For $i=n,\ldots,1$, run $\operatorname{Notify}(q_i)$.
5. Run $\operatorname{Notify}(p+f)$.
6. Write the base with `extended=true`.
7. Run $\operatorname{Notify}(p)$.

Each old source cell is overwritten by the preceding payload's destination; the first source cell is replaced by the moving head. Payloads are moved farthest first so that the complete pre-move snapshot is preserved.

At the end of the event, the base is already extended even though the moving head and payloads have not finished. There is no separate scheduled base lock keeping it unextended until movement completion.

Motion registration order is farthest payload first, nearest payload last, then the head. This order is later reflected in the movement snapshot unless subsequent mutations remove or append records.

### 3.4 Retraction mutation order

Let $f$ be the live base facing, $h=p+f$, and $r=p+2f$. Let $f_c$ be the event's captured facing.

1. If $B(h)$ is moving, interrupt-complete it using section 3.7.
2. Replace the base at $p$ by a retracting source moving cell carrying an unextended base with facing $f_c$ and the current sticky flag. The moving cell itself travels along $f$.
3. Run $\operatorname{Notify}(p)$.
4. Determine whether $r$ is a moving-piston cell with an extending entity whose direction is $f$.
5. Delete the entity at $h$ and write air at $h$.
6. If the base is sticky and $r$ is that matching in-flight extension, interrupt-complete $r$ and do not pull it. This applies even to an ordinary Retract event.
7. Otherwise, if the base is sticky and the action is Retract, inspect $B(r)$. If it is nonair, not moving, and not a recognized container, snapshot its entity, create a retracting nonsource moving payload at $h$, clear $r$ and its entity, and notify $r$.
8. Run $\operatorname{Notify}(h)$.

A nonsticky base does not pull. RetractWithoutPull suppresses the ordinary stationary-payload pull branch. The pull branch has no separate full extension-scan validation and does not reject every block excluded by Minecraft's general piston rules.

### 3.5 Movement snapshot and identity

After all currently queued piston events are consumed, the game tick snapshots

$$
L=((\mu_1.\mathrm{pos},\mu_1.\mathrm{identity}),\ldots,
(\mu_k.\mathrm{pos},\mu_k.\mathrm{identity}))
$$

in $M$'s current order.

When an entry $(r,i)$ is consumed, it acts only if the matching identity still exists at $r$. A replacement moving entity at the same position receives a new identity and is not advanced by stale work. If the block or entity at $r$ is no longer a moving piston, remove the old motion record and do nothing further.

Motions created or replaced after $L$ was taken wait until a later movement snapshot. Motions created in the preceding piston-event phase are included in the current tick's movement snapshot.

### 3.6 Exact progress recurrence

For a valid motion with current progress $g$, each movement operation first sets

$$
\mathrm{last\_tick}:=t,\qquad
g_{\mathrm{previous}}:=g.
$$

If $g\ge1$, complete the motion immediately. Otherwise set

$$
g':=\min(1,g+\tfrac12)
$$

and store the **old** value $g_{\mathrm{previous}}$ in the moving entity's serialized progress field.

An uninterrupted motion created at progress zero therefore evolves as

| Movement operation | Exact progress after operation | Serialized/interpolated progress | Cell state |
| --- | --- | --- | --- |
| Creation | $0$ | $0$ | Moving piston |
| First | $1/2$ | $0$ | Moving piston |
| Second | $1$ | $1/2$ | Moving piston |
| Third | Removed | Entity removed | Restored carried block, subject to validity |

Completion checks the progress **before** incrementing. Reaching progress $1$ at the second operation does not itself restore the block.

The legacy byte conversion is

$$
\operatorname{enc}(g)=\left\lfloor\operatorname{clamp}(255g,0,255)\right\rfloor.
$$

Decoding byte $127$ returns exactly $1/2$; other bytes return $b/255$ in `f32`. Exact current and previous progress for active simulation live in $M$, not only in this byte field.

### 3.7 Normal and interrupted completion

Completion requires both a moving-piston block and a moving-piston entity at the destination. It then performs:

1. Choose restored state $b$ as the carried block, except that **interrupted source** motions restore air.
2. For normal completion only, clear waterlogging while preserving other state properties.
3. Retrieve the optional carried entity from the motion record.
4. Delete the moving entity/motion, write $b$, and restore the carried entity if present.
5. If $b$ is a piston head, retain it only when its opposite-facing neighbor is an extended base with matching facing and sticky flag; otherwise write air.
6. If $b$ is a powered observer and no observer-type request is pending at this position, reset it to unpowered and issue observer-output notifications.
7. If the restored state is invalid under the support predicate, delete its entity and write air.
8. Read the resulting live block. Invoke $U(B(r),r,\bot)$ once unless it is an observer or piston head.
9. Run $\operatorname{Notify}(r)$.

Interrupted nonsource payloads restore their carried state immediately and preserve waterlogging. Interrupted source heads or source bases disappear. Normal completion restores the carried source state, subject to the head and support checks.

The explicit head-ownership check in step 5 remains even if a custom world's support-check hook bypasses the generic support check.

### 3.8 Removing owned piston parts

Destroying an extended base also removes its owned stationary head or owned extending source moving head. Ownership requires matching direction and sticky flag. Destroying a stationary head removes its matching extended base. Destroying an extending source moving head can remove that base as well.

Transported nonsource payloads are independent. Removing the base/head does not retroactively erase unrelated payload motions, which may finish later. Deleting a source motion removes its identity, so its old completion cannot recreate a destroyed base or head.

## 4. BUD switches as sampled state

A block update detector (BUD) exploits a separation between power and notification. In this implementation, quasi-connectivity changes the extension predicate $X$ without introducing continuous polling. The stationary extended bit $e$ changes through an accepted operation, not merely because $X$ changed.

Let an external data change affect $X(p,f)$, but deliver no qualifying base/head callback. Then the physical transition is

$$
X^+\ne X^- \text{ is permitted while } e^+=e^-.
$$

This power/state disagreement is allowed. Empty queues and repeated game ticks do not repair it automatically. A callback delivered to a stationary base samples $x_j=X(p,f;\Sigma_j)$ and supplied extension $e_j$, producing a sample record

$$
\nu_j=(p,e_j,x_j).
$$

When $x_j=e_j$, the sample creates no request. Otherwise section 2 may queue a request after payload validation. A callback to a matching stationary head can recheck its base. A callback to an ordinary `MovingPiston` state has no such base-update handler.

### Sampling, acceptance and storage

Write $r_j$ for the requested action, if any. At event consumption, the power sample is recomputed:

$$
x_j^{\mathrm{accept}}=X(p,f_{\mathrm{live}};\Sigma_j^{\mathrm{accept}}).
$$

Neither $x_j^{\mathrm{accept}}=x_j$ nor acceptance itself is guaranteed. Source changes, another event, an edit, a blocked extension or changed base type/sticky state can invalidate the request. A duplicate request can also be coalesced before consumption. Therefore neither a notification nor an enqueue log is an accepted write log.

For an isolated, unblocked transaction with matching supplied/live base state at sampling, unchanged facing/sticky state, valid ownership and power held at $x_j$ from sampling through acceptance and settlement, the settled extension satisfies

$$
e_j^+=x_j.
$$

This statement includes a same-value sample: if $e_j=x_j$, no movement is required. It does not identify sampling time with movement-completion time. It also does not apply to overlapping transactions whose notification or event ordering changes the live data.

A storage decoder $D$ establishes logical polarity. For example, with retracted = one and extended = zero,

$$
q=D(e)=1-e,\qquad d_j=1-x_j.
$$

Under the stated transaction protocol, a logical accepted-sample indicator $u_j$ gives the settled storage equation

$$
q_j^+=u_jd_j+(1-u_j)q_j.
$$

Here $u_j=1$ means a valid logical sample transaction, including a same-value sample, rather than necessarily a physical movement. If $u_j=0$, the request has no committed logical write of its own; other independent operations still retain their physical effects. Reversing the decoder polarity reverses $d_j$ and $q_j$ together. Neither rising-edge-only writes nor retracted = one is a universal piston rule.

While a required observation cell is moving, a stationary decoder returns $\bot$ (invalid), rather than assuming zero. Another boundary, such as an ordinary repeater, can remain a valid electrical observation throughout that same interval. Validity belongs to the declared port.

### Ordered and atomic transactions

An ordinary updater can supply notifications on extension, retraction or restoration. Both movement edges can sample a receiver; holding enable need not cause repeated sampling. Button widths and upstream torch/repeater behavior determine which notifications actually reach it. The sampling signal is an ordered notification stream, not simply the level of an enable wire.

For successive updates on one cell,

$$
q_{j+1}=H_j(q_j,d_j,u_j),
$$

with each $d_j$ decoded at its own boundary. If data-change operation $a$ and sampling operation $b$ do not commute, then

$$
\operatorname{Obs}(T_b(T_a(\Sigma)))\ne
\operatorname{Obs}(T_a(T_b(\Sigma)))
$$

can hold even when both histories end with the same external source levels. A simultaneous bank equation $\mathbf q^+=H(\mathbf q,\mathbf d)$ is justified only when its protocol establishes a common old-bank sample before committing writes. It does not follow merely from updates sharing one game tick.

Read gates can retain geometry independently of the data bank. In general, an observable read channel has the form

$$
o=\mathcal A(\mathbf q,\mathbf r,c,\mathbf s),
$$

where $\mathbf r$ is retained read-gate state, $c$ the adapter's lifecycle/selection state and $\mathbf s$ ordinary electrical strengths. Replacing this by continuously reading `bank[address]` requires proof that those retained variables are unnecessary for the selected protocol.

## 5. Instant computation and nanotick order

### Levels, falling events and first responses

An electrical level is a strength $s\in\{0,\ldots,15\}$. A falling event at ordered boundary $k$ is

$$
e_k=[s_{k^-}>0\land s_k=0].
$$

Held zero generates no new external falling event. Internal observers and geometry restoration can still produce subsequent callbacks and response cycles. A prepared data value, a falling event and stored state are separate inputs with separate decoders.

When an extended sticky piston is sampled unpowered and ordinary retraction pulls a stationary far source, that event immediately replaces the head/source representation with moving cells. Their electrical absence can update a downstream mechanism while the event phase is still running. If that downstream request is enqueued before the phase drains, it can execute in the same game tick. Thus an eligible chain need not pay one game tick per actuator. Early/drop retractions use the different branches in section 3.4. Each uninterrupted motion still follows section 3's three movement operations.

An electrical join always uses maximum. Under fixed topology, two initially positive contributors disappear from one consumer only after both disappear:

$$
[\max(s_A,s_B)=0]=[s_A=0]\land[s_B=0].
$$

That can yield logical AND in a falling-event decoder. A shared far payload can disappear when any accepted owner retracts, yielding an OR-like disappearance predicate. It is a shared occupancy/ownership event, rather than two independent copies of the payload. If a later operation transfers or recaptures that block, subsequent behavior must include the new ownership state.

For inhibition, a first-response projection can have

$$
y=e_T\land\neg i.
$$

Physically, $i$ must be effective at the relevant consumer acceptance/sampling boundary to suppress that response. An inhibit that arrives later in the same tick can change later behavior without cancelling a completed earlier acceptance. An intended truth table alone therefore does not specify the physical transition system.

### Causal order within a game tick

Let $\tau=(t,k)$ identify the $k$th ordered semantic transition during game tick $t$. $k$ orders queue dispatches and their nested callbacks; it is not elapsed nanoseconds. Within a callback, writes and nested notifications execute in source order. Across callbacks, the scheduler chooses priority/FIFO order; piston events use FIFO; motion uses its identity snapshot.

The phase order is

$$
\mathrm{ScheduledTicks}\prec\mathrm{PistonEvents}\prec\mathrm{MovingEntities}.
$$

The scheduled phase drains both the old cursor bucket and, after its one advance, the new bucket. An event created during scheduled processing can run in the current event phase. An event created by another event is appended and can run in that same phase. An event created during movement waits for the next event phase. A motion created before the movement snapshot is eligible in the current tick; a replacement created after that snapshot waits for another snapshot.

For operations $a,b$, write $a\prec b$ when the dispatcher or a nested call requires $a$ to complete before $b$ starts. Independent-looking changes are only interchangeable if their writes, reads and generated work commute. Cached dust states, source ownership and callback delivery can introduce dependencies beyond adjacent base positions. Tick equality alone does not prove synchronization.

Piston event acceptance reads live power. Consequently, the important inhibition relation is

$$
\operatorname{inhibit\ effective}\prec
\operatorname{consumer\ acceptance},
$$

where “effective” includes all ordinary component delays and geometry needed by that consumer. A queued consumer request may still be rejected if inhibition becomes effective before acceptance; an accepted request has already performed its mutation. For a BUD interface, the analogous boundary is its delivered power sample and any subsequent validated transaction.

### Fine stepping APIs

`picotick_advance(n)` attempts $n$ individual interpreter operations. An operation consumes a scheduled entry, piston event or motion-snapshot item, including all nested callbacks. An iteration may instead finish an exhausted tick administratively.

`nanotick_advance(n)` prepares the next operation and snapshots the current phase's remaining operation **count**. It then executes that many normal operations. This does not freeze an operation list: newly inserted higher-priority scheduled work can precede older entries, and new event work can remain after the count is exhausted. A tick-finishing preparation also consumes an iteration. Both APIs return without advancing while history recording is enabled.

Neither API exposes a dust-walk layer or pauses inside a synchronous callback. Completing a tick by game, nano or pico stepping follows the same transition sequence when external inputs are supplied at equivalent operation boundaries. Test instrumentation can observe finer callback ordering, but changes no production simulation rule.

### Reset, reuse and boundary strength

Ready, response, restoration and rearm are distinct states. Restoring an extended base does not prove that payload ownership, observer power, ordinary input levels and pending work are ready for another independent transaction. An observer reset can recur while an external source remains absent. Ordinary repeaters can retain queued activations after a brief restore pulse, so publishing only a permanently settled result loses boundary behavior.

For a certified geometry state $g$, consumer channel $c$ and contribution $i$, a useful electrical projection is

$$
s_c=\max\left(\{s_i\mathbin{\dotminus}a_i:
\gamma_i(g)=1\}\cup\{0\}\right),
$$

where $a_i$ is dust attenuation and $\gamma_i$ the directional/occupancy guard. Conducting payloads expose permitted strong sources; they do not generate an intrinsic strength fifteen. Fixed contributors remain in the maximum when a mobile contributor disappears. A moving payload supplies no stationary power or conduction. Comparator overrides and side channels still use their distinct redstone equations.

A first-response function $\mathbf y=F(\mathbf d,\mathbf e,\mathbf q)$ is a derived projection over a declared input protocol and observation window. Complete reuse adds retained reset/read/clock state $z$ and transitions $z^+=H(z,\sigma)$ for the ordered actions $\sigma$. No universal reset period or universal observation window follows from a piston truth table. The [compiled model](REDPILER_MODEL.md) states which physical ordering is represented, normalized or restricted in each execution path.

## 6. General rules and implementation limits

The power/notification separation, event revalidation, identity-based motion and strength/occupancy projections explain broad classes of instant and BUD circuits. They do not require recognizing an adder, counter or memory by its name, bit coordinates or schematic hash.

The following literal implementation choices matter when extending those models:

- Payload movement is straight-line transport. The scan has no twelve-block cap, general push-reaction table, destroy-on-push branch or slime/honey adhesion. Those mechanics require an explicit new transport rule rather than exceptions for individual schematics.
- Observers are triggered by delivered callback directions. Caller-specific exclusions distinguish shape changes from indirect power rechecks; the observer itself does not compare old/new observed block states.
- Early retraction depends on phase and exact motion history, not only on a byte progress field. Replacing that condition with a fixed cooldown changes short-pulse and recapture behavior.
- Event validation uses live facing while retraction's carried base and emitted action retain captured facing. Equality of those directions is not currently required. A universal orientation invariant would need an implementation change and regression evidence.
- Support is a directional predicate. A moving cell usually loses stationary support, with an explicit downward retracting source-base exception for dust. Restore-time validity and head ownership still apply.
- Same-value BUD samples are observable even without a movement. A movement count cannot stand in for an ordered sample trace or establish continuously sampled storage.

The interpreter can learn a per-piston address certificate from an actual native
retract/reset cycle. Its initial family is a sticky, non-upward actor moving one
entity-free redstone block, with a downward-facing reset observer directly above
the base and an entity-free solid cap. Admission requires ordinary pull
retraction, both exact motion identities completing, an observer-driven
single-block reset without an alternate cap writer, and both reset motions
completing with the expected restored footprint. Geometry alone never enables
the fast path.

A proven actor caches the eleven addresses from section 2's power query and the
six strong-power source addresses around each potential conductor. Each sample
still reads live block states and electrical strengths in the original order.
Native events, notifications, BUD samples and movement phases remain
unchanged. Relevant block/entity edits revoke certificates; mutable chunk or
piston-state access clears them. Certificates are bounded, ephemeral interpreter
state, rather than saved-world data or permission to collapse future waves.
Runtime coverage and benchmark protocols are recorded in
[tests/INSTANT_PISTONS.md](tests/INSTANT_PISTONS.md).

| Subject | Source |
| --- | --- |
| Power, requests, acceptance and mutation order | [piston.rs](../crates/core/src/redstone/piston.rs) |
| Event/motion/phase types and exact progress recurrence | [world/lib.rs](../crates/world/src/lib.rs) |
| Queue deduplication, movement registration, phase stepping and loading | [plot/mod.rs](../crates/core/src/plot/mod.rs) |
| Moving entity progress encoding | [block_entities.rs](../crates/blocks/src/block_entities.rs) |
| Support checks, destruction and owned-part removal callers | [interaction.rs](../crates/core/src/interaction.rs) |
| Power queries and observer pulse callbacks | [redstone/mod.rs](../crates/core/src/redstone/mod.rs) |
| Test-only sample/acceptance instrumentation | [piston/trace.rs](../crates/core/src/redstone/piston/trace.rs), [instant_piston_tests.rs](../crates/core/src/redstone/instant_piston_tests.rs) |

[tests/README.md](tests/README.md) contains runnable verification and fixture protocols. Saved reference traces are evidence for their input histories, not a replacement for these general transition rules.
