# Architecture cleanup plan

Status: Phases 0–3 implemented; Phases 4–6 not started.

## Objective

Make ownership predictable: a module's name and location should tell a reader
what it owns. Separate the live plot session from its simulation world, and put
shared simulation machinery outside the compiler. Preserve behavior throughout.

This plan follows the repository architecture review. Its findings are structural
observations, not proof of runtime defects. The review did not execute the test
suite or examine every implementation path.

## Existing architecture

| Area | Owns |
| --- | --- |
| `src/` | Executable startup and logging |
| `core::server` | Connections, login, server loop and plot routing |
| `core::plot` | Live plot session, world, editing, commands and presentation |
| `core::world` | World interface, chunk storage and wire topology |
| `core::redstone` | Physical interpreter component behavior |
| `core::redpiler` | Compilation, probing, execution and interpreter handoff |
| `core::headless` | Playerless simulation sessions |
| `crates/world` | Serializable tick and piston state types |
| `crates/blocks` | Block, item, position and block entity model |
| `crates/network` | Minecraft packets and connection transport |
| `crates/save_data` | Save formats, migrations and atomic file operations |
| `crates/redgear-api` | Network service around headless sessions |
| `waveform-viewer` | Browser waveform viewer |
| `tools`, `test_data` | Capture tooling, fixtures and frozen references |

Minecraft input passes through the server into a plot thread, then through
commands or packet handlers into world mutation and simulation. World changes
produce client updates and persisted state. Headless sessions reuse `PlotWorld`;
waveform recording exposes simulation observations through the HTTP/event API.

## Rules for implementation

- One responsibility move per commit. Keep behavior fixes in separate commits.
- Before each move, search all callers, exports, tests, benchmarks, documentation
  and CI filters. Record the affected paths in the commit description.
- Keep data formats, database schemas, protocol bytes, ordering, permissions,
  bounds checks and error handling unchanged.
- Preserve public Rust paths with re-exports where needed. Check visibility before
  moving implementations: child modules cannot automatically expose private
  methods to their parent. Use the narrowest required visibility.
- Reuse existing modules and direct calls. Introduce no dependencies, traits,
  service layers or configurable abstractions for these moves.
- Keep unrelated working-tree changes intact. The review found pending changes
  in Cargo metadata, compiler probes and piston reference tests; recheck the
  working tree before implementation.
- If a proposed move needs broad redesign, stop that phase and document the
  actual dependency before expanding its scope.

## Phase 0: establish the baseline

1. Inspect current exports and consumers, including `redgear-api`, integration
   tests, benchmarks and external-facing APIs.
2. Run formatting, workspace checking and workspace tests using the commands
   below. Record existing failures before moving code.
3. List current test names for suites that will move. CI uses name filters, and
   Cargo can succeed when a filter matches zero tests.

Done when existing failures are distinguished from refactoring regressions and
the affected test suites are known.

### Phase 0 baseline record (2026-10-31)

- `cargo fmt --all -- --check` already failed with formatting diffs across
  unrelated files.
- `cargo check --workspace --locked --all-targets` succeeded with three
  existing dead-code warnings in `redpiler/scratch.rs`.
- `cargo test --workspace --locked --no-fail-fast` had 647 core unit tests pass,
  four fail and six ignored. The failures were
  `redpiler::scratch::tests::scratch_plot_edge_context_uses_the_world_air_contract`,
  `redpiler::scratch::tests::scratch_reports_unknown_boundary_and_pending_entry_work`,
  `redpiler::sampled::tests::rejects_invalid_or_unreadable_piston_contacts`, and
  `tests::run_mandelbrot_chungus`. The `cpu_references` integration target also
  failed `frozen_cpu_inputs_and_initial_states` because its tick-zero block and
  entity hashes did not match the frozen reference. Other workspace targets
  passed.
