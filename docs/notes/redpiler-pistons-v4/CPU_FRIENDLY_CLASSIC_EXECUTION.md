# CPU-friendly classic execution: measured baseline and proposed next step

Started 2026-10-18; status updated 2026-10-19. Packing and the second whole-fixed execution stage are implemented at O2+. Mixed circuits now have the fixed-face power-query slice described below; broader mixed lowering, passive-net contraction and SIMD remain proposed work. See [the current state](CURRENT_STATE.md) for measured throughput and remaining scope. This work keeps the existing Graph backend and its single authoritative machine state and future.

## Recommendation

Build a compact typed execution plan, then replace eligible passive wiring with direct signal transfers. Keep an exact ordered program for the parts whose intermediate receiver samples matter. Use bitsets to find active work and contiguous arrays to execute it. Do not sweep every actor every tick or turn the entire machine into a synchronous Boolean circuit.

The first implementation should change storage and dispatch without changing operation order. The larger algorithmic gain comes afterward, from eliminating interior propagation work. SIMD is useful once the work is represented as independent arithmetic or Boolean operations; it cannot repair a representation that still revisits millions of wire sites.

## What has actually been checked

The bounded all-device harness discovered 82 entries. At both O2 and O3, 56 entries matched the independent interpreter through their active output traces, chat, reset state, exact exported future, and interpreter continuation. All 22 lever-equipped IO fixtures participated. The other entries were classified rather than counted as passes:

| Result per tier | Count | Meaning |
| --- | ---: | --- |
| Matched | 56 | Paired execution plus first-tick reset/recompile and continuation checks passed |
| Missing input protocol | 21 | The generic harness only drives saved levers; these entries need their declared raw-input/setup protocol |
| Admission declined | 5 | Two raw adders have potentially unsupported signs; raw PM1 reaches unloaded chunk 16,2; saved PM1 has missing piston motion metadata; raw TEST_XOR can lose dust support |
| Output/future mismatch among executed entries | 0 | This establishes the tested histories, not arbitrary circuit equivalence |

The existing normalized TEST-family checks also passed: all 18 fixtures' independent control histories, the 13-family active protocols, horizontal rotations, and a support-loss rejection test (four test functions). These deliberately supply the controls/floor supports recorded by earlier fixture research. They use the general/default compiler configuration, so they supplement the O2/O3 sweep rather than establish O3 execution for every normalized fixture. The adder/mechanism module passed 55 tests, with one research test ignored and the independently changing instant-layer module excluded from this focused run. This includes physical one-bit combinations, eleven-bit vectors, FIFO counterexamples, translated/rotated adders, and reset/continuation cuts.

Full BubbleSort passed 10,036 ticks at both O2 and O3, checking the frozen native RAM trace and sorted result, unchanged other RAM banks, reset and interpreter continuation. Its existing loader removes the documented unsupported wall sign at world (188,10,186); the source schematic is unchanged. A fresh default-O2 CHUNGUS run matched the general control and independent interpreter at 1,025 boundaries, including analog outputs and the exact future. The full 12,411,975-tick CHUNGUS application has not been run.

Evidence is in `target/cpu-friendly-validation-{o2,o3}.json`, `target/cpu-friendly-validation-summary.json`, `target/cpu-friendly-validation-test-family.log`, `target/cpu-friendly-validation-adders.log`, `target/cpu-friendly-validation-bubblesort-{o2,o3}.json`, and `target/cpu-friendly-validation-chungus-1024.json`. These new runs used frozen **dev/debug** executables and overlapped other work: their timings are not performance evidence. The original all-device reports incorrectly label that build bench/release; the summary corrects this and retains the raw reports. The harness now reports the build distinction correctly.

The fresh current-source Redpiler suite passed **450 tests, zero failures, with 19 research tests ignored** (`target/cpu-friendly-validation-current-suite.log`). The earlier four experimental instant-layer failures were resolved by concurrent work before this fresh build; this result supersedes that earlier failure count. Missing fixture protocols/admission declines and the unrun long CHUNGUS application remain explicit coverage limits.

## Why current classic execution is still expensive

Whole fixed compilation partially evaluates the general graph into typed queries, routes, receiver updates and dust traversal descriptors. It removes spatial searches, dynamic guard selection and generic Turbo dispatch. It intentionally retains the native order, captured properties, intermediate writes and delayed scheduling. CHUNGUS has 100% whole fixed executable coverage; that does not mean its passive network has been contracted.

