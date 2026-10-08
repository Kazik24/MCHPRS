# TEST_ schematic inventory and timing report

Downloaded 18 schematics from read-only `urmom:/srv/mchprs/data/schems` at 2026-10-08T18:32:29.893618+00:00. Each file matches the remote byte count and SHA-256 in [download-manifest.json](download-manifest.json). All are Sponge v2, DataVersion 4325. The inspection sidecars retain block states, block entities, sign text, offsets, and local coordinates.

Captures ran at repository commit `d39175768898db0a8c2ecff9cd691a78e95bd680` with uncommitted work present. Schematics were pasted at selection minimum `(40,30,40)` using their saved loader offsets. Schematics do not retain scheduled tick or piston queues, so the interpreter starts from the pasted state and is observed for 12 idle ticks. Then all saved lever combinations are applied through the normal lever callback and followed for 12 ticks. Every lever also receives a one-game-tick pulse followed by 12 settle ticks. The ordered data records logical tick, phase, piston samples, accepted piston events, callbacks, scheduled work and dynamic redstone state.

The compiler matrix covers default, `-O`, `--assume-instant`, and both flags at budget multipliers 1 and 8, using the full loaded plot bounds. A successful compile establishes admission only; compiled/native replay equivalence was not measured. Coordinates below are selection-local `(x,y,z)`.

## Per-schematic results

| File | Size | Levers | Sticky / ordinary pistons | Observers | Compile result, 8 cases | Native lever sweep |
|---|---:|---:|---:|---:|---|---|
| `TEST_BUD_1.schem` | 4×8×13 | 2 | 1 / 1 | 2 | reject 0/8: clocked BUD at `(3, 3, 8)` needs a downward redstone-block storage mechanism | mask 1: 62 samples/4 applies; mask 2: 7 samples/0 applies; mask 3: 41 samples/5 applies |
| `TEST_BUD_2.schem` | 3×8×13 | 2 | 1 / 1 | 1 | reject 0/8: ordinary sampling generator at `(1, 4, 8)` is watched by observer at `(1, 5, 8)`; timed movement notifications require a proven shared-clock contract | mask 1: 23 samples/4 applies; mask 3: 25 samples/5 applies |
| `TEST_CODER_1.schem` | 6×8×14 | 1 | 4 / 0 | 3 | pass only with `--assume-instant` (4/8) | mask 1: 62 samples/8 applies |
| `TEST_DEC.schem` | 4×9×13 | 2 | 8 / 0 | 4 | pass 8/8 | mask 1: 168 samples/16 applies; mask 2: 105 samples/16 applies; mask 3: 258 samples/32 applies |
| `TEST_DEC2.schem` | 5×7×14 | 0 | 10 / 1 | 10 | reject 0/8: observer clock has no independent sampling output | idle only; no lever |
| `TEST_INSTANT_3.schem` | 3×5×14 | 1 | 1 / 0 | 1 | pass 8/8 | mask 1: 26 samples/4 applies |
| `TEST_OR1.schem` | 3×5×13 | 2 | 2 / 0 | 2 | pass 8/8 | mask 1: 41 samples/4 applies; mask 2: 36 samples/4 applies; mask 3: 51 samples/8 applies |
| `TEST_OR2.schem` | 4×8×13 | 2 | 2 / 0 | 0 | pass 8/8 | mask 1: 67 samples/4 applies; mask 2: 78 samples/4 applies; mask 3: 63 samples/5 applies |
| `TEST_OR_3.schem` | 1×8×12 | 0 | 2 / 0 | 2 | pass 8/8 | idle only; no lever |
| `TEST_PISTION.schem` | 1×5×18 | 1 | 1 / 0 | 1 | pass 8/8 | mask 1: 25 samples/4 applies |
| `TEST_PISTION_2.schem` | 1×5×18 | 1 | 1 / 0 | 1 | pass 8/8 | mask 1: 16 samples/4 applies |
| `TEST_WEIRD.schem` | 1×8×13 | 1 | 1 / 1 | 1 | reject 0/8: ordinary sampling generator at `(0, 3, 6)` is watched by observer at `(0, 5, 6)`; timed movement notifications require a proven shared-clock contract | mask 1: 57 samples/7 applies |
| `TEST_WEIRD_3.schem` | 3×6×13 | 1 | 2 / 0 | 1 | pass 8/8 | mask 1: 50 samples/8 applies |
| `TEST_WEIRD_INSTANT.schem` | 2×5×13 | 1 | 1 / 1 | 1 | reject 0/8: ordinary sampling generator at `(1, 3, 8)` is watched by observer at `(0, 3, 7)`; timed movement notifications require a proven shared-clock contract | mask 1: 50 samples/7 applies |
| `TEST_WEIRD_INSTANT_2.schem` | 1×5×14 | 1 | 1 / 1 | 1 | reject 0/8: ordinary sampling generator at `(0, 3, 8)` is watched by observer at `(0, 3, 6)`; timed movement notifications require a proven shared-clock contract | mask 1: 42 samples/7 applies |
| `TEST_WEIRD_INSTANT_3.schem` | 1×8×13 | 1 | 1 / 1 | 1 | reject 0/8: ordinary sampling generator at `(0, 6, 9)` is watched by observer at `(0, 6, 7)`; timed movement notifications require a proven shared-clock contract | mask 1: 53 samples/7 applies |
| `TEST_XOR.schem` | 3×6×12 | 0 | 3 / 0 | 3 | pass 8/8 | idle only; no lever |
| `TEST_XOR2.schem` | 4×6×14 | 2 | 3 / 0 | 2 | pass 8/8 | mask 1: 63 samples/4 applies; mask 2: 80 samples/4 applies; mask 3: 104 samples/12 applies |

