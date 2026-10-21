# Step 08: scaling, performance, rollout and removal

Status: planned; requires complete lifecycle correctness. Heavy validation belongs here,
after review. Do not promise a speedup from plan counts or fewer stored guards.

## Correctness matrix

| Workload | Required evidence |
| --- | --- |
| Primitive `instant-pistons-io` and `test-prefix-20261008` fixtures | Buffer/invert/AND/OR, shared payload, blocked/inhibited paths, observation effects and cycle limits. |
| ADDER1BIT | All physical actors accounted for; all eight arithmetic inputs and every pinned preparation/history; actual kernel output/effect execution. |
| ADDER11BIT | All actors accounted for, all pinned vectors, carry chains, alternating preparation order, repeated reset and native handoff. No input-width cap. |
| Divider | Published compiled/declined region census; all existing declared input/output phases and internal arithmetic/memory observations. |
| Counter | Exact BUD memory sequence and qualifying updates; memory remains ordinary. No claim the complete counter is combinational. |
| CPU_BubbleSort | Mixed domains, RAM/register/output checkpoints and pending continuation; start with 100 active ticks, then completed sort only after the short gate passes. |
| ANPU | Correct ordinary behavior and no unexplained regression or forced instant classification. |
| FPU | Complete compile/ownership census, then declared operation cases and observation windows using an existing or newly documented manifest. A census alone is not arithmetic validation. |

Reuse current manifests including `piston-research/fixtures/fpu_divider.json` and
`cpu_bubblesort.json`. Read their actual `fixture` fields; do not assume old schematic names.
Keep the repaired CPU SHA-256 and saved sign intact. If a support sign still blocks baseline
admission, use one explicitly reported identical in-memory variant for both executors;
that is a benchmark restriction, not permission to repair circuits silently.

Retain the fixed-point adder failure as a regression. Compare an atomic-frame test with
the same native atomic-frame adapter, and physical preparation tests with the physical
protocol. Do not silently substitute one protocol for another to obtain agreement.

## Work-replacement gate

- Every advertised kernel must actually execute in the measured response interval.
- All accepted internal wires/Turbo walks and reset-wire propagation are bypassed. Remaining
  walks/callbacks are attributed to boundary adapters or uncompiled domains.
- Report physical actors recognized, actors covered, kernels executed and declined reasons
  separately. A fallback result does not satisfy an all-actor fixture target.
- ADDER1BIT target: at least 75% fewer total walks and 50% fewer callbacks over identical
  preparation + activation + reset. These are review targets, not existing measurements.
- Show minimized expressions, effect counts, local hot-state bytes, cold witness bytes and
  peak/retained memory separately. Do not turn an actor's every intermediate pose into state.

## Runtime measurements

Use the current `graph_short.rs`/`cpus.rs` harness; first check its command-line routing.
Example commands (run only when this stage is approved):

```text
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component io/adder_1bit --graph-tier 4 --stable-instant-inputs --iterations 5 --output target/layered-adder1.json
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component io/adder_11bits --graph-tier 4 --stable-instant-inputs --iterations 5 --output target/layered-adder11.json
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component fpu_divider --graph-tier 4 --stable-instant-inputs --iterations 5 --output target/layered-divider.json
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component counter_basic --graph-tier 4 --iterations 5 --output target/layered-counter.json
cargo bench -p mchprs_core --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --graph-tier 4 --iterations 5 --output target/layered-cpu.json
cargo bench -p mchprs_core --bench cpus --locked -- --cpu anpu_pong --graph-short --graph-tier 4 --iterations 5 --output target/layered-anpu.json
```

CPU/Counter/ANPU do not automatically satisfy a whole-device stable-input promise. Their
kernel entries require local certified frames; otherwise leave them general and report it.
Do not add a blanket promise to an invalid workload just to make synthesis run.

Compare old O3/general Graph, the layered path and interpreter with the SAME contract,
fixtures, inputs, clock/publication setup and observation windows. Use fresh worlds and
alternate executor order per pair. Keep individual samples, paired changes, medians/ranges,
actual compiled coverage, source revision and build/profile metadata.

Compilation/loading are outside runtime timers. Report both active-tick cost and full
execution-episode cost including preparation/input/activation/reset work, so moving expensive
work into untimed start stimulation is not a speedup. The harness already has episode fields;
verify their implementation and correct its stale report labels rather than building a new
harness. Separate projection/flush/handoff costs and optionally end-to-end server TPS.

Use uninstrumented release for speed and existing external sampled profiling for attribution.
Profile one tier/workload per capture. Inclusive timings overlap. No custom nested timer
framework, per-event strings or always-on counters beyond the agreed lightweight work counts.

An initial useful target is a repeatable >=2x improvement for an actually contracted adder
transaction; >=10x is the project ambition, not a guaranteed property. If there is no material
gain, identify remaining executed work before adding another optimization family.

## Final checks and retirement

Run registered focused tests first, then complete debug/release redpiler suites and native
reference contracts. Discover tests with `--list` and report zero-match mistakes explicitly.
Run core/server/bench checks, touched-file formatting and diff checks. Add generated bounded
small-circuit histories and deterministic seeds; distinguish a finite campaign from a proof.

After review, activate the new analysis/compiler at their intended tiers and stable/local
protocol. Execute the deletion audit in [migration](../MIGRATION.md): remove old discovery,
stable composition and proxy route VM where superseded. Keep normal Graph fallback and
complementary exact wire/reset optimizations. Search every registration, validator, export,
constructor, test and benchmark before declaring a type removed.

## Final acceptance

Publish a concise report with actual correctness, coverage, removed runtime work, speed and
memory results per workload. List every unsupported region and contract deviation. Do not
mark the project complete if the adder still runs through physical propagation, if only
idle reset time improves, or if faster results depend on changing the stimulus protocol.
