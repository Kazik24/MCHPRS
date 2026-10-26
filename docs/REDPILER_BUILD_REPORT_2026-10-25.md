# Redpiler build compilation and throughput report

25 October 2026. Production runtime baseline: `d383f78c`. The changes accompanying this report add benchmark workloads, permit O6 in the existing CPU benchmark, and expose BubbleSort runtime-kernel counters. The experimental compiler changes described below were removed.

All six requested builds compile in the tested configurations. A stronger new ADDER11BIT benchmark exposes an ordered handoff mismatch, so its observed rate is not accepted as a validated result. Their executors differ: compilation can mean a logical device, a stateful event kernel, compiled power queries, or fixed classic propagation. Successful compilation does not imply that an entire CPU has become a Boolean circuit.

## Release throughput

| Build | Median engine TPS | Sample range | Timed game ticks/sample |
| --- | ---: | ---: | ---: |
| Rylax memory | 234,156 | 229,235–236,206 | 1,176 |
| ADDER11BIT | **Not accepted**; ~16,872 observed once | Handoff validation failed | 96 plus trigger work |
| ANPU | 3,004 | 2,912–3,019 | 5,000 |
| CHUNGUS | 3,390 | 3,370–3,463 | 16,384 |
| COUNTER_BASIC | 4,732,384 | 4,683,256–4,772,358 | 60,000 |
| CPU_BUBBLESORT | 430 | 413–445 | 10,036 |
| FPU | 47,898 | 46,557–47,915 | 24,576 |

These are unthrottled **engine game ticks per second**, not server TPS with players, networking, or rendering. Each accepted compiled result is the median of three fresh-world release samples on an AMD Ryzen 9 5950X, Windows, Rust 1.98.1. Builds use O6 and compilation budget 8. Local benchmarks use PLOT_SCALE=4 (256-block plots); production uses PLOT_SCALE=5 (512-block plots). The timing scopes differ intentionally to respect each machine's operating protocol; these figures are not an arithmetic-performance ranking.

- Rylax: seven independent addressed write/read histories, addresses 0 and 2–7, including their declared 24-tick settling phases. Only tick calls are timed. Lever actions, compilation, projection, reads, and handoff are excluded. Address 1 is excluded because the schematic has a documented missing torch; the fixture is not patched.
- ADDER11BIT: four prepared input histories, 0+0, 1+0, 1023+1, and 2047+1. Timing includes each falling trigger and its 24 response ticks. Operand preparation, compilation, projection, reads, and handoff are excluded. This measures response windows, not peak additions per second. The fixture exposes an 11-bit sum; overflow is checked through that sum and native state, rather than an invented carry output.
- ANPU and CHUNGUS: contiguous active `tick_with_world` loops. Loading, compilation, activation, final projection, verification, and native continuation are excluded.
- COUNTER_BASIC: 60,000 active ticks following 24 inactive ticks and 600 active warmup ticks. Only tick calls are timed, with the observed progression checked against a fresh native reference. Each timed sample is short, approximately 13 ms; treat the reported rate as a microbenchmark.
- BubbleSort: a complete 10,036-tick sort. Only `tick_with_world` is timed; per-tick projection, RAM observations, compilation, activation, reset, and continuation are excluded. The harness removes one known wall sign in memory before compilation. This is not a paired speedup against an identically normalized native timing run.
- FPU: 4,096 changing raw input frames, held for six ticks each, with one start edge and no warmup. Timing includes input changes, tick execution, projection/output reads, and retained command effects; compilation is excluded. One logical evaluation executes per frame. This is approximately 7,983 frames/s; the throughput harness checks activity, not IEEE arithmetic correctness.

Raw rates, hashes, and timings are retained in [the measurement summary](../target/build-audit-tps/summary.json). The directory also contains individual reports, logs, and pinned executables. Compile time is substantial: BubbleSort's median is approximately 27 seconds and FPU's approximately 23 seconds; neither is included in TPS.

## What each build executes

