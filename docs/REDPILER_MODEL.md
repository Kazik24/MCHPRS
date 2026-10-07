# Redpiler execution model

This document defines the mathematical meaning of the current compiler and
Direct backend. [Redpiler.md](Redpiler.md) describes the architecture,
[REDPILER_PARSER.md](REDPILER_PARSER.md) describes extraction, and
[REDPILER_OPTIMIZER.md](REDPILER_OPTIMIZER.md) describes transformations and their
limits. Physical electrical and piston execution are defined separately in
[REDSTONE_MODEL.md](REDSTONE_MODEL.md) and [PISTON_MODEL.md](PISTON_MODEL.md).
Runnable evidence and fixture protocols are in [tests/README.md](tests/README.md).

Redpiler has an ordinary electrical graph and three region execution paths:
an acyclic response-wave adapter, a recognized shared-clock adapter, and a
notification-driven sequential adapter. Their temporal semantics differ.
`--assume-instant` selects a certified logical executor for the first two paths.
It rejects regions needing the notification-driven sequential adapter, rather
than executing movement and reset bookkeeping under the logical flag.
Compilation success establishes that the implementation can represent a region;
it does not establish universal equivalence to every physical input history.

## 1. State, time and observations

Let $\mathbb S=\{0,\ldots,15\}$ be electrical strengths and $\mathbb B=\{0,1\}$
Boolean values. Write $[P]$ for the indicator of proposition $P$, and define

$$
x\mathbin{\dotminus}a=\max(0,x-a),\qquad \operatorname{nz}(x)=[x>0].
$$

One step $n\to n+1$ is one Direct `tick()` or complete interpreter
`tick_interpreted()` advancement: one game tick. The scheduler calls this a
half-tick, meaning half a redstone tick. `schedule_tick(d)` advances the deadline
by $2d$ such steps; `schedule_half_tick(d)` advances it by $d$. An observer pulse
deadline of two steps and a torch delay of one redstone tick therefore use the
same interval. Timing below uses steps, including the six-step region cycle.

A compiled state is

$$
S_n=(G,\mathbf s_n,\mathbf b_n,\mathbf l_n,\mathbf p_n,Q_n,
     z_{1,n},\ldots,z_{r,n},E_n).
$$

Here $G$ is the fixed ordinary graph, $s_v$ is a node's output strength, $b_v$
its powered/display state, $l_v$ its repeater lock state, $p_v$ its pending-work
flag, $Q$ the ordered scheduler, $z_j$ each region's retained state, and $E$
external output events. Strength and powered state are separate fields: a wire
or inventory constant need not encode its strength through a powered flag.
The saved block presentation is also separate from these live values.

For an input action history $h$ and declared boundary observation map $O$, the
correctness obligation for a transformation $C$ is

$$
\forall h\in\mathcal P:\quad O(\operatorname{Reference}(S_0,h))
 =O(\operatorname{Execute}(C(S_0),h)).
$$

$\mathcal P$ specifies admitted initialization, input preparation, update/sample
actions, ordering, reset/reuse, and observation times. The reference can be the
ordinary interpreter or an explicitly idealized logical region. Choose it
before asserting equivalence. A final truth table alone cannot prove this
equation for a stateful consumer, reset episode or memory protocol.

A logical result port can additionally declare validity $v_o(n)$ and decoder
$D_o$. Its observation is $D_o(S_n)$ when $v_o(n)=1$ and $\bot$ otherwise.
An invalid observation is not Boolean zero. Validity belongs to the declared
result interface; an ordinary repeater or observer can still have an observable
electrical state during internal movement or reset. Thus a result-window check
and a complete consumer-waveform check establish different properties.

## 2. Ordinary electrical graph

The compile graph is a directed multigraph $G=(V,L)$.
An edge $e=(u,v,c,a)$ carries channel $c\in\{\mathrm{main},\mathrm{side}\}$ and
attenuation $a$. It contributes

$$
t_e=s_u\mathbin{\dotminus}a,\qquad
I_{v,c}=\max\bigl(\{t_e:e=(u,v,c,a)\in L\}\cup\{0\}\bigr).
$$

Sources join by maximum rather than sum. A strongest path through dust is a
minimum-attenuation path, so parallel paths from the same source to the same
channel can be represented by the least attenuation. Edges of attenuation at
least fifteen contribute zero for every valid source strength.

The graph bypasses dust when discovering real component inputs. Without
`--optimize`, retained wire nodes display the strongest extracted input;
Direct wire updates do not recursively propagate through a separate dust graph.
Dust position, strong/weak power, conductor behavior, component facing and
conditional wire shape are extraction concerns. Their effects become channels
and attenuations before ordinary execution.