The previous quiet optimized 4,096-tick comparison measured 5.7010647 seconds for general O2 versus 4.5085177 for whole fixed execution: 1.2645x, or about 20.9% less core time. Both performed 380,926 scheduled component ticks and counted 20,076,058 callbacks. Fixed execution still performed 210,566 ordered episodes and 15,105,341 wire visits. This is instrumented test execution, not server throughput. The general scheduler is already a 32-half-tick ring of priority FIFO queues; replacing a heap is not an available win.

The fixed executable contains 100,100,492 bytes of tables, plus the original graph retained for inspection/projection. It has 2,517,665 retained route operations, 421,644 signal operands, and 1,032,940 capture-site aliases for 251,485 actors and 133,623 wires. The previous explicit route DCE did not reduce this payload: the retained rows are live output/shape programs. Further elimination requires a new argument, not another unreachable-route filter.

Compiler layout diagnostics on the current Windows x64 build give these actual sizes:

| Record | Bytes | Implication |
| --- | ---: | --- |
| ComponentState / NodeState | 104 | Even wire states have this array stride |
| CommandBlockEntity | 96 | Its inline optional storage dominates NodeState |
| RedstoneWire | 5 | Power and sides need a small fraction of the current stride |
| FixedClassicWire | 200 | 24 neighbor IDs and six 12-byte NoDust queries are embedded |
| FixedClassicNode | 72 | Every kind carries three queries and optional comparator data |
| FixedClassicQuery | 12 | Constants and common small arities can be represented more cheaply |
| FixedClassicOperand | 16 | Tagged source plus attenuation/cross requirement |
| FixedClassicSite | 12 | Includes many inert aliases |
| FixedClassicRouteOp | 12 | 2.5 million rows account for about 30.2 MB |
| ScheduledNodeTick / PendingNodeTick | 36 | Common fixed-node entries retain generic identity metadata |

For CHUNGUS, the component array alone is about 26.2 MB decimal before auxiliary state. These are compiler sizes, not cache-miss measurements. Diagnostic command: `RUSTC_BOOTSTRAP=1 cargo rustc -p mchprs_core --lib -- -Zprint-type-sizes`; log `target/cpu-friendly-type-layout.log`. This was a diagnostic build only, with no production compiler flag change.

Another source-level candidate: wire evaluation and propagation repeatedly identify a 24-neighbor set; identification captures the neighbors before checking cached heading. Avoiding repeated descriptor/slot lookups may help. Eager first capture, shared-site layer bookkeeping, and nested walk isolation must remain intact. No sampling profiler has established its cycle share yet.

## Pass contract and placement

Provisional name: `pack-classic-execution`, a PlanBuilder after the applicable semantic rewrites and prerequisite plan builders. Register it in its own pass file. At O2 it consumes whole fixed plans; at higher tiers it runs after the final relevant instant ownership plans. Follow `crates/core/src/redpiler/passes/README.md`.

**Input:** validated CompileGraph, prepared signal queries and ordered callback routes, fixed/classic and accepted instant ownership plans, and the read-only entry-state/time/imported-work contract. Geometry is already resolved. A rejected instant region must remain ordinary Graph execution; it is not an unchecked input to this pass.

**Output:** an immutable `PackedClassicProgram` published through ExecutionPlans and lowered into CircuitProgram, plus the normal initial CircuitState and imported future. The plan contains executable tables, original-ID mappings, ownership, projection recipes, and precise decline reasons. JSON/viewer export describes these same tables and ownership, not a separately reconstructed representation. The runtime executes the attached plan within GraphBackend; it is not a new backend.

Validate ID/range bounds, source interpretation, constants, operation ordinals, entry obligations, and ownership before lowering. Structural/rederivation checks detect stale or forged plans; they must not be described as an independent semantic proof. Packing needs a one-to-one instruction/state refinement argument. Contraction additionally needs an electrical and event-order proof. Unsupported candidates are ordinary declines.

## Representation

### One mutable state owner

First remove cold command/entity payload from every ordinary signal row. Either a cold sparse entity store with compact per-component references or per-kind state arrays can achieve this; choose the smaller patch after measuring layout. Do not allocate one box for every ordinary node. Keep the live entity fields in graph-owned state for command blocks that actually exist.

Then introduce compact authoritative signal storage where useful: byte output strengths, packed powered/locked flags, compact pending metadata, and wire side state only where it can change. CircuitState accessors resolve original NodeId to this storage. General execution, specialized execution, inspection, flush and reset all access that same value. Do not keep a stale general array plus a second fast-state cache. Migration must either remove the old owned value or explicitly transfer ownership.

