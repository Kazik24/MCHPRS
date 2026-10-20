# Production Redpiler optimization levels

Supported production passes are included in ordinary builds. Select a tier
with `/rp compile -O1` through `/rp compile -O5`. `-O`, `-o` and `--optimize`
select O5. Compilation without an optimization flag still selects O0.
The same levels apply to `/rp analyze` and its graph export.

| Tier | Added work |
|---|---|
| O1 | Guard/update cleanup, inert and unhandled callback pruning, Turbo callback indexes and certified static dust paths. |
| O2 | State-guard simplification; wire queries, hops and callback specialization; compiled signal queries; packed whole-fixed classic execution, certified local classic nets and mixed-circuit fixed queries. |
| O3 | Local piston and reset specialization, built-in reset clocks, compiled wire shapes/capture recipes, private instant regions and proxies. |
| O4 | Instant boundary discovery, closed single-wire transfers, passive capture elimination, private BUD reads and connected FIFO-local event kernels. |
| O5 | Equivalent delayed-update coalescing; flat CPU predicates; dense and guarded instant transport; typed drives; minimized exact transition programs; value regions and ordered signal channels. Stable power-expression programs additionally require `--stable-instant-inputs`. |

Higher tiers retain lower-tier rules. O4 event kernels replace the O3
region/proxy builders, avoiding duplicate ownership and unnecessary compilation.
All rewrites run before dependent plan builders. Every pass retains its admission
checks; unsupported or over-budget regions keep general Graph execution.

O4/O5 require no stable-input promise. Event entries sample live Main/Quasi power
at the original acceptance cut and commit through existing state/future helpers.
They preserve original requests, clocks, reset transitions, contact selection and
deferred revisions. Their single-wire transfers retain ordered receiver delivery;
they do not contract arbitrary multi-wire nets to equilibrium.

The stable-input assumption remains explicit:

- `--stable-instant-inputs` explicitly promises stable external inputs through
  reset. It enables the older O3 composed-region mode and O5 stable power-expression
  programs. O5 without this flag retains exact ordered transport/replay and live
  input sampling.

Classic-redstone compilation is automatic from O2. After whole-fixed execution
declines, the local classic pass certifies each candidate region independently.
Unsuitable regions keep general Graph execution; no `--classic-redstone` switch
is needed or accepted. The explicit stable-input promise remains separate from
optimization level: topology may prove dependencies irrelevant in some cases,
but uncertain dependencies and their possible pending effects must be retained.

CPU matching selects AVX2 once when the CPU/OS supports it; other machines use
scalar word operations. No architecture-specific build flag is required.

## Maximum compilation

```text
/rp compile --optimize
/rp compile --optimize --stable-instant-inputs
```

`--optimize`, `-O`, `-o` and `-O5` select the same current maximum pipeline.
The first command supports changing inputs. Use the second only when external
inputs remain stable through the computation/reset interval. The flag does not
increase compilation budgets; the server controls those through player rank.

The O5 instant stages run in this build order after boundary/event compilation:

| Pass | Compiled representation |
|---|---|
| `optimize-instant-kernels-cpu` | Shared typed live reads and minimized Boolean predicates; scalar short circuits and AVX2 mask matching. |
| `compile-instant-transport` | Dense local wire geometry and ordered external receivers. |
| `compile-guarded-instant-transport` | Validated fixed admission masks with fallback for dynamic cuts. |
| `compile-instant-drives` | Typed source/contact power expressions. |
| `compile-instant-transition-programs` | Eligibility for bounded adaptive exact episodes, minimized into contiguous Capture/Select/SetPower/Call instructions. |
| `compile-instant-value-regions` | Bounded electrical-distance and source-region tables for certified nets. |
| `compile-signal-channels` | Exact ordered transition programs for supported channels. |
| `compile-stable-instant-logic` | Shared max/attenuation expressions for certified stable-input nets; installs no program without the explicit promise. |

This is compilation order, not runtime precedence. Runtime checks stable logic
and signal channels before transport; transport can replay a guarded learned
episode or use its exact walk. Pure reads remain at the required sampling cuts;
piston requests, reset scheduling and deferred publication keep their existing
state owners. Entry checks fail before optimized effects, and unsupported work
uses the existing path. A connected kernel does not mean the physical graph has
been completely removed.

The slower guard-packing experiment is excluded: five production pairs measured
8.38% more BubbleSort inference time. Known incorrect abstract-net/fixed-point
experiments also remain outside normal production selection.

Methods, algorithms and fresh results are documented in
[exact transition programs](notes/redpiler-pistons-v4/INSTANT_TRANSITION_PROGRAMS.md),
[the inference profile](notes/redpiler-pistons-v4/INSTANT_TRANSITION_PROFILE.md),
and [production FPU measurements](notes/redpiler-pistons-v4/FPU_CURRENT_INFERENCE.md).
The FPU random-input medians are 387.01 TPS interpreted, 1,281.73 TPS O5 and
1,347.51 TPS O5+stable, with physical-state, semantic-future and continuation
checks. These are switching-cost measurements; native output remains constant.

The old `experimental-instant-events`, `experimental-instant-wire-nodes` and
`experimental-cpu-kernels` Cargo names remain accepted as compatibility features;
they no longer change pass selection. Existing diagnostic pass IDs retain their
names for tooling compatibility. Production behavior is determined by O0–O5,
including builds using `--no-default-features`.

The known incorrect `unsafe-abstract-instant-nets` and `unsafe-turbo-fixed-point`
experiments remain excluded from normal tiers. The older whole-wave layer
algorithm remains available to explicit research tests; production selects the
FIFO-local event family instead.

Tests cover the complete tier/ordering matrix, actual O4/O5 compilation without
input promises, rapidly changing controls through unfinished reset work,
state/pending-work comparison against the interpreter and native continuation.
Existing capture, BUD, arithmetic, analog, imported-work and FIFO tests remain
part of the normal test suite. O4/O5 inspection fixtures contain the actual
selected event/CPU programs.

Historical tier-promotion verification: 555 redpiler tests, 33 loader tests, 39 viewer tests and the
command-completion test passed. Workspace checking with `--no-default-features`
and the ordinary production `cargo build --release --locked` passed. The release
build uses the configured fat LTO. Logs and binary identity are recorded under
`target/production-tiers/`; the server executable is `target/release/mroww.exe`.

The [earlier TPS report](notes/redpiler-pistons-v4/CPU_KERNEL_TPS_REPORT.md)
records the frozen opt-in build before this tier promotion. Its timings have
not been relabeled as measurements of the new production tier configuration.

The 2026-10-20 follow-up has 613 passing broad checks after excluding the slower
guard-packing experiment. The tier/alias test additionally checks every new O5
stage for all maximum flags, with and without the stable-input promise.