The Direct backend stores a histogram for each channel:

$$
C_{v,c}[j]=\sum_{e=(u,v,c,a)}[s_u\mathbin{\dotminus}a=j],\qquad
I_{v,c}=\max\bigl(\{j:C_{v,c}[j]>0\}\cup\{0\}\bigr).
$$

When $s_u$ changes from $x$ to $y$, each outgoing edge decrements
$C_{v,c}[x\dotminus a]$ and increments $C_{v,c}[y\dotminus a]$.
If the attenuated values agree, that edge delivers no update. Otherwise the
backend reevaluates the receiving component. This is an incremental
implementation of the maximum equation, not an approximation.

[Direct lowering](../crates/core/src/redpiler/backend/direct/compile.rs)
requires strengths in $\mathbb S$ and at most 255 incoming edges per channel.
The packed forward-link representation requires attenuation below fifteen and
target indices below $2^{27}$. Node IDs belong to one fixed backend array;
unchecked runtime access relies on those construction invariants.

## 3. Ordinary state transitions and scheduler

`update_node` decides whether a change needs future work; `tick_node` applies
due work using the input at execution time. They are different transition
operators, $U_v$ and $T_v$. Replacing them with a static gate equation loses
pending pulses, locking and delayed releases.

An item in $Q$ has a deadline, priority, FIFO position and node identity.
Priorities are `Highest`, `Higher`, `High`, `Normal`, in that order.
`NanoTick` is an alias of `Normal`, not another priority class. The current
scheduler uses 32 buckets; its callers must keep deadlines within that ring.
Pending flags prevent duplicate scheduling only in the component branches that
check them. For example, lamp updates can schedule more than one pending
extinguish request; the scheduler itself does not deduplicate nodes.

The important ordinary component rules are:

| Component | Update and due-work semantics |
| --- | --- |
| Repeater | $l_v\gets[I_{v,\mathrm{side}}>0]$. When unlocked and without pending work, schedule if $b_v\ne\operatorname{nz}(I_{v,\mathrm{main}})$, at $n+2d$. On execution, an unlocked powered repeater with low input turns off; an unpowered repeater turns on even if its input has already fallen, scheduling a later turn-off in that case. |
| Torch | If not pending and $b_v\ne\neg\operatorname{nz}(I_{v,\mathrm{main}})$, schedule at $n+2$. On execution, apply the current inverted input. The compiled torch rule has no burnout state. |
| Comparator | Compute the analog function below. If output differs and no work is pending, schedule at $n+2$; recompute at execution. |
| Lamp | Rise immediately when input becomes positive. When lit and input is zero, schedule extinguishing at $n+4$; on execution, extinguish only if still unpowered. |
| Trapdoor | Apply current input positivity immediately. |
| Note block | Apply current input positivity immediately; emit a play event on a rising transition. |
| Button | A use of an unpowered button sets strength fifteen and schedules release after twenty steps. A due release sets strength zero. |
| Lever / pressure plate | An authorized interaction sets strength zero or fifteen and propagates its edges. |
| Wire | Update its display strength to $I_{v,\mathrm{main}}$. |

A repeater's chosen priority is `Highest` when facing a diode, otherwise
`Higher` for a scheduled fall and `High` for a scheduled rise. Comparator work
uses `High` when facing a diode and `Normal` otherwise. This ordering is part of
the state machine, not an optimizer hint.

For a comparator, let $M=I_{v,\mathrm{main}}$ and $J=I_{v,\mathrm{side}}$.
If a fixed far override $f$ exists, define $R=f$ when $M<15$ and $R=M$
otherwise; without that override, $R=M$. Then

$$
K_{\mathrm{compare}}(R,J)=\begin{cases}R,&R\ge J,\\0,&R<J,\end{cases}
\qquad K_{\mathrm{subtract}}(R,J)=R\mathbin{\dotminus}J.
$$

A stationary direct rear inventory override is extracted as an ordinary input;
it is not merely the power conducted through that block. Command-block success
counts can change while compiled; their direct/far comparator readers are
refreshed accordingly.

Command blocks add external actions to $E$: impulse output schedules on rising
power, repeating output schedules while powered or automatic, and chains run
through the existing command executor. First command work uses one step rather
than one redstone tick. `tick_with_world` processes command events before and
after graph ticking, so executing commands does not depend on rendering.
Command entity state and allowed commands remain governed by the ordinary
command-block implementation.

Note-block power transitions are retained even when the block is acoustically
blocked; the sound event checks its live unblocked condition when flushed.
Electrical input identity therefore does not depend on whether a sound can play
at compilation entry.