| Build | Circuit and current execution | Correctness evidence |
| --- | --- | --- |
| Rylax memory | A BUD memory bank with 64 storage actors. O6 compiles the full graph and power queries; certified storage owners preserve event sampling and delayed publication. Storage remains stateful. | Full-bank Main/Quasi predicate checks; selected-owner sample/accept/publication/busy/handoff tests; benchmark manifest histories against native output traces and exported state/future. |
| ANPU | Pong CPU. All 2,898 piston actors have compiled power queries, with no original-input fallbacks; 967 logical BUD owners activate. General Graph still executes substantial work. | 5,000 ticks match every screen state, 14 screen transition frames, and 1,552 ordered BUD-update ticks. The release benchmark additionally matches final blocks/entities, ordered command output, exported future, and 32 native continuation ticks. |
| CHUNGUS | Classic redstone CPU running Mandelbrot. Whole fixed execution covers 251,485 component nodes and 133,623 wires. It retains ordered propagation and scheduled events. | A bounded 64-tick test checks 65 boundaries against general Graph and an independent interpreter. The release benchmark checks the 16,384-tick endpoint and 32 native continuation ticks. This is not a claim that Mandelbrot finished. |
| COUNTER_BASIC | A 16-bit stateful counter with a six-tick logical cadence. O6 installs a logical device with register and publication state. | Native parity at 260 storage cuts and 259 publication cuts over 1,560 ticks, plus stop/restart checks. The release benchmark matches the native count progression and records 10,000 logical evaluations with zero physical callbacks, wire walks, piston samples/actions, or scheduled ticks during its window. |
| ADDER11BIT | Two active-low operand buses and a separate falling trigger. One stateful event kernel covers 142 actors; physical wire/reset execution remains. | Forty pinned histories and six interrupted-cycle handoffs; a separate seven-vector DAG test; the new response benchmark checks four arithmetic histories but fails ordered handoff validation for 1+0. |
| CPU_BUBBLESORT | CPU with BUD RAM. All 16,561 piston actors have compiled power queries, with no original-input fallbacks; 950 logical BUD owners activate. Event kernels already cover 15,200 actors. | Every RAM sample in the full 10,036-tick sort matches the frozen native trajectory; the last bank sorts to 0–15. The O6 regression also compares 32 interpreted continuation ticks against frozen RAM. |
| FPU | O6 executes a logical frame with three retained native output cells and preserved command outputs. Inputs are stable within each six-tick frame. | The throughput run asserts one evaluation/frame, varying output, zero wire walks, and real command output. Frame/clock regression tests are separate; the timing run alone does not establish arithmetic correctness or arbitrary-input timing equivalence. |

COUNTER_PC is a different build from COUNTER_BASIC. Its O3 contract tests preserve 23 independent controls, 16 output bits, memory, and pending work. The tested runtime owns all 3,388 component nodes through general Graph, with no instant-kernel ownership. Its experimental layer census lowers only 95 actors and reports 349 declined groups: 81 unsupported boundary effects and 268 unproven signal arrivals. No fresh compiled COUNTER_PC TPS is included; older temporary benchmark figures do not have matching source/binary provenance.

## Newly exposed ADDER handoff gap

The prepared 1+0 history reaches the expected arithmetic output, but at handoff tick 56 Graph exports two pending retractions in East-then-North order; native exports North-then-East. Logical tick, scheduled ticks, and motions match. The exact future check remains strict; the events are not sorted to make the test pass.

The differing piston positions are `(43,33,83)` and `(40,34,81)` in the test fixture world. Existing 11-bit active-history tests and the specifically tested kernel interruption cuts passed; those checks did not cover this complete production-O6 response/handoff history. This is an additional coverage gap, not evidence that the previous focused tests failed.

See [the actual and expected futures](../target/build-audit-tps/adder-future-diff.json). Fix the queue-order discrepancy and rerun the response benchmark before accepting an ADDER TPS figure.

## Why BubbleSort remains expensive

The full release sort already executes substantial compiled work:

| Counter | Full-sort total |
| --- | ---: |
| Kernel-owned component nodes | 15,200 |
| Instant-kernel invocations | 1,765,993 |
| Logical whole-device evaluations | 0 |
| Callbacks | 89,396,432 |
| Physical piston samples | 33,432,214 |
| Physical piston actions | 3,289,427 |
| Scheduled component ticks | 2,344,242 |
| Generic wire walks | 0 |
| Stable-logic invocations | 8,677,382 |

Zero generic wire walks does not mean zero wire computation. Existing compiled wire/transition machinery still evaluates values and delivers boundary effects. Likewise, event kernels preserve many sample, acceptance, reset, and publication operations. These counts quantify remaining work; they are not a CPU-time profile attributing a percentage to each operation.

## Greedy partial-compilation experiments

The existing compiler already admits independent regions separately. The remaining all-or-nothing restriction is inside a connected candidate region.

Two bounded fallback experiments tried to split declined connected regions while preserving the original graph's boundary evidence, FIFO rules, and full certification:

