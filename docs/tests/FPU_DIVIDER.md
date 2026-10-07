# FPU_DIVIDER checks

The unchanged `FPU_DIVIDER.schem` compiles with **`/rp compile`**, without `--assume-instant`. It reports 16 missing-head warnings. The compiled output pulse sequence matches the interpreter for the saved operands, changed operands, single and repeated triggers, and interpreter handoff. Ordinary compilation and `--optimize --io-only` are both covered by the [runnable regression](../../crates/core/src/redpiler/analysis/tests/research/fpu_divider.rs).

This is an FPU subassembly, not a complete IEEE divider. The author describes mantissa inputs whose first bit has weight 1 and subsequent bits have weights 1/2, 1/4, etc. The exact legal input range and complete output encoding remain uncertain. Tests preserve the circuit's measured behavior rather than substitute host-language division.

## Binary and placement

| Item | Value |
| --- | --- |
| Source | Read-only download from `urmom:/srv/mchprs/data/schems/FPU_DIVIDER.schem` |
| Binary | [FPU_DIVIDER.schem](../../test_data/piston-research/fpu-divider/FPU_DIVIDER.schem) |
| Verification | [Download manifest](../../test_data/piston-research/fpu-divider/download-manifest.json) |
| SHA-256 | `19097d93d55e1c759ef37bbb261168eb79e51d86666282f9b2b775275211bbdb` |
| Size and format | 6,701 bytes; Sponge v2; DataVersion 4325 |
| Dimensions | 73 × 26 × 49 |
| Legacy WorldEdit offset | `[-1,-23,-44]` |
| Rust loader offset | `[1,23,44]` |
| Test selection | World minimum `(40,30,40)`, maximum `(112,55,88)` |
| Test paste anchor | World `(41,53,84)` |

Circuit positions below are **selection-local**. Add the selection minimum for world coordinates. Import uses the production schematic loader and a strict paste with no notifications or implicit settling. No synthetic heads or other repairs are applied.

## Ports and protocol

| Port | Local position | Meaning |
| --- | --- | --- |
| Trigger | Lever `(1,23,44)` | ON is idle; ON → OFF starts computation |
| A | Ten levers `(6,23,5+4*i)`, `i=0..9` | Red-wool bank; weight `2^-i`; OFF is one |
| B | Ten levers `(6,23,3+4*i)`, `i=0..9` | Blue-wool bank; same encoding |
| Output | Ten repeaters `(18+6*i,5,48)`, `i=0..9` | Negative pulses, read in increasing X order |

Saved A is `1000000000` and B is `1100000000`, representing **1 and 1.5** under the author's input convention. The ten construction buttons are not used as operand or trigger controls.

All output repeaters are saved powered: this is reset, not a retained numeric result. A logical output one is a repeater's negative pulse. The original interpreter response is:

| Game tick | Powered repeater pattern | Negative output pattern |
| --- | --- | --- |
| 0–2 | `1111111111` | `0000000000` — reset |
| 3–7 | `1101010101` | **`0010101010`** |
| 8 onward, before the next response | `1111111111` | `0000000000` — reset |

This contains the reported repeating `10101010` fraction pattern. Mathematically, `1 / 1.5 = 2/3 = 0.(10)` in binary; the finite eight-bit fraction `0.10101010` is `0.6640625`. The complete ten-port output's binary point or possible status bits are not independently established, so assertions use the observed bit pattern rather than impose a numeric scale on every port.

A four-game-tick OFF pulse followed by ON produces one response and returns to idle. Holding the trigger OFF repeats responses: the interpreter's first starts are ticks 3, 46 and 90. Turning it ON stops repetition. Operand-change cases set A/B with the trigger ON, wait 64 ticks, then trigger.

## Assertions and arithmetic limits

The [protocol manifest](../../test_data/piston-research/fixtures/fpu_divider.json) declares eleven cases. The regression runs all cases under both compilation settings, checks ordered output changes, resets the compiler, then compares another 48 interpreter ticks. It checks head geometry against the physical run after reset and that warnings are cleared on reset.

