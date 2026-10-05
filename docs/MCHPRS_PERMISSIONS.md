# MCHPRS ranks and permissions

## Active configuration

The private `/srv/mchprs/backend-config/Config.toml` enables these LuckPerms
settings alongside its existing database credentials:

```toml
[luckperms]
# Existing connection settings omitted.
server_context = "mchprs"
world_context = "redstoneplots"
mchprs_permissions = true
redstonefun_ranks = true
```

`mchprs_permissions` isolates handler permission checks under `mchprs.*` and
disables the shared PlotSquared permission-pack fallback. Missing permissions
deny access. The reader still supports user overrides, group inheritance,
explicit denials, wildcard nodes and configured server/world contexts.
`redstonefun_ranks` enables rank prefixes and the RedstoneFun join/chat format;
it grants no permissions by itself. Both settings default to false.

Permission and rank changes take effect when players reconnect. The server
reads LuckPerms using its existing read-only database login. Manage later
changes through the existing LuckPerms plugin; MCHPRS does not write the tables.
Supported contexts are static `server` and `world`; other dynamic contexts do
not apply. This reader is not the complete LuckPerms engine.

## Rank policy

| Database group | Display | Current MCHPRS access |
| --- | --- | --- |
| `admin` | Bold red `[A]` | All permissions, including other/unowned plots and administration |
| `moderator` | Bold green `[M]` | All permissions, including other/unowned plots and administration |
| `engineer` | Cyan `[I]` | Edit own plots and use ordinary commands |
| `expert` | Purple `[E]` | Edit own plots and use ordinary commands |
| `advanced` | Orange `[Z]` | Edit own plots and use ordinary commands |
| `builder` | Yellow `[B]` | Join and chat; spectator mode, no edits or backend commands |
| `default` | Gray `[G]` | Join and chat; spectator mode, no edits or backend commands |

The existing database prefixes supply the exact tag and nickname colors. All
seven database groups currently have equal weights, so the display rank is the
highest applicable known group in the order above. Parent groups contribute
permissions without replacing a higher rank's prefix. The selected group's
highest applicable prefix priority is used; built-in colors are a fallback if
that group has no prefix. No rank-name check grants access to an action.

Join: `[+] Nick`, with bold dark-gray brackets, bold dark-green `+`, and a gray
nickname. Chat: `[Rank] Nick » message`, with the database prefix/nickname colors,
dark-gray `»`, and gray literal message text. Player text cannot inject prefix
formatting. These are global messages, including players on different plots.

The baseline has `mchprs.* = false` plus join/chat grants on `default`.
`advanced` has explicit ordinary-command and own-plot editing grants, and
explicit denials for administration. `expert` and `engineer` inherit `advanced`
through their existing memberships. `builder` inherits the read-only baseline.
`admin` and `moderator` have `mchprs.* = true`.
All new baseline nodes use `server=mchprs`; Paper's existing nodes and group
inheritance were retained. User overrides can deliberately change this policy.

## Access, ownership and direct actions

| Permission | Controls |
| --- | --- |
| `mchprs.access.join` | Join MCHPRS after authentication |
| `mchprs.access.chat` | Send ordinary chat |
| `mchprs.access.commands` | Run any backend command; command-specific grants are also required |
| `mchprs.build` | Base permission to edit an owned plot |
| `mchprs.plots.admin.interact.other` | Extend editing to someone else's plot |
| `mchprs.plots.admin.interact.unowned` | Extend editing to unclaimed plots |
| `mchprs.build.place` | Place blocks |
| `mchprs.build.break` | Break blocks |
| `mchprs.build.interact` | Change repeaters/comparators, use levers/buttons, trigger pressure plates and other block interactions |
| `mchprs.build.sign` | Update sign text |
| `mchprs.build.container` | Open/change container contents |
| `mchprs.build.commandblock` | Open/update command blocks; also requires the command-block editing node below |
| `mchprs.inventory.creative` | Creative inventory changes and block picking |

Direct world actions require the base build permission, the action permission,
and ownership or the corresponding other/unowned permission. WorldEdit and
mutating plot commands use the same ownership rule. Inventory and command access
are independent of building. A player without `mchprs.build` starts in spectator
mode; spectators do not trigger pressure plates. Ordinary ranks cannot bypass
ownership by carrying a command-block item or sending a sign/container packet.

## Native commands

All commands also require `mchprs.access.commands`. Aliases use the same
permission as their canonical command.

| Command | Permission(s) |
| --- | --- |
| `/help`, `/version` | `mchprs.commands.help`, `mchprs.commands.version` |
| `/tp`, `/teleport` | `mchprs.commands.teleport` |
| `/speed` | `mchprs.commands.speed` |
| `/gamemode`, `/gmc`, `/gmsp` | `mchprs.commands.gamemode`, plus `.creative` or `.spectator` |
| `/rtps` | `mchprs.commands.rtps.view` or `.set`; `timings` uses `.view` |
| `/wsr`, `/worldsendrate` | `mchprs.commands.worldsendrate.view` or `.set` |
| `/screenonly` | `mchprs.commands.screenonly`, plus `.view` or `.set` |
| `/piston_anim`, `/bisdon_anim` | `mchprs.commands.piston_anim`, plus `.view` or `.set` |
| `/rp`, `/redpiler` | `mchprs.commands.redpiler.compile`, `.reset`, `.inspect` or `.help` |
| `/radv`, `/radvance` | `mchprs.commands.radvance` |
| `/toggleautorp` | `mchprs.commands.toggleautorp` |
| `/curse`, `/bless` | `mchprs.commands.curse`, `mchprs.commands.bless` |
| `/rhistory` | `mchprs.commands.rhistory`, plus `.status`, `.enable`, `.disable`, `.limit.view` or `.limit.set` |
| `/rback` | `mchprs.commands.rback` |
| `/say`, `/tellraw` | `mchprs.commands.say`, `mchprs.commands.tellraw` |
| `/stop`, `/whitelist` | `mchprs.commands.stop`, `mchprs.commands.whitelist` |
| Command-block editor | `mchprs.commands.commandblock.edit` |

