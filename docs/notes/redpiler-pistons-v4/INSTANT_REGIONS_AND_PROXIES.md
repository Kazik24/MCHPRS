# Instant region boundaries and private-cell proxies

This batch implements the first two steps: explicit ownership/event boundaries and an executable proxy for the private, self-resetting cell family. It does not replace an entire adder with a truth table or remove the connecting wire walks. Shared-resource mechanisms remain general execution inside explicitly identified regions.

## Representation and pipeline

At O3, after built-in reset compilation:

1. `identify_instant_regions.rs` records ownership closure and query/event boundaries.
2. `compile_instant_proxies.rs` certifies private two-contact cells and compiles their internal route instructions.
3. Structural validation independently rederives the boundaries and every instruction from the original graph.
4. Lowering discards the replaced routes' raw delivery arrays, retains the certified instructions, and installs direct route/proxy and resource/owner bindings.

The shared graph-only reset witness discovery is in `passes/utils/instant_mechanisms.rs`. Boundary and certificate inference are in `analysis/instant_regions.rs`; typed data is in `program_graph/instant_regions.rs`. Runtime dispatch is in `backend/graph/instant_proxy.rs`. Each optional pass has its own file; the backend does not discover patterns.

Unknown enabled passes invalidate both boundaries and proxy certificates. Original draft routes remain intact, so invalidation safely restores general execution. Disabled passes preserve metadata. Repeated identification/compilation is deterministic and idempotent. Cancellation publishes no partially inferred table.

## Ownership and boundary

A region is an ownership group containing one or several piston mechanisms. Ordinary power and callback connections cross its boundary without merging the owners. It is a starting boundary for composition, rather than a claim that the entire connected arithmetic circuit has been contracted.

Union-find joins actors sharing possible occupied contacts, resource positions, or reset drivers. Resource identities participate in closure; reset consumers are joined with the actor watched by their reset clock. A region may therefore contain an actor that does not independently match the reset witness. Such a region is conservatively retained.

Boundary tables expose all actor/resource/reset state, all owned contact/component sites, power-link inputs and external power readers, incoming/outgoing route occurrences, and external wire/observer readers. Occurrences reference original route ordinals, preserving duplicates, guards, directions, and captured/live operands. Pending obligations retain their original actor/resource/reset IDs and deadlines. All state remains observable; this batch does not infer that an unlisted intermediate state can be discarded.

Large source tables are scanned once per inference, rather than once per region. Temporary indexes and retained boundary references grow with the input tables. The boundaries are compile-time data; they are dropped during lowering. Their nested vectors and proxy instructions are included in the existing memory estimates.

For pinned ADDER1BIT, the 13 actors form six regions:

| Region actors | First proxy status |
|---|---|
| P0, P2, P5, P8, P10 | Shared resources/reset dependencies; general execution |
| P1, P4, P7, P9 | Shared resources/reset dependencies; general execution |
| P3 | Private-cell proxy |
| P6 | Private-cell proxy |
| P11 | Private-cell proxy |
| P12 | Private-cell proxy |

ADDER11BIT has 142 actors in 65 ownership regions; 43 qualify as private-cell proxies. A higher region count reflects a larger circuit, not a different definition of region.

These IDs describe the pinned fixture only. Production matching uses graph relationships, never fixture names or coordinates. Recognition still covers all 13 actors; executable proxy coverage is four, not thirteen.

## Private-cell certificate

A candidate must have one actor, one resource, and one self-reset clock. The actor is sticky with a two-contact Near/Far lane. Its resource has exactly those two distinct allowed contacts; Base is separate. The clock is a compiled built-in reset watching this actor's base, with no output callback route. Ownership closure proves that foreign resources, heads, and bases cannot occupy this lane.

Selected non-Turbo routes compile into an ordered instruction sequence:

- `dispatch`: execute the original callback, including its guard and captured/live operands.
- `sample`: for a live self-update, evaluate the original guard and directly sample the actor at the original sequence position.
- `arm_reset`: for an unconditional live notification with the reset's qualifying direction, directly schedule the existing clock at delay 1, normal priority, at the original sequence position.

Captured, conditional, wrong-direction, or otherwise uncertain reset notifications retain general dispatch. The proxy requires at least one proved direct clock-arm occurrence. No outward boundary event is absorbed. Each instruction retains its original delivery as a validation witness; JSON exports the original routes plus ordinal-aligned operation lists.

## Correctness argument

Each direct instruction replaces the exact primitive reached by the original handler under its certificate. `arm_reset` is the qualifying branch of `arm_builtin_reset`, including scheduler deduplication, priority, and insertion position. `sample` calls the same sampling routine with the same live power reads and guard, preserving requests, cancellation, and acceptance revalidation. Instructions run synchronously one at a time, so nested callbacks complete before the next instruction, just as before.