| Prepared A | Prepared B | Observed negative response, increasing X |
| --- | --- | --- |
| 1 | 1.5 | `0010101010` |
| 1 | 1 | `1000000000` |
| 1.5 | 1 | `1100000000` |
| 0.5 | 1 | `0111111111` |
| 0 | 1 | `0111111111` |
| 1.75 | 0.5 | `1111111111` |
| 1 | 0 | `1111111111` |

These characterize the assembly, not general mathematical division. Inputs below 1 and division by zero may be outside the parent FPU's mantissa protocol. Changed operands do not establish one conventional fixed-point interpretation for all ten output ports. They remain stress cases for preserving circuit behavior.

The first compiled response matches the interpreter's timing. With a held trigger, later compiled responses can arrive one game tick later. **The regression requires the same ordered output words and resets, not identical timestamps.** This follows the author's permission to simplify timing. Exact held-trigger timing is not certified.

## Missing heads and admission

The save contains 2,055 sticky pistons, 1,827 observers, 1,566 payload groups and 25,455 nonair blocks. There are 489 groups with two owners and 1,077 with one. Payloads are redstone blocks and supported wool conductors; the 100 targets and 109 furnaces are fixed context.

Sixteen extended downward pistons have absent heads:

```text
base: (x,15,z), x in {14,15}, z in {12,16,20,24,28,32,36,40}
missing head: (x,14,z)
far material: (x,13,z), redstone block for x=14, white wool for x=15
```

They are sampled in the original 1/1.5 episode but accept no movement; their heads stay absent and the circuit still produces its output. Compilation warns and begins from saved geometry. It does not delete their electrical contributions or synthesize replacement heads at import. In the 1.75/0.5 stress case, the interpreter accepts twelve events at these positions, so permanently deleting or freezing all headless actors would change behavior. Compiled execution and handoff preserve the observed geometry for that case too.

Earlier admission stopped at these local positions:

| Mode | Earlier first failure |
| --- | --- |
| Normal, budgets 1×/2×/4×/8× | `(16,8,8)`: unverified observer return path; `AdditionalResetWriter` at `(16,10,7)` and `NoResetPath` |
| `--assume-instant`, budget 8× | `(14,15,12)`: missing stationary head at `(14,14,12)` |

These reset-template failures were limitations of the former admission path. The current compiler selects its sampling/notification path for coupled reset writers and missing-head entry geometry. Data, delivered updates, observer deadlines and shared payload ownership remain explicit compiled state; no runtime interpreter fallback is used. The fresh admission capture accepts normal compilation at 1×/2×/4×/8×. The optional assumption mode also accepts but is unnecessary. Each attempt verifies that physical state and scheduled work are unchanged by compilation.

## Evidence and reproduction

- [Current summary](../../test_data/piston-research/references/fpu-divider-current.summary.json) and [source identity](../../test_data/piston-research/references/fpu-divider-current.source.json).
- [Earlier rejection and operand summary](../../test_data/piston-research/references/fpu-divider-original-vectors.summary.json) and [earlier source identity](../../test_data/piston-research/references/fpu-divider-original-vectors.source.json). Earlier rejection is preserved separately, not relabeled as success.
- [Compiled regression](../../crates/core/src/redpiler/analysis/tests/research/fpu_divider.rs). JSON captures characterize the interpreter and admission; this regression asserts compiled output and handoff.

Full ordered captures are local generated files under the ignored `test_data/piston-research/traces/` directory. Summaries and source sidecars remain under `references/`. Capture sources were hashed before the current run and checked afterward; they did not change during capture. This uses the MCHPRS interpreter as the physical reference; independent Java behavior and complete parent-FPU arithmetic are not certified.

```powershell
py tools/download_piston_research.py --names FPU_DIVIDER.schem --output-dir test_data/piston-research/fpu-divider
cargo test -p mchprs_core --lib divider_compiles --locked
py tools/capture_piston_research.py --fixture fpu_divider --output <new-output.json> --target-dir <build-directory>
py tools/summarize_piston_research.py --ingest <new-output.json> --id <new-id>
py tools/summarize_piston_research.py --check
```
