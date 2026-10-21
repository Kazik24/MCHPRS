# Exact whole-episode transition programs

The `compile-instant-transition-programs` O5 pass generalizes ordered signal
programs to transport nets with many independent inputs and conditional wire
edges. It compiles eligibility, then the backend specializes observed episodes.
The first implementation retains exact wire writes and boundary helpers; it
does not yet replace the entire CPU with a minimized memory-only program.

## Static contract

Every member wire must have an existing typed no-dust drive query. Every
retained slot targeting another member wire must have statically known callback
admission in its source wire's validated callback-selection table. The underlying
transport certificate supplies fixed member ownership and source emissions and
geometry that remain stable during the walk. Delayed-device callbacks retain
their original ordered sample cuts. Canonical repeater emissions are checked by
the existing transport-entry admission before execution.

The product stores one drive query ID per transport-local wire. Compilation
is bounded and cancellable; publication and full validation independently
rederive it. Unsupported nets keep exact transport. All 2,614 CPU transport
nets, containing 39,144 wires, qualify, including hot nets 15 and 76.

## Program and mutable state

A program stores contiguous `SignalOp` instructions:

- `Capture(site)` at the site's original first discovery, retaining only sites
  used by a boundary selection or call.
- `Select(wire, slot, selection)` at the original enqueue-time admission cut.
- `SetPower(wire, value)` at the original write cut, preserving live side
  properties and normal dirty/cache invalidation bookkeeping.
- `Call(site, selection, layer, route)` at the original FIFO boundary cut.

Piston/resource state and all pending work stay authoritative in `CircuitState`
and `GraphFuture`. A program never caches a callback's acceptance result,
pending membership, resource availability or piston request. Those are sampled
by the existing helpers at replay time.

The entry certificate contains first-captured member wire strengths consumed by
transport control, the distinct typed drive queries actually read, and the selected input
masks for conditional transfers actually evaluated. There is no whole-net scan
or probabilistic hash in the correctness check. Side properties are read live
when writing; internal callback decisions are statically certified rather than
sampled from mutable side properties.

## Recording, replay and proof obligation

On a miss, the backend executes the original dense episode and records its
ordered effects. It captures wire entry values before their first write, reads
stable typed drives normally and records conditional input masks. A rejected
non-wire admission adds a no-op placeholder to the recording queue so the
potential `Call` still occupies its correct FIFO cut in the program. Layer
marking precedes admission in both paths. Boundary admission is recomputed
live on every replay, and all required first-discovery captures stay in order.

Before a replay performs any effect, every recorded entry condition must match.
Equal consumed wire entry values, stable drive values, conditional selections and fixed
internal admission reproduce the wire-control decisions inductively: the same
captures, writes, visited/heading/bias decisions and queue insertions lead to
the same boundary cuts. The program can therefore omit the interior wire
captures, markers and queues. Boundary captures and calls remain live, including
repeater locking, pending scheduling, request provenance and deferred revisions.
An entry mismatch falls back before optimized effects. A recording error or
budget overflow cannot publish a partial program.

Each root retains at most four variants and makes at most sixteen recording
attempts. Stored program payloads have a 32 MiB backend budget. Exhausting a
budget retains exact execution. Derived caches start empty on backend creation;
they are not serialized as machine state.

## Initial evidence

Three focused tests pass: cancellation, forged product rejection, and arbitrary
entry profiles with independent inputs, repeat hits, exact state/future and
semantic boundary-order comparison. Hits in that test execute zero interior
wire visits. The broader suite has 604 passing checks plus the expected O5
export-fixture change, inspected and successfully rechecked. Fixture changes
are confined to capability/eligibility metadata.

The first 128 CPU ticks match the reference graph's complete state/future and
all native RAM observations. The candidate records 21,728 hits and 16,492
misses, removes 772,997 visits, and executes 611,966 dense visits. Their sum
matches the retained pre-transition work in this window. Piston samples,
accepted actions, scheduled ticks, deferred publications and kernel invocations
match; callback counts fall because removed wire deliveries are not counted
as executed work. This is a work-reduction observation, not a throughput claim.

The complete normal-mode CPU check also passes: all 10,036 exact graph
state/future boundaries, all 768 native RAM bits, final sorted words and sixteen
interpreted continuation ticks. It records 2,962,608 hits, 456,043 misses and
33,565 published programs. Interior visits fall from 118,723,749 to 23,724,751;
the 94,998,998 elided visits account exactly for the difference. Callback count
falls from 200,945,830 to 105,946,832 by the same amount. Piston samples
(35,415,396), accepted actions (3,295,083), scheduled ticks (2,344,242) and
deferred publications (3,295,083) match. The 117,822,640 replayed effect
instructions still need throughput measurement and later minimization.

