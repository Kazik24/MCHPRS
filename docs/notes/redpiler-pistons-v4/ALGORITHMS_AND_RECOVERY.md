# Retained execution and archived compiler algorithms

Status: 2026-10-18. The C/LLVM demo and the older finite/symbolic synthesis
experiments have been removed from source. Classic redstone, ordinary Graph
execution, O3 reset/piston specialization, instant-region composition and
private-cell proxies remain. Historical notes and benchmark reports describe
their original snapshots; they are not claims about code still enabled today.

## Recovery commits

The complete last committed source for **both compiler families** is
`576ae3137492dfa3dcacaf787ac6c9e482840d6e` (`C inference engine experiment`).
It also contains classic redstone and sparse contact projection. Do not revert
that whole commit to remove or restore a compiler: its changes are mixed.

| Snapshot | Commit | Purpose |
| --- | --- | --- |
| Complete native engine and latest retained synthesis | `576ae3137492dfa3dcacaf787ac6c9e482840d6e` | Primary recovery source, including C ABI, emitter, native verification and classic integration |
| Before the whole-engine C experiment | `34966d0f97d6a681feca373901c94a96cf5074d1` | Older synthesis plus the early native-query prototype |
| Synthesis disabled in default pipelines | `f8037ae3fef3e2a1946d08f45e82ff853a0bc844` | Records the disablement boundary |
| Boundary-driven compiler stop checkpoint | `ad1573f8b4071d7eb3f578519507c1d46d73fd47` | Original unfinished symbolic event compiler checkpoint |

Inspect a removed file without changing the current checkout:

```powershell
git show 576ae3137492dfa3dcacaf787ac6c9e482840d6e:crates/core/src/redpiler/backend/graph/native_engine.rs
git show 576ae3137492dfa3dcacaf787ac6c9e482840d6e:crates/core/src/redpiler/analysis/instant_regions/compiler/event.rs
```

For a complete, buildable historical checkout, use a separate directory:

```powershell
git worktree add --detach ../MCHPRS-compiler-archive 576ae3137492dfa3dcacaf787ac6c9e482840d6e
```

To reintroduce an experiment, port its source and integration together from
that checkout. Native execution needs `native_engine.{rs,c,h}`, its emitter,
`native_query.rs`, the standalone C selftest, Cargo feature/dependency wiring,
Compiler APIs, Graph input/tick/projection hooks, and native benchmark/tests.
Synthesis needs the `analysis/instant_regions/compiler/` tree, finite machine
and symbolic analyses, `crates/redpiler_opt`, three pass builders, program
types/lowering/validation, runtime/cursor/projection support and focused tests.
The historical commit contains the exact wiring. Restoring only the deleted
files would leave these integrations missing.

Re-run equivalence and performance checks after porting. Existing ignored
`target/` binaries, objects and JSON reports are local evidence, not a source
archive or a portable cache. The uncommitted unchecked ANPU experiment is
separate from the committed compiler archive and remains in the working tree.
Offline graph loaders and the viewer retain support for historical exported
machine/kernel records so archived artifacts can still be inspected.

## Current Graph algorithm

Preparation reads the selected World and imported work into a typed graph of
components, directional power dependencies, guarded contacts, payload resources,
wire neighborhoods and ordered callback routes. Rewrites simplify that graph;
read-only plan builders certify shortcuts. Validation precedes lowering into
immutable `CircuitProgram` tables and mutable `CircuitState`.

The Rust `GraphBackend` owns the clock, typed pending membership, scheduler,
piston request FIFO and deferred publications. Each tick drains due work before
and after advancing the scheduler, accepts piston actions in order, then publishes
deferred connections. Power and QC are sampled when their semantic operation
runs. Requests are revalidated when accepted; shared resources retain one owner.
General wires still use ordered Turbo walks with captured observations.

O3 plans replace only certified local work: wire query/transfer calculations,
reset mechanisms and private-cell proxy routes. A proxy retains ordered
Dispatch, Sample and ArmReset operations using the same state and queues.
`--stable-instant-inputs` still allows region composition; higher optimization
levels have no additional whole-region synthesis builders.