- Before moving tests, the scheduler suite was
  `redpiler::queue::tests::{test_reset_queue,test_zero_tick}`; the player model
  suite had seven tests under `plot::redstone_tools::small::tests`; and CI's
  `plot::piston_tests::` filter selected seven tests.
- After the moves, those suites list as two tests under
  `world::scheduler::tests`, seven under `plot::player_model::tests`, and the
  same seven under `plot::piston_tests::`. The CI filter still selects all
  seven piston tests.
- Final checks reproduced the same four core unit failures and the same frozen
  CPU hash mismatch; no new workspace failures appeared. The workspace check
  and benchmark-target compilation succeeded. Scheduler, player model,
  database, piston CI, headless and interpreter suites passed; the Redpiler
  suite had 84 passes and the same three baseline failures. The workspace
  formatting check still reports repository-wide diffs.

## Phase 1: put shared scheduling under the world module

Move `crates/core/src/redpiler/queue.rs` to
`crates/core/src/world/scheduler.rs`. This is the core world module, not the
separate `crates/world` package that owns serialized state types.

Scope:

- Move `Queues`, `TickScheduler`, `ScheduledBlockTick` and their existing tests.
- Update compiler, plot, scratch, headless and benchmark imports.
- Preserve the existing `redpiler` exports when needed for API compatibility.
- Keep queue capacity, tick priority, FIFO ordering and tick handoff unchanged.

Validate scheduler tests, interpreter tests, headless tests and Redpiler tests.
Done when interpreted execution imports scheduling from its shared owner.

## Phase 2: correct player feature ownership

Move `plot/redstone_tools/small.rs` and its tests to `plot/player_model.rs`
and an adjacent test module. Keep player model state types in `player`.

Scope:

- Update command, spawn, movement, gamemode, plot entry and plot exit callers.
- Preserve collision/headroom checks, permissions, saved appearance and packets.
- Adjust restricted visibility paths to match the new location.

Validate existing small-player command, model, persistence and client
synchronization tests where available. Done when player appearance no longer
depends on a redstone-tools module.

## Phase 3: align filenames and share fixture support

Make these independent changes in separate commits:

1. Rename `plot/repository/` to `plot/database/`, retaining `plot::database` and
   removing its path override. Leave `plot/git/repository` in place: that module
   owns the separate plot version repository.
2. Move plot-owned piston tests from `redstone/piston/tests/plot.rs` into
   `plot/tests/piston.rs`. Preserve the `plot::piston_tests` module name so existing
   CI filters continue to select it.
3. Move CPU fixture helpers used by both benchmarks and regression tests out of
   `benches/support` into `crates/core/test_support/`. Keep benchmark-only timing
   code in `benches`. Update every path inclusion; do not copy the helpers.

Validate database and piston tests, compile benchmark targets, and compare test
names before and after. Frozen fixtures, capture provenance and expected outputs
must remain unchanged.

Done when imports lead to matching locations and correctness tests no longer
load shared helpers from the benchmark directory.

## Phase 4: separate PlotWorld from the live plot session

First move `PlotWorld`, `UpdateStats` where required, its inherent implementation
and its `World` implementation from `plot/mod.rs` into `plot/world.rs`.
Keep `pub use world::PlotWorld` so callers retain `plot::PlotWorld`.

Then move physical advancement methods into `plot/world/interpreter.rs` and
move the existing interpreter cache under that module. Use normal module
declarations instead of the current interpreter cache path override.

Scope:

- Keep `Plot` responsible for players, channels, thread lifecycle, runtime
  coordination, compilation jobs and presentation scheduling.
- Keep world storage, mutation, scheduling and interpreter state together.
- Update sibling-module access, headless and scratch callers, tests and docs.
- Preserve cache invalidation, scratch boundaries, tick phases, piston ordering,
  history recording, waveform recording and output collection exactly.
- Keep current presentation fields in `PlotWorld` during this extraction.
  Separating transport from mutation is a later design decision, not a
  prerequisite for giving the world its own file.