Use u8 analog values first. Four-bit packing halves an already small signal array but introduces masking and read/modify/write costs. Boolean bitplanes are attractive for wide independent cones, not mandatory for every scalar update.

### Immutable instruction tables

Use dense u32 signal/operation IDs and CSR ranges: `offsets[owner]..offsets[owner+1]` selects contiguous inputs or fanout. Keep Main, Side and Quasi channels distinct. Pool common queries and constant values. Start with specialized constant/unary/binary/general reductions; retain a scalar general opcode for uncommon arity. Comparator overrides and wire geometry belong in separate kind-specific tables.

Precompile mutable neighbor references and inert sentinels. Compress aliases only when their captured identity, layer and heading behavior is equivalent; two sites referring to one actor are not automatically interchangeable. Keep exact route instructions contiguous in original ordinal order. Structural sharing of identical route/query bodies is legal only when their source/capture bindings are also identical.

The program tables are immutable Rust data shared by Arc. Ordinary simulation state is not volatile. No generated C expansion, external compiler or huge DLL is required for this representation.

### Active work, not a full scan

Retain the delay ring and priority/FIFO semantics initially. A later common-path representation can store dense node IDs in contiguous FIFO buffers and keep exceptional imported/untyped/stale identities in cold records. Exact FIFO, deadlines, duplicate multiplicity and pending membership must survive import, execution and reset; a single pending bit cannot represent every existing input state.

Use dirty bitsets and an active-word/chunk list for proven batchable operations. Iterate set bits with trailing-zero extraction, then evaluate contiguous operands. Deduplication is allowed only where repeated work is proven redundant. Bitsets used for membership must not sort or replace an order-sensitive receiver FIFO. At the measured prefix CHUNGUS averages about 93 scheduled component ticks per game tick; scanning 251,485 actors unconditionally would add substantial work.

Generation-stamped capture slots are an experiment, not an assumed win: they trade a sparse clearing loop for extra stamp reads/storage. Measure both, handle epoch wrap with a real reset, and give nested propagation episodes independent scratch lifetimes.

## Next-state algorithm

The transition remains an ordered composition of operations, with batches inside proven independent regions:

1. Advance the shared logical clock and obtain the original due priority/FIFO stream.
2. Execute each due stateful boundary operation with its original captured/live reads and pending rules.
3. When an output changes, execute either a certified direct transfer program or the packed exact propagation program immediately at the same causal point.
4. Direct transfers update delivered signal state, mark/evaluate proven receivers, and schedule the same delayed obligations. A sensitive receiver group uses its preserved ordered program rather than a bulk unordered commit.
5. Repeat through the current tick's nano-tick/priority rules. The resulting state and future are the next full-tick machine state. Projection is outside the hot loop and materializes requested observables/reset state.

This is not universally `next = f(all old outputs)`: torch/repeater/comparator deadlines and within-tick propagation can expose a newer signal to a later receiver. A synchronous two-buffer evaluator is valid only for a region with a proved synchronous boundary.

### Contracting passive networks

For a fixed passive net, the settled electrical strength at an exit has the form `max(constant, max_i saturating_sub(delivered_i, attenuation_i))`, with analog channel distinctions and any required geometric qualification retained. Precompute weighted source-to-exit dependencies from the prepared graph. Multiple ingress sources must be supported algebraically; enumerating all 16^N source vectors is not a scalable proof strategy. Bound construction/output size and preserve the exact program for dense cases where an expanded source-exit matrix would be larger than the network.

This formula establishes a settled value only. A separate boundary proof must establish which receiver samples happen, their order, captured/live interpretation, pending checks, and the resulting deadlines/priorities/insertion order. Store **last delivered** ingress values; reading the source's newest output can publish a change before its delivery. An unchanged final strength can still cause a meaningful callback. Pistons, observers, BUD behavior and dynamic contacts require explicit notification boundaries, even when power is unchanged.

The CHUNGUS tick-512 counterexample is mandatory: comparator (100,80,212) has a main leaf at (101,81,212). The unordered weighted experiment sampled 9 where the exact execution sampled 13, and created a different pending tick, although both leaves ended at 13. There are no pistons in this counterexample. Removing the queue or choosing node-ID order is therefore not justified by full-tick-only observation. Preserve the saved ignored diagnostic and the adder FIFO regression.

