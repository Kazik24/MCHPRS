# MROWW

**Minecraft Redstone o Wysokiej Wydajności**

MROWW is a creative redstone server based on
[MCHPRS](https://github.com/MCHPR/MCHPRS). This checkout targets Minecraft Java
1.21.5, protocol 770, DataVersion 4325. Each plot has its own simulation thread.
The physical interpreter executes spatial redstone and piston updates; redpiler
compiles supported circuits into a graph and logical runtime.

Read [the documentation index](docs/README.md) for the current architecture,
mathematical models, parser, optimizer, and [test suites](docs/tests/README.md).
Supported server mechanics are defined by the implementation; do not assume
complete vanilla physics or that every interpreted circuit can be compiled.

## Build and run

Install Rust, then run from the repository root:

```sh
cargo build --release --locked
./target/release/mchprs
```

On Windows the executable is `target/release/mchprs.exe`. Required Minecraft
registry inputs are checked in; [their reference](mc_data/README.md) explains
generation and provenance. Executable/crate names and the `MCHPRS_CONFIG` and
`MCHPRS_LOG` environment variables retain their existing identifiers.

## Configuration

The server loads `Config.toml` in its working directory and fills missing defaults.
Set `MCHPRS_CONFIG` to select another file. The complete fields and defaults are
defined in [config.rs](crates/core/src/config.rs); Docker's example is
[docker/Config.toml](docker/Config.toml).

| Field | Default | Purpose |
| --- | --- | --- |
| `bind_address` | `0.0.0.0:25565` | Listen address and port. |
| `max_players` | `99999` | Advertised capacity; login does not enforce this limit. |
| `view_distance` | `8` | Chunk view distance. |
| `neighbor_update_interval_ms` | `2000` | Neighbor-plot snapshot refresh; zero hides neighbors. |
| `default_tps` | `20` | Initial game ticks per second for new plots. |
| `auto_redpiler` | `false` | Automatic compilation. |
| `fast_render_threshold` | `200` | Configured TPS above which visual updates are throttled. |
| `fast_render_send_rate` | `10` | Throttled visual flush rate. |
| `proxy_chat` | `false` | Optional authenticated Velocity chat bridge. |
| `worldedit_max_blocks` | `67108864` | Maximum blocks per WorldEdit operation; matches the schematic import/export cap and fits a full 512 × 256 × 512 arena. |
| `worldedit_history_blocks` | `134217728` | Maximum blocks retained in undo/redo per player. |

Existing config values are preserved when the server fills defaults. For production
Docker, update `/srv/mchprs/backend-config/Config.toml` on the host and restart the
container; that mounted config overrides the example bundled in the image.

Plot speed and rendering settings are separate. Neighbor plots appear as
read-only snapshots; entering a plot changes the active simulation context.
See [client synchronization](docs/CLIENT_SYNC.md) for presentation and queue
semantics and [permissions](docs/MCHPRS_PERMISSIONS.md) for LuckPerms setup.

## Use

`/help` provides the quick-start guide; `/help <topic>` explains plots, ticks,
WorldEdit, tools, schematics, pistons, screenonly, history, chat, redpiler, and Git.
`//help <command>` gives detailed WorldEdit arguments. Use Tab completion for
supported command forms.

Common simulation commands are `/tps 0` to pause, `/adv <ticks>` to advance game
ticks, and `/adv nano <count>` or `/adv pico <count>` to inspect partial
interpreter execution. Two game ticks form one redstone tick; nano/pico steps
are operations in the interpreter, not fixed fractions of elapsed time.

`/rp analyze` reports circuit structure; `/rp compile` starts admitted compiled
execution and `/rp reset` hands it back to the interpreter. Read
[redpiler architecture](docs/Redpiler.md) for flags and lifecycle contracts.
`/rhistory` and `/back` record and restore interpreter state.
[Plot Git](docs/PLOT_GIT.md) saves build versions, branches, and comparisons.

## Administration and logging

`/serverinfo` or `/serverinfo plots [page]` lists running plots and their players,
ten plots per page. `/serverinfo plot` reports the current plot's TPS samples,
simulation/render settings, pending work, history memory, Git state and last
compile statistics. `/serverinfo plot <x> <z>` requests the same report from
another running plot without loading it. `/serverinfo settings` shows resource
limits and selected server settings, excluding credentials.

Diagnostics require an explicit `mchprs.commands.serverinfo` permission in
dedicated LuckPerms mode, or `commands.serverinfo` in legacy mode. Standalone
permission fallback does not grant this administrative command. Reports are
also written to the terminal and log; remote plot reports wait for that plot's
thread to handle the request.

At INFO level, command requests include player name, UUID, plot coordinates,
command and up to 1024 characters of arguments. Denials, history operations,
Git worker results and duration, compilation statistics, plot loads/unloads and
player movement between plots are logged. Command spans follow Git and compiler
workers. `MCHPRS_LOG=debug` enables existing detailed diagnostics in debug builds;
the current release build compiles out DEBUG and TRACE events. The new activity
and resource logs use INFO/WARN and remain available in release builds.

New file logs use `logs/mchprs.log` and `logs/mchprs.log.1`, each capped at
16 MiB. The oldest file is replaced during rotation, keeping at most 32 MiB
of new logs. Previous daily logs and `old_output.log` are preserved; their
cleanup remains the operator's responsibility. The production Compose file also
rotates Docker's stdout logs at 16 MiB with two retained files; other launchers
must configure their own stdout retention.

## Development

```sh
python tools/validate_docs.py
cargo fmt --all -- --check
cargo test --workspace --locked --no-fail-fast
```

Long replays, independent Java captures, and baseline creation are explicit
operations documented under [docs/tests/](docs/tests/README.md). Update the
model and affected checks alongside semantic changes. Keep frozen expectations
and source provenance separate from newly captured results.

## License

The repository uses the [MIT license](LICENSE). Upstream contributors and their
work remain recorded in Git history.