1. Isolating private cells accepted 64 additional static proof regions. At O6, every resulting one-actor/no-wire PullWave was superseded by an existing exact event kernel. A full sort still installed 15,200 kernel nodes and executed 1,765,993 invocations. It added compile work without changing execution.
2. Regrouping private cells into connected multi-cell subsets, excluding corridor ownership, accepted no additional executable groups. The experimental all-admitted census remained at 2,354 compiled actors across 2,242 regions and 2,197 lowered actor kernels. These diagnostic O3 counts are not O6 runtime ownership.

Both experiments were removed. There is no claimed BubbleSort speedup from this work. Runtime coverage and invocation assertions remain in the full-sort regression so future compiler changes must demonstrate execution, rather than just a larger cold plan.

A concrete blocker is a 426-cell/384-corridor region. The compiler cannot certify a Main-port weak-power read at `RequestSample`: actor 14830 reads wire node 124567. In the fixture test world's coordinates, the piston is at `(178, 42, 3)` and the wire at `(177, 42, 3)`. These are loader-world coordinates, not coordinates relative to a pasted server schematic.

## Next compiler step

Greedy selection should operate on **certified event-aware regions**. It cannot make an unresolved timing obligation disappear.

First establish an arrival certificate for one active multi-cell region: identify the event that starts it, bind incoming values at the actual sample cuts, retain BUD state and qualifying updates as explicit boundaries, and prove which internal notifications can disappear. Instant actors may fire once until their proven reset; ordinary BUD memory still needs its own sample/accept/commit/publication semantics.

Then attempt the whole candidate, split a declined candidate at explicit boundaries, re-certify each smaller candidate, and greedily select disjoint regions that eliminate useful internal work. Ownership must include actors, resources, wires, and consumed callbacks. Unsupported candidates retain Graph execution, and runtime entry guards preserve queue order and handoff.

The first target should be one active region with an actionable arrival witness, rather than another global selector. Admission, real invocations, reduced work counts, native state/future equivalence, and complete-sort release TPS should all be measured before enabling it.

## Production FPU discrepancy

The player reports approximately 250 TPS on the production server, versus the isolated frame benchmark's 47,898 engine TPS. The benchmark uses the production Compiler and real tick execution, including retained native cells and command outputs; it is not the older direct-DAG benchmark.

Read-only Docker inspection confirms:

- The request was `/rp compile --optimize --io-only --stable-instant-inputs`, budget 8. These options select O6.
- Compilation covered the complete plot, from `(-512,0,-512)` through `(-1,255,-1)`, and completed in approximately 34.65 seconds.
- The compiled plot contains 87,048 graph nodes, 10,155 piston actors, 30,324 wires, and 9,838 instant-kernel-owned component nodes. Kernel ownership alone does not establish whole-frame activation.
- The BUD tables have **0 paired storage/read bindings** and **15 logical BUD owner bindings**. They are different tables.
- The server uses a Ryzen 7 7700, has a four-CPU quota, and reports `nr_throttled=0`. A sampled container load was approximately one CPU core, with memory usage well below its 6 GiB limit.
- Production uses PLOT_SCALE=5. The saved plot has 1,024 chunks; the offline diagnostic used that geometry, then restored local PLOT_SCALE=4.

### Reproduced admission failure

The saved plot reproduces the production graph counts exactly. Its clock is recognized, its green trigger lever at `(-267,86,-177)` is powered/stopped, and all imported queues and scheduled-tick counts are zero. Entry timing and pending work are not the rejection in this replay.

Each of the 16 FPU output wires feeds a terminal repeater configured with delay 1. For example, wire `(-103,55,-221)` has an Always-guarded, zero-attenuation Stored-power link into the repeater Main input at `(-102,55,-221)`. That repeater has no downstream power consumers.

Automatic `frame_power_targets` selects terminal **wires** and command-block inputs. It marks each FPU output wire consumed because the repeater reads it, but does not promote the terminal repeater input to an externally retained boundary. Consequently all 16 arithmetic taps are absent from the attempted frame roots.

The attempted frame contains 4,529 expressions and 16 Power outputs, with **zero ResourceState outputs**. The resource-state admission predicate rejects it. Compilation succeeds through fallback, but the resulting 76,103-expression artifact has no frame timing and `logical_device=false`. The fast executor that produces the 47.9k benchmark is not active.

Explicitly probing the 16 arithmetic output wires instead produces **22,193 expressions, 31 outputs, and 15 ResourceState outputs, with no declines**. The algebra can be compiled; automatic boundary discovery omitted its useful outputs.