Classify each candidate as a direct transfer with certified commutative receivers, a direct transfer with an ordered receiver subprogram, or packed exact propagation. The last class preserves whole fixed coverage while proofs improve; it must not silently fall back to the slow spatial interpreter.

Interior dust can disappear from hot state only if its values are reconstructible at every supported observation/handoff cut. Entry-state stale/unsettled values and work not yet delivered matter: do not settle the entire net at activation. Reconstruction needs a validated initial-state recipe and update rule, including imported work. Reset must reproduce blocks, comparator strengths and exact future so native execution can continue.

## SIMD and batching

Begin with scalar packed arithmetic and let the compiler vectorize contiguous independent loops. Saturating u8 subtraction and max reductions fit analog transfers; u64 AND/OR/XOR/NOT bitplanes fit Boolean cones. Group by kind/arity/locality only where operations commute or compute from an explicitly frozen input snapshot with a valid ordered commit.

Only add architecture-specific SIMD after the scalar representation produces a measured bottleneck and gain. A shipped AVX path needs feature detection and a scalar path. Do not issue one SIMD lane per successive tick of a feedback CPU: later ticks depend on earlier state. SIMD across independent cones or separate machine instances is a different, valid opportunity.

An `advance(n)` API may avoid per-tick call/projection overhead and fast-forward quiescent gaps up to the next deadline/input/required observation. It cannot skip active clock cycles without a separate transition summary proof. Millions of ticks processed in a batch and millions of ticks per second are different targets. The measured CHUNGUS prefix is about 909 game ticks/s; layout changes alone do not establish a thousandfold improvement. Report active game ticks/s and useful node/transfer operations/s separately, and never use an idle circuit to claim active CPU throughput.

## Milestones and acceptance

1. **Baseline and profile:** retain the current fixtures/failure categories; profile an optimized active CHUNGUS prefix and BubbleSort separately. Break down signal reads, wire work, route dispatch, scheduler and projection. The recorded counters/layout justify hypotheses, not percentages of CPU time.
2. **Pack without semantic changes:** cold entity split, compact kind/query tables, contiguous ordered operations and cheaper neighbor references. Preserve every existing operation/sample/order. Integrate validator, lowering, execution, reset/inspection, statistics and JSON together. Measure total retained bytes as well as hot working-set bytes. Avoid making a compact third copy of two existing programs.
3. **Remove passive work:** certify multi-ingress nets and replace internal propagation with weighted direct transfers plus appropriate receiver programs. Census candidate/owned wires and precise refusal reasons on CHUNGUS and ANPU before choosing proof budgets. Report eliminated dynamic wire visits, not only static wire coverage.
4. **Batch proven independent work:** dirty chunks, Boolean bitplanes and analog SIMD where measured useful. Retain ordered islands and protected instant/BUD boundaries. Optimize scheduler metadata only if its measured share warrants the change.

Each milestone must pass the existing native/reference differential checks, analog/multi-source/locking tests, pending import/duplicates/untyped work, tick-512 and adder FIFO cases, mixed ownership, reset at pending cuts, and interpreter continuation. Accepted instant plans retain all lifecycle obligations. Missing fixture protocols and admission declines remain explicit limits; they cannot be waived by a throughput result.

For performance use a quiet optimized executable, alternate paired samples, report medians/spread, and keep setup, compile, observation and flush separate from core execution. Include an active long-window CPU run and the actual application result before claiming long-run speed. A million-tick target should use a real active protocol; use a smaller measured development window until it is affordable. Report compilation cost, executable/table bytes, memory overhead, operations eliminated and fallback ownership alongside time. Retain the old exact executor as a test control, with no hidden user-facing optimization switch for each internal experiment.

## 2026-10-19 implementation update: first packed execution stage

The original proposal above now has an implemented first stage. Cold command entities are boxed, reducing shared electrical/wire rows from 104 to 16 bytes. Fixed propagation captures its neighborhood once and executes FIFO frontiers using local u32 capture indices. CircuitState and GraphFuture remain authoritative; plan tables, ownership, callback order and graph exports are unchanged. This is runtime storage refinement within the existing compile-fixed-classic program, without another semantic pass or backend.

452 Redpiler tests, 1,025 whole-CHUNGUS native boundaries, and all 18 measured production endpoint/future/continuation checks passed. Three fresh 16,384-tick CHUNGUS samples improved from 1,187.0 to 1,755.6 game TPS against the frozen control: 1.479x throughput and 32.39% less inference time. ANPU has no demonstrated gain: combined six-sample medians are effectively equal amid substantial external-load variation. Its general propagation path remains the next target.