Validate interpreter, piston, history, client synchronization, headless and
compiler activation/reset/handoff suites. Compare test lists after moves.

Done when a reader can inspect the world without traversing the live plot
session implementation. This is the highest-risk phase; split extraction and
interpreter relocation into separate commits.

## Phase 5: separate command declarations and command families

Convert `plot/commands.rs` into `plot/commands/mod.rs` without changing its module
path. Extract packet command-tree construction into `declaration.rs`. Then move
the existing plot and Redpiler handlers into `plot.rs` and `redpiler.rs` children.

Scope:

- Keep a small dispatcher with direct calls into command handlers.
- Keep permission and ownership checks on the execution paths they protect.
- Keep aliases, completion, argument parsing, feedback and player-transfer
  early-return behavior unchanged.
- Move existing tests with the behavior they cover. Preserve test names where
  practical and update CI filters where names must change.
- Keep diagnostics, WorldEdit and tools in their existing owning modules.
  Extract additional command families only when they have a clear responsibility.

Validate command declarations and edge references, permissions, completion,
coordinate parsing, aliases and queued commands that transfer players.
Done when command declaration edits and command execution edits have distinct
homes, without a new command framework.

## Phase 6: isolate server admission

Move login/configuration methods into `server/login.rs`, retaining methods on
the existing `MinecraftServer` type. Inspect the packet handler trait first;
keep a single trait implementation or use direct delegation if required by Rust.

Scope:

- Include handshake, login, Velocity verification and configuration sequencing.
- Keep the main loop, player routing, plot registry and shutdown in `server.rs`.
- Keep proxy message routing with its current runtime owner unless admission
  alone owns it.
- Preserve whitelist checks, forwarding validation, disconnect behavior and the
  point at which a player is admitted to a plot.

Validate existing server, proxy, permission and connection tests. If existing
tests do not cover admission order or rejected forwarding, record that gap;
add focused coverage before changing behavior in those paths.

Done when admission can be reviewed independently of plot coordination.

## Optional follow-up: block model and network dependency

`blocks` uses `network` for packed positions, text/NBT conversion and item
components. Inventory all those uses before proposing a crate change.

First group pure Minecraft data conversions coherently within the existing
network crate. Extract a shared data crate only if the complete move removes
the `blocks -> network` dependency and produces a simpler dependency graph.
Do not add a crate just to relocate one helper. This follow-up is not required
to complete phases 0–6.

## Validation commands

Run the baseline and final checks from the repository root:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked --all-targets
cargo test --workspace --locked --no-fail-fast
```

During each phase, run the affected existing suites and check all targets.
Use test listing to verify renamed modules still run:

```sh
cargo test --locked -p mchprs_core --lib -- --list
cargo test --locked -p mchprs_core --test headless_api
cargo check --locked -p mchprs_core --benches --features plot-bench
```

Inspect `.github/workflows/test.yml` whenever test module names change. Check
the actual selected test count for each affected CI filter. Keep long replays,
independent Java captures and reference regeneration explicit; follow their
documented workflows if a semantic change later requires them.

## Completion criteria

- Shared physical scheduling lives outside Redpiler.
- Player appearance lives outside redstone tools.
- Module names match file locations for the paths addressed here.
- Benchmarks and regression tests share one copy of CPU fixture helpers.
- `PlotWorld` and physical advancement have clear homes separate from `Plot`.
- Command declarations and the extracted command families have distinct homes.
- Server admission is separate from server coordination.
- Public paths remain compatible where required, and CI still selects moved tests.
- Baseline and final checks are compared; new failures are resolved.
- No save migration, protocol change, dependency or frozen-reference update is
  introduced by these structural changes.

Line count reduction is not a success measure for file moves. Success means a
reader can find an owner from its name, and each change reaches fewer unrelated
responsibilities.