A restricted FPU slice contains exactly 8,100 pistons and reproduces the same rejection. The neighboring Divider accounts for the additional circuit scope but is not the cause of this particular omitted-output failure. The slice includes the adjacent repeater column so its boundary is inventoried correctly.

### Implemented fix (26 October)

The compiler now discovers terminal repeater Main inputs and keeps their upstream wires as public frame outputs. The adapter latches computed input strengths at computation edges and uses Graph's native repeater update and scheduled-tick handling. Retained-cell resampling does not change those latched values. Command outputs remain native effects.

Terminal discovery checks callbacks as well as power links. Repeater self-updates are retained; callbacks into frame-published wires cannot overwrite those values with skipped physical state. Repeaters with downstream power consumers, side inputs, or foreign semantic callback receivers are not admitted by this adapter. In particular, two internal FPU repeaters notify piston heads and are not terminal readouts.

Compile completion now states whether the logical-device evaluator activated or whether general Graph execution remains, with the admission reason. Structural kernel counts alone are no longer the only player-visible evidence.

Simply adding repeater Main inputs to the current frame output list is insufficient: `publish_frame_output` directly sets ordinary component state and would bypass repeater scheduling. Removing the ResourceState guard would likewise activate an incomplete computation. Neither shortcut was applied.

The production investigation made no server changes. Temporary offline diagnostic instrumentation and the plot-scale change were removed. Evidence: [production compile log](../target/build-audit-tps/prod-mroww.log), [offline admission and crop](../target/build-audit-tps/prod/offline-admission.log), [exact output links and explicit-root probe](../target/build-audit-tps/prod/offline-admission-links.log).

### Verification and remaining mixed-plot limit

The new release benchmark adds **16 native delay-1 readout repeaters** to the FPU fixture. Across three samples it reaches **42,754 engine TPS median** (42,375–43,394). Each sample runs 4,096 changing input frames / 24,576 game ticks, with exactly 4,096 logical evaluations, zero bulk wire walks, 32 actual command outputs, and observed native repeater publication. Input changes, ticking, projection, and output reads are timed; compilation is excluded. This is activity/throughput validation, not an IEEE arithmetic oracle or live-server TPS claim. [Raw results](../target/build-audit-tps/fpu-native-readouts-after.json).

The same-code original fixture, without added readouts, measures 44,281 engine TPS median in this run. The earlier 47,898 result above is retained as its historical measurement. The new readout benchmark uses `terminal_readout_delay: 1` in a copy of the existing FPU manifest; the production engine needs no extra flag. Pinned readout benchmark SHA256: `1AF1F0908C6366A5CEA579163F9B7DB1924E7D4D03F5E0680A3C685544C8864B`.

The updated normal Redpiler suite passes **699 tests**, with 93 opt-in research tests skipped. Separate opt-in runs also pass:

- FPU with 16 added readouts: actual frame admission, witness inputs changed after compilation, start/stop replay, nonzero wire and native repeater output, final held-input/readout agreement, and zero bulk wire walks.
- Original FPU: existing frame/clock and native command-output regression, including rejection of foreign reset and command-result consumers.
- ANPU: all 14 frozen screen frames and 1,552 ordered BUD updates through 5,000 ticks.
- BubbleSort: every frozen RAM tick across the complete 10,036-tick sort and 32 interpreter handoff ticks.
- CHUNGUS: the default 64-tick Graph/interpreter differential, including its saved block entities.

The read-only saved production replay uses a pinned PLOT_SCALE=5 executable; source PLOT_SCALE is restored to 4. Its **isolated FPU slice activates** (70,474 nodes, 8,100 pistons) and runs without bulk wire walks. The **complete FPU + Divider plot remains general Graph** (87,048 nodes, 10,155 pistons), reporting `no certified logical-device timing`. This is not a validated fast mixed-plot executor. The replay requires either a visible decline or independent-neighbor state/FIFO/scheduler parity if whole-plot activation becomes possible.

Next engine work is certified per-device scope: infer and compile a clock/output island while leaving unrelated devices on ordinary Graph execution. Removing the whole-frame route filter or merging the Divider into the FPU cadence would not establish that proof. The separate ADDER11BIT ordered handoff gap described above also remains open.

Current evidence is in [the final normal suite](../target/build-audit-tps/prod/redpiler-suite-final.log), [saved-plot replay](../target/build-audit-tps/prod/saved-plot-after.log), and the `*-final.log` build replays alongside them. No updated code was deployed to production.

## Independent logical devices (26 October)

