# Compiled wire captures

2026-10-19. `compile-wire-captures` is a separate O3+ read-only pass. It
specializes the capture operation, preserving the instant at which each walk
first observes a site. This is not wire contraction or kernel composition.

## Representation and correctness

One table shared by all walks records each site's capture recipe: fixed
observation, electrical properties, bulb properties, wire state, piston pose or
guarded contact selection. Constant observations are stored in the program;
mutable values and contact variants are sampled at their original discovery cut.
The table is used by compiled hops and isolated Turbo capture. Without it, the
original capture path remains available.

Site identity, heading, visited state, repeated deliveries, source routes and
nested-walk scratch retain their original owners. Validation rederives every
recipe. Rewrites clear the table, lowering retains it, and JSON exposes each
site's recipe kind through the optional `wire_capture` field and `wire_captures`
capability. No new scheduler, pending-work owner or simulator is introduced.

The equivalence relation is equality of every captured `ObservedSite` at the
same discovery cut. Subsequent wire writes, admission tests, requests and
schedules therefore use the same operands and order. Differential tests cover
all 256 old/new analog-strength combinations, seeded cyclic dust, synchronous
nested propagation and changing piston/resource contact poses. Plan tests cover
stale bindings, cancellation, idempotence and rewrite invalidation.

## Five alternating release-harness pairs

These use a frozen release **test executable with fat LTO**, with recording
disabled, no competing compiler, identical prepared O5+stable graphs and only
the capture plan toggled. Initialization, projection and checks are outside the
execution timers. They do not measure production-server throughput.

| Workload | Baseline median | Captures median | Ratio | Retained table |
| --- | ---: | ---: | ---: | ---: |
| BubbleSort, 1,000 ticks | 6.484842 s | 6.408041 s | 1.012x | 12,528,264 bytes |
| FPU saved-trigger diagnostic, 256 ticks | 0.415016 s | 0.409545 s | 1.013x | 7,937,256 bytes |

The CPU candidate wins three pairs and loses two. These are small, noisy changes,
not evidence of a substantial speedup. Every pair preserves all execution counts,
final projected state and ordered future. The FPU diagnostic does not verify
floating-point arithmetic or changing-operand throughput.

Construction takes about 5.1 ms for 522,011 CPU sites and 3.7 ms for 330,719 FPU
sites, excluding validation. Of these, 421,656 CPU and 273,378 FPU recipes are
fixed observations. The current representation duplicates those values across
sites; sharing templates or avoiding inert discovery storage is a possible later
improvement, not a measured result of this pass.

## Independent checks

The frozen harness matches native for 100 CPU ticks and 256 FPU ticks, followed
by 16 native continuation ticks for each. A separate production benchmark
executable completes all 10,036 BubbleSort ticks, matching frozen interpreter RAM
on every tick, final/reset checkpoints and native continuation. Its single
62.018596-second sample is a correctness run, not a paired speed comparison.

Workspace/all-target and experimental-feature checks pass. Python loader tests
pass 29 tests; viewer tests pass 35. The Rust suite's initial fixture failure was
new export metadata; regenerated O3+ fixtures differ only in capture fields and
capability. A later O0 byte check exposed line-ending normalization in the fixture
files; restoring their generated LF bytes resolves that check.

Artifacts: `target/wire-captures/{cpu_bubblesort,fpu}-paired.json`, `summary.json`,
`paired.log`, `production-tests.exe`, `cpu-bubblesort.exe`, `cpu-full-sort.json`,
`source.diff`, `sources/` and `source-provenance.json`. These preserve the capture
milestone independently of subsequent composition work.
