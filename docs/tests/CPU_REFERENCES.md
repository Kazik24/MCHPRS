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

### Fixed container support

The furnace-specific reset support exception now uses one shared
`analysis::families::fixed_container` predicate for structural recognition,
observer certification, and ownership exclusion. It requires a solid conductor,
no neighbor-update handler, and a container entity whose type matches
`ContainerType::from_block`. This admits stationary barrels alongside furnaces.
Of the four `/container` options, chest is nonconducting and hopper has a powered
enabled-state update; neither qualifies for this exception. Their existing
ordinary comparator support is unchanged. Container payloads remain unsupported.

Regression coverage uses `/container`'s `ItemStack::container_with_ss` generator
at all strengths 0 through 15, both optimization modes, and both I/O modes.
Container material, complete inventory, comparator output, and lamp state agree
with the interpreter on every tick of the reset-support case. Conditional dust
above the container cannot replace its analog comparator main input. Flush and
reset preserve the inventory, and mismatched entities, mobile supports, and
pending reset work still reject. The analysis suite passed 87 tests with
8 ignored; PM1 admission and throughput were not rerun for this container change.

### PM1 observer-generator configuration investigation

The reported default-flags/8x-budget rejection is reproducible with the checked-in
corrected PM1, at source commit `0a5d6459dce4ed23daaf6f36ad9fbe8b8e4cfdbe`.
This copy reports 66 candidates, rather than the user's 63. Four paste origins
`(8,8,8)`, `(16,8,8)`, `(8,8,16)`, and `(16,16,16)` all report 66: their
error coordinates translate, while the candidate count remains unchanged.
The exact 63-candidate live snapshot has not been reproduced. The isolated
checkout avoided unrelated incomplete edits in the shared workspace.

The [diagnostic evidence](../../test_data/cpu-references/pm1-sort-clock-analysis.json)
contains candidate-local geometry, power sources, update interfaces, native
tick/phase-stamped operations, and translated compiler results. The opt-in
[diagnostic test](../../crates/core/src/redpiler/analysis/tests/research/pm1_clocks.rs)
reuses the native piston recorder. It passed in release, including comparison
of all 25 ordered command messages in the first 500 ticks with the unchanged
original frozen reference. Analysis and rejected compiler calls preserve the
input worlds. No circuit blocks or compiler admission checks were changed.

All 66 candidates are ordinary downward pistons with a matching saved head and
a downward observer above. 64 have stationary glowstone at Far; two have air.
Their own observers feed power back through quasi-connectivity. Each also has
a distinct moving redstone-payload control source; some controls reach the base
through dust rather than an adjacent payload. They share a region containing
66,015 pistons. These are data-controlled observer-feedback generators, not
66 proven copies of the single ordinary-torch-controlled counter clock.

The bounded native run exercised 12 candidates and accepted 391 events
(195 extensions and 196 retractions). The other 54 did not move during these
500 ticks; that does not establish their roles in later execution. Examples
below use selection-local coordinates:

| Generator | Observed native movement |
| --- | --- |
| `(114,35,143)` | Retract 4, extend 7, retract 10, extend 13; initially a six-tick cycle. Extend 301 is followed by retract 306, a five-tick gap that shifts the phase; subsequent edges are again three ticks apart. |
| `(64,38,108)` | Retract 4, extend 7, repeated three-tick edges through its last extension at 295; no further movement through tick 500. |
| `(176,20,90)` | Burst pairs at 306/309, 318/321, 330/333, etc.; later gaps vary, so its sampling cannot be represented as an always-running common clock. |
| `(113,30,31)` | One retract/extend pair at 456/459 within the captured window. |

`clocked::recognize` counts the observer/piston shape before investigating
control sources or sampling ownership. Its subsequent validator also requires
one ordinary torch control and prohibits clock control that depends on stored
data. The direct executor carries one `clock` and one shared `next_sample`
deadline, advancing a whole memory bank every six ticks. Removing the count
guard would neither classify these controls nor preserve their individual
bursts, stop/restart phases, or sampling fanout.

A read-only diagnostic call to the existing independent classifier, with no
owned periodic clock, additionally rejects at world `(37,28,51)`, local
`(29,20,43)`: `BUD ... has no independent sampling source; data changes alone
cannot update memory`. This is a west-facing, retracted sticky piston. This
probe is not executable admission or proof of invalid circuit behavior; it
shows that routing all candidates around the legacy clock recognizer is not
a complete solution either.

Next step: isolate a data-controlled generator together with its downstream
sampling interfaces, trace an enable/burst/stop/restart episode, and establish
delivery ordering between sources. Then generalize generator control and
per-source event scheduling/ownership. The capture preserves each candidate's
event tick and phase, but does not establish ordering between different
candidates inside the same phase. Full 50,100-tick equivalence and compiled
TPS remain outstanding for any future admitted PM1 implementation.

