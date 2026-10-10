# Redpiler optimizer

> INSTANT compiler retired: piston regions require the interpreter. The
> --assume-instant flag is no longer accepted. Piston-specific sections below
> are retained as historical research, not supported compiler behavior.

Redpiler first extracts the graph described in the
[parser document](REDPILER_PARSER.md), then optionally rewrites it. Its optimizer
is a fixed sequence of graph passes, not an adaptive planner or a global
equivalence prover. Instant logic also performs Boolean reduction during
extraction; that happens independently of `--optimize`.

The authoritative order and enable conditions are in
[`passes::run_passes`](../crates/core/src/redpiler/passes/mod.rs). The command
currently warns that optimization is unstable. The exact implemented guards
and remaining correctness limitations are recorded below so an agent does not
mistake the intended optimization for a general theorem.

## Semantic contract

An electrical edge `e = (u,v,c,w)` contributes

$$
a_e(t)=\max(0,s_u(t)-w)
$$

to receiving channel `c`, where strengths range from 0 to 15. The channel value
is the maximum of its incoming contributions. Equivalence must preserve the
observable transition trace, including scheduled deadlines, priority,
repeater lock state, command/note events, region sampling, and state exported
on reset. Matching a final Boolean truth table alone does not prove that trace
equivalence.

[`CompileNode::is_removable`](../crates/core/src/redpiler/compile_graph.rs)
returns false for graph inputs, graph outputs, pending-tick nodes, and all
`InstantInput`, `MobileSource` and `InstantOutput` nodes. Required region graph
sources and receiving components are marked as retained interfaces before
optimization. Constant folding has its own conditions; it does not use
`is_removable` to decide whether a diode or torch can change type.

## Ordered pipeline

| Order | Pass | Enabled |
| --- | --- | --- |
| 1 | Identify nodes | Always |
| 2 | Search inputs | Always |
| 3 | Clamp weights | Always |
| 4 | Deduplicate links | `--optimize` |
| 5 | Fold constants | `--optimize` |
| 6 | Prune unreachable comparator outputs | `--optimize` |
| 7 | Coalesce constants | `--optimize` |
| 8 | Coalesce logic | `--optimize` |
| 9 | Prune orphans | `--optimize --io-only` |
| 10 | Export graph | `--export` |

There is no whole-pipeline fixed point. Only constant folding repeats internally
until it changes no further nodes. Later coalescing can introduce parallel
edges; the earlier deduplication pass is not rerun. Ten pass slots, including
skipped passes, advance monitor progress; backend lowering is the eleventh
step. Enabled passes check cancellation before starting.

## Identification, input search and weight clamping

[`identify_nodes`](../crates/core/src/redpiler/passes/identify_nodes.rs)
omits ordinary wire display nodes when optimization is enabled. Input search
still traverses physical dust, so this removes display work rather than dust's
attenuation. It is observable in world presentation: intermediate wire powers
are not maintained by removed graph nodes.

[`input_search`](../crates/core/src/redpiler/passes/input_search.rs) creates
attenuated main/side links and respects region ownership. Boundary sources stay
dynamic even when their initial strength is 15. Wire searches stop at distance
15 before incrementing distances further.

[`clamp_weights`](../crates/core/src/redpiler/passes/clamp_weights.rs)
removes every edge with `w >= 15`. This follows directly from `s <= 15`, so
such an edge contributes zero for every permitted strength.

After clamping, ordinary selections containing any comparator or directed cycle
select native propagation and skip optional rewrites. Collapsed dust links cannot
represent intermediate dust states, unchanged-strength comparator notifications,
or the native order of callbacks with equal deadlines and priorities. The native
backend retains physical components, dust topology and live strengths, and runs
the existing interpreter logic over a private snapshot. This supports comparator
feedback, acyclic shared-input forks and torch oscillators with either setting
of `-O`. Flush frequency and `--io-only` affect display only; reset restores all
physical state and pending work.

The initial selector is deliberately broad: the complete ordinary selection uses
one native queue if it contains comparators or feedback. Statistics report this
choice. Acyclic selections without comparators retain the fast graph path.
Native selections reject graph export, whose format cannot preserve these event
semantics. Instant piston compilation retains the comparator cycle/shared-input
guards until native propagation can share its scheduler and geometry changes.

