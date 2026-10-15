# Closed instant transition kernel — first implementation

Status: implemented for bounded closed selections; awaiting review and final benchmark results.

The ordered region proxy reduced dispatch overhead but still ran the internal Turbo walks and reset callbacks. `compile-instant-machine` is the next O3 pass: for a small closed instant selection it evaluates exact ordered transitions during compilation, then executes their checked memory writes and future work directly.

The first supported real fixture is **ADDER1BIT**, including all 13 actors and shared payloads. This is a whole-selection kernel, not yet a kernel extracted from a larger CPU. ADDER11BIT, Divider and CPU keep the existing ordered graph execution.

## Admission and bounds

- Explicit `-O3 --stable-instant-inputs`; no new default assumption.
- Exactly one certified stable region enclosing every selected actor, contact, payload and wire.
- At most 128 nodes, 16 pistons and four lever controls.
- Admitted node kinds: lever, constant, repeater, built-in reset, wire, piston and contact.
- Observers with outside effects, command/note blocks, other component kinds and imported entry ticks decline this optimization.
- At most 2,048 normalized states and an estimated 16 MiB of program storage. An incomplete table is valid: missing states/actions execute ordinary Graph before effects.

The stability promise starts at the activating **falling edge** and lasts until certified reset. Internal reset, carry and inhibition signals still change. This kernel actually records exact transitions for the admitted histories, including extra histories it happens to explore; it never assumes unordered propagation or guesses a reset duration.

## Compiler stages

1. Check the closed boundary and the supported node family.
2. Lower a copy without a transition kernel, using the ordinary Graph executor as the compile-time evaluator.
3. Explore held-control histories first: control combinations in forward/reverse order, followed by ticks until the exact normalized state repeats. A periodic endpoint is an exploration endpoint, not a declaration of reset readiness.
4. From each periodic endpoint, evaluate each single control toggle followed by its held-input trajectory. Unknown histories remain general; no broad exploration of mid-reset input changes is needed.
5. Record exact target state, component/actor/resource writes, dirty owners, ordered presentation events, and future obligations for each successful edge.
6. Independently replay every retained edge during graph validation. Reject incorrect targets, writes, revision deltas, presentation, or obligation counts.

No fixture name, coordinates, arithmetic truth table, learned runtime response, or second runtime simulator participates in compilation. A proof-budget limit leaves a missing edge; it does not authorize guessing.

## State representation and normalization

`MachineState` retains **all** admitted component values, wire power and four shape sides, actor poses/availability, payload contact/enabled selections, ordered scheduled ticks, FIFO piston requests and ordered deferred publications. Complete structural equality resolves hash collisions. It is not an input-vector response cache.

Absolute origins are normalized:

- Scheduled ticks retain relative half-tick delay, dispatch priority and receiver identity. Unique generated pending membership is verified; stale/duplicate/mismatched imported work remains general.
- Actor and payload revision witnesses retain their independent wrapping offsets from current revisions. Transitions add recorded revision increments to the live origins.
- Deferred deadlines retain their offset from the logical clock. Execution near clock overflow remains general.
- Scheduler ring rotation is unobservable; relative ordered queues reconstruct the same dispatch order.

Dirty bits are append-only between flushes: transitions OR their touched nodes into the existing bits. Piston presentation events are also append-only and retained in their original order. Neither feeds circuit calculations. Host effects that can feed results back are outside this family.

## Runtime stages

At `tick` or a selected lever toggle:

1. Capture the complete normalized key into a reused scratch buffer.
2. Look up a checked edge by full state and action.
3. On a miss, return to the original executor before changing semantic state.
4. On a hit, apply recorded writes and revision increments, restore ordered future work, append presentation, and advance the public clock for a tick.

There are no internal Turbo walks, callbacks, piston samples or reset-route dispatches on a hit. This still materializes all memory and pending obligations so projection, future sampling and interpreter handoff see the same state at public step boundaries. It does not replay a synthetic internal trace.