The private-resource binding removes only foreign occupancy searches proved empty. It still tests the owning head's live availability, resource position/enabled state, and selected contact block. Resource identity, revisions, detachment, drop/recapture, and empty extensions remain unchanged.

This is a refinement over arbitrary admitted histories, not a protocol assumption. The existing global FIFO, deferred publications, early retraction, reset scheduler, and handoff state remain authoritative. There is no second simulator, duplicate mutable state, guessed reset timer, physical Motion, or atomic-gate shortcut.

Test recordings project absorbed internal notifications through the original event witness so interpreter semantic traces remain comparable. Runtime callback counters count actual callback dispatch only. Tests separately require reduced dispatch counts and equal state, future work, and outward events; projected traces alone are not the correctness oracle.

## Validation and measurement

Both full Redpiler suites passed: **323 passed, 7 ignored** in debug and release. Core tests/benches and server checks passed. The Python loader passed 12 tests and the graph viewer passed 20. The ADDER1BIT golden exports were regenerated for O0 through O3; older exports without the additive tables remain accepted.

The four pinned private proxies compile 28 routes into 52 direct samples, 8 direct clock arms and 14 retained dispatch instructions. These are static instruction counts, not dynamic work savings. The finite checks include 1,280 private-contact comparisons across every logical availability, head/extended/enabled combination and permitted resource position, and 896 route/state comparisons with alternate poses and pending reset work. The real adder suite checks all one-bit inputs and eleven-bit manifest vectors against interpreter state and ordered operations, cancellation/hold/rearm histories, rotations/translations, and early/late reset continuation.

Tests cover private-family admission, shared ownership closure, outward event preservation, uncertain-ingress rejection, deterministic reruns, corrupted witnesses, pass invalidation, cancellation, finite logical-state checks, and real adder/interpreter histories including reset continuation.

The pinned pre-batch executable is `target/instant-proxy-bench/baseline.exe`, SHA-256 `d821805556a4e778c9105e2b8fc0b2d890a9783701142ef4e1e3fa69e58197e9`. It already contains compiled wire hops. New measurements must compare against this baseline, not claim the preceding hop improvement again. Setup/compilation are outside runtime measurements.

## Next boundary

The larger shared regions need their own joint resource/reset transition contract before contraction. Connecting wires remain ordinary ordered Turbo execution (with compiled hops). Proving and absorbing those internal wire effects is a subsequent optimization; this batch does not claim a substantial gate-level or CPU speedup merely from four compiled cells.

## Completed private-proxy benchmark

The final private-proxy checkpoint keeps the delivery helper inlined. Five paired release runs per CPU/adder workload and one per larger device checked interpreter observations, memory checkpoints, pending reset work and continuation. All 18 pairs passed. CPU uses 100 active ticks; other devices retain their distinct bounded protocols.

| Workload | Baseline median | Private-proxy median | Median paired change | Pairs |
|---|---:|---:|---:|---:|
| CPU_BUBBLESORT | 0.931146 s | 0.916638 s | -3.01% | 5 |
| ADDER1BIT | 0.007305 s | 0.007148 s | +0.62% | 5 |
| ADDER11BIT | 0.050497 s | 0.050227 s | -0.54% | 5 |
| Divider | 0.408214 s | 0.383446 s | -6.07% | 1 |
| Counter | 0.633782 s | 0.593493 s | -6.36% | 1 |
| ANPU | 0.791540 s | 0.798552 s | +0.89% | 1 |

The ratio of tier medians is not the median paired change. CPU ranges overlap (baseline 0.799045-1.009056 s; proxy 0.880944-1.007449 s), with one substantial proxy regression. This does not demonstrate a repeatable large speedup. One-pair results are characterization only. Individual reports, logs, binaries and `summary.json` are in `target/instant-proxy-bench`.

CPU callbacks fall from 1,760,070 to 1,703,695. Other counts stay unchanged: 20,005 scheduled ticks, 32,505 wire walks, 302,784 piston samples and 27,884 accepted actions/deferred publications. Estimated lowered immutable tables grow from 89.7981 to 94.2361 MiB (+4.438 MiB), not an RSS measurement. No full sort or new sampled profile ran for this checkpoint.

The opt-in composed-adder follow-up is described in [STABLE_INSTANT_REGION_CONTRACT.md](STABLE_INSTANT_REGION_CONTRACT.md). Its assumption and measurements must remain separate from the arbitrary-history results above.