```powershell
$env:MCHPRS_PM1_CLOCK_OUTPUT = 'target/pm1-clock-analysis-new.json'
cargo test -p mchprs_core --lib --release --locked pm1_observer_clock_candidates -- --ignored --nocapture --test-threads=1
```

The output path must be new; this diagnostic never overwrites frozen references.

### Fresh PM1 compilation save, 2026-10-08

Downloaded `PM1_FIXED_COMPILATION_1.schem` again from the existing read-only
SSH source `/srv/mchprs/data/schems`. The remote modification time was
`2026-10-08T09:37:36.522127+00:00`; the download was verified at
`2026-10-08T09:41:07.972009+00:00`. The
[download manifest](../../test_data/piston-research/pm1-compilation-1-20261008-fresh/download-manifest.json)
records SHA-256 `cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c`.
The earlier schematics and frozen references remain unchanged.

This save has dimensions `(207,234,177)`, loader offset `(0,1,176)`, and
65,762 piston bases. Matching 218 unique block entities identifies a
selection-local translation `(-31,+32,-3)` from `PM1_SORT_FIXED.schem`.
The CPU harness deliberately clears the clipboard offset and pastes at `(8,8,8)`;
its translated start/stop positions are world `(156,67,69)` / `(156,64,69)`,
selection-local `(148,59,61)` / `(148,56,61)`. The
[geometry comparison](../../test_data/piston-research/pm1-compilation-1-20261008-fresh/geometry-comparison.json)
records 4,861 changed cells within the new selection and 10,202 differences
including excluded old cells. Many are saved dust shapes and piston/lamps states.
This revision cannot use the earlier two-cell geometry-hash normalization.

The extended opt-in diagnostic passed in release: one test, 61.72 seconds,
excluding its release build. Its
[evidence](../../test_data/piston-research/pm1-compilation-1-20261008-fresh/clock-analysis.json)
records all flag/budget attempts, four paste origins, generator controls,
potential direct base/head recipients, and the first 500 native ticks.
Native operations now include a sequence index within each tick, preserving
cross-generator ordering of recorded samples and accepted events.

| Flags | Normal budget | Maximum budget (8x) |
| --- | --- | --- |
| default | piston budget exceeded, 0.096 s | 51-generator rejection, 2.590 s |
| `-O` | piston budget exceeded, 0.102 s | 51-generator rejection, 2.336 s |
| `--assume-instant` | piston budget exceeded, 0.110 s | 51-generator rejection, 2.445 s |
| both | piston budget exceeded, 0.109 s | 51-generator rejection, 2.609 s |

Times cover only the compiler call, excluding loading, hashing, release builds,
and native replay. All rejected calls preserve the input and leave the compiler
inactive. Each translated default/8x attempt also reports 51. The downloaded
revision therefore reproduces the same error type, with 51 rather than the
previously reported 55. First candidates are local `(69,52,97)` and `(31,62,28)`.
All 51 are in one region of 65,756 pistons. 49 have stationary glowstone at Far;
two have air.

The user's stated role is BUD-switch update generation. The static inspection
finds 12 candidates with a potentially notified adjacent sticky base/head;
the other 39 lack such direct adjacency, so this inventory does not establish
their downstream observer/dust fanout. For example, generator local
`(188,80,12)` has an empty ordinary head, its own observer feeding QC, an
adjacent moving-redstone-block control at `(187,80,12)`, and a head adjacent to
sticky base `(187,79,12)`. Its head notification can recheck that base; it is not
an independently ordinary-torch-controlled shared bank clock. This candidate
does not move during the bounded capture, which does not establish it is unused.

Four candidates move during the first 500 ticks, with 203 accepted edges:
`(69,52,97)` has burst pairs 108/111, 120/123, 132/135;
`(84,67,139)` starts at 168/171 and repeats pairs every 30 ticks;
`(83,67,140)` accepts 142 edges; `(33,70,105)` accepts 32, stopping after 97.
These differing controls and bursts cannot be represented by the existing
single bank deadline. Native ordered output also differs from the old frozen
prefix: 43 messages versus 25, with program download completion at tick 95
instead of 293. This observation does not establish a defect in the circuit;
the save has changed selection and initial state. No expectations were regenerated.

A separate read-only call to the independent sampling classifier rejects at
world `(17,59,8)`, local `(9,51,0)`:
`BUD ... has no independent sampling source; data changes alone cannot update memory`.
It is a downward, extended sticky piston. The classifier accepts ordinary empty
piston pose notifications and selected stationary independent dust writers;
it does not yet establish a sampling source for this actor. This is a diagnostic
probe, not a second compiler rejection or proof of invalid circuitry.

The smallest general direction is to reuse qualified notification delivery,
the existing timed observer scheduler, and the existing BUD sampling path.
Generator count alone must not select a shared-clock model. Each delivered
update rechecks its recipients against current data; data changes without a
delivered update must still preserve BUD storage. Before changing admission,
trace the unclassified writer at `(9,51,0)` and establish observer/dust fanout
for the burst generators. Neither compiler admission nor runtime was changed
in this investigation. This revision has no compiled equivalence or TPS result;
its full 50,100-tick replay remains outstanding.

