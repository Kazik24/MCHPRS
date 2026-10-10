# MROWW

**Minecraft Redstone o Wysokiej Wydajności**

Authors: **kazik24** and **lord225**.

MROWW is a creative redstone server based on
[MCHPRS](https://github.com/MCHPR/MCHPRS). This checkout targets Minecraft Java
1.21.5, protocol 770, DataVersion 4325. Each plot has its own simulation thread.
The physical interpreter executes spatial redstone and piston updates; redpiler
compiles supported circuits into a graph and logical runtime.

Read [the documentation index](docs/README.md) for the current architecture,
mathematical models, parser, optimizer, and [test suites](docs/INTERPRETER_ARCHITECTURE.md#8-verification-and-reproduction).
Supported server mechanics are defined by the implementation; do not assume
complete vanilla physics or that every interpreted circuit can be compiled.

## Build and run

Install Rust, then run from the repository root:

```sh
cargo build --release --locked
./target/release/mroww
```

On Windows the executable is `target/release/mroww.exe`. Required Minecraft
registry inputs are checked in; [their reference](mc_data/README.md) explains
generation and provenance. Internal crate names and the `MCHPRS_CONFIG` and
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
| `native_chat` | `true` | Render public chat and join/leave notices; false leaves public chat entirely to an external provider. |
| `schematic_save_prefix` | `""` | Prepend to saved schematic names: `./mroww/` creates a subfolder, `aaa@` adds a filename prefix, and `./mroww/aaa@` combines both. Relative to `schems/`, or the player's UUID folder with `schemati = true`. |
| `worldedit_max_blocks` | `67108864` | Maximum blocks per WorldEdit operation; matches the schematic import/export cap and fits a full 512 × 256 × 512 arena. |
| `worldedit_history_blocks` | `134217728` | Maximum blocks retained in undo/redo per player. |

Existing config values are preserved when the server fills defaults. For production
Docker, update `/srv/mchprs/backend-config/Config.toml` on the host and restart the
container; that mounted config overrides the example bundled in the image.

With `native_chat = false`, MROWW discards public chat and emits no chat formatting
or join/leave notices. It does not relay approved messages or format external
messages. `/say`, `/tellraw`, and command-block chat remain visible only to matching
players on the plot where they execute, regardless of `native_chat`. Other command
feedback and player-list updates still work.

Authenticated `[velocity]` forwarding automatically honors the proxy's allow,
cancel, and replacement decisions for chat and commands using the
[SignedVelocity 1.5.0 protocol](https://github.com/4drian3d/SignedVelocity/tree/1.5.0).
The proxy must run SignedVelocity. Input waits for its decision; exceeding 16
pending inputs or decisions of either kind closes the connection. With
`native_chat = true`, approved public messages use backend rendering, as on the
main Paper server. Backend command permissions and input limits still apply.
The old `signed_velocity` switch migrates to its inverse `native_chat` value;
an explicit `native_chat` wins. The obsolete `proxy_chat` setting is removed.

The root and Docker configs set `native_chat = false` and refer to the current
read-only secret mount at `/run/velocity/forwarding.secret`. The current proxy's
ChatRegulator and SignedVelocity plugins moderate messages but do not relay
public chat; an external broadcaster is needed to make public chat visible in
this mode. For a standalone launch, remove `[velocity]` and set
`native_chat = true`. Keep the production config's existing `[luckperms]` section
and credentials; the commented example documents its non-secret options.

Plot speed and rendering settings are separate. Neighbor plots appear as
read-only snapshots; entering a plot changes the active simulation context.
See client synchronization for presentation and queue
semantics and [permissions](docs/MCHPRS_PERMISSIONS.md) for LuckPerms setup.

## Use

`/help` provides the quick-start guide; `/help <topic>` explains plots, ticks,
WorldEdit, tools, wire, schematics, pistons, screenonly, history, chat, redpiler, and Git.
`//help <command>` gives detailed WorldEdit arguments. Use Tab completion for
supported command forms.

`/small` toggles a half-size player (0.3 blocks wide, 0.9 blocks tall), fitting
through one-block gaps. `/small on|off` selects the mode explicitly. Other players
see an ocelot following you by default; `/small wolf|fox|cat|ocelot|baby` selects and
saves its appearance. Baby selects a baby ocelot and gives you a quarter-size hitbox.
Your own view stays a scaled player so block clicks,
inventory, and tools work normally. `/gm cat` enables creative play in this mode;
other gamemodes restore normal size. Restoring normal size requires headroom.
The mode and selected animal follow you between plots and survive reconnects.
Dedicated permissions use `mchprs.commands.small`; `/gm cat` also requires the usual creative gamemode
permissions. `/help small` explains the command. Player saves upgrade to version 5;
older saves load normally, but older server builds cannot read upgraded saves.

Common simulation commands are `/tps 0` to pause, `/adv <ticks>` to advance game
ticks, and `/adv nano <count>` or `/adv pico <count>` to inspect partial
interpreter execution. Two game ticks form one redstone tick; nano/pico steps
are operations in the interpreter, not fixed fractions of elapsed time.

`/rp analyze` reports circuit structure; `/rp compile` starts admitted compiled
execution and `/rp reset` hands it back to the interpreter. Read
[redpiler architecture](docs/REDPILER_ARCHITECTURE.md) for flags and lifecycle contracts.
`/rhistory` and `/back` record and restore interpreter state.
Plot Git saves build versions, branches, and comparisons.

Hold any carrot on a stick to use the wire pen: right-click
to start, aim to preview, right-click to build and continue. `/wire` supplies a
named pen or resets your route. F cycles horizontal, two vertical drawing planes,
and Free aiming; sneak + F changes the preferred bend. Build intermediate points
and switch planes to form a 3D route. `/wire free` aims at blocks in any direction;
`/wire plane` restores horizontal aiming. Switching items cancels the route;
hold the pen and right-click to start again. `/wire off` disables the tool until
an explicit `/wire`, `/wire free`, or `/wire plane`. The pen builds supports
and dust together, sampling the starting support's passive material and color.
Search is bounded and circuit safety is checked conservatively. It does not
check signal range or insert repeaters.

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
cargo fmt --all -- --check
cargo test --workspace --locked --no-fail-fast
```

Long replays, independent Java captures, and baseline creation are explicit
operations documented under [verification reference](docs/INTERPRETER_ARCHITECTURE.md#8-verification-and-reproduction). Update the
model and affected checks alongside semantic changes. Keep frozen expectations
and source provenance separate from newly captured results.

## License

The repository uses the [MIT license](LICENSE). Upstream contributors and their
work remain recorded in Git history.
