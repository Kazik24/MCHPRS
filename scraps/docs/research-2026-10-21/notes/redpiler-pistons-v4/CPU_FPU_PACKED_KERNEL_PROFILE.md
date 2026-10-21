# FPU and BubbleSort: current packed-kernel CPU profile

Reported 2026-10-20, using captures from 2026-10-19 UTC. **Wire propagation
still dominates after event composition, CPU packing, scalar specialization
and typed wire-emission reads.** The next useful targets are neighborhood
capture, contact selectors and ordered route guards. Wider SIMD and an index
for deferred resource extensions have smaller measured opportunities.

These are fresh profiles of the current production O5 pipeline, rather than
the earlier singleton/whole-wave kernel implementation described in
[the historical profile](CPU_FPU_FINAL_GRAPH_PROFILE.md).

## Captures and accounting

- Production-style release optimization, fat LTO, matching PDBs, Windows x64,
  Ryzen 9 5950X; no experimental Cargo flags or stable-input promise.
- ETW sampling requested at 1,000 Hz, retaining the target's main thread and
  source inline frames. Default process affinity. No builds or test suites ran
  during the accepted captures.
- Both accepted traces report **zero lost buffers and zero lost events**.
  The first BubbleSort capture exhausted F: during conversion and had 39 lost
  buffers. It is retained for diagnosis and excluded from the results below.
- BubbleSort: one complete 10,036-tick sort, all per-tick RAM observations equal
  to the frozen interpreter trace, final RAM sorted `0..15`, plus the harness's
  32-tick native continuation. It uses the established loader/reset sequence and
  identical in-memory removal of the documented wall sign.
- FPU: `FPU_MROWW.schem`, green-wool trigger, 64 seeded random operand/opcode
  vectors, held OFF64/ON64. Only the final O5 replay is included; the preceding
  O0/O5 validation replays are excluded by a recorded stdout timestamp.
  The final replay includes 1,088 initialization/warmup ticks and 8,192 vector
  ticks. Its seed is `0x4d43485052532026`.
- Inference excludes loading, compilation, projection and native continuation.
  Stacks are weighted by `threadCPUDelta`; zero-CPU samples are excluded. These
  percentages describe sampled CPU execution, not elapsed-time throughput or
  hardware cache counters. The analyzer verifies sample accounting and the
  exclusive partition totals.

There are **32,727 BubbleSort inference samples and 13,265 FPU inference
samples**. All 7,644 / 9,683 distinct application addresses in the respective
captures resolve through the matching PDBs, including inline call chains.
No FPU input-update sample was identified inside the final replay; this does
not establish zero input-update cost.

## Where inference spends CPU time

The following rows are **inclusive and overlap**. Capture, guards and predicates
can be inside propagation or piston callbacks; their percentages must not be
added together.

| Runtime path | BubbleSort | FPU |
|---|---:|---:|
| Wire propagation | **68.19%** | **63.84%** |
| Neighborhood capture/discovery | 36.09% | 30.50% |
| Guard evaluation | **19.00%** | **19.65%** |
| General power queries | 4.80% | 5.92% |
| Packed predicate evaluation | 4.58% | 4.81% |
| Raw wire-side calculation | 2.86% | 2.67% |
| Typed wire emission | 1.38% | 0.86% |
| Scheduler helpers | 0.69% | 1.29% |
| `pending_resource_extension` | **0.34%** | **0.18%** |

For a partition that **does sum to 100%**, the analyzer attributes each stack
to the first matching operation beneath its outermost propagation call. Other
stacks remain outside walks. This includes direct `observe_wire_site` work and
therefore differs slightly from the inclusive capture group above.

| Exclusive propagation branch / remainder | BubbleSort | FPU |
|---|---:|---:|
| Neighborhood discovery and capture | **36.29%** | **31.52%** |
| Walk/queue bookkeeping | **17.05%** | **16.36%** |
| Wire strength calculation | 6.12% | 5.50% |
| Callback admission | 5.08% | 5.53% |
| Receiver callbacks inside walks | 3.64% | 4.93% |
| Execution outside walks | 31.81% | 36.16% |

Within execution outside walks, deferred publication owns 14.80% / 17.00% of
all inference samples and piston acceptance owns 10.86% / 10.26%. Those scoped
branches include their subordinate work. A large inclusive entry-function share
does not mean the entry function itself is expensive.

The nearest graph functions to the sampled leaf are:

| Function | BubbleSort | FPU |
|---|---:|---:|
| `identify_wire_neighbors` | **14.54%** | **12.34%** |
| `guard_matches_using` | **13.72%** | **13.46%** |
| `deliver_route_inner` | 6.94% | 7.02% |
| `propagate_wire_changes` | 6.13% | 5.36% |
| `capture_walk_site` | 5.33% | 4.71% |
| `observe_wire_site` | 5.17% | 4.26% |

These nearest-owner rows are an exclusive attribution to graph owners; the
remaining functions account for the rest. They are not interchangeable with
the inclusive table.

