# Redpiler execution model

This document defines the mathematical meaning of the current compiler and
Direct backend. [REDPILER_ARCHITECTURE.md](REDPILER_ARCHITECTURE.md) describes the architecture,
[REDPILER_PARSER.md](REDPILER_PARSER.md) describes extraction, and
[REDPILER_OPTIMIZER.md](REDPILER_OPTIMIZER.md) describes transformations and their
limits. Physical electrical and piston execution are defined separately in
[REDSTONE_MODEL.md](REDSTONE_MODEL.md) and [PISTON_MODEL.md](PISTON_MODEL.md).
Runnable evidence and fixture protocols are in [tests/README.md](tests/README.md).

Redpiler has an ordinary electrical graph, cached Boolean piston circuits,
explicit BUD memory nodes, and scheduled electrical output boundaries.
Inputs must be synchronized and held through the supported response/reset
episode; changes outside that protocol have undefined compiled behavior.
Default compilation and `--assume-instant` use the same runtime;
the flag relaxes construction proofs, not sampling or execution semantics.
The former wave and sequential executors are retired. Retained sequential
extraction helpers contribute dependency and admission proofs, not another
active runtime. Native/compiled partial execution remains a
[proposal](notes/REDPILER_PARTIAL_COMPILATION.md).
Ordinary selections can instead use the separate Native backend; the equations
below describe Direct execution, not Turbo's physical callback traversal.
Section 13 specifies a proposed activation extension which is not implemented.
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
| Lamp | Rise immediately when input becomes positive. When lit, input is zero, and no work is pending, schedule extinguishing at $n+4$; on execution, extinguish only if still unpowered. |
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
blocked. For an owned BUD position above the note, the event captures committed
logical occupancy at its power rise and retains it until sound delivery. Other
notes check live obstruction when flushed. BUD display cadence and intervening
memory commits therefore cannot change eligibility for an already queued note.
Electrical input identity therefore does not depend on whether a sound can play
at compilation entry.

Each ordinary scheduled callback is followed by region evaluation and publication
through ordinary `set_node` propagation; shared-clock deadlines run after the
ordinary due work. Interactions also evaluate affected regions immediately.
Region outputs can schedule new ordinary work. Source changes used by
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
power or conduction. Internal Boolean decisions use settled geometry;
electrical outputs and observers use scheduled boundary geometry. Let $g$
collect the admitted geometry predicates and committed stored state.

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

### QC influence and sampling admission

Let $D(g,\mathbf s)$ be direct piston power and $Q_t(g,\mathbf s)$ one QC
contribution. Feedback admission uses the same guarded physical geometry to
prove whether

$$
Q_t\land\neg D\equiv 0.
$$

When this holds for every assignment, that contribution cannot change the
piston's power and does not require a separate sampling boundary. Receiving
dust positions retain independent strength variables during this proof. Two
wires sharing an upstream emitter can have different transient strengths, so
their settled source identity cannot establish this implication.

Remaining QC contributions are followed through every possible conductor pose
to recover internally driven data sources, including paths absent at import.
Data delivery and qualifying update writers remain separate channels; an
independent interface still needs represented sampled state. Incomplete context,
cancellation, and exhausted proof budgets fail admission.

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
$R_a$. After Boolean simplification it must be acyclic. Substitution in
topological order defines the functions

$$
F_a(\mathbf s,\mathbf q)=R_a\bigl(\mathbf s,
 \{x_b\gets F_b(\mathbf s,\mathbf q)\},\mathbf q\bigr).
$$

Stored bits break combinational dependency cycles; they are not substituted by
their next values. The runtime retains each local $R_a$ and connects
actuator decisions to the corresponding response root. It computes the same
acyclic functions without expanding them into global decision diagrams.

[boolean.rs](../crates/core/src/redpiler/instant/boolean.rs) represents functions
as reduced ordered decision diagrams. A decision obeys the Shannon equation

$$
D(v,L,H)=(\neg v\land L)\lor(v\land H).
$$

Equal children remove a decision; identical decisions share an identity;
conjunction, negation and substitution rebuild canonical decisions. Final
compaction retains only nodes reachable from the required runtime and handoff roots.
Variables include source thresholds, provisional actuators, memory, geometry,
owned observers and wire-dot shape; construction helpers can use variables
that executable logical preparation later rejects. This is Boolean canonicalization
under the chosen variable order. Thresholds for one analog source have implications such
as $[s>7]\Rightarrow[s>3]$; the arena does not implement a separate analog
constraint solver. Treating them independently can miss simplifications while
retaining correctness for valid strengths.

