# Redpiler architecture

Redpiler compiles a live redstone world into an electrical graph and, when
pistons are present, logical region programs. The direct backend executes those
representations without repeating spatial power searches or interpreting piston
movement on each step. Ordinary components keep their scheduled timing; admitted
piston regions evaluate conditional geometry and explicitly sampled state.

This document describes the current Rust implementation. The source is the
authority; structural recognition, logical execution, and compatibility with a
physical Minecraft waveform are different claims. Detailed companion references
are the [parser](REDPILER_PARSER.md),
[optimizer](REDPILER_OPTIMIZER.md),
[compiled model](REDPILER_MODEL.md), and
[physical piston model](PISTON_MODEL.md).

## 1. Implementation map

| Responsibility | Implementation | Result |
| --- | --- | --- |
| Compilation lifecycle | [redpiler/mod.rs](../crates/core/src/redpiler/mod.rs) | Options, activation, diagnostics, statistics, reset |
| Structural inference | [analysis/](../crates/core/src/redpiler/analysis/mod.rs) | Piston geometry, payload ownership, reset candidates, power and update dependencies |
| Region admission | [instant/program.rs](../crates/core/src/redpiler/instant/program.rs), [regions.rs](../crates/core/src/redpiler/instant/regions.rs) | Validated region programs and ordinary graph boundaries |
| Conditional geometry solver | [instant/logic.rs](../crates/core/src/redpiler/instant/logic.rs), [boolean.rs](../crates/core/src/redpiler/instant/boolean.rs) | Boolean response DAG, guarded electrical outputs, handoff expressions |
| Observer reset proof | [instant/observer.rs](../crates/core/src/redpiler/instant/observer.rs), [logic/sequential.rs](../crates/core/src/redpiler/instant/logic/sequential.rs) | Owned reset circuitry and certified response actors |
| Clock and storage inference | [instant/clocked.rs](../crates/core/src/redpiler/instant/clocked.rs), [sampling.rs](../crates/core/src/redpiler/instant/sampling.rs) | Memory cells and their explicit write events |
| Electrical graph optimization | [passes/](../crates/core/src/redpiler/passes/mod.rs) | Prepared and optionally reduced `CompileGraph` |
| Runtime lowering | [backend/direct/compile.rs](../crates/core/src/redpiler/backend/direct/compile.rs) | Dense nodes, packed links, region bindings, transferred ticks |
| Electrical execution | [backend/direct/mod.rs](../crates/core/src/redpiler/backend/direct/mod.rs), [update.rs](../crates/core/src/redpiler/backend/direct/update.rs), [tick.rs](../crates/core/src/redpiler/backend/direct/tick.rs) | Input propagation, scheduled component transitions, world output events |
| Logical piston execution | [backend/direct/instant.rs](../crates/core/src/redpiler/backend/direct/instant.rs), [instant/logical.rs](../crates/core/src/redpiler/backend/direct/instant/logical.rs) | Cached decisions, frozen snapshots, memory commits, settled geometry |
| Independent Boolean optimizer | [redpiler_opt/src/lib.rs](../crates/redpiler_opt/src/lib.rs) | Pure Boolean plans; currently separate from the server compiler |

## 2. Compilation lifecycle and representations

[`Compiler::compile`](../crates/core/src/redpiler/mod.rs) accepts a world view,
inclusive bounds, saved `TickEntry` work, `CompilerOptions`, and a shared
`TaskMonitor`. The server command supplies the current plot's corners.

```mermaid
flowchart TD
    W[Live world and scheduled ticks] --> A[Read-only analysis]
    A --> P{Pistons present?}
    P -->|No| G[Ordinary graph preparation]
    P -->|Yes| I[Split regions and infer roles]
    I --> C[Certify construction and sampling boundaries]
    C --> S[Extract conditional geometry and response DAG]
    S --> G
    G --> O[Optional electrical graph passes]
    O --> B[Lower fresh direct backend and bind logical plans]
    B --> R[Publish active compiler]
    R --> T[Scheduled electrical work and logical region evaluation]
    T --> H[Reset: export settled state and return remaining ticks]
    H --> W
```

Analysis inventories geometry without mutating the world. With no pistons, an
issue-free report goes directly to graph preparation. With pistons, region
preparation must resolve the report's runtime ownership requirements and reject
other entry issues. Scheduled ticks whose saved block type differs from the
current block are filtered out before graph construction.

