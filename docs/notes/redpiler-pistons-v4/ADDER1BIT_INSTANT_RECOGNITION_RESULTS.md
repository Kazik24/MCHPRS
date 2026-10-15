# ADDER1BIT instant recognition implementation

Status: all 13 actors recognized and their fixed-conductor input reads specialized. A follow-up now compiles eligible reset observers into built-in reset clocks; see the final section. Full actor/resource lifecycle proxies remain unimplemented.

Read [the source catalogue and mechanism map](ADDER1BIT_INSTANT_RECOGNITION_RESEARCH.md) first. It gathers the model, current and historical instant research, previous attempts and the pinned adder evidence; older notes now live under `scraps/docs/redpiler-pistons-v4/`.

## Corrected recognition

The previous `simplify-instant-resets` matcher recognizes one private redstone-token/two-wire output family. Its conditions are appropriate for that exact output kernel but incorrectly narrow as a definition of an instant mechanism.

The new O3 pass, `specialize-instant-pistons`, follows a concrete reset path:

```text
observer watching a piston base
    -> live correctly oriented Strong output
    -> immutable solid conductor (or an already expanded input)
    -> actor Quasi input
```

It verifies both base-change and base-restoration callbacks to the observer. It retains the observer's watched actor separately from the actor receiving its reset power. Resources remain shared and authoritative; it requires neither a private redstone payload, a horizontal actor, an individual observer, singleton inputs nor two forward wires. Production recognition uses graph references, not filenames, hashes, positions or numeric actor IDs.

The adder's coverage is:

| Reset observer | Actor(s) supplied |
|---|---|
| n63 | P0 |
| n69 | P1, P7, P9 |
| n71 | P2, P8, P10 |
| n72 | P3 |
| n77 | P4 |
| n80 | P5 |
| n90 | P6 |
| n98 | P11 |
| n101 | P12 |

Thus all 13 actors are recognized: nine own-base reset mechanisms and four using shared reset drivers. The three shared payload groups remain `{P1,P4}`, `{P2,P5}`, and `{P0,P10}`.

## Executable change and proof

For recognized actors, expand eligible fixed-conductor Main/QC reads into their original contributors. Keep every source port, target input port, original query direction, attenuation and guard. Multiple positive contributors remain a maximum, including the extra QC controls and P12's multiple Main branches. Mutable contacts remain live source reads rather than being frozen into constants.

For strengths in 0..15, saturating attenuation distributes over max. An outer guarded conducted query therefore equals the max of all contributors with the two guards conjoined and the attenuations added, clamped at 15. Guard/source reads are pure, and no circuit event runs inside a power query. The expressions consequently return the same value at every query boundary for every state. Since no state or event ordering is changed, the equivalence extends by induction over ordinary execution; it does not depend on an arithmetic input protocol.

In the checked O3 ADDER1BIT export, conducted piston-input occurrences fall from 29 to 1. The remaining occurrence is a mutable-contact read and stays general. Total power links grow from 192 to 199 because multi-source maxima are represented explicitly. These are static table counts, not measured runtime visits.

The pass does not remove piston/observer handlers, callbacks, pending requests, imported ticks, live acceptance, shared-resource arbitration or deferred publication. The separate private output kernel retains its own certificate. A shared high-fan-in expansion can at most double the retained power table; further occurrences stay general. Recursive conductor inputs also stay general. Cancellation is checked before atomic publication, and applying the pass again is inert.

Recognition identifies observer-reset provenance. It is not a certificate that a whole reset circuit may be erased, nor a promise that every input history produces an accepted response. Absorbing lifecycle work remains a separate proof and implementation step.

## Validation

The recognition test checks exactly 13/13 actors and 13 actual input rewrites, four rotations, translation, the four borrowed reset drivers, retained callback/resource tables, idempotence and lowering. Negative tests sever a change callback or reverse observer power direction. Cancellation/disabled cases preserve the graph. A shared high-fan-in case verifies bounded table growth.

The backend equivalence test compares actual original and rewritten Main/QC queries for all actors over 1,024 strength/guard/contact snapshots (26,624 channel comparisons). It includes independently guarded additional contributions, nonzero attenuation, changed resource contacts/enabled state and mismatched saved powered/output bits. This evaluates the real program, not a duplicate algebra implementation.

Existing differential tests run the new pass with the previous exact instant pass: all one-bit arithmetic inputs, 40 eleven-bit vectors, inhibition/cancellation, input reversal, held controls, rearm, rotations/translations and reset continuation. Private output kernel tests still exercise their own default-O3 preparation and lowering.

