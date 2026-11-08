# Permissions and rank metadata

Permission evaluation is implemented in
[permissions/mod.rs](../crates/core/src/permissions/mod.rs). The server reads
LuckPerms data; it does not provision groups or write permission tables. Database
policy belongs to the deployment, so rank names alone do not grant actions.

## Configuration and evaluation

Two sources are supported. The default is the database; `source = "redbox"`
instead asks RedBox on the proxy to resolve the player's LuckPerms data with the
native API and send the effective permission map plus group/prefix over the
`redbox:main` channel. No database connection is opened and inheritance,
wildcards, negations and dynamic contexts are handled by LuckPerms itself.

```toml
[luckperms]
source = "redbox" # requires redbox = true; "proxy" is an accepted alias
server_context = "mchprs"
world_context = "redstoneplots"
plotsquared_compat = true
redstonefun_ranks = true
mchprs_permissions = true
```

The alternative reads raw nodes directly from a database and resolves inheritance
in MROWW. Deployment-specific credentials are supplied locally:

```toml
[luckperms]
source = "database" # default when omitted
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

Database evaluation supports user nodes, inherited groups, explicit denials,
wildcards, expiry, and configured static `server`/`world` contexts. Other dynamic
contexts are outside this reader. Matching nodes have an implementation-defined
priority from user/group depth, specificity, and scope; a denial wins an
equal-priority tie. The applicable cache expires after 30 seconds and stale data
cannot keep granting access when refresh fails. This is a subset of the LuckPerms
engine. In proxy mode LuckPerms performs that resolution and MROWW only matches the
resulting map, so those caveats do not apply.

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
| `/tp` (internal `--highlight` links only), `/speed` | `mchprs.commands.teleport`, `.speed` |
| `/small [on|off]` | `mchprs.commands.small` |
| `/gm cat` | `mchprs.commands.gamemode`, `.gamemode.creative`, and `.small` |
| `/tps` | `mchprs.commands.rtps.view` or `.set` |
| `/adv` | `mchprs.commands.radvance` |
| `/visual` | `mchprs.commands.visual` plus `.view` or `.set` |
| `/rp`, `/redpilator` | `mchprs.commands.redpilator.analyze`, `.compile`, `.reset`, `.inspect`, or `.help` |
| `/toggleautorp` | `mchprs.commands.toggleautorp` |
| `/rhistory` | `mchprs.commands.rhistory` plus the requested subcommand node |
| `/rev` | `mchprs.commands.rback` |
| `/git` | `mchprs.commands.git` |
| `/waveform` | `mchprs.commands.waveform` plus `.enable`, `.disable`, `.status`, `.add`, `.addbulk`, `.remove`, `.list`, `.clear`, `.reset`, or `.web` |

Command spelling and permission spelling are separate: `/tps`, `/adv`, and
`/rev` retain the legacy permission-node names above. `/rtps`, `/radv`,
`/radvance`, and `/rback` are removed command aliases. Read-only `/rp analyze`
has its own permission and does not require plot edit access. The network
commands `/warp`, `/setwarp`, and `/tp` (player and coordinates) are provided by
the RedBox proxy, not MROWW; the `commands.warp` and `commands.setwarp` nodes no
longer exist. `/tp` remains only for the internal redstone-tool `--highlight`
diagnostic links, which still require `mchprs.commands.teleport`.
RedBox's network `/back` command is separate from the native tick rewind `/rev`.

With the RedBox bridge enabled, the proxy also owns `/speed`, `/gm` and
`/gamemode` and checks their `redbox.*` permissions before sending backend
operations. MROWW accepts all four standard gamemodes and `SPEED` messages with
`auto`, `walk` or `fly` selection. `auto` uses the player's current flight state.
Speed values must be finite and in `(0, 100]`; effective multipliers cap at 10
for flight and 5 for walking, matching RedBox Paper. Both speeds are saved with
the player. Direct backend `/speed` retains its flight-only `0–10` syntax and
`mchprs.commands.speed` gate.

WorldEdit gates retain the `mchprs.worldedit.*` namespace for navigation,
selection, region, clipboard, history, and analysis commands. Native additions
use `mchprs.we.update`, `.invalidatecaches`, and `.replacecontainer`. Redstone tools
use `mchprs.redstonetools.find`, `.signsearch`, `.rstack`, `.autostack`, `.container`,
`.cursel`, and `.wire`. Mutating commands still validate current-plot access and bounds.

The wire pen requires `mchprs.redstonetools.wire`, plot edit
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
| `mchprs.waveform.signals.limit.<count>` | Explicit numeric signal allowance; also capped by the actor's rank profile and server limits. |
| `mchprs.waveform.ticks.limit.<ticks>` | Explicit numeric capture-window allowance, with the same ceilings. |

Resource validation, work budgets, and allocation failures still apply to
unlimited permission holders. History recording clears when the last player
leaves the plot. The Git allowance follows the plot owner even when a member
operates on the plot.

Every `/waveform` action requires command access, the family permission, its
action permission and plot edit access. `/waveform on` maps to `.enable`; `off`
maps to `.disable`. HTTP reads are public, including targeted command-block
`tellraw` output; they need no Minecraft permissions. Recordings capture plain
text only. `off` discards history/events and keeps probes. `clear` empties history
and keeps probes/recording; `reset` only warns, and `reset confirm` removes both
and stops recording. Both reset forms require the `.reset` action permission.
`addbulk <name> <bits>` selects probes with left clicks from low to high, then
saves them together. `cancel` only drops the caller's pending selection. Clicks
recheck permissions, plot access and quotas; leaving the plot cancels selection.

The deployed waveform policy grants the family permission, its action wildcard
and explicit signal/tick numeric nodes to these existing groups, scoped to
`server=mchprs`:

| Rank / existing group | Signals | Ticks |
| --- | ---: | ---: |
| Default / `default` | 16 | 100 |
| B / `builder` | 16 | 100 |
| Z / `advanced` | 64 | 1,000 |
| E / `expert` | 1,024 | 1,000 |
| I / `engineer` | 1,024 | 1,000 |
| M / `moderator` | 1,024 | 10,000 |
| A / `admin` | 1,024 | 10,000 |

Plot editing rights remain required; these grants do not bypass ownership or
build access. Builder's configured profile ceiling is 16 signals/100 ticks; the
other-rank profile ceiling is 1,024/10,000. Numeric grants set each group's lower
allowance. Missing numeric grants admit zero, and stale/unknown external rank
metadata denies new allocation. Dedicated mode uses `mchprs.`; legacy mode omits
that prefix. Standalone servers without a permission provider use the configured
other-rank ceiling. Permission-cache refresh can take up to 30 seconds.

[Rank](../crates/core/src/permissions/rank.rs) supplies display rank priority
from default through admin; database prefixes may override fallback colors.
Configure inheritance and action grants through the deployment's LuckPerms
installation and preserve the selected server/world contexts when changing them.

Redpilator access is intended for every rank above B: Z, E, I, M and A. G
(`default`) and B (`builder`) should not receive access. Compilation has these
wall-clock budgets, including snapshot preparation and cached/backend admission:

| Rank / existing group | Compilation time |
| --- | ---: |
| Z / `advanced` | 30 seconds |
| E / `expert` | 120 seconds |
| I / `engineer`, M / `moderator`, A / `admin` | No additional time limit |

These are cooperative cancellation deadlines: an in-progress compiler phase or
external native compiler may take longer to return, but an expired result never
activates or enters the plot cache. Existing structural/allocation limits still
apply to I and higher. Unknown or stale rank metadata denies new compilation;
standalone servers without a permission provider retain their existing behavior.
Automatic compilation uses the greatest allowance among current authorized plot
editors with both compile and automatic-compilation permissions; an unauthorized
visitor cannot raise the budget.

`/rp off` and `/rp r` are aliases for `/rp reset`; all use `mchprs.commands.redpilator.reset`
and discard this plot's prepared executor and sampling cache.
`/rp clear` uses the same permission and also clears the process-shared owned LLVM artifact cache. Active
executors on other plots and custom `MCHPRS_LLVM_DLL` files are retained.

Budgets do not grant command access or override explicit denials. Enforce access
through LuckPerms, preserving the deployment's contexts and higher-rank grants.
For the existing inheritance chain (`expert` inherits `advanced`, and higher
ranks inherit these grants), run these commands in the LuckPerms console after
deploying the budget enforcement:

```text
lp group advanced permission set mchprs.commands.redpilator.* true server=mchprs
lp group advanced permission set mchprs.commands.toggleautorp true server=mchprs
```

The existing engineer grants remain unchanged. This code change does not modify
the live LuckPerms database; the two deployment grants are required separately.