For each backend step, ordinary due work runs first, then each region advances
and publishes changes through ordinary `set_node` propagation. Region outputs
can schedule new ordinary work for subsequent steps. Source changes used by
electrical output terms can also refresh those outputs immediately. Event
ordering and resulting pending work are observable even when the eventual
settled strengths agree.

See [update.rs](../crates/core/src/redpiler/backend/direct/update.rs),
[tick.rs](../crates/core/src/redpiler/backend/direct/tick.rs),
[direct/mod.rs](../crates/core/src/redpiler/backend/direct/mod.rs), and
[queue.rs](../crates/core/src/redpiler/backend/queue.rs).

## 4. Conditional geometry and electrical boundaries

For actuator $a$, geometry predicates describe far payload, near payload,
stationary head, stationary retracted base, and moving base. They are different
variables because a moving block does not provide the stationary material's
power or conduction. Let $g$ collect these predicates and any owned observer or
wire-shape state.

An extracted consumer channel $o$ is a finite set of terms
$\mathcal T_o=\{(\gamma_t,\operatorname{source}_t,a_t)\}$, where $\gamma_t(g)$
is a guard. Its output is

$$
A_o(g,\mathbf s)=\max\left(
 \{s_t\mathbin{\dotminus}a_t:\gamma_t(g)=1\}\cup\{0\}\right).
$$

A term without an ordinary source denotes constant strength fifteen from a
redstone block. Wool, concrete, stone, sandstone, quartz and smooth quartz
can conduct permitted sources and change connectivity; they do not become
strength-fifteen emitters. All contributors, including unaffected fixed sources,
participate in the maximum. Removing one moving contributor need not turn an
output off.

A port replaces exactly one ordinary consumer channel. Comparator main and side
inputs stay distinct, and an unaffected channel keeps its ordinary links.
Adjacent dust is read directly by a diode regardless of the dust's side shape.
Comparator sides accept dust, appropriately facing diodes and redstone blocks;
ordinary solid-block conduction is not a comparator side source. Dynamic
inventory reads through mobile geometry have no general compiled contract.

`MobileSource` is a mutable position alias, not an immutable constant.
`InstantOutput` is the mutable strength of a consumer channel. Both lower to
Direct `InstantSource` nodes and require their owning prepared region.
`InstantInput` is a diagnostic candidate node; it is not sufficient to execute a
piston. World-I/O flags control preservation and display, whereas region ports
describe electrical and notification interfaces. Neither substitutes for the
other.

See [outputs.rs](../crates/core/src/redpiler/instant/outputs.rs) and
[boundary.rs](../crates/core/src/redpiler/instant/boundary.rs).

## 5. Extracted Boolean functions

For each actuator, extraction collects guarded power paths. A contribution from
ordinary source $i$ at attenuation $d$ is powered exactly when $s_i>d$.
With path guards $\gamma_t$, define

$$
P_a(\mathbf s,\mathbf x,\mathbf q)=
 \bigvee_{t\in\mathcal T_a}\left(\gamma_t(\mathbf x,\mathbf q)
 \land[s_{i(t)}>d_t]\right),\qquad R_a=\neg P_a.
$$

Constant redstone terms replace the threshold by true. In the acyclic path,
$x_b$ means another actuator's response; $q_b$ means stored retracted occupancy.
Own-payload feedback and owned reset observers are excluded from first-response
power. This exclusion is valid only with the associated ready/reset protocol.
It is not a statement that those sources cease to exist physically.

The actuator dependency graph has edge $b\to a$ whenever $x_b$ survives in
$R_a$. After Boolean simplification it must be acyclic. The physical wave
adapter substitutes dependencies in topological order to produce functions

$$
F_a(\mathbf s,\mathbf q)=R_a\bigl(\mathbf s,
 \{x_b\gets F_b(\mathbf s,\mathbf q)\},\mathbf q\bigr).
$$

Stored bits break combinational dependency cycles; they are not substituted by
their next values. The logical adapter retains each local $R_a$ and connects
actuator decisions to the corresponding response root. It computes the same
acyclic functions without expanding them into global decision diagrams.
The sequential path instead keeps local current-geometry,
wire and observer variables and supplies a temporal transition system.
It does not expand a complete memory-bearing network into one acyclic formula.

[boolean.rs](../crates/core/src/redpiler/instant/boolean.rs) represents functions
as reduced ordered decision diagrams. A decision obeys the Shannon equation

$$
D(v,L,H)=(\neg v\land L)\lor(v\land H).
$$