The component payload shrinks about 84.6%; total retained graph memory does not shrink by that proportion. Whole fixed tables remain about 100.1 MB and captured-neighborhood caching adds temporary storage. Passive-net contraction, mixed boundaries and SIMD remain proposed work. Detailed implementation, equivalence argument, sample ranges, external-load observations and artifact hashes are appended in CLASSIC_REDSTONE_PROGRESS.md; structured results are in target/packed-classic-v1-summary.json.

## 2026-10-19 implementation update: second execution stage

The whole-fixed executor now uses contiguous frontier buffers, borrowed wire rows, one joined block-power query, precompiled mutable-neighbor masks and eight-byte alias descriptors. Empty/constant-fifteen queries return directly; unchanged seeds avoid an Arc retain/drop. Capture and FIFO semantics remain exact. Entry-state shape invariants also remove unreachable shape-route bodies, with explicit attachment validation and retained handling for stale shapes. Original per-face queries remain inspectable.

Five alternating production release pairs over 16,384 active ticks improve the median from 2,139.18 to 3,283.65 game ticks/s: **1.535x throughput, 34.85% less inference wall time** against the preceding packed executor. Compilation is essentially unchanged at about 25.4 seconds. Every pair has identical executed work and 100% typed coverage; all ten native endpoint/future and 32-tick continuation checks passed. A separate comparison checks every native/general boundary 0..1024, including tick 512. No competing build/test process was detected during timing.

Executable table payload falls from 100,100,492 to 73,588,960 bytes, and route operations from 2,517,665 to 474,520. This is retained executable storage, not total process memory. The original Graph, intermediate wire writes, ordered propagation and shared scheduler remain. SIMD, private net contraction and mixed-piston compilation are still separate work.

The pass named `experimental-coalesce-delayed-updates` was subsequently promoted into default O5, bare `-O` and `--optimize`; O0-O4 retain their prior selection. It removes equivalent read/schedule-only notifications inside protected runs, with pending/FIFO and direction/lock proofs. CHUNGUS and ANPU match complete state/future through 1,024 ticks plus reset/native continuation. The rule does not replace a wire walk by one piston sample and has no isolated production timing result. See CLASSIC_REDSTONE_PROGRESS.md and target/chungus-stage2-summary.json for detailed counts, sample ranges, hashes and limitations.

## 2026-10-19 stage 3 trial: active coverage and a rejected runtime experiment

An O5 census ran 16,384 CHUNGUS ticks through the existing exact executor. It records
seed calls/changes and frontier visits/changes per wire, then attributes them to the
existing electrical regions. The optional counters exist only in test builds.

| Measurement | Count | Share of all frontier visits |
| --- | ---: | ---: |
| All frontier visits | 57,645,484 | 100% |
| Visits in small 1–4-source binary electrical candidates with invariant wire shapes | 30,150,407 | 52.30% |
| Same family, without admitted callbacks to outside wires | 427,231 | 0.74% |

These are opportunity counts, not admitted transfer programs or CPU-time shares.
The closed-net family is too narrow to justify expecting another 1.5x gain. The
remaining candidates need an explicit interface for outside wire callbacks and
shared capture/layer state. A binary-source truth table alone cannot establish it.

A separate conservative runtime experiment reused an unchanged wire evaluation
until any captured wire write advanced an episode-local epoch. Reuse required a
constant no-dust query; all queue entries, captures, intermediate writes and receiver
callbacks remained in their original order. Epoch wrap invalidated all entries.
The experiment passed the focused analog/pending tests and every CHUNGUS boundary
0..1024 against the general Graph and independent interpreter, including tick 512.
It reused only 103,970 of 4,422,713 counted wire evaluations: **2.35%**.

Six production O5 samples of 4,096 ticks each passed native endpoint state/future
and 32-tick interpreter continuation. Four samples overlapped other Cargo/rustc
jobs. The two uncontaminated samples belonged to different pairs, leaving **zero
complete clean pairs**. No speedup is established. The limited-reach runtime
experiment was removed; only the test-only census remains. No five-pair promotion
run or longer-window performance claim is made for this rejected candidate.

