# Redpiler status — 31 October 2026

The sampled compiler is now the only compiled Redpiler engine. The Graph
runtime, analysis pipeline, optimization passes, Graph export tools, commands,
flags, statistics, and benchmarks have been removed. Plot and headless callers
use the same `Compiler` facade; unsupported compile requests stay in native
simulation.

## Runtime contract

- Compilation owns an immutable program and mutable runtime state. A rejected
  or cancelled compile does not activate a partial engine or consume native
  scheduled work.
- Flush publishes supported block values and moving piston entities. Repeated
  flushes retain motion identity and timing. Reset exports pending ticks,
  piston actions, and active motion state before native execution resumes.
- Failed export leaves the compiler active and reports the error. Plot and
  headless paths do not silently resume native simulation from partial state.
- Plot activation still checks the captured world version and supports worker
  cancellation. The headless public API runs an admitted compiled lever circuit.

## Admission and coverage

The maintained sampled tests cover ADDER1BIT, ADDER11BIT, COUNTER, and focused
TEST outputs, plus scratch boundaries, wire discovery, repeaters, pistons,
scheduling, and state handoff. The full `TEST_PISTION.schem` fixture declines
because it contains a connected copper bulb; the other focused TEST timelines
remain covered. Unsupported blocks, block entities, attachments, boundary
reads, and active piston entry states produce explicit errors so the world can
continue in the interpreter.

Pressure plates and command blocks remain native-only. Copper bulbs with
discovered inputs are also declined because their state is not modeled by the
compiled runtime. Full FPU support and full internal-state parity remain
outside this migration.

## Repeated arithmetic follow-up — 31 October 2026

The rebuilt decompiled MAIR reference, Rust native mode, and Rust compiled mode
all passed two reconstructed 16-operation ADDER1BIT replays (all eight inputs,
then repeated). The declared windows were raw sum ticks 1–3, sum repeater ticks
3–7, and carry repeater ticks 5–9. The shared Rust acceptance test now reuses
one compiled runtime for the rearm-wait protocol, checks outputs and pending
work at preparation boundaries, asserts compiled work per operation, and runs
past multiple scheduler-ring rotations. A six-operation ADDER11BIT test covers
`2047+1` overflow, `1023+1` carry propagation, and return to zero.

Focused piston tests cover phase invalidation on token movement and piston
completion, plus pushing a second token into an open opposed-piston mate home.
The runtime now clears update marks for all pistons in a moved token's cell
domain and supports that onward push within a compiled piston group. These
changes follow the MAIR mechanisms identified in the replay report; they do not
claim parity for arbitrary piston arrangements.

The CPU_BUBBLESORT compile attempt used its existing load/reset protocol and
declined at `BlockPos { x: 168, y: 15, z: 158 }` with `unsupported component`,
before the CPU start button. Compiled CPU execution remains unsupported.
The report at `target/mair-crosscheck/REPORT.txt` records protocol details,
2,304-frame native/compiled comparison, the 64 observed out-of-window port
differences, and limits: no original pinned binary or pending-work parity.

## Verification

Passed:

- `cargo test -p mchprs_core --lib adder1bit_reference_manifest_arithmetic` — 1 test
- `cargo test -p mchprs_core --lib adder11bit_reference_manifest_acceptance` — 1 test
- `cargo test -p mchprs_core --lib redpiler::sampled::` — 57 tests
- `cargo fmt --all` (check mode still flags pre-existing trailing whitespace in untouched `crates/core/src/container/tests.rs`)
- `cargo check --workspace --locked`
- `cargo check -p mchprs_core --all-targets --locked`
- `cargo test -p mchprs_core --lib redpiler::` — 69 tests
- `cargo test -p mchprs_core --lib -- headless::` — 9 tests
- `cargo test -p mchprs_core --test headless_api` — 1 test
- `cargo test -p mchprs_core --lib redstone::master_tests::dust_steps_over_opaque_blocks_and_powers_the_block_below -- --exact`

The plot regression group passed except for the existing PM1 full-clipboard
work-limit test. The complete core library run also found the native Chungus
fixture loading a 256-chunk save before reading chunk index 359. Those two
failures are outside the Redpiler migration; the affected test and fixture
code were not changed here.

See [architecture](REDPILER_ARCHITECTURE.md) for the current lifecycle and
[the historical engine plan](REDPILER_SAMPLED_ENGINE_PLAN.md) for earlier
development notes.