Equal children remove a decision; identical decisions share an identity;
conjunction, negation and substitution rebuild canonical decisions. Final
compaction retains only nodes reachable from response, output and sensor roots.
Variables include source thresholds, provisional actuators, memory, geometry,
owned observers and wire-dot shape. This is Boolean canonicalization under the
chosen variable order. Thresholds for one analog source have implications such
as $[s>7]\Rightarrow[s>3]$; the arena does not implement a separate analog
constraint solver. Treating them independently can miss simplifications while
retaining correctness for valid strengths.

Functions arise from geometry and electrical rules. Filename, sign label,
schematic hash, expected sum, or expected counter increment is not an execution
instruction. Arithmetic truth tables are tests of extracted functions.
The distinction matters when a layout computes a different function than its
author intended.

## 6. Acyclic response-wave adapter

The nonideal wave adapter retains

$$
z=(\phi,\mathbf f,\mathbf q,\mathbf m,
   \text{launch/ready sources},\text{handoff history}),\quad
\phi\in\{0,1,\ldots,6\}.
$$

$\phi=0$ is idle/ready; $\phi=6$ is a completed reset checkpoint.
$f_a$ is the actuator response sampled for the current wave. For each shared
payload group $B$, define $f_B=\bigvee_{a\in B}f_a$. This is one block with
several possible owners, not one independent block per actuator.

At $\phi\in\{0,6\}$ the runtime samples all
$f_a\gets F_a(\mathbf s,\mathbf q)$. If any response is true for an unclocked
region, begin at phase one; otherwise return to phase zero. At other phases,
advance $\phi\gets\phi+1$. The following step after phase six can begin another
wave when the held ordinary levels still call for a response. This recurrence
belongs to the adapter, even when the external input supplied only one falling
transition.

Let $A=[\phi\notin\{0,6\}]$. For a nonmemory actuator, current output geometry is

| Predicate | Nonideal wave value |
| --- | --- |
| Far payload for group $B$ | $\neg A\lor\neg f_B$ |
| Near payload for actor $a$ | $[\phi=3]\land f_a$ |
| Head for actor $a$ | $\neg A\lor\neg f_a$ |
| Stationary retracted base | $[\phi=3]\land f_a$ |
| Moving base | $[1\le\phi\le2]\land f_a$ |

Far redstone aliases supply fifteen only when present. Near occupancy contributes
through an extracted electrical output port. Nonideal admission rejects an
ordinary consumer whose guard observes the near ownership of a shared group:
union-of-firings alone cannot choose its physical owner.

This phase adapter models a supported boundary waveform. Ordinary consumers
retain their own delays and pending transitions. For instance, a one-step rise
at reset can start a repeater activation that executes after the region has
already begun its next low phase. Publishing a constant logical result in place
of the phase waveform changes that consumer's behavior.

Nonideal acyclic admission requires a ready powered extended mechanism with a
matching stationary head, permitted payload, coupled power/update interface, and
an owned reset or proven payload-following response. A follower satisfies the
symbolic condition that its response is identically false when other actuators
have ready occupancy, independently of external source strengths. Other guards
exclude unowned observers, extra reset writers, exposed reset signals and
destructive attachments. Stationary conducting reset caps can retain matching
furnace inventories; this does not authorize carried entities.

Prepared data is assumed stable through the accepted response/reset episode.
The runtime's checkpoint is a level evaluation, not a general edge journal.
[contract.rs](../crates/core/src/redpiler/instant/contract.rs) separately defines
electrical falling edges, rechecks and provisional trigger cancellation, but
its `TriggerState` is not the mechanism executing this wave path.
Ready-entry checks prevent activation merely by compiling an already-active
nonideal wave network. Do not infer arbitrary independent BUD sampling semantics
from a formula $F_a$ alone.

## 7. Shared-clock memory adapter

The specialized clock recognizer owns one empty ordinary downward generator,
its observers, and 1–64 independently owned downward redstone-block cells.
The clock response depends on one ordinary torch control and not on stored data;
the bank has an independently identified sampling route without extra writers.
Different independent regions can own different clocks.

For a stored cell, $q_a=1$ means retracted/near and $q_a=0$ means extended/far.
At each phase-zero or phase-six checkpoint, sample all responses from the same
old bank. Only the clock actor's response decides whether to start a wave.
In nonideal mode, at phase three commit

$$
q_a^+\gets f_a\quad(a\in\text{memory}),\qquad
m_a\gets[q_a^-\ne f_a].
$$

All $f_a$ were calculated before this commit. Updating cells one at a time and
then calculating the next cell would produce a different sequential machine.
At phase five clear $m_a$. The stored geometry is