```powershell
$env:MCHPRS_PM1_CLOCK_OUTPUT = 'target/pm1-compilation-1-analysis-new.json'
cargo test -p mchprs_core --lib --release --locked pm1_compilation_1_update_generators -- --ignored --nocapture --test-threads=1
```

### PM1 base-to-head route and cancelled request, 2026-10-08

Downloaded the compilation save again with
`tools/download_piston_research.py --names PM1_FIXED_COMPILATION_1.schem` into
an ignored diagnostics directory. Verification at
`2026-10-08T10:21:22.322877+00:00` returned the same 243,256 bytes and SHA-256
`cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c`.
Original schematics and frozen expectations were preserved.

The new [route diagnostic](../../crates/core/src/redpiler/analysis/tests/research/notification_routes.rs)
hash-checks that save and inventories the target's data, notification ports,
neighbor reset family and shared payload. The first 500 ticks of the unchanged
native start protocol produce no samples or motion at local `(9,51,0)` or its
neighboring source. This does not establish inactivity for the full protocol.

A separately loaded copy is deliberately perturbed; it is not an original
protocol equivalence result. Raw removal of QC data at local `(9,53,0)` leaves
the target extended with power false. Removing source control at `(7,51,0)`
and rechecking source base `(8,50,0)` then starts a native movement/reset
episode. At tick 2, the source's retraction notifies the target's existing head
at `(9,50,0)` from West. This queues the target's retraction with data false.
Later callbacks restore power before that event executes; the event is never
applied and the target stays extended. Further head callbacks occur when the
source settles and extends. The source shares its redstone payload with the
horizontal piston at `(6,48,0)` and has an observer above at `(8,51,0)`.

Notification discovery now records piston-base changes independently of power,
including whether the receiving head must exist. The classifier diagnostic
names source `(8,50,0)`, receiver `(9,50,0)` and the absent ordered movement/reset
timing certificate. This fixes the missing route inventory; it does not certify
an atomic instant sample. Replacing the episode with one settled pose edge
would retract the target incorrectly in this perturbation.

The next execution change must preserve notification, queued request, power
recheck and accepted commit separately, using the existing scheduler. It must
also prove or preserve the observer/reset and shared-payload episode. Compiler
admission, full 50,000-tick equivalence and active-window TPS remain outstanding
for PM1. No frozen expectations were replaced with perturbation results.

```powershell
$env:MCHPRS_PM1_ROUTE_OUTPUT = 'F:/rustrepos/MCHPRS/target/pm1-route-validation-20261008/route-native-new.json'
cargo test -p mchprs_core --lib --locked pm1_missing_sampling_route_inventory_and_native_trace -- --ignored --nocapture --test-threads=1
```

The output must be new. It records ordered callbacks, event phases and the
explicit `is_original_protocol: false` perturbation separately. A small ordinary
native regression also verifies that a base-to-head route samples an existing
head but cannot sample after that head disappears.

Final release validation of this change repeats the flag/budget matrix:

| Flags | Normal budget | Maximum budget (8x) |
| --- | --- | --- |
| default | piston budget exceeded, 0.071 s | 51-generator rejection, 1.778 s |
| `-O` | piston budget exceeded, 0.068 s | 51-generator rejection, 1.829 s |
| `--assume-instant` | piston budget exceeded, 0.079 s | 51-generator rejection, 1.816 s |
| both | piston budget exceeded, 0.072 s | 51-generator rejection, 1.790 s |

Times cover compiler calls only. The complete matrix/500-tick diagnostic took
47.14 seconds after a separate fresh release build; loading, preparation and
that build are excluded from the table. Every rejection preserves input and
leaves compilation inactive. The independent classifier now gives the explicit
base-to-head timing diagnosis above. The unchanged native prefix remains 43
messages versus 25 in the earlier frozen prefix, as recorded for this newer
save before these changes.

The full core run passes 544 tests with 16 ignored using an isolated configuration
with the documented 67,108,864-block WorldEdit cap. The local core configuration's
4,194,304-block cap rejects the existing 8,639,540-block WorldEdit fixture; no
server settings or fixture were changed to run validation. Final release checks
also pass all five BUD presentation tests, nine client packet tests, and both
small notification regressions. Scoped documentation links and Rust formatting
checks pass. Large-bank display cost and PM1 compiled TPS are unmeasured.
The final release route diagnostic passes in 29.30 seconds and again records
zero target-region callbacks in the original 500-tick prefix and the separately
labelled tick-2 request cancellation. Release checks use
`target/redpiler-release-validation` because the existing release artifacts
contain older block/network APIs; a fresh build succeeds without code changes
to those packages.