Functions arise from geometry and electrical rules. Filename, sign label,
schematic hash, expected sum, or expected counter increment is not an execution
instruction. Arithmetic truth tables are tests of extracted functions.
The distinction matters when a layout computes a different function than its
author intended.

## 6. Cached logical response execution

An unclocked region evaluates $F$ when its bound ordinary inputs change.
Ordinary delayed graph nodes remain separate dynamic inputs. Internal Boolean
evaluation uses settled geometry. For a nonmemory actor, this geometry is

$$
\mathrm{Far}_B=\neg f_B,\quad
\mathrm{Head}_a=\neg f_a,\quad
\mathrm{RetractedBase}_a=f_a,\quad \mathrm{MovingBase}_a=0,
\qquad f_B=\bigvee_{a\in B}f_a.
$$

For a shared group, choose the first firing actor in the group's stored order
as the logical near owner; only that actor has near occupancy. This is a
deterministic logical convention, not a universal physical ownership law.
For stored cells, use committed $q_a$ rather than the proposed response $f_a$:

$$
\mathrm{Far}_a=\mathrm{Head}_a=\neg q_a,\qquad
\mathrm{Near}_a=\mathrm{RetractedBase}_a=q_a,\qquad
\mathrm{MovingBase}_a=0.
$$

The executable decision program binds Boolean input identities and thresholds
once. Response, output, and sampling domains have separate snapshots and
caches. Changed inputs invalidate dependent decisions; evaluation walks needed
branches and shares cached subexpressions, including computed actuator
conditions in the local response DAG. Ordinary source dependencies are indexed,
and the reviewed worktree captures dirty binding values before applying their
invalidation, with full capture at initialization. Dirty roots restrict response
evaluation. These cache optimizations preserve the response equations only if
every source, memory, and geometry mutation marks its affected bindings.
Unchanged unclocked regions need no Boolean reevaluation. Electrical output
guards instead read a small scheduled boundary phase. Only actors used by
ordinary consumers or observers need this state; a shared payload includes all
its possible owners. No native piston events, movement entities, world scans,
or neighbor callbacks run in the compiled executor.

For a certified observer or payload-fed dust reset, a response launched at $r$
uses these electrical phases:

| Phase | Tick | Conducting payload occupancy |
| --- | --- | --- |
| Retracting | $r$ | Neither far nor near |
| Retracted | $r+2$ | Near |
| Extending | $r+3$ | Neither far nor near |
| Extended | $r+5$ | Far |
| Next response while active | $r+6$ | Start the next cycle |

Direct input actions between ticks launch on the next tick. Scheduled source
changes can launch within their current tick. Moving payloads do not emit or
conduct power. Ordinary repeaters, observers and bulbs consume the resulting
electrical edges through their existing graph rules. An idle tick checks the
next deadline; Boolean functions are not reevaluated for each boundary phase.

Compilation validates stationary entry, matching extended heads or an
unambiguous retained near payload, supported materials, coupled combinational
notifications, explicit independently sampled storage, and acyclic responses
after cutting storage boundaries. Default compilation additionally checks the
applicable reset/construction proofs; `--assume-instant` relaxes those proofs
without changing the executable model or authorizing unproved sampling.
Moving entities, destructive attachments, selection escape, ambiguous
ownership, and pending owned work remain admission failures.

A reset observer can be omitted only after guarded extraction proves that its
pulse resets pure response targets without independently sampling memory or
reaching an ordinary data consumer. Explicit storage and clocks retain their
sampling owners. Certification retains the reset owner and its electrically
affected actors so exposed reset edges survive the Boolean abstraction.
The model covers synchronized instant constructions; arbitrary interacting
movement, destructive payloads and desynchronized reset histories are outside
its contract.

An ordinary timed node fed by piston outputs can provide QC data without
notifying the receiving piston. A separate piston update then samples that
data. This interface requires an explicit sampling domain; treating it as a
combinational input launches responses on the wrong tick. The revised
`TEST_POTADOS_PC_COUNTER` contains this interface: repeater data changes at
tick 5, the update wave launches the response at tick 7, and its output
repeater changes at tick 9. The simple unclocked model launched at tick 5
and changed that output at tick 7. Admission must reject an unrepresented
sampling interface with either flag. This limitation does not establish that
the physical counter is noninstant.