The checked-in ADDER1BIT O3 JSON was regenerated to represent the expanded power channels. O0/O1/O2 graph contents remain unchanged. No schema extension, backend field, runtime simulator, Motion/progress state or dependency was added for this pass.

Runtime benchmarks below measure this change. Recognition/power normalization, exact private output transfer and lifecycle absorption are separate coverage counts; 13 recognized actors must not be reported as 13 fully absorbed proxies.


Final checks:

- Redpiler debug suite before the last growth-budget test: 300 passed, 7 ignored; the final four pass tests and real channel-equivalence test also pass in debug.
- Final Redpiler release suite: 301 passed, 7 ignored.
- Broader instant/reference/native tests: 74 passed, 2 ignored before the final test refinements; final release Redpiler coverage includes the refined runtime equivalence and growth-bound tests.
- `cargo check -p mchprs_core --tests --benches --locked`, focused rustfmt checks and `git diff --check` pass.
- ADDER1BIT O3 JSON was regenerated and its golden test passes; its recognition-only checkpoint SHA-256 was `db42327b6ecee1c9ba493fe7deff128abef694846e6875da2e0f7a4e426c38f8`.

Not checked: full workspace tests, broad fuzz campaigns or full lifecycle contraction. The unsafe fixed-point experiment is independent and remains off by default. No commit was made.

## Release speed and Divider correctness (2026-10-14)

Built the current default-feature release benchmark, without `unsafe-turbo-fixed-point`, graph profiling or test trace collection. The same pinned executable ran all workloads sequentially. Each of five samples uses fresh worlds and alternates executor order. Compilation, loading, publication, observation and continuation checks are outside the timer. The adder protocol additionally times its 64 control toggles; each adder has 512 timed ticks. CPU has 100 timed active ticks and the identical approved in-memory sign removal on both inputs. Divider has four 128-tick response/reset phases after preparation.

Times below are seconds: median [minimum, maximum]. Paired change is the median of `(O3 / O2 - 1)` for corresponding samples; positive means slower. The different workload windows must not be compared directly.

| Workload | Active ticks | O2 | O3 | Interpreter | Paired change |
|---|---:|---:|---:|---:|---:|
| adder1bit | 512 | 0.010503 [0.009754, 0.010729] | 0.010236 [0.009137, 0.010396] | 0.008200 [0.007448, 0.008564] | -4.29% |
| adder11bit | 512 | 0.078817 [0.075363, 0.082311] | 0.075702 [0.071701, 0.084097] | 0.055210 [0.052201, 0.056641] | +0.45% |
| divider | 512 | 0.797812 [0.749478, 0.894052] | 0.740382 [0.548189, 0.847619] | 0.467465 [0.372484, 0.491624] | -5.19% |
| cpu | 100 | 1.375816 [1.277252, 1.497372] | 1.432013 [1.187660, 1.518090] | 0.956124 [0.898085, 1.028846] | +0.84% |

All five samples of each workload passed the existing interpreter comparison: observations at every tick, chat, projected reset blocks/entities, pending future work, and 32-tick continuation. Each Graph tier also passed a first-tick reset-and-native-continuation check. O2 and O3 retained identical semantic work counts for every workload. CPU retained 20,005 scheduled ticks, 1,760,070 callbacks, 32,505 wire walks, 302,784 piston samples and 27,884 actions/publications per 100 ticks.

The timings do not establish a general speedup. ADDER1BIT's paired median is lower, but ADDER11BIT and CPU have mixed paired results and no reliable benefit. Divider's O3 median is 7.20% lower than O2, with a 5.19% lower paired median; four of five pairs favored O3, but the ranges overlap and one pair regressed. This is an incremental query optimization, not event-work elimination. CPU O3 remains about 1.50 times slower than the interpreter in this run.

Coverage separates recognition from executable rewrites:

| Workload | Total pistons | Recognized reset mechanisms | Actors with new query rewrites | Old private reset cells |
|---|---:|---:|---:|---:|
| ADDER1BIT | 13 | 13 | 13 | 0 |
| ADDER11BIT | 142 | 142 | 142 | 0 |
| Divider | 2,055 | 1,389 | 1,351 | 38 |
| CPU_BubbleSort | 16,561 | 10,283 | 9,876 | 407 |

The old private reset pass already expands some inputs, so fewer actors need the new rewrite. Its certified output-pair count is zero for all four workloads. O2/O3 directly isolates the new pass on the adders; on Divider and CPU the comparison includes both O3 passes. Divider still has 666 pistons outside current recognition; successful replay is not proof of complete instant recognition. No full lifecycle proxy is implemented.