| Representation | Contents | Can execute? |
| --- | --- | --- |
| `AnalysisReport` | Geometry, ownership groups, recognition results, ports, admission issues | No |
| `CandidateGraph` | Diagnostic graph and fresh analysis report | No |
| `CompileGraph` | `StableGraph<CompileNode, CompileLink>` with component state and electrical edges | Only after lowering |
| `PreparedInstant` | Pistons, payload groups, aliases, owned positions, `WaveLogic`, clocks, memory, sampling, handoff context | Only after binding |
| `WaveLogic` | Decision arena, response roots/order, sources, output terms and settled dust expressions | Compiler representation |
| `DirectBackend` | Fixed nodes, packed forward links, tick scheduler, event queues and bound region runtimes | Yes |

The compiler stages a fresh backend and publishes it only after all preparation,
binding, and cancellation checks succeed. `backend.is_some()` defines active
state. Calling `compile` while active returns `AlreadyActive`; the `/rp compile`
command resets an existing compiler first. A failed attempt never publishes
partial statistics or consumes interpreter scheduling ownership. Requested
export files are separate side effects and are not rolled back on later failure.

[`Plot::start_redpiler`](../crates/core/src/plot/mod.rs) compiles in a scoped worker
while servicing player connections. Success transfers simulation ownership by
clearing the world's scheduler and invalidating its tick index. The plot also
closes containers, disables plot history when enabled, and updates its scoreboard.
The command currently uses a default monitor without interactive cancellation.

## 3. Inference engine: deriving behavior from blocks

Inference is implemented as bounded spatial queries and explicit recognition
rules. It derives roles from connectivity and material properties, rather than
schematic names, signs, known arithmetic functions, or bit coordinates.

### Geometry and ownership

[`analysis::analyze`](../crates/core/src/redpiler/analysis/mod.rs) normalizes bounds,
requires loaded chunks, skips empty sections, and inventories live pistons,
heads, observers, consumers, and pending work. Each `PistonDescriptor` records
the base, facing, saved pose, present power, head, payload, reset seeds, and
diagnostics. Entry power can differ from the saved pose: that can be a stored BUD
state, so a power query alone cannot determine execution behavior.

Union-find groups pistons whose possible near/far payload positions overlap.
This preserves one payload identity across physical aliases. An empty ordinary
generator has a base/head footprint rather than an assumed pulled far payload.

[`Topology`](../crates/core/src/redpiler/analysis/topology.rs) follows directional
weak/strong power, conductors, and dust attenuation. It labels direct versus
quasi-connectivity routes and ordinary, constant, observer, or mobile sources.
Mobile redstone blocks remain dynamic even when their saved strength is 15.

### Reset recognition and notification channels

[`families.rs`](../crates/core/src/redpiler/analysis/families.rs) recognizes observer
above, torch, dust below head, and lateral dust reset paths. It records support,
return routes, supply ownership, and failures such as active reset work or an
additional writer. A matched path is a structural candidate, not authorization
to execute it.

[`ports.rs`](../crates/core/src/redpiler/analysis/ports.rs) separately discovers
electrical inputs, wire/head/base notifications, ordinary consumer interfaces, and
exposed reset signals. Quasi-connectivity may supply power without delivering a
base recheck. Conversely, a qualifying update may sample unchanged low data.
Electrical strength and delivered sampling events therefore use separate
channels; an ordinary graph `Side` edge means diode side input, never BUD sampling.
Base-to-head routes carry a head-presence condition. Discovery of such a route
does not certify its movement/reset timing or make it executable by the instant
sampler; an unsupported route reports its source and receiver explicitly.

[`regions::split`](../crates/core/src/redpiler/instant/regions.rs) joins actors by
shared payload/reset ownership, electrical influence, observer connectivity, and
sampling dependencies. It conservatively joins moving geometry within two cells
of a dependency, which can merge otherwise independent nearby circuits. Sharing
an ordinary control alone does not establish shared moving ownership.

### Storage and clock inference

[`clocked::recognize`](../crates/core/src/redpiler/instant/clocked.rs) recognizes a
specific empty downward observer-clock generator and its independently sampled
BUD bank. It requires one owned generator per such region, a sampling observer,
and supported memory geometry. Validation requires a single ordinary torch
control for the response network and proves that clock control is independent
of stored data and releases when torch power disappears.

[`sampling::recognize`](../crates/core/src/redpiler/instant/sampling.rs) identifies
independent memory cells, empty notification generators, and delivered write
events. A source can be a generator pose change or a strength change on static
dust driven by one fixed writer. Head-delivered targets retain a
`requires_extended` condition. Sampling dust depending on moving geometry or
another writer is rejected rather than treated as an implicit clock.

