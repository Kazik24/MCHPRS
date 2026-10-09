# Permissions and rank metadata

Permission evaluation is implemented in
[permissions/mod.rs](../crates/core/src/permissions/mod.rs). The server reads
LuckPerms data; it does not provision groups or write permission tables. Database
policy belongs to the deployment, so rank names alone do not grant actions.

## Configuration and evaluation

Example configuration, with deployment-specific credentials supplied locally:

```toml
[luckperms]
storage = "postgres" # "postgresql" alias, or "mysql" for MySQL/MariaDB
host = "localhost"
db_name = "permissions"
username = "reader"
password = "replace-locally"
server_context = "mchprs"
world_context = "redstoneplots"
table_prefix = "luckperms_"
mchprs_permissions = true
redstonefun_ranks = false
plotsquared_compat = false
```

Use a read-only database account. The optional port defaults to 5432 for
PostgreSQL or 3306 for MySQL. `mchprs_permissions=true` maps handler nodes into
`mchprs.*`, including `minecraft.command.<name>` into `mchprs.commands.<name>`,
and disables PlotSquared fallback. Missing permissions deny access in this mode.
Legacy mode checks the original nodes and optionally the supported PlotSquared
permission-pack mappings. Standalone behavior is defined by
[Player::has_permission](../crates/core/src/player.rs).

Evaluation supports user nodes, inherited groups, explicit denials, wildcards,
expiry, and configured static `server`/`world` contexts. Other dynamic contexts
are outside this reader. Matching nodes have an implementation-defined priority
from user/group depth, specificity, and scope; a denial wins an equal-priority
tie. The applicable cache expires after 30 seconds and stale data cannot keep
granting access when refresh fails. This is a subset of the LuckPerms engine.

`redstonefun_ranks` selects rank metadata and styled messages; it does not assign
group permissions. Login identity is a UUID, not a current display name. Database
records must use the UUID established by the configured authentication/forwarding
path; stale nickname records do not transfer ownership or membership.

## Action gates

In dedicated mode, ordinary commands require `mchprs.access.commands` as well as
their individual permission. Editing additionally requires base build access,
the action permission, and plot ownership/membership or the applicable override.

| Permission | Gate |
| --- | --- |
| `mchprs.access.join` | Admission after authentication. |
| `mchprs.access.chat` | Ordinary public chat. |
| `mchprs.access.commands` | Backend command access. |
| `mchprs.build` | Base plot editing access. |
| `mchprs.build.place`, `.break`, `.interact` | Placement, destruction, and circuit interaction. |
| `mchprs.build.sign`, `.container`, `.commandblock` | Sign/container/command-block changes. |
| `mchprs.commands.commandblock.edit` | Additional command-block editor gate. |
| `mchprs.inventory.creative` | Creative inventory edits and picking. |
| `mchprs.plots.admin.interact.other`, `.unowned` | Extend edits to other/unowned plots. |

Check [player.rs](../crates/core/src/player.rs),
[packet_handlers.rs](../crates/core/src/plot/packet_handlers.rs), and
[commands.rs](../crates/core/src/plot/commands.rs) for the complete gate at each
trust boundary. Possession of an item or permission to run a command does not
waive its plot or packet validation.

## Simulation and build commands

| Command | Dedicated permission |
| --- | --- |
| `/help`, `/version` | `mchprs.commands.help`, `.version` |
| `/tp`, `/warp`, `/setwarp`, `/speed` | `mchprs.commands.teleport`, `.warp`, `.setwarp`, `.speed` |
| `/small [on|off]` | `mchprs.commands.small` |
| `/gm cat` | `mchprs.commands.gamemode`, `.gamemode.creative`, and `.small` |
| `/tps` | `mchprs.commands.rtps.view` or `.set` |
| `/adv` | `mchprs.commands.radvance` |
| `/wsr` | `mchprs.commands.worldsendrate.view` or `.set` |
| `/screenonly` | `mchprs.commands.screenonly` plus `.view` or `.set` |
| `/piston_anim` | `mchprs.commands.piston_anim` plus `.view` or `.set` |
| `/rp`, `/redpiler` | `mchprs.commands.redpiler.analyze`, `.compile`, `.reset`, `.inspect`, or `.help` |
| `/toggleautorp` | `mchprs.commands.toggleautorp` |
| `/rhistory` | `mchprs.commands.rhistory` plus the requested subcommand node |
| `/back` | `mchprs.commands.rback` |
| `/git` | `mchprs.commands.git` |

Command spelling and permission spelling are separate: `/tps`, `/adv`, and
`/back` retain the legacy permission-node names above. `/rtps`, `/radv`,
`/radvance`, and `/rback` are removed command aliases. Read-only `/rp analyze`
has its own permission and does not require plot edit access.

WorldEdit gates retain the `mchprs.worldedit.*` namespace for navigation,
selection, region, clipboard, history, and analysis commands. Native additions
use `mchprs.we.update`, `.invalidatecaches`, and `.replacecontainer`. Redstone tools
use `mchprs.redstonetools.find`, `.signsearch`, `.rstack`, `.autostack`, `.container`,
`.cursel`, and `.wire`. Mutating commands still validate current-plot access and bounds.

The [wire pen](WIRE_TOOL.md) requires `mchprs.redstonetools.wire`, plot edit
access, and `mchprs.build.place` when committing. Legacy mode checks
`redstonetools.wire` and the existing WorldEdit ownership/member/bypass gate.
Possession of its carrot on a stick does not grant tool access. `/wire off`
remains available after tool permissions change. Search budgets and unsupported
circuit contexts apply regardless of permissions.

## Numeric limits and rank budgets

`numeric_limit(prefix)` considers explicit numeric nodes whose effective value
is positive and chooses the largest value. Denials and expiry apply. A wildcard
grant does not itself supply a number; deny larger inherited values when
configuring a smaller cap.

| Node | Meaning |
| --- | --- |
| `mchprs.plots.limit.<count>` | Owned-plot cap; missing numeric grants default to one. |
| `mchprs.plots.limit.unlimited` | Remove that ownership cap; claim permission remains separate. |
| `mchprs.history.limit.<ticks>` | History buffer/rewind cap in dedicated mode. |
| `mchprs.plots.admin.rewind.unlimited` | Remove the permission-based history tick ceiling. |
| `mchprs.plots.admin.rewind.memory` | Change the shared history memory limit. |
| `mchprs.git.storage.<MiB>` | Plot owner's disk allowance, bounded by server Git limits. |

Resource validation, work budgets, and allocation failures still apply to
unlimited permission holders. History recording clears when the last player
leaves the plot. The Git allowance follows the plot owner even when a member
operates on the plot.

[Rank](../crates/core/src/permissions/rank.rs) also selects a compilation-budget
multiplier when valid rank metadata is enabled:

| Groups | Multiplier |
| --- | --- |
| `default`, `builder` | 1 |
| `advanced` | 2 |
| `expert` | 4 |
| `engineer`, `moderator`, `admin` | 8 |

Expiry/stale metadata falls back to multiplier 1. These multipliers raise
bounded compiler work limits; they do not permit unsupported semantics. Display
rank priority follows the same group order from default through admin; database
prefixes may override fallback colors. Configure inheritance and action grants
through the deployment's LuckPerms installation and preserve the selected
server/world contexts when changing them.