Evidence: `target/chungus-stage3-summary.json`,
`target/chungus-stage3-active-net-census-control.json`,
`target/chungus-stage3-native-1024.{json,log}` and
`target/chungus-stage3-smoke-pair-{1..3}-{control,candidate}.{json,log}`.
Frozen executables, source diff, prototype tests and process observations are
retained beside them. These ignored artifacts may be absent in another checkout.

## ANPU next step: recognize BUD memory as a separate stateful contract

The existing ANPU profile points first to general wire work, capture and power/site
queries, rather than the scheduler. Whole-fixed execution declines on its mixed
piston/contact circuit. The older local-net optimization eliminated only eight of
183,751 walks; broad fixed-query/hop execution inside mixed circuits remains useful.

The user's proposed BUD-memory pass is a distinct possible route to eliminating
private feedback and update wiring. It must recognize retained state and qualifying
events, rather than admit ordinary memory as a stationary instant-computation cell.
Existing tests intentionally keep those ownership contracts separate.

For ANPU, use two implementation stages. First extend typed ordered execution to
eligible fixed wiring inside a mixed piston/contact circuit: compile static queries,
neighbor descriptors and dispatch while retaining dynamic guards and the original
Graph piston/contact operations at their exact positions. The current whole-fixed
builder declines this entire circuit, so CHUNGUS's two execution-stage gains do not
establish an ANPU gain. This stage changes execution cost without requiring a proof
that a complete memory mechanism can disappear. Then add `compile-bud-memory` to
remove certified private wiring around complete storage/read pairs. Measure each
stage separately; neither has a demonstrated ANPU speedup yet.

The user's cell description has **two pistons: upper storage and a separate read
output**. Recognition must prove their relationship and include both roles; proximity
alone is insufficient. Keep write-data/write-update inputs distinct from read-enable
and read-output publication. A read must preserve storage unless the physical
mechanism proves a destructive-read/restore transition. One logical stored bit does
not replace the read actor's pending motion or observable output state. Shared read
or write distribution remains a boundary unless explicitly covered by the proof.

A fresh O5 ANPU census executes the existing 5,000-tick Pong window and attributes
all 327,585 piston power samples and 10,682 accepted actions to individual actors:

| Structural input signature | Actors | Power samples | Accepted actions |
| --- | ---: | ---: | ---: |
| QC present, Main structurally zero | 960 | 15,998 | 1,538 |
| Both Main and QC present | 1,916 | 292,460 | 7,762 |
| Main present, QC absent | 21 | 18,914 | 1,382 |
| Both absent | 1 | 213 | 0 |

No actor in this census has the existing observer-reset provenance. **None of these
signatures proves BUD memory.** The QC-only group covers 4.88% of samples, while
the Main-plus-QC group covers 89.28%. A recognizer restricted to permanently zero
Main misses most active piston sampling. Main may carry read control or an update
pulse while QC carries retained data; the graph and finite-state behavior must establish the
actual roles. The report inventories explicit update route/ordinal/guard/view rows,
but contact alternatives and captured Turbo activation still require closure.

More directly, the existing frozen `test_data/cpu-references/anpu_bud_updates.json`
labels **896 lower read actors**. Joining those positions to the active Graph
census accounts for **245,453 samples (74.93% of piston samples)** and **2,768
accepted actions (25.91%)**. All 896 have both Main and QC inputs. The reference's
geometric fixture selector is a downward sticky piston below a note block; this
is an existing fixture label and oracle, not a general memory-recognition proof.
The expanded census links each lower actor's QC source contact to exactly one upper
sticky piston's far contact and verifies its movable payload is a redstone block.
All 896 pairs are disjoint; each upper base is four blocks above its lower base.
The upper actor moves the stored power source, and the lower actor reads it through
QC. This establishes the fixture's structural pairing, not an executable replacement
certificate. Both actors and their notification channels still need transition proof.

| Cell role | Actors | Power samples | Accepted actions |
| --- | ---: | ---: | ---: |
| Upper storage | 896 | 15,227 | 1,421 |
| Lower read output (frozen labels) | 896 | 245,453 | 2,768 |
| Paired total | 1,792 | 260,680 | 4,189 |

Pairs account for **79.58% of piston samples and 39.22% of accepted actions**.
Most sample activity is in the read side, so a storage-only shortcut would target
only 4.65% of piston samples. Measure the read-enable/QC/output wire cone before
choosing which private work to contract. These are event counts, not total inference
time shares. The 50,000-tick frozen BUD replay was inspected rather than rerun.

