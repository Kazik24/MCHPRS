# Experimental whole-frame scope

This is a proposal, not an enabled compiler mode. The user explicitly accepted
the reset-only exit on 28 October 2026. The existing Graph backend and its
native-continuation contract are unchanged.

The first target is a closed circuit with declared input and output ports. The
compiler must reject unknown crossings and shared mutable resources; read-only
and event dependencies must not be confused with ownership. An independent
neighbor must keep executing. An isolated fixture is an acceptable first
milestone if the saved plot cannot yet be isolated.

Inputs are sampled at a declared frame boundary and remain stable until the
frame completes. Results are observed at declared result ports. Internal
transitions may use deterministic idealized timing; intermediate visual states
and piston animation are not promised. A supported event must still trigger
the computation even if a final sampled value has not changed. The model must
carry the state needed for the next frame and reset/rearm correctly.

For the FPU protocol the user identifies the falling clock edge as the result
publication boundary: complete the instant computation for that cycle and
publish its final interface values there. The native circuit's intermediate
settling ticks are not observations required by this mode. Other circuits must
derive their own supported controller or use an explicitly declared boundary.

The user also approved ignoring nanotick ordering inside instant circuits and
omitting internal visual updates. BUD switches, lamps and repeaters at the
declared interface remain observable. Only the specified frame boundaries need
to match; an intermediate native difference alone is not a failure. An
idealized result may differ from native execution when nanotick order changes
the result, so native comparison is evidence rather than a universal oracle.
Reset the owned region when execution ends or a write cancels compilation.

Compatibility exclusions must be explicit before activation. The current FPU
has 35 input indicator lamps and command-block outputs in addition to 16 result
repeaters. A mode that omits any of them must identify those omissions; it must
not claim equivalence for them. No assumption about the FPU's numerical format
or opcode meanings follows from the port positions alone.

Reset-only exit means restoring the selected region to its recorded starting
state, not materializing the frame's computed internal physical state. This is
not implemented by merely calling the current `Compiler::reset`: that method
projects the live compiled state and returns pending work to the interpreter.
Any implementation must cover explicit reset, compile replacement, plot unload,
save/checkout and execution errors. It must not revert an independent neighbor
or discard its work. A running frame with an unrepresentable outside write,
callback or shared resource must not be admitted.

The first executable oracle should pin the corrected saved snapshot and use
declared port order to run consecutive changing operand/opcode vectors, repeat
from a fresh starting snapshot, and record the 16 native readouts. Such results
are compatibility evidence, not arithmetic proof. Add arithmetic expectations
only when operation meanings, operand encoding and output decoding have an
independent source. The old settled-equation model has produced wrong outputs
under random input; its successful construction is not evidence of a working
frame.

Acceptance requires a frame executor that actually replaces internal Graph
work, correct outputs across successive input frames and restart, progress of an
independent native neighbor, and a measured changing-input throughput. Report
execution counts separately from plans and compiler admission. If the frame
cannot meet the boundary contract, decline it before suppressing native work.

## Native compatibility examples

The opt-in `corrected_fpu_native_frame_baseline` test pins the corrected
FPU_27 snapshot and the port manifest, starts from the saved stopped clock,
warms up for 24 game ticks, and samples the result repeaters after each six-tick
frame. Input masks use manifest order: blue lever entries in bits 0–15, red
entries in bits 16–31, then opcode entries in bits 32–34. Output words use
the physical labels 15 through 0. These are raw lever and repeater states, not
decoded numbers or proof that each sample is an arithmetic result.

| Raw input mask | Native boundary sample |
| --- | --- |
| `0x000000000` | `0xffff` |
| `0x3ffff0000` | `0x0000` |
| `0x412345678` | `0x8800` |
| `0x7abcdef01` | `0xffff` |
| `0x10000ffff` | `0x7fff` |
| `0x6ffff0000` | `0x7fff` |
| `0x25555aaaa` | `0x0a80` |
| `0x5aaaa5555` | `0x2aaa` |

The test repeats the sequence after loading the saved snapshot again; this is
not a test of an in-world reset. It requires the external snapshot at
`target/build-audit-tps/prod/p-1,-1-FPU27-2026-10-27` and `PLOT_SCALE=5`:

