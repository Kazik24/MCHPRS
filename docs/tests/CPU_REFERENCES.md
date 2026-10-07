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

## PM1 Sort correction (2026-10-08)

The corrected copy is [`PM1_SORT_FIXED.schem`](../../test_data/PM1_SORT_FIXED.schem).
The original CPU fixture, its duplicate in `instant-pistons`, `CPUS`, frozen
expectations, and the existing piston-shape migration remain unchanged.
The new [measurement file](../../test_data/cpu-references/pm1-sort-correction-results.json)
records evidence; validation never reads it as an expectation.

The CPU loader verifies the original SHA-256, zeros the clipboard's saved
offset `(0,1,1)`, and pastes the entire selection at `(8,8,8)`. The integration
test, CPU benchmark, and old reference capture share this loader. The separate
instant-piston manifest references the identical original binary, retains its
offset, and is excluded from the CPU admission acceptance set. Neither path
was redirected to the corrected copy.

Only these two entries were changed to air:

| Selection-local | CPU world | Saved state |
| --- | --- | --- |
| `(198,34,32)` | `(206,42,40)` | `moving_piston[facing=down,type=normal]` |
| `(198,35,32)` | `(206,43,40)` | `moving_piston[facing=down,type=sticky]` |

These are the fixture's only moving-piston entries. They form an adjacent,
down-facing orphan pair: no block entity supplies either one's moved state or
source identity, so identifying base versus head unambiguously is impossible.
There is no saved stationary base/head at either position; the next cell above
is gray wool and the cell below the pair is air. Nearby stationary pistons,
their heads, payloads, wool, dust, and supports were retained. Import creates no
scheduled ticks, piston events, or motions in this fixture, so no scheduled work
or entities needed removal. Pasting imports raw geometry before entities; motion
registration requires a moving-piston entity.

The original-fixture maximum-budget probe reproduces the reported rejection at
`(206,42,40)` in 0.583918 seconds. Removing only that first entry in memory
exposes the second moving-piston rejection at `(206,43,40)` in 0.562769 seconds.
This establishes why both orphan entries must be removed from the corrected copy.

[`fix_pm1_sort.py`](../../tools/fix_pm1_sort.py) preserves the decompressed NBT
outside the block-data array and its length, replaces exactly two palette
entries, and checks all decoded cells and entities. Original SHA-256:
`e338028d50a4400056e25037d1f43d37c08baed079edede6298f78b0a6341654`.
Corrected SHA-256:
`ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893`.
Dimensions remain `(235,202,182)`; Sponge v2/DataVersion 4325 and all metadata
and palette entries are preserved.

The [release runner](../../crates/core/examples/pm1_sort_correction.rs) checks
the full 50,000-tick start episode and 100-tick manual stop tail. Its first
sample replays original and corrected worlds side by side: command output and
quiet/nonquiet state agree at every tick, and the original orphan geometry
remains unchanged throughout. All 1,535 ordered, tick-stamped messages match
the frozen reference. `shut` occurs at tick 12,051; first empty scheduled/event/
motion queues occur at 12,055; manual stop output occurs at 50,007; final state
at 50,100 is quiet.

All ten whole-world checkpoints pass on every replay. The corrected block
hashes necessarily differ: the runner temporarily restores only these two
entries directly in chunk storage, verifies the entire checkpoint against the
original expectation (including the pre-existing shape migration), then removes
them again. Thus section block counts and state hashes are accounted for
explicitly. Entities, scheduler, piston state, lamps, chat, and message counts
retain their original assertions. No expectations were regenerated.

Compilation uses whole-plot bounds, the imported scheduler, and unchanged
admission checks. Times below are release compiler-call durations, including
analysis until rejection, excluding fixture loading and verification:

| Flags | Normal budget (1x), seconds | Maximum budget (8x), seconds |
| --- | ---: | ---: |
| Default | 0.062295 | 1.658488 |
| `-O` | 0.059865 | 1.614157 |
| `--assume-instant` | 0.059883 | 1.706730 |
| Both | 0.060090 | 1.680619 |

All eight reject without changing the world or publishing an active compiler.
Normal analysis stops at its 65,536-piston limit: the fixture has 66,021
stationary bases (65,840 sticky plus 181 ordinary). Inspection in the analyzer's
chunk/section/cell order locates the 65,537th base at world `(230,62,48)`, local
`(222,54,40)`; the compiler's budget error itself has no coordinate.

At maximum budget, all four configurations reject with:

```text
logical piston admission failed: unsupported payload minecraft:gold_block at BlockPos { x: 49, y: 18, z: 60 }, owned by piston BlockPos { x: 49, y: 20, z: 60 }; expected a redstone block or supported fixed conductor
```

This is local payload `(41,10,52)`, below the matching sticky head at
`(41,11,52)` and extended, downward sticky base at `(41,12,52)`.
`instant::outputs::supported_payload` does not admit gold blocks; material
validation runs before clock/sampling recognition. Neither flag bypasses it.
This payload was retained. No configuration is admitted, so compiled observable
equivalence and compiled TPS cannot be measured.