$$
\mathrm{Far}_a=\mathrm{Head}_a=\neg q_a\land\neg m_a,\qquad
\mathrm{Near}_a=\mathrm{RetractedBase}_a=q_a\land\neg m_a,\qquad
\mathrm{MovingBase}_a=q_a\land m_a.
$$

The extracted bank transition is $\mathbf q_{k+1}=H(\mathbf s_k,\mathbf q_k)$,
where $H_a=F_a$ for stored actors. A counter is one possible instance of $H$;
the evaluator contains no arithmetic increment instruction. Output responses
also retain the wave's old-state evaluation and pass through ordinary consumers,
so displayed state can lag stored state.

This specialized physical adapter admits fresh ready extended storage.
Clear, arbitrary mid-episode controls and stop/restart physical equivalence
require their own protocol evidence. The generic sequential adapter can
represent other settled entries, but dispatch to it is an implementation choice,
not a claim that every shared-clock waveform remains equivalent.

See [clocked.rs](../crates/core/src/redpiler/instant/clocked.rs) and
[direct/instant.rs](../crates/core/src/redpiler/backend/direct/instant.rs).

## 8. Ideal acyclic and clocked mode

With `--assume-instant`, an unclocked acyclic region evaluates $F$ when its bound
ordinary inputs change. External inputs are held between a stimulus and its
settled output. Ordinary delayed graph nodes remain separate dynamic inputs.
It has no movement phase or reset waveform. Its geometry is

$$
\mathrm{Far}_B=\neg f_B,\quad
\mathrm{Head}_a=\neg f_a,\quad
\mathrm{RetractedBase}_a=f_a,\quad \mathrm{MovingBase}_a=0.
$$

For a shared group, choose the first firing actor in the group's stored order
as the logical near owner; only that actor has near occupancy. This is a
deterministic logical convention, not a universal physical ownership law.
Each new stimulus starts another logical evaluation; ordinary diodes, torches
and output events retain their existing timing.

In ideal clocked mode, an inactive clock holds the stored bank and last sampled
response. An active clock starts a sampling deadline immediately, then samples
again every six steps. At a sampling event, freeze inputs and the old bank,
evaluate all $F_a$, and only then commit every proposed memory bit together.
Changing data alone does not sample a cell. Successive active samples are six steps apart.
Stopping and restarting holds memory and begins a fresh cadence; there is no
movement state or physical reset pulse. Output terms that depend on ordinary
sources remain electrical functions and can still react to those sources.

Compilation validates a logical certificate: settled stationary entry,
matching extended heads or an unambiguous retained near payload, supported materials, coupled combinational notifications,
explicit independently sampled storage, and acyclic responses after cutting
those storage boundaries. Reset timing and reset caps are unnecessary for a
certified logical gate; this does not authorize unrecognized BUDs or feedback.
Rejection identifies the boundary and suggests explicit sampling or physical mode.

Compilation reads each certified storage bit from its saved extended/retracted
pose. Canonical near/far addresses describe both values without modifying the
world during preparation. A stopped bank can therefore be recompiled with
nonzero data; binding and inactive evaluation do not sample or clear it.

A side-mounted observer can be owned as logical reset work only after guarded
electrical extraction proves its pulse resets its pure response targets for
every data and payload-geometry assignment. Shared reset targets require the
same proof. The certificate checks notification recipients separately: a reset
must not become an independent memory sample or reach an ordinary data consumer.
Explicit storage and clocks retain their existing sampling owners.

The executable decision program binds Boolean input identities and thresholds
once. Response and output domains have separate snapshots and caches. Changed
inputs invalidate dependent decisions; evaluation walks needed branches and
shares cached subexpressions, including computed actuator conditions in the
local response DAG. The backend indexes ordinary source dependencies,
so unchanged unclocked regions and intervals between clock samples perform no
Boolean evaluation. Output guards read the committed bank and settled geometry,
never a response-domain cache from before commit. This is a decision-program
adapter, not a retained full-net reference plan. After `/rp reset`, the
interpreter resumes physical semantics; it need not continue this ideal model.

## 9. Notification-driven sequential adapter

The sequential adapter exists because an electrical level and a delivered
sampling update are independent channels. Its state is larger than a bit bank:

$$
z=(\mathbf q,\mathbf h,\mathbf b^{\mathrm{move}},\mathbf d^{\mathrm{actor}},
 \mathbf p^{\mathrm{payload}},\mathbf d^{\mathrm{payload}},
 \mathbf w,\mathbf\omega,\mathbf o,
 Q^{\mathrm{sample}},Q^{\mathrm{deferred}},Q^{\mathrm{complete}},Q^{\mathrm{observer}}).
