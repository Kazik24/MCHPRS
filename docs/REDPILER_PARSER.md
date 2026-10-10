# Redpiler spatial parser

> INSTANT compiler retired: piston regions require the interpreter. The
> --assume-instant flag is no longer accepted. Piston-specific sections below
> are retained as historical research, not supported compiler behavior.

Redpiler has no textual circuit-language parser. Its circuit input is the live
world: block positions and states, block entities, pending ticks, and piston
execution state. `CompilerOptions::parse` parses command flags only. Here,
"parser" means spatial analysis, ownership/admission, and construction of the
electrical and region intermediate representations.

The entry point is [`Compiler::compile`](../crates/core/src/redpiler/mod.rs).
Read the [architecture](REDPILER_ARCHITECTURE.md) for lifecycle and the
[compiled model](REDPILER_MODEL.md) for the meaning of extracted functions.

## Input and analysis

[`analysis::analyze`](../crates/core/src/redpiler/analysis/mod.rs) normalizes
inclusive bounds, validates height and coordinate arithmetic margins, requires
loaded chunks, and visits live blocks in occupied sections. Empty sections are
skipped; inspected air cells inside occupied sections still count against the
cell budget. It does not build a dense whole-plot snapshot, consume ticks, or
move blocks. Cancellation checks cover chunk traversal and occupied rows.

Default structural budgets are 16,777,216 inspected cells, 65,536 pistons, and
4,194,304 dependency steps. The server-selected budget multiplier scales them
by one through eight. These are resource limits, not semantic circuit sizes.

For each piston, analysis records its base, facing/stickiness/extension state,
computed present power, head position, payload position, reset seeds, and
diagnostics. For an extended piston the saved payload position is two cells
along its facing; for a retracted piston it is the head cell. Reset seeds come
from nearby observer, torch and dust arrangements. A seed is a place to start
following dependencies, not evidence that a whole mechanism is valid.

Pistons are ordered by `(y,z,x)`. Union-find merges actors whose possible near
or far payload positions overlap, producing shared payload ownership groups.
Group identity therefore follows geometry rather than labels, signs, schematic
names, or one actor's apparent current ownership.

The report separately records observers, unmatched stationary heads, moving
pistons, entry phase, pending piston events, active motions, and movement work.
Every piston and observer initially carries a runtime-ownership requirement.
For ordinary circuits `can_compile()` means the issue list is empty. For piston
circuits executable preparation must establish ownership and satisfy entry
requirements; `can_compile()` is not the final admission predicate.

## Structural dependency views

[`Topology`](../crates/core/src/redpiler/analysis/topology.rs) performs bounded,
cancellable reads. Queries preserve missing context as `outside_bounds`; they
do not quietly treat a smaller selection's missing dependencies as zero power.
`PowerDependencies` contains source positions, source kind, attenuation, direct
versus quasi-connectivity route, wire aliases, and out-of-bounds positions.
Normalization keeps the shortest dependency for each source and route.

Piston input search reads the base's receiving faces except its front, plus all
receiving faces around the cell above for quasi-connectivity. Static dependency
search shares the interpreter's weak/strong power predicates. Conductors pass
strong power; dust traversals account for horizontal connections and supported
up/down steps. Distances of 15 or more cannot supply power and stop traversal.
A possible payload cell has `MobilePayload { group }` identity even when it
currently contains a piston head. It is not a fixed redstone constant.

[`families::recognize`](../crates/core/src/redpiler/analysis/families.rs)
recognizes observer-above, torch, under-head dust and lateral-dust reset paths.
Its ready-mechanism checks cover stationary head compatibility, supported
payload, moving entities, ready power, direction, required bounds, reset source
activity and pending work. Recognition follows return paths and support
conduction; local proximity alone is insufficient.

Observer recognition checks a down-facing observer, fixed conducting cap,
return power path, and competing writers. It permits a prepared floor lever on
the cap only when the same source reaches the base with no greater attenuation.
Torch recognition verifies that all controlling sources fall below their
threshold whenever piston power falls. Dust recognition verifies that the
near payload supplies the return network and that other writers are absent.
Shared-payload group recognition checks that one supported payload exists and
each member's reset supply remains valid for every possible near owner.

These diagnostics describe the current ready-reset matcher. The sampled
sequential compiler can admit some states that this matcher rejects; its
admission checks and local transition representation are separate.

