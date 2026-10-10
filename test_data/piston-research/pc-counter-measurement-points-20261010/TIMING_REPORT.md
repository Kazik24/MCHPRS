# PC counter measurement-point timing report

## Result

The newest saved schematic is admitted by the Direct compiler with its default, unoptimized options. A 4,096-tick paired replay now reaches the probe repeaters. It shows that the `adder_stage` input and output timing match interpretation, but the compiled inputs at `adder_out`, `adder_to_memory`, and `output_tap` do not. The output errors follow those input waveform differences. This points upstream into Direct's compiled signal or activation path; this run does not identify the exact missing dependency or event.

The repeater named `memory_output` is absent in this revision, so that point could not be measured. The other four native traces are identical to those captured from the immediately preceding save.

## Fixture and provenance

The latest file was downloaded read-only from `urmom:/srv/mchprs/data/schems/PC_COUNTER_MESURMENT_POINTS.schem` on 2026-10-10. Its SHA-256 is `0749f1b8d5e52e13f62323d42aa194b5d1440f48fcc679aaf687eaa67d5e078b`; it is 4,885 bytes and loads as a Sponge schematic of `24 × 19 × 76` blocks with offset `[23, 1, 0]`. The hash and remote modification time are in [download-manifest.json](download-manifest.json).

The 76-block Z dimension adds a slice relative to the preceding 75-block save. The probe locations in the saved grid are consequently one block higher in Z than their earlier local coordinates. The revised locations below were checked against the loaded repeater blocks. The old `memory_output` probe at the corresponding shifted position `(14, 8, 64)` is absent.

The test ran on branch `redpiler/piston-1.21.5`, HEAD `7fc44e33922eed0d2a210b8110554c3fdc8f1415`.

## Method

Two fresh worlds were pasted from the same downloaded file. Each was checked to contain 23 initially-off levers; the harness turned them all on and ran one interpreter activation tick. Direct compilation used the post-activation world and its pending scheduler entries. The first successful compiler configuration was `optimize=false`, `assume_instant=false`, with the default budget multiplier. It compiled after the activation tick. No optimization comparison was run.

Tick 0 below is the state immediately after that activation tick. The harness then advanced both models once per tick through tick 4,096, ticking and flushing Direct on each step. At every surviving probe it recorded `powered`, `locked`, and main-input strength. The block-level input readback uses `get_redstone_power`, with the stored wire strength as a fallback when the neighbor is redstone wire.

For Direct, the test also recorded the compiled repeater node's `default_inputs.strength_counts` at each change. The compiled input value reported below is the highest nonempty strength bucket. This is distinct from the wire block's stored power in the Direct world: that block-level readback remained at its startup value throughout the run, while the compiled node inputs changed. The compiled node snapshot is therefore the relevant Direct input trace; the world block readback alone would falsely suggest a constant signal.

Powered-edge counts exclude the initial tick-0 state. All four sampled repeaters were unlocked throughout the captured run. The full event traces are in [TIMING_WAVEFORMS.json.gz](TIMING_WAVEFORMS.json.gz).

## Measurements

| Probe | Local position | Interpreter powered edges | Direct powered edges | First powered-state difference |
|---|---:|---:|---:|---|
| Adder out | `(0, 10, 62)` | 683 | 2 | Tick 7: Direct turns on; Interpreter stays off until tick 8 |
| Adder stage | `(3, 11, 65)` | 1,365 | 1,365 | None through tick 4,096; powered-edge times match exactly |
| Adder output / memory input | `(14, 10, 66)` | 683 | 1,365 | Tick 10: Direct turns off while Interpreter stays on until tick 15 |
| Output tap | `(18, 14, 64)` | 683 | 2 | Tick 15: Interpreter turns off; Direct stays on |
| Memory output | `(14, 8, 64)` | — | — | Probe repeater is missing |

### Compiled repeater input traces

The values are `tick:strength`. Tick 0 is included to show the startup state.