## 4. Solver: conditional geometry and Boolean responses

The solver combines a reduced ordered Boolean decision arena with an acyclic
actuator dependency graph. It does not invoke an external SAT/SMT engine or solve
arbitrary feedback by searching for a fixed point.

### Extracting conditional electrical paths

[`Extractor`](../crates/core/src/redpiler/instant/logic.rs) maps bases, near cells,
and far cells to actors and payload groups. A spatial position can have several
possible blocks, each guarded by a Boolean expression. It enumerates local owner
assignments, merges identical block variants, derives conditional dust shapes,
and walks electrical paths through those variants.

Dust searches carry `(position, attenuation, guard)`. False guards and paths
attenuated by 15 are discarded. A per-position Boolean coverage expression avoids
walking already covered conditions again. Power queries include direct input
and quasi-connectivity above the piston.

For a source strength `s`, path attenuation `w`, and geometry guard `g`, a
Boolean powered contribution is:

```text
g AND (s > w)
```

The extracted response of actor `i` is the negation of its combined powered
input. `true` denotes a retracted/fired logical pose; it is not a universal
inversion of ordinary electrical output polarity. Memory reads use held state,
while response actors can depend on other response actors.

### Decision arena and dependency solving

[`BooleanArena`](../crates/core/src/redpiler/instant/boolean.rs) represents false
and true as expression IDs 0 and 1. Every other expression references
`Decision { variable, low, high }`. Variables include signal thresholds,
provisional actuator responses, memory, and geometry; the certification oracle
also uses observer and wire-dot variables.

The arena reduces equal branches, interns identical decisions, memoizes AND/NOT,
and builds OR/select from these operations. Thus complementary geometry paths
can cancel a provisional dependency before cycle detection. Signal thresholds
are separate Boolean tests evaluated against the same actual strength; the arena
does not impose a global theory of their threshold relationships.

Extraction collects actuator dependencies from the reduced roots and orders
them with a deterministic topological traversal. In executable preparation it
retains local response DAG edges to avoid expanding a whole circuit into a large
BDD. The standalone first-response extraction path can instead substitute
already resolved responses into pure port functions. Unresolved actors cause a
position-bearing cycle error asking for a sampled memory or clock boundary.

Final compaction retains decisions reachable from response roots, electrical
output guards, sampling guards, and handoff dust expressions. Restoration-only
decisions remain in the arena but need not enter the active runtime plans.

### Observer certification as conservative inference

[`observer::certify`](../crates/core/src/redpiler/instant/observer.rs) uses
[`logic::sequential::extract`](../crates/core/src/redpiler/instant/logic/sequential.rs)
as a compile-time oracle for local wire sensors, observer inputs, geometry, and
notifications. It finds the reset influence cone, checks exposure to ordinary
outputs and independent samplers, and identifies electrical and notification-only
recipients.

In default admission, guaranteed pulse strength lower bounds increase until
stable. `fixed_value` follows known conditions and explores both branches for
unknown data or geometry; a definite answer must agree on those branches. Each
electrically affected response must be guaranteed to extend under the reset
pulse. Notification-only recipients must have coupled independent data rather
than hidden storage. Final response extraction still has to prove acyclicity.

The sequential extractor is a certification oracle, **not a physical runtime
fallback**. The adjacent `instant/sequential.rs` module collects expression
dependencies. Their names do not indicate an active movement executor.

## 5. Instant piston admission and execution

[`program::prepare`](../crates/core/src/redpiler/instant/program.rs) prepares all
regions before running the shared electrical graph pipeline. It requires entry
between ticks, no active piston events/motions/movement work, and a selection
within one plot. Region preparation validates payload identity and destination,
saved heads or retained near payloads, block entities, attachments using moving
support, selection context, reset ownership, and sampling representability.

Supported payloads are redstone blocks or fixed opaque solid cubes with no block
entity, neighbor-update behavior, analog override, or unknown state. Shared
payload groups need exactly one supported block and a common destination;
recognized empty generators are an exception. Moving comparator inventory reads
are rejected because ordinary electrical power cannot represent that analog read.

The two admission policies select the same logical executor:

| Policy | Additional admission behavior | Runtime |
| --- | --- | --- |
| Default | Requires construction/reset evidence; checks reset pulse guarantees, exposed reset signals, and additional direction/response guards | Cached logical regions |
| `--assume-instant` | Trusts instant/BUD construction and skips selected reset/direction proofs; still validates geometry, ownership, sampling, data dependencies, and acyclicity | The same cached logical regions |