### Additional Divider input history

Ran the same pinned Divider fixture through the existing generic fixture protocol (`research/fpu_divider`): 64 round-robin toggles across its existing levers and 512 ticks. O2, O3 and the interpreter agreed on all 513 electrical snapshots, including 104 distinct snapshots, and passed pending reset, final projection and continuation checks. O2/O3 work was identical: 594,961 callbacks, 174,385 piston samples and 10,196 actions/publications. This is one correctness episode, not a timing series or exhaustive arithmetic validation.

The dedicated five-sample Divider protocol also checked that each OFF phase produces raw output 170 and each ON phase returns to zero; all observed output words were 0 or 170. Its work per sample was identical between O2/O3: 822,608 callbacks, 14,380 wire walks, 247,532 piston samples and 13,572 actions/publications. The manifest does not independently certify arithmetic scaling, so these checks establish interpreter parity for these histories rather than a mathematical division specification.

### Reproduction and artifacts

```powershell
cargo bench -p mchprs_core --bench cpus --locked --no-run
Copy-Item target/release/deps/cpus-6536be3d5ba1e0dd.exe target/adder-instant-bench/cpus.exe
# Run these separately, with no competing benchmark process.
target/adder-instant-bench/cpus.exe --graph-short --all-devices --cpu io/adder_1bit --iterations 5 --output target/adder-instant-bench/adder1bit.json
target/adder-instant-bench/cpus.exe --graph-short --all-devices --cpu io/adder_11bits --iterations 5 --output target/adder-instant-bench/adder11bit.json
target/adder-instant-bench/cpus.exe --graph-short --cpu cpu_bubblesort --iterations 5 --output target/adder-instant-bench/cpu.json
target/adder-instant-bench/cpus.exe --graph-short --cpu fpu_divider --iterations 5 --output target/adder-instant-bench/divider.json
target/adder-instant-bench/cpus.exe --graph-short --all-devices --cpu research/fpu_divider --iterations 1 --output target/adder-instant-bench/divider-input-history.json
```

Raw reports and command logs are under `target/adder-instant-bench/`; `summary.json` contains compact timing and work-count results. Cargo's executable hash can differ on another build; use the path printed by `--no-run`. The report's top-level timer description predates generic control timing; the adder/input-history protocol above states the actual timed scope.

No Rust implementation changes were needed for this measurement. Not checked in this batch: full CPU sorting, ANPU/Counter/FPU replay, exhaustive Divider arithmetic vectors, fresh broad test suites or sampling profiles. The source and fixture files remain unchanged by these benchmark runs.

## Reset deletion experiment (2026-10-14)

Attempted reset-notification erasure in a test-only transformation after the full O3 pipeline. It discovers all 13 actors and their nine distinct reset observers, deletes 34 route rows targeting those observers (including implicit receiver bindings), and removes corresponding update links. Both the unchanged control and the modified graph use linear Turbo callback selection, so stale route offsets or different callback-selection algorithms cannot explain the result. Both graphs remain structurally valid. The source schematic, production passes and runtime are unchanged.

The experiment replays all eight pinned prepared operand cases and 32 response ticks. At every operation the unchanged O3 control matches the independent interpreter, including projected cells, entities, scheduling and piston future. The reset-erased variant is expected to diverge; this is a passing counterexample regression, not a passing optimization.

| Operand case | Pending work differs | Actor/resource state differs | Sum-payload cell differs |
|---|---|---|---|
| 000 | No | No | No |
| 001 | Yes | Yes | Yes |
| 010 | Yes | Yes | Yes |
| 011 | Yes | Yes | No in this window |
| 100 | Yes | Yes | Yes |
| 101 | Yes | Yes | No in this window |
| 110 | Yes | Yes | Yes |
| 111 | Yes | Yes | Yes |

For the seven active cases, scheduled work first differs at the first response tick and actor/resource state at the fourth response tick. For 110, the fourth-tick sum cell is a redstone block in the control but Air after erasure. For 111, the seventh-tick control has Air while erasure has a redstone block. Other first cell differences include a missing in-progress restoration. These demonstrate changed recovery/pulse behavior; they must not be reported as a claim that every first arithmetic sample is wrong. Cases 011 and 101 also show why matching the sum alone cannot justify deleting reset behavior.

The regression is `deleting_recognized_adder_reset_notifications_changes_actor_state_and_future_work` in `backend/graph/tests/reset_elision.rs`, registered under the existing adder test module. The mutation helper is test-only; there is no enabled erasure pass or unsafe default. This attempt does not implement a compact equivalent reset machine, remove reset behavior in production, or establish a speedup.