$$

$q_a$ is current retracted state, $h_a$ head presence, $b_a^{\mathrm{move}}$
moving-base state, $d$ availability deadlines, $p_B^{\mathrm{payload}}$ one shared
payload's position and owner, $w$ wire strengths, $\omega$ retained wire shapes,
and $o$ observer states. Queue order is retained. This state covers storage,
reset, generator and sampled-read geometry without pretending that each is a
stateless Boolean gate.

Local power functions are $R_a(\mathbf s,\mathbf w,\mathbf\omega,\mathbf o,g)$.
Here $R_a=1$ means the sampled electrical condition calls for retraction.
A delivered base notification samples that function; a head notification samples
only while a stationary head is present. Changing remote quasi-connectivity
power alone does not necessarily deliver either notification.

A request is emitted only when $R_a\ne q_a$ and the base is not moving.
The action is extension for $R_a=0$, retraction for $R_a=1$, or early
`RetractWithoutPull` when a same-facing extension payload remains unavailable at
the far position. There is at most one queued request per actor/action pair.
On dequeue, the runtime rechecks live power, current state and moving-base
availability. With $r=[\text{action}\ne\mathrm{Extend}]$, acceptance requires

$$
R_a=r\ \land\ R_a\ne q_a\ \land\ \neg b_a^{\mathrm{move}}.
$$

An invalid queued request is discarded. A notification, queued request and
accepted movement are therefore three different observations. Same-value
sampling can be meaningful even when it emits no request.

Accepted movement changes $q_a$ and head/base availability. Payload movement
uses actual stored ownership and position: a sticky normal retract can pull a
stationary far payload to near; early retract does not pull; an extension from
near moves that payload to the group's common far destination. A moving payload
provides neither stationary conduction nor power. Completion deadlines are two
steps after acceptance. The runtime retains and checks the actor/payload
deadline when consuming completion entries, so replaced work is not applied as
current completion.

Current geometry predicates are

$$
\begin{aligned}
\mathrm{Far}_a&=[d_B^{\mathrm{payload}}=0\land p_B^{\mathrm{payload}}=\mathrm{far}_a],\\
\mathrm{Near}_a&=[d_B^{\mathrm{payload}}=0\land p_B^{\mathrm{payload}}=\mathrm{near}_a],\\
\mathrm{Head}_a&=h_a,\\
\mathrm{RetractedBase}_a&=q_a\land\neg b_a^{\mathrm{move}},\\
\mathrm{MovingBase}_a&=b_a^{\mathrm{move}}.
\end{aligned}
$$

Each compiled wire sensor uses the same guarded maximum equation as an output
port, with neighboring wire values as local sources. A dirty sensor reevaluates
its strength. A changed strength drives precomputed neighbor work in the
interpreter's `TURBO_ORDER`, retaining traversal direction/layer information.
Neighbor shape changes use `on_neighbor_changed_from`, and shape changes can
notify observers and other sensors. This is an ordered worklist system;
solving only the final electrical fixed point would discard sampling history.
Sensors include dust adjacent to moving base/head/far positions even when no
power path uses the payload. A geometry notification can alter saved wire power
or shape without being an electrical dependency of the sampled piston.

Observers watch owned positions. A qualifying watched change schedules a toggle
two steps later if no observer work is already pending. A rising observer
toggle schedules its fall two steps later. Its strength feeds sensor equations
and its output delivers the precomputed sample notifications. Observer pulse
state and pending deadlines are part of the region state.

The current per-step sequence is:

1. Bring previous completion-generated deferred requests into the sample queue;
   detect ordinary source changes and deliver their compiled notifications.
2. Settle dirty wires and process due observer toggles and their sample routes.
3. Drain sample requests FIFO, rechecking each request; accepted actions emit
   further notifications and settle affected sensors.
4. Record boundary strengths before movement completion.
5. Apply due actor/payload completions, settle their wire/shape changes, and put
   generated movement requests in the deferred queue for the next step.
6. Record boundary strengths after completion and return the ordered changes.

The boundary publisher can return several strength changes for the same port in
one step. Collapsing them to the last level can erase a pulse seen by an
ordinary consumer. Extension destination shape changes also differ from full
neighbor notifications; upgrading one into the other can spuriously sample a
quasi-powered cell. These distinctions are general consequences of separate
power, shape, notification and availability channels.