Stable-input full-sort correctness also passes all native RAM/readiness,
commands, final sorted words and sixteen interpreted continuation ticks.
It records 1,579,401 hits and 334,641 misses, with 18,404 published programs.
Interior visits fall from 95,393,263 to 21,717,079, exactly accounting for
73,676,184 elided visits (77.23%). Callback count falls by the same amount to
100,350,340. Piston samples (33,492,510), accepted actions (3,295,083),
scheduled ticks (2,344,242) and deferred publications (3,295,083) are unchanged.
The program executes 85,157,316 effect instructions; remaining work needs
minimization after collecting this pass's baseline throughput.

All five focused tests pass, including conditional geometry changing with
unchanged wire strengths and typed drives, and budget overflow preserving
already queued placeholder decisions. The final test build and three
production benchmark targets compile. The only post-production-build source
change fixes a test-only empty-table assertion; production execution is
unchanged. The frozen fat-LTO production executable is
`target/instant-transition-programs/cpu-bubblesort-production.exe`, SHA-256
`9638A240CAE16A4666359835C08504A5513750501EDE342501D1F831B9BD4C83`.

## Production baseline before minimization

Five alternating frozen release fat-LTO pairs use O5 plus stable inputs and
the complete 10,036-tick native-trace-checked sort. Timing includes only
`tick_with_world`; compilation, activation, projection and validation probes
are excluded. No builds, correctness tests or profiling ran during the pairs.
The reference is guarded transport, not the regressing cache-only rewrite.

| Pair | Guarded reference TPS | Transition-program TPS | Execution time reduction |
|---|---:|---:|---:|
| 1 | 344.23 | 365.83 | 5.91% |
| 2 | 327.12 | 354.78 | 7.80% |
| 3 | 301.73 | 381.17 | 20.84% |
| 4 | 351.45 | 368.12 | 4.53% |
| 5 | 325.79 | 362.70 | 10.18% |

Median throughput is **327.12 -> 365.83 TPS**. Median paired execution-time
reduction is **7.80%**. These statistics differ because the reference varies
between runs; do not derive a paired speedup from the ratio of separate medians.
All five pairs preserve RAM hashes, sorted words, active/reset/continuation
checkpoints and exported semantic work counts. Candidate execution counters
are identical across the pairs and match the full stable correctness run.

This is a verified acceleration over guarded transport and exceeds the
user-accepted 300 TPS reference. It is not whole-circuit replacement: exact
fallback still handles 21,717,079 visits, and 85,157,316 replay effect
instructions retain substantial boundary work. The first minimization should
fold only statically known callback admissions using the existing channel
minimizer, then prune dead selections/calls/captures and compact selection IDs.
Further predicate/sample fusion must establish equality of the *live* predicates;
learned boundary power values do not cover unguarded external state inputs.
Fresh FPU timing is not collected for this stage.

Measurement artifacts: `target/instant-transition-programs/{pairs,summary,correctness}.json`,
raw pair logs/reports, production provenance and both full correctness logs.

Artifacts: `target/instant-transition-programs/` and the frozen test executable
`target/instant-transition-tests.exe`.

## Instruction and entry-guard minimization

The next version shares the existing channel minimizer with adaptive transition
programs. Only known bits from validated callback-selection tables become
constants: known-false calls disappear; known-true calls keep their original
delivery cut without a selection; dynamic selections retain their enqueue and
delivery cuts with compact IDs. Captures remain at their original first-discovery
cut whenever a surviving selection or call needs them. Learned callback outcomes
are never promoted to constants.

Entry guards now retain only first-captured powers consumed by transport control:
the evaluated wire's held power and the selected maximum-input powers when the
drive is below fifteen. Capturing a wire for neighborhood discovery alone no
longer constrains replay. Conditional masks and typed drive queries retain their
checks. Boundary predicates remain live at the retained ordered power-write and
call cuts; their external reads are not assumed constant. A later write alone
does not justify removing an entry guard.

The admission-only intermediate passes its focused checks and the exact
128-tick CPU/native check. It keeps the same hits, misses, visits and semantic
work while reducing replay instructions from 902,227 to 782,733 (13.24%). This
is an instruction-count observation, not a measured throughput result.

