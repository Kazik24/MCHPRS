# MROWW

**Minecraft Redstone o Wysokiej Wydajności**

MROWW is RedstoneFUN's redstone server, built on
[MCHPRS (Minecraft High-Performance Redstone Server)](https://github.com/MCHPR/MCHPRS).

[![Build Status](https://travis-ci.org/MCHPR/MCHPRS.svg?branch=master)](https://travis-ci.org/MCHPR/MCHPRS) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Discord Banner 2](https://discordapp.com/api/guilds/724072903083163679/widget.png)](https://discord.com/invite/svK9JU7)

A Minecraft 1.21.5 creative server built for redstone. Each 256x256 plot runs on a separate thread, allowing for less lag, more concurrency, and many awesome extra features!

MROWW is very different from traditional servers. Because this server is tailored to the use of computation redstone, many things that are a part of Vanilla Minecraft servers don't exist here. That being said, MROWW comes with many of its own unique features.

MCHPRS made it possible to run programs such as [Graph Rendering, Conway's Game of Life, and Mandelbrot Rendering](https://www.youtube.com/watch?v=FDiapbD0Xfg) on CPUs in Minecraft. MROWW uses its [Redpiler](docs/Redpiler.md), the "Redstone Compiler", to run circuits at these speeds.

This branch ports the recovered piston implementation to protocol **770** (Minecraft DataVersion **4325**). See [implementation and validation](docs/PORTING_1_21_5.md) and [Sponge v2/v3 support](docs/SPONGE_V3_IMPLEMENTATION.md). Full piston reference compliance remains a separate follow-up.

## Table of Contents

- [Table of Contents](#table-of-contents)
- [Building](#building)
- [Configuration](#configuration)
    - [LuckPerms](#luckperms)
- [Usage](#usage)
    - [General Commands](#general-commands)
    - [Plot Ownership](#plot-ownership)
    - [Worldedit](#worldedit)
- [Acknowledgments](#acknowledgments)
- [Contributing](#contributing)
- [License](#license)

## Building

If the Rust compiler is not already installed, you can find out how [on their official website](https://www.rust-lang.org/tools/install).

```shell
git clone https://github.com/MCHPR/MCHPRS.git
cd MCHPRS
cargo build --release
```

Once complete, the optimized executable will be located at `./target/release/mchprs` or `./target/release/mchprs.exe` depending on your operating system.

## Configuration

MROWW will generate a `Config.toml` file in the current working directory when starting the server if it does not exist.

Set `MCHPRS_CONFIG` to use a different configuration file. The Docker image ships
the tracked `docker/Config.toml` at `/etc/mchprs/Config.toml`. The production Compose
deployment uses the host's `/srv/mchprs/backend-config/Config.toml`, mounted at
`/run/config/Config.toml`, so its settings persist across image updates.

The executable/crate names, `MCHPRS_CONFIG` and `MCHPRS_LOG` environment variables,
`mchprs.*` permission nodes, and LuckPerms `server=mchprs` context retain their
existing identifiers for compatibility with deployments and saved worlds.

For existing installations, set the top-level `motd` in the active `Config.toml`
to `"§4§lmroww.redstoneFUN.pl §r§71.21.5\n§cMinecraft Redstone o Wysokiej Wydajności"`.
The production backend reads `/srv/mchprs/backend-config/Config.toml`; its custom
MOTD survives image updates. Deploy the updated Velocity image to update the
public proxy listing. New configurations use the MROWW MOTD automatically.

The folowing options are available at the toplevel (under no header):
| Field | Description | Default |
| --- | --- |--- |
| `bind_address` | Bind address and port | `0.0.0.0:25565` |
| `motd` | Message of the day | `"§4§lmroww.redstoneFUN.pl §r§71.21.5\n§cMinecraft Redstone o Wysokiej Wydajności"` |
| `chat_format` | How to format chat message interpolating `username` and `message` with curly braces | `<{username}> {message}` |
| `proxy_chat` | Submit public chat to Velocity; requires authenticated modern forwarding | `false` |
| `max_players` | Maximum number of simultaneous players | `99999` |
| `view_distance` | Maximal distance (in chunks) between players and loaded chunks | `8` |
| `neighbor_update_interval_ms` | Refresh interval for neighboring plot snapshots; `2000` = 0.5 Hz, `0` hides neighbors | `2000` |
| `bungeecord` | Enable compatibility with [BungeeCord](https://github.com/SpigotMC/BungeeCord) | `false` |
| `whitelist` | Whether or not the whitelist (in `whitelist.json`) shoud be enabled | `false` |
| `schemati` | Mimic the verification and directory layout used by the Open Redstone Engineers [Schemati plugin](https://github.com/OpenRedstoneEngineers/Schemati) | `false` |
| `block_in_hitbox` | Allow placing blocks inside of players (hitbox logic is simplified) | true |
| `auto_redpiler` | Use redpiler automatically | true |

To change the plot size edit the constants defined in [plot/mod.rs](./crates/core/src/plot/mod.rs).

Neighboring builds inside the view distance are shown as snapshots, with static
pistons, refreshed only when their chunk data changes. The current plot keeps its
normal simulation and visual update rates. Neighboring plots are read-only until
you enter them; players and sounds are shown only within the current plot.
Unloaded plots use saved data without starting simulation. Snapshot caches are
bounded, and reading neighboring saves does not migrate or rewrite them.

Right-click with a compass in either hand to teleport onto the block you are
pointing at, up to 1,024 blocks away within the current plot. The compass centers
you on top of the block and preserves your view direction. Teleportation requires
space for your full standing body. If the spot above the target is blocked, it
searches nearby surfaces along the last eight blocks of the sight line and a few
blocks above the hit. A miss or lack of a safe surface leaves you in place.
Collision checks are conservative for blocks with complex shapes.

Plot Git saves whole-plot commits, searches history and supports branches and
checkout. Start with `/git commit Working build`, then `/git branch experiment`
and `/git checkout experiment`. `/git diff main experiment` offers private glow
markers; right-click one with a sword to inspect its From/To states. Checkout
preserves unfinished work and pauses simulation. See [Plot Git](docs/PLOT_GIT.md)
for commands, recovery, limits and the `mchprs.commands.git` permission.

For shared public chat between Paper RedstoneFun and MROWW, see
[the network chat setup](docs/NETWORK_CHAT.md). The Velocity and Paper plugins
build with Maven or standard Docker Compose; MROWW includes a native adapter.

### LuckPerms

MROWW reads existing LuckPerms permission data from PostgreSQL, MySQL or MariaDB.
Manage permissions through LuckPerms on another server (`/lp`) or proxy (`/lpb`).
Use a database login with SELECT access only. See [LuckPerms setup](docs/LUCKPERMS.md)
for PostgreSQL configuration, account UUID requirements and compatibility details.

To use LuckPerms, append this to your `Config.toml`:

```toml
[luckperms]
# Optional; defaults to "mysql". PostgreSQL accepts "postgres" or "postgresql".
storage = "mysql"
# Define the address for the database.
host = "localhost"
# Optional; defaults to 3306 for MySQL or 5432 for PostgreSQL.
port = 3306
# The name of the database the LuckPerms data is in.
db_name = "minecraft"
# Credentials for the database.
username = "minecraft"
password = "minecraft"
# The name of the server, used for server specific permissions.
# See: https://luckperms.net/wiki/Context
server_context = "global"
# Optional settings, shown with their defaults:
world_context = "global"
table_prefix = "luckperms_"
plotsquared_compat = false
```

## Usage

### General Commands
Use `/help` for a quick start and topic list. `/help tps`, `/help we`,
`/help plots`, `/help schematics`, `/help pistons`, `/help chat` and
`/help redpiler` explain the usual workflows. `/help rewind` describes the
`/rhistory` and `/back` commands for recording and restoring interpreter ticks.
`//help <command>` still shows detailed WorldEdit arguments and flags.

| Command | Alias | Description |
| --- | --- |--- |
| `/help [topic]` | None | Show the quick-start guide or a topic tutorial. |
| `/tps [tps\|unlimited]` | `/rtps` | Show TPS averages over 2 seconds, 10 seconds and 1 minute, or set game ticks per second in the plot. `0` pauses; `20` is normal game speed and the default for new plots. There are two game ticks in a redstone tick. |
| `/adv [ticks]` | `/radv`, `/radvance` | Advances the plot by `[ticks]` game ticks. |
| `/rhistory [on [ticks]\|off\|status\|limit [MiB]]` | None | Record interpreter history, keeping up to 100 ticks by default; recording stops and clears when the last player leaves the plot; show compressed/uncompressed sizes. Admins can change the shared memory limit (default 2 GiB). |
| `/back [ticks]` | `/rback` | Rewind one or more recorded game ticks and pause the plot. Restores the entire plot, including later edits, and clears WorldEdit undo/redo. |
| `/teleport [player]` | `/tp` | Teleports you to `[player]`. |
| `/setwarp <name>` | None | Saves your exact position and facing direction as a shared server-wide warp. Reusing a name replaces its destination. Names are case-insensitive, using 1-32 letters, digits, underscores or hyphens. |
| `/warp [name]` | None | Visits a saved warp, or lists warps when no name is given. Both warp commands support Tab completion; `/help warps` explains them. Warps persist across restarts in `world/plots.db`. |
| `/teleport [x] [y] [z]` | `/tp` | Teleports you to `[x] [y] [z]`. Supports relative coordinates. Floats can be expressed as described [here](https://doc.rust-lang.org/std/primitive.f64.html#grammar). |
| `/speed [speed]` | None | Sets your flyspeed. |
| `/gamemode [mode]` | `/gmc`, `/gmsp` | Sets your gamemode. |
| `/container [type] [power]` | None | Gives you a container (e.g. barrel) which outputs a specified amount of power when used with a comparator. |
| `/redpiler compile` | `/rp c` | Manually starts redpiler compilation. Available flags: --io-only --optimize --export --update (or in short: -ioeu) |
| `/redpiler reset` | `/rp r` | Stops redpiler. |
| `/toggleautorp` | None | Toggles automatic redpiler compilation. |
| `/adv nano/pico [ticks]` | `/radv nano/pico`, `/radvance nano/pico` | Advances the plot by `[ticks]` redstone nano or pico-ticks, useful for debugging piston circuits. |
| `/stop` | None | Stops the server. |

### Plot Ownership
The plot ownership system in MROWW is very incomplete.
These are the commands that are currently implemented:
| Command | Alias | Description |
| --- | --- |--- |
| `/plot info` | `/p i` | Gets the owner of the plot you are in. |
| `/plot claim` | `/p c` | Claims the plot you are in if it is not already claimed. |
| `/plot auto` | `/p a` | Automatically finds an unclaimed plot and claims. |
| `/plot middle` | None | Teleports you to the center of the plot you are in. |
| `/plot visit [player]` | `/p v` | Teleports you to a player's plot. |
| `/plot tp [x] [z]` | None | Teleports you to the plot at `[x] [y]`. Supports relative coordinates. |
| `/plot lock` | None | Locks the player into the plot so moving outside of the plot bounds does not transfer you to other plots. |
| `/plot unlock` | None | Reverses the locking done by `/plot lock`. |

### Worldedit
MROWW provides its own implementation of [WorldEdit](https://github.com/EngineHub/WorldEdit). Visit their [documentation](https://worldedit.enginehub.org/en/latest/commands/) for more information.
These are the commands that are currently implemented:
| Command | Alias | Description |
| --- | --- |--- |
| `/up` | `/u` | Go upwards some distance |
| `/ascend` | `/asc` | Go up a floor |
| `/descend` | `/desc` | Go down a floor |
| `//pos1` | `//1` | Set position 1 |
| `//pos2` | `//2` | Set position 2 |
| `//hpos1` | `//h1` | Set position 1 to targeted block |
| `//hpos2` | `//h2` | Set position 2 to targeted block |
| `//sel` | None | Clears your worldedit first and second positions. |
| `//set` | None | Sets all the blocks in the region |
| `//replace` | None | Replace all blocks in a selection with another |
| `//copy` | `//c` | Copy the selection to the clipboard |
| `//cut` | `//x` | Cut the selection to the clipboard |
| `//paste` | `//v` | Paste the clipboard's contents (`-a` to ignore air, `-u` to also update) |
| `//undo` | None | Undoes the last action (from history) |
| `//redo` | None | Redoes the last action (from history) |
| `//rstack` | `//rs` | Stack with more options, Refer to [RedstoneTools](https://github.com/paulikauro/RedstoneTools) |
| `/autostack [direction] [count] [spacing] [-e]` | None | Automatically stack your placements and removals in the selected region; `/autostack off` stops it. |
| `//stack` | `//s` | Repeat the contents of the selection |
| `//move` | None | Move the contents of the selection |
| `//count` | None | Counts the number of blocks matching a mask |
| `//load` | None | Loads a schematic from the `./schems/` folder. Make sure the schematic in the Sponge format if there are any issues. |
| `//save` | None | Save a schematic to the `./schems/` folder. |
| `//expand` | `//e` | Expand the selection area |
| `//contract` | None | Contract the selection area |
| `//shift` | None | Shift the selection area |
| `//flip` | `//f` | Flip the contents of the clipboard across the origin |
| `//rotate` | `//r` | Rotate the contents of the clipboard |
| `//update` | None | Updates all blocks in the selection (`-p` to update the entire plot) |
| `//help` | None | Displays help for WorldEdit commands |

Select a source region before enabling `/autostack`, for example
`/autostack east 5 2` for five copies spaced two blocks apart. Defaults are your
look direction, one copy, and spacing two. It mirrors your future placements
and removals; existing blocks can be copied first with `//rstack`. The source
region stays fixed even if you change the selection or use `-e` to expand it.
WorldEdit operations and simulation changes do not trigger automatic copies.
Leaving the plot, disconnecting, or running `/autostack off` clears the session.

## Acknowledgments
- [@AL1L](https://github.com/AL1L) for his contributions to worldedit and other various features.
- [@DavidGarland](https://github.com/DavidGarland) for a faster and overall better implementation of `get_entry` in the in-memory storage. This simple function runs 30% of the runtime for redstone.

## Contributing
Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License
[MIT](https://choosealicense.com/licenses/mit/)