A safe follow-up must replace the physical reset implementation while retaining its logical transitions: pending reset membership, scheduler insertion order, ON/OFF power changes, rearm and interrupted restoration. The shared observers must remain one shared reset authority (for example, the first junction's observer supplies P1, P7 and P9). Keep action requests in the global FIFO and deferred publication in its original phase. Any circuit operations absorbed into that reset machine still need exact projection and handoff to the interpreter. The negative control above is a required rejection test for a future implementation that accidentally drops those transitions.

Validation commands and logs:

```powershell
cargo test -p mchprs_core --lib --locked redpiler::backend::graph::adder_tests:: -- --test-threads=1 --nocapture
cargo test -p mchprs_core --lib --release --locked redpiler::backend::graph::adder_tests:: -- --test-threads=1
```

Debug and release each passed 10 tests with 1 ignored on the final test source. This includes the new negative-control regression and the existing arithmetic, ordering, inhibition, rotation and continuation checks. Logs are under `target/adder-instant-bench/reset-erasure-adder-*.log`. Focused rustfmt and `git diff --check` also pass. No benchmark is claimed for a variant already disproven by correctness checks.

## Built-in reset replacement (2026-10-14)

This follow-up replaces reset execution, rather than deleting reset behavior. The O3 pass `compile-builtin-resets` discovers the existing reset drivers, requires a matching raw observer orientation and an empty outgoing callback route, and rewrites eligible nodes to `BuiltinReset { watched, facing }`. ADDER1BIT uses nine shared clocks for thirteen actors. A clock retains its original node ID, component state and pending scheduler identity; there is no duplicate payload owner, private scheduler, physical Motion model or hidden interpreter.

A directional or captured notification qualifies exactly as before. The built-in arm operation calls the existing deduplicating scheduler directly, avoiding the ordinary observer handler's additional pending lookup. A due clock transition toggles powered/output state; on the rising edge it first schedules the falling transition in its original insertion order. The formerly empty output route is unbound, so no output callback circuit is dispatched. Imported entries still dispatch in place, including duplicate, untyped and stale work. Reset/flush projects the original observer block and scheduled work from the same state.

Eligible clock emission links become `PowerPort::Output` at their original source site, with target, direction, guard and attenuation unchanged. Those typed fixed-component value reads now access stored scalar state directly without constructing a block or resolving its properties. Dynamic contacts and directional weak/strong queries retain the general path. Thus no clock-specific lookup is inserted into unrelated power reads.

The rewrite is an exact local transition replacement, not a whole instant-region contraction. Input notifications, ON/OFF obligations, piston requests, accepted actions, resource publication and connecting dust still execute. Callback/scheduler counts are retained honestly. Ordinary observers with outward callbacks or incompatible raw states stay general; cancellation leaves the input graph untouched. The existing private two-wire certificate also remains valid with the built-in clock.

Coverage includes one shared clock per driver, idempotence, cancellation, output-route/raw-state rejection, 3,360 combinations of all four-bit stored strengths, powered bits, all directional/None notifications, captured powered operands, and five imported-work families (empty, immediate NanoTick, duplicate high-priority ticks, stale type and untyped target). Every step checks component/resource/actor state, power inputs, dirty flags, pending membership and ordered scheduled work against the unmodified graph. All directional output ports are checked too. Existing adder arithmetic, reversal/cancellation, held-input, rearm, rotations, inhibition and reset/continue tests now run the built-in reset path. The deletion counterexample remains as a negative control.

JSON retains the observer's projected identity and adds `builtin_reset: true`, with no output route. Rust validation, the Python loader and the HTML viewer accept the additive marker and reject malformed clock bindings; the viewer labels these nodes as built-in resets. O0/O1/O2 exports omit the marker. The ADDER1BIT O3 golden export was regenerated.

Final source verification: Redpiler debug and release each passed **304 tests with 7 ignored**, including the enlarged 3,360-case matrix, all adder arithmetic/rearm/inhibition/rotation histories, the private output kernel and native reset-continuation checks. `cargo check -p mchprs_core --tests --benches --locked` and `cargo check -p mroww --locked` passed. The loader and viewer checks are recorded in `target/builtin-reset-bench/loader-tests.log` and `viewer-tests.log`. The ADDER1BIT O3 export now contains nine marked clocks and 22 direct clock-output links; O0/O1/O2 contain no marked clocks. Its SHA-256 is `626d053cb15233d02f966746e99f5cce02f7c38403af7ad93e90f1859b4aa9ae`.

### Release measurement of the replacement batch

Compared the pinned previous normal O3 executable (`target/adder-instant-bench/cpus.exe`) with the new normal O3 executable (`target/builtin-reset-bench/cpus.exe`). The previous binary's SHA-256 matches the successful earlier CPU report. Both were built in the same optimized release profile, with the same harness and no unsafe fixed-point feature or test trace collection. Five pairs alternate which binary runs first. Each executable invocation uses `--graph-short --graph-tier 3 --iterations 1`, a fresh input world, its own interpreter check and first-tick reset continuation check. Compilation, projection and correctness checks remain outside the timer; the adder episodes include their 64 control toggles and 512 ticks, while CPU uses 100 active ticks. CPU applies the same approved in-memory sign removal to both inputs.

Times are median [minimum, maximum] seconds. Paired change is the median of `(new / previous - 1)` across the five pairs; positive means slower.

| Workload | Built-in clocks | Previous O3 | New O3 | Paired change |
|---|---:|---:|---:|---:|
| adder1bit | 9 | 0.010526 [0.009751, 0.010819] | 0.010642 [0.010178, 0.011592] | +5.71% |
| adder11bit | 98 | 0.077173 [0.074561, 0.078184] | 0.075718 [0.073570, 0.076500] | -0.87% |
| cpu | 7,093 | 1.110885 [1.085505, 1.255290] | 1.114779 [1.085168, 1.154614] | +2.18% |

There is **no demonstrated speedup**. CPU medians differ by +0.35%, with overlapping ranges; its median paired change is +2.18%, and the five pairs are mixed. ADDER1BIT has a +1.10% ratio of medians and regresses in four of five short pairs; ADDER11BIT has a -1.89% ratio of medians with mixed pairs. These measurements do not justify describing reset compilation as an acceleration. The batch includes both built-in reset lowering and the shared fixed-component value-read path, so it does not isolate the clock change alone.

Every old/new pair matched interpreter observations, projected checkpoints and continuation. Cross-binary checks additionally compared all declared trace/chat/future fields, block/entity/chat checkpoints and complete semantic work counters. CPU retains 20,005 scheduled ticks, 1,760,070 callbacks, 32,505 wire walks, 302,784 piston samples and 27,884 actions/publications. Thus this replacement establishes a simpler executable reset primitive but has not removed the dominant event workload.

The new O3 Divider replay also passed its four response/reset phases (512 ticks), expected output words 0/170, first-tick reset and 32-tick continuation, with 1,110 built-in clocks. It retained 822,608 callbacks, 14,380 wire walks, 247,532 piston samples and 13,572 actions/publications. This single episode is correctness evidence, not a before/after Divider speed comparison.

Individual JSON reports and logs are `target/builtin-reset-bench/{adder1bit,adder11bit,cpu}-{baseline,builtin}-{1..5}.{json,log}`; compact measurements are in `summary.json`. `divider-correctness.json` contains the additional Divider replay. Reports capture the current worktree as metadata even when executing the pinned previous binary; binary hashes distinguish the compiled versions. No source schematics were modified, no dependency was added and no commit was made.

Remaining work: eliminate or compile internal wiring and callback delivery within certified instant regions while retaining shared reset clocks, live acceptance and boundary order. Not checked in this batch: a complete CPU sort, full workspace tests, new broad fuzz campaigns or sampled CPU attribution. Ingress/scheduler work and outward-observed reset sources remain general; this is not a complete instant lifecycle proxy.


## Connecting dust: first exact transfer lowering (2026-10-14)

The follow-up extends the existing wire-query pass with compiled live/captured maximum expressions for connecting dust. All 32 ADDER1BIT wires receive a plan, including payload-dependent geometry. This avoids the old private two-wire grammar without erasing intermediate writes or boundary notifications. It is not a complete region/lifecycle proxy; detailed proof, validation, and release results are in [COMPILED_WIRE_TRANSFERS.md](COMPILED_WIRE_TRANSFERS.md).


## Follow-up: explicit regions and executable private-cell proxies

The next batch adds ownership/event boundaries for all 13 ADDER1BIT actors and executable private-cell proxies for P3, P6, P11 and P12. Shared actors form two joint regions and keep general execution. The proxy replaces internal reset/self callback dispatch with direct ordered instructions, and removes certified-empty foreign occupancy searches; connecting wire walks and the global acceptance/publication lifecycle remain. Earlier measurements in this file predate this batch. See [INSTANT_REGIONS_AND_PROXIES.md](INSTANT_REGIONS_AND_PROXIES.md) for its contract, proof limits, tests and measurements.