Flush presents graph state without advancing inference. Contact presentation
indexes selector dependencies on pistons/resources and writes affected contacts
in contact order; initial projection is complete, io-only flush preserves hidden
work, and duplicate positions retain the conservative full loop. Reset projects
current blocks, motion and ordered pending work for interpreter continuation.

## Retained classic-redstone algorithm

`--classic-redstone` enables `compile-classic-redstone` at O2 or above, after
rewrites and before the remaining plan builders. It remains off by default.

1. Discover whole potential electrical wire regions. Require fixed geometry,
   one variable ingress, representable complete receiver inputs, and delayed
   torch/unlocked-repeater receivers. Reject contacts, observers, independent
   receiver inputs, imported private-wire work and overlapping ownership.
2. Derive each owned wire's attenuation from the ingress and bind every external
   route entry by its actual delivery ordinal. The first admitted Live Update
   commits; the prefix must not synchronously reenter or propagate foreign wire
   work. Duplicate/stable entries and Shape notifications must be proved inert.
3. A cold ordinary Graph recorder checks all 256 old/new four-bit signal pairs
   for committing entries and all 16 stable strengths for inert entries. It
   verifies final wire strengths and ordered receiver samples. A delayed
   receiver's powered state cannot change in the synchronous episode; pending
   membership can only become present. Therefore the first sample of each
   polarity suffices for every live powered/pending combination.
4. Compact those samples into ordered `(receiver, nonzero, mask)` operations.
   Each 256-bit mask selects old/new pairs using `(old << 4) | new`; it does not
   store a complete execution transcript per pair. Independent validation
   replays the supplied program against fresh reference samples.
5. Lower to dense tables and one committed signal byte per net. At the bound
   route occurrence, read old/new signal, commit the new value and execute the
   selected operations through the existing scheduler, preserving priorities
   and pending deduplication. No private Turbo walk or intermediate dust write
   runs for that owned episode.
6. Receiver inputs combine attenuated committed signal with any proved direct
   live ingress contribution. Inspection, flush and reset reconstruct wire
   power from the committed signal minus attenuation. The source's newest
   output must not publish private wires before the route occurrence commits.

Bounds remain 32 wires per region, 2,000,000 reference-work units per attempted
net and 32,000,000 cumulative units. Unsupported candidates keep ordinary Graph
execution. The certified small fixture uses 584 program bytes plus one signal
byte and showed about 25% less inference time; BubbleSort and ANPU accept zero
certified nets. See [classic progress](CLASSIC_REDSTONE_PROGRESS.md) for proof,
coverage and measurements. Historical unchecked full-ANPU weighted variants
produced incorrect output. The ongoing test-only hybrid work is separate and
does not broaden production admission.

## Archived C/LLVM algorithm

The final native path prepared the ordinary graph, applied optional certified
classic lowering and existing O3 plans, then serialized immutable graph tables
to C. It additionally generated graph-specific power expressions and live-only
guard functions over typed component/piston/resource/committed-net buffers.
Unsupported expression roots used the general C evaluator. Captured callback
predicates required their real event context; they could not use the ordinary
power-query convention that a missing capture is false.

Clang compiled one generated translation unit at `-O3` for Windows x86_64;
lld linked a freestanding DLL. Rust checked ABI sizes, loaded symbols, allocated
state and attached an engine before activation. C owned scheduling, pending
membership, callbacks, captured wire walks, piston FIFO acceptance, resource
transitions, deferred publication and presentation effects. Inference had no
Rust Graph fallback. Rust copied changed state for World presentation and
exported the complete future at handoff.

Sparse function-pointer dispatch avoided the compile explosion of one giant
query switch. Indexed capture misses returned absence directly; mapless small
captures retained linear lookup. A process-local hash shared immutable code
for identical source/runtime/header/toolchain paths, with independent mutable
engine state. The benchmark's separate verified-object helper was measurement
tooling, not a persistent product cache or a prerecorded simulation.