A supported feedback example, viewed from above with west on the left:

```text
       x0 x1 x2 x3
z=-1    .  W  W  .
z= 0    V  A  W  .
z= 1    .  T  B  L
```

Place all blocks on stone supports. `W` is dust, `V` a floor lever, `T` a standing
redstone torch, and `L` an optional lamp. Both comparators point east (rear input
on the west, block-state `facing=west`) and use subtract mode. Turn the lever on.
A's output returns through dust to its own side and also drives B's side input.

## Link deduplication

[`dedup_links`](../crates/core/src/redpiler/passes/dedup_links.rs) removes
an incoming edge if another edge has the same source, target and channel with
no greater attenuation. Equal duplicates reduce to one survivor. Main and side
links never deduplicate against each other.

For `w_1 <= w_2`,

$$
\max(0,s-w_1)\geq\max(0,s-w_2)
$$

at every valid strength, so the larger weight cannot change a channel's maximum.
The implementation scans parallel incoming links rather than maintaining a
global edge index.

## Constant folding

[`constant_fold`](../crates/core/src/redpiler/passes/constant_fold.rs)
considers only comparators, repeaters and torches. Every incoming source must
already be `Constant`; an empty input set is permitted. It skips nodes with
pending ticks and physical comparators whose far cell is a command block.
The main and side strengths include edge attenuation.

Its candidate output is:

$$
\begin{aligned}
\operatorname{torch}(m)&=\begin{cases}15&m=0\\0&m>0\end{cases},\\
\operatorname{repeater}(m,L,s)&=\begin{cases}s&L\\15&\neg L\land m>0\\0&\text{otherwise}\end{cases},\\
\operatorname{compare}(m,d)&=\begin{cases}m&m\geq d\\0&m<d\end{cases},\\
\operatorname{subtract}(m,d)&=\max(0,m-d).
\end{aligned}
$$

A comparator's saved far override replaces `m` when `m < 15`. A repeater uses
its saved lock flag. Folding occurs only if the candidate equals the current
saved output strength: a differing eventual constant still requires runtime
timing and propagation. The pass changes the node type to `Constant` and
removes incoming edges, preserving outgoing links and the rest of its saved
state. Repeated sweeps allow newly constant nodes to expose later folds.

This is a settled-state transformation. Its equality check does not separately
prove that the saved lock/powered flags are consistent with all constant inputs
for an arbitrarily malformed or unsampled world. Do not use folding as an
implicit world-normalization operation.

## Comparator-output reachability

[`unreachable_output`](../crates/core/src/redpiler/passes/unreachable_output.rs)
considers subtract-mode comparators without pending ticks and with exactly one
incoming side edge whose source is a constant. For source strength `k`, side
attenuation `w_s`, and saved comparator output `s`, its output bound is

$$
d=\max(0,k-w_s),\qquad M=\max(s,15-d).
$$

Outgoing links with attenuation at least `M` cannot carry either the current
output or a future output and are removed. A constant 15 reaching the side with
attenuation 1 supplies 14, so an outgoing link of attenuation zero is retained.
Including `s` preserves an existing output that has not yet settled.

## Constant coalescing

[`constant_coalesce`](../crates/core/src/redpiler/passes/constant_coalesce.rs)
builds undirected connected components from graph edges except those leaving
removable constants. For each component and strength, it creates one positionless
replacement constant and transfers the original outgoing edges to it. A
constant feeding several components can produce one replacement in each.
Original removable constants are removed, including ones with no outputs.
Protected constants remain distinct.

This preserves edge channels and weights. Sharing a constant across the entire
plot would spuriously join independent subgraphs, which is why the key is
`(consumer component, strength)` rather than strength alone. Replacement-node
tracking handles stable-graph slot reuse so a synthesized constant is not
visited as another removable original during the same pass.

## Logic coalescing

