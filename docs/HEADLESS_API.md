# Headless session API

`mchprs_core::headless` provides an in-process Rust API for external hosts such as RedGear. The host owns transport, version negotiation, world capture and recovery; this API does not access Paper or define JSON/IPC. One owner must serialize calls on each session. Dropping the session closes it.

`Snapshot` contains inclusive plot-coordinate bounds, native blocks, block entities and ordered pending `TickEntry` records. Missing blocks are air. Dimensions are at most 256 blocks per axis and must fit the plot. `Session::new` validates positions, duplicate blocks/entities, entity compatibility, moving pistons and scheduler limits before constructing a fresh world. Tick delays are half-game-ticks in the range 0–31, and a zero delay requires `NanoTick` priority. Initial load writes are not returned by `drain_changes`.

A session starts in interpreter mode. `compile(CompilerOptions)` selects Redpiler and returns its statistics; a rejected compile leaves the interpreter snapshot intact. After successful compilation the interpreter's pending ticks are handed to the compiler. `reset` returns compiled state and pending ticks to the interpreter. It does not discard resulting block writes. The host must explicitly decide whether a failed compile permits interpreter use.

`use_block` and `set_pressure_plate` apply inputs synchronously in call order. `advance_with_inputs` validates a list of `Input::Use` and `Input::Plate` against the current state, applies them in order, and advances at the following boundary. If application fails after validation, the session fails because earlier inputs may have changed it. `advance(n)` advances exactly `n` game ticks unless execution fails; the compiled world is flushed after each tick. `ticks()` reports completed ticks, including those completed before failure. `get` reads a block and `power` reads redstone power at a face. `block_entity` and `pending_ticks` are available only in interpreter mode: the compiled backend owns its live entities and pending work. `drain_changes` returns changed `(position, native block-state ID)` pairs, including air, and consumes them; an empty drain means no pending writes. It is a state projection, not a stream of intermediate events.

Unsupported input targets and invalid positions return errors. After a compiled execution error the session fails: do not retry or continue it. The host must reject data it cannot map exactly, enforce its own input and message limits, and preserve or reject effects and block-entity behavior it cannot transport. This API does not provide a complete version/capability handshake or effect stream. Compiled block-entity/tick inspection and complete fidelity have not been verified.

```rust
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::headless::{Session, Snapshot};

let pos = BlockPos::new(5, 5, 5);
let mut session = Session::new(Snapshot {
    min: BlockPos::new(4, 4, 4),
    max: BlockPos::new(6, 6, 6),
    blocks: vec![(pos, Block::from_name("lever").unwrap())],
    entities: vec![],
    ticks: vec![],
})?;
session.use_block(pos)?;
session.advance(1)?;
let changes = session.drain_changes()?;
# Ok::<(), anyhow::Error>(())
```
