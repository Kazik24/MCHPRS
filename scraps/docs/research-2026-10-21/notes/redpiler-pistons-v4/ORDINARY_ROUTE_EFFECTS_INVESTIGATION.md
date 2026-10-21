# Ordinary-route Shape elimination: rejected after the real CPU census

Follow-up: the [runtime sampling census](PISTON_SAMPLE_REUSE_CENSUS.md) is now
complete. Conservative per-episode reuse covers 1.75% of all piston samples;
the next candidate targets captured-profile checks rather than a sampling cache.

The 2026-10-20 candidate compiled ordinary route delivery ordinals separately
from the original rows used by Turbo. It omitted explicit Component Shape
notifications to non-wire nodes, retaining wire shapes, piston base/head
notifications, classic-bound ordinals and proxy ownership. Guards and captures
would keep their original timing for surviving effects.

The candidate compiled, but its BubbleSort admission census disproved the
performance premise:

| O5 + stable inputs, budget 8 | Census |
|---|---:|
| Original static route rows | 1,162,416 |
| Routes with an eliminated effect | 0 |
| Eliminated rows | 0 |
| Additional route-table storage estimate | 3,623,536 bytes |

O1's `prune-unhandled-updates` already removes ordinary non-wire Shape
identities. Its retained Turbo rows remain necessary for receiver selection;
this CPU offered no additional eligible Shape rows. This census is about
static program rows, not timed callback invocations or a CPU-time percentage.

The full-sort test intentionally required nonzero elision and failed before
execution. No paired production timings were run, and this is not a completed
correctness or performance result. The candidate and all its production
registration/runtime/storage changes were removed. The previous production
executor and measured results remain unchanged.

Evidence is retained under `target/ordinary-routes/`: the failed admission gate
is `cpu-correctness-initial.log`, and `rejected-source/` preserves the experiment.
The frozen pre-candidate executable is `baseline-production.exe`, SHA-256
`f815f5eb789db5f2284999052b9054a2de6e815b6b79723aa4ccbf15de7a019d`.

The next admission measurement should count repeated **qualifying** piston
samples inside certified queue-only transport episodes. Merely comparing total
samples with actions cannot establish how many reads are redundant. A possible
rewrite must preserve the first request/source, retain callbacks, and discard
its memo across nested walks, action acceptance, scheduler work and phase/tick
changes. Ordered receivers need a separate proof. Existing guard evaluation
remains expensive, but the previous DNF guard-packing attempt was also slower;
neither rejected idea should be reintroduced solely from its static appeal.