`instant_transitions` counts actual kernel hits. Callback/walk/sample counters count actual work and therefore fall. Scheduled dispatches, accepted actions and publications retain their logical obligation counts. These distinctions must remain explicit in benchmark reports.

## Why this is equivalent within the admitted family

Each stored edge is the exact deterministic ordered execution for its complete source state and action. The normalization erases only origin choices that the allowed handlers cannot distinguish; ordered work, relative deadlines and stale revision comparisons remain. The closed family has no external callback receiver or synchronous host capability that can observe/intervene in intermediate execution. Presentation is retained separately. Exact lookup selects the edge only when the entire live source matches; otherwise general execution runs.

This is a local transition argument, not a proof that all possible circuit histories are in the table, or a proof that Graph already matches every interpreter history. Differential fixture tests remain required.

## Evidence and review targets

`backend/graph/tests/instant_machine.rs` covers:

- All eight prepared ADDER1BIT vectors, full mutable state and pending work after every step, real interpreter boundary outputs, and zero propagation work during all 16 response ticks.
- Every retained operation under shifted clocks and independent revision origins, including wrapping revision arithmetic.
- Stale external ticks and recovery to the kernel after their dispatch.
- 1,000 deterministic arbitrary control/tick actions against exact execution, including misses.
- Corrupt writes, targets, presentation, deadline and obligation records.
- Opt-in gating, cancellation without partial publication, deterministic rebuild and pass invalidation.

The existing 48-case adder matrix checks actual falling edges and full projected future state. Reset tests hand execution to the interpreter at cuts before and during reset. These now exercise the one-bit kernel while larger adders keep ordered proxies.

The JSON export contains the full program under optional `graph.instant_machine`. Owner arrays define the order of packed state vectors. Electrical bits are strength 0..3, powered 4, repeater lock 5, pending 6; wire bits are strength 0..3 and N/S/E/W two-bit sides starting at 4/6/8/10 (`Up=0, Side=1, None=2`). Piston bits are extended 0, head-present 1 and availability from bit 2. Action column zero is tick; remaining columns toggle `controls` in order. `null` means general execution. Revision offsets/increments are decimal strings, preserving all 64 bits in JavaScript.

## Next limit to address

Whole-selection enumeration is deliberately bounded. It must not be enlarged until it consumes CPU-scale state spaces. The next milestone is extracting small reusable causal kernels with explicit ports and their own state, then composing them in the existing region scheduler. That requires a separate boundary proof for intermediate cross-region reads and callback timing. Merely grouping ADDER11BIT into one region does not provide that proof.

## Measured first milestone (2026-10-15)

Five paired release runs compared the pinned pre-kernel ordered-proxy executable with the new executable. Both used `--graph-short --all-devices --cpu io/adder_1bit --graph-tier 3 --stable-instant-inputs --iterations 1`; before/after launch order alternated between pairs. Each sample used fresh worlds and the same prepared carry case, held every external lever, and timed only 512 tick calls. Preparation, compilation, activation, flush, observation, reset and interpreter continuation remained outside the timer.

| Execution | Median | Range |
| --- | ---: | ---: |
| Previous ordered composed proxy | 12.3471 ms | 12.1913–12.8203 ms |
| Closed transition kernel | 0.9324 ms | 0.9215–0.9745 ms |
| Interpreter in the new-binary runs | 13.1092 ms | See individual reports |

The ratio of before/after medians is **13.24×**; all five pairs favored the kernel. The median paired time change is −92.44%. This is a short, held-input ADDER1BIT workload, not a CPU throughput claim.

Every successful harness comparison checks interpreter observations, projected memory checkpoints, pending reset work and interpreter continuation. All ten reports passed. All five new samples used 512 transitions and zero actual callbacks, wire walks or piston samples; their ordered-proxy counterparts used 36,250 callbacks, 941 walks and 17,205 samples. Logical obligations stayed at 935 scheduled dispatches, 1,197 accepted actions and 1,190 publications.