```powershell
$env:MCHPRS_SAVED_PLOT = 'F:\rustrepos\MCHPRS\target\build-audit-tps\prod\p-1,-1-FPU27-2026-10-27'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
cargo test -p mchprs_core --locked --release --lib corrected_fpu_native_frame_baseline -- --ignored --test-threads=1
```

The ignored `corrected_fpu_logical_frame_probe` extracts a computation from the
full corrected plot and compares it with this native protocol. When input
`0x412345678` was held for 54 ticks, the logical dust word stayed at `0x84f3`;
native dust repeated five ticks at `0x8800` and one at `0xffff`, while the
repeater read `0x8800` at each six-tick boundary. This difference also occurs
when native dust and repeaters both read `0x8800` in the same tick.

This probe advances a `SettledMachine` on every game tick; the production
frame executor has different feedback and dispatch rules. The observation is
not a production-executor test. The user confirmed that `0x84f3` is the correct
result for this input and that the native FPU is broken for this non-instant
case. Thus matching the native `0x8800` result is not the desired idealized
behavior for this example.
Run it with the same environment as above, using
`cargo test -p mchprs_core --locked --release --lib corrected_fpu_logical_frame_probe -- --ignored --nocapture --test-threads=1`.


## Offline evaluator throughput

The ignored `corrected_fpu_logical_frame_probe` also measures 10,000 changing
input vectors against the circuit extracted from the pinned corrected plot.
It generates deterministic xorshift inputs, updates the 35 declared lever
bindings, retains the other sampled controls, and projects the sixteen output
bits into a checksum. One loop advances persistent feedback once per vector;
the other resets feedback and iterates to convergence, with a 128-pass limit.
The setup, graph extraction, and native replay are outside the timed loops.

On the measured release build at HEAD `65bf0905` with a dirty working tree,
O6, budget multiplier 8, and `stable_instant_inputs=true`, persistent feedback
took 0.293531 seconds (34,068 evaluations/s; checksum `be55`). Fixed-point
feedback took 0.364642 seconds (27,424 frames/s; checksum `0313`), converging
on all 10,000 frames in 27,349 passes. Their checksums differ; these are
separate feedback semantics, and the test does not establish arithmetic
correctness for the benchmark vectors. Both offline feedback methods also returned the user-confirmed `0x84f3` for
the held input vector; this does not test the admitted Graph runtime.

Multiplying these rates by six yields 204,408 and 164,545 equivalent game
ticks/s respectively, solely as a six-game-ticks-per-frame conversion. They
are not measured plot TPS: the loops exclude Graph dispatch, clock and native
neighbor work, output delay scheduling, and retained effects. The full saved
plot still declines scoped frame admission. Run the test with the same
snapshot and release environment shown above:

```powershell
cargo test -p mchprs_core --locked --release --lib corrected_fpu_logical_frame_probe -- --ignored --nocapture --test-threads=1
```

## Resource semantics gap

The user specifies at most one successful move per movable resource per cycle,
shared across all pistons that can move it. A failed request must not consume
that resource. The compact computation's 15 feedback outputs are not a list of
every movable resource, and fixed-point convergence alone does not enforce this
rule.

For raw input `0x558e1d715`, the persistent evaluator produced `0xffff` from
both the initial state and the tested input history. The user confirmed this
all-ones output is invalid. Fixed-point evaluation produced `0x871e`; its
correctness is unconfirmed. A diagnostic that merely limits each of the 15
feedback bits to one value change produced `0x88f3` for `0x412345678`, rather
than the user-confirmed `0x84f3`. That proxy omits source-qualified requests,
same-state events and acceptance ordering, so it is not an implementation of
the resource rule.

In the current extractor, `Equations::feedback_resource_leaf` reduces a
noncanonical near-contact leaf to false for the supported shared-far model;
`shared_far_presence` builds the corresponding far-presence equation. The
settled compiler emits feedback only for retained, reachable feedback leaves.
Consequently, this reduced output does not retain a complete record of
resource requests or successful transfers. Source-derived composition must
carry those events through lowering before a once-per-resource executor can
enforce the rule. Its experimental resource-wave constructor is test-only.

Before activation, validate source-derived resource acceptance and rearm, then
prove the selected device's complete ownership and explicit interface. The
selected snapshot and logical-frame experiments remain disconnected from plot
lifecycle and must not be represented as an active FPU executor.