[`coalesce`](../crates/core/src/redpiler/passes/coalesce.rs) chooses a
removable non-comparator representative with exactly one incoming edge. That
edge must be `Default`, and its source must not be a comparator. It walks other
outputs of that source and merges a sibling when:

- its node type exactly equals the representative's type;
- its entire `NodeState` equals the representative's state;
- it is removable and has exactly one incoming edge;
- its incoming edge is `Default` and has equivalent attenuation.

The sibling's outgoing edges are moved to the representative and the sibling
is removed. Its physical block and any prior aliases stay attached to the
representative. Display flushing writes every alias; reset restores their current
states and exports pending ticks to every original position with the same
remaining delay and priority. Type equality includes repeater delay/facing-diode information;
state equality includes powered, locked, strength and pending-tick state.
Distinct command-block and note-block outputs are protected from merging.

Attenuation must match unless both nodes are Boolean consumers (repeaters or
torches) of a source that only outputs 0 or 15, and both attenuations are below
15. Those inputs have the same on/off behavior. Analog inputs and wire outputs
require equal attenuation. Equal initial states alone would not justify
merging different receiving functions, or a repeater's main input with another
repeater's locking input.

## Orphan pruning

[`prune_orphans`](../crates/core/src/redpiler/passes/prune_orphans.rs)
starts from every nonremovable node and walks all incoming dependencies. It
removes nodes outside that closure. Its roots include inputs, outputs, pending
work, required region sources and all instant boundary types; the pass is not
merely a backwards lamp reachability search.

This runs only with both optimization and I/O-only presentation, because
internal display states otherwise remain observable. An unrelated input still
survives because user interaction is itself a retained interface. Pending work
survives even if it currently has no route to an output.

## Export and lowering

[`export_graph`](../crates/core/src/redpiler/passes/export_graph.rs)
serializes the final supported graph to `redpiler_graph.bc`; export is not an
optimization. It rejects command-block and instant boundary nodes. Executable
instant preparation rejects binary export before this pass. Backend DOT export
happens later during lowering and represents the direct backend, not a
round-trippable circuit program.

[`direct::compile`](../crates/core/src/redpiler/backend/direct/compile.rs)
validates strengths and counter fan-in, creates dense node IDs, initializes
channel counters, binds region interfaces, and restores scheduled ticks. It
does not repair a semantically invalid optimizer rewrite. Constant nodes do
not retain runtime forward-update links because their output cannot change.

## Boolean simplification of instant programs

[`BooleanArena`](../crates/core/src/redpiler/instant/boolean.rs) reduces equal
branches, shares identical ordered decisions, caches conjunction/inversion,
substitutes resolved acyclic actuator functions, and compacts the final roots.
Complementary geometry paths can cancel a provisional actuator dependency
before cycle detection. This is semantic Boolean simplification over explicit
signal thresholds and state variables, independent of ordinary graph flags.

The sequential extractor deliberately retains state boundaries instead of
inlining a storage feedback loop into one function. Region ownership protects
physical aliases and required graph ports from ordinary removal. Timing,
notifications, memory and reset materialization remain additional contracts;
decision reduction alone does not validate them.

The certified `--assume-instant` executor lowers these decisions into separately
cached response and output programs. Input changes invalidate dependent
decisions; needed branches and shared subexpressions are evaluated once, while
unchanged regions are skipped using the backend's source dependency index.
Memory changes only at a recognized sample event, with old-bank reads preceding
atomic commit. This runtime preparation is independent of `--optimize`, which
still controls ordinary graph passes. The isolated `mchprs_redpiler_opt` crate
is not connected to the server, and these decision programs are not a retained
full-net reference representation.

## Maintaining correctness

The reusable optimization criterion is observable transition equivalence under
the [compiled model](REDPILER_MODEL.md), including initial state and handoff.
Use actual attenuated channel strengths for bounds; include side/main identity
and timing in merge proofs; preserve pending work and retained interfaces.
The limitations above are present implementation constraints, not reasons to
add circuit-name exceptions. Checks should vary analog strength, path length,
saved state and scheduler order across the relevant family. See the
[test guide](REDPILER_ARCHITECTURE.md#9-commands-limits-and-verification) for existing runnable suites.
