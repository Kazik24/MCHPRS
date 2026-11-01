# Sampled Redpiler → LLVM DLL experiment

## Goal

Reproduce the previous native engine's performance improvement using the current sampled backend: compile its validated representation into a circuit-specific DLL and execute the circuit inside that module. Target approximately 1.5× kernel throughput as the experimental ambition, with measured correctness and setup costs. This is an opt-in experiment, not a proposed default switch.

The primary deliverable is a complete native tick runner with useful specialization. A DLL that only forwards tick calls into Rust does not satisfy that objective. Early query-only or generic-engine stages are development checkpoints, not completion.

Keep the current serial semantics, including ordered callbacks, scheduled priorities, piston events, motion completion, operation limits, and reset continuation. Do not combine this experiment with the relaxed parallel runtime: that would make it difficult to distinguish code-generation gains from changed simulation work.

## 1. Recovered architecture and results

The implementation is recoverable from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e` (`C inference engine experiment`). Commit `d917e568` removed it. Inspect history without reverting the current backend:

```powershell
git show 576ae313:docs/notes/redpiler-pistons-v4/LLVM_NATIVE_INFERENCE_DEMO.md
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.rs
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine/emit.rs
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.h
git show 576ae313:crates/core/src/redpiler/backend/graph/native_engine.c
git show 576ae313:crates/core/src/redpiler/backend/graph/native_query.rs
```

### Successful design to carry forward

1. Rust performed normal circuit discovery and lowering.
2. The emitter serialized immutable graph tables and generated graph-specific power/guard functions.
3. Clang compiled C at `-O3` to an x86-64 Windows object; lld linked a DLL.
4. The DLL owned tick scheduling, pending membership, ordered callbacks, wire execution, piston/resource transitions, and deferred effects. Unsupported expression roots used the general C evaluator, with zero Rust graph-evaluator fallback during native inference.
5. Rust loaded the module, supplied independent mutable engine instances, projected state into the host, and implemented verification/handoff.
6. Shared immutable machine code was reused; mutable engine state was never shared between benchmark instances.
7. Sparse dirty projection was required to retain the kernel gain in integrated timings.

The historical Rust/C interface used explicit C records, allocator callbacks, loaded function pointers, ordered future snapshots, dirty-state transfer, and ordered effect queues. Reuse these architectural ideas. Port current sampled semantics rather than resurrecting the old graph/contact/guard machinery.

### Performance evidence

The old experiment eventually succeeded; source size and cold setup were its practical limits. The work log records:

| Experiment | Result | Scope |
| --- | --- | --- |
| Early query-only full sort | Native 90.877 s vs Rust 75.369 s | About 2% native query coverage; not the successful final architecture |
| Final v13 full sort, 10,036 ticks | Rust core 66.327 s vs native 49.040 s | 1.3525× kernel throughput in one paired full-sort sample |
| Same v13 pair with projection | Rust 71.797 s vs native 57.321 s | 1.2526× combined throughput |
| Final v13 repeat, three 1,000-tick pairs | Median Rust core 6.480 s vs native 4.972 s | 1.3032× core and 1.2038× combined throughput; all pairs favor native |
| Earlier specialized v12 repeat, three 1,000-tick pairs | 1.2050× ratio of median core times | Repeatable gain; different pinned stage from v13 |

The v13 work log also records three alternating 1,000-tick repeatability pairs in its “Sparse projection repeatability” section. Preserve these as historical evidence, not timings for the current backend. Complete per-tick state/future/effect verification, frozen RAM, reset, and interpreted continuation passed in the recorded full-sort run.

The user's approximately 1.5× recollection motivates reproducing the experiment. The exact recovered v13 full-sort figure above is 1.3525× core throughput; benchmark the new backend independently rather than assuming either figure transfers unchanged.

## 2. Why the old source became enormous

The raw O0 representation had 129,379 nodes, 522,011 sites, 226,471 routes, 5,812,695 deliveries, 3,758,922 guards, 41,354 wires, 16,561 pistons, and 13,118 resources. One designated-initializer export was **1,074,091,660 bytes**. A later measured raw source was 1,069,701,237 bytes, producing a 244,669,952-byte DLL and taking 131.3 seconds for native setup.

The final specialized v13 representation was smaller but still expensive:

| Artifact/cost | Historical measurement |
| --- | ---: |
| Generated C | 451,136,648 bytes |
| DLL | 118,535,168 bytes |
| Original cold native setup | 501.814 seconds |
| Immutable table initializer text | 350,005,491 bytes, about 77.6% of C source |
| Guard definitions | 385,113 |
| Query definitions | 71,155 |

The final file was about 451 MB decimal, while earlier versions exceeded the reported 500 MB problem by a wide margin. Largest final text tables included 1,162,483 delivery entries consuming 115,345,887 source bytes. Repeated field names inflated C source beyond binary payload size.

A giant query switch also caused an aborted compile after more than ten CPU minutes. The later design used bounded sparse function-pointer dispatch. Literal guard folding and verified code reuse improved practicality, but cold compilation remained substantial.

The new design must bound generated instructions and keep table payloads out of C initializer text. Smaller instruction streams help, but do not by themselves guarantee a small translation unit or short LLVM optimization time.

## 3. Historical sampled Program size census

Existing measurements were read and checked from:

- `target/container-census-20261101/report.txt`
- `target/container-census-20261101/{zcore,fpu,chungus}.json`
- `target/container-census-20261101/row-statistics.csv`
- `target/kernel-plan-trials/smallvec-memory.log`

These are retained-capacity measurements from earlier source snapshots, not a fresh compile of today's worktree. They include nested vector buffers and exclude allocator bookkeeping, map metadata/spare buckets, some deep command data, Arc headers, and temporary compiler allocations. They are neither full process memory nor predicted DLL sizes. Later SmallVec and flattened-plan changes alter the current totals.

### Overall retained vector buffers

| Fixture | Program bytes | Program MiB | Initial Runtime MiB |
| --- | ---: | ---: | ---: |
| ZCore12B_FIX | 131,607,405 | 125.51 | 19.59 |
| FPU_27 | 69,982,424 | 66.74 | 10.09 |
| Chungus | 178,114,019 | 169.86 | 56.25 |

### Logical array lengths

| Program array | ZCore | FPU | Chungus |
| --- | ---: | ---: | ---: |
| Components / `positions` | 87,786 | 46,702 | 246,601 |
| `operations` | 35,578 | 41,839 | 1,269,014 |
| `links` | 79,569 | 46,620 | 466,517 |
| `movement_steps` | 159,417 | 93,742 | 0 |
| `movement_flips` | 166,314 | 96,954 | 0 |
| `movement_wires` | 35,958 | 13,481 | 0 |
| `movement_groups` | 27,019 | 13,880 | 0 |
| `input_arrangements` | 73,869 | 44,453 | 0 |
| `arrangement_links` | 75,448 | 45,749 | 0 |
| Pistons | 16,834 | 8,100 | 0 |
| Tokens | 13,169 | 6,609 | 0 |

### Important ZCore buffers

“Used” below is `len * Rust element size`; “retained” is `capacity * element size`. Pointer-bearing records exclude their nested payload in this table.

| Array | Used MiB | Retained MiB |
| --- | ---: | ---: |
| `operations` | 0.543 | 1.000 |
| `links` | 1.821 | 3.000 |
| `movement_steps` | 4.865 | 8.000 |
| `movement_flips` | 6.344 | 10.000 |
| `output_programs` | 2.679 | 4.019 |
| `arrangement_links` | 4.605 | 7.500 |

The deep arrangement tables are also important: ZCore has 248,841 `input_arrangements.inputs.sources` rows but only 88,645 source entries. That family retains 14,476,536 bytes in container and payload storage. Its parent input-row family records another 7,745,040 container/payload bytes, with overlapping parent/child storage accounting; do not add arbitrary family totals to the already computed Program total. FPU has 152,204 such source rows and 55,370 entries. Empty row headers and excess capacity are material costs, not generated instruction counts.

Recent flattened-plan logging for ZCore additionally records 62,852 `movement_transitions` (2,621,440 retained bytes), 200,700 `movement_ranges` (4,194,304 retained bytes), and 29,937 entries in `movement_cells` (872,208 retained bytes including row storage). These later buffers must be counted in a fresh census before choosing the native encoding.

### Consequence for code generation

ZCore's 35,578 ordinary operations are far fewer than the old million-entry delivery table, but movement is a separate, substantial stream. Chungus has over 1.26 million ordinary operations: generating a distinct C function or long unrolled body for every occurrence can still reproduce source/compile bloat.

Refresh the complete census on the current source before implementation. Report lengths, capacities, actual element sizes, deep payloads, row histograms, and the reachable runtime subset. Generate payload from logical lengths only. Do not serialize Rust allocation slack, Vec/SmallVec headers, duplicate compiler proof tables, or lookup maps that the native tick never uses.

## 4. Proposed pipeline

```text
World snapshot
  -> existing sampled discovery / validation / instruction sharing
  -> native preparation: reachable tables + compact typed records
  -> binary Program blob + bounded specialized C functions
  -> Clang -O3 object(s) + lld -> experiment DLL
  -> native scheduler / signal execution / piston movement
  -> dirty-state and ordered-event export
  -> existing Rust host publication / reset handoff
