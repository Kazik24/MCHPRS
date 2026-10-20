# Production Redpiler optimization levels

All certified passes are included in ordinary production builds. Select a tier
with `/rp compile -O1` through `/rp compile -O5`. `-O`, `-o` and `--optimize`
select O5. Compilation without an optimization flag still selects O0.
The same levels apply to `/rp analyze` and its graph export.

| Tier | Added work |
|---|---|
| O1 | Guard/update cleanup, inert and unhandled callback pruning, Turbo callback indexes and certified static dust paths. |
| O2 | State-guard simplification; wire queries, hops and callback specialization; compiled signal queries; packed whole-fixed classic execution and mixed-circuit fixed queries. |
| O3 | Local piston and reset specialization, built-in reset clocks, compiled wire shapes/capture recipes, private instant regions and proxies. |
| O4 | Certified local classic nets, instant boundary discovery, closed single-wire transfers, passive capture elimination, private BUD reads and connected FIFO-local event kernels. |
| O5 | Equivalent delayed-update coalescing and flat CPU kernel programs with minimized masks, contiguous reads and AVX2/scalar matching. |

Higher tiers retain lower-tier rules. O4 event kernels replace the O3
region/proxy builders, avoiding duplicate ownership and unnecessary compilation.
All rewrites run before dependent plan builders. Every pass retains its admission
checks; unsupported or over-budget regions keep general Graph execution.

O4/O5 require no stable-input promise. Event entries sample live Main/Quasi power
at the original acceptance cut and commit through existing state/future helpers.
They preserve original requests, clocks, reset transitions, contact selection and
deferred revisions. Their single-wire transfers retain ordered receiver delivery;
they do not contract arbitrary multi-wire nets to equilibrium.

Two existing flags remain compatible:

- `--stable-instant-inputs` explicitly promises stable external inputs through
  reset for the older O3 composed-region mode. Higher tiers do not assume it.
- `--classic-redstone` requests certified local classic nets at O2/O3. O4/O5
  attempt that pass automatically after whole-fixed execution declines.

CPU matching selects AVX2 once when the CPU/OS supports it; other machines use
scalar word operations. No architecture-specific build flag is required.

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

Verification: 555 redpiler tests, 33 loader tests, 39 viewer tests and the
command-completion test passed. Workspace checking with `--no-default-features`
and the ordinary production `cargo build --release --locked` passed. The release
build uses the configured fat LTO. Logs and binary identity are recorded under
`target/production-tiers/`; the server executable is `target/release/mroww.exe`.

The [earlier TPS report](notes/redpiler-pistons-v4/CPU_KERNEL_TPS_REPORT.md)
records the frozen opt-in build before this tier promotion. Its timings have
not been relabeled as measurements of the new production tier configuration.