The combined version passes seven focused checks and 609 broader redpiler
checks, with no fixture changes. Its new regression changes a wire captured for
discovery but never consumed by transport control and requires a replay hit;
changing a selected input instead requires a miss. Both paths match exact
state, future and semantic operation order. The minimizer unit checks dynamic
admission outcomes, retained capture/delivery cuts and idempotence.

Both complete 10,036-tick CPU checks pass, including all 768 native RAM bits,
sorted output and sixteen interpreter continuation ticks. Normal mode also
compares complete graph state/future at every tick. Stable mode checks native
RAM/readiness and command outputs. Piston samples, accepted actions, scheduled
ticks, deferred publications and kernel invocations match the preceding version.

| Stable-input work | Initial transition program | Minimized program | Reduction |
|---|---:|---:|---:|
| Remaining dense wire visits | 21,717,079 | 10,822,421 | 50.17% |
| Transition misses | 334,641 | 160,580 | 52.01% |
| Published programs | 18,404 | 12,563 | 31.74% |
| Replayed instructions | 85,157,316 | 79,214,540 | 6.98% |
| Executed callbacks | 100,350,340 | 89,455,682 | 10.86% |

The minimized version records 1,753,462 hits and removes 84,570,842 wire visits:
**88.65%** of the guarded-transport reference's 95,393,263 visits. In normal
mode it removes 107,580,446 of 118,723,749 visits, leaving 11,143,303. These are
real omitted deliveries, not counters adjusted to hide executed work.

All three production benchmark targets compile. The frozen fat-LTO executable
is `target/instant-transition-minimized/cpu-bubblesort-production.exe`, SHA-256
`2570CA49FAD7281D84D9CB2EE22B6B6043FA7679D3EEC3FD056F25D8725A0D0A`.
The subsequent source change only fixes a test fixture's draft-node lookup;
production execution is unchanged.

Five alternating production fat-LTO pairs compare the preceding frozen
transition program against the minimized version, O5 plus stable inputs, with
10,036 ticks per run. Timing remains `tick_with_world` only. No agent build,
correctness run or inference profiling ran alongside the timing pairs.

| Pair | Previous program TPS | Minimized program TPS | Execution time reduction |
|---|---:|---:|---:|
| 1 | 202.98 | 214.99 | 5.59% |
| 2 | 215.35 | 231.79 | 7.09% |
| 3 | 208.92 | 221.03 | 5.48% |
| 4 | 221.00 | 238.36 | 7.28% |
| 5 | 211.29 | 227.45 | 7.10% |

Median throughput is **211.29 -> 227.45 TPS**; median paired execution time
falls **7.09%**. Other desktop applications were active, and the unchanged
reference binary is substantially slower than in the preceding session. Its
hash is still `9638A240CAE16A4666359835C08504A5513750501EDE342501D1F831B9BD4C83`.
Use the current paired comparison; do not compare these absolute TPS medians
with the historical 365.83 or accepted 300 TPS references as an algorithmic
regression, and do not multiply the separate experiments into a claimed total.

Every pair preserves RAM hashes, sorted words, active/reset/continuation
checkpoints and exported semantic counts. All 27 exported work counters of
each executable match its corresponding full correctness run across every
pair. Deferred publications are checked by the full correctness tests, not
exported by the production benchmark's counter object.

This remains a modest acceleration. Stable inference still executes 79,214,540
replay instructions and 33,492,510 live piston samples. A fresh inference
profile should determine how much remains in entry checks, captures, replay
dispatch, power predicates and state commits. Typed piston-call specialization
can bypass generic site dispatch and prune captures only when receiver identity
is proved; all live samples must retain their original cuts. Whole-circuit
replacement and new FPU performance measurements remain unfinished.

Artifacts and source provenance: `target/instant-transition-minimized/`,
including `pairs.json`, `summary.json`, `correctness.json`, raw reports and logs.

The subsequent [fresh inference profile](INSTANT_TRANSITION_PROFILE.md) matches
all production gates and reports exact dense transport at 10.47%, shared effect
replay at 17.70%, stable logic at 19.71%, and transition entry checks at 7.70%
in an exclusive partition. Other graph execution remains 44.42%. General guard
evaluation accounts for 18.96% in an overlapping inclusive group, directing the
next separate pass toward compact pure boundary predicates rather than only
bypassing generic piston dispatch (0.50%).