The retained ANPU runtime diagnostic estimates piston sampling at about 8.9% of
instrumented tick time and general wire walks at about 63.4%. These are coarse,
inclusive call-timer estimates from an older source snapshot, not additive current
CPU shares. They nevertheless argue against expecting 1.5x throughput from reducing
memory-piston sampling alone. The larger opportunity is using proven memory
boundaries to remove private feedback propagation and simplify its data/output reads.

First implementation contract for a separate `compile-bud-memory` builder:

1. **Identify the pair, state and encoding.** Start with a bounded private mechanism
   and prove storage/read roles, their dependency, stable states, payload ownership
   and observable output. Inventory both actors' activity and attributable wire work. Reuse
   CircuitState's actor/resource values; introduce no independently mutable cached
   memory bit. Retain pending/transient phases in GraphFuture. Shared payloads or
   observable interior geometry require an explicit larger contract or decline.
2. **Derive write, read and activation separately.** Record Main/QC power expressions,
   polarity and guards independently of every qualifying base/head update. Include
   captured/live reads, contact alternatives, direction when relevant, multiplicity
   and original route ordinals. QC availability alone is not an activation event;
   a notification with unchanged final power can still be meaningful. Derive the
   read-enable and stored-state-to-output path independently of the write transition.
3. **Compile the real transition sequence.** A qualifying notification samples power
   and may enqueue the original request. Acceptance rechecks live power, pose,
   availability and resource conditions at the original FIFO position. Only an
   accepted action changes stored state; publication retains the original deadlines,
   contacts and outgoing notifications. Do not replace this with immediate
   `Q = data` or one toggle per update. Derive the mechanism's encoding and transition
   table, rather than assuming a generic D latch.
4. **Attach one executable owner.** Publish the immutable memory plan through
   ExecutionPlans; integrate validation, lowering, export, inspection and reset.
   Keep it disjoint from instant/proxy/local-net ownership and register its query
   and ownership prerequisites explicitly. Unsupported entry states retain ordinary
   Graph execution before any optimized side effect. Benchmark compilation cost too.
5. **Eliminate proven private work.** Begin with exact precompiled data/update reads,
   then remove only the internal feedback/wire/callback work covered by the
   certificate. Count eliminated dynamic wire visits and query/guard work. Replacing
   piston bookkeeping alone does not address ANPU's dominant propagation cost.
6. **Validate before promoting.** Use the independent-data/update fixture first,
   then Rilax memory, the counter and ANPU. Exercise data-only changes, repeated
   updates, both pulse edges, read-only pulses, disabled reads, overlapping reads/writes,
   data changing between request and commit, shared
   update clocks, pending/imported work, resource conflicts and reset/continuation
   at intermediate phases. Compare ANPU's real output protocol and exact future,
   then run quiet alternating production samples over its active window.

The starting fixture is `bud_instantmemorycellobserverupdate`: its existing regression
holds memory unchanged for 32 ticks after a data-only change, then applies the
independent update and checks the sampled state and interpreter handoff. Its stored
logical one is a retracted piston, so even its polarity must not be guessed.
See `regional_bud_memory_changes_only_after_the_independent_update` and
`bud_memory_is_outside_cells_while_its_power_and_update_channels_remain_distinct`.

ANPU evidence is `target/anpu-memory-cell-opportunity.{json,log}`. This is an active
opportunity census with no memory replacement and no speed measurement. No BUD-memory
pass, broad BUD recognition percentage or ANPU throughput gain is claimed yet.

### Verification of the retained census changes

The final frozen executable passes **497 Redpiler tests, zero failures, 33 ignored**
in 149.27 seconds. This includes the existing independent-data/update, BUD boundary,
Rilax storage and counter regressions. The ANPU labeled census was explicitly run
despite its ignored-by-default status; its per-actor totals match Graph's counters
and all 896 frozen memory positions map to actors. Workspace checking passes with
the two existing fixed-classic test-adapter dead-code warnings. Owned Rust formatting
and whitespace checks pass. Logs: `target/classic-stage3-final-suite.log`,
`target/classic-stage3-final-check.log`, and
`target/anpu-memory-cell-opportunity-labeled.log`.

After the two-piston clarification, the expanded census was rebuilt and explicitly
rerun: one pass in 15.21 seconds. Assertions verify all 896 one-to-one QC/contact
pairings, upper redstone-block payloads, four-block vertical offsets, disjoint roles
and unchanged global activity totals. Evidence: `target/anpu-memory-pair-census.log`
and the expanded `target/anpu-memory-cell-opportunity.json`. This follow-up changes
only research inventory and documentation; the full suite was not rerun for it.

