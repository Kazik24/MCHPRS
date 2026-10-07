# CPU references

The shared [CPU harness](../../crates/core/benches/support/cpus.rs) loads exact
schematics and replays their protocols. [Integration tests](../../crates/core/tests/cpu_references.rs)
and [benchmarks](../../crates/core/benches/cpus.rs) use the same assertions.
These references freeze interpreted execution; they are not independent proofs
of the CPUs' instruction sets or universal compiler compatibility.

## Inputs and preparation

| CPU ID | Schematic under `test_data/` | Selection minimum | Start button, selection-local | Protocol |
| --- | --- | --- | --- | --- |
| `pm1_sort` | `PM1_SORT.schem` | `(8,8,8)` | `(187,35,72)` | Run 50,000 game ticks; press stop `(187,32,72)`; run another 100. |
| `anpu_pong` | `Q2CK@Q2CK_Anpu1_Pong_KBTV.schem` | `(8,8,8)` | `(122,68,56)` | Run 50,000 game ticks without paddle inputs. |
| `cpu_bubblesort` | `piston-research/cpu-bubblesort/CPU_BubbleSort.schem` | `(2,8,2)` | `(166,7,156)` | Prepare load/reset, then run 50,000 game ticks including automatic stop. |

The harness verifies schematic hashes before loading. It presses the actual
button with support notifications and release scheduling; it does not compile
the world. Replay disables live wall-time/chat-output throttling, preserving
command parsing and ordered output. Registry inputs are part of the baseline.

BubbleSort preparation enables Load program at `(150,17,119)`, waits for twenty
consecutive quiet ticks, presses Reset PC at `(166,7,158)`, and waits again.
Quiet means no scheduled ticks, piston events, or motions. The load lever stays
on; prepared logical time is checked as 364, and benchmark tick zero is that
prepared state. A 4,096-tick bound prevents an indefinitely unsettled preparation.

## Compared state

The files under `test_data/cpu-references/` contain:

- `pm1_sort.json`, `anpu_pong.json`, `cpu_bubblesort.json`: whole-world checkpoints
  at ticks 0, 10, 100, 1,000, 5,000, 10,000, 20,000, 30,000, and 50,000, plus
  PM1's 50,100 stop checkpoint and complete ordered tick-stamped command output.
- `anpu_screen.json`: every change in the 32 by 32 lamp projection, including the
  initial frame; descending Y rows and ascending Z columns at selection-local
  `x=140`, `y=58..89`, `z=38..69`.
- `anpu_bud_updates.json`: ordered sampling/accepted-event projections for the
  selected physical memory cells, including same-value samples. The dedicated
  BUD replay also checks the screen and CPU checkpoints.
- BubbleSort's `ram_trace`: every change in 64 physical 12-bit words. Missing or
  moving bases produce null words. Replay checks transient RAM, lower-bank
  retention, final sorted top bank, and the exact stop boundary.

Checkpoint hashes include block states and section coordinates, sorted block
entities, scheduled ticks in execution order, piston events/motion identities,
progress and phase, and command output. Item-NBT compound keys are canonicalized
because their ordering is irrelevant; list ordering and typed values remain
significant. Palette layout, network dirty flags, and packet caches are excluded.

`piston-shape-checkpoints.json` is an explicit block-hash migration over retained
original references. Before applying a patch, the harness asserts its original
hash. Other checkpoint fields and screen/chat traces are still checked. The
performance JSON and source sidecars are historical measurements/provenance,
not current throughput promises.

## Interpreter replay

The ordinary input check runs without the long episodes:

```sh
cargo test -p mchprs_core --test cpu_references --locked
```

Run the three frozen replays while excluding the baseline-capture test:

```sh
cargo test -p mchprs_core --test cpu_references --release --locked -- --include-ignored --skip capture_bubblesort_reference --test-threads=1
```

The additional ANPU physical BUD replay is separate:

```sh
cargo test -p mchprs_core --lib --release --locked redstone::piston::tests::bud_reference::anpu_interpreter_preserves_frozen_bud_updates_and_screen -- --ignored --exact
```

## Compiled sampled protocols

Physical and compiled tests select different observations. Current sequential
lowering supports a sampled runtime used by the RILAX memory tests and the
BubbleSort comparison. This does not imply that every CPU is admitted. The
existing ANPU rejection regression asserts rejection without mutation for its
entry/flag matrix. The separate opt-in `anpu_current_compile_admission` probe
records either admission or rejection; it is not a passing screen comparison.
`anpu_compiles_and_preserves_interpreted_screen` requires actual admission and
a complete 50,000-tick comparison. These are distinct checks with distinct claims.

```sh
cargo test -p mchprs_core --lib --locked redpiler::analysis::tests::research::rilax_material_diagnostics_and_compiled_sampling_preserve_memory_transactions -- --exact
cargo test -p mchprs_core --lib --release --locked redpiler::analysis::tests::research::bubblesort_compiled_sampled_protocol -- --ignored --exact
```

The BubbleSort opt-in test compares declared sampled behavior in both compiled
modes. Extra geometry/event comparisons are enabled with
`MCHPRS_CPU_COMPARE_GEOMETRY` / `MCHPRS_CPU_COMPARE_EVENTS`; inspect the test
before using these diagnostics as an equivalence contract. Physical moving-base
nulls and internal animation are not automatically part of an ideal logical
projection. Read [the compiled model](../REDPILER_MODEL.md) for the actual
deadlines, ordering, and admission limits.

## Benchmarking

```sh
cargo bench -p mchprs_core --bench cpus -- --iterations 3 --label current --output target/cpu-performance.json
cargo bench -p mchprs_core --bench cpus -- --cpu cpu_bubblesort --iterations 3
cargo bench -p mchprs_core --bench piston -- piston-cycle/independent --noplot
```

CPU benchmarks retain correctness assertions. By default only interpreter tick
calls are timed; loading, preparation, hashing, observation checks, and PM1's
manual stop tail are outside the measured region. When visual flushing is
explicitly enabled, its cost is included. TPS is game ticks per measured second.
Fixed active windows are declared in `CPUS`; the 50,000-tick average includes
idle tails. Record machine, source, profile, protocol, and client/visual settings
when comparing runs. Do not transplant historic TPS numbers into a model.

## Deliberate capture

For a complete BubbleSort reference, use the explicit
`capture_bubblesort_reference` test with
`MCHPRS_PISTON_RESEARCH_OUTPUT` set to a new file. For ANPU's BUD projection,
use `capture_anpu_bud_reference_to_new_file` with `MCHPRS_ANPU_BUD_CAPTURE`.
Both are ignored capture operations, separate from validation. Review baseline
changes against the declared semantics and retain original evidence.

The older [capture example](../../crates/core/examples/cpu_references.rs) requires
repair before use: its `Reference` initializer lacks `ram_trace`, and its button
helper assumes the original `(8,8,8)` origin. It cannot currently establish a
complete BubbleSort reference. Existing frozen replay commands remain the
validation procedure.
