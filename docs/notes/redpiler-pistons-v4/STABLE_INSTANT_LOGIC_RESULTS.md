# Stable-input logic: CPU_BUBBLESORT, 2026-10-20

Latest follow-up: [minimized transition programs](INSTANT_TRANSITION_PROGRAMS.md#instruction-and-entry-guard-minimization)
remove 88.65% of guarded-transport wire visits. Five new pairs show 7.09% less
time than the initial transition program; current host TPS is 211.29 -> 227.45.
The unchanged reference is slower under the current background load, so these
absolute numbers should not be compared with historical runs below.

The new `compile-stable-instant-logic` pass is correct on the complete pinned
BubbleSort run and measures **326.07 median inference TPS**. The whole-circuit
replacement goal remains open.

## Production measurements

Production release, opt-level 3, fat LTO; O5 with `--stable-instant-inputs`;
10,036 ticks per sample, three samples, no competing build or correctness run.

| Sample | Inference seconds | Inference TPS | Compile seconds | Projection seconds |
|---|---:|---:|---:|---:|
| 1 | 30.7127392 | 326.77 | 10.596 | 5.153 |
| 2 | 30.7782684 | 326.07 | 10.941 | 5.223 |
| 3 | 30.8743187 | 325.06 | 10.757 | 5.247 |

Inference measures `tick_with_world` only. Preparation, activation, World
projection, RAM probes, reset and continuation are excluded. Median TPS is
8.69% above the user-accepted 300 TPS baseline. This comparison is descriptive:
it is not a fresh alternating pass-enabled/pass-disabled experiment and does
not isolate the new pass from the intervening typed-drive/channel changes.

Each production sample matched the frozen interpreter RAM trace (10,036
samples, 415 changes, SHA256
`92a5a7fb51b64681e36b72e481593e9b49b1f9df168590b9a27a58b0b6f333d4`)
and final sorted words. Raw report:
`target/stable-instant-logic/production-enabled.json`; executable/source
provenance: `target/stable-instant-logic/production-provenance.json`.
Frozen executable SHA256:
`A464D786A0465F29AA3CF06F90973BE7F1C5C54782AC8B1F35D534DF994E1951`.

## Correctness and representation

The pass is enabled only under the explicit stable-input contract: external
inputs remain fixed from triggering change through reset. It compiles weighted
source expressions rather than reproducing internal physical callback ordering.
It still samples internal resource state live at successive invocations.

Coverage is 2,138 nets, 17,426 of 41,354 wires (42.14%), 8,772 shared typed
inputs, 18,925 terms and 26,780 possible boundary output sites. Physical wire
values remain materialized for remaining consumers and interpreter handoff.

Checks passed: 574 regression tests, all forty pinned Adder11 output windows,
and a complete native CPU comparison of all 768 RAM bits/readiness every tick,
command output, final sort and sixteen interpreter continuation ticks. The full
comparison recorded zero stable-entry fallbacks. Its interleaved diagnostic
timings are not the production TPS above. Evidence:
`target/stable-instant-logic/cpu-10036-boundaries.json` and
`target/stable-logic-full-cpu-v2.log`.

## Remaining cost

Against the same current exact pipeline with only stable logic disabled, the
full correctness run reduced callbacks from 200,945,830 to 174,026,524.
However, 2,009,418 general wire walks remain. Stable logic itself performed
8,677,382 invocations and 109,252,352 wire-expression evaluations. Entry checks
already stop after one expression when the root does not change; changed roots
still evaluate the full net and scan its boundary sites.

The incremental implementation now compiles input-to-expression and
wire-to-output dependency indices. It samples all current inputs on a changed
entry, compares them with the last committed snapshot, and recomputes only
affected expressions. Wire writes, classic overlays, reset and incompatible
captures invalidate the derived cache; imports start inactive. Source changes
are detected by fresh sampling and comparison, rather than invalidating the
snapshot that describes the old materialized wires. Output admission and
commit order are preserved.

The complete 10,036-tick native CPU check passed again, including all 768 RAM
bits/readiness, final sort and sixteen continuation ticks. Expression
evaluations fell from 109,252,352 to 31,205,330 (71.44% fewer); all other
execution counts stayed equal to the previous stable implementation. Focused
tests compare cached execution against forced full solves and verify that a
32-wire fixture updates only fifteen expressions for one endpoint change.
The broader run passed 588 tests; two stale UI expectations for O5 suggestions
and default classic-pass enablement were corrected and passed separately.
Five alternating production pairs are complete. All active/reset/continuation
checkpoints, RAM hashes and operation counts other than expression evaluations
matched between executables.

| Pair | Previous stable TPS | Incremental TPS | Inference time reduction |
|---|---:|---:|---:|
| 1 | 302.34 | 296.96 | -1.81% |
| 2 | 304.84 | 307.76 | 0.95% |
| 3 | 320.57 | 322.58 | 0.62% |
| 4 | 329.21 | 332.18 | 0.90% |
| 5 | 320.21 | 321.27 | 0.33% |

Median inference TPS is **320.21 previous / 321.27 incremental**. Median paired
time reduction is **0.62%**, with one pair slower; this is not a demonstrated
material performance gain. The new representation does less expression work,
but propagation and callbacks still dominate. Do not combine this paired
comparison with the earlier unpaired 326.07 TPS median as a cumulative speedup.
The incremental median is 7.09% above the user-accepted 300 TPS reference.

Raw pairs: `target/stable-instant-logic/incremental-pairs.json`, with each full
run retained as `paired-N-baseline.json` / `paired-N-incremental.json`.
Incremental executable/source provenance:
`target/stable-instant-logic/production-incremental-provenance.json`;
executable SHA256
`9CB16F28FD6291B7742EA77AEE054F97E1039D10B8A2B0354BCE36D3E8A18E48`.

Beyond this, replace physical actor notification routes with compiled state
dependencies; joining the existing actors into one container alone does not
remove the remaining propagation.

The pre-extension rejection census (`target/instant-drives/cpu-transport-rejections-before-guarded.json`)
matches the installed compiler's rejection counts and identifies the next
coverage limit: 113 corridors / 17,736 wires have actual conditional Turbo
inputs, including one 13,340-wire corridor. None fail only the blanket geometry
check. Another 106 corridors / 3,725 wires fail receiver admission; their first
witnesses are 105 repeater corridors / 2,719 wires and one torch corridor /
1,006 wires. These are first rejection witnesses, not proofs that removing
one restriction would admit every corridor. The 2,466 remaining singleton
corridors account for 2,466 wires. One peripheral wire lies outside the catalog.

No new FPU or randomized-input performance claim is made by this report.

## Fresh inference CPU profile after incremental execution

The current O5 + stable-input implementation was rebuilt with the existing
`profiling` profile: release optimization, fat LTO, full debug symbols. A fresh
1,000 Hz ETW capture covers one complete 10,036-tick sort. The capture has zero
lost events/buffers, and its active/reset/continuation checkpoints, RAM hashes
and execution counts match the unprofiled incremental production executable.
All 8,978 distinct application addresses resolve against the matching PDB;
3,896 include inline chains.

Inference is selected by the benchmark callsite at `cpu_bubblesort.rs:177`,
excluding compilation, World projection, RAM probes and native continuation.
There are 35,055 positive-CPU inference samples representing 32.750 sampled
thread CPU seconds. The profiled 35.152 seconds of inference wall time is not a
new throughput result. Projection separately consumed 6.003 wall seconds;
preparation consumed 11.493 wall seconds.

| Inference path | Sampled CPU share |
|---|---:|
| General wire propagation | **52.47%** |
| Compiled stable-logic execution, including its boundary delivery | **14.26%** |
| Neighborhood capture/discovery | **30.71%** |
| General guard evaluation | **14.54%** |
| Stable expression evaluation and input sampling | 5.07% |
| Typed drive-condition evaluation | 3.98% |
| Packed piston predicates | 4.82% |
| Scheduler helpers | 0.50% |
| Deferred resource-extension search | 0.27% |

These rows are inclusive and generally overlap: capture and guard evaluation
can happen inside propagation. General propagation and stable logic have no
overlap in this capture; their union is **66.74%** of inference CPU. The stable
program's 14.26% includes output capture/admission/dispatch and bookkeeping,
not just arithmetic on compiled expressions.

For an exclusive view, attribute each stack to its nearest graph function:

| Nearest graph function | Inference CPU share |
|---|---:|
| `identify_wire_neighbors` | **13.75%** |
| `guard_matches_using` | **11.75%** |
| `deliver_route_inner` | **7.77%** |
| `evaluate_turbo_wire_node` | 5.28% |
| `capture_walk_site` | 4.65% |
| `observe_wire_site` | 4.58% |

Thus the main cost remains discovering/capturing physical neighbors and
delivering guarded updates. Further expression-only tuning addresses a small
share of total inference. Prioritize compiling the remaining conditional wire
connections and replacing internal notification routes with typed dependencies;
retain ordered boundary samples wherever they can affect pending work.

The large 13,340-wire corridor contains 20,650 direct transfer edges and 707
conditional edges, with 681 geometry selector tuples. All 707 conditional
sources resolve to fixed member wires. It also has 156 wires with an unsafe
receiver witness after ignoring the conditional restriction: slot-level
witnesses include 142 repeater and 26 Torch occurrences. Conditional topology
alone will therefore not admit that whole corridor. Repeater lock/output and
scheduler effects need an ordered or separately proven boundary treatment.

Artifacts: `target/profiles/stable-inference-current/` contains the compressed
profile, ETL, zero-loss trace statistics, matching executable/PDB provenance,
per-address inline symbols, `hotspots.json`, `inference.folded`, record log and
validated episode. Reproduction scripts are in
`target/stable-instant-logic/{record-profile,profile-symbols,analyze-profile}.py`.

## Guarded transport coverage and complete CPU verification

The separate O5 `compile-guarded-instant-transport` pass compiles conditional
wire connections and ordered repeater/torch/comparator receiver work. It owns
**39,144 / 41,354 wires (94.65%) in 2,614 corridors**, including the largest
13,340-wire corridor. The remaining 2,209 catalog wires overlap other optimized
owners; one peripheral wire is outside the catalog. Its current rejection
census is `target/instant-drives/cpu-transport-rejections.json`; the report now
also lists the twenty largest admitted nets.

Both complete 10,036-tick checks passed. Without the stable-input assumption,
every graph component/piston/resource state and pending-work boundary matches
the plan-free reference. Every tick also matches all 768 native RAM bits and
their availability, command output and the final 0..15 sorted bank. A further
sixteen interpreted ticks after handoff match native RAM again. The separate
stable-input check passes the same RAM, command, final-sort and continuation
requirements. The stable solver still covers 17,426 wires; conditional/ordered
receiver regions execute dense transport, preserving their exact sample cuts.
Four focused boundary tests also pass: conditional ascent/descent with retained
selector captures, both poses of a live resource selector, ordered delayed
receiver scheduling with locked/unlocked/pending repeater cases, and fallback
for a noncanonical imported repeater. Singleton coverage isolates the existing
single-wire owner and signal channels to exercise dense transport directly.
The broader run passed 589 tests; the expected export snapshot change was
regenerated, inspected and passed separately, giving 590 existing checks plus
the four new boundary checks. This is the relevant redpiler subset, not a run
of every core test. Boundary log:
`target/guarded-transport-boundary-verified-tests.log`.

| Full sort, O5 with stable inputs | Before extension | Guarded transport |
|---|---:|---:|
| General wire walks | 2,009,418 | **0** |
| Dense wire visits | 0 | 95,393,263 |
| Dense invocations | 0 | 1,914,042 |
| Compiled signal-channel invocations | 0 | 511,239 |
| Stable expression evaluations | 31,205,330 | 31,205,330 |
| Callbacks | 174,026,524 | 174,026,524 |
| Piston samples | 33,492,510 | 33,492,510 |
| Accepted piston actions | 3,295,083 | 3,295,083 |
| Scheduled ticks | 2,344,242 | 2,344,242 |

This removes the general walker from the measured sort, not all propagation.
The dense program still visits individual wires and executes the retained
boundary callbacks. Whole-circuit logical/stateful replacement remains open.
The earlier 52.47% propagation profile describes the previous implementation
and must not be presented as a profile of this new executor.

Artifacts: `target/guarded-transport/correctness.json`,
`stable-boundaries.json`, `stable-boundaries-before-extension.json`,
`target/guarded-transport-full-cpu-exact.log` and
`target/guarded-transport-full-cpu-stable.log`. The production executable
is recorded separately in `target/guarded-transport/production-provenance.json`.
Five alternating production fat-LTO pairs are complete. All RAM hashes,
active/reset/continuation checkpoints, accepted actions, piston samples,
scheduled ticks and deferred-publication counts match. No build or correctness
test ran concurrently with these timing samples.

| Pair | Previous O5 stable TPS | Guarded O5 stable TPS | Inference time reduction |
|---|---:|---:|---:|
| 1 | 230.44 | 301.00 | 23.44% |
| 2 | 286.98 | 342.68 | 16.25% |
| 3 | 294.34 | 337.59 | 12.81% |
| 4 | 277.24 | 328.04 | 15.49% |
| 5 | 302.12 | 357.38 | 15.46% |

Median inference throughput is **286.98 previous / 337.59 guarded TPS**;
median paired inference-time reduction is **15.49%**. The guarded median is
12.53% above the user-accepted 300 TPS reference. That reference comparison is
unpaired; use the alternating pairs to assess this pass. The relatively broad
sample ranges are retained rather than replacing today's baseline with an
earlier, faster run. Preparation and World projection are excluded from TPS.

Raw samples and source/executable hashes are in
`target/guarded-transport/{pairs,summary,production-provenance}.json`, with the
individual `paired-N-{baseline,guarded}.{json,log}` files. Candidate executable
SHA256: `A5515751A62623C8C2FF02B4A06A69D3047207F821F9A025813AA64517C32023`.
This is a measured transport improvement; the 95.4 million dense visits and
174.0 million callbacks still need replacement by lighter dependencies and
stateful operations before the whole-circuit goal can be called complete.

## Profile of the guarded executor

A new profiling build and complete-sort ETW capture were taken after guarded
transport. Its RAM hashes, active/reset/continuation checkpoints and every
execution count match the frozen guarded production executable. ETW reports
zero lost events/buffers. All 9,164 distinct application addresses resolve
against the matching PDB, including 3,783 inline chains. Inference comprises
35,139 positive-CPU samples representing 32.498 sampled thread CPU seconds.

The following execution regions partition inference CPU; they do not overlap:

| Execution region | CPU share |
|---|---:|
| Dense guarded transport, including its boundary calls | **47.17%** |
| Stable logic, including its boundary calls | **16.18%** |
| Compiled signal channels | 0.46% |
| Other execution | **36.18%** |
| General wire propagation | **0%** |

Dense capture/discovery consumes **20.76% of total inference CPU within the
dense region**; general guard evaluation consumes 5.10% there. Outside the
three compiled regions, general guard evaluation consumes another 8.82%.
These are inclusive subpaths and overlap with their containing region.
Across all inference, guard evaluation is 15.86%, packed piston predicates
5.28%, stable expression/input evaluation 5.58%, and scheduler helpers 0.53%.

The largest nearest-function owners, forming an exclusive attribution, are
`guard_matches_using` 12.48%, `deliver_route_inner` 8.63%,
`run_instant_transport` 6.92%, `capture_transport_site` 6.31%,
`observe_wire_site` 5.42%, and `propagate_transport` 4.70%. Inlining prevents
the whole wire-value evaluator from appearing as one distinct owner; an absent
standalone function must not be interpreted as zero evaluation cost.

This confirms that eliminating the general walker did not eliminate transport
or notification work. The next compiler product must replace internal captures
and wire events with value dependencies while retaining sparse ordered stateful
boundary effects. Callback-family and per-net attribution is being collected
to choose the broadest useful removal, rather than choosing by wire count alone.

Artifacts: `target/profiles/guarded-inference/` contains `hotspots.json`,
`inference.folded`, `profile.json.gz`, the zero-loss ETL statistics, matching
executable/PDB, symbol chains, record log and validated episode. The profiled
35.351 seconds of inference wall time is diagnostic, not another TPS sample;
World projection separately took 6.999 seconds and preparation 13.019 seconds.

The opt-in full-sort callback census also passes, including sorted RAM and
the frozen callback/piston sample/action totals. Accepted callbacks comprise
123,791,475 wire updates, 8,650,780 wire-shape updates, 32,787,537 piston
updates, 3,286,718 observer updates and 5,510,014 other electrical updates.
Only 8,874 accepted piston deliveries find a base already pending. Skipping
those alone cannot address the dominant work; 8,416,395 explicit piston
candidates are base-pending before their existing guards are evaluated.

| Hot transport net | Wire visits | Share of dense visits | Possible wire-power components | Largest component |
|---|---:|---:|---:|---:|
| 15 (13,340 wires) | 68,221,728 | **71.52%** | **2,150** | **56 wires** |
| 76 (1,006 wires) | 11,659,899 | **12.22%** | 79 | 15 wires |
| 372 (667 wires) | 3,555,570 | 3.73% | 51 | 22 wires |
| 877 (353 wires) | 3,430,160 | 3.60% | 53 | 16 wires |

The hottest corridor's median power-component size is two wires. These are
weak components of direct plus possible conditional Turbo strength edges;
they do not erase source/state dependencies or notification ordering. The top
two nets account for **83.74% of dense visits**. This supports compiling small
electrical value regions within the existing ordered event envelope, rather
than solving one 13,340-wire state space. The next pass and its interval/ordering
certificate are specified in `INSTANT_CIRCUIT_ACCELERATION_PLAN.md`.

Attribution artifacts: `target/guarded-transport/callback-census.json`,
`callback-census-summary.json` and `callback-census.log`. Instrumentation is
test-only and opt-in; this diagnostic run makes no new throughput claim.

### Electrical value-region baseline

The separate O5 `compile-instant-value-regions` pass now builds compact
max-plus programs inside the certified transport nets. It groups equal typed
drives, interns byte distance rows and retains conditional geometry selectors.
Its first runtime certificate solves a region at its first member capture in
each walk. If every member already has the solved strength, the executor caches
its typed wire values and skips subsequent power calculations for that region.
Unsettled regions use exact transport. Root propagation, capture cuts, heading,
layer marks and boundary calls remain ordered. No interior wire events are
removed by this baseline stage.

Five focused value-region tests pass: generated/direct/conditional solver
comparisons, work/cancellation limits, forged-row validation and runtime
settled/unsettled/source-change/boundary-order checks. The redpiler suite has
598 passing checks plus its expected changed O5 export fixture, which was
regenerated, inspected and rechecked successfully. The production BubbleSort,
CPU and instant benchmark targets also compile.

Both full CPU gates pass: normal mode compares every graph state and pending
future at all 10,036 ticks against the reference graph and checks all 768 native
RAM bits; stable mode checks native RAM/readiness, commands and the final sorted
0..15 result. Both check sixteen interpreted continuation ticks after handoff.

| Execution mode | Region solves | Wire calculations skipped | Ordered wire visits retained |
|---|---:|---:|---:|
| Exact | 15,526,700 | 44,008,500 | 118,723,749 |
| O5 + stable inputs | 9,164,091 | 32,096,479 | 95,393,263 |

Stable execution retains 174,026,524 callbacks, 33,492,510 piston samples,
3,295,083 accepted actions and 2,344,242 scheduled ticks, matching the prior
guarded stage. These are calculation skips, not removed visits or callbacks.
The five alternating frozen fat-LTO production pairs show a regression:

| Pair | Guarded baseline TPS | Value-region TPS | Execution time change |
|---|---:|---:|---:|
| 1 | 344.54 | 307.05 | +12.21% |
| 2 | 304.28 | 311.11 | -2.20% |
| 3 | 334.31 | 295.42 | +13.16% |
| 4 | 327.05 | 295.48 | +10.69% |
| 5 | 351.19 | 285.94 | +22.82% |

The median TPS is **334.31 baseline / 295.48 value regions**; median paired
execution time increases **12.21%**. Every pair preserves RAM hashes, final
sorted words, active/reset/continuation checkpoints and semantic work counts.
Callbacks are unchanged. Timing excludes compilation, projection and probes,
and no builds, correctness runs or profiling ran during these pairs. The prior
guarded 337.59 TPS median remains a separate earlier experiment.

This is a correct experimental representation baseline, not a performance win.
The pass body itself takes a median 0.0171 seconds to compile, but runtime still
solves 9.16 million regions and retains all 95.39 million dense visits. A fresh
runtime profile is needed to partition the added costs. The next proposed
optimization is to reuse a region's complete typed wire snapshot when its live
driver/selector snapshot is unchanged and no member wire has been modified.
Use the existing node-to-transport binding for region-local invalidation in
`set_wire`, with independent per-region input snapshots and revision checks.
Shared interned query/selector IDs must not let one region overwrite another's
certificate. Then remove proven marker/capture work; whole-circuit replacement
remains unfinished. The stable correctness harness's timing is diagnostic
because it shares a run with the reference/native checks.

Artifacts: `target/value-regions/` holds the build logs, correctness logs,
`correctness.json`, immutable production executable/provenance and alternating
pair measurements. The candidate SHA-256 is
`F1AA812D225F894AD351ECA3BA0C33F81965AE7AB14F2FBBACE04F1416D579E5`.

### Value-region inference profile and runtime rewrite

A fresh 1 kHz ETW profile of the regressing version covers a complete native-
trace-checked sort, with zero lost events/buffers and matching production RAM,
checkpoints and execution counters. The 37,216 inference samples represent
34.743 sampled thread-CPU seconds. Dense transport accounts for 52.37% of
inference, stable logic 14.25%, channels 0.33% and other execution 33.05% in an
exclusive partition. Capture/discovery is 31.52% inclusive and overlaps dense
transport. The region-certification callsite consumes 10.33%: typed drive reads
3.40%, geometry reads 1.67%, wire validation reads 1.06%, and the remaining
inlined solver/bookkeeping 4.20%. PDB information merges the classifier/solver
into the caller at `transport.rs:151` in that frozen version; absent standalone
function samples cannot be interpreted as zero solver cost.

The runtime rewrite caches certificates per electrical region. Current typed
driver and selector values are sampled once per walk; a valid unchanged region
reuses its complete typed wire snapshot. Actual power or shape writes invalidate
only their owning region through the shared wire-program invalidation hook.
Driver-group and conditional-arc snapshots are stored in region-owned slots,
even when query or selector IDs are interned across regions. Revision checks
guard publication. Construction starts with empty caches.

A second representation change stores member-wire capture power as a byte
when all inbound callback-selection slots are source-certified as known.
Unknown admission retains the full observation. Changed power updates the
captured byte at the original cut, and a full wire read preserves side properties
on actual writes. Non-wire captures, heading, queue layers and boundary order
are preserved. The recording/reference path retains full observations.

The focused cache/selector/invalidation/fallback/forgery tests pass. The broader
suite has 601 passing checks plus the expected export-fixture update, regenerated
and successfully rechecked; the fixture difference is confined to value-region
metadata. The complete exact CPU test passes all state/future/native RAM and
interpreter-continuation gates. Its region solves fall from 15,526,700 to
4,878,723, with 10,647,977 certificate hits and 55,966,509 scalar captures.
Ordered visits and all semantic work counts are unchanged. The full stable-input
check also passes all 10,036 native RAM/readiness boundaries, commands, final
sorted words and sixteen interpreted continuation ticks. Stable execution uses
2,929,286 solves, 6,234,805 certificate hits and 42,603,086 scalar captures,
retaining 95,393,263 ordered visits and 174,026,524 callbacks.
The frozen production executable is
`target/value-regions-cached/cpu-bubblesort-production.exe`, SHA-256
`D4AABA086F66D6780B1E182DB7F5D04DE3C3E1ACEF9AAF44723E6C58E1441815`.
Five alternating frozen fat-LTO production pairs retain all RAM hashes, sorted
words, active/reset/continuation checkpoints and semantic work counts:

| Pair | Guarded baseline TPS | Cached/scalar TPS | Execution time change |
|---|---:|---:|---:|
| 1 | 341.00 | 297.46 | +14.64% |
| 2 | 342.61 | 316.29 | +8.32% |
| 3 | 352.76 | 314.46 | +12.18% |
| 4 | 321.31 | 305.26 | +5.26% |
| 5 | 356.21 | 298.85 | +19.19% |

Median throughput is **342.61 / 305.26 TPS**, with **12.18% more execution
time** in the median paired comparison. No builds, correctness tests or
profiling ran during timing. This remains a performance regression against
guarded transport; fewer solves and compact captures do not demonstrate
acceleration. The earlier uncached value-region experiment is a separate
measurement and is not a matched baseline for claiming a cache speedup.
The full measurements are `target/value-regions-cached/{pairs,summary}.json`.

An opt-in test-only region census independently completes the same full sort
with matching frozen semantic counts. It attributes all 95,393,263 transport
visits, including 32,096,479 inert visits, with zero unattributed visits and
32,396,233 changed-power writes. The hottest individual region is net 15,
region 313: 56 wires, three driver groups, two conditional arcs and 773,715
visits. No single region dominates: the large net combines many small
electrical regions through notification dependencies. The report retains typed
drivers, conditional geometry, member positions and ordered boundary references
for the hundred hottest regions in `target/value-regions-cached/region-census.json`.

Of those hundred hot regions, forty in net 15 have exactly one typed drive
group and no conditional arcs. Together they execute 11,127,067 visits across
648 wires (about 11.7% of all retained visits). Region 90 is a concrete
15-wire straight line with 348,576 visits. Electrical independence does not
prove that its notification markers and captures can be removed: neighboring
regions may observe them during the same transport episode. A test-only
follow-up census measures those structural dependencies and complete episodes
in which every visited wire is certified inert. Boundary-reference lists are
structural references, not an observed chronological delivery trace.

That follow-up full-sort census passes and gives a stronger scope decision:

| Candidate subset | Retained visits |
|---|---:|
| All one-drive, zero-conditional regions | 62,858,499 (65.89%) |
| Same subset with zero foreign mapped-wire slots and zero shared non-wire capture sites | 0 |
| Complete all-inert transport episodes | 1,231,414 (1.29%) across 502,423 invocations |

The first subset contains 26,060,801 inert visits and 20,804,671 changed-power
writes. Even the simple region 90 has 31 canonical slots referencing wires in
other mapped regions and one non-wire capture site shared with another region.
These are structural dependencies, not proof that every such interaction is
observable in a memory output. They do rule out treating these regions as
unconditionally private. Restricting the next pass to structurally isolated
single-drive regions yields no measured coverage; restricting it to all-inert
whole episodes addresses only 1.29% of visits. Prefer a composable ordered
program that preserves the cross-region marker/capture interface and then
minimizes it across regions. Neither shortcut alone meets the acceleration goal.

Profile artifacts: `target/profiles/value-region-inference/`; rewrite build and
correctness logs: `target/value-regions-cached/`. This remains preparatory work:
the retained wire visits still need replacement by compiled stateful operations.

### Whole-episode program baseline

The subsequent `compile-instant-transition-programs` pass replaces whole
transport episodes with guarded contiguous effect programs, including coupled
electrical regions. Both complete CPU correctness gates pass. In stable mode
it removes 73,676,184 of 95,393,263 interior visits (77.23%) while preserving
native RAM, commands, sorted output, state/future obligations and interpreted
continuation. Five clean alternating fat-LTO pairs measure **327.12 -> 365.83
median TPS** versus guarded transport, with **7.80% less paired execution time**.
This is a separate matched experiment from the earlier cache-only regression.
The effect program still executes 85,157,316 instructions and retains 21,717,079
fallback visits, so whole-circuit replacement remains incomplete. See
[the full contract, tests and pair table](INSTANT_TRANSITION_PROGRAMS.md).

### Instruction and entry-guard minimization

Known callback admissions now use the shared channel minimizer. Guards retain
only wire powers consumed by transport control, alongside live drive and
conditional-mask checks. Seven focused checks, 609 broader checks, both full
CPU runs and interpreter continuation pass. Remaining stable wire visits fall
21,717,079 -> 10,822,421 (50.17%); replay instructions fall 85,157,316 ->
79,214,540 (6.98%). Piston samples and future effects remain unchanged.

Five alternating fat-LTO pairs against the initial transition program give
211.29 -> 227.45 median TPS and 7.09% less median paired execution time.
Other desktop applications were active; both binaries match their full
correctness work counts. This is a modest additional gain. Fresh profiling
should guide removal of the remaining replay/boundary execution before broader
typed call fusion. Full evidence: `INSTANT_TRANSITION_PROGRAMS.md`; artifacts:
`target/instant-transition-minimized/`. Whole-circuit replacement remains open.
