# Retired notification-gated instant response proposal

Status: retained research note; this execution path has been removed. Piston
builds are rejected by Redpiler and remain interpreter-only. The equations
below describe the former proposal, not current compiler behavior or evidence
of physical equivalence. True storage remains separate from committed response.

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
retained their separate old-bank sampling contract in the former executor.

### Boundary and selective-capture contract

In the proposal, internal dust had no physical runtime executor. Compiled
notification routes produced activation events; ordinary sources provided
live electrical inputs.
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

## Remaining obligations

The active model records current source correspondence. Broader admission needs
proofs and differential checks for multi-wire order, callback multiplicity,
acceptance/cancellation, interacting regions and reset continuation. Selective
capture must cover source, memory, committed-response and geometry mutations.
A queued activation must survive an empty dirty-binding set.

Storage writes in the proposal kept the clocked/independent old-bank transaction
boundaries described by the former executor.
General native/compiled partial ownership is outside this activation contract.