Sequential admission requires one supported stationary payload per ownership
group, or an empty all-ordinary group; every owner shares a common far
destination. Actual alias occupants must be air, a supported payload, or a saved
head at an owning actor's head position. It permits stationary extended or
retracted entry and multiple
ordinary update pistons. It rejects incompatible present heads, moving-context
entities, destructive moving supports and pending owned entry work. A missing
saved head represented by air can be retained as an actual initial state;
recognition does not synthesize a stationary head merely from `extended=true`.
This does not amount to arbitrary push chains, arbitrary materials, transported
entities or unrestricted editing during execution.

[program.rs](../crates/core/src/redpiler/instant/program.rs) selects this path
when the region contains multiple ordinary pistons, a non-downward ordinary
piston, retracted entry, a missing/mismatched-head diagnostic, or an additional
reset-writer recognition failure. The fallback then performs its own checks.
Some unsupported acyclic cases simply fail rather than being retried here;
representational capability and dispatch coverage are separate.

The sequential adapter is available without `--assume-instant`. Logical
compilation rejects regions requiring these local notifications, availability
deadlines or retained-read protocols until they have a logical sampling certificate.
Execution uses compiled decisions and indices, with no runtime block search or
interpreter callback invocation. It nevertheless retains selected physical
ordering because these updates affect observable memory state.

See [sequential extraction](../crates/core/src/redpiler/instant/logic/sequential.rs),
[notification preparation](../crates/core/src/redpiler/instant/sequential.rs), and
[sequential runtime](../crates/core/src/redpiler/backend/direct/instant/sequential.rs).

## 10. Regions and composition

Represent a region by $\mathcal R=(F,H,\mathcal P,A)$: response functions,
retained-state transition, admitted histories, and boundary adapters.
An implementation of $F$ alone is insufficient whenever $H$ or $A$ needs
storage, sampling, reset, read-gate or clock state.

For independent regions, $z=(z_1,\ldots,z_r)$ and each $H_j$ advances its own
phase and queues while sharing the ordinary graph. Common plot membership or
an ordinary input is not a dependency between their retained states. Payload
ownership, reset ownership, sampling routes and moving geometry do establish
dependencies. Region splitting conservatively joins those dependencies before
execution. The implementation advances the resulting runtime vector in its
stored order; it is not a global simultaneous fixed-point solver for arbitrary
mutually coupled clocks.

An abstract sampled bit illustrates the storage obligation:

$$
q^+=\begin{cases}D(\mathbf s,g),&u=1,\\q,&u=0,\end{cases}
$$

where $u$ is an accepted transaction and $D$ its data decoder. Physical
notifications need not equal $u$: the request may be cancelled, blocked, early,
or a same-value sample. Multiple updates on one cell require ordered
transactions unless the family proves an atomic batch. A held read adapter may
retain its own $r$, giving output $A(\mathbf q,r,\mathbf s)$ rather than a
continuous read of the current bank. The sequential runtime's geometry and
queues can represent such history; a generic word-level RAM interface is not
derived merely from that capability.

Falling events also remain distinct from data:

$$
e_i(n)=[s_i(n^-)>0\land s_i(n)=0],\qquad d_i=D_i(s_i,g).
$$

Held zero creates no new external event. A prepared-data decoder may use either
polarity. In a source-removal encoding, an output supplied by two independent
positive contributors falls only when both disappear; its decoded event is
an AND although its electrical join is a maximum. A shared payload removed by
either owner gives an OR. Such Boolean interpretations follow the declared
decoder and episode; they are not universal component replacement rules.

## 11. Returning execution to the interpreter

Handoff requires an interpreter state with equivalent future boundary behavior,
not only matching current output bits. If $M$ materializes compiled state, the
obligation is

$$
\forall h\in\mathcal P_{\mathrm{continue}}:\quad
O(\operatorname{Interpreter}(M(S_n),h))
=O(\operatorname{PhysicalReference}(n,h)).
$$

This concerns continuation of the compatible physical protocol. An ideal
region can intentionally differ from physical execution, so materializing it
does not promise continuing ideal/interpreter equality.

Reset first flushes ordinary hidden state and events. Nonideal wave handoff
builds a private interpreter world from the region snapshot, restores prepared
and launch values, and replays a bounded tail to reconstruct owned geometry,
motion and reset work. It uses at most 32 preparation steps and one current
six-step wave plus one previous cycle. Clocked handoff retains earlier bank
values so this cost does not grow with elapsed counting. Only owned work is
transferred; the live graph scheduler remains authoritative for ordinary
deadlines.

Ideal acyclic/clocked handoff writes stationary logical occupancy and the last
committed bank. Shared payload ownership uses the first firing actor in stored
group order. Owned reset observers are off; other owned reset components retain
their saved presentation. Owned dust receives settled shape and strength from
compiled guarded paths; saved reset-source strengths are fixed only for this
restoration. Handoff creates no movement entities, piston events or reset
callbacks and invokes no electrical update propagation. The ordinary scheduler
retains its pending work. This is a deterministic settled snapshot, not replay
of a historical physical reset wave.