Logical-device execution is now represented by a collection of certified scopes rather than a whole-plot switch. The partition follows dynamic power and conducted paths, resources and contacts, callback receivers, guard dependencies, watched sites, and command-chain adjacency. Shared immutable profile facts do not couple unrelated devices. No build names or coordinate rules participate in admission.

Each accepted scope owns its nodes, sites, contacts, resources, and source routes, and has its own source-derived clock. Admission checks pending work within that scope. A declined scope stays on ordinary Graph while other scopes can activate. All active devices and ordinary components use one native scheduler and piston FIFO; owned callback suppression cannot discard unrelated work. Compile completion reports the installed logical-device count.

The saved production replay now admits the FPU in both its isolated crop and the complete **87,048-node / 10,155-piston FPU + Divider plot**. A read-only 48-tick replay compares neighboring component/piston/resource state, piston FIFO, and delayed-work order against ordinary Graph. The isolated FPU still performs no bulk wire walks. This replaces the whole-plot admission limitation documented above; it is not a live-server deployment or TPS measurement. [Replay evidence](../target/build-audit-tps/prod/islands-saved-plot.log).

The architecture deliberately preserves connected dependencies. If two clocks share an interaction island, that island currently remains Graph-owned. Arbitrary connected kernels require narrower ownership and explicit boundary ports describing sampled values, publication phase, native delays, and event order. Those contracts must be established before cutting a live connection. Unsupported observable sinks must force local fallback rather than vanish from the frame roots. Independent admission is a foundation for that composition, not a claim that every connected circuit already lowers to separate kernels.

The new two-COUNTER integration passes with staggered clocks, an active ordinary repeater/lamp, per-tick outside-Graph state and ordered-work parity, stable memory and publication-cut checks, and eight exact interpreter continuation ticks after a stopped drain. Admission tests also confirm that unrelated preexisting scheduled work survives, in-scope pending work prevents activation, and a malformed third controller does not block two valid devices.

This exposed an existing register-machine output bug after stopping: memory settled correctly but the last active output word remained published. The runtime now resamples changed source inputs only when its clock is quiescent, using `SettledMachine::observe` without committing a register. It does not overwrite a result between its computation and delayed physical commit. The existing six-OFF-phase regression, 24-tick drains, and restarts pass.

Frame completeness uses transient source-expansion support from the existing equation compiler, together with native retained power boundaries. It does not assume that an unlisted sink disappeared. Unsupported native effects remain a reason for local fallback; constant comparator inputs are accepted only when the shared comparator rules prove no state transition from the saved entry. Final FPU completeness and release-throughput checks are in progress. No production deployment has been made.

## Reproducing the measurements

Run from `crates/core` so the expected configuration is loaded. The commands use existing benchmark entry points; `--output` paths below are resolved by those harnesses.

```powershell
cargo bench -p mchprs_core --locked --bench classic_baseline -- --cpu anpu --graph-tier 6 --ticks 5000 --iterations 3 --output target/build-audit-tps/anpu-current.json
cargo bench -p mchprs_core --locked --bench classic_baseline -- --cpu chungus --graph-tier 6 --ticks 16384 --iterations 3 --output target/build-audit-tps/chungus-current.json
cargo bench -p mchprs_core --locked --bench cpu_bubblesort -- --graph-tier 6 --iterations 3 --output ../../target/build-audit-tps/bubblesort-before.json
cargo bench -p mchprs_core --locked --bench instant -- --component fpu_legal --device-frame --tier 6 --episodes 4096 --iterations 3 --output target/build-audit-tps/fpu-current.json
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --interpreted --ticks 60000 --iterations 1 --output target/build-audit-tps/counter-native.json
cargo bench -p mchprs_core --locked --bench instant -- --component counter_basic --tier 6 --ticks 60000 --iterations 3 --reference target/build-audit-tps/counter-native.json --output target/build-audit-tps/counter-current.json
cargo bench -p mchprs_core --locked --bench instant -- --graph-short --all-devices --component research/rilax_memory_bank_bud --graph-tier 6 --iterations 3 --output target/build-audit-tps/rilax-current.json
cargo bench -p mchprs_core --locked --bench instant -- --graph-short --all-devices --component io/adder_11bits --graph-tier 6 --stable-instant-inputs --iterations 3 --output target/build-audit-tps/adder-current.json
```

The benchmarks run sequentially. No release TPS is derived from a unit-test wall time. Active Graph display can differ from native moving-contact serialization; the memory/adder benchmarks compare observed traces while active and materialized state plus pending work after handoff.

