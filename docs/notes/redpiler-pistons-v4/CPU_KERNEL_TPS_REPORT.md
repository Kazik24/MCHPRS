# Interpreter and maximum compilation TPS

These frozen measurements precede promotion to the normal production tiers.
Current activation is described in [O1–O5 production levels](../../REDPILER_OPTIMIZATION_LEVELS.md);
the historical build flags and timing evidence below remain unchanged.

Medians of five alternating release-harness pairs, measured on 2026-10-19:

| Build | Interpreter TPS | Maximum TPS | Throughput ratio | Measured ticks per pair |
|---|---:|---:|---:|---:|
| ADDER11BIT | 4,975 | 28,872 | 5.80x | 2,240 |
| COUNTER | 1,799 | 8,208 | 4.56x | 126 |
| Divider | 744 | 3,147 | 4.23x | 2,172 |
| FPU_MROWW, switching diagnostic | 168 | 584 | 3.48x | 4,096 |
| CPU_BUBBLESORT, complete sort | 58.7 | 194.2 | 3.31x | 10,036 |

All five complete BubbleSort pairs passed sorted RAM and state/future parity.
Median inference time is **171.058 s interpreter versus 51.689 s compiled**.

| BubbleSort pair | Interpreter seconds | Maximum seconds |
|---|---:|---:|
| 1 | 168.758871 | 54.037293 |
| 2 | 174.157991 | 51.185454 |
| 3 | 171.058029 | 51.689386 |
| 4 | 435.370045 | 52.003167 |
| 5 | 159.726406 | 44.020020 |

Pair 4 contains a large interpreter outlier; it is retained. A brief process
sample showed other desktop-app activity on the host. Its cause is unestablished;
the machine was not globally idle. Five-pair medians reduce this sample's effect,
but these are desktop-host measurements, not noise-free hardware limits.

Raw results: `target/cpu-kernels/maximum-vs-interpreter-tps.json` and
`maximum-tps.log`. Source snapshots, executable hashes and build flags are
recorded in `target/cpu-kernels/provenance-final.json` and `source-final/`.
The ratios compare median TPS, and include all compatible passes together;
they do not attribute the total improvement to CPU packing alone.

| Build | Packed actors | Graph preparation and passes, seconds |
|---|---:|---:|
| ADDER11BIT | 142 | 0.045 |
| COUNTER | 146 | 0.065 |
| Divider | 2,034 | 1.001 |
| FPU_MROWW | 7,804 | 4.631 |
| CPU_BUBBLESORT | 15,200 | 8.754 |

Each build retains one connected event kernel. Counter also retains 16 compiled
BUD read pairs and the original storage-write execution. Ordinary outside actors
and dynamic wire propagation remain in the graph.

The new CPU pass alone reduces FPU switching time by **7.81%** in five separate
graph-versus-graph pairs: median **6.396827 → 5.897059 s** over 4,096 ticks.
All five improve, with exact state, complete GraphFuture and work-count equality.
See [the isolated measurements and representation](CPU_KERNEL_EXECUTION.md).
This is a separate paired batch from the interpreter matrix; the matrix's
total speedups include the earlier optimization passes as well.

The comparison selects O5, stable instant inputs, the stateful event family,
CPU kernel packing, passive capture minimization, BUD reads and the certified
single-wire experiment. It retains all compatible default passes. The actual
selected pass names are recorded for each device. Known incorrect abstract-net
and Turbo fixed-point experiments are excluded. Old whole-wave layer execution
is superseded by the event family.

Hardware: AMD Ryzen 9 5950X, Windows x86-64, logical CPU 4 affinity `0x10`.
The test-harness release build uses `lto=false`, `codegen-units=256` and
incremental compilation. These are harness results, not production-LTO timings.
TPS is simulated full ticks divided by timed seconds; it is uncapped and
includes input actions. Loading, compilation, projection and validation are
outside timing. Graph inference uses an empty command-effect host containing
no original circuit blocks.

ADDER11BIT uses 40 pinned arithmetic histories, Counter both pinned histories,
and Divider all 11 histories, each from its own imported snapshot. FPU uses
32 seeded random vectors after four warm vectors and initialization, with the
green-wool lever in `FPU_MROWW.schem`. BubbleSort executes a complete 10,036-tick
sort after the frozen loader's preparation and the previously documented
in-memory wall-sign removal, applied equally to both sides. Every paired
history compares final blocks/entities and pending work; BubbleSort also
requires RAM sorted as `0..15`. FPU's constant native output restricts that row
to switching-cost TPS, not arithmetic throughput.