[`ports::discover`](../crates/core/src/redpiler/analysis/ports.rs) builds a
second dependency view for notifications. Dust updates can recheck pistons at
one- and two-face offsets, including wires that contribute no electrical power.
Adjacent piston-head changes also contribute notifications. The resulting
`UpdateDependency.independent_of_power` is intentional: an unchanged low input
can be sampled by an independent update. Neither electrical main nor side links
encode that event.

Output interfaces start at actual receiving faces of repeaters, comparators,
torches, lamps, trapdoors, note blocks and command blocks. A comparator main
channel with a fixed inventory override remains an analog read. Discovery stops
at ordinary timed consumers; a lamp behind a repeater is downstream ordinary
logic rather than a duplicate piston-output port. Reset power visible outside
its ownership group is recorded as a reset exposure.

## Ordinary graph construction

The mandatory stages in [`passes`](../crates/core/src/redpiler/passes/mod.rs)
produce `CompileGraph`:

1. [`identify_nodes`](../crates/core/src/redpiler/passes/identify_nodes.rs)
   creates component nodes from live blocks and entities. It initializes saved
   output strengths, powered state and repeater locking. Buttons, levers and
   pressure plates are inputs; lamps, trapdoors, note blocks and command blocks
   are outputs. Far command-block comparator reads are also protected outputs.
   Note blocks retain output nodes even when blocked; playback checks live
   obstruction during flushing. Inventory/comparator overrides
   become constants except command blocks, which keep live output identity.
   `--optimize` omits ordinary wire display nodes.
2. [`input_search`](../crates/core/src/redpiler/passes/input_search.rs)
   builds source-to-consumer links by following receiving faces, weak power,
   strong power through conductors, and breadth-first wire distance. Repeater
   side inputs accept directed diode outputs; comparator side inputs also
   accept dust and redstone blocks. Each edge retains its `Default`/`Side`
   channel and attenuation. Multiple paths can initially create parallel links.
3. [`clamp_weights`](../crates/core/src/redpiler/passes/clamp_weights.rs)
   removes links with attenuation at least 15.

Ordinary dust nodes are strength displays. Input search crosses dust to the
real source instead of requiring consumers to propagate through display nodes.
The wire search stops before distance arithmetic could overflow and before
visiting paths whose power is necessarily zero.

Identification uses input bounds, while ordinary input search queries the
world through the `World` interface. If an encountered electrical source has
no graph owner, it returns `MissingSource`; it does not silently create an
unbounded source node. Plot-world isolation determines what outside the plot
reads return. A smaller compiler selection must include its relevant sources.

Pending entries that retain matching block identity mark corresponding nodes
as `pending_tick`. Optional rewrites run only after this initialization, as
described in the [optimizer document](REDPILER_OPTIMIZER.md).

## Region partition and admission

[`regions::split`](../crates/core/src/redpiler/instant/regions.rs) merges
pistons through overlapping owned geometry/reset positions, shared payloads,
and reads of owned positions. It follows independent notification wires and
their upstream sources so BUD cells stay connected to their sampling clock.
It additionally joins nearby moving geometry and dependency positions within
two face steps. Observers need a supported region owner. Each resulting report
remaps actor/group IDs, mobile dependencies and ports consistently.

Executable preparation rejects non-between-tick entry, active motion/events or
movement work, and selection spanning plots. Binary instant export is rejected.
Each region prepares the same cached logical runtime: guarded acyclic responses,
optional recognized clock/storage, independent sampling and restricted activation.
`--assume-instant` relaxes construction/reset proofs while retaining geometry,
ownership, context, sampling and dependency validation. Rejected piston regions
have no sequential or native execution fallback.