Two other notification interfaces are excluded: a reset observer whose
falling pulse returns through a notifying wire can interrupt extension and
drop the retained block, and an ordinary observer watching a piston-driven
trapdoor or note block depends on native callbacks that those blocks do not
emit on state changes. Neither interface is replayed by the Boolean executor.

See [program.rs](../crates/core/src/redpiler/instant/program.rs),
[logical.rs](../crates/core/src/redpiler/backend/direct/instant/logical.rs), and
[observer.rs](../crates/core/src/redpiler/instant/observer.rs).

## 7. Shared-clock memory

The specialized clock recognizer owns one empty ordinary downward generator,
its observers, and 1–64 independently owned cells with supported payloads.
Its control depends on one ordinary torch source and not on stored data.
Validation follows the clock's control expression, including upstream pure
actuators; independent data inputs elsewhere in the region are not clock controls.
The existing source-type and release-on-loss checks still apply.
The bank has an independently identified sampling route without extra writers.
Different independent regions can own different clocks.

For a stored cell, $q_a=1$ means retracted/near and $q_a=0$ means extended/far.
An inactive clock holds the bank. An active clock launches a response every six
steps. Each launch freezes inputs and the old bank and evaluates the Boolean
responses. The update net samples two steps later and commits the bank together:

$$
q_a^+\gets F_a(\mathbf s,\mathbf q^-)\quad(a\in\text{memory}).
$$

Updating one cell before evaluating another would define a different machine.
The extracted transition is $\mathbf q_{k+1}=H(\mathbf s_k,\mathbf q_k)$,
where $H_a=F_a$ for stored actors. A counter is one possible instance of $H$;
the evaluator contains no arithmetic increment instruction. A bank commit does
not reevaluate the pure circuit until the next response launch.

Changing data alone does not sample a cell. Stopping preserves memory;
restarting begins a fresh cadence. Output terms dependent on ordinary sources
can still react electrically. Compilation reads storage from the saved pose,
and binding establishes event baselines without sampling or clearing it.
A stopped bank with nonzero memory can therefore be recompiled.

See [clocked.rs](../crates/core/src/redpiler/instant/clocked.rs) and
[the counter regression](../crates/core/src/redpiler/analysis/tests/ideal.rs).

## 8. Independent notification sampling

Power and notification discovery are separate. A generic BUD's desired
response is $F_a(\mathbf s,\mathbf q)$, but data changes alone do not commit it.
Source discovery uses native torch, observer and diode notification geometry.
Potential piston heads are inventoried even when absent from the saved pose;
their discovery does not establish a movement timing certificate.
Current independent sources are certified empty ordinary generators' settled
base/head pose edges and independently driven fixed dust strength changes.
Dust requires one writer, no moving power source, and a notification route
separate from the cell's data route. An ordinary graph observer can supply
that fixed writer while retaining its scheduled pulse.

For a notification event $e$ with source value $v_e$, the current adapter
detects a delivery when $v_e$ differs from its previous value. Compilation
establishes the baseline; activation itself is not a delivery. An unchanged
generator pose does not sample periodically. A distinct source action that
produces another edge is another delivery, even in the same game tick.

The runtime groups affected cells by their generator or fixed writer. A delivered
group reads one frozen old bank, evaluates eligible recipients, and commits
them together. A later writer group reads the preceding group's committed bank.
A base route always qualifies; a head-only route qualifies only while the
cell is extended in that group's old state. Same-value memory samples remain
deliveries even when no stored bit changes. This is the current logical
transaction contract, not a proof of arbitrary native callback ordering.

Several generic generators and recipients can be represented. Multiple
clock-shaped candidates defer to independent sampling instead of failing on
their count. Independent sampling rejects observer routes watching a generator
base or head: settled pose edges cannot reproduce timed movement notifications.
Stored-state generator control, multiple dust writers, and unproved
geometry-dependent notification routes remain rejected. Delivery flags and
writer ordering do not preserve every native callback's multiplicity and order;
the proposed generalization must establish those semantics before extending
admission.

See [sampling.rs](../crates/core/src/redpiler/instant/sampling.rs),
[the runtime](../crates/core/src/redpiler/backend/direct/instant.rs), and
[memory regressions](../crates/core/src/redpiler/analysis/tests/memory.rs).

## 9. Committed BUD presentation

Display flush projects each supported BUD's committed bit into its base, head,
and payload positions. Extended cells show an extended base, stationary head
at near, and payload at far. Retracted cells show a retracted base, payload at
near, and air at far. Independent cells and shared-clock banks use the same
projection.

