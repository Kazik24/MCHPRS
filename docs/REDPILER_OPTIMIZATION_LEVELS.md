# Redpiler optimization levels

Select `/rp compile -O1` through `/rp compile -O6`. `-O`, `-o`, and `--optimize`
select O6; no optimization flag selects O0. The same tiers apply to `/rp analyze`
and graph export. Higher tiers include the lower-tier pipeline, subject to
admission checks and compilation budgets. Unsupported work retains Graph execution.

| Tier | Main additions |
| --- | --- |
| O0 | General graph compilation and execution. |
| O1 | Guard/update cleanup, inert callback pruning, callback indexes, static dust paths. |
| O2 | Compiled signal queries, wire specialization, fixed classic execution and local classic nets. |
| O3 | Piston/reset specialization, reset clocks, wire captures, private instant regions. |
| O4 | Instant boundaries, single-wire transfers, private BUD reads, connected event kernels. |
| O5 | Delayed-update coalescing, compiled predicates, transport, typed drives, transition programs and signal channels. Stable power expressions require the stable-input promise. |
| O6 | Experimental logical clock/device execution and admitted finite pull waves; ordinary piston power queries can also be compiled while physical events remain. |

## Stable inputs and version scope

In **tagged v0.6.0**, O6 implies stable inputs through reset and includes the
qualified COUNTER and settled FPU runtimes. No additional flag is required for
that release. Internal glitches, previews, and reset propagation may disappear
under the admitted logical contract; consequential outputs and clock cadence remain.
Interpreter handoff for those logical devices remains experimental.

The **original development checkout** has a different policy: the
`--stable-instant-inputs` promise remains explicit, including for O6 finite pull
waves. Its one-shot firing runtime is experimental work beyond the release.
Use source and the [architecture](REDPILER_ARCHITECTURE.md) to identify the policy
of the checkout being built; do not assume that a branch contains every release change.

```text
/rp compile --optimize
/rp compile --optimize --stable-instant-inputs
```

Promise stable inputs only when external inputs remain stable through the required
computation/reset interval. The flag does not increase compilation budgets.
O4/O5 without that promise retain ordered event execution and live sampling.
Classic-redstone compilation is automatic from O2; no separate switch is needed.

Compiling power queries does not eliminate piston callbacks, scheduler work, or
physical state. BubbleSort and ANPU retain physical execution despite compiled
queries. A plan's actor coverage does not measure removed runtime work or imply
whole-device logical execution. See the [development plan](REDPILER_DEVELOPMENT.md)
for state/event compilation and verification requirements.

## Compilation reports and inspection

Completion chat reports mode, stage time, graph size, expensive stages, and plan
coverage. Inspect a prepared pipeline without activating it:

```text
/rp analyze --graph --optimize
/rp analyze --graph --optimize --stable-instant-inputs
```
`/rp analyze` checks compilation and reports aggregate execution coverage.
`--graph` prepares the selected graph pipeline without activating an executor;
it reports graph counts and aggregate local instant evidence where available.
Local evidence is not a complete lifecycle proof. Optimization tier and
`--stable-instant-inputs` retain their normal meanings.

For individual details, point at a component within ten blocks and run
`/rp inspect` (or `/rp i`) after resetting Redpiler. The asynchronous, read-only
report shows incoming power/QC paths, update routes, shape checks and, for
pistons, shared payload counts and local instant evidence or its decline reason.
Counts include guarded alternatives; they are not timing warnings. Inspection
prepares a snapshot graph and may take time on large builds.

`--updates` and `--unproven` were removed from analysis in favor of targeted
inspection. `--timing` was removed because independent input probes could not
establish whole-circuit timing correctness. Native nanotick regression tests
remain; there is currently no whole-circuit timing checker.

Historical stage inventories, measurements, and earlier verification counts are
preserved in the [previous guide](../scraps/docs/research-2026-10-21/REDPILER_OPTIMIZATION_LEVELS_SNAPSHOT.md).
They describe their recorded revisions. Use the development plan's benchmark
discipline for new measurements.
