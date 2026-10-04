# Piston timing repair

Updated 2026-10-04. The engine now separates scheduled block ticks, FIFO piston events and moving block entities. Extension changes the base state during its event, without a three-game-tick scheduling lock. Retraction preserves the captured short-pulse drop decision. Movement progresses through exact half steps and completes in its own phase.

The current movement scope follows the user's instruction: payloads are assumed movable and sticky; push reactions, the twelve-block rule and slime/honey attachment graphs are deferred. There is no piston rules table. Plot boundaries remain storage safety checks. Normal and sticky piston source types and retraction/drop actions remain distinct.

## Correctness fixes

- Ordinary ticks retain the requesting block registry type; stale observer work cannot complete a replacement moving piston.
- Movement snapshots retain an entity identity. Replaced/deleted entities cannot inherit old completion work. Completion writes only its own position, rather than reconstructing a neighboring base.
- Breaking an owned head/base removes the matching counterpart. Unrelated bases and heads are preserved.
- Moving payloads have destination entities and preserve overlapping straight-chain payloads and their block entities.
- Normal completion clears waterlogging, checks support and applies the observer's conditional reset/output notification. Interrupted completion retains its separate waterlogging behavior.
- Destination shape changes notify observers. Neighbor notifications use West, East, Down, Up, North, South locally; they do not recheck the source piston itself after each event write. Restored components receive one placement/recheck callback.
- Game, nano and pico stepping share one phase machine and preserve partial advancement across saves.

## Reference and tests

`tools/capture_piston_reference.py` runs the SHA-pinned official Java 1.21.5 server in a temporary void world, freezes ticking, imports fixtures with strict writes, applies the same stimulus and records one observation per game tick. Its output is [java-traces.json](piston-repair/java-traces.json). The server SHA-1 is `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`.

All four update-tester circuits and the memory cell now match their twelve-game-tick Java traces. The memory cell's previous guessed array was replaced by the actual Java recording. The recording includes intermediate memory-cell piston/observer states. This establishes the checked fixtures, not complete vanilla compliance or internal Java callback traces.

The normal test suite includes 34 focused piston regressions, partial-step save/restart tests, stale incarnation work, and legacy format-3 motion conversion. Validation passed: all 85 workspace unit tests, independent protocol/restart smoke and plot-load failure smoke. Tests run in isolated worlds; the production server has not been deployed or modified by this repair.

## Persistence

Plot format 4 stores expected tick types, queued events, logical time, phase, current/previous movement progress, identities and carried block entities. Frozen format-3 readers convert the old moving-head payload representation into destination entities and preserve the original backup. Occupied/outside legacy destinations fail conversion without altering the original. Missing historical timing conservatively restarts at recorded progress; it cannot be reconstructed exactly. Formats 0/1 retain their existing ID conversion; format 2 remains unsupported. Player storage is unchanged.

## Remaining work

See [task progress](TASK_PROGRESS.md) for the requested review, adder circuit test, upstream feature ports and benchmarking. Push reactions and adhesive graphs remain deferred by explicit instruction. Graphical animation/client checks and full vanilla behavior coverage remain future validation work.