The last published bit is separate from observer state tracking. Flush writes
only changed cells through ordinary block storage and packet collection;
it invokes no physical updates or handoff. A data change without a qualifying
sample leaves the displayed cell unchanged. Pose publication creates no extra
samples, observer events, or scheduled work; ordinary flush evaluation retains
its existing role.

Normal display cadence, lever/button interaction, and manual game-tick `/adv`
publish the committed pose. `--optimize` retains BUD presentation.
`--io-only` suppresses internal BUD writes. Screen-only suppresses internal
incremental packets through its existing storage overlay; storage still records
the latest published pose for reads, chunk snapshots, and disabling that mode.
A chunk snapshot before the next display flush contains the previous published
pose. At high TPS, client batching can skip short-lived intermediate poses.

Other virtual region internals keep their saved appearance until handoff.
There is no smooth movement, internal dust/reset animation, or new admission
from these display writes. Note-block obstruction over BUD-owned geometry uses
canonical committed occupancy even when visual writes are suppressed or deferred.

See [Direct flush](../crates/core/src/redpiler/backend/direct/mod.rs),
[manual advancement](../crates/core/src/plot/commands.rs), and
[screen updates](../crates/core/src/plot/screen_updates.rs).

## 10. Regions and composition

Represent a region by $\mathcal R=(F,H,\mathcal P,A)$: response functions,
retained-state transition, admitted histories, and boundary adapters.
An implementation of $F$ alone is insufficient whenever $H$ or $A$ needs
storage, sampling, reset, read-gate or clock state.

For independent regions, $z=(z_1,\ldots,z_r)$ and each $H_j$ retains its own
bank, sampling baselines, and clock deadline while sharing the ordinary graph.
Common plot membership or an ordinary input is not a dependency between their
retained states. Payload
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
transactions unless the family proves an atomic batch. A held read adapter
would need its own retained state rather than a continuous read of the current
bank; a generic word-level RAM interface is not established by the current
logical equations.

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

Handoff must leave valid piston geometry and enough pending native work for
active reset loops to resume. It preserves committed storage, but may restart
the reset episode with different timing. It does not promise a waveform equal
to uninterrupted physical execution across the transition. An ideal region
can also intentionally differ from physical execution.

Reset first flushes ordinary hidden state and events, including comparator
entity strengths. Logical handoff writes stationary occupancy and the last
committed bank. Shared payload ownership uses the first firing actor in stored
group order. Owned reset observers are off; owned dust receives settled shape
and strength from compiled guarded paths. The ordinary scheduler retains its
pending work.

After writing the complete snapshot, handoff schedules native observer pulses
and base rechecks for active nonmemory reset owners, including the internal
clock and its sampling observer. These resume the reset protocol from a valid
pose without exporting historical movement entities or motion progress. Inactive
owners and stored BUD cells receive no unsolicited reset. The same handoff rule
applies with and without `--assume-instant`.

Finally ordinary scheduler entries return to the world with relative deadlines,
priority and FIFO order. Constants preserve current physical presentation
rather than overwriting it with an obsolete compile snapshot. BUD display
already exposes the most recently published committed pose unless suppressed;
handoff exports the current bank regardless of the display cadence. Other
virtual internal components may have remained at entry presentation.

Explicit reset-with-update additionally invokes the requested physical updates;
ordinary handoff schedules only the active reset protocol's continuation. Compilation
still requires stationary entry geometry with no pending piston events. Repeated
handoff tests check geometry, continuing activity, and settled recompilation.
They allow a timing shift at the domain boundary. This does not prove all edit/save/load histories; new
lifecycle support needs continuation tests at the affected state boundaries.

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
| Power cache versus notification | Changed data alone cannot write stored state. |
| Committed memory versus presentation | Display cadence cannot become a sampling event. |
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

- Clock-shaped candidates still select a specialization requiring exactly one
  generator. Some notification routes remain unproved, including the current
  PM1 investigation. Display support does not
  change those rejection boundaries.
- Independent sampling currently compares source values and groups recipients
  by writer. It does not preserve every native repeated notification or callback
  order. Stored-state control and multiple dust writers remain unsupported.
- The logical executor omits native movement entities and piston events, but
  models certified electrical movement/reset phases for boundary outputs. Native hybrid
  ownership, ordered boundary callbacks, and operation-preserving fallback are
  proposed work, not a current continuation guarantee.
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

## 13. Proposed notification-gated instant responses

Status: specification for the minimal PC counter extension, not current runtime
support. Sections 5–8 describe the existing response and storage mechanisms.
This section adds activation state without reclassifying instant receivers as
storage BUDs. Route and phase certification remain required with either flag.