Interpreter reference timings are recorded for ticks 1 through 12,051 in three
release samples. Only `tick_interpreted` calls are timed: loading, compilation,
checks, and the manual stop tail are excluded. No runtime visual flushing is
performed; paste-time flushing is preparation outside timing. Sample 1 also
interleaves the original-world validation, outside its timed calls. The host is
Windows x86-64, AMD Ryzen 9 5950X (16 cores/32 threads), Rust 1.98.1, release
with fat LTO, source commit `a0c5f293039981b365e058eae5c8a6d989104ee4`.
An unrelated build was running on the shared host, so these are observations,
not isolated throughput promises. The JSON retains individual seconds and TPS.

| Release sample | Active seconds | Active interpreter TPS |
| --- | ---: | ---: |
| 1 (with interleaved original validation) | 63.602189 | 189.47 |
| 2 | 62.682345 | 192.26 |
| 3 | 60.816953 | 198.15 |

Median active interpreter throughput is 192.26 TPS. Compiled throughput is
unavailable because every configuration rejects; no speedup is claimed.

Recommended next step: retain the validated copy and use the interpreter.
Separately investigate gold-payload support with its electrical/geometry
semantics and tests, then retry at maximum budget to expose subsequent blockers.
Do not broaden admission or replace circuit materials solely to admit PM1.

```sh
py tools/fix_pm1_sort.py
cargo run -p mchprs_core --release --locked --example pm1_sort_correction
cargo run -p mchprs_core --release --locked --example pm1_sort_correction -- --probe-original
```

### PM1 compiler generalization follow-up

The preceding correction measurements are historical. With the subsequent
compiler changes, gold is admitted and compilation reaches a clock-model
boundary. The corrected schematic and original frozen expectations are unchanged.
The new [measurement record](../../test_data/cpu-references/pm1-sort-generalization-results.json)
contains all eight release compiler-call results.

`instant::outputs::supported_payload` now admits modeled, opaque solid cubes
whose material is inert: no block entity, neighbor-update handler, or comparator
override. Redstone blocks retain their explicit support. Unknown registry states
remain excluded because their geometry uses fallback properties. Zero-power
targets retain their identity and use the existing shared conditional dust-shape
rules; powered target states remain excluded. This supports gold, iron, planks,
clay, and other qualifying modeled materials without replacing fixture blocks.

An ordinary piston with an empty head slot does not transport the block two
cells ahead: extension stops at the empty slot and retraction does not pull.
Recognition now permits that stationary context, including the glowstone below
PM1's ordinary generator at world `(20,25,44)`. Executable boundaries preserve
occupied stationary cells instead of treating them as empty aliases. Actual
moving payload and occupied-head guards remain intact.

The redpiler analysis regression suite passed **86 tests**, with **8 ignored**
and no failures. Added coverage exercises the material guards, conductor outputs,
and stationary context through native extension/retraction and compiled counter
sampling, optimization, flush, and reset. Existing invalid-head and unsupported
ordinary-payload rejection tests also pass. No frozen expectation was regenerated.

| Flags | Normal budget (1x), seconds | Maximum budget (8x), seconds |
| --- | ---: | ---: |
| Default | 0.060968 | 1.604379 |
| `-O` | 0.059923 | 1.563935 |
| `--assume-instant` | 0.059964 | 1.569873 |
| Both | 0.060449 | 1.571598 |

Loading, verification, and the release build are excluded from these durations.
All attempts leave the input world unchanged and the compiler inactive. Normal
budget still rejects at the 65,536-piston limit described above. At maximum
budget, all four configurations now reject with:

```text
logical piston admission failed: clocked instant execution needs one owned generator; found 66 (first two at BlockPos { x: 20, y: 25, z: 44 } and BlockPos { x: 184, y: 28, z: 90 })
```

These positions are selection-local `(12,17,36)` and `(176,20,82)`. The count
is 66 **candidates** in one connected region, identified by an ordinary downward
piston with a downward observer above it. It does not establish that all 66
are independent clocks. `ClockedProgram` stores one owned clock and one sampled
memory bank; the direct executor uses one shared sampling deadline, advanced
in six-tick steps. Relaxing the count check would therefore require execution
semantics, not just broader geometry admission. The check remains intact.

No PM1 configuration is admitted, so compiled observable equivalence and
compiled TPS remain unavailable. The earlier three interpreter measurements
and full 50,100-tick fixture comparison remain the interpreter evidence; they
were not rerun or relabeled as compiled results for this follow-up.

Recommended next step: trace these candidates in the interpreter to distinguish
clock generators from notification/sampling roles, capture their update and
sampling order, and check whether the region can safely partition. Use that
evidence to decide whether independent sampling domains are needed before
changing the single-clock executor. Continue using the interpreter for PM1.

```sh
cargo test -p mchprs_core --lib --locked redpiler::analysis::tests:: -- --test-threads=1
cargo run -p mchprs_core --release --locked --example pm1_sort_correction -- --probe-fixed
```