Changing plot timing, render settings, redpiler state, history or curse state
also requires permission to edit the current plot. Viewing settings does not
change a plot. Changing the shared history memory limit additionally requires
`mchprs.plots.admin.rewind.memory`; exceeding the ordinary history tick limit
requires `mchprs.plots.admin.rewind.unlimited`.

| `/plot` subcommands | Permission |
| --- | --- |
| `info`, `i` | `mchprs.plots.info` |
| `claim`, `c` | `mchprs.plots.claim` |
| `auto`, `a` | `mchprs.plots.auto` |
| `visit`, `v`, `teleport`, `tp` | `mchprs.plots.visit` |
| `middle` | `mchprs.plots.middle` |
| `lock`, `unlock` | `mchprs.plots.lock` |
| `select`, `sel` | `mchprs.plots.select` |

## WorldEdit and redstone tools

WorldEdit uses the existing per-command nodes with the `mchprs.` prefix:

| Commands | Permission suffix after `mchprs.worldedit.` |
| --- | --- |
| `/up`, `/ascend`, `/descend` | `navigation.up`, `.ascend`, `.descend` |
| `//pos1`, `//pos2` | `selection.pos` |
| `//hpos1`, `//hpos2` | `selection.hpos` |
| `//sel` | `selection.sel` |
| `//expand`, `//contract`, `//shift` | `selection.expand`, `.contract`, `.shift` |
| `//set`, `//replace`, `//stack`, `//move` | `region.set`, `.replace`, `.stack`, `.move` |
| `//copy`, `//cut`, `//paste`, `//load`, `//save`, `//flip`, `//rotate` | `clipboard.copy`, `.cut`, `.paste`, `.load`, `.save`, `.flip`, `.rotate` |
| `//undo`, `//redo` | `history.undo`, `.redo` |
| `//count` | `analysis.count` |
| `//help`, `//wand` | `help`, `wand` |

The MCHPRS-specific commands retain their existing native nodes:
`mchprs.we.update`, `mchprs.we.invalidatecaches`, and
`mchprs.we.replacecontainer`. Redstone tools use
`mchprs.redstonetools.find`, `.signsearch`, `.rstack`, `.container`, and `.cursel`.
WorldEdit commands and plot searches/stacking require editing access to the
current plot. Selection bounds still stay inside that plot, even for staff.

## Changing individual permissions

Use the existing LuckPerms plugin's standard permission commands with a context.
For example, disable tick-rate changes for Zaawansowany and its inheriting ranks,
or deny only Lord225's stop command:

```text
/lp group advanced permission set mchprs.commands.rtps.set false server=mchprs
/lp user Lord225 permission set mchprs.commands.stop false server=mchprs
```

To reverse an override, use `permission unset` with the same context. Explicit
user nodes take precedence over inherited group nodes. More specific scoped
nodes can override wildcard grants or denials; an equal-priority denial wins.
The standard syntax is documented in the
[LuckPerms permission command reference](https://luckperms.net/wiki/Permission-Commands).
Reconnect affected MCHPRS players after saving a change. Deployment does not
reapply the baseline or overwrite later permission edits.

## Provisioning record: 2026-10-05

44 group permission rows were added in the `mchprs` server scope. Lord225's
authenticated UUID is `ec223c83-35a1-4838-9429-76de03eb2fb8`. His permanent global
`group.engineer` membership was replaced with `group.moderator`, and
`luckperms_players.primary_group` was set to `moderator`. The two existing
`rf2.pcmd.*` user permissions were preserved. No existing group nodes, prefixes,
table definitions, Paper files or Paper processes were changed.

The validated pre-change database dump and private backend configuration are in
`/srv/mchprs/deploy-backups/ranks-20261005T125500Z/`. The old backend image is
tagged `mchprs-mchprs:before-ranks-20261005` for rollback. World/player ownership
was not migrated; old offline UUIDs still require verified linking.

The release build succeeded and was deployed with the standard Compose command.
Backend image:
`sha256:045065a44377b152c50889c2c436c411bc354c033e44c472156e9608cb41cfdd`.
An authenticated Lord225 session and plot commands were observed after startup.
No automated tests were added or run for this change; in-game visual styling has
not been confirmed. Paper's existing process and configuration hashes were
unchanged.

## Requested plot cleanup: 2026-10-05

With MCHPRS stopped, the whole `world/` directory was archived and its compressed
backup validated at
`/srv/mchprs/deploy-backups/plot-cleanup-20261005T132052Z/world.tar.gz`.
The saved plots `p-1,0`, `p0,-1`, `p1,-1` and `p1,0`, plus `p1,0.bak`, were removed.
The central `p0,0` file and its two backups were retained; its SHA-256 was
identical before and after cleanup. Plot database ownership/visual rows outside
`(0,0)` were removed, and the SQLite integrity check passed. The central plot's
Lord225 ownership and all player files were preserved. MCHPRS was restarted;
Paper remained running. Exploring other coordinates can generate fresh empty
plots normally.