### State and activation

Partition response actors into signal-driven actors $A_s$ and notification-gated
actors $A_e$. Actual storage actors remain a separate class $A_m$. For $a\in A_e$
retain committed response $c_a$ (one means retracted), initialized from admitted
entry state, and certified phase state $\phi_a$. The expression cache is derived
state; it is not $c_a$ and is not a stored circuit memory bit $q_a$.

Let $H_a(\mathbf s,\mathbf q,\mathbf c)$ be the candidate response obtained from
the existing guarded power equation $R_a=\neg P_a$. References to gated actor
occupancy use committed $c_b$, while signal-driven dependencies use their
acyclic candidate functions. Cut substitution at gated references; do not inline
their newly computed candidates into downstream geometry. An event-mediated
dependency cycle needs its own admission proof; the cut alone does not certify it.

An activation event is

$$
e=(\tau,k,w,p,d,a),
$$

where $\tau$ is its supported time/phase, $k$ its delivery order, $w$ the writer,
$p$ the recipient position, $d$ the callback direction (possibly absent), and
$a$ the target actor. A certified route determines eligibility, including whether
a head exists at delivery. Distinct callbacks retain distinct $k$, even when
their writer, target, and electrical values agree.

For a data-only change $\Delta\mathbf s$:

$$
\mathbf s^+=\mathbf s+\Delta\mathbf s,\qquad c_a^+=c_a,
\qquad \phi_a^+=\phi_a \quad(a\in A_e).
$$

This transition invalidates affected electrical caches but does not launch the
gated actor. Previously scheduled phase work can still execute at its deadline.

For an eligible delivered event targeting $a$, use its pre-event state:

$$
v=H_a(\mathbf s^-,\mathbf q^-,\mathbf c^-),\qquad
c_a^+=v,\qquad c_b^+=c_b\ (b\ne a),
$$

$$
(\phi^+,Q^+,E^+)=\operatorname{ActivateCertified}
 (\phi^-,Q^-,E^-,a,v,e).
$$

`ActivateCertified` applies the existing admitted phase/event acceptance rules.
It does not mean restart a cycle on every callback. Same-value activations still
reach this transition; its phase/pending checks determine their effects.
An ineligible callback changes neither response nor phase state.

Process activations in delivery order, publishing effects required before the
next event. Shared writer identity does not imply an atomic batch. Only an
explicit certificate can authorize old-state fanout batching. Storage writes
retain their separate old-bank sampling contract from sections 7–8.

### Boundary and selective-capture contract

Internal dust has no physical runtime executor. Compiled notification routes
produce activation events; ordinary sources provide live electrical inputs.
Generic external placement/shape callbacks and destruction of decorative blocks
are not part of an assembly's interface. Supported electrical and observer
events require explicit ports. Intermediate geometry remains internal to those
certified expressions; physical state is materialized on handoff.

For each plan, let $B(x)$ be the binding IDs depending on input identity $x$.
A changed dependency marks $D\gets D\cup B(x)$. At an evaluation boundary read
all bindings in $D$ against one coherent state, then update snapshots and
invalidate affected decisions. Initialization captures all bindings. Source,
memory, committed response, and scheduled geometry changes must mark their
respective dependencies. An activation is not discarded because $D$ is empty.

### Current Rust correspondence and gaps (2026-10-10)

| Formal operation | Current implementation | Status |
| --- | --- | --- |
| Guarded power and candidate response | `instant/logic.rs`, Boolean arena, bound response plan | Present for existing admitted families |
| True stored state $q$ and old-bank writes | `MemoryCell`, `memory`, clocked/independent sampling | Present; must remain distinct |
| Selective coherent capture and dirty roots | `backend/direct/instant/logical.rs`; mutation marking in runtime | Present in worktree; source review, not fresh validation |
| Separate $c$ and candidate $H$ | `fired` currently updated from dirty candidate responses | Missing for gated actors |
| Ordered activation events | Sampling uses value changes and Boolean `delivered` flags | Missing; multiplicity/order not represented |
| Route certification for gated responses | `PistonPorts.updates`, reset proofs, QC feedback rejection | Discovery exists; executable activation proof missing |
| Certified phase state | `backend/direct/instant/timing.rs` | Existing machinery; event acceptance adaptation required |

Thus the current runtime matches the restricted signal-driven model and cannot
claim conformance to this extension. Successful admission must wait for its route,
commit, delivery, and continuation implementation and differential validation.