Default admission does not enable movement delays or replay physical reset
waveforms. [`Runtime::bind`](../crates/core/src/redpiler/backend/direct/instant.rs)
unconditionally creates `logical::State`, and preparation always uses
`extract_ideal_with_state`. Older first-response extraction helpers and physical
interpreter tests exist, but do not define the active backend's piston timing.

### Connecting logical regions to the ordinary graph

[`Boundaries`](../crates/core/src/redpiler/instant/boundary.rs) removes owned
internals from ordinary node identification, retains ordinary sources needed for
binding, and supplies dynamic mobile aliases and projected consumer outputs.
`MobileSource` and `InstantOutput` lower to `InstantSource` runtime nodes.
`InstantInput` is diagnostic and is not an executable node.

Each output is a list of `PowerTerm { guard, source, attenuation }`, where an
absent source means a full-strength redstone block. The receiving strength is:

```text
max(enabled terms: source_strength.saturating_sub(attenuation)), or 0
```

Main and comparator-side channels remain distinct. Moving a conductor can gate
an ordinary source without reducing it to a Boolean 0/15 output. Output guards
read settled geometry; moving-base occupancy is always false in this runtime.
Shared groups choose the first fired actor as deterministic near-payload owner.

### Cached runtime plans

[`logical::Plan`](../crates/core/src/redpiler/backend/direct/instant/logical.rs)
binds separate plans for responses, outputs, and sampling guards. Binding checks
expression references, response order, allowed input kinds, and dependency
cycles, then keeps only reachable decisions and deduplicates input/threshold
bindings. Runtime source bindings cannot be ordinary display wire nodes.

Each plan holds a frozen Boolean input snapshot, cached decision values,
input users, and reverse parent dependencies. `capture` invalidates paths
affected by changed threshold results; it can keep an unaffected selected branch
cached. `evaluate` uses an explicit stack and evaluates only the selected paths.
Repeated roots share cached results. Unchanged sources, no due clock, and no
pending sample allow an initialized region to skip evaluation.

### Memory and event ordering

Compilation seeds stored bits from saved piston geometry and establishes event
baselines. Activation itself is not a write event. Changing prepared data alone
does not overwrite an independent memory cell.

A recognized active observer clock evaluates all responses against one old bank,
commits its memory cells together, and schedules its next sample six backend
half-tick advances later. Stopping the clock preserves stored bits and the last
data response. The six-step interval belongs to this recognized protocol.

Independent writes are grouped by writer. All delivered notifications from one
writer evaluate eligible targets against one frozen old bank, then commit them
together. Head-target eligibility uses the old extended state. Separate writers
observe the preceding writer's committed bank; they are not collapsed into one
tick-wide transaction. Remaining delivered groups stay pending for another
backend evaluation pass. Non-memory responses refresh after a bank commit.

## 6. Optimizer

Three distinct kinds of reduction exist: electrical graph passes, Boolean arena
reduction during extraction, and runtime decision caching. `--optimize` controls
the electrical passes and ordinary wire display elision; it does not disable
Boolean simplification or cached logical execution.

### Electrical graph passes

[`passes::run_passes`](../crates/core/src/redpiler/passes/mod.rs) uses this fixed order:

| Order | Pass | Enabled | Purpose |
| --- | --- | --- | --- |
| 1 | Identify nodes | Always | Read components and saved state; with optimization, omit ordinary wire display nodes |
| 2 | Input search | Always | Convert spatial electrical paths to main/side links with attenuation |
| 3 | Clamp weights | Always | Remove edges with attenuation at least 15 |
| 4 | Deduplicate links | `--optimize` | Keep strongest parallel source/target/channel paths |
| 5 | Constant fold | `--optimize` | Fold settled constant-fed diodes/torches while preserving saved output and pending-work constraints |
| 6 | Unreachable comparator output | `--optimize` | Prune links beyond the computed subtract-comparator output bound |
| 7 | Coalesce constants | `--optimize` | Share removable equal-strength constants within consumer components |
| 8 | Coalesce logic | `--optimize` | Merge eligible sibling nodes with equal type and saved state |
| 9 | Prune orphans | `--optimize --io-only` | Retain the dependency closure of protected interfaces and pending work |
| 10 | Export graph | `--export` | Serialize the supported ordinary electrical graph |