A broader `bud_` test filter also exposed an existing stale assertion outside the
Redpiler suite: `redstone::piston::tests::bud_reference::anpu_cannot_bypass_compiled_graph_admission`
expects ANPU compilation to fail, but the current graph admits it. That run had ten
passes, one failure and two ignored tests (`target/bud-memory-existing-contracts.log`).
No admission rule or that test was changed in this work. The earlier three transient
instant-layer suite failures were resolved by concurrent work before the final run.

## 2026-10-19 ANPU implementation: mixed fixed-face power queries

`compile-mixed-classic` is now an O2+ plan builder after fixed signals and the
whole-fixed attempt. It joins eligible wire no-dust faces into a typed maximum,
reusing the whole-fixed electrical compiler. ANPU compiles 204,727 of 208,686
faces across 34,781 wires (98.10% static face coverage). Contacts, uncertain
conductors and unsupported emitters retain their original individual queries.
Whole-fixed circuits skip the extra plan. No wire walk, callback or piston
lifecycle is removed; this is the first mixed-execution slice, not BUD recognition.

One captured/live counterexample was caught before measurement: general captured
block reconstruction keeps lever/button/pressure-plate flags live, unlike held
torch/repeater properties. Those control faces deliberately remain general.
The regression checks held torch properties against changed live comparator values
over all 256 old/new analog combinations, and checks live control flags separately.
Other checks cover changing payload contacts, ordered propagation, pending work,
plan corruption/cancellation/invalidation, and exported query references/ranges.

An independent ANPU run matches at every boundary 0..1,024: full CircuitState and
typed Graph scheduler/pending counts, piston request/action FIFO, deferred events
and clock match the pass-off control. Projected component/contact blocks, comparator
values, ordered command output, screen frames and scheduled work match the physical
interpreter. Reset projections and 32 native continuation ticks match; physical
motion identity allocation is deliberately not compared as a raw encoding.

Three alternating **release cfg(test)** pairs over 5,000 active Pong ticks measure:

| Metric | Pass off | Pass on |
| --- | ---: | ---: |
| Median inference seconds | 3.541512 | 3.2172332 |
| Sample range, seconds | 3.4974253–3.5420881 | 3.1486146–3.3779743 |
| Active game ticks/s at median | 1,411.83 | 1,554.13 |
| Median compile seconds (separate) | 7.5087985 | 7.3902732 |

This is **1.10079x throughput, 9.16% less inference time** in the optimized test
executable, not production-only server TPS. Activation and tick execution are timed;
loading, compilation, reset/projection and correctness observations are outside the
inference timer. Every sample performs the same 267,998 scheduled ticks, 16,324,348
callbacks, 183,752 wire walks, 327,585 piston samples and 10,682 accepted actions.
Every pair matches full endpoint state, Graph future, reset and 32 native continuation
ticks. Compilation varies and these three pairs establish no compile-time gain.

The timed repeat has 34 process observations, with no Cargo/rustc/linker process
observed. The earlier load-contaminated batch is excluded and retained separately.
Evidence: `target/mixed-classic-anpu-summary.json`,
`target/mixed-classic-anpu-timing.{json,log}`, its `-observations.json`, and
`target/mixed-classic-anpu-differential.{json,log}`. The frozen timed executable's
SHA-256 is `388219840cbdf7da805d6e1211168f7e6ac4cdd36a5091cfa712dbace9b18616`.

The storage/read-pair contraction remains the next separate optimization. This
slice preserves all wire walks, so it does not claim the larger gain available
from eliminating certified private read/output wiring. No 50,000-tick physical
ANPU replay, production-only timing or broad mixed typed dispatch is claimed.

The rebuilt frozen snapshot passes **511 Redpiler tests, zero failures, 35 ignored**
in 157.90 seconds. Workspace checking and all 25 graph-loader tests pass. This
includes the regenerated O2–O5 export fixtures, expected pipeline order and the
corrected test invalidation after deliberately changing an emitter's block class.
Logs: `target/mixed-classic-final-suite.log`, `target/mixed-classic-final-check.log`
and `target/mixed-classic-loader-tests.log`. The final suite executable's SHA-256
is `b1ca5dd90710790a587d0b1e603d592b960568fe8c77d132866f6be344c8aa00`.
Concurrent unrelated work continues in this shared checkout; these results describe
the frozen executables, rather than certifying later edits by other work.