Guard costs are primarily outside the packed kernel predicates. Contact/site
capture accounts for **7.87% / 6.51% of all inference samples**, and ordered
routing for **7.36% / 8.62%**. Callback admission adds 1.65% / 1.88%. Thus,
specializing only kernel predicates leaves most guard execution untouched.

## Work still executed

The complete BubbleSort run executes:

| Operation | Count |
|---|---:|
| Callbacks | 202,834,986 |
| General wire walks | **3,581,461** |
| Piston samples | 35,415,396 |
| Accepted piston actions | 3,295,083 |
| Scheduled component ticks | 2,344,242 |
| Instant kernel invocations | 1,766,428 |
| Classic-net invocations | 0 |

Compilation reports 15,200 instant-kernel nodes and zero accepted classic nets.
Connected event composition and predicate packing have not eliminated these
walks. SIMD cannot remove their discovery, capture or FIFO-delivery obligations.

## What to improve next

1. **Specialize contact selectors and ordered route guards.** Extend typed
   scalar evaluation to the state-only selector/guard patterns that actually run
   in capture and routing. `observe_wire_site` still searches contact variants
   with the general guard evaluator; `deliver_callback` still evaluates each
   route guard. Preserve first-match contact priority, captured/live operands and
   the original sampling point. Do not reuse the CPU query builder's handling
   of absent captured observations as a shortcut for route captures.
2. **Reduce neighborhood setup and plan copying.** `identify_wire_neighbors`
   clones a whole wire plan, builds 24 slots, computes visit/bias information
   and publishes cached neighbors. Other hot propagation paths clone plans too.
   Separate hot execution fields from cold metadata and specialize the admitted
   capture-slot operations. Preserve heading, visited state and each walk's
   observations. Compiled walks already use dense slot tables; replacing a
   presumed hash lookup is not a new optimization here.
3. **Remove proven local wire work.** A certified ordered multi-wire transfer
   can address the dominant operation count. Begin with a small net whose
   receivers only enqueue work; preserve threshold crossings, captured inputs,
   request provenance/order and reset deadlines. Kernel connectivity alone is
   insufficient proof, and unordered/fixed-point experiments remain excluded.
4. **Keep predicate fusion and wider SIMD secondary.** Packed evaluation is
   currently about 4.6–4.8% inclusive, giving a limited ceiling for additional
   kernel-only evaluation improvements. A deferred-resource index is lower
   priority still: the current search costs 0.34% / 0.18%.

This profile changes the priority of the proposed work. Existing scalar work
has [separate paired timing evidence](CPU_KERNEL_EXECUTION.md), but it is not
the main remaining runtime cost. Typed wire reads are implemented and tested;
no isolated throughput claim is made for them in this profiling batch. Further
optimization work was paused to collect this evidence.

## Projection, server TPS and limits

The harness retains the original World for command effects, presentation and
correctness observations; graph state owns compiled inference. BubbleSort
publishes all blocks every tick; FPU's final replay publishes at phase
boundaries. Projection is separate from inference in both profiles. BubbleSort
records 5.207 s of projection and 32.748 s of tick wall time under the profiler;
these are diagnostics, **not a new TPS benchmark**.

The live server log uses `--io-only` and batches publication. Networking,
players and the full server update loop are not profiled here, so these captures
do not establish why the server reports 150 TPS. `/tps timings` is still needed
to distinguish live simulation from client/update-loop costs.

FPU's observed numeric output remains `0xffff`, including the earlier native
checks. The profile measures switching execution, not verified floating-point
arithmetic throughput. No cache-miss or branch-misprediction counters were
collected. Layout changes remain candidates to measure, rather than a proven
hardware-cache diagnosis.

## Evidence and verification

Artifacts are under `target/profiles/packed-kernels-current/`:

- `binaries.json`, `provenance.json`, `source-hashes.json`, `source.diff`,
  `source/` and `bin/` preserve the source/build cut and executable/PDB hashes.
- Accepted captures: `bubblesort-clean/` and `fpu/`, each containing
  `profile.json.gz`, symbol data, `trace-stats.log`, `hotspots.json` and
  `inference.folded`. `record.json` contains commands and stdout timestamps.
- `bubblesort-clean-episode.json` preserves the RAM and continuation results;
  `fpu-episode.json` preserves stimulus, output and validation details.
- `record.py` records newly launched children; `analyze.py` checks accounting
  and computes the partitions. `symbols.json` retains per-address PDB results
  with inline frames, rather than relying on function-start labels alone.

Raw ETL files are retained outside the full workspace drive, under
`C:/Users/Maciej/AppData/Local/Temp/MCHPRS-packed-kernel-profile-20261019/`.
The damaged first BubbleSort capture is explicitly excluded. Compressed accepted
profiles and all analysis needed for the tables remain in the repository's
`target` directory.

Validation after the changes: **558 Redpiler tests passed**, including 11 focused
CPU checks; 33 Python loader tests and 39 viewer tests passed. The profiling
benches build with production fat LTO. Logs are in `target/cpu-wire-emission/`
and `target/cpu-scalar/`. No server process was restarted or modified.