Only constant folding repeats until stable; the entire pipeline is not rerun to
a fixed point. Region boundary types, required source/consumer interfaces, user
inputs, outputs, and pending work protect nodes from ordinary removal. With
optimization, removed dust display nodes no longer maintain their world powers,
although physical dust still determines link attenuation during compilation.

Some rewrite guards have known limits. Comparator-output pruning uses the saved
constant strength without applying side-edge attenuation or accounting for a
larger saved comparator output. Logic coalescing does not compare sibling input
attenuation or separately require the sibling channel to match. These passes
are not universal equivalence proofs for arbitrary analog graphs. The
[optimizer reference](REDPILER_OPTIMIZER.md) records the actual guards and
counterexamples; backend validation does not repair an incorrect rewrite.

### Independent pure Boolean optimizer

[`mchprs_redpiler_opt`](../crates/redpiler_opt/src/lib.rs) defines `Circuit`, `Op`,
and a dense `Plan` for input, constant, buffer, NOT, AND, OR, XOR, and select
operations. It validates declared inputs, operands, roots, and cycles, builds a
topological reference, then applies one rewrite sweep, structural sharing,
root-based pruning, and dense layout. All declared roots are observable,
including state-write roots without a display output.

Budget exhaustion returns the validated reference with `used_fallback = true`;
cancellation returns an error. Evaluation uses caller-provided scratch and root
buffers without allocation. This crate has its own workspace and currently has
no production import in `mchprs_core`; it is not the implementation of the live
server's graph passes or cached decision plans.

## 7. Direct backend and execution order

[`direct::compile`](../crates/core/src/redpiler/backend/direct/compile.rs) maps
stable graph indices to dense `NodeId`s, initializes nodes from saved state,
binds world positions and region outputs, builds observer/dependency tables, and
transfers retained scheduled ticks with remaining half-tick deadlines and
priorities. Backend node IDs are local to that backend instance.

Each node channel has sixteen byte counters, one for each strength `0..=15`.
A source change subtracts link attenuation from old and new strengths, adjusts
the corresponding counters, and reevaluates the consumer. Strongest input is
the highest occupied strength bucket; Boolean input means any nonzero-strength
bucket is occupied. Runtime execution follows compiled links instead of walking
neighbor blocks.

Lowering validates strengths and limits incoming edges to 255 per channel.
[`ForwardLink`](../crates/core/src/redpiler/backend/direct/node.rs) packs a 27-bit
target ID, one side-channel bit, and four attenuation bits. Fixed node storage
and validated IDs support unchecked indexing on hot paths.

[`update.rs`](../crates/core/src/redpiler/backend/direct/update.rs) reevaluates
inputs and requests work; [`tick.rs`](../crates/core/src/redpiler/backend/direct/tick.rs)
applies scheduled transitions and propagates new outputs. This separation
preserves repeater locking/delay, comparator timing, observer pulses, and delayed
lamp turn-off. [`TickScheduler`](../crates/core/src/redpiler/backend/queue.rs) is a
32-slot half-tick ring with FIFO queues per priority; redstone-tick delays are
converted to twice as many slots. It is not an arbitrary-duration scheduler.

One compiled advance proceeds as follows:

1. Increment each region's elapsed half-tick and advance the scheduler slot.
2. Execute ordinary callbacks in priority/FIFO order. Process command outputs
   when a world is available, then evaluate affected regions after each callback.
3. After ordinary callbacks, evaluate due owned periodic clocks.
4. Publish changed aliases/output strengths through `set_node`, mark dependent
   regions dirty, and notify ordinary observers of committed geometry changes.
   Repeat region evaluation while dirty regions or delivered samples remain.

The dependency refresh loop coordinates prepared programs; it is not a general
solver for arbitrary cyclic physical circuits. Region preparation rejects
unrepresented combinational feedback. Ordinary observer watches use separate
notification tables rather than electrical input edges.

## 8. World output and interpreter handoff

Compiled state is authoritative while active. `tick_with_world` processes
command output events around callbacks and advances the world's logical tick.
Commands execute during simulation advances, including `/adv`; note/button
effects and visible dirty block states are emitted by `flush`. Notes beneath
owned BUD geometry capture canonical obstruction at their power rise, so
deferred rendering cannot change sound eligibility. Other notes retain the
existing live-world obstruction check.