Verification compared specialized queries/guards with general C, and complete
state, ordered work and effects with Rust after inputs/ticks. Frozen interpreter
RAM, reset and continuation supplied additional checks. The final full-sort pair
was 71.80 s Rust versus 57.32 s native including projection; repeated short pairs
showed 16.9% less combined time. Cold setup was about 502 s; generated C was
451,136,648 bytes and the DLL 118,535,168 bytes. Graph initializers account for
77.6% of source bytes. Callback and walk counts stayed unchanged. No server TPS
or scalable semantic-kernel synthesis was established. Full history:
[LLVM_NATIVE_INFERENCE_DEMO.md](LLVM_NATIVE_INFERENCE_DEMO.md).

## Archived finite and symbolic algorithms

The finite prototype enumerated at most three data controls after a graph-derived
trigger, proved a common entry relation, and executed ordered Graph trajectories
until normalized complete state repeated. Normalization removed time/revision
origins while retaining state and ordered future obligations. A compact shared
expression/phase program replaced the trajectories; specialization produced
unconditional effect lists for each held-input assignment. Private state and
future work were reconstructed for display, input changes and handoff. This
bounded approach did not scale to ADDER11BIT. See
[CLOSED_INSTANT_MACHINE.md](CLOSED_INSTANT_MACHINE.md) and
[COMPACT_INSTANT_KERNEL.md](COMPACT_INSTANT_KERNEL.md).

The symbolic compiler instead bound reads to expressions, updated state with
guarded SSA writes and expanded ordered callbacks, captures, FIFO deduplication,
acceptance and scheduled phases during compilation. Shared Boolean/strength
expressions used canonical BDDs or structural ITE DAGs with bounded local proofs.
Backward liveness was intended to retain only boundary outputs, logical memory,
ordered effects, future work and cold projection roots. The regional variant
compiled callback/sample/accept/reset/publication entries against the existing
global clock rather than requiring a whole-selection periodic cursor.

Earlier grouped actor/reset routines retained ordinary propagation. The later
whole-event implementation remained unfinished: the first ADDER1BIT event hit
the 1,048,576-node limit without producing a plan, runtime event dispatch was
unconnected, and callback-effect handling was a placeholder. Stable inputs did
not permit unordered internal piston work. Reintroduction must complete event-time
views, captures, boundary delivery, reentrancy and ownership proof before enabling
runtime dispatch. See [BOUNDARY_DRIVEN_INSTANT_COMPILER.md](BOUNDARY_DRIVEN_INSTANT_COMPILER.md),
[SYMBOLIC_VIRTUAL_CIRCUIT.md](SYMBOLIC_VIRTUAL_CIRCUIT.md), and the preserved
[edge-case register](INSTANT_COMPILER_EDGE_CASES.md).

## Removal validation

`cargo check --workspace --all-targets --all-features --locked --offline` passed.
The Redpiler library filter passed 349 tests with 12 ignored after excluding
`unchecked_hybrid_preserves_whole_contact_region_but_keeps_remote_fixed_line`.
That concurrently edited, uncommitted ANPU experiment's fixture fails during
construction with `MissingComponentSite { node: NodeId(5) }`; it was left intact.
The initial broad run also exposed an old feedback-test assumption that every
pass is enabled by `--optimize`. Its expectation now correctly keeps the
explicitly opt-in classic pass disabled by default.

Python loader tests passed 15/15 and viewer tests 23/23. The existing O4 export
regression now requires removed synthesis fields to be absent. The pipeline
regression checks that removed builders are absent across levels and stable-input
settings. The retained BubbleSort benchmark passes its focused rustfmt check;
performance was not remeasured. `git diff --check` passed, and all 59 deleted
tracked files were verified present as blobs in the primary recovery commit.

Local validation logs: `target/compilation-removal-final-check.log`,
`target/compilation-removal-tests.log` (initial failures) and
`target/compilation-removal-final-tests.log` (final run). The final test command:

```powershell
cargo test -p mchprs_core --lib redpiler:: --locked --offline -- --test-threads=1 --skip unchecked_hybrid_preserves_whole_contact_region_but_keeps_remote_fixed_line
```
