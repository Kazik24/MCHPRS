# LLVM native inference demo: append-only work log

> Archive status, 2026-10-18: C/LLVM and finite/symbolic synthesis source has been removed.
> Recover the last combined source from commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> Historical results and plans below remain preserved; see
> [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current behavior and restoration details.

Append new dated sections at the end. Preserve earlier observations, including failed
experiments; supersede an incorrect conclusion explicitly in a later section. Separate
plans, successful compilation, actual native execution, correctness checks and performance.

## 2026-10-16 12:29 +02:00 — scope, toolchain and first benchmark issue

### Objective and representation

The objective is maximum measured CPU_BubbleSort inference throughput while preserving
observable output, logical memory, timing, pending work and interpreter continuation.
The requested sequence is a simple C-generation demo, a second test covering more of
the graph, then work toward complete `Graph -> DLL -> inference` execution. Deeper
production integration depends on measured benefit.

The native experiment is independent of the unfinished symbolic compiler. Its initial
input is the ordinary O3 graph after instant-mechanism recognition. The first emitter
reads the stable lowered tables for that graph; it does not consume symbolic kernels,
BDD trajectories or learned input histories. The user has disabled symbolic synthesis
separately. Concurrent changes to that compiler are outside this experiment.

The first version specializes unconditional `Output`/`Stored` power queries whose
sources are fixed components. Generated C reads the live electrical/wire strength
fields and performs saturating attenuation and maximum. Dynamic guards, contacts and
other unsupported queries retain general Graph execution. Scheduling, wire propagation,
callbacks, mutations and projection still execute in Rust. This is partial native query
execution, not a complete C implementation of the CPU.

### Toolchain evidence

- The installed ST ARM Clang rejected the x86_64 Windows target; it cannot compile this demo.
- Portable official LLVM/Clang 23.1.3 was downloaded under `target/native-query-demo/`.
  The archive SHA-256 matched the GitHub release asset digest:
  `7e69dc07a4536b79ff24a651a2611990bf8eaab17ea560f1092707c4f31ba01d`.
- Clang compilation to an x86_64 MSVC object followed by `lld-link /dll /noentry`
  produced a loadable DLL without a CRT dependency. A separate byte-attenuation probe
  passed all 256 input values through the loaded DLL.
- Toolchain source:
  <https://github.com/llvm/llvm-project/releases/tag/llvmorg-23.1.3>.

### Native state-layout obligation

The first emitter measures scalar field offsets from actual Rust field references in
the current build. Each query passes the current component allocation base; it does
not retain field pointers across mutations or assume a stable Rust ABI across builds.
Generated code is specific to this graph and executable layout and must not be reused
with a different build. Component variants must continue to match their fixed graph
owners. C only reads supported scalar bytes during a synchronous immutable borrow.

### Confirmed benchmark bug: idle CPU produced meaningless timings

The first dedicated harness incorrectly activated
`cpu.origin + cpu.start - BlockPos(8, 8, 8)`. The subtraction belongs to the existing
`click_cpu` helper because its nested `click` adds eight; `Compiler::on_use_block`
already accepts absolute world coordinates. The dedicated harness therefore clicked
an unrelated position and left the CPU idle.

Observed tick totals of approximately 0.00003–0.00015 seconds per 100 ticks are invalid
performance evidence. They must not be reported as an LLVM speedup. The original
report is preserved as `target/native-query-demo/v1-idle-invalid.json`.

The corrected activation is `cpu.origin + cpu.start`, with an assertion that the block
is a stone button. Acceptance now additionally requires actual scheduled and wire/piston
work, positive native-query hits in native mode, and every tick's RAM matching the
frozen interpreter reference. Matching two idle executors is not a correctness check.

### Verification and timing contract

- Run query-by-query native/general comparison separately from performance samples.
- Alternate Graph/native sample order using one release executable and fresh worlds.
- Validate per-tick RAM against the pinned interpreter trace, then compare reset and
  32-tick interpreter continuation between modes.
- Report graph preparation and C compilation separately. Report activation separately
  from tick time; do not claim savings merely moved into activation.
- Tick timing includes native calls and all remaining Graph work. Display flush, RAM
  inspection, reset and continuation are outside that timer; tick speed is not whole-server TPS.
- Count compiled targets and actual native hits/fallbacks separately. Positive native
  execution does not imply eliminated wire walks or callback work.
- Background Rust compilation occurred during exploratory runs. Final performance
  comparisons need uncontended samples; no accepted native speed result exists yet.

### Checks and next experiment

The ordinary O3 100-active-tick benchmark passed its existing RAM/reset/continuation
comparisons with the interpreter. A single exploratory measurement was 0.912953 seconds
for Graph and 0.941588 seconds for interpreter, with concurrent builds; it is not a
native comparison or a reliable speed estimate.

The `native-demo` feature bench passes `cargo check -p mchprs_core --bench
cpu_bubblesort --features native-demo --locked` after fixing export visibility and
passing the frozen RAM trace into the run helper. The corrected release benchmark is
being rebuilt. No full-sort native execution has been established at this checkpoint.

The next native coverage experiment will compile live guard/source selection directly,
use current component/piston/resource allocations, and replace additional hot map lookups
with dense bindings. Unsupported captured operands, recursive dependencies or unknown
layout must remain explicit fallback cases. The existing instant-compiler edge-case
register still governs event-time reads, ordering, reentry and handoff.

## 2026-10-16 — corrected v1: real workload, low coverage, no significant speedup

The corrected benchmark executed the start button at its absolute world coordinate.
All seven runs (one separately verified native run and three alternating Graph/native
pairs) matched the frozen interpreter RAM trace at every one of the 100 active ticks.
The paired reset checkpoints and 32-tick interpreter continuation also matched.

The separate verifier checked **50,392 native query results** against the general Graph
evaluator with no mismatches. Native runs recorded 50,392 hits and 2,344,449 fallbacks:
only **2.10% of runtime query invocations** used C. Compiling many cold targets is not
the same as covering the hot execution path.

| Pair | O3 Graph tick seconds | O3 plus native query tick seconds |
| --- | ---: | ---: |
| 1 | 0.692521 | 0.638111 |
| 2 | 0.615682 | 0.641744 |
| 3 | 0.650925 | 0.681669 |
| Median | 0.650925 | 0.641744 |

The ratio of medians is 1.0143x (about 1.4% less tick time), the ranges overlap, and
two of the three pairs favor Graph. **This is not evidence of a significant speedup.**
The earlier favorable first pair does not represent the completed sample group.

Compilation emitted 13,863 of 94,807 nonempty power targets, containing 14,150 links.
Generated C was 2,300,120 bytes and the DLL 505,856 bytes. One native sample took
5.233982 seconds to generate/compile/load its C module, in addition to ordinary Graph
preparation. This compilation cost is excluded from the inference table above.

Both paths retained the ordinary work: 20,005 scheduled ticks, 1,703,697 callbacks,
32,505 wire walks, 302,784 piston samples and 27,884 accepted actions in the 100-tick
episode. This demo removes interpretation inside selected queries; it does not contract
wire propagation or whole instant regions.

Artifacts: `target/native-query-demo/v1-active-smoke.json`, pinned executable
`cpu-bubblesort-v1-active.exe`, and the `power-query-26800-*.c/.dll` modules in the same
directory. The verifier's tick time includes comparison overhead and is not included
in the performance samples.

A complete 10,036-tick Graph/native sort comparison is now running on the pinned v1
executable. Its result is not yet established. Phase 2 is being developed separately:
live guard/piston/resource reads, contact selection, fixed directional emission,
conducted dependencies and dense target bindings, independent of symbolic synthesis.

## 2026-10-16 — v1 full sort passes correctness but regresses inference speed

The pinned corrected v1 executable completed one fresh Graph/native pair of **10,036
ticks**, covering the 10,035-tick sort and one settling tick. Both modes matched every
tick of the frozen interpreter RAM trace and ended with sorted data. Their reset
checkpoints, continuation RAM and 32-tick interpreter continuation checkpoints matched.

| Execution | Tick seconds | Additional native compilation seconds |
| --- | ---: | ---: |
| O3 Graph | 75.368733 | 0 |
| O3 plus v1 native queries | 90.877051 | 5.253992 |

Native tick time was **20.6% worse** in this pair. This is a single complete-sort
comparison, not a repeated estimate, but it rules out treating the earlier favorable
100-tick pair as a demonstrated end-to-end benefit.

The full native run recorded 5,701,254 native query calls and 273,696,584 fallbacks
(approximately 2.04% native invocation coverage). Both paths still performed 2,344,242
scheduled dispatches, 199,211,691 callbacks, 3,882,011 wire walks, 35,541,685 piston
samples and 3,295,083 accepted piston actions. Low coverage combined with per-query
selection/counter overhead is a plausible explanation of the regression, not an isolated
profile proving its exact distribution.

Report: `target/native-query-demo/v1-full-sort.json`. Inference timing excludes loading,
ordinary Graph preparation, C compilation, activation, projection, RAM inspection, reset
and continuation. Activation is recorded separately and was negligible in this episode.
No production integration of this narrow path is justified by these measurements.

### Explicit scope change: compile the complete executor, including queues

The user now explicitly requests the whole graph as a long C program, including the
tick queue. The next target must execute scheduler/pending membership, callbacks,
wire walks and captures, piston request FIFO/acceptance, shared resource transitions
and deferred publications inside C. Delegating those operations to Graph through an
FFI trampoline would not satisfy that scope.

Rust may construct the graph, allocate buffers, project results to the World and run
independent verification. Unsupported semantics must be rejected before activating
the native executor rather than silently counted as full native execution. The target
fixture remains CPU_BubbleSort; inventory or a rejected unsupported subset is not its
completion. The symbolic compiler is not a dependency.

### Phase-2 source review observations, before execution

Review identified two obligations while expanding native query coverage: field offsets
inside `ComponentState` must be relative to the outer enum element, not its payload;
and a conducted read through a contact must return zero if no live variant resolves.
These were source-review findings, not observed fixture failures. Captured predicates
in ordinary `query_power` have no capture input and must retain that exact context.
The initial concern about component-receiver profile orientation was checked against
the current source: `resolve_profile` deliberately uses the receiver's `node_block`,
while solidity/transparency remain profile facts. That concern was withdrawn.

## 2026-10-16 — Whole-engine first-link preparation and stronger verifier

The full native path serializes the O0 graph (user authorized earlier representations), compiles one C translation unit with immutable graph tables and a native causal runtime, and attaches its DLL before activation. Rust is responsible for loading, external input calls and world projection; C owns scheduled ticks, pending membership, route callbacks, captured wire walks, pistons, resources and deferred publication. There is no Rust graph-evaluator fallback inside a native tick. Optimized proxy programs are rejected explicitly in this prototype.

First fixture attempt stopped before C compilation: specialized contact nodes legitimately have no block template. The emitter now preserves those IDs with a sentinel profile; contact variants supply their actual runtime profile. This was an emitter precondition bug, not an inference result.

Review caught and corrected: captured Site guards must fail when their capture is absent; captured Turbo power changes must propagate even when the live wire already equals the computed value; piston availability guard values must be encoded in the ABI value field; reallocating a walk's site array invalidates pointers into that array. The C runtime builds with freestanding Clang -O3 -Wall -Wextra -Werror.

Added an optional Rust Graph shadow for the full-engine validation run. It compares all component/piston/resource state, ordered relative scheduler entries (including type witnesses), queued piston work/deferred publications and ordered presentation effects after each input/tick. This expensive verification is separate from performance samples. Tick and flush time are now reported separately; native state projection is part of flush, so tick-only speed is not whole-server speed.

### Whole-graph compilation cost and interrupted attempt

The raw O0 BubbleSort program contains 129,379 nodes, 522,011 sites, 226,471 routes, 5,812,695 deliveries, 3,758,922 guards, 41,354 wires, 16,561 pistons and 13,118 resources. Initial designated-initializer C was 1,074,091,660 bytes. This is a literal complete graph export, not a reduced CPU algorithm or prerecorded trace.

Stopped one compile attempt after identifying an emitter error: `.value` (a byte used only for piston availability) was populated with child/contact indexes for unrelated guards. That generated conversion warnings in bulk and excessive captured diagnostics. Fixed encoding to set the byte only for the relevant state test and added `-Werror` to generated-program compilation. No inference or speed result came from the interrupted attempt.

The Luna subagent added a standalone freestanding C self-check (`native_engine_test.c`) for imported tick type witnesses, duplicates, stale entries, typed pending dedup, zero-delay FIFO/priority, scheduler wraparound/relative snapshot and missing captured guard operands. Built with Clang -O3 -Wall -Wextra -Werror, linked without CRT, returned 0 through Windows P/Invoke. This complements the full-fixture Rust comparison; it does not replace it.

### First complete native execution validation

The corrected raw O0 export compiled and loaded successfully. Its separate 100-tick full-engine shadow run completed: every input/tick matched Rust Graph component/piston/resource state, ordered scheduled entries with type witnesses and relative delays, queued/deferred piston work and ordered presentation effects. The same run also checked frozen interpreter RAM, reset checkpoint and 32 interpreted continuation ticks. This establishes native execution of the real active circuit, including causal queues, rather than the earlier query-only path. The paired unverified timing run is still pending.

Read-only review found one additional emitter omission: container profile flags were absent although native piston lane admission tests them. The next build sets the container bit using the same `ContainerType::from_block` predicate as Rust. The already-running raw fixture validation passed without that bit; that does not establish container-edge correctness.

The benchmark now permits O0 through O2 native inputs; O2 is the default for the next experiment, after existing safe graph reductions and before instant proxy lowering. Symbolic synthesis remains disabled independently. This reduces code-generation cost without requiring a kernel compiler.

## 2026-10-16 — Raw whole-engine smoke performance (O0)

Pinned executable: `target/native-query-demo/whole-engine-v3.exe`; report: `target/native-query-demo/whole-engine-smoke.json`. Separate full-state/future/effect shadow validation passed for 100 active ticks. The unverified pair measured:

| Scope | Rust O0 Graph | Whole native O0 C |
| --- | ---: | ---: |
| 100 inference ticks | 1.1326737 s | 1.0466377 s |
| flush/projection | 0.7101483 s | 0.8950924 s |
| combined tick + flush | 1.8428220 s | 1.9417301 s |

Inference-only ratio is 1.0822x (8.2% faster); combined execution is about 5.4% slower. This is one smoke pair, not a robust full-sort performance claim. Native compile/load cost was 131.2994 s. Generated source was 1,069,701,237 bytes and DLL 244,669,952 bytes. All work counters match: 20,005 scheduled ticks; 2,059,909 callbacks; 32,505 wire walks; 302,784 piston samples; 27,884 actions. Every tick ran inside C with zero Graph fallbacks. Golden RAM, active/reset checkpoints and 32-tick interpreter continuation match the paired Graph run.

The raw C result does not justify making native execution the production default. Next experiments use existing graph reductions, native early exit at maximum signal strength, and sparse state projection. The whole graph is currently immutable C tables plus a native event runtime; it is not yet straight-line machine-specific code for every route. LLVM is optimizing that runtime and table accesses; this experiment does not demonstrate automatic domain/kernel synthesis by LLVM.

### O3 follow-up explicitly requested

After the current O2 experiment, test O3 -> C -> DLL -> inference. Preserve validated instant proxy instructions rather than ignoring them. Keep original route data for captured Turbo lookup and emit a separate native instruction stream for ordered Dispatch, guarded Sample and direct ArmReset. Continue full Rust shadow verification before performance conclusions.

### Concurrent classic-redstone compiler work

User notified that `COMPILE_CLASSIC_REDSTONE_SCOPE.md` is being implemented independently. Read the full scope; do not duplicate that pass in the LLVM experiment. Its native integration point is the lowered direct-net representation: serialize committed net strengths as separate mutable state, dense direct input ranges, original entry-occurrence bindings and ordered receiver operations. Native execution must publish the committed signal at the original route occurrence, schedule through the same C queue and reconstruct private wire presentation from the committed signal, not the ingress's newest output. Accepted nets must perform zero private Turbo walks/callbacks/intermediate dust writes. Preserve proof metadata/ownership boundaries and continue whole-state, future-order, flush/reset/continuation checks. Adapt to actual landed IR rather than inventing a parallel representation now.

## 2026-10-16 — O2 whole-engine comparison before porting wire certificates

Pinned executable: `target/native-query-demo/whole-engine-v5.exe`; report: `target/native-query-demo/whole-engine-o2-smoke.json`. Full 100-tick state/future/effect shadow comparison passed, as did frozen RAM and paired active/reset/continuation checkpoints. Three alternating unverified pairs measured Graph tick seconds `[0.6490150, 0.6816113, 0.6888388]` versus C `[0.8231626, 0.8200071, 0.8544574]`. Ratio of medians is 0.8280x: native was about 20.8% slower. Including flush, ratio was 0.8185x (about 22.2% slower). All matched work counts: 20,005 scheduled, 1,760,072 callbacks, 32,505 walks, 302,784 piston samples, 27,884 actions.

Native compile/load times were `[39.8216322, 40.2282353, 39.4059172]` seconds. Source was 307,240,673 bytes; DLL 87,420,928 bytes. Graph construction/pass time was around 10 seconds and is separate. This version used the whole generic C runtime with Turbo indexing, but not Rust's retained wire query/transfer, isolated, dust-path and instant-reset shortcuts. It is a correctness/performance baseline, not a claim that every O2 acceleration was translated. The following O3 work ports those retained semantic plans and proxy instructions; it also includes max-signal early exits and sparse component projection. None of these results justify enabling this experimental backend by default.

### Agreed final pipeline boundary

User explicitly set the pipeline to:

```text
generalgraph -> prep_passes -> classic_redstone -> instant_inference -> dll
```

Generate C after domain inference. The domain passes prove and retain direct classic/instant operations; LLVM lowers and optimizes their C implementation plus the general residual runtime. C generation must preserve committed net state, entry occurrence and effect/future ordering. LLVM is not responsible for rediscovering these semantic certificates from a raw event interpreter.

## 2026-10-16 — O3 native plan port ready for fixture validation

Luna implemented native lowering/execution for validated proxy Dispatch/Sample/ArmReset and retained wire query-fact/transfer masks, dust paths, isolated replay and certified instant-reset transfer. Hop-root scratch pooling was already native. Original Turbo routes/offsets remain authoritative. Lowering clears executable delivery lists on proxy-bound routes; the emitter reconstructs their exact ordered rows from retained instruction witnesses and validates any nonempty original slice against those witnesses. Proxy inference excludes Turbo routes.

Source review caught a root/nested distinction: isolated and instant-reset shortcuts belong only at a newly seeded wire episode, not at arbitrary captured wire visits inside an existing Turbo walk. Moved them to seed evaluation; nested visits keep captured geometry/power semantics. Added actual-execution counters for six shortcut categories, rather than treating emitted plan counts as hits. The ABI now asserts the expanded counters structure size.

Native state projection now copies only changed component scalars, retaining the C dirty bitmap and Rust projection semantics; piston/resource states still copy completely. This addresses measured flush overhead without changing inference. Max-15 early exits were added only to pure power reductions. The C runtime links standalone without CRT, and the independent selftest returned 0 again via Python ctypes through the installed `py` launcher. O3 fixture correctness/performance is still pending at this section.

The explicitly requested GPT-6.1 Sol classic-redstone subagent is running alongside the C work. Discovery found 34 structural ANPU candidates (381 wires/35 receivers; largest candidate <=15 wires) and one small graph-fixture candidate. This is not accepted proof coverage. Native generation continues independently while exact classic IR/proofs are implemented.

### O3 smoke result, before expression-specialized query experiment

Pinned `target/native-query-demo/whole-engine-o3-v6.exe` passed 100 full state/future/effect comparisons and paired frozen RAM/reset/continuation checks. Report `target/native-query-demo/whole-engine-o3-smoke.json`: Rust O3 inference 0.6299003 s vs native O3 0.6903737 s (0.9124x ratio; native about 9.6% slower). Flush was 0.4911332 vs 0.6144730 s. This is one pair; full 10,036-tick shadow+pair is running separately.

Actual native shortcut calls: 1,278,479 wire-query-fact evaluations; 3,471 isolated replays; 62,373 instant route invocations / 190,330 steps. Dust-path and instant-reset hits were zero on this fixture; their presence in code does not establish BubbleSort coverage of those paths. All reported general work counters match Rust O3 exactly (including 1,703,697 callbacks for the 100 ticks). Native source 341,109,391 bytes, DLL 93,742,592 bytes, compile/load 42.317081 s, no Rust graph fallback.

Next experiment specializes supported power queries as actual C expressions over compact state, using the existing expression generator rather than adding a second optimizer. Unsupported query roots continue in the native general evaluator. Native verification will compare specialized queries against general C and complete ticks against Rust; inference performance remains unverified until those checks pass.

## 2026-10-16 — O3 full-sort result (complete native engine, retained O3 plans)

Pinned executable `target/native-query-demo/whole-engine-o3-v6.exe`; machine-readable report `target/native-query-demo/whole-engine-o3-full-sort.json`. Separate full shadow validation completed all **10,036 active/settling ticks** (323.1976681 s including setup/compile/checking). Component/piston/resource state, ordered scheduled work/type witnesses, pending membership, deferred work and ordered effects matched after every tick. Frozen RAM matched every tick, final RAM was sorted, paired active/reset checkpoints matched and 32 interpreter continuation ticks matched. This is real native queue/callback/wire/piston inference for the entire active sort, with no Rust Graph inference fallback.

Unverified single full-sort pair:

| Scope | O3 Rust Graph | O3 -> C -> DLL |
| --- | ---: | ---: |
| inference | 82.9315472 s | 88.4512619 s |
| flush/projection | 64.3822458 s | 73.8345447 s |
| tick + flush | 147.3137930 s | 162.2858066 s |

Native inference is about **6.7% slower** (0.9376x); including projection it is about **10.2% slower** (0.9077x). Cold native compile/load was 43.055245 s, separate from the roughly 13 s graph preparation/pass cost. This result does not justify a production switch. Absolute timings differ from earlier pinned builds/runs; compare the paired samples rather than combining unrelated runs into a speedup.

Work counts match exactly: 2,344,242 scheduled; 199,211,691 callbacks; 3,882,011 wire walks; 35,541,685 piston samples; 3,295,083 actions. Actual native O3 shortcuts: 149,011,455 wire-query-fact evaluations; 405,204 isolated replays; 7,174,222 proxy route invocations / 21,703,305 steps. No dust-path or certified instant-reset replay hits in BubbleSort. Generated C 341,109,391 bytes; DLL 93,742,592 bytes.

Conclusion for this stage: changing a carefully optimized Rust event executor into a C event executor is insufficient. Both are already compiled by LLVM. The next experiment emits graph-specific guard/power expressions directly into the same DLL, retaining native general evaluation for unsupported query roots. Classic committed-net lowering is being integrated concurrently; it removes semantic work before native generation, as agreed in the final pipeline.

### Classic/native ABI integration and query specialization under development

The classic pass now exposes flat `CircuitProgram::classic()` tables and `CircuitState::classic_signals`. Native StateView gains a fourth pointer for committed signals. Wire projection uses the committed signal minus its attenuation; full-state verification normalizes both executors' private wire projection because hot private-wire scalar storage is intentionally stale. No source-output shortcut may replace the committed signal for wire presentation.

Two entry kinds matter: committing Live Update occurrences publish the net and run ordered masked receiver operations; proven inert Shape occurrences skip the owned callback without publishing early. Entry placement uses true delivery ordinals, including duplicate sequence numbers and notifications from delayed receiver output routes, not only the ingress's own route. The complete receiver input may also have a direct live source contribution: max(committed net minus net attenuation, live source output minus source attenuation). C mirrors that distinction.

The next native version reuses the query expression compiler over typed compact C structs, emits dense target bindings and graph-specific guards/max/attenuation expressions into the full DLL. It does not call Rust from inference. The C runtime retains unsupported query evaluation; an opt-in verifier computes both specialized and general C results and the outer verifier compares full state/future/effects to Rust. Added `--classic-redstone` to the BubbleSort harness to exercise the agreed pipeline once accepted plans are available. This section records implementation work, not a successful native classic/query-specialized benchmark yet.
### Query specialization compile-cost limit and dispatch correction

The first specialized build (pinned `whole-engine-o3-v7-queries.exe`) emitted 398,599,529 bytes of C with 71,155 query functions and 110,525 guard functions. Its 71,155-case query dispatcher encouraged an enormous optimization unit. Stopped this task-owned Clang process after more than ten minutes (614.48 CPU seconds); no DLL, correctness result or performance number was produced. This is a failed compile-cost experiment, not a speedup claim.

The next build uses a sparse, bounds-checked function-pointer table for compact query dispatch. Each query still contains graph-specific expressions and can inline its called guards/dependencies; the dispatcher no longer invites LLVM to inline every root into one huge switch. Added a focused sparse-ID generation check. Added process-local SHA256 reuse of immutable DLLs for identical generated source, embedded runtime/header and compiler/linker paths. Every load still creates independent engine state. Reports mark cache hits; generation/hash/load time remains charged to setup, and cold compilation is reported separately. There is no persistent cross-process cache.

Native classic C/header/emitter support is now frozen for integration validation. It includes ordered committing/shape entries, stored delivered signals, direct-live receiver contributions, dirty projection and the existing native scheduler. Luna's standalone C selftest passes; Rust/C/interpreter integration is pending at this section.
### Native classic integration passed

Ran `cargo test -p mchprs_core --features native-demo --lib classic_native_dll_matches_rust_and_interpreter --locked -- --ignored --nocapture` using portable Clang/lld. Result: 1 passed, 0 failed, 942 filtered, 0.67 s test execution. This loads a real generated DLL with classic nets and O3 plans, verifies native power queries against general C, verifies Graph state/future/effects against Rust, compares world cells and pending work against the interpreter, and tests reset plus eight continuation ticks. Actual classic net/receiver counters are positive and private wire walks are zero. This is a small positive fixed-wire fixture; it does not establish ANPU classic coverage.

Pinned `whole-engine-o3-v8-query-table.exe` contains the native classic port, compact expression lowering, bounded table dispatch and process-local code reuse. Started separate 1,000-tick full verification followed by three alternating 1,000-tick Graph/native pairs. Results are pending at this section.
## 2026-10-16 — Specialized query table experiment: correct, still slower

Pinned `whole-engine-o3-v8-query-table.exe`; report `target/native-query-demo/whole-engine-o3-specialized-1000.json`. Full state/future/effect verification and general-C query comparison passed all 1,000 ticks, followed by three unverified alternating pairs. Frozen per-tick RAM, active/reset checkpoints and 32-tick interpreter continuation matched. Supported nonempty query roots: 71,154 / 94,807; 23,218,750 actual compiled power calls per unverified sample. Unsupported roots execute general C, not Rust. The report's legacy `fallbacks: 0` field means Rust Graph inference fallbacks only; it does not count native general-query calls. The next report names it `rust_graph_fallbacks` explicitly.

Graph inference seconds: `[7.3359718, 7.3035362, 7.3100061]`. Native: `[7.8155414, 7.8782464, 7.9267674]`. Ratio of medians 0.92787x: C was about 7.77% slower. Graph flush `[4.9039710, 5.1346482, 4.8688888]`; native flush `[5.7150220, 5.7034926, 5.8358008]`. Combined ratio of medians 0.90121x: about 10.96% slower. No production switch is justified.

Cold generated-code setup/compile/load: 162.8195917 s, down from the aborted giant-switch compile. C 406,642,874 bytes; DLL 107,489,280 bytes. All three timing instances reused the immutable DLL with independent mutable engines (`compile_cache_hit: true`), and generation/hash/load still cost `[1.8843504, 1.8717403, 1.8836243]` s. The separate verifier's additional general reductions make its compiled-call counter unsuitable for direct comparison with unverified samples.

Next focused experiment: compile live-only guard roots into the same DLL. Callback guards and contact selection still run recursive generic guard interpretation after query specialization. Use a separate strict lowering context: any captured operand anywhere in a root's dependencies declines that root. Such roots retain exact general C captured behavior. Do not reuse the power-query rule (missing capture is false) for callback roots, because callbacks may actually own captures. Reuse existing guard functions where bodies agree, a bounded sparse function table and the same native general/Rust verifier. This is an attempt to remove more interpretation, not an observed speedup yet.
### Live-guard compiler and indexed-capture observation

Added a strict callback-guard lowering context and root dispatch for delivery guards, proxy witnesses, contact variants and residual power-link guards. Only roots whose complete expressions can be lowered without captured operands are supported. Constants use compact tags; dynamic roots use function pointers. Existing general C recursion handles every other root. The verifier compares handled roots against general C even when callbacks carry captures. Added actual `compiled_guard_evaluations` and emitted-target coverage counters. Six Rust native-query tests passed, including a new transitive-captured rejection test. Independent standalone Clang `-Wall -Wextra -Werror` / ctypes selftest with the final 17-u64 counters ABI returned 0.

Read-only hot-path inspection found that `ne_capture_find` unnecessarily scans all captured items after a miss in its complete generation-indexed map. All indexed snapshots are built by `ne_walk_capture_set` from full-site maps; `ne_walk_capture_site` records slot and current generation for every item. Thus a map miss proves absence. Only small one-item snapshots without maps need linear search. Luna is changing indexed misses to return immediately and adding indexed-hit/absent/stale-generation plus nonindexed hit/miss checks. This is a performance correction with unchanged snapshot semantics; final full-sort validation is still required.
### Final candidate preparation and remaining checks

The strict guard-only v9 compile was stopped before producing a DLL (127.40625 Clang CPU seconds) to include the independently confirmed capture lookup correction in the final candidate. No v9 correctness/performance result is claimed. Final source retains indexed misses as O(1) absence and preserves linear lookup for mapless snapshots. The focused native selftest passes after that correction.

The latest native classic integration test passed again with the 17-u64 ABI, query/guard general verification and shared-code/independent-state cache checks (1 passed, 0.63 s). Classic tests now pass 10 focused cases, ordinary Graph 95 passed / 1 ignored, program_graph 13 passed; a pass registration test expected-name list required an additive update for the disabled-by-default classic pass. The unrelated default-O4 symbolic coverage failure and long explicit synthesis test remain recorded in the classic work log.
### Final validation checks completed outside performance timing

After the final capture fix, six native-query tests passed again, the corrected optimization-level/pipeline registration test passed, and the ignored classic/native DLL test passed again (0.65 s) with guard/query verification and cache state-isolation assertions. `cargo check --workspace --locked` also passed, including the server and feature-disabled core. Existing warnings originate in the independently disabled symbolic/instant synthesis work; they were not changed here.

Classic final release evidence is now refreshed in `CLASSIC_REDSTONE_PROGRESS.md`: 6.5703 ms O2 vs 4.9148 ms classic vs 7.1444 ms interpreter, five alternating pairs on the real small fixture. One committed byte and 584 direct-table bytes replace owned wire work; the remaining wire walks are outside the accepted net. Pipeline compile time is 0.2732 ms vs 4.1186 ms, including repeated proof validation. This does not establish ANPU coverage (still zero accepted nets) or isolate default-disabled branch overhead.
### Fixed Boolean guard folding before C generation

The v10 candidate did produce a DLL: source 451,136,648 bytes, DLL 118,535,168 bytes. Source/DLL timestamps place compilation/linking at roughly 505 seconds (not the benchmark's unavailable exact setup field). It completed at least 3,000 full state/future/effect verification ticks with no mismatch. Stopped before full-sort completion or timing to fix the emitter's excessive work on known constants; no v10 performance result is claimed.

Inspection counted 385,113 guard bodies and 142,906 dynamic guard-table entries. Fixed site properties were still represented by functions returning literal zero/one, and larger Boolean guards retained calls to those functions. The emitter now propagates only literal Boolean constants through Not/All/Any and omits constant guard functions. Callback lowering still first checks every dependency: a constant OR/AND does not hide an unsupported captured operand. Dynamic state expressions remain dynamic. Extended the captured-context regression to check literal negation, All/Any identities/decisive constants and conservative captured rejection. This reduces frontend/LLVM work and avoids dynamic calls for roots proved constant; it does not change delivery or snapshot semantics.
### Release snapshot check before benchmarking

The first release pin after the Boolean-folding edit still emitted exactly the previous 451,136,648-byte source / 385,113 guard bodies. Its generated output therefore did not contain the intended folding change, regardless of its distinct executable hash. Stopped that task-owned compile before inference. No result from it is treated as a folded-guard measurement. The latest debug unit suite and native classic DLL test both pass the new literal/captured-context checks. Rebuilding once more with all source files frozen, then checking generated size/function counts before accepting any runtime result. This guards against source edits during an in-flight Cargo build being confused with the binary's actual snapshot.
### Correction: identical source did not establish a stale release snapshot

The fully frozen rebuild also emits exactly 451,136,648 bytes and the same guard counts. The earlier stale-snapshot diagnosis was incorrect. The debug test's fixed component predicates do fold, but the real BubbleSort guards are predominantly dynamic contact/resource/availability predicates. Representative emitted site predicates are conditional profile selections, and the large earlier guard-ID range consists of state tests and compound dynamic conditions. Literal-only folding therefore does not change this fixture's generated C. The distinct release executable hashes and identical generated output are compatible with the emitter edit being present but irrelevant to this graph.

Keep the small literal-folding correction and its passed conservative captured-dependency test, but make no source-size/compile-time/runtime benefit claim for BubbleSort. Let the frozen v12 candidate finish compilation, complete full-sort verification and measure its actual inference. Do not add further emitter changes before the result.
## 2026-10-16 — Full-sort native specialization result: measured inference gain

Pinned `target/native-query-demo/whole-engine-o3-v12-folded-guards.exe` SHA256 `8e196ba4e4f8b3a5117c41efe74bee05573039a36729857bf32b89009b93c8b5`; report `target/native-query-demo/whole-engine-o3-specialized-full-sort.json`.

The complete 10,036-tick verifier passed specialized/general C power and guard comparisons, complete Rust state/future/effect comparison, frozen per-tick RAM, final sorted RAM, active/reset checkpoints and 32-tick interpreter continuation. Verification elapsed 758.0809789 s includes cold compilation/setup and checking, and is not inference timing. Native execution contains the complete queue/callback/Turbo/piston/resource/deferred engine, retained O3 plans, graph-specific queries/live-only guards and the indexed-capture miss correction. No Rust Graph inference fallback occurred.

| Scope | O3 Rust Graph | O3 C / LLVM DLL |
| --- | ---: | ---: |
| Inference, full sort | 71.5397947 s | 58.0017794 s |
| Flush/projection | 48.5434336 s | 55.4645896 s |
| Inference + projection | 120.0832283 s | 113.4663690 s |

This single uncontended full-sort pair shows **1.2334x inference throughput** (18.92% less inference time). Including projection gives **1.0583x** (5.51% less time). It is not a claim of 23% higher whole-server TPS. Both modes used the same release executable, generic x86_64 target and identical O3/classic options; all inference work counters match exactly: 2,344,242 scheduled ticks, 199,211,691 callbacks, 3,882,011 wire walks, 35,541,685 piston samples and 3,295,083 actions. This improves evaluation cost; it does not remove those callbacks/walks or prove LLVM inferred a compact redstone kernel.

Actual native specialization work in the unverified sample: 228,474,159 compiled power queries and 541,571,470 compiled guard evaluations. Supported nonempty query targets 71,154 / 94,807; supported guard roots 142,907 / 544,909 total guard IDs. Unsupported roots, especially captured predicates, retain general C evaluation. Actual retained shortcuts: 149,011,455 wire facts, 405,204 isolated replays, 7,174,222 instant routes / 21,703,305 steps. Dust-path and instant-reset hits remain zero. Classic optimization was enabled in the pipeline, but **BubbleSort accepted zero classic nets**. The separate small positive fixture establishes native classic execution; this CPU speedup must not be attributed to classic-net elimination.

Cold native source generation/compile/load cost **501.813576 s**; generated C **451,136,648 bytes**, DLL **118,535,168 bytes**. Graph preparation/pass cost was 13.390 s Graph / 13.351 s native and is separate. The timing native instance reused its verified DLL in-process; source/hash/load still cost 2.3350472 s. At this one measured inference saving (13.538 s per sort), cold native cost alone amortizes over about 37 full sorts; including projection savings (6.617 s) needs about 76. These are illustrative reuse thresholds, not latency guarantees.

The product integration remains experimental under `native-demo`: Graph owns loading and controlled handoff, C executes inference, native statistics expose real coverage, inspection/flush project live C state, and reset exports complete ordered future work for interpreter continuation. The one-pair gain warrants preserving this path and checking repeatability; cold compilation and projection overhead do not justify switching the ordinary server default.
### Repeatability with verified LLVM object reuse

Three alternating 1,000-tick pairs from the same pinned v12 executable confirm the inference gain. Graph seconds `[7.1644787, 7.1455788, 7.3558651]`; native `[5.9458069, 5.9393814, 6.1001578]`. Ratio of medians **1.20496x** (about 17.01% less inference time). Graph projection `[5.0110618, 4.7737084, 5.1150470]`; native `[5.7055862, 5.8498407, 6.2830767]`. Combined median ratio **1.03277x** (about 3.17% less time). All six samples matched exact work counts, frozen RAM, reset and continuation: 236,132 scheduled ticks, 20,244,727 callbacks, 395,187 wire walks, 3,605,555 samples and 333,899 actions per run.

Raw report: `target/native-query-demo/whole-engine-o3-specialized-1000-repeat.json`, explicitly annotated `native_compilation.mode: verified_object_reuse`. To avoid repeating an 8.4-minute compile, an artifact-only compiler helper reused the **machine-code object**, not simulation results. It accepts only the exact original Clang flags and byte-identical `graph.c`, `native_engine.c` and `.h`, verifies original object SHA256 `3ecac0db8173d2f53fe2d1b629c0cd12a22bd8db8330b6b3911fe13eac7f2236`, then copies that object for normal lld DLL linking. Each benchmark still creates fresh world/engine state and executes all inference. Helper source/executable live only under ignored `target/native-query-demo/verified-object-cache.*`; the guarded-copy and wrong-output rejection checks passed. This is measurement tooling, not a new product cache or a cold-compilation claim. Actual native setup was 4.0665 s first object reuse and 2.3102 / 2.2716 s in-process DLL reuse; the cold reference remains 501.8136 s.

Reproduction using the real compiler (cold first run, warm subsequent in-process samples):

```powershell
$llvm = 'F:/rustrepos/MCHPRS/target/native-query-demo/clang+llvm-23.1.3-x86_64-pc-windows-msvc/bin'
cargo bench -p mchprs_core --bench cpu_bubblesort --features native-demo --locked --no-run
& target/release/deps/cpu_bubblesort-8c97f76c6d7e1cdd.exe --native-engine --verify-native --classic-redstone --graph-tier 3 --iterations 3 --clang "$llvm/clang.exe" --linker "$llvm/lld-link.exe" --output target/native-query-demo/reproduction.json
```

Omit `--ticks` for all 10,036 ticks, or use `--ticks 1000` for the shorter paired protocol. Verification is separate from timing. Generated source, object and DLL paths are recorded in each report; generated programs are not checked into source control.
## 2026-10-16 — Interpreter context exposed projection as the next integration bottleneck

The existing release interpreter benchmark completed all 50,000 ticks, frozen whole-world/RAM/chat/halt checks and final sorted RAM. Active window 10,035 ticks: **68.075619 s**; all 50,000 ticks **68.0769473 s**. Report `target/native-query-demo/interpreter-full-sort-v12-context.json`. It uses the original schematic (including the inert wall sign) and no visual-network flush; World block updates happen inside interpreted ticks. It is a separate context run, not a paired identical-normalization native timing. C inference excludes World projection; C plus per-tick projection is the practical comparison. Thus the measured 113.466 s native tick+projection path is slower than this interpreter run despite its faster 58.002 s core.

Deep integration now targets presentation: `GraphBackend::flush` was selecting and writing every one of BubbleSort's **46,305 contacts** at every full flush, independent of whether their owners changed. These are contacts, not all 522k sites. Existing `validate_contact_guard` already rejects Site observations transitively; accepted contact selectors depend only on piston/resource state through Boolean guards. Luna is indexing those dependencies once, comparing last-projected piston/resource state, queuing only affected contacts with deduplication, and preserving initial full presentation, io-only hidden work, reset/full projection and ascending effective-write order. Duplicate physical positions keep the original full loop conservatively. The dependency walker reuses generation stamps/scratch rather than clearing the whole guard table per contact.

Parent also merged native dirty-bit publication into the existing changed-component copy loop, eliminating a second full node scan. C runtime/header/emitter/ABI and generated programs are unchanged, so subsequent native experiments can reuse the strictly verified O3 object. Full state/future/effect verification and end-to-end timing will be repeated after the projection tests; no projection speedup is claimed at this section.

## 2026-10-16 — Sparse projection: full-sort end-to-end gain

Pinned `target/native-query-demo/whole-engine-o3-v13-sparse-projection.exe`, SHA256 `9badcf8c97855c8394a2b23417b83dcc3fd76d2b4a5cda65ba1a31d26a078d28`. Report `target/native-query-demo/whole-engine-o3-sparse-projection-full-sort.json` explicitly records verified O3 object reuse; the original cold native compilation remains **501.813576 s**. C/header/runtime/generated graph bytes are unchanged. Initial external object validation/copy/link/load took 6.1087991 s; the timing instance reused the DLL in-process, taking 2.2610768 s for generation/hash/load.

All **10,036 ticks** passed complete C/Rust state, future and effects verification, specialized/general C query/guard comparisons, frozen per-tick RAM, final sorted RAM, reset and 32-tick interpreter continuation. Verification took 227.9038173 s including setup/checks, outside timing. Active, reset and continuation checkpoints and both RAM digests also exactly match the previous v12 full-sort report. Work counters match between executors and the earlier full sort: 2,344,242 scheduled ticks, 199,211,691 callbacks, 3,882,011 wire walks, 35,541,685 piston samples and 3,295,083 actions. Classic accepted nets remain zero for this CPU.

| Scope, full 10,036-tick sort | O3 Rust Graph | O3 C / LLVM DLL |
| --- | ---: | ---: |
| Inference | 66.3268698 s | 49.0402049 s |
| Per-tick World projection | 5.4703942 s | 8.2804499 s |
| Inference + projection | 71.7972640 s | 57.3206548 s |

One uncontended paired sample gives **1.35250x core throughput** (26.06% less inference time), **1.25255x including projection** (20.16% less combined time). The shared Graph projection change benefits both paths. Compared with v12's earlier runs, projection drops from 48.5434 to 5.4704 s for Rust and from 55.4646 to 8.2804 s for native; these cross-run reductions are context, not isolated microbenchmarks. Absolute inference times also changed, so attribute the observed result to the complete pinned implementation rather than claiming an isolated effect for every edit. Repeatability checks follow.

Native plus projection is now below the separately measured interpreter active-window 68.075619 s (about 1.188x in this context), but the interpreter's original wall-sign normalization and timing protocol differ; this is not an identical-normalization paired interpreter speedup claim. Compilation, activation, RAM probes, reset, continuation and networking remain outside the combined timing. Graph preparation was 16.8848710 s for the first Graph sample and 13.3323724 s for native, reported separately.

### Projection edge cases and the focused test's oracle lifecycle

Contact selectors are indexed transitively through Not/All/Any to piston/resource state. A last-projected snapshot determines dirty contacts; queue deduplication and ascending contact-ID writes preserve effective order. Initial projection includes all contacts. An io-only flush neither consumes hidden contact work nor advances its snapshots. Resource enable/position/revision and piston availability/head/extended/revision changes are all covered. Unknown dependencies conservatively remain dirty; duplicate positions retain the original full loop. Reset still materializes the complete graph future.

The first focused test constructed invalid manual movement states and then called `project_future` after `reset` had transferred pending motion into World and cleared the graph future. Its missing-live-event assertion did not demonstrate a sparse projection mismatch. The test now keeps a live deferred event until reset and captures the full projection oracle **before** reset. Runtime source was frozen throughout full verification and timing; only the test fixture changed. The corrected focused test and ordinary Graph tests are being rerun before accepting the change.

### Projection validation completed

The corrected focused test passes. The ordinary Graph test namespace passes **96 tests, 0 failures, 2 ignored**. The ignored native classic fixture was run explicitly and passes (0.68 s); its graph contains an accepted classic net and verifies native/Rust/interpreter equivalence, dirty projection, reset/continuation and independent mutable engine state sharing one DLL. All six native query/guard emitter tests pass. Latest `cargo check --workspace --locked` passes with native disabled, including the server. The release native benchmark builds successfully. Targeted `git diff --check` passes; line-ending warnings are informational. No runtime source changed after the pinned v13 executable was built.

The focused contact test checks initial writes, zero writes for unchanged full flushes, io-only preservation, resource retraction/publication, stale actor revisions and pending-motion reset against the independent full projector. Calling full projection on a backend after reset is outside the handoff lifecycle; the test now compares that handoff against its pre-reset full-world snapshot.

The full-sort paired saving is 17.2866649 s for inference and 14.4766092 s including projection. At that one observed combined saving, the original 501.813576 s cold compile amortizes over approximately **35 full sorts**, excluding common graph preparation, activation and any network work. This is a reuse illustration, not a guarantee. Native remains an explicit experimental feature/API; ordinary server defaults were not switched.

### Sparse projection repeatability: three alternating 1,000-tick pairs

Report `target/native-query-demo/whole-engine-o3-sparse-projection-1000-repeat.json` uses the same pinned v13 executable and byte/SHA-verified O3 object. Sample order alternates Graph/native, native/Graph, Graph/native. No task-owned builds or other benchmarks ran during these timed samples.

| Seconds, 1,000 ticks | Pair 1 | Pair 2 | Pair 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Graph inference | 6.3988869 | 6.4797643 | 6.5477762 | 6.4797643 |
| Native inference | 5.0263723 | 4.9723543 | 4.9038103 | 4.9723543 |
| Graph projection | 0.5070952 | 0.5132178 | 0.5317579 | 0.5132178 |
| Native projection | 0.8558651 | 0.8369375 | 0.8341674 | 0.8369375 |
| Graph combined | 6.9059821 | 6.9929821 | 7.0795341 | 6.9929821 |
| Native combined | 5.8822374 | 5.8092918 | 5.7379777 | 5.8092918 |

Ratio of medians: **1.30316x core throughput** (23.26% less inference time), **1.20376x including projection** (16.93% less combined time). All three pairs favor native. All six samples match frozen RAM, active/reset checkpoints and 32-tick interpreter continuation. Exact work remains 236,132 scheduled ticks, 20,244,727 callbacks, 395,187 wire walks, 3,605,555 piston samples and 333,899 actions per sample. Native setup was 3.9838232 s on the first external object reuse and 2.2820845 / 2.2328683 s for process-local DLL reuse. These are not cold LLVM compilation timings.

### Final experiment boundary and next useful attack

The working path is now ordinary graph preparation, optional certified classic-redstone lowering, retained instant/wire plans, stable lowered graph serialization plus graph-specific C power/guard expressions, Clang **-O3**, lld-linked DLL and native inference. The DLL owns scheduling, pending membership, ordered callbacks, wire walks, piston/resource transitions, deferred publication and presentation effects. Rust supplies loading, World presentation, inspection and interpreter handoff. Unsupported expression roots use the general **C** evaluator; no Rust Graph inference fallback occurred. The experiment does not depend on the disabled symbolic compiler.

This establishes a repeatable practical gain for CPU_BubbleSort, but not a production default. Current limits: Windows x86_64 demo toolchain, 451,136,648-byte generated C, 118,535,168-byte DLL, approximately 502-second original cold native setup, and no whole-server/network TPS measurement. The unrelated broad experimental suite's recorded symbolic coverage failure/long synthesis test was not repaired or represented as passing. Ordinary Graph, focused native/ABI/continuation checks, export/viewer checks and the full CPU oracle provide the stated validation boundary.

The largest remaining semantic work is unchanged: almost 200 million callbacks and 3.88 million wire walks per full sort. LLVM optimizes their evaluation but has not inferred their removal. Extend certified classic/instant admission where the complete boundary and ordering proof supports it, then emit those direct operations to C. BubbleSort and ANPU currently accept zero classic nets, so the successful small classic fixture must not be advertised as CPU coverage. A separate compile-cost target is sharing repeated generated guard/query bodies; a separate projection target is avoiding full native-to-Rust dirty/state scans. These require fresh measurements and, if C changes, a newly compiled and fully verified DLL. No further unmeasured optimization was added after the accepted v13 result.

## 2026-10-18 — Exact generated-source size census

Read the actual v13 artifact `target/native-query-demo/whole-graph-24552-0/graph.c` in binary mode and classified every source byte; category sum exactly matches the file's **451,136,648 bytes**. This is a source-size inspection, not a new performance run.

| Category | Bytes |
| --- | ---: |
| Immutable graph table initializers | 350,005,491 |
| Guard function bodies | 38,729,538 |
| Guard forward declarations | 17,604,093 |
| Power-query function bodies | 32,430,952 |
| Power-query forward declarations | 3,125,736 |
| Generated dispatch and other specialization text | 9,239,974 |
| Other graph preamble/ABI text | 864 |

The generator emits **385,113 guard definitions** and **71,155 query definitions**, plus matching forward declarations. Definition count includes dependency helpers and does not equal supported nonempty query/root counts reported by runtime coverage statistics.

Graph data is about 77.6% of the source. Largest tables: `ne_deliveries`, 1,162,483 entries / 115,345,887 bytes; `ne_guards`, 544,909 / 43,914,642 bytes; `ne_sites`, 522,011 / 38,297,752 bytes; `ne_wires`, 41,354 / 27,194,673 bytes; `ne_nodes`, 129,379 / 27,127,185 bytes; `ne_power`, 192,781 / 25,000,443 bytes; `ne_routes`, 226,471 / 24,494,480 bytes. Delivery records describe possible callback routes, not a trace unrolled across benchmark ticks. Each repeats textual designated field names, for example `{.target_site=76586,.guard=46678,.receiver_id=2029,.kind=0,.direction=255,.view=0,.receiver_kind=3}`. This verbose serialization materially inflates source bytes relative to binary data.

The resulting DLL is still exactly 118,535,168 bytes. Reducing initializer verbosity or using binary data for runtime-indexed tables could reduce source/frontend cost without proving simulation work removable; runtime benefit requires its own measurement. Discarding unreachable or unused tables and canonicalizing repeated guard/query bodies are distinct semantic/reachability opportunities. No size reduction or fresh speedup is claimed here.
