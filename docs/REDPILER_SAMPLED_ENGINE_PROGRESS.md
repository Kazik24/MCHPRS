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

## Verification

Passed:

- `cargo fmt --all -- --check`
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