Native counts are raw callback-level operations across 12-tick episodes; repeated samples and accepted requests are not unique pose changes. `mask 0` is the saved lever pattern. Mask bit order is the lever order recorded in the JSON.

## Findings

- **Admitted in all 8 compiler cases:** `TEST_DEC`, `TEST_INSTANT_3`, `TEST_OR1`, `TEST_OR2`, `TEST_OR_3`, `TEST_PISTION`, `TEST_PISTION_2`, `TEST_WEIRD_3`, `TEST_XOR`, and `TEST_XOR2`. These still need compiled/native ordered comparisons.
- **`TEST_CODER_1`:** passes only with `--assume-instant` (4/8). Default proof rejects ordinary piston `(3,5,3)` because no observer reset or payload-following response is verified.
- **`TEST_BUD_1`:** sticky memory piston `(3,3,8)` faces south. All 8 cases reject because clocked BUD storage currently requires a downward redstone-block cell. This is the clearest orientation-coverage example.
- **`TEST_BUD_2`:** ordinary generator `(1,4,8)` is watched by observer `(1,5,8)`, so all modes reject its unproven movement-notification timing. `TEST_WEIRD` and all `TEST_WEIRD_INSTANT*` files have the same class of blocker: ordinary piston head changes are watched by observers. Their generator/observer pairs are `TEST_WEIRD` `(0,3,6)` / `(0,5,6)`; `TEST_WEIRD_INSTANT` `(1,3,8)` / `(0,3,7)`; `_2` `(0,3,8)` / `(0,3,6)`; `_3` `(0,6,9)` / `(0,6,7)`. The ordered traces show why a settled pose edge alone cannot replace these callbacks.
- **`TEST_DEC2`:** 10 sticky pistons, one ordinary piston, and 10 observers; no levers. All modes reject with “observer clock has no independent sampling output.” Only its saved idle state was timed; its intended controls are unclear.
- **No lever protocol:** `TEST_OR_3` and `TEST_XOR` also have no levers. They compile, but native testing only confirms 12 idle ticks and does not exercise their logic.
- **One-tick pulses:** all 21 tested pulses end without any piston `Sample`/`Applied` operation and restore the original piston poses. Wire, repeater, observer, comparator and copper-bulb frames are also retained to inspect where each pulse is filtered.
- **`TEST_PISTION`:** contains three waxed copper bulbs and two comparators. Their properties change in the native frame records. The block implementation preserves copper bulbs as unknown registry states with helper-based lit/powered properties, so this state is part of the required runtime behavior.

## Recommended next work

1. Add compiled/native ordered comparisons for the ten workloads already admitted, using the saved lever combinations and current timing JSON as the native reference.
2. Generalize BUD storage geometry for the south-facing cell in `TEST_BUD_1`, validating payload placement and near/far positions.
3. Model scheduled observer delivery from moving ordinary piston bases and heads for `TEST_BUD_2` and the `TEST_WEIRD*` cases before relaxing their guard.
4. Get the intended stimulus/control protocol for `TEST_DEC2`, `TEST_OR_3`, and `TEST_XOR` before declaring their dynamic behavior characterized.

## Artifacts

- `download-manifest.json`: verified remote metadata and hashes.
- `inspection/*.json`: decoded schematic contents.
- `timing-full.json`: ordered native traces, per-tick state, input combinations, pulse episodes, and compiler results.
- [reproducible opt-in capture test](../../../crates/core/src/redpiler/analysis/tests/research/test_prefix_timing.rs).

The opt-in capture test passed for all 18 schematics. Compiler checks took about 0.000–0.005 seconds each in the local test profile. Rust formatting and `git diff --check` pass. The full core suite was not rerun; this adds an ignored research harness and no production code. No frozen expectations changed.