[`program::prepare_region`](../crates/core/src/redpiler/instant/program.rs)
recognizes clocks, reset candidates, gated actors and independent memory before
extracting expressions. Binding is a later admission step; recognition alone
cannot authorize execution. Admission and transaction contracts are in
[the compiled model](REDPILER_MODEL.md#6-cached-logical-response-execution).

## Conditional geometry extraction

[`instant/logic.rs`](../crates/core/src/redpiler/instant/logic.rs) maps possible
occupancy into guarded blocks. A far payload may disappear, a head may be
removed or replaced by a near payload, and an extended base may become
retracted or moving. Fixed conductors can therefore change circuit connectivity
without emitting redstone themselves.

For an acyclic response, another actor's firing temporarily removes its mobile
payload's supply and conduction; an actor's own payload is excluded from its
initial firing predicate. Memory geometry is an explicit stored variable; gated
references use committed response state. Data and notification-route discovery
remain separate.
Wire shape is recomputed under conditional neighbor configurations using the
shared wire shape functions. Equivalent shapes and blocks share a disjunction
of guards. Conditional dust walks carry both distance and a Boolean path guard;
they retain uncovered configurations rather than treating one visit as valid
for every geometry.

The Boolean response of actor `i` is the negation of its extracted powered
predicate. Ordinary source power is represented by threshold variables
`strength(source) > attenuation`, not a blanket Boolean input. The extractor
builds an actuator-dependency graph, uses a deterministic ready queue for
topological substitution, and rejects unresolved cycles. Finished acyclic
responses contain ordinary signal and memory variables, with no provisional
actuator variables.

Consumer output extraction preserves analog values with `PowerTerm` entries:
`(guard, source, attenuation)`. A missing source denotes a redstone block with
strength 15. The port strength is the maximum attenuated strength among enabled
terms. Main and comparator side channels are separate ports. Changing geometry
around an inventory override requires an unsupported dynamic-read protocol and
is rejected on the ready acyclic path; a fixed rear override stays ordinary.

Decision representation and compaction are specified in
[the solver architecture](REDPILER_ARCHITECTURE.md#decision-arena-and-dependency-solving).
Executable extraction retains local candidate DAG edges; standalone helpers can
substitute resolved actors. Gated references cut at committed state. The remaining
candidate graph must still be acyclic.

## Sampled sequential extraction

This retained extractor supplies dependency and reset-certification analysis.
It is not an active executor or a fallback for rejected regions.

[`logic/sequential.rs`](../crates/core/src/redpiler/instant/logic/sequential.rs)
uses the same conditional geometry engine while retaining local boundaries:
geometry variables, observer state, wire dot/connection state, and current wire
strengths. It does not substitute cyclic actuator dependencies into one global
function. Piston responses, consumer ports and wire sensors remain separate
local power expressions.

Wire discovery follows power dependencies, independent notification nets,
observer-watched dust, and wires adjacent to changing base/head/payload cells.
The last set can lose saved power through geometry notifications even without
a mobile power dependency. Discovery expands sensor dependencies until all
required wire sources and output terms have owners. Each sensor stores initial strength,
initial shape, and guarded alternative shapes. Each payload group requires one
supported payload or an empty ordinary generator, and shared actors must agree
on a common far destination. Unsupported nonair occupancy in possible payload
positions is rejected; saved actor heads have their own compatibility checks.

[`instant/sequential.rs`](../crates/core/src/redpiler/instant/sequential.rs)
collects retained expression dependencies. Storage delivery comes from `sampling.rs`;
gated delivery comes from `activation.rs`. Electrical graph edges do not represent
implicit sampling events.

## Graph boundary and lowering

[`Boundaries`](../crates/core/src/redpiler/instant/boundary.rs) supplies the
ordinary graph builder with ownership and required external identities:

- Mobile aliases become dynamic `MobileSource` nodes.
- Region-owned reset, geometry and wire positions are omitted from ordinary
  node identification.
- Required ordinary sources are retained for backend binding; receiving
  components are protected outputs.
- Each projected consumer/channel receives an `InstantOutput` source and a
  zero-attenuation link. Ordinary input discovery skips the replaced channel
  so it cannot also compute the same conditional input.
- Sequential sampled wires are identified as runtime-owned sensors rather
  than unresolved ordinary graph sources.

Diagnostic graphs may also contain an aggregate `InstantInput` per actor.
These nodes describe an electrical interface only and are never a substitute
for notification tables or an executable region program.

Direct lowering assigns dense IDs, initializes strength counters, binds
program sources and output ports, and rejects missing bindings or unresolved
variables. Successful parsing therefore establishes both graph connectivity
and region execution ownership. It does not merely find nearby blocks and
trust the runtime to recover their meaning later.

## General contracts and current recognition limits

Circuit identity should be derived from material properties, receiving faces,
possible occupancy, delivered notifications and state ownership. Fixtures can
exercise those properties but must not become the definition of a circuit.

The current ready matcher still has deliberately narrow cases: supported
payloads are a material whitelist; a stationary furnace container is permitted
as conducting support while other entity-bearing supports reject; prepared
observer-cap inhibition is recognized as a floor lever in a particular direct
position. The general contracts are fixed material behavior, unchanged analog
reads, no moving entity state, and a verified threshold/return relationship.
Those contracts explain the restrictions, but the implementation has not
generalized every case. Document or change the actual gate when adding support;
do not claim all solid blocks, all block entities, or all inhibit arrangements
are interchangeable.
