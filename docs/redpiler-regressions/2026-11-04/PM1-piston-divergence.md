# PM1 tick-10 piston divergence

The saved slice reproduces the original PM1 signal/movement mismatch: blocks agree through tick 9; at tick 10 the interpreter removes the redstone block at `(155,10,124)`, while Rust retains it. The adjacent wire at `(156,10,124)` is power 0 in the interpreter and 15 in Rust. No production engine fix is included.

This is the **smallest verified slice found**, not a minimal causal circuit. It retains the original Start history because stationary local restarts lost the triggering signal.

## Reproduce

From the repository root, run:

```powershell
cargo test -p mchprs_core pm1_saved_piston_slice_matches_interpreter --lib -- --ignored --nocapture
```

Expected result on the investigated engine: **FAIL**, first block mismatch at tick 10, `(155,10,124)`, Rust `RedstoneBlock` versus interpreter `Air`. The ignored regression asserts equality, so a future fix should make it pass. It loads only `pm1-piston-slice.json.gz`; the original PM1 schematic is unnecessary for this test. It also asserts that the interpreter performs the original payload/wire transition, to detect a fixture that loses its stimulus.

The fixture contains absolute bounds `(8,8,32)..(210,65,132)`, 176,279 non-air block states, 318 block entities, zero initial pending ticks, and Start `(195,43,80)`. It is 691,070 bytes compressed. Initial state is stationary; no moving-piston state was discarded. The test preserves block entities, imports pending ticks in their original order, clicks Start in both backends, flushes all compiled block state, and compares every cell after each of ten ticks. The generated slice has 452 differing cells at tick 10; the full build had 764 because the slice excludes downstream regions.

SHA-256 of the compressed fixture: `09c9fa86d93e70a7a97b7d382a67db3861a6fc9771155f9bbe73c0d1a76fd461`.

Fixture format is gzip-compressed JSON: absolute positions paired with numeric block-state IDs, serialized block entities and pending entries. Those numeric IDs are coupled to this repository's current block registry; regenerate the fixture if that registry changes.

## Slicing procedure and results

The generator loads `PM1_SORT_FIXED.schem` (SHA-256 `ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893`), copies blocks/entities within inclusive bounds into an empty world, retains in-bounds pending entries without sorting, and compares interpreter execution against Rust compilation. Snapshots containing moving pistons are rejected. Temporary raw fixtures and logs are under `target/`; `slice-attempts.json` summarizes the trials.

| Initial-history bounds | Non-air blocks | Result |
| --- | ---: | --- |
| `(8,8,8)..(240,80,180)` | 428,614 | Original first mismatch at tick 10 |
| `(90,8,75)..(200,45,145)` | 52,232 | Boundary mismatch at tick 1; original transition lost |
| `(8,8,8)..(240,45,180)` | 229,107 | Original payload transition lost |
| `(8,8,8)..(210,80,145)` | 293,848 | Original first mismatch at tick 10 |
| `(8,8,8)..(210,65,145)` | 251,265 | Original first mismatch at tick 10 |
| `(8,8,32)..(210,65,145)` | 202,274 | Original first mismatch at tick 10 |
| `(65,8,32)..(210,65,132)` | 147,148 | Original transition lost |
| `(65,9,38)..(210,62,132)` | 125,284 | Original transition lost |
| `(65,8,32)..(210,65,145)` | 168,694 | Original transition lost |
| `(8,8,32)..(210,65,132)` | 176,279 | **Saved; original first mismatch at tick 10** |

Twelve stationary crops at interpreter stages 6–9, around the shared payload, matched each other but retained the redstone block in both backends: their pending queues were empty and the external trigger was cut. Expanding a stage-9 crop to `(8,8,90)..(180,40,145)` also lost the trigger. A 138-block experiment replacing the upstream source with a scheduled button release produced a different movement mismatch while both removed the original payload. It was rejected as a boundary artifact, not presented as the PM1 reproduction.

To regenerate the final slice, with the original schematic available:

```powershell
$env:MCHPRS_PM1_INITIAL = '1'
$env:MCHPRS_PM1_SLICE_BOUNDS = '8,8,32,210,65,132'
cargo test -p mchprs_core pm1_staged_piston_slices --lib -- --ignored --nocapture
```

The generator writes `target/pm1-slices/pm1-0-custom.json`, including the selected interpreter trace. Compress that JSON with standard gzip to replace the saved fixture. `target/pm1-slice-final-generate.log` records the verified generation run; `target/pm1-slice-standalone.log` records the standalone regression.

## What is established

`full-tick10-piston-trace.json` records the full interpreter: North piston `(155,10,126)` samples powered, then unpowered, requests and accepts retraction, and pulls the far redstone block. Its East neighbor `(153,10,124)` remains powered in that trace. Therefore the first position in a coordinate-sorted difference list must not be called the first causal event.

There are upstream missed retractions too: `(119,10,126)` retains its payload at `(121,10,126)` in Rust, keeping the wire toward `(138,10,126)` powered; that stage feeds the North piston through the source at `(140,10,126)`. Differences continue up the control staircase and other pistons around x74–100, y20–40. The next causal investigation should compare event/power-update order upstream, rather than assume shared-payload ownership is the root cause.

Final generation passed in 11.51 seconds. The standalone saved regression was actually executed with `target/debug/deps/mchprs_core-000d6c7be101014f.exe pm1_saved_piston_slice_matches_interpreter --ignored --nocapture`: it failed as expected in 13.09 seconds, first mismatch tick 10 at `(155,10,124)`, left `RedstoneBlock`, right `Air`.

Validation used revision `fd1747fb9c1283242c4f4eb112078c350cf94efb` plus concurrent workspace edits. Only the focused generator and saved regression were run for this task; no benchmarks or production changes. This report does not establish the first internal pending/FIFO divergence, the precise faulty engine operation, or global minimality of the fixture.