**Adder out:** Interpreter begins `0:9, 1:0, 6:9, 7:8, 12:9, 13:0`. Direct begins `0:9, 1:0, 5:8, 10:0, 11:8, 16:0`. The first logical input divergence is tick 5: Direct already has strength 8 while the interpreter input is still zero. Direct then has a one-tick low gap before the next strength-8 pulse. The repeater rises at tick 7 in Direct and tick 8 in interpretation, then Direct remains powered for the rest of the replay while the interpreter continues its 12-tick output cycle.

**Adder stage:** Both inputs begin `0:14, 1:0, 6:14, 7:0, 12:14, 13:0` and continue identically. Both output edge streams begin `3:off, 8:on, 10:off, 14:on, 16:off, 20:on` and remain identical through tick 4,096. This is strong evidence against a general delay-1 repeater timing error.

**Adder output / memory input:** The interpreter input begins `0:1, 1:0, 6:1, 13:0, 18:1, 25:0`. Direct begins `0:1, 1:0, 6:1, 7:0, 12:1, 13:0, 18:1, 19:0`. Direct turns the input on for one tick every six ticks; interpretation holds it on for seven ticks every twelve ticks. The Direct repeater consequently produces roughly twice as many output edges: 1,365 versus 683.

**Output tap:** The interpreter input begins `0:15, 1:0, 6:15, 13:0, 18:15, 25:0`. Direct begins `0:15, 1:0, 6:15` and stays at 15 through tick 4,096. Its repeater turns on at tick 8 and remains on; the interpreter turns off at tick 15 and continues cycling.

## Topmost repeater compared with Adder Out

The only repeater on the schematic's highest repeater layer is the `output_tap` at `(18, 14, 64)`. `Adder Out` is at `(0, 10, 62)`. Both are delay-1 repeaters, and the interpreter's `powered` transitions match exactly at both probes for all 4,096 ticks: `0:on, 3:off, 8:on, 15:off, 20:on, 27:off, ...`. Their input strengths differ (`Adder Out` is attenuated to 9/8 while the top probe receives 15), but their input on/off windows and resulting native repeater states match.

Direct breaks that agreement at the first reactivation: its `Adder Out` repeater turns on at tick 7 while the topmost repeater turns on at tick 8. They agree again from tick 8 because Direct keeps both powered, but both then miss the interpreter's repeated off transitions. The compiled input snapshots already disagree before those output states diverge: `Adder Out` begins pulsing strength 8 at tick 5, while the topmost point returns to strength 15 at tick 6 and remains high. This localizes the mismatch upstream of the repeaters, in Direct's generation/timing of the signal presented at these points; it is not a repeater-delay discrepancy.

## What this establishes

The first measured point, `adder_stage`, receives the same compiled input waveform as the interpreter and produces the same output edges. At later points, Direct's compiled input waveform is already different before the repeater output diverges. In particular, the one-tick low gaps at `adder_out` are consistent with its delay-1 repeater retaining its powered output, and the short pulses at `adder_to_memory` produce a faster output cycle. The persistent high input at `output_tap` accounts for its persistent output.

The evidence therefore localizes the measured mismatch to signal generation or timing upstream of those repeater inputs. It does not support changing repeater delay logic as the first fix. The exact activation, feedback, or state boundary that creates the altered waveforms remains unproved; a follow-up should trace the Direct expression dependencies backward from the first divergence at `adder_out` (tick 5), while preserving the matching `adder_stage` trace as a control.

The native input/output event traces for `adder_out`, `adder_stage`, `adder_to_memory`, and `output_tap` match exactly between the immediately preceding 4,880-byte schematic and this latest 4,885-byte schematic. That confirms the user's expectation that adding the redstone block preserved those four interpreter waveforms.

## Limits

This is a timing-point comparison, not a complete counter replay: the old bus helper targets the previous schematic coordinate grid, so the new capture intentionally does not decode or report the counter's 16-bit output. The missing memory-output repeater also leaves that point unobserved. The run used default unoptimized Direct compilation only; it did not test `optimize=true`, compare full workspace behavior, or change production code.
