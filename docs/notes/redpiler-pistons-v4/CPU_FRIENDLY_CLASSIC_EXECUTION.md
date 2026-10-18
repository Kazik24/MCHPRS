# CPU-friendly classic execution: measured baseline and proposed next step

Date: 2026-10-18. Status: design, not an implemented executor. Whole fixed compilation is already the default at O2 and higher. This proposal keeps the existing Graph backend and its single authoritative machine state and future.

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

The side experiment `experimental-coalesce-delayed-updates` remains outside defaults. It removes equivalent read/schedule-only notifications inside protected runs, with pending/FIFO and direction/lock proofs. CHUNGUS and ANPU match complete state/future through 1,024 ticks plus reset/native continuation. The rule does not replace a wire walk by one piston sample and has no production timing claim. See CLASSIC_REDSTONE_PROGRESS.md and target/chungus-stage2-summary.json for detailed counts, sample ranges, hashes and limitations.