```

Preserve two native paths within the same DLL:

1. A complete generic native evaluator for the sampled representation, including queues and movement. This is the correctness foundation and handles large/cold sequences.
2. Circuit-specific native functions for selected ordered operation/movement bodies. They remove opcode dispatch and embed validated topology/index constants while using live mutable state.

Measure generic native and specialized native separately against Rust. Both languages already use LLVM; the intended advantage is specialization, compact representation, and removal of dispatch/dependent lookup work. The generic port is useful for validation and fallback inside the DLL, but is not assumed to produce the final performance gain alone.

Do not introduce LLVM library bindings in the first experiment. Reuse the established C → Clang/LLVM → DLL workflow. Consider direct LLVM IR/object emission only if measurements identify the C frontend as a remaining bottleneck after binary table separation.

## 5. Compact Program data and source-size controls

### Native encoding

Build a separate immutable native representation from validated `Program`. Use explicit fixed-width C-compatible records and flat payloads with checked offset/count ranges. Example starting layouts:

| Record | Proposed encoding | Nominal bytes |
| --- | --- | ---: |
| Operation | `u32 operand`, `u8 opcode`, explicit padding | 8 |
| Electrical link | `u32 source`, `u32 target`, `u8 attenuation`, flags/padding | 12 |
| Range | `u32 start`, `u32 count` | 8 |
| Output program | Two ranges | 16 |
| Movement step | `u32 node`, wire range, flags/padding | 16 |
| Arrangement flip | `u32 target`, `u32 source`, `u32 link`, attenuation/source-kind/flags/padding | 16 |

These are proposed ABI records, not Rust struct casts. Validate actual size, alignment, field offsets, integer narrowing, and maximum aggregate offsets. Reject oversized programs before allocating or generating code. Encode enum tags, option sentinels, booleans, and flag masks explicitly.

For scale, the three ZCore streams `operations + movement_steps + movement_flips` would contain 5,496,320 bytes under the proposed 8/16/16-byte records, before ranges, other tables, and alignment. This is a calculated encoding example from the historical counts, not a measured complete native blob.

Flatten nested lists to offsets plus contiguous elements. Share identical immutable sequences where the existing ordering/identity rules permit. Preserve component/link identity even when instruction storage is shared. Retain only runtime-reachable metadata after a field-by-field reader audit. Original token positions used to project vacated geometry are runtime-required metadata, even though named as initial state.

### Separate binary data from executable text

Rust owns an immutable validated blob and passes a versioned table view to the DLL. The blob may be persisted as a binary artifact for inspection/reuse; load it into owned aligned native records with bounds validation. Do not cast unaligned file bytes to structs, or dump pointer-bearing Rust objects.

Generic native loops read this blob. Circuit-specific C contains index constants and selected code, not designated initializers for every Program row. This keeps C frontend work proportional to selected code rather than all graph metadata. The DLL need not embed the entire blob to be circuit-specific.

Start with explicit internal experiment limits: 8 MiB specialized generated C total and 64 KiB per generated body, measured in actual emitted bytes; adjust only through reported experiments. Exceeding a code budget routes that body to the generic native evaluator, without changing semantics or falling back to Rust. Limit table bytes separately through checked allocation budgets and report them. Avoid a giant switch that invites whole-program inlining. Dispatch by validated body ID through a bounded function table and keep the general native evaluator available.

Compile generic runtime code separately from specialized translation units. Share repeated bodies by canonical content and compatible operand bindings. Reuse shared rise/fall instruction ranges already in Program. Do not clone an entire movement body for every joint arrangement if a compact range parameter gives the same ordered work.

## 6. Native mutable state and ABI

Use a versioned `NativeApi` with ABI version, descriptor sizes, semantic-version fingerprint, target/CPU requirements, and create/destroy/tick/use-control/export functions. Explicitly validate the C/Rust layout contract at module load. Rust `Block`, Vec, HashMap, SmallVec, Option, and Rust enum layouts do not cross the ABI directly.

The native state contains compact current block descriptors, comparator levels, contribution arrays/histograms, input caches if retained, token positions/occupancy, joint arrangements, piston flags/motions, phase stamps, scheduler storage, pending counters, dirty cells, and effect buffers. Represent block type/properties in explicit native fields; convert to Rust blocks at host export. Keep enough information to preserve all admitted component behavior and exact reset state.

Start each engine from converted entry state and imported typed scheduled work. Use the existing supported-entry validation, including piston-state restrictions. Keep native buffers persistent across ticks. Use host allocator callbacks or fully native-owned allocation with matching destruction; never free memory through the wrong allocator. Bound queues and propagate allocation, capacity, and semantic-operation failures with component IDs/error codes.

One native tick executes all existing sampled phases, including the two scheduler drains and cursor advancement, piston event FIFO, motion completion, and nested ordered updates. Preserve early-retract, interrupted-motion, joint arrangement, side-input, observer, copper-bulb, note, and command semantics. Initialization, controls, and snapshot APIs must also preserve operation accounting and errors.

Keep the library handle alive until every engine and function pointer using it has been destroyed. Share immutable code/blob ownership across instances while allocating independent mutable state. No module unload or program replacement during an executing call.

## 7. Specialize the measured hot path

The current ZCore profile attributes 65.1% of kernel samples inclusively to `set_token_pos -> run_move_steps`; arrangements/flips account for 24.9%, pre-callback wire refresh 12.4%, with additional ordered and watcher delivery costs.

Prioritize native specialization of movement-plan execution and shared output programs:

1. Select the current movement range from live arrangement state.
2. Emit direct native calls or bounded straight-line code for ordered flips, with constant link/target/attenuation metadata and live source strengths.
3. Preserve input aggregate updates, wire refresh ordering, switched-node deduplication, watchers, callbacks, and operation limits exactly.
4. Specialize constant source kinds only when their invariant is validated. A movable redstone block can have a constant intrinsic strength while its position/connectivity remains dynamic.
5. Retain native generic loops for large ranges and dynamic branches. Record calls and executed operations by specialized/general native path, so cold target counts cannot masquerade as hot coverage.
6. Test compiler optimization levels and bounded inlining only after correctness. Preserve code-size budgets; many functions or repeated full-body inlining can increase instruction-cache and compile costs.

Profile after specialization: identify whether dispatch, indexing, input histograms, or the unchanged semantic work now dominates. Do not assume LLVM will remove callbacks whose ordering remains observable.

## 8. Host integration, flags, and toolchain

Begin with a headless benchmark-only native feature/selection, using explicit compiler/linker paths. Expose `/rp compile --native` after full lifecycle validation. Keep it independent of `--publish-all`, `--publish-redstone`, and IO-only options. Reject combining `--native` and `--parallel` during this experiment. Default execution remains the current Rust runner.

No per-query or per-flip Rust callback during native inference. Host-only effects are queued with their captured payloads and exported in order. Full command support must preserve shared output quotas and command state; early stages may explicitly reject unsupported circuits before activation. FPU census includes command entities, so FPU admission cannot be assumed until those semantics are implemented.

Publish dirty state only at host refresh boundaries or forced synchronization. Avoid copying the entire native state into a shadow Rust Runtime after every tick. Read-only verification can use complete snapshots outside performance timing. Reset exports all blocks/entities, relative ordered scheduled entries, piston events/motions, and queued effects required for interpreted continuation. Keep ownership on export failure.

Compile in the existing isolated worker, with cancellation, stale-snapshot rejection, and one large job per plot. Invoke tools with structured argument arrays, bound diagnostic capture, kill/join task-owned child processes on cancellation, and distinguish missing-tool, emit, compile, link, load, and activation errors. Compilation failure never partially activates the DLL.

The old portable Clang path is historical evidence, not an available-tool guarantee. In the present shell, `clang`/`clang-cl` were not resolved and the discovered `lld-link` belongs to the ST toolchain. Confirm a matching x86-64 Clang/linker installation before implementation benchmarks. Record executable versions, hashes, flags, and target features.

For initial reuse, cache immutable modules only with bounded retention. Key executable code by ABI/semantic version, generic runtime and generated source hashes, compiler/linker versions or binary hashes, flags, target/CPU features, and any embedded program constants. Validate each blob separately against the code contract. Hash initial state only when actually baked into generated code. A path string alone is not toolchain identity. Report cold compilation separately from warm module reuse.

## 9. Implementation sequence and deliverables

1. **Fresh census and feasibility:** enumerate runtime-read fields, classify compiler-only tables, export compact binary descriptors, record source/blob estimates for ZCore/FPU/Chungus. Deliver a size report and descriptor validator before porting execution.
2. **ABI and small full engine:** implement version/layout handshake, lifecycle, blocks, fixed contributions, scheduler, controls, and effects on tiny circuits. Exercise a real compiled/loaded DLL. Zero Rust tick fallback is required even here; unsupported circuits decline explicitly.
3. **Movement parity:** port arrangements, ordered movement plans, piston event/motion machinery, input/wire updates, and operation/error handling. Validate ZCore intermediate state and reset continuation against the Rust sampled backend.
4. **Coverage:** add remaining admitted components, reactive paths, commands/notes, and FPU/Chungus coverage. Record unsupported semantics as a list, not a silent fallback.
5. **Circuit specialization:** emit bounded shared output/movement bodies targeting profile evidence. Measure generic native vs specialized native to isolate the benefit.
6. **Cold/warm measurement:** run paired benchmarks with separate generation, blob conversion, compiler, linker, load, initialization, kernel, projection, and total request latency. Verify cancellation and module/state isolation.
7. **Optional server integration:** wire the compilation flag, diagnostics, publication, save/reset, toolchain error messages, and module lifetime only after headless acceptance.

Likely new code: `redpiler/sampled/native/` containing ABI descriptors, compact exporter, source generator, module loader/process runner, native runtime C/header, and verifier. Adapt `redpiler/mod.rs`, options, compiler preparation, sampled runtime initialization/publication adapters, benchmark examples, and targeted tests. Add dynamic loading only under the experiment feature if no suitable existing dependency is present; reuse installed hashing/process facilities where available.

## 10. Correctness and performance protocol

Freeze a baseline from the current retained optimized Rust runner, including movement layout, SmallVec, diagnostic-load, wire-cache, and publication changes actually present in that build. Record source revision/worktree patch, fixture hash/import preparation, toolchain, affinity, and flags. Compare the same admitted representation and input sequence.

### Correctness outside timing

- Run a native/Rust shadow after every control and logical tick. Compare blocks/projection, comparator outputs, contribution slots and input aggregates, token and arrangement state, piston flags, active motion identities/deadlines/order, relative ordered scheduler entries/type bindings, pending membership, and ordered notes/commands.
- Compare dirty export and full publication under both publication feature configurations and supported override flags. Check that sparse export does not lose hidden IO-only work.
- Test reset in active motion and stale scheduled entries, then compare at least 32 interpreted continuation ticks. Include final native-interpreter screens plus intermediate observations.
- Check zero-delay scheduling, priority/FIFO, duplicate entries, operation/queue exhaustion, source-state changes during callbacks, equal-strength competing inputs, analog inputs, interrupted motion, two-token groups, notes, command quotas, and independent states sharing one DLL.
- Run specialized/general native comparison separately where practical. Validate emitted ranges and source-generation limits on small malformed/corrupted descriptor fixtures.

### Timings

Use at least seven alternating baseline/candidate pairs on ZCore and FPU, including established 10,000-tick runs; include Chungus as a large non-piston stress case. Add tiny circuits and idle/active checks. Verify actual native dispatch/work counters and meaningful output so idle engines cannot appear fast. Measure correctness in separate runs; no per-tick timers or shadow snapshots inside kernel-only loops.

Report kernel TPS/time, integrated tick plus publication time at the same cadence, total CPU usage, generated source bytes, emitted function counts, largest body, blob bytes, DLL `.text`/read-only data/file size, cold wall time, compiler peak memory where measurable, and warm setup. Kernel improvement and reduced publication cost must remain distinguishable.

For amortization, compute `extra cold setup / measured per-tick time saving`; report required ticks or workload episodes only when the denominator is positive. Do not omit setup because timing instances reused a module. Treat approximately 1.5× as the goal, and retain conclusions based on measured paired results and variance.

## Decision

Proceed as an experiment to reproduce the demonstrated old native-engine gain on the sampled IR. The new representation offers a credible reduction in generated-code volume, especially for piston CPUs, and exposes a narrower hot path for specialization. Binary table separation and code budgets make compile size controlled by design. Whether native specialization reaches the desired gain, and whether cold setup is economical, remain measurements to obtain.
