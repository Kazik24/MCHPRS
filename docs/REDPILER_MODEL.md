# Redpiler execution model

This document defines the mathematical meaning of the current compiler and
Direct backend. [REDPILER_ARCHITECTURE.md](REDPILER_ARCHITECTURE.md) describes the architecture,
[REDPILER_PARSER.md](REDPILER_PARSER.md) describes extraction, and
[REDPILER_OPTIMIZER.md](REDPILER_OPTIMIZER.md) describes transformations and their
limits. Physical electrical and piston execution are defined separately in
[REDSTONE_MODEL.md](REDSTONE_MODEL.md) and [PISTON_MODEL.md](PISTON_MODEL.md).
Runnable evidence and fixture protocols are in [verification reference](REDPILER_ARCHITECTURE.md#9-commands-limits-and-verification).

Redpiler has an ordinary electrical graph, cached Boolean piston circuits,
explicit BUD memory nodes, and scheduled electrical output boundaries.
Inputs must be synchronized and held through the supported response/reset
episode; changes outside that protocol have undefined compiled behavior.
Default compilation and `--assume-instant` use the same runtime;
the flag relaxes construction proofs, not sampling or execution semantics.
The former wave and sequential executors are retired. Retained sequential
extraction helpers contribute dependency and admission proofs, not another
active runtime. Native/compiled partial execution remains a
proposal.
Ordinary selections can instead use the separate Native backend; the equations
below describe Direct execution, not Turbo's physical callback traversal.
Section 13 describes the restricted notification-gated activation path in the
current worktree. Its generalization remains a proposal, not a conformance claim.
Compilation success establishes that the implementation can represent a region;
it does not establish universal equivalence to every physical input history.

## Region contract

Represent a region by $\mathcal R=(F,H,\mathcal P,A)$: response functions,
retained-state transition, admitted histories, and boundary adapters.
An implementation of $F$ alone is insufficient whenever $H$ or $A$ needs
storage, sampling, reset, read-gate or clock state.

| Mechanism | Trigger and retained state | Commit/observation boundary |
| --- | --- | --- |
| Signal-driven response | Affected input changes; cached candidate $F$ | Response and scheduled electrical phases, section 6 |
| Notification-gated response | An admitted delivery; committed response $c$ | One queued recipient at a time, section 13 |
| Shared-clock memory | Recognized clock; frozen sources and old bank $q$ | Launch evaluation, atomic commit two steps later, section 7 |
| Independent memory | Delivered writer edge; current old bank $q$ | Atomic recipient group per writer; later writers see its commit, section 8 |

These mechanisms share guarded functions and electrical adapters. Their triggers,
retained state and transaction boundaries remain distinct.

## 1. State, time and observations

Use the strength domain $\mathcal S$, indicator $[P]$ and saturating subtraction
from [Redstone section 1.3](REDSTONE_MODEL.md#13-signal-algebra); write
$\mathbb B=\{0,1\}$ and $\operatorname{nz}(x)=[x>0]$.

One Direct `tick()` is one game-tick step. Units and scheduler priorities follow
[Redstone section 5.1](REDSTONE_MODEL.md#51-time-units-and-priorities).
The six-step region cycle below is a certified compiled protocol, not a universal
physical reset period. Unlike a full Direct tick, `tick_interpreted()` entered
mid-tick completes only the remaining physical phases.

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
Each region's $z_j$ includes its applicable memory bank, committed gated responses,
sampling baselines, activation FIFO, electrical phases and deadlines. Decision
caches are derived from those values and their coherent input snapshots.

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

Direct implements these maxima incrementally. Counter representation,
packed-link limits and lowering invariants are defined in
[Redpiler architecture section 7](REDPILER_ARCHITECTURE.md#7-direct-backend-and-execution-order).
Construction validates strengths, incoming edge counts and backend-local IDs.

## 3. Ordinary state transitions and scheduler

`update_node` decides whether a change needs future work; `tick_node` applies
due work using the input at execution time. They are different transition
operators, $U_v$ and $T_v$. Replacing them with a static gate equation loses
pending pulses, locking and delayed releases.

An item in $Q$ has a deadline, priority, FIFO position and node identity.
The shared [scheduler units, priorities and ring limits](REDSTONE_MODEL.md#51-time-units-and-priorities)
apply. Direct stores node identities and component pending flags; it does not use
PlotWorld's position/type deduplication. Pending guards in component handlers,
including the lamp handler, prevent duplicate requests in those branches. The
generic scheduler itself appends without deduplication.

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

An extracted channel $o$ stores terms
$\mathcal T_o=\{(\gamma_t,\operatorname{source}_t,a_t)\}$.
Its output $A_o(g,\mathbf s)$ is the shared
[guarded channel projection](REDSTONE_MODEL.md#36-guarded-electrical-channel-projection).
A term without a source denotes a strength-fifteen redstone block; conductor
materials change connectivity. All enabled contributors participate, including
fixed sources unaffected by movement.

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

The substitution equation describes signal-driven responses. Gated references
instead bind to committed $c_b$ under section 13; their newest candidates do not
replace downstream occupancy before delivery. Stored bits are not substituted by
their next values. The runtime retains each local $R_a$ and connects
actuator decisions to the corresponding response root. It computes the same
acyclic functions without expanding them into global decision diagrams.

Functions are canonical Boolean decisions under the arena's variable order.
Construction, reduction, threshold relationships and compaction are described in
[the decision-arena architecture](REDPILER_ARCHITECTURE.md#decision-arena-and-dependency-solving).
Preparation still rejects unsupported variables and unresolved dependencies.

Functions arise from geometry and electrical rules. Filename, sign label,
schematic hash, expected sum, or expected counter increment is not an execution
instruction. Arithmetic truth tables are tests of extracted functions.
The distinction matters when a layout computes a different function than its
author intended.

## 6. Cached logical response execution

Signal-driven actors in an unclocked region evaluate $F$ when bound inputs change.
Gated actors retain $c_a$ until delivery under section 13; for their occupancy,
use $f_a=c_a$ in the equations below.
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

Response, output and sampling domains use separate coherent input snapshots.
Relevant source, memory, committed-response and geometry mutations must invalidate
their dependent bindings before evaluation. Initialization captures all bindings.
Caches are derived state; they cannot replace storage or committed actuator state.
Binding and selective capture are specified in
[cached runtime plans](REDPILER_ARCHITECTURE.md#cached-runtime-plans).

Electrical guards read scheduled boundary geometry. Actors exposed to ordinary
consumers, observers or activation routes need that state; a shared payload includes
its possible owners. Compiled execution creates no native piston events or movement
entities and runs no world neighbor callbacks.

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

An ordinary timed node can provide QC data without notifying its receiver.
Such a receiver needs a represented sampling or activation route; data changes
alone cannot launch it. Section 13 describes restricted activation. Unrepresented
routes remain admission failures with either compilation flag.

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

The introductory region contract separates response, retained state, admitted
histories and boundary adapters. Composition must preserve all four.

For independent regions, $z=(z_1,\ldots,z_r)$ and each $H_j$ retains its own
bank, sampling baselines, and clock deadline while sharing the ordinary graph.
Common plot membership or an ordinary input is not a dependency between their
retained states. Payload
ownership, reset ownership, sampling routes and moving geometry do establish
dependencies. Region splitting conservatively joins those dependencies before
execution. The implementation advances the resulting runtime vector in its
stored order; it is not a global simultaneous fixed-point solver for arbitrary
mutually coupled clocks.

Storage transactions, decoder polarity, falling events and invalid observations
follow [Piston sections 4–5](PISTON_MODEL.md#4-bud-switches-as-sampled-state).
Compiled adapters must preserve their declared ordering and atomicity. A held
read adapter needs retained state; a generic word-level RAM interface is not
established by the current logical equations.

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

Use the introductory region contract when generalizing behavior. Geometry
extraction determines eligible power paths; temporal adapters determine sampling
and retained state. Ownership and stable identities must survive optimization,
and display writes must not become sampling events.

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
  by writer. Section 13 adds ordered delivery only for restricted activation
  routes; general native callback order, stored-state memory control and multiple
  memory-sampling dust writers remain unsupported.
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

<a id="13-proposed-notification-gated-instant-responses"></a>

## 13. Notification-gated instant responses

This describes source present in the current worktree, not fresh differential
validation or general native callback equivalence. The
[general activation contract](notes/NOTIFICATION_GATED_RESPONSES.md) remains a
proposal beyond the admitted subset.

### State and activation

An admitted gated actor retains committed response $c_a$ in `fired`, initialized
from entry pose. Its candidate $H_a$ is the guarded unpowered response. Gated
references bind to `Input::Committed`; other response dependencies stay in the
acyclic candidate DAG. Storage bits $q$ and decision-cache values are separate.

A data-only change dirties candidates without updating $c_a$. Compiled wire-strength
changes and boundary-pose notifications append `Delivery` records containing source,
actor, recipient, direction and head-presence eligibility. `Runtime::advance` pops
one FIFO delivery, checks current boundary eligibility, captures dirty inputs and
evaluates its recipient. It updates only that actor's committed response, marks
dependent bindings, requests boundary advancement and publishes electrical effects
before later deliveries. Eligible same-value deliveries still reach boundary processing.

### Admission and boundary limits

`activation::recognize` selects reset-observed actors with a non-notifying data
route and an independent wire notification fed by mobile geometry. Topology and
conditional extraction reject propagation across multiple dust positions.
Tables retain repeated deliveries for the represented isolated-wire walk and
explicit base/head pose notifications. They do not replay arbitrary physical
callbacks, placement, destruction or the full Wire Turbo traversal.

Activation wires produce deliveries on strength changes. Head-only recipients
require a stationary head at delivery. Boundary work uses the existing admitted
phase machine. FIFO order within a region does not prove global physical order
across interacting regions or arbitrary reset histories.

### Current source correspondence

| Operation | Implementation | Scope |
| --- | --- | --- |
| Guarded candidate | `instant/logic.rs`, Boolean arena, response plan | Admitted families |
| Stored state and old-bank writes | `MemoryCell`, clocked/independent sampling | Separate from actuator commitment |
| Coherent selective capture | `instant/logical.rs`, runtime mutation marking | Dirty bindings captured before dependent evaluation |
| Committed gated response | `Input::Committed`, `fired`, gated actor filtering | Data-only changes retain committed pose |
| Ordered delivery | `activation::Delivery`, runtime FIFO | Restricted wire/pose routes; one recipient per advance |
| Route discovery/rejection | `instant/activation.rs`, `instant/logic.rs` | Multi-wire propagation rejected |
| Electrical continuation | `backend/direct/instant/timing.rs` | Existing phases; full native acceptance equivalence unproved |

Broader admission needs ordered-route, acceptance, continuation and differential
validation evidence. Compilation success alone does not establish the proposal.