`--io-only` restricts display writes to input/output nodes. It retains the
electrical dependencies they need. Supported BUD memory publishes its committed
base/head/payload pose at display flush, including after nonzero manual `/adv`.
A separate last-published bit avoids rewriting unchanged cells and does not
consume observer bookkeeping. `--optimize` permits this display; screen-only
suppresses its deltas while retaining current storage for re-enable and chunk
snapshots. Other logical region geometry remains virtual until reset; observer
notification reads committed settled state independently of presentation.

`Compiler::reset` removes the active backend, flushes surviving hidden node
state, exports comparator entities, materializes region geometry and stored bits,
and returns remaining ordinary scheduled ticks to the interpreter. Region
materialization in physically compatible mode restores current piston movement,
payload positions, observer power, and their remaining continuation work. Hidden
actors retain phase tracking so stopping compilation cannot strand a running
reset loop. Owned dust is recalculated without update callbacks. Assumption mode
instead writes settled logical occupancy with dormant reset observers; it does
not promise physical continuation of the ideal protocol.
`--update` explicitly runs interpreter updates over the bounds afterwards.

Edits that change compiler input geometry must end compiler ownership first.
Lever/button use and pressure-plate changes have compiled input paths; other
interaction rules live in the plot/player callers. The proposed mixed
native/compiled ownership design in
[partial compilation scope](notes/REDPILER_PARTIAL_COMPILATION.md) is a plan,
not an implemented fallback for rejected piston regions.

## 9. Commands, limits, and verification

`/rp` and `/redpiler` are aliases, implemented in
[plot/commands.rs](../crates/core/src/plot/commands.rs).

| Command or flag | Behavior |
| --- | --- |
| `/rp compile [flags]`, `/rp c` | Reset an existing compiler, then compile the plot |
| `/rp analyze [flags]` | Analyze and stage the compiler without transferring world ownership |
| `/rp analyze --graph [flags]` | Prepare a diagnostic candidate graph; never activate it |
| `/rp inspect`, `/rp i` | Log the compiled node under the player's ray trace |
| `/rp reset`, `/rp r` | Export compiled state and resume interpretation |
| `--assume-instant` | Relax construction admission while preserving logical/data/sampling validation |
| `--optimize`, `-o` | Enable optional electrical rewrites and omit ordinary wire displays |
| `--io-only`, `-i` | Limit display writes; combined with optimization, prune removable orphans |
| `--update`, `-u` | Run interpreter updates after reset |
| `--export`, `-e` | Write `redpiler_graph.bc` for supported ordinary graphs |
| `--export-dot` | Write `backend_graph.dot` for inspection |

Unknown flags fail. Short flags combine and are case-insensitive. Analysis
rejects export flags. Candidate preparation uses fresh analysis and the same
region preparation path, but does not lower or bind a backend; its success is
not a substitute for staged executable compilation.
Binary export rejects instant, command-block, and observer-notification graphs
that its format cannot represent. DOT is not a saved executable region program.

Automatic compilation runs only when enabled, the compiler is stopped, the plot
is unlimited or falling behind, no Git checkout is locked, and live chunks have
no pistons, piston heads, moving pistons, or observers. Admitted piston regions
therefore require explicit compilation.

Analysis defaults allow 16,777,216 inspected cells, 65,536 pistons, and 4,194,304
dependency steps. The Boolean arena defaults to 1,048,576 decisions; extraction
also bounds traversal, source count, and local owner variants. Executable
extraction permits at most eight conditional owners at one variant position.
Rank-provided resource multipliers are clamped to `1..=8`, never selected by
command flags. Cancellation and exhaustion return diagnostics instead of
activating a truncated program. Successful statistics include graph counts and
timings, wire elision, region counts, arena/plan decisions, outputs, and bindings.

For documentation changes, run:

```sh
python tools/validate_docs.py
```

For implementation changes, select relevant checks from the
[test guide](tests/README.md). The main compiler suite is:

```sh
cargo test -p mchprs_core --lib --locked redpiler::
```

Decision-arena and cached-plan tests exercise truth tables, dependency
cancellation, shared roots, invalidation, threshold handling, and frozen memory
snapshots. Analysis tests cover geometry admission, rejected input without world
mutation, electrical output strengths, observer ownership, independent write
ordering, clock banks, and reset/recompile continuity. Physical interpreter
protocols have [separate tests](tests/INSTANT_PISTONS.md); a matching logical
output is not evidence that intermediate physical motion or reset pulses match.
The isolated Boolean optimizer can be checked separately with:

```sh
cargo test --manifest-path crates/redpiler_opt/Cargo.toml --locked
```