Sequential handoff writes retained actor/head/payload geometry, sensor shape and
power, observer state, remaining observer work, and queued sample requests.
Unfinished actor and payload availability become moving entities with the
appropriate remaining completion interval. This restores current temporal state
rather than reconstructing an unbounded input history.

Finally ordinary scheduler entries return to the world with relative deadlines,
priority and FIFO order. Constants preserve current physical presentation
rather than overwriting it with an obsolete compile snapshot. Internal visual
blocks may have remained at entry presentation while compiled; render flushes
are not observations of the live internal region state.

The bounded replay and direct reconstruction are implemented handoff mechanisms,
not proofs of all edit/save/load/recompile histories. Required new lifecycle
support needs continuation tests at the affected state boundaries.

## 12. Generalization, admission and known gaps

The common model is an electrical maximum over guarded geometry, coupled to an
owned temporal transducer. This explains several otherwise local-looking rules:

| Implementation distinction | General reason |
| --- | --- |
| Additional conductor materials | Material contributes connectivity, not an invented signal source. |
| Fixed comparator rear overrides | Inventory observation is a separate analog channel from conducted power. |
| Mutable mobile aliases and protected graph sources | Ownership and stable identity must survive optimization. |
| Shared far occupancy versus near owner | A shared block's existence and its owner's position are different state. |
| Old-bank sampling before commit | A clocked bank defines an atomic transaction. |
| Deferred completion notifications | Sampling order crosses an availability boundary. |
| Extension shape change versus neighbor notification | Electrical connectivity and BUD sampling are distinct actions. |
| Retained read geometry | A sample-and-hold interface has history even when its data source is ordinary memory. |
| Selected-context and full-plot rules | Extraction needs a closed electrical world; a small selection must include its dependencies. |

Universal documentation should preserve these rules and their state variables,
rather than assigning one formula or coordinate convention to every fixture.
Local recognition seeds are candidates, not executable certificates. Admission
also checks live entry phase, pending events/motions/work, ownership, available
context, budgets and backend bindings. Piston regions remain within one plot;
instant binary export is unsupported. Preparation is staged, and the compiler
publishes the new backend only after successful preparation/lowering.

The implementation has finite analysis, traversal, source and decision-arena
budgets. Server multipliers scale selected limits; local occupancy enumeration
also has fixed limits. Exhaustion is a compilation resource limit, not evidence
that a circuit's logical function is false or physically invalid. Consult the
current constants in [analysis/mod.rs](../crates/core/src/redpiler/analysis/mod.rs),
[logic.rs](../crates/core/src/redpiler/instant/logic.rs), and
[boolean.rs](../crates/core/src/redpiler/instant/boolean.rs) before tuning them.

The following limits must remain explicit:

- The acyclic reset-wave path can currently admit a layout whose later inhibitor
  interaction does not match the interpreter. The ignored regression
  `xor_complete_reset_waveform_matches_interpreted_consumers` documents a
  tick-eight mismatch. A correct first-wave XOR does not certify reset closure.
- The sequential worklist is an implemented finite geometry/notification model,
  not a theorem of equivalence for arbitrary Java callback traces, simultaneous
  external changes, replacement ordering, or every consumer attachment. Its
  source-change scan iterates a hash map rather than an explicit external action
  journal, so multiple ordinary source transitions within one step do not gain
  a universal application-defined ordering. Its supported payload and ownership
  checks remain necessary.
- `--assume-instant` does not mean one universal piston semantics: temporal
  deadlines remain on the sequential path, while acyclic and shared-clock
  adapters use the explicit ideal rules above.
- Ordinary optimization contains specialized binary/attenuation assumptions.
  The preservation equation is the required contract; it is not a claim that
  every current transformation has been proved for every analog graph. See
  [REDPILER_OPTIMIZER.md](REDPILER_OPTIMIZER.md) for concrete audit boundaries.
- Tests of selected protocols, rotations, truth vectors or long runs establish
  those observations. They do not establish untested sampling orders,
  initialization, dynamic inventory transport, lifecycle histories or physical
  synchronization.

When extending a family, first identify which existing $F$, $H$, $\mathcal P$ or
$A$ is missing the behavior. A new sample order belongs in the temporal adapter;
a new material rule belongs in geometry extraction; a retained analog value
belongs in the appropriate channel. Adding a fixture-specific arithmetic
instruction or globally changing power into notification hides the actual
contract and breaks composition.
