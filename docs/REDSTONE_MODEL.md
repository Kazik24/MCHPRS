# Mathematical specification of the MCHPRS interpreter redstone model

This document defines the redstone simulation implemented by the **world interpreter** in this checkout. It describes a discrete, spatial, ordered transition system, including the event queues needed to reproduce its timing. The Boolean gate interpretation of redstone is a useful consequence of these rules, but does not specify the interpreter completely.

The specification was reconciled with the working-tree sources on **2026-10-06**. The base revision was `4a2fb767821567ba62c5a63bb9af68fdc19d26e3`; the working tree also contained local changes. Source links below refer to this repository, and the code is authoritative if it subsequently changes.

The scope is `PlotWorld::tick_interpreted` and the functions it calls. Redpiler's compiled graph executor is a separate model; its graph optimizations are not assumptions of this specification. Network rendering, permissions, and wall-clock pacing are included only where they affect inputs or observable execution results.

## Contents

1. [Mathematical conventions and spatial domain](#1-mathematical-conventions-and-spatial-domain)
2. [Block states, metadata, and complete simulator state](#2-block-states-metadata-and-complete-simulator-state)
3. [Directional signal functions](#3-directional-signal-functions)
4. [Immediate callbacks and ordered notification procedures](#4-immediate-callbacks-and-ordered-notification-procedures)
5. [Scheduled ticks and the game-tick transition](#5-scheduled-ticks-and-the-game-tick-transition)
6. [Dust geometry and the initial dust update](#6-dust-geometry-and-the-initial-dust-update)
7. [The exact Wire Turbo propagation procedure](#7-the-exact-wire-turbo-propagation-procedure)
8. [Torches](#8-torches)
9. [Repeaters](#9-repeaters)
10. [Comparators and analog overrides](#10-comparators-and-analog-overrides)
11. [Sources, immediate consumers, and note blocks](#11-sources-immediate-consumers-and-note-blocks)
12. [Observers](#12-observers)
13. [Piston power, requests, and event validation](#13-piston-power-requests-and-event-validation)
14. [Piston payload transport and completion](#14-piston-payload-transport-and-completion)
15. [Support geometry, placement, destruction, and use](#15-support-geometry-placement-destruction-and-use)
16. [Command blocks](#16-command-blocks)
17. [Initialization, persistence, and stepping](#17-initialization-persistence-and-stepping)
18. [Derived properties and limits of simpler models](#18-derived-properties-and-limits-of-simpler-models)
19. [Worked traces](#19-worked-traces)
20. [Implementation and regression-test index](#20-implementation-and-regression-test-index)

## 1. Mathematical conventions and spatial domain

### 1.1 Positions and directions

A position is an integer triple $p=(x,y,z)$. The six face directions and their displacement vectors are

| Symbol | Rust face | Displacement |
| --- | --- | --- |
| $U$ | `Top` | $(0,1,0)$ |
| $D$ | `Bottom` | $(0,-1,0)$ |
| $N$ | `North` | $(0,0,-1)$ |
| $S$ | `South` | $(0,0,1)$ |
| $E$ | `East` | $(1,0,0)$ |
| $W$ | `West` | $(-1,0,0)$ |

Write $p+f$ for `p.offset(f)` and $\bar f$ for the opposite direction. Thus $\bar U=D$, $\bar N=S$, and $\bar E=W$. Horizontal directions form $\mathcal H=\{N,E,S,W\}$. Clockwise horizontal rotation is $r(N)=E$, $r(E)=S$, $r(S)=W$, $r(W)=N$; counterclockwise rotation is $r^{-1}$.

Three different ordered face lists occur in the implementation:

$$
\begin{aligned}
F&=(U,D,N,S,E,W),\\
F_{\mathrm{piston}}&=(W,E,D,U,N,S),\\
F_{\mathrm{shape}}&=(W,E,N,S,D,U).
\end{aligned}
$$

$F$ is `BlockFace::values()`. These lists are **sequences**, not merely sets. Nested callbacks execute in sequence order, so interchanging two entries can change a transient circuit trace.

For representable ordinary coordinates, offsets are integer vector addition. Literally, `BlockPos::offset(Bottom)` uses `i32::saturating_sub(1)`; saturation matters only at `i32::MIN`, outside the playable vertical domain. Other coordinate arithmetic assumes no integer overflow.

### 1.2 Plot domain and boundary conditions

For plot coordinates $(a,b)$, the writable domain is

$$
\Omega_{a,b}=\{256a,\ldots,256a+255\}\times\{0,\ldots,255\}
\times\{256b,\ldots,256b+255\}.
$$

Each plot contains $16\times16$ chunks, each with $16$ sections of height $16$. Reading a block outside the plot or outside its section range returns the air state, whose raw state ID is $0$. Writing a block outside $\Omega_{a,b}$ fails and changes no block. In particular, another plot is not an electrical boundary neighbor in this interpreter instance.

Piston extension uses a stronger boundary check: every cell examined in its forward payload scan must be within the height limit and have a chunk available in this world, including the terminating air cell.

### 1.3 Signal algebra

The intended signal-strength domain is the finite chain

$$
\mathcal S=\{0,1,\ldots,15\}.
$$

Define $[P]$ to be $1$ when proposition $P$ is true and $0$ otherwise. Define Boolean-to-strength conversion $\beta(P)=15[P]$, and saturating subtraction

$$
a\mathbin{\dotminus}b=\max(0,a-b).
$$

Unless stated otherwise, $\max\varnothing=0$. Electrical joins use maximum, not addition. Two inputs of strength $8$ therefore combine to strength $8$.

Most stored strengths are Rust `u8` fields. The equations assume valid block states and valid analog data in $\mathcal S$. A raw API can construct inconsistent or out-of-range entities; arbitrary such states are not promised to satisfy the boundedness properties proved later. For byte-level reproduction, use the actual integer widths, saturation, and `f32` computations noted below.

## 2. Block states, metadata, and complete simulator state

### 2.1 Registry state versus block type

Let $B(p)$ be the complete block state stored at $p$. Let

$$
\operatorname{sid}(B)=\text{block-state ID},\qquad
\operatorname{type}(B)=\text{registry block ID}.
$$

These are different quantities. Changing a repeater's `powered`, `locked`, `delay`, or `facing` property changes its state ID while keeping its block type. A normal piston and a sticky piston have different block types.

The representation is a tagged sum of modeled Rust `Block` variants and `Unknown { id }` states. `Unknown` means that the registry state is retained without a dedicated modeled variant; it does not mean that the block is absent. Registry names and properties can still be inspected. Commands, newer sign species, many pressure plates, and many slab states use this path.

### 2.2 Dynamic electrical state

The principal dynamic fields are

| Block | Dynamic or configuration state |
| --- | --- |
| Dust | $P\in\mathcal S$; four sides in $\{\mathrm{None},\mathrm{Side},\mathrm{Up}\}$ |
| Standing torch | `lit` $\ell\in\{0,1\}$ |
| Wall torch | $\ell$ and horizontal facing $f$ |
| Repeater | facing $f$, delay $d\in\{1,2,3,4\}$, locked $l$, powered $q$ |
| Comparator | facing $f$, mode $m\in\{\mathrm{Compare},\mathrm{Subtract}\}$, powered $q$; separate entity output $o$ |
| Lever/button | attachment face, horizontal facing, powered $q$ |
| Binary pressure plate | powered $q$ |
| Lamp | lit $\ell$ |
| Hopper | facing and enabled $e$; container entity |
| Iron trapdoor | facing, half, powered $q$ |
| Note block | instrument, note $n\in\{0,\ldots,24\}$, powered $q$ |
| Observer | six-direction facing $f$, powered $q$ |
| Piston | facing $f$, sticky $s$, extended $e$ |
| Piston head | facing $f$, sticky $s$, short-head flag |
| Moving piston | facing and sticky state; entity containing carried state, direction, progress, extending/source flags |
| Command block | registry facing/conditional/mode properties; entity containing activation and execution state |

Let $E(p)$ be the optional block entity. A comparator's analog output is read from $E(p)$, independently of its block-state powered bit. A moving block's payload is also stored in an entity rather than exposed as the electrical state of the destination cell.

### 2.3 Electrical geometry metadata

Define the predicates

$$
\operatorname{Solid}(B)=B.\texttt{is\_solid()},\quad
\operatorname{Transparent}(B)=B.\texttt{is\_transparent()},\quad
\operatorname{Cube}(B)=B.\texttt{is\_cube()}.
$$

Their meanings are operational: `Solid` selects the conduction branch of a power query; `Transparent` affects climbing dust propagation; `Cube` participates in attachment support. These predicates must not be inferred from visual appearance.

Representative values in this checkout are

| State category | Solid | Transparent | Cube |
| --- | --- | --- | --- |
| Modeled opaque simple cube, stone, concrete, wool, ordinary lamp | 1 | 0 | 1 |
| Modeled barrel, furnace, note block | 1 | 0 | 1 |
| Glass, stained glass, glowstone | 0 | 1 | 1 |
| Redstone block | 0 | 1 | 1 |
| Observer | 0 | 1 | 1 |
| Piston, piston head, moving piston | 0 | 1 | 1 |
| Hopper | 0 | 1 | 1 |
| Chest | 0 | 1 | 0 |
| Air, dust, torches, repeaters, comparators, modeled stone button/lever/plate | 0 | 0 | 0 |
| Top slab, including registry-backed slab species | 0 | 1 | 1 |
| Bottom slab | 0 | 1 | 0 |
| Double slab | 1 | 0 | 1 |
| Generic `Unknown`, except special slab geometry | 1 | 0 | 1 |

Slab properties override the general variant metadata. Command blocks explicitly count as solid cubes. Generic `Unknown` states otherwise use the fallback values shown above, even if the corresponding Minecraft block would have different geometry. In particular, recognizing an `Unknown` binary plate in the primitive weak/strong-power functions does not bypass the solid-block branch of a general power query.

The full predicate definitions are the `blocks!` macro and its block declarations in [blocks/mod.rs](../crates/blocks/src/blocks/mod.rs), together with `slab_type`. These declarations are part of the mathematical model's finite parameter table.

### 2.4 The simulator state

A sufficient semantic state between public interpreter operations is

$$
\Sigma=(B,E,Q,c,t,\phi,a,A,M,I,L,j,K,O).
$$

Here

- $B,E$ are the block and entity maps.
- $Q$ is the scheduled-tick ring, with ordered queues for every priority.
- $c\in\{0,\ldots,31\}$ is its current bucket cursor.
- $t\in\mathbb N$ is `logical_tick`.
- $\phi$ is the advancement phase.
- $a$ is `scheduled_advanced`, recording whether the cursor has advanced during the current game tick.
- $A$ is the ordered piston-event queue.
- $M$ is the ordered list of exact piston-motion records.
- $I$ is the next motion-identity counter.
- $L$ and $j$ are the movement-phase identity snapshot and its cursor.
- $K$ is configuration and effective world-hook values relevant to rules, including the support-check hook discussed in section 15.
- $O$ is the ordered observable output stream, including sound, piston actions, and command results.

During an immediate callback, the call stack and any active Wire Turbo walk state are additional operational state. They are exhausted before that callback returns; pico stepping does not pause inside them.

Persistent address caches and piston indexes accelerate lookup. They do not replace $B$, $Q$, $A$, or $M$. Wire Turbo's **per-walk block snapshots**, however, are semantically relevant during an active walk and are defined in section 7.

## 3. Directional signal functions

### 3.1 The side argument is a query direction

For the general neighbor-input sampling of a receiver at $p$ and a neighbor at $q=p+f$, the interpreter calls the neighbor's power function with side **$f$**:

$$
\operatorname{Power}(B(q),q,f).
$$

Consequently, side $f$ points **from the receiver toward the emitter**. The physical direction from that emitter to the receiver is $\bar f$.

This resolves several otherwise confusing conventions. A diode whose stored facing is $f$ reads its input at $p+f$ and emits toward $p+\bar f$. An observer with facing $f$ watches the neighbor at $p+f$ and emits toward $p+\bar f$. A standing torch's strong-power side $D$ means it powers the block **above** the torch.

Some specialized helpers supply a different literal side argument, notably the standing-torch input helper in section 8. Their equations retain those calls. Direction arguments supplied to update callbacks are a separate convention and must be reproduced per notification procedure; they are not universally identical to power-query sides.

### 3.2 Primitive weak power

Let $W_\delta(b,p,f;\Sigma)$ be `get_weak_power`, where $\delta\in\{0,1\}$ permits or suppresses dust emission. Omitted cases return $0$.

| Emitter state $b$ | Condition | $W_\delta$ |
| --- | --- | --- |
| Standing torch | lit and $f\ne U$ | $15$ |
| Wall torch with facing $g$ | lit and $f\ne g$ | $15$ |
| Redstone block | any side | $15$ |
| Recognized binary pressure plate | powered | $15$ |
| Modeled lever or stone button | powered | $15$ |
| Repeater with facing $g$ | powered and $f=g$ | $15$ |
| Comparator with facing $g$ | $f=g$ | comparator entity output $o(p)$ |
| Dust with strength $P(p)$ | $\delta=1$, $f=U$ | $P(p)$ |
| Dust | $\delta=1$, $f=D$ | $0$ |
| Dust | $\delta=1$, $f\in\mathcal H$, regulated side $\bar f$ is non-None | $P(p)$ |
| Observer with facing $g$ | powered and $f=g$ | $15$ |

If the comparator entity is absent or has another entity type, $o(p)=0$. Comparator emission does **not** test its block-state `powered` flag.

For horizontal dust emission, the sides are recomputed by the regulation procedure in section 6 using current world geometry. The stored sides alone are not the final predicate.

The binary plate predicate includes the modeled stone plate and `Unknown` registry names ending in `_pressure_plate` that have a `powered` property. Weighted plates with a `power` property do not satisfy this predicate.

### 3.3 Primitive strong power

Let $S_\delta(b,p,f;\Sigma)$ be `get_strong_power`. Omitted cases return $0$.

| Emitter | Condition | $S_\delta$ |
| --- | --- | --- |
| Either torch type | lit and $f=D$ | $15$ |
| Lever/button attached to floor | powered and $f=U$ | $15$ |
| Lever/button attached to ceiling | powered and $f=D$ | $15$ |
| Wall lever/button with facing $g$ | powered and $f=g$ | $15$ |
| Recognized binary plate | powered and $f=U$ | $15$ |
| Dust, repeater, comparator | any side | $W_\delta(b,p,f;\Sigma)$ |
| Observer with facing $g$ | powered and $f=g$ | $15$ |

A redstone block has primitive weak power $15$ and primitive strong power $0$. It directly powers adjacent nonconducting receivers through the weak-power branch, but does not act as a strong-power source for an intervening solid cube.

### 3.4 Solid-block conduction and general power

Define the strong power incident on a cell $p$ as

$$
C_\delta(p;\Sigma)=\max_{f\in F}S_\delta(B(p+f),p+f,f;\Sigma).
$$

The general directional power query is

$$
R_\delta(b,p,f;\Sigma)=
\begin{cases}
C_\delta(p;\Sigma),&\operatorname{Solid}(b),\\
W_\delta(b,p,f;\Sigma),&\text{otherwise}.
\end{cases}
$$

`get_redstone_power` is $R_1$; `get_redstone_power_no_dust` is $R_0$.

Solid conduction has no independent stored charge and no scheduled delay. It is computed from immediately adjacent primitive strong emitters whenever queried. Since $C$ uses $S$, rather than recursively using $R$, a chain of ordinary solid cubes does not repeatedly relay power through cube-to-cube conduction.

The $b$ argument is explicit because Wire Turbo sometimes supplies a cached state while the conduction neighborhood and comparator entity are read from the live world. For ordinary calls, $b=B(p)$.

### 3.5 Common input predicates

The any-neighbor powered predicate is

$$
H(p;\Sigma)=\left[\max_{f\in F}R_1(B(p+f),p+f,f;\Sigma)>0\right].
$$

Lamps, hoppers, trapdoors, note blocks, and command-block activation use this predicate.

The diode rear-input strength for facing $f\in\mathcal H$ is

$$
D(p,f;\Sigma)=
\begin{cases}
P(p+f),&R_1(B(p+f),p+f,f;\Sigma)=0\ \land\ B(p+f)\text{ is dust},\\
R_1(B(p+f),p+f,f;\Sigma),&\text{otherwise}.
\end{cases}
$$

This dust fallback lets a diode read adjacent dust strength even when dust's directional weak-emission rule returned zero.

## 4. Immediate callbacks and ordered notification procedures

### 4.1 Three different operations

The model distinguishes

1. **Storage write**: `set_block` changes a cell and performs limited entity housekeeping. It does not automatically notify all neighbors.
2. **Neighbor update**: `redstone::update(b, world, p, dir)` recalculates or schedules component behavior.
3. **Scheduled callback**: `redstone::tick(b, world, p)` executes a previously queued tick against the current block state.

There is also a **shape/support callback**, `interaction::change`, which checks attachment validity and dust sides. A shape callback can destroy unsupported components or trigger further redstone callbacks.

Write $U(b,p,d)$ for a redstone neighbor update with $d\in F\cup\{\bot\}$, where $\bot$ is Rust `None`. Write $T(b,p)$ for a scheduled callback and $G(b,p,d)$ for a shape callback. These are sequential state-transforming procedures, not pure functions on a frozen world.

The direction arguments of $U$ and $G$ are distinct operational inputs. In the immediate-neighbor shape notifiers below, $G$ receives the face from the changed cell toward its neighbor, while the corresponding observer update receives the opposite face. Do not normalize both callbacks to the same direction. The general shape notifier's additional vertical-diagonal calls retain their literal face argument, as specified in section 15.2.

Unless a rule explicitly specifies a cached argument, every notification below reads $B$ immediately before calling $U$ or $G$. Any nested work completes before the next notification is invoked. Repeated positions are not eliminated.

### 4.2 Surrounding updates

Define $\operatorname{Around}(p,k)$, with $k$ meaning “skip diagonal piston bases,” as the following sequence for each $f$ in $F$:

```text
q := p + f
U(B(q), q, opposite(f))
if B(q + U) is not an observer and (not k or B(q + U) is not a piston base):
    U(B(q + U), q + U, D)
if B(q + D) is not an observer and (not k or B(q + D) is not a piston base):
    U(B(q + D), q + D, U)
```

`update_surrounding_blocks(p)` is $\operatorname{Around}(p,1)$.
`on_torch_state_change(p)` is $\operatorname{Around}(p,0)$.

The six primary neighbors are always updated, including observers. Observers in either additional vertical-diagonal position are skipped for both values of $k$. Piston bases in those additional positions are also skipped when $k=1$; piston heads are not excluded by that test.

The additional calls are power rechecks through a neighboring cell, not shape changes to a block watched by a diagonal observer. Sending a qualifying callback to that observer could schedule an early pulse and quasi-power a piston before the intended movement. This exclusion belongs to this notification procedure; it does not change the observer trigger predicate in section 12.1.

### 4.3 Dust-neighborhood notifications

`update_wire_neighbors(p)` executes

```text
for f in F:
    q := p + f
    U(B(q), q, opposite(f))
    for g in F:
        r := q + g
        if the live B(r) is not an observer:
            U(B(r), r, opposite(g))
```

This is six first-neighbor calls and at most thirty-six second-neighbor calls, subject to nested mutations. An observer watching the changed dust can receive a first-neighbor callback. Observers are excluded from the second-neighbor power rechecks, which must not pulse an observer watching an unchanged intervening cell. Each exclusion tests the live block at that point in the sequence; repeated positions are not eliminated.

This procedure is distinct from the Wire Turbo walk used after a dust **power** change. Changing dust **shape**, using a dot/cross, placement, or destruction can invoke this explicit neighborhood procedure.

### 4.4 Diode output notifications

For a repeater whose stored facing is $f$, `on_state_change` first sets $q=p+\bar f$, then calls

$$
U(B(q),q,f),\qquad
\text{followed by }U(B(q+g),q+g,g)\text{ for each }g\in F.
$$

The comparator's private `on_state_change` uses **$\bar f$** for the first call instead:

$$
U(B(q),q,\bar f),\qquad
\text{followed by }U(B(q+g),q+g,g)\text{ for each }g\in F.
$$

This difference is present in the source and is retained in the specification. It can be visible to direction-sensitive callbacks; it must not be normalized into a single generic diode-notification rule.

### 4.5 Observer output notifications

For observer facing $f$, let $q=p+\bar f$. Call $U(B(q),q,f)$, then, for every $g\in F$ except $g=f$, call

$$
U(B(q+g),q+g,\bot).
$$

The excluded face leads back to the observer. Secondary callbacks carry no direction.

### 4.6 Piston notifications

Define $\operatorname{Shape}(p)$ by visiting $f\in F_{\mathrm{shape}}$ in order:

```text
q := p + f
G(B(q), q, f)
if the live B(q) is an observer:
    U(B(q), q, opposite(f))
```

Define $\operatorname{Notify}(p)$ as $\operatorname{Shape}(p)$ followed by, for each $f\in F_{\mathrm{piston}}$, a call $U(B(p+f),p+f,\bar f)$ if the live neighbor is not an observer.

Thus piston movement explicitly notifies observers through ordered shape/state changes and excludes them from its subsequent ordinary power callbacks. The shape callback receives $f$ while the observer callback receives $\bar f$. After $G$ and its nested work return, the observer test rereads the live neighbor; it does not use the block supplied to the shape callback.

## 5. Scheduled ticks and the game-tick transition

### 5.1 Time units and priorities

One interpreter game tick advances the scheduler cursor by one bucket. The API name `schedule_half_tick` denotes a delay in **game ticks**. The API `schedule_tick` denotes a delay in **redstone ticks**, with

$$
1\text{ redstone tick}=2\text{ game ticks}.
$$

The logical simulation does not require a fixed wall-clock duration per tick. Server TPS settings determine its pacing.

The ordered priorities are

$$
\mathrm{Highest}=0<\mathrm{Higher}=1<\mathrm{High}=2<\mathrm{Normal}=3.
$$

Smaller numbers execute first. `NanoTick` is an alias for `Normal`, not a fifth priority.

### 5.2 Exact scheduler representation

The scheduler has $32$ buckets. Each bucket has four FIFO sequences:

$$
Q=(Q_{h,\pi})_{0\le h<32,\ 0\le\pi<4}.
$$

An entry is a pair $(p,\tau)$, where $\tau$ is the expected **block type**. Scheduling at current cursor $c$, game delay $\Delta$, priority $\pi$ appends to

$$
Q_{(c+\Delta)\bmod32,\pi}.
$$

For supported scheduler use, $0\le\Delta<32$; delay zero requires `NanoTick`/`Normal`. These constraints are debug assertions in the generic scheduler. Its ring is not an unbounded timestamp queue: larger delays alias modulo $32$ if they reach it without an assertion failure.

### 5.3 Deduplication and pending tests

The `PlotWorld` wrapper binds a new request to $\tau=\operatorname{type}(B(p))$. It inserts only when no equal pair $(p,\tau)$ exists **anywhere** in $Q$:

$$
\operatorname{Pending}(p;\Sigma)
\iff\exists h,\pi:(p,\operatorname{type}(B(p)))\in Q_{h,\pi}.
$$

Neither delay nor priority participates in equality. A later request cannot shorten, lengthen, or reprioritize the existing request of the same type at the same position. A stale request of another block type does not prevent scheduling the current type.

This deduplication belongs to `PlotWorld`; the generic `TickScheduler` itself just appends. Deserialized/imported queue entries can therefore include repetitions not generated by ordinary wrapper calls.

### 5.4 Dispatch and stale requests

To execute one scheduled operation, find the smallest $\pi$ with $Q_{c,\pi}\ne\varnothing$, remove its FIFO head $(p,\tau)$, read the live $b=B(p)$, and execute $T(b,p)$ only if

$$
\tau=\operatorname{type}(b).
$$

The entry is removed **before** the callback, so the callback may schedule another entry of the same type and position. A stale entry is consumed without dispatch.

The guard is type-based, not instance-based. Changing state properties preserves a request. Removing a block and replacing it with the same type can also leave its old request applicable. Position-bound requests do not follow a block carried by a piston.

Priority selection is repeated after every callback. A newly appended delay-zero request can execute during the same scheduled phase. Within each priority, FIFO order is preserved.

### 5.5 Phase state machine

The four phases are

$$
\phi\in\{\mathrm{BetweenTicks},\mathrm{ScheduledTicks},\mathrm{PistonEvents},\mathrm{MovingEntities}\}.
$$

`prepare_operation` performs administrative transitions until there is an operation or the current game tick finishes:

```text
repeat:
    if phase == BetweenTicks:
        t := t + 1
        scheduled_advanced := false
        phase := ScheduledTicks

    else if phase == ScheduledTicks:
        if Q[c] is nonempty: return operation_available
        if not scheduled_advanced:
            clear Q[c]
            c := (c + 1) mod 32
            scheduled_advanced := true
        else:
            phase := PistonEvents

    else if phase == PistonEvents:
        if A is nonempty: return operation_available
        L := [(motion.pos, motion.identity) for motion in M, in order]
        j := 0
        phase := MovingEntities

    else if phase == MovingEntities:
        if j < length(L): return operation_available
        L := []
        j := 0
        phase := BetweenTicks
        return tick_finished
```

The scheduled phase therefore drains the old current bucket **before** advancing the cursor, then drains the new current bucket. Draining the old bucket handles delay-zero work left by earlier phases or external actions.

`advance_operation` invokes this preparation and then executes exactly one of

| Phase | One operation |
| --- | --- |
| ScheduledTicks | Consume one scheduled entry; dispatch if its type matches |
| PistonEvents | Pop the FIFO head of $A$; validate and execute the piston event |
| MovingEntities | Consume $L[j]$, increment $j$, and attempt that identity's motion step |

All nested immediate updates within that operation complete before it returns. A piston event generated during scheduled ticks is available in the same game tick's event phase. Events generated during the event phase are appended and drained in that phase. Events generated during movement wait until the next game tick's event phase; the engine does not jump backward through phases.

### 5.6 What a full tick means

`tick_interpreted` repeatedly calls `advance_operation` until preparation returns `tick_finished`. From `BetweenTicks`, this advances one complete game tick. From a partially stepped tick, it completes the remaining portion of that tick.

An arbitrary callback's delay should be understood relative to the scheduler cursor, not just to $t$. Before and after the within-tick cursor advance, the same $t$ can coexist with different cursors. Likewise, a zero-delay request made after scheduled processing waits until a later scheduled phase. The phase-machine definition is the precise timing rule at these boundaries.

## 6. Dust geometry and the initial dust update

### 6.1 Connection predicate

Define $\operatorname{Connect}(b,f)$ for a horizontal direction $f$. It is true for

- dust, either torch type, comparator, redstone block;
- a recognized binary plate, modeled stone button or lever;
- modeled tripwire hook or target;
- a repeater precisely when its facing is $f$ or $\bar f$;
- an observer precisely when its facing is $f$.

All other cases are false. Comparator connections are not restricted by comparator facing. Tripwire hooks and targets are geometrical connections even though this interpreter does not implement a corresponding dynamic power source for them.

### 6.2 Raw side calculation

For dust at $p$, the raw side toward horizontal $f$ is

$$
\operatorname{RawSide}(p,f)=
\begin{cases}
\mathrm{Side},&\operatorname{Connect}(B(p+f),f),\\
\mathrm{Up},&\neg\operatorname{Solid}(B(p+U))\land B(p+f+U)\text{ is dust},\\
\mathrm{Side},&\neg\operatorname{Solid}(B(p+f))\land B(p+f+D)\text{ is dust},\\
\mathrm{None},&\text{otherwise}.
\end{cases}
$$

The cases are tested in this order. In particular, the raw upward side test does not require the immediate horizontal neighbor to be solid. Dust's graphical side relation and its power-transfer relation are separately defined procedures.

### 6.3 Side regulation

Let $w$ be the supplied wire state and $s_f=\operatorname{RawSide}(p,f)$ for all horizontal $f$. Define `dot` as all four sides being None, and `cross` as all four sides being Side. A state with an Up side is not a cross under this definition.

If both $w$ and the raw result are dots, return the raw dot. Otherwise calculate the following flags **from the unmodified raw result**:

$$
\begin{aligned}
n_f&=[s_f=\mathrm{None}],\\
n_{NS}&=n_N\land n_S,\\
n_{EW}&=n_E\land n_W.
\end{aligned}
$$

Then independently replace

$$
\begin{aligned}
s_N&:=\mathrm{Side}&&\text{if }n_N\land n_{EW},\\
s_S&:=\mathrm{Side}&&\text{if }n_S\land n_{EW},\\
s_E&:=\mathrm{Side}&&\text{if }n_E\land n_{NS},\\
s_W&:=\mathrm{Side}&&\text{if }n_W\land n_{NS}.
\end{aligned}
$$

Call this transformation $\operatorname{Regulate}(w,p)$. Its power field is unchanged. A non-dot isolated wire becomes a cross; one connected direction is extended into a straight line on that axis.

Primitive horizontal dust emission in section 3 tests the regulated side toward the physical receiver, $\bar f$, rather than just testing raw connectivity.

### 6.4 Placement, shape changes, and dot/cross use

Dust placement calculates initial strength using section 6.5, regulates sides, and changes any resulting dot into a cross.

The shape-change helper `on_neighbor_changed(w,p,d)` behaves as follows:

1. If $d=U$, return $w$ unchanged.
2. If $d=D$, return $\operatorname{Regulate}(w,p)$.
3. If $d$ is horizontal, first recompute the stored side **opposite $d$** and retain that single result as `new_side`; then regulate all four sides.
4. If the old state was a cross and `new_side` is None, return the entire old state.
5. Otherwise, if the old state was not a dot and the new regulated state is a dot, convert the result to a cross, retaining power.

User use is permitted only for a dot or a cross. It builds the opposite form, retains the old power, regulates sides, and writes/notifies through `update_wire_neighbors` only if the resulting state differs.

There is no universal rule that every neighboring storage write recalculates all dust sides. It happens through the explicit shape/use procedures.

### 6.5 Live-world initial power calculation

For any position $q$, define

$$
\operatorname{DustPower}(q)=
\begin{cases}
P(q),&B(q)\text{ is dust},\\
0,&\text{otherwise}.
\end{cases}
$$

The non-dust external input at a wire cell is

$$
b(p)=\max_{f\in F}R_0(B(p+f),p+f,f).
$$

The initial helper's candidate dust neighborhood is

$$
\begin{aligned}
\mathcal N_{\mathrm{seed}}(p)={}&\{p+f:f\in F\}\\
&\cup\{p+f+U:f\in\mathcal H,\ \neg\operatorname{Solid}(B(p+U)),\
\neg\operatorname{Transparent}(B(p+f))\}\\
&\cup\{p+f+D:f\in\mathcal H,\ \neg\operatorname{Solid}(B(p+f))\}.
\end{aligned}
$$

Define

$$
\operatorname{SeedPower}(p)=\max\left(b(p),
\left(\max_{q\in\mathcal N_{\mathrm{seed}}(p)}\operatorname{DustPower}(q)\right)\mathbin{\dotminus}1\right).
$$

Every ordinary dust neighbor update $U(w,p,d)$ calculates this value from the **live world**. The direction $d$ is not used by this power-update helper. If it differs from the supplied wire's power, the helper writes the supplied state with the new power and starts a Wire Turbo walk at $p$. If it is equal, it does nothing.

The supplied wire argument can itself be cached by a calling procedure. The power calculation nevertheless reads live neighbors.

## 7. The exact Wire Turbo propagation procedure

### 7.1 Why it needs a separate definition

Wire Turbo is a finite, ordered work-queue procedure executed synchronously inside the initiating dust callback. It does not consume scheduler ticks. Its “layers” represent work ordering, not game ticks, redstone ticks, or public nano steps.

It differs from the initial helper in two ways: it uses per-walk cached states and a heading-dependent queue order, and its dust-neighbor selection is not identical to $\mathcal N_{\mathrm{seed}}$. These distinctions are part of the implemented model.

### 7.2 Canonical neighborhood

Each walk lazily constructs nodes for positions. A node at $p$ contains

$$
n_p=(p,\widehat B_p,\widehat F_p,v_p,x_p,z_p,\lambda_p,\mathcal O_p,\mathcal D_p),
$$

where $\widehat B_p$ is the block state read when the node is first created, $\widehat F_p$ its cached solid/transparent/update facts, $v_p$ the visited bit, $(x_p,z_p)$ heading biases, $\lambda_p$ its greatest assigned layer, and $\mathcal O_p,\mathcal D_p$ its oriented and direct neighbor lists.

New nodes have $v_p=0$, $(x_p,z_p)=(0,0)$, $\lambda_p=0$, and unidentified neighbor lists. The canonical neighbor offsets $a_0,\ldots,a_{23}$ are

| Index | Offset | Index | Offset |
| --- | --- | --- | --- |
| 0 | $W$ | 12 | $E+D$ |
| 1 | $E$ | 13 | $E+U$ |
| 2 | $D$ | 14 | $E+N$ |
| 3 | $U$ | 15 | $E+S$ |
| 4 | $N$ | 16 | $2D$ |
| 5 | $S$ | 17 | $D+N$ |
| 6 | $2W$ | 18 | $D+S$ |
| 7 | $W+D$ | 19 | $2U$ |
| 8 | $W+U$ | 20 | $U+N$ |
| 9 | $W+N$ | 21 | $U+S$ |
| 10 | $W+S$ | 22 | $2N$ |
| 11 | $2E$ | 23 | $2S$ |

These $24$ cells are the first and second Manhattan-distance neighbors, excluding the center. Creating or identifying these nodes is not itself a redstone callback.

### 7.3 Heading selection

When the neighbors of $p$ are first identified, collect their current visited bits $v_i$ and set

$$
\begin{aligned}
V_W&=v_0\lor v_7\lor v_8,&V_E&=v_1\lor v_{12}\lor v_{13},\\
V_N&=v_4\lor v_{17}\lor v_{20},&V_S&=v_5\lor v_{18}\lor v_{21},\\
c_x&=[V_W]-[V_E],&c_z&=[V_N]-[V_S].
\end{aligned}
$$

Define the heading function $h(x,z)$ by the lookup table

| $(x,z)$ | Heading |
| --- | --- |
| $(-1,-1)$ | North |
| $(0,-1)$ | North |
| $(1,-1)$ | East |
| $(-1,0)$ | West |
| $(0,0)$ | West |
| $(1,0)$ | East |
| $(-1,1)$ | South |
| $(0,1)$ | South |
| $(1,1)$ | South |

If $(c_x,c_z)=(0,0)$, use $h(x_p,z_p)$ and copy $(x_p,z_p)$ into every canonical neighbor's bias. Otherwise, if both $c_x$ and $c_z$ are nonzero, set $c_z:=0$ when $x_p\ne0$, then set $c_x:=0$ when $z_p\ne0$. Use $h(c_x,c_z)$ and copy the resulting pair to every canonical neighbor's bias.

This is an ordered computation with mutation of neighbor metadata. Biases are not an electrical direction vector and are not chosen by randomness.

### 7.4 Exact orientation permutations

The oriented neighbor sequence is $\mathcal O_p[i]=n_{p+a_{\rho_h[i]}}$, using

```text
North:  2, 3,16,19, 0, 4, 1, 5, 7, 8,17,20,12,13,18,21, 6, 9,22,14,11,10,23,15
East:   2, 3,16,19, 4, 1, 5, 0,17,20,12,13,18,21, 7, 8,22,14,11,15,23, 9, 6,10
South:  2, 3,16,19, 1, 5, 0, 4,12,13,18,21, 7, 8,17,20,11,15,23,10, 6,14,22, 9
West:   2, 3,16,19, 5, 0, 4, 1,18,21, 7, 8,17,20,12,13,23,10, 6, 9,22,15,11,14
```

The direct list is always $(n_{p+U},n_{p+D},n_{p+N},n_{p+S},n_{p+E},n_{p+W})$, regardless of heading. Once identified, a node's neighbor orientation is retained for the rest of that walk.

### 7.5 Which nodes have callbacks

The cached `updates` fact is true precisely for dust, either torch, repeater, comparator, lamp, hopper, iron trapdoor, piston base, piston head, observer, note block, and recognized command-block states. All other nodes remain part of the geometry/heading graph but are omitted from callback queues.

Thus air and inert solid cubes can influence sampled geometry or conduction without receiving a redstone update callback.

### 7.6 Layer queues and enqueue rule

Maintain three ordered sequences $(V_0,V_1,V_2)$ and a current walk layer $k$. For a changed node $p$ at layer $\ell$, first identify its neighborhood if necessary, then perform:

```text
for n in O[p], in order:
    if ell + 1 > n.layer:
        n.layer := ell + 1
        if n.updates: append n to V1

for n in the first four entries of O[p], in order:
    if ell + 2 > n.layer:
        n.layer := ell + 2
        if n.updates: append n to V2
```

The first four oriented neighbors are always $D,U,2D,2U$. They can receive both a next-layer and a following-layer entry.

The rule suppresses assignments that would not increase a node's recorded layer; it does not remove earlier queued entries. There is no later “skip if this entry's layer is old” guard. A node may receive multiple callbacks at different queue positions.

### 7.7 Walk execution

The initiating node is constructed **after** the seed's changed power has been written, marked visited, and propagated at layer $0$. Initially all queues are empty. Then shift the queues

$$
(V_0,V_1,V_2)\leftarrow(V_1,V_2,\varnothing),\qquad k:=1.
$$

While $V_0$ or $V_1$ is nonempty, process the original length of $V_0$ in order:

- A cached dust node is marked visited, recalculated by section 7.8, and propagated at layer $k$ if its cached power changes.
- Another node executes $U(\widehat B_p,p,\bot)$ with its cached block argument.

Then shift the three queues again and increment $k$. Propagation appends only to the next two layers; it does not modify the current layer's original iteration sequence.

### 7.8 Turbo dust recalculation

Let $\widehat P(q)$ be cached dust power, or $0$ for a cached non-dust state. Read the six direct cached neighbors to compute

$$
\widehat b(p)=\max_{f\in F}R_0(\widehat B_{p+f},p+f,f;\Sigma).
$$

The explicit neighbor state is cached. The $R_0$ call can still inspect live comparator entities and live neighbors of solid cells.

If $\widehat b(p)<15$, calculate candidate dust strength over

$$
\begin{aligned}
\mathcal N_{\mathrm{turbo}}(p)={}&\{p+f:f\in\mathcal H\}\\
&\cup\{p+f+D:f\in\mathcal H,\ \neg\operatorname{Solid}(\widehat B_{p+f})\}\\
&\cup\{p+f+U:f\in\mathcal H,\ \operatorname{Solid}(\widehat B_{p+f}),\
\neg\operatorname{Transparent}(\widehat B_{p+f}),\
\neg\operatorname{Solid}(\widehat B_{p+U})\}.
\end{aligned}
$$

Then

$$
P'(p)=\max\left(\widehat b(p),
\left(\max_{q\in\mathcal N_{\mathrm{turbo}}(p)}\widehat P(q)\right)\mathbin{\dotminus}1\right).
$$

If $\widehat b(p)=15$, the dust-neighbor scan is skipped and $P'(p)=15$.

When power differs, write the cached wire state with power $P'(p)$ to the live world, update the node's cached power to $P'(p)$, and propagate its change. Its cached geometry facts and stored wire sides remain unchanged by this power-only update.

Important distinctions from the initial helper are

- Turbo does not include directly vertical dust cells in its dust-power maximum.
- Turbo's downward and upward diagonal tests form an `if`/`else if`; the upward case requires a solid, nontransparent horizontal neighbor.
- Turbo reads cached dust powers that are updated in walk order, not a simultaneous snapshot of all dust for every layer.
- Non-dust callbacks receive cached states even when preceding callbacks have changed the live state. The note-block callback explicitly rereads its live powered flag; not every component does this.

### 7.9 Recursive walks and cache lifetime

A callback can initiate another dust walk. The implementation removes reusable scratch storage from its thread-local slot before executing callbacks, so a recursive walk obtains independent node/queue state. It runs to completion on the same live world before the outer callback resumes.

After a walk, its cached block states, orientations, visited bits, and queues are discarded. Persistent topology caches contain canonical addresses only; generation-stamped scratch maps do not reuse live block snapshots across walks. Clearing interpreter caches therefore does not cancel scheduled ticks, piston events, or motion records.

## 8. Torches

### 8.1 Desired state

For a standing torch at $p$, define

$$
\operatorname{Off}_{\mathrm{standing}}(p)
=[R_1(B(p+D),p+D,U)>0].
$$

For a wall torch with facing $f$, its attachment cell is $p+\bar f$, and

$$
\operatorname{Off}_{\mathrm{wall}}(p,f)
=[R_1(B(p+\bar f),p+\bar f,\bar f)>0].
$$

The two side arguments above reproduce the literal helper calls. Let $x$ be the applicable Off predicate and $\ell$ the supplied lit state.

### 8.2 Update and tick transitions

On neighbor update, request $(\Delta=2,\pi=\mathrm{Normal})$ exactly when

$$
\ell=x\quad\land\quad\neg\operatorname{Pending}(p).
$$

Equivalently, the current lit state differs from the desired lit state $1-x$.

On scheduled tick, recompute $x$ from the current world. If $\ell\ne1-x$, write $\ell:=1-x$ and run $\operatorname{Around}(p,0)$. Otherwise do nothing.

This is a delayed recheck. If a contradictory input disappears before the callback, the callback may leave the torch unchanged. No burnout history or repeated-toggle suppression is implemented by these torch handlers.

## 9. Repeaters

### 9.1 Rear input and side locking

For repeater facing $f$, let

$$
i=[D(p,f)>0].
$$

Define side-diode power

$$
J(p,g)=
\begin{cases}
W_0(B(p+g),p+g,g),&B(p+g)\text{ is repeater or comparator},\\
0,&\text{otherwise}.
\end{cases}
$$

The desired lock is

$$
l^*=[\max(J(p,r(f)),J(p,r^{-1}(f)))>0].
$$

Only a correctly oriented powered side diode can lock a repeater. Dust and a redstone block beside it do not directly satisfy this lock predicate. A comparator locks through its entity output, including any transient disagreement between that output and its block-state flag.

### 9.2 Neighbor update

Given supplied state $(f,d,l,q)$:

1. If $l\ne l^*$, immediately write the repeater with $l:=l^*$.
2. If the resulting $l=0$, there is no pending request, and $i\ne q$, schedule a tick with game delay $2d$.

The request priority is

$$
\pi=
\begin{cases}
\mathrm{Highest},&B(p+\bar f)\text{ is a diode},\\
\mathrm{Higher},&i=0,\\
\mathrm{High},&i=1.
\end{cases}
$$

Here a diode means repeater or comparator, irrespective of its own orientation. The output-neighbor diode condition takes precedence over whether the request is for powering or depowering.

Lock state changes do not invoke the diode-output notification procedure, because locking by itself does not change the repeater's signal output.

### 9.3 Scheduled tick

The transition is

| Current state | Rechecked input $i$ | Action |
| --- | --- | --- |
| $l=1$ | any | Return without changing output |
| $l=0,q=1$ | $0$ | Set $q:=0$ and notify diode output |
| $l=0,q=1$ | $1$ | No change |
| $l=0,q=0$ | $1$ | Set $q:=1$ and notify diode output |
| $l=0,q=0$ | $0$ | Schedule another tick with delay $2d$, priority Higher; set $q:=1$ and notify diode output |

The last row is essential: a queued tick on an unpowered repeater turns it on even if the input has disappeared. This preserves a short activation as an output pulse, then schedules the falling edge after the selected delay.

The tick reads the stored lock bit; it does not independently recalculate side locking. Locking is updated by neighbor callbacks. If a pending activation is consumed while locked, it is not retried by the tick itself; a later unlocking update can schedule the then-required transition.

### 9.4 Delay selection and pulse semantics

Using a repeater cycles its delay by

$$
d':=1+(d\bmod4).
$$

An existing request retains its original bucket. If a new delay is selected before that request executes, the current delay is used by any subsequent request made inside the tick callback.

For fixed delay $d$, no locking, and a short input that generates an activation request then disappears before execution, the output remains high for $2d$ scheduler advances after its rising callback. A repeater is therefore not exactly a pure transport delay $q(t)=i(t-2d)$, nor a conventional inertial delay that discards all shorter pulses.

## 10. Comparators and analog overrides

### 10.1 Override availability and values

Let $\operatorname{Override}(b)$ mean that $b$ is a modeled barrel/furnace/hopper/cauldron/composter/cake, a recognized container type, an end portal frame, or a command block. Its value is

| Block/entity | Override $V(b,p)$ |
| --- | --- |
| Recognized container with a container entity | Stored `comparator_override` |
| Recognized container with missing/wrong entity | $0$ |
| Cauldron with level $h$ | $h$ |
| Composter with level $h$ | $h$ |
| Cake with bites $b$ | $14-2b$ |
| End portal frame | $15$ if property `eye=true`, else $0$ |
| Command block with command entity | $\min(15,\max(0,\mathrm{success\_count}))$ |
| Command block with missing/wrong entity | $0$ |

`ContainerType::from_block` defines the recognized container set. It is not inferred from the existence of an arbitrary block entity.

### 10.2 Container fullness

For $n>0$ container slots, let $c_j$ be item count and $m_j$ the item-specific maximum count for an occupied slot. Empty slots contribute zero. The ordinary inventory-update formula is

$$
V_{\mathrm{inventory}}=
\min\left(15,
\left\lfloor\frac{14}{n}\sum_{j\ \mathrm{occupied}}\frac{c_j}{m_j}\right\rfloor
+[\text{at least one occupied slot}]\right).
$$

The implementation evaluates the sum, division, multiplication, and floor using `f32`. Rational arithmetic gives the conceptual formula but is not a promise of bit-identical results at rounding boundaries. Imported inventory data also calculates a stored override during decoding; a comparator subsequently reads the stored field rather than recalculating fullness on every query.

Changes to inventory contents update that field and notify around the container and around adjacent solid cells, so comparators reading through a solid intermediary are rechecked.

### 10.3 Rear input and far override

For comparator facing $f$, set $a=D(p,f)$, $q=p+f$, and $r=p+2f$. Its actual rear strength is

$$
I(p,f)=
\begin{cases}
V(B(q),q),&\operatorname{Override}(B(q)),\\
V(B(r),r),&a<15\land\operatorname{Solid}(B(q))\land\operatorname{Override}(B(r)),\\
a,&\text{otherwise}.
\end{cases}
$$

A direct override replaces ordinary input unconditionally. A far override is considered only through a solid direct neighbor and only if ordinary rear strength is below $15$. A far override replaces the base value; it is not joined with it by maximum.

### 10.4 Side input

Define

$$
C_{\mathrm{side}}(p,g)=
\begin{cases}
W_0(B(p+g),p+g,g),&B(p+g)\text{ is a diode},\\
P(p+g),&B(p+g)\text{ is dust},\\
15,&B(p+g)\text{ is a redstone block},\\
0,&\text{otherwise}.
\end{cases}
$$

Then $s=\max(C_{\mathrm{side}}(p,r(f)),C_{\mathrm{side}}(p,r^{-1}(f)))$.

The side-input function does not use general solid conduction. A powered solid cube, lever, button, or torch is not itself one of these accepted side-source cases.

### 10.5 Desired output and powered flag

For mode $m$, rear strength $a=I(p,f)$, and side strength $s$, define

$$
o^*=
\begin{cases}
a,&m=\mathrm{Compare}\land a\ge s,\\
0,&m=\mathrm{Compare}\land a<s,\\
a\mathbin{\dotminus}s,&m=\mathrm{Subtract}.
\end{cases}
$$

The separately calculated desired powered flag is

$$
q^*=[a>0]\land\left([a>s]\lor[a=s\land m=\mathrm{Compare}]\right).
$$

For valid strengths this equals $[o^*>0]$. Compare mode passes a positive equal-strength rear input; subtract mode yields zero at equality.

### 10.6 Neighbor update and scheduled tick

An update returns immediately if $\operatorname{Pending}(p)$. Otherwise, if $o^*\ne o(p)$ or $q^*\ne q$, request a game delay of $2$ with priority High when $B(p+\bar f)$ is a diode and Normal otherwise.

At the tick, recompute $o^*$. Enter the mutation/notification body only when

$$
o^*\ne o(p)\quad\lor\quad m=\mathrm{Compare}.
$$

Within that body, write a comparator entity with output $o^*$, recompute $q^*$, change the block's powered flag if necessary, and invoke the comparator-specific diode-output notifications.

Thus Compare mode notifies even if the output strength is unchanged. Subtract mode with an unchanged output skips the entire body, including powered-bit correction. On well-formed settled states the bit and output agree, but the operational distinction matters for imported or temporarily inconsistent states.

Using a comparator toggles mode, immediately calls this tick procedure with the toggled supplied state, then writes the toggled comparator block state. It therefore includes an immediate reevaluation and preserves the literal write order of `interaction::on_use`.

## 11. Sources, immediate consumers, and note blocks

### 11.1 Constant and externally controlled sources

A redstone block has no dynamic activation state. Its primitive output is fixed by section 3.

A lever's use transition is $q':=1-q$. Write its state, notify $\operatorname{Around}(p,1)$, then notify around its attachment cell: $p+D$ for a floor lever, $p+U$ for a ceiling lever, or $p+\bar f$ for a wall lever. This second notification exposes changes to receivers powered through the attachment block.

A stone button can be activated only when unpowered. Activation sets $q:=1$, requests a game delay of $20$ with Normal priority, and notifies around the button and its attachment cell. Further uses while it is already powered do not extend its existing request. Its scheduled callback sets $q:=0$ if currently powered, emits the click-off sound, and repeats these notifications.

Recognized binary plates are driven by player movement in the plot host. Moving onto an unpowered plate while on the ground activates it. Moving away from a powered plate deactivates it if no qualifying grounded player remains at that block position. Spectators and players disallowed by the relevant interaction checks do not participate. The state write is followed by notifications around the plate and the cell below it. There is no plate-specific release timer in the redstone tick dispatcher.

These are explicit external state transitions. The interpreter does not derive button, lever, or plate state from the electrical input predicate $H$.

For registry-backed plates, their recognition in $W$ and $S$ coexists with the metadata-based selection of $R$. In particular, generic `Unknown` plate states are solid under this checkout's fallback metadata. The exact formulas, rather than an assumption that all plate species are electrically interchangeable, determine their general-query behavior.

### 11.2 Lamps

Let $\ell$ be the supplied lamp lit state and $h=H(p)$. The update rule is

$$
U_{\mathrm{lamp}}:
\begin{cases}
\text{request }(\Delta=4,\mathrm{Normal}),&\ell=1,h=0,\\
\ell:=1\text{ immediately},&\ell=0,h=1,\\
\text{no change},&\text{otherwise}.
\end{cases}
$$

At the scheduled callback, recompute $h$ and set $\ell:=0$ only if $\ell=1$ and $h=0$. No general neighbor-notification procedure is called by a lamp's lit-state write in these handlers.

The first off request remains pending if input briefly returns. Subsequent off requests are deduplicated, and the eventual callback examines current input. The rule is a delayed check after the first requested falling edge, not a timer that necessarily restarts on every falling edge.

### 11.3 Hoppers and iron trapdoors

On each hopper neighbor update, set

$$
e':=1-H(p)
$$

if its supplied enabled state differs. On each modeled iron-trapdoor update, set

$$
q':=H(p)
$$

if its supplied powered state differs, retaining facing and half. These writes are immediate and do not invoke output notifications.

The redstone handlers define the hopper's enabled flag, not an autonomous inventory-transfer simulation. The modeled trapdoor stores a coupled powered/open representation rather than an independently simulated opening mechanism.

### 11.4 Note blocks

Let $h=H(p)$. Unlike most cached-argument handlers, the note-block update rereads the **live block's powered bit** $q$ before deciding whether to change it. If $q=h$, do nothing. Otherwise:

1. Recompute the instrument from the current block below.
2. Retain the supplied note value $n$.
3. If $h=1$ and the block above is exactly modeled air, emit that instrument's note.
4. Write the note block with powered bit $h$.

Thus electrical sound is emitted on a rising powered-state transition only, and an obstructed note block still updates its powered bit. Removing the obstruction while it remains powered does not by itself create a new rising transition.

The pitch table approximates

$$
\operatorname{pitch}(n)=2^{(n-12)/12},\qquad 0\le n\le24,
$$

using checked-in `f32` constants. Sound category is Records, volume is $3$. Out-of-range notes are ignored by the sound helper.

The instrument table is a literal variant match:

| Block below | Instrument |
| --- | --- |
| Stone, coal block, quartz, sandstone, concrete, terracotta | Basedrum |
| Sand | Snare |
| Glass or stained glass | Hat |
| Modeled sign, note block, barrel, composter | Bass |
| Clay | Flute |
| Gold block | Bell |
| Wool | Guitar |
| Packed ice | Chime |
| Bone block | Xylophone |
| Iron block | Iron xylophone |
| Soul sand | Cow bell |
| Pumpkin | Didgeridoo |
| Emerald block | Bit |
| Hay block | Banjo |
| Glowstone | Pling |
| Other variants | Harp |

This table does not infer material categories for every registry block. Manual use changes $n$ to $(n+1)\bmod25$ and, if unblocked, plays the new note using the instrument supplied by the current block state.

### 11.5 Inert or partially represented components

Modeled targets and tripwire hooks can affect dust connection geometry, but their redstone handlers do not implement projectile pulses or tripwire activation. Ordinary blocks, signs, and unsupported component variants have no dynamic redstone update or tick handler unless a special named-registry path recognizes them.

Such blocks may still support attachments, conduct strong input when solid, be comparator overrides, or be transported by a piston. Being inert in the dispatcher does not imply being geometrically or electrically irrelevant.

## 12. Observers

### 12.1 Trigger predicate

For supplied observer state $(f,q)$, a neighbor update requests $(\Delta=2,\mathrm{Normal})$ when there is no matching pending request and

$$
\operatorname{Trigger}(f,q,d)=
\begin{cases}
[d=f],&d\ne\bot,\\
q,&d=\bot.
\end{cases}
$$

The $d\ne\bot$ branch tests direction without testing whether the observer is currently powered. The $d=\bot$ branch is a powered-state recovery/recheck path.

An observer stores no previous observed block-state ID and performs no before/after equality comparison in this dispatcher. Its input is the actual sequence of direction-bearing callbacks delivered by the notifying procedures.

The observer exclusions in sections 4.2 and 4.3 prevent particular indirect power notifications from reaching this predicate. A direct qualifying callback can still trigger a pulse without a block-state comparison. In particular, a dust shape change can notify an observer adjacent to that dust without notifying an observer watching an unchanged cell one step farther away.

### 12.2 Scheduled transition and pulse

At a scheduled callback,

$$
q':=1-q.
$$

If the old $q=0$, also request another $(\Delta=2,\mathrm{Normal})$ callback. Then invoke the observer-output notifications from section 4.5.

An idle unpowered observer that receives one qualifying notification rises after two scheduler advances and falls after two more. A matching pending request coalesces additional notifications. A notification arriving while powered normally finds the off request already pending and cannot add another request.

This is an event-driven pulse generator. It does not sample every adjacent block every game tick, and it does not distinguish a redundant qualifying callback from one associated with an actual state change.

### 12.3 Observers during movement

A moving observer is represented by a moving-piston state, not by an active observer state. It emits no observer power while carried. On restoration, a powered observer is reset to unpowered if no request for observer type is pending at the destination. That reset invokes observer-output notifications.

A matching observer request already at the destination is retained and can execute after restoration. Requests at the old source cell remain position-bound and do not accompany the carried observer.

## 13. Piston power, requests, and event validation

### 13.1 Power predicate and quasi-connectivity

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

### 13.2 Requests are not scheduled block ticks

A piston update receives a supplied base state $(f,s,e)$, calculates $x=X(p,f)$, and returns if $x=e$.

If $x=1,e=0$, validate the forward payload line in section 14.1. If valid, request an Extend event. If invalid, request nothing.

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

### 13.3 Event validation

When $\epsilon$ is popped, read the live block at $p$:

- It must be a piston base.
- Its sticky flag must equal the captured sticky flag.
- Recompute $X$ using the **live facing**.
- Extend executes only when currently powered and unextended.
- Either retract action executes only when currently unpowered and extended.
- Extension also rescans the live payload line and can fail.

No additional guard requires the live facing to equal the captured event facing. Extension geometry uses the live facing. Retraction geometry also uses the live facing, but the retracted carried base's facing and the emitted block-action direction use the captured facing. This is the literal behavior for an intervening state mutation.

If validation rejects the event, it has still been consumed. Successful execution emits a piston block action carrying the event action and captured direction.

### 13.4 Legacy ticks and heads

A scheduled callback on a piston base simply calls the state-request procedure above. It is not a movement-completion callback or a cooldown.

A neighbor update on a piston head looks one cell opposite its facing; if that cell is a piston base, it rechecks that base. Ordinary moving-piston states have no redstone tick or update action. Their progress is advanced through the movement phase.

## 14. Piston payload transport and completion

### 14.1 Forward payload line

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

### 14.2 Creating a moving cell

Define $\operatorname{MoveCell}(r,b,f,s,e,u,E_b)$, where $b$ is a carried state, $e$ indicates extension, $u$ indicates a source head/base, and $E_b$ is an optional carried entity. It performs, in order:

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

### 14.3 Extension mutation order

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

### 14.4 Retraction mutation order

Let $f$ be the live base facing, $h=p+f$, and $r=p+2f$. Let $f_c$ be the event's captured facing.

1. If $B(h)$ is moving, interrupt-complete it using section 14.7.
2. Replace the base at $p$ by a retracting source moving cell carrying an unextended base with facing $f_c$ and the current sticky flag. The moving cell itself travels along $f$.
3. Run $\operatorname{Notify}(p)$.
4. Determine whether $r$ is a moving-piston cell with an extending entity whose direction is $f$.
5. Delete the entity at $h$ and write air at $h$.
6. If the base is sticky and $r$ is that matching in-flight extension, interrupt-complete $r$ and do not pull it. This applies even to an ordinary Retract event.
7. Otherwise, if the base is sticky and the action is Retract, inspect $B(r)$. If it is nonair, not moving, and not a recognized container, snapshot its entity, create a retracting nonsource moving payload at $h$, clear $r$ and its entity, and notify $r$.
8. Run $\operatorname{Notify}(h)$.

A nonsticky base does not pull. RetractWithoutPull suppresses the ordinary stationary-payload pull branch. The pull branch has no separate full extension-scan validation and does not reject every block excluded by Minecraft's general piston rules.

### 14.5 Movement snapshot and identity

After all currently queued piston events are consumed, the game tick snapshots

$$
L=((\mu_1.\mathrm{pos},\mu_1.\mathrm{identity}),\ldots,
(\mu_k.\mathrm{pos},\mu_k.\mathrm{identity}))
$$

in $M$'s current order.

When an entry $(r,i)$ is consumed, it acts only if the matching identity still exists at $r$. A replacement moving entity at the same position receives a new identity and is not advanced by stale work. If the block or entity at $r$ is no longer a moving piston, remove the old motion record and do nothing further.

Motions created or replaced after $L$ was taken wait until a later movement snapshot. Motions created in the preceding piston-event phase are included in the current tick's movement snapshot.

### 14.6 Exact progress recurrence

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

### 14.7 Normal and interrupted completion

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

### 14.8 Removing owned piston parts

Destroying an extended base also removes its owned stationary head or owned extending source moving head. Ownership requires matching direction and sticky flag. Destroying a stationary head removes its matching extended base. Destroying an extending source moving head can remove that base as well.

Transported nonsource payloads are independent. Removing the base/head does not retroactively erase unrelated payload motions, which may finish later. Deleting a source motion removes its identity, so its old completion cannot recreate a destroyed base or head.

## 15. Support geometry, placement, destruction, and use

### 15.1 Attachment support and validity

For support block $b$ and attachment face $f$, define

$$
\operatorname{Support}(b,f)=
\begin{cases}
0,&b\text{ is moving piston},\\
[g=f],&b\text{ is piston head facing }g,\\
[g\ne f],&b\text{ is extended piston facing }g,\\
\operatorname{Cube}(b),&\text{otherwise}.
\end{cases}
$$

For dust only, a moving-cell exception also supplies support: a retracting source moving base facing Down supports dust on its top face when its carried state is an unextended downward-facing piston. Moving heads and transported payloads do not provide stationary dust support.

Normal position validity is determined by these cases:

| Component | Required support |
| --- | --- |
| Dust | Dust-support predicate at $p+D$ |
| Repeater, comparator, standing torch, standing sign | $\operatorname{Support}(B(p+D),U)$ |
| Recognized binary plate | $\operatorname{Support}(B(p+D),U)$ |
| Wall torch/sign or tripwire hook facing $f$ | $\operatorname{Support}(B(p+\bar f),f)$ |
| Floor lever/button | $\operatorname{Support}(B(p+D),U)$ |
| Ceiling lever/button | $\operatorname{Support}(B(p+U),D)$ |
| Wall lever/button facing $f$ | $\operatorname{Support}(B(p+\bar f),f)$ |
| Piston head facing $f$, sticky $s$ | Matching extended base at $p+\bar f$, or a retracting source moving entity there facing $f$ |
| Other blocks | True |

For the extended-base head case, both facing and sticky flag must match. The alternative moving-entity test checks source/retracting flags and facing. Registry-backed ordinary signs use an oak standing/wall-sign support model with corresponding properties; hanging signs are excluded from that recognition.

`interaction::is_valid_position` first returns true if the trait method `World::is_cursed()` returns true. **In this checkout, `PlotWorld` does not override that method, whose default is false.** The separate `PlotWorld.is_cursed` field changed by `/curse` is therefore not a support bypass in these interpreter calls. A custom `World` can supply the bypass.

### 15.2 Shape callbacks

The shape callback $G(b,p,d)$ first checks validity. If invalid, destroy the component and return. If valid and the block is dust, apply section 6.4's shape transition; if the write changes the state, invoke `update_wire_neighbors(p)`. Other valid block types have no further shape action.

The general placement/destruction shape notifier visits each $f\in F$ and, in order, calls

$$
G(B(p+f),p+f,f),\quad
G(B(p+f+U),p+f+U,f),\quad
G(B(p+f+D),p+f+D,f).
$$

It passes $f$ directly, including to the vertical-diagonal calls. The piston shape notifier also passes $f$ to $G$, but visits only the six immediate neighbors in $F_{\mathrm{shape}}$ order. It then separately sends observer updates with $\bar f$, as defined in section 4.6.

### 15.3 Placement

Relevant ordinary item-placement defaults are:

- Torches start lit.
- Repeaters start unpowered with delay $1$ and locking calculated from current side diodes.
- Comparators start unpowered in Compare mode.
- Dust calculates current strength and regulated sides, using a cross for isolated placement.
- Lamps start lit according to $H(p)$; hoppers start enabled according to $1-H(p)$.
- Buttons, levers, binary plates, and observers start unpowered; pistons start unextended.
- Note blocks start at note $0$, instrument Harp, unpowered.

Ordinary diode placement facing is opposite the player's horizontal direction. Piston placement uses the opposite six-direction player-facing value; observer placement uses that six-direction value directly. Item placement checks support validity before committing the chosen state.

`place_in_world` performs, in order: import any applicable `BlockEntityTag`; write the block; update command-block activation if applicable; notify surrounding shapes; for dust invoke `update_wire_neighbors`; invoke $\operatorname{Around}(p,1)$.

The routine does not generally call $U$ directly on the placed component itself. Self-rechecks can nevertheless occur through its notification neighborhood, and some placement defaults already sample input.

### 15.4 Storage housekeeping and destruction

`set_block_raw` removes an old entity when a recognized container is replaced by a noncontainer, or when a moving-piston state is replaced by a different state ID. It also removes an existing sign entity when an ordinary sign block is replaced by a nonsign. A new recognized container receives an empty entity if an entity of the required container type is absent. A new ordinary sign receives a default sign entity if the current entity is absent or is not a sign entity; an existing sign entity and its text are retained across sign-state changes. The ordinary-sign predicate excludes hanging signs. This is not universal deletion of every incompatible entity on every raw write.

These entity checks are not contingent on the block-state write returning “changed.” A raw write of the same sign or container state can therefore repair a missing entity without changing $B$. The storage operation still does not issue general redstone or shape notifications.

Explicit destruction first removes owned piston counterparts and deletes the entity if the block's `has_block_entity` predicate holds. Then:

- Dust becomes air, invokes surrounding shape notifications, and invokes `update_wire_neighbors`.
- A lever becomes air and invokes shape/power notifications around its attachment cell.
- Other blocks become air, invoke surrounding shape notifications, and invoke $\operatorname{Around}(p,1)$.

Any removed piston counterpart receives $\operatorname{Notify}$ afterward. Cake use increments bites and notifies around the cake until final consumption destroys it. Barrel opening/closing changes its open state and issues surrounding power callbacks. These explicit actions can matter to callback-sensitive circuits even when primitive output is unchanged.

## 16. Command blocks

### 16.1 Activation and condition

Command blocks use registry-backed states named `command_block`, `repeating_command_block`, and `chain_command_block`. Their entity stores command text, custom name, success count $s$, powered state $q$, automatic flag $a$, latched condition $k$, last execution $\ell$, and flags controlling output and execution-time tracking.

Let $f$ be registry facing, defaulting to North for an unrecognized value. Define

$$
\operatorname{Condition}(p)=
\begin{cases}
1,&\text{conditional property is not true},\\
[B(p+\bar f)\text{ is command block with entity success count }>0],&\text{otherwise}.
\end{cases}
$$

This checks the cell behind the facing, which need not be the preceding visited cell in a turning chain.

Activation ensures a command entity exists, creating defaults if necessary. Compute $h=H(p)$ and rising activation $r=h\land\neg q$, then set $q:=h$. Define

$$
\operatorname{Start}=r\lor(a\land\ell<0),\qquad
\operatorname{Repeat}=[\text{repeating mode}]\land(h\lor a).
$$

If not chain mode, Start or Repeat holds, and no matching request is pending, latch $k:=\operatorname{Condition}(p)$ and schedule game delay $1$, Normal priority. This is one game tick, not one redstone tick.

### 16.2 Execution and chains

Let $t_c=\min(t,\mathrm{i64::MAX})$. Execution returns “not attempted” if the entity is missing or if execution-time tracking is enabled and $\ell=t_c$. Otherwise:

1. For chain mode, recompute the condition now; for nonchain mode, retain the activation-latched condition.
2. Set $s:=0$.
3. If the condition holds and the trimmed command is nonempty, call the world's command executor. Successful execution sets $s:=1$; failure leaves $s=0$.
4. Update output text only when output tracking is enabled. If execution-time tracking is enabled, set $\ell:=t_c$.
5. Write the resulting entity and invoke $\operatorname{Around}(p,1)$.

An attempt returns true even when the condition fails or the command returns an error. Success count and attempted execution are different observables.

A scheduled chain-block tick does nothing. A queued impulse/repeating activation still executes after electrical power disappears. If its attempt is suppressed, return. If its latched condition is false, skip the chain and recheck activation only for a repeating source.

Otherwise traverse at most **256** following cells, each time using the current block's facing. Stop at a nonchain block or missing command entity. Execute a powered or automatic chain block; stop if its attempt is suppressed. Skip an inactive chain block while continuing traversal from its position. A failed command with a true condition does not automatically stop traversal; a following conditional block can observe its zero success count. After traversal, a repeating source rechecks activation.

### 16.3 Host interface

The current `PlotWorld` command executor accepts supported `say` and `tellraw` commands through the repository's chat parser. It does not dispatch arbitrary world-editing commands.

Live output limits are $64$ queued outputs, $64$ accepted outputs per one-second window, and $65{,}536$ message bytes in that window. Commands longer than $131{,}068$ bytes or source names longer than $256$ bytes are rejected. The window follows wall-clock time, not $t$.

For complete reproduction of command success counts, extend $\Sigma$ with the output queue, window timestamp, counters, and elapsed-time observations. Without that host state, command-bearing traces are not solely a function of electrical state and logical ticks. Offline replay can disable output rate/queue limits; length restrictions and supported-command parsing remain.

## 17. Initialization, persistence, and stepping

### 17.1 Initialization and saved state

`Chunk::load` repairs missing entities for loaded ordinary sign blocks by inserting default sign entities. Existing entries at those positions are retained. This can make the loaded entity map differ from a legacy save that contained sign states without entities; it does not replay placement or redstone notifications. Chunk instance and revision counters used by neighboring-plot snapshots track storage changes, not electrical connections across plot boundaries.

`from_chunks` initially sets logical tick $0$ and phase BetweenTicks. It binds old requests lacking a block type to the currently loaded type once. Legacy requests on moving-piston states are discarded; legacy requests on bases become delay-zero Normal state-recheck requests.

Loaded moving entities are collected in lexicographic $(x,y,z)$ order and registered using progress decoded from their entities. This is a fallback for data without complete piston state. Loaded repeating command blocks receive an activation update. Normal plot loading subsequently restores a saved complete `PistonState` when persisted tick/identity/event/motion data indicate its presence, preserving exact progress, event order, phase, and partial movement snapshot.

Scheduled entries persist in cursor-relative bucket order, priority order, and FIFO order, retaining position, expected block type, remaining bucket distance, and priority. Reloading can rebase the physical ring while preserving relative delays.

History snapshots capture chunks/entities, scheduled entries, and `PistonState`. Restoration rebuilds them, invalidates interpreter lookup caches, resets screen tracking, and clears queued command messages. It does not replay emitted sound/chat/network effects or restore every host output-rate field.

Identical block maps with different requests, piston events, motion identities, or phases can have different futures. A simulation snapshot must preserve those fields.

### 17.2 Pico and nano stepping

`picotick_advance(n)` calls `advance_operation` $n$ times. Each call can execute one scheduled request, piston event, or motion item, with nested immediate callbacks completed. A call that administratively finishes an exhausted tick consumes an iteration without executing another component operation.

`nanotick_advance(n)` repeats: prepare the next operation; if preparation finishes the tick, consume this iteration; otherwise snapshot the current phase's remaining operation count and call `advance_operation` that many times. The count is current scheduler-bucket length, piston-event queue length, or remaining movement-snapshot length.

The snapshot is a **count**, not a selected operation list. Newly inserted immediately executable work is still selected by normal queue order; newly inserted higher-priority work can precede older work. Additional entries can remain after the snapshotted count is exhausted.

Nano stepping does not denote one priority, one dust-walk layer, or a fixed fraction of a game tick. Both fine-stepping APIs return without advancing when history recording is enabled.

### 17.3 Rendering

Static-piston and screen-only rendering can suppress or project client animation packets. They do not substitute static payload states into the electrical world or change motion progress. Simulation observations and rendered client observations are different projections of the execution.

## 18. Derived properties and limits of simpler models

### 18.1 Conditional determinism and boundedness

For fixed valid initial state, metadata, external-input order, and host outcomes, the procedures define an ordered trace

$$
\Sigma_0\xrightarrow{o_0}\Sigma_1\xrightarrow{o_1}\Sigma_2\xrightarrow{o_2}\cdots.
$$

Scheduled work has FIFO/priority order, events have FIFO order, movement uses an identity snapshot, and dust headings use deterministic tables. The trace depends on external-action order relative to these operations. “At the same tick” does not specify which side of the cursor advance or event phase an action occurs on.

Assuming strengths and overrides begin in $\mathcal S$, primitive emitters, maximum, saturating subtraction, and valid comparator arithmetic preserve $\mathcal S$. Generated dust/comparator outputs therefore stay in $\mathcal S$. This is a conditional invariant, not validation of every arbitrary raw entity write.

### 18.2 Dust equilibrium under frozen conditions

Freeze geometry, nondust outputs, comparator entities, and callback side effects. Choose one dust-neighborhood relation, such as $\mathcal N_{\mathrm{turbo}}$, and suppose every wire is evaluated with that relation until stable. Let wire vertices be $V$, with edge $u\to v$ when $u$ is an eligible predecessor of $v$.

For fixed external input $b_v$, equilibrium obeys

$$
P_v=\max\left(b_v,\max_{u\to v}(P_u\mathbin{\dotminus}1)\right).
$$

Let $d(u,v)$ be shortest directed edge distance, with $d(v,v)=0$ and unreachable distance infinity. The unique bounded equilibrium is

$$
P_v=\max_{u\in V}\max(0,b_u-d(u,v)),
$$

with unreachable terms zero. Existence follows by splitting shortest paths into the length-zero injection case and paths ending in an incoming edge. For uniqueness, repeatedly substitute the equilibrium equation: each inherited term loses one strength per edge. After $15$ substitutions, dependence on any initial value bounded by $15$ vanishes, leaving only source terms.

Consequently, a strength-$15$ wire injection remains positive across at most $14$ subsequent dust edges; sources join by maximum of attenuated strengths; a source-free cycle has only zero equilibrium. Vertical geometry can make the graph directed.

These results concern frozen equilibrium equations. They neither replace the seed/Turbo distinction nor prove that mixed callbacks are confluent or give every full circuit a unique stable state.

### 18.3 Boolean abstraction, quiescence, and termination

Mapping strength to $[P>0]$ helps describe threshold consumers but loses attenuation, analog comparison/subtraction, override values, and directionality. Even a strength-valued connectivity graph omits pending-event history: an off repeater with a queued activation can later produce a pulse while an otherwise identical off repeater does not.

Satisfying desired-state equations does not imply quiescence if queued callbacks or motions remain. Empty queues also do not imply that every desired state has been rechecked: a quasi-connected piston can remain unrechecked.

A quiescent state has no scheduled work, events, active motions, or immediate callbacks. With a fixed external environment, further ticks preserve its electrical state. No universal termination claim is made for arbitrary raw states or circuits. Oscillators deliberately generate future work, and bounded strength alone does not prove termination of compound callback/event sequences within a tick.

### 18.4 Scope of equivalence

This model does not assume Minecraft's general push reactions, twelve-block piston limit, adhesion, torch burnout, dynamic tripwire/projectile behavior, autonomous hopper transport, or general furnace processing.

Redpiler's optimized graph can merge or remove nodes. Equality of ordinary lamp outputs alone does not establish equality of observer callbacks, transient dust states, analog overrides, piston motions, or pending-request traces. Compiled execution needs a separate equivalence argument for the selected observables.

## 19. Worked traces

### 19.1 Short repeater pulse

Take an unlocked, initially off repeater with $d=2$ and no diode at its output. At a clean boundary, deliver a nonzero input and update, then remove the input and update before advancing. The existing delay-$4$ activation request is retained.

| Completed game tick | Output | Action |
| --- | --- | --- |
| Initial boundary | $0$ | Activation queued at High priority |
| 1, 2, 3 | $0$ | Await callback |
| 4 | $15$ | Power despite absent input; request delay $4$, Higher |
| 5, 6, 7 | $15$ | Await falling callback |
| 8 | $0$ | Recheck absent input and turn off |

The output lasts four game-tick intervals despite the brief input. The trace assumes no intervening lock, replacement, delay change, or feedback.

### 19.2 Comparator examples

For rear $a=9$ and side maximum $s=9$, Compare outputs $9$ and desires powered true; Subtract outputs $0$ and desires powered false. For $a=12,s=5$, Compare outputs $12$ and Subtract outputs $7$.

If ordinary rear strength through a solid neighbor is $10$ and the far container override is $3$, actual rear strength is $3$: the far override replaces the base value. At ordinary strength $15$, the far override is ignored.

### 19.3 Dust line

A frozen straight line injected at its first wire with strength $15$ has settled strengths

$$
P_k=\max(0,15-k),\qquad k=0,1,2,\ldots.
$$

The injected wire has $15$, its successor $14$, wire $14$ has $1$, and wire $15$ has $0$. Attenuation is per wire edge, not per elapsed tick. Removing the source causes an ordered immediate callback walk, not a simultaneous all-wire reset.

### 19.4 Observer pulse

An idle observer receiving $d=f$ at a clean boundary requests rise after game delay $2$. After completed tick $1$ it remains off; after tick $2$ it is on and has requested fall after delay $2$; after tick $3$ it remains on; after tick $4$ it is off.

A directionless notification while off does not trigger this pulse. A raw storage write to its watched neighbor need not trigger it unless the operation sends a qualifying callback.

### 19.5 Moving payload

An East-facing piston with one stone at $p+E$ and air at $p+2E$ queues Extend when powered and rechecked. The event creates a moving payload at $p+2E$, a moving head at $p+E$, and an extended base. That tick's movement phase advances the new motions to exact $1/2$, reporting serialized progress $0$. The next tick advances them to exact $1$, reporting $1/2$. The following movement phase restores stone/head.

An electrical-source payload emits no original source power while represented as moving. Its restoration can generate new work through self-update and notifications; work requested during movement follows the later-phase rules in section 5.

### 19.6 Different stale-work guards

A queued observer callback is discarded if its position contains stone when due. It can apply to a replacement observer of the same type, even with another facing. A stale movement item $(p,i)$ cannot apply to replacement identity $i'\ne i$ at that same position.

### 19.7 Dust shape observation versus an indirect power recheck

Place powered North-South dust at $p$ on stone, a redstone source at $p+N$, and an unchanged stone cell at $p+S$. Place one idle observer at $p+E$ facing West, so it watches the dust, and another at $p+S+U$ facing Down, so it watches the unchanged stone.

Destroy the source through the ordinary interaction path. The resulting dust shape change invokes `update_wire_neighbors(p)`. Its first-neighbor callback can schedule the observer watching the dust; its second-neighbor loop skips the observer watching the stone. The source's surrounding-update procedure also skips observers in its additional vertical-diagonal calls.

After destruction, the first observer has a pending rise request and the second has none. After two completed game ticks, the first is powered and the second remains unpowered. The observer dispatcher itself has not acquired state-change detection: the difference comes from which notifications the callers send. [observer_tests.rs](../crates/core/src/redstone/observer_tests.rs) constructs this case explicitly.

### 19.8 Two instant piston stages and their physical reset cycle

[EDGECASE_PISTION.schem](../test_data/EDGECASE_PISTION.schem) contains two initially extended South-facing sticky pistons. Each has a redstone-block payload two cells ahead, an observer immediately above the base facing Down, and a wool cap above the observer. The first output feeds the second stage through dust. Facing Down means the observer watches the base and emits upward through the cap, providing a quasi-connectivity reset path.

The [Java 1.21.5 reference](../test_data/piston-repair/java-piston-oscillator-trace.json) pastes the fixture strictly, settles eight game ticks, removes its external redstone source, and samples completed game-tick boundaries. Its first cycle is:

| Tick after source removal | Both piston bases | Both observers | Second-stage input dust |
| --- | --- | --- | --- |
| 0 | Extended | Unpowered | $15$ |
| 1 and 2 | Moving source bases | Unpowered | $0$ |
| 3 | Retracted | Powered | $0$ |
| 4 | Extended | Powered | $0$ |
| 5 | Extended | Unpowered | $0$ |
| 6 | Extended | Unpowered | $15$ |
| 7 and 8 | Moving source bases | Unpowered | $0$ |

Both stages begin retracting during the first game tick, so this falling computation wave does not add a game tick per stage. The reset and payload restoration span subsequent phases and ticks, and the fixture keeps cycling after source removal. Its valid computational abstraction therefore needs an observation point or input protocol; a permanently settled Boolean wire value does not describe its physical trace. The interpreter regression in [piston/tests.rs](../crates/core/src/redstone/piston/tests.rs) compares this reference using game, nano and pico stepping.

## 20. Implementation and regression-test index

| Subject | Source |
| --- | --- |
| Positions, offsets, face order, rotations | [blocks/lib.rs](../crates/blocks/src/lib.rs) |
| State/type IDs and geometry predicates | [blocks/mod.rs](../crates/blocks/src/blocks/mod.rs) |
| Component properties and instruments | [blocks/props.rs](../crates/blocks/src/blocks/props.rs) |
| Comparator/container/moving/command entities | [block_entities.rs](../crates/blocks/src/block_entities.rs) |
| World interface and hooks | [world/mod.rs](../crates/core/src/world/mod.rs) |
| Chunk storage and boundary reads | [world/storage.rs](../crates/core/src/world/storage.rs) |
| Signals, dispatch, notification helpers | [redstone/mod.rs](../crates/core/src/redstone/mod.rs) |
| Initial dust power and side regulation | [wire/mod.rs](../crates/core/src/redstone/wire/mod.rs) |
| Turbo snapshots, orientation, queues | [wire/turbo.rs](../crates/core/src/redstone/wire/turbo.rs) |
| Canonical geometry and cached facts | [world/wire_cache.rs](../crates/core/src/world/wire_cache.rs) |
| Walk lookup generations | [wire/turbo_cache.rs](../crates/core/src/redstone/wire/turbo_cache.rs) |
| Repeater state transitions | [repeater.rs](../crates/core/src/redstone/repeater.rs) |
| Comparator calculations | [comparator.rs](../crates/core/src/redstone/comparator.rs) |
| Piston events, transport, completion | [piston.rs](../crates/core/src/redstone/piston.rs) |
| Phase/event/motion types and priorities | [world/lib.rs](../crates/world/src/lib.rs) |
| Ring scheduler and priority FIFO queues | [backend/queue.rs](../crates/core/src/redpiler/backend/queue.rs) |
| Interpreter phases, stepping, loading | [plot/mod.rs](../crates/core/src/plot/mod.rs) |
| Placement, validity, destruction, use | [interaction.rs](../crates/core/src/interaction.rs) |
| Note sound and pitch table | [noteblock.rs](../crates/core/src/redstone/noteblock.rs) |
| Fullness and cake/barrel actions | [container.rs](../crates/core/src/container.rs) |
| Inventory-change notifications | [plot/containers.rs](../crates/core/src/plot/containers.rs) |
| Command lifecycle and chains | [command_block.rs](../crates/core/src/redstone/command_block.rs) |
| Host command parsing | [chat_commands.rs](../crates/core/src/chat_commands.rs) |
| History capture/restore | [history/codec.rs](../crates/core/src/plot/history/codec.rs) |

Existing regression evidence includes:

- The in-module tests in [redstone/mod.rs](../crates/core/src/redstone/mod.rs): Java output-pulse traces, the four UpdateTester wire traces, and the memory-cell piston-state trace.
- [redstone/master_tests.rs](../crates/core/src/redstone/master_tests.rs): repeater short pulses, torch direction, analog overrides, plate primitives, slab support, and stepped dust.
- [redstone/adder_tests.rs](../crates/core/src/redstone/adder_tests.rs): the sign-defined 11-stage adder interface, stored-input arithmetic cases, moving-output observations and the retained Java changed-input circuit limitation.
- [redstone/observer_tests.rs](../crates/core/src/redstone/observer_tests.rs): observer emission and conduction direction, and direct dust-shape observation without an indirect observer pulse.
- [redstone/piston/tests.rs](../crates/core/src/redstone/piston/tests.rs): six-direction movement, event cancellation, short pulses, long payloads, waterlogging, owned-part removal, progress, identities, and Java reference traces for observer feedback, periodic instant resets and dropped-payload recapture.
- [plot/piston_tests.rs](../crates/core/src/plot/piston_tests.rs): restart/partial-step state, cache clearing, stale work, and rendering independence.
- [plot/sign_tests.rs](../crates/core/src/plot/sign_tests.rs) and [world/storage.rs](../crates/core/src/world/storage.rs): sign-entity creation, replacement, persistence and repair of legacy loaded signs.
- [wire/turbo_tests.rs](../crates/core/src/redstone/wire/turbo_tests.rs): walk structure and traversal regressions.
- [plot/command_block_tests.rs](../crates/core/src/plot/command_block_tests.rs): activation, stale requests, conditional chains, bounded loops, and restart behavior.

These tests support their specific assertions. They do not establish exhaustive conformance for every circuit or input history.