The tighter compiler records **292 states, 365 edges and 813,748 estimated bytes**. Estimated lowered table storage increases from 94,792 to 908,644 bytes, because the general graph remains available for exact fallback. These are allocation estimates, not peak RSS. This storage cost makes the current whole-selection table an experimental prototype; it must not be replicated per adder across a CPU. A compact shared causal program is the intended follow-up, with full snapshots retained as proof data where useful.

Pinned baseline SHA-256: `48e4f6fabf0c563c2aeb9a78bdbbb8b7afb32c2e92c5637afaab21cb664b3608`.
New binary SHA-256: `02f541d7e35b747f855445211f944e831a17e31ec4ad4bc0973d4ea00db1acdf`.
Raw paired reports, binary, launch script and `summary.json`: `target/closed-instant-bench/`.

A separate ordinary O3 CPU_BubbleSort smoke run checked 100 ticks against the interpreter, including RAM, reset and continuation. Graph took 0.634030 s, interpreter 0.595557 s. The kernel recorded zero hits: this workload still uses the existing ordered graph. One pair establishes neither a CPU speedup nor a reliable regression estimate. The schematic normalization remains the same in-memory sign removal in both inputs; source schematics were not changed.

Verification: the final release Redpiler suite passed 336 tests with eight ignored; the debug suite passed 336 with eight ignored before the exploration-only reduction. The reduced compiler also passed all six focused release tests. Core tests/benches with `graph-trace` and server checks passed. Python loader tests passed 13/13 and viewer tests 21/21. Formatting is clean for changed Redpiler files; whole-core formatting reports existing drift in `plot/worldedit/safety.rs`. No commit, full CPU sort, broad fuzz campaign or CPU kernel extraction was performed.

## ADDER11BIT scaling check (2026-10-15)

The same pinned new executable ran five fresh-world pairs on `io/adder_11bits`, with the stable-input protocol and 512 timed ticks. All benchmark comparisons passed. The existing release test executable also reran all six stable-region tests successfully: these include the 40 prepared eleven-bit vectors, full state/future comparisons after each step, and reset handoff at cuts 0, 1, 2, 4, 8 and 16 for a carry-producing case.

| Execution | Median | Range |
| --- | ---: | ---: |
| Ordinary O3 | 38.6786 ms | 38.4782–40.2311 ms |
| Stable composed O3 | 38.6055 ms | 38.4853–39.3007 ms |
| Interpreter | 41.3726 ms | 41.3259–42.1135 ms |

The ordinary/composed median difference is about 0.19%, with overlapping ranges; no meaningful composition speedup is established in this run. These timings do **not** measure the new closed transition kernel: it records **zero hits** in every eleven-bit sample. This graph has 1,083 nodes, 142 actors and 295 wires, beyond the kernel's admission limits of 128 nodes and 16 actors. The manifest also declares 22 operand bits and a trigger, beyond the four-control limit. Enlarging the whole-selection enumeration would introduce millions of operand combinations before reset/timing state; it is not the scaling strategy.

Stable execution still performs 192,521 callbacks, 2,651 Turbo walks and 50,413 piston samples, with 2,550 scheduled dispatches, 2,907 accepted actions and 2,890 publications. It retains one composed region and one ordered proxy, but internal execution remains general. Estimated lowered tables are 942,436 bytes (ordinary O3: 863,692); these are not RSS measurements.

The next scaling milestone should combine compact kernel storage with extraction/composition of small causal programs, preserving shared trigger and carry timing. Simply increasing table limits or naming the entire adder one region cannot provide this acceleration. Current full-state snapshots remain necessary for exact lookup and cannot all be discarded as compile-time proof data until a replacement runtime matching/reconstruction scheme is certified.

Reports: `target/closed-instant-bench/adder11.json`, `adder11.log`, `adder11-correctness.log`. Command: `target/closed-instant-bench/cpus.exe --graph-short --all-devices --cpu io/adder_11bits --graph-tier 3 --stable-instant-inputs --iterations 5 --label closed-instant-adder11-scaling --output target/closed-instant-bench/adder11.json`.

No implementation, admission limits, or source schematics were changed for this check. No exhaustive eleven-bit input sweep or new sampled profile was run.
