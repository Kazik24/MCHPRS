# MCHPRS rank permissions

Database snapshot: 2026-10-05, including the restriction of /curse and /bless to Admin and Moderator. Applies to `server=mchprs`, world context `redstoneplots`.

These are effective group permissions, including inheritance. Individual user overrides can change them. Rank names and colors do not grant access; permission nodes do. Unlisted actions are denied for non-staff.

See [the permission catalog](MCHPRS_PERMISSIONS.md) for individual command/action nodes and configuration.

## Admin — [A]

- Database group: `admin`; bold red tag.
- Allowed: `mchprs.* = true` — every MCHPRS permission, including edits on owned, other, and unclaimed plots.
- Allowed: `mchprs.plots.admin.rewind.unlimited = true` — no history tick-count ceiling.
- Includes server shutdown, whitelist management, global say/tellraw, and shared history memory settings.
- History allocation and server resource validation still apply.

## Moderator — [M]

- Database group: `moderator`; bold green tag.
- Allowed: `mchprs.* = true` — every MCHPRS permission, including edits on owned, other, and unclaimed plots.
- Allowed: `mchprs.plots.admin.rewind.unlimited = true` — no history tick-count ceiling.
- Includes server shutdown, whitelist management, global say/tellraw, and shared history memory settings.
- History allocation and server resource validation still apply.

## Inżynier — [I]

- Database group: `engineer`; cyan tag.
- Join and chat; creative inventory and ordinary backend commands.
- Edit, interact, use WorldEdit, and change simulation/render/history settings **only on plots owned by the player**.
- Maximum history buffer and rewind request: **1000 game ticks**.

### Allowed permissions

- `mchprs.access.join = true`
- `mchprs.access.chat = true`
- `mchprs.access.commands = true`
- `mchprs.build = true`
- `mchprs.build.* = true`
- `mchprs.inventory.creative = true`
- `mchprs.worldedit.* = true`
- `mchprs.we.* = true`
- `mchprs.redstonetools.* = true`
- `mchprs.plots.info = true`
- `mchprs.plots.claim = true`
- `mchprs.plots.auto = true`
- `mchprs.plots.visit = true`
- `mchprs.plots.middle = true`
- `mchprs.plots.lock = true`
- `mchprs.plots.select = true`
- `mchprs.commands.help = true`
- `mchprs.commands.version = true`
- `mchprs.commands.teleport = true`
- `mchprs.commands.speed = true`
- `mchprs.commands.gamemode = true`
- `mchprs.commands.gamemode.* = true`
- `mchprs.commands.rtps.* = true`
- `mchprs.commands.worldsendrate.* = true`
- `mchprs.commands.screenonly.* = true`
- `mchprs.commands.piston_anim.* = true`
- `mchprs.commands.redpiler.* = true`
- `mchprs.commands.radvance = true`
- `mchprs.commands.toggleautorp = true`
- `mchprs.commands.rhistory = true`
- `mchprs.commands.rhistory.* = true`
- `mchprs.commands.rback = true`
- `mchprs.commands.commandblock.edit = true`
- `mchprs.history.limit.200 = true`
- `mchprs.history.limit.1000 = true` — the larger effective numeric limit is used.

### Denied permissions

- `mchprs.commands.curse = false`
- `mchprs.commands.bless = false`

- `mchprs.plots.admin.* = false`
- `mchprs.plots.worldedit.bypass = false`
- `mchprs.commands.stop = false`
- `mchprs.commands.whitelist = false`
- `mchprs.commands.say = false`
- `mchprs.commands.tellraw = false`
- Editing other players' plots or unclaimed plots; changing the shared history memory budget; unlimited history.

## Ekspert — [E]

- Database group: `expert`; purple tag.
- Join and chat; creative inventory and ordinary backend commands.
- Edit, interact, use WorldEdit, and change simulation/render/history settings **only on plots owned by the player**.
- Maximum history buffer and rewind request: **200 game ticks**.

### Allowed permissions

- `mchprs.access.join = true`
- `mchprs.access.chat = true`
- `mchprs.access.commands = true`
- `mchprs.build = true`
- `mchprs.build.* = true`
- `mchprs.inventory.creative = true`
- `mchprs.worldedit.* = true`
- `mchprs.we.* = true`
- `mchprs.redstonetools.* = true`
- `mchprs.plots.info = true`
- `mchprs.plots.claim = true`
- `mchprs.plots.auto = true`
- `mchprs.plots.visit = true`
- `mchprs.plots.middle = true`
- `mchprs.plots.lock = true`
- `mchprs.plots.select = true`
- `mchprs.commands.help = true`
- `mchprs.commands.version = true`
- `mchprs.commands.teleport = true`
- `mchprs.commands.speed = true`
- `mchprs.commands.gamemode = true`
- `mchprs.commands.gamemode.* = true`
- `mchprs.commands.rtps.* = true`
- `mchprs.commands.worldsendrate.* = true`
- `mchprs.commands.screenonly.* = true`
- `mchprs.commands.piston_anim.* = true`
- `mchprs.commands.redpiler.* = true`
- `mchprs.commands.radvance = true`
- `mchprs.commands.toggleautorp = true`
- `mchprs.commands.rhistory = true`
- `mchprs.commands.rhistory.* = true`
- `mchprs.commands.rback = true`
- `mchprs.commands.commandblock.edit = true`
- `mchprs.history.limit.200 = true`

### Denied permissions

- `mchprs.commands.curse = false`
- `mchprs.commands.bless = false`

- `mchprs.plots.admin.* = false`
- `mchprs.plots.worldedit.bypass = false`
- `mchprs.commands.stop = false`
- `mchprs.commands.whitelist = false`
- `mchprs.commands.say = false`
- `mchprs.commands.tellraw = false`
- Editing other players' plots or unclaimed plots; changing the shared history memory budget; unlimited history.

## Zaawansowany — [Z]

- Database group: `advanced`; orange tag.
- Join and chat; creative inventory and ordinary backend commands.
- Edit, interact, use WorldEdit, and change simulation/render/history settings **only on plots owned by the player**.
- Maximum history buffer and rewind request: **200 game ticks**.

### Allowed permissions

- `mchprs.access.join = true`
- `mchprs.access.chat = true`
- `mchprs.access.commands = true`
- `mchprs.build = true`
- `mchprs.build.* = true`
- `mchprs.inventory.creative = true`
- `mchprs.worldedit.* = true`
- `mchprs.we.* = true`
- `mchprs.redstonetools.* = true`
- `mchprs.plots.info = true`
- `mchprs.plots.claim = true`
- `mchprs.plots.auto = true`
- `mchprs.plots.visit = true`
- `mchprs.plots.middle = true`
- `mchprs.plots.lock = true`
- `mchprs.plots.select = true`
- `mchprs.commands.help = true`
- `mchprs.commands.version = true`
- `mchprs.commands.teleport = true`
- `mchprs.commands.speed = true`
- `mchprs.commands.gamemode = true`
- `mchprs.commands.gamemode.* = true`
- `mchprs.commands.rtps.* = true`
- `mchprs.commands.worldsendrate.* = true`
- `mchprs.commands.screenonly.* = true`
- `mchprs.commands.piston_anim.* = true`
- `mchprs.commands.redpiler.* = true`
- `mchprs.commands.radvance = true`
- `mchprs.commands.toggleautorp = true`
- `mchprs.commands.rhistory = true`
- `mchprs.commands.rhistory.* = true`
- `mchprs.commands.rback = true`
- `mchprs.commands.commandblock.edit = true`
- `mchprs.history.limit.200 = true`

### Denied permissions

- `mchprs.commands.curse = false`
- `mchprs.commands.bless = false`

- `mchprs.plots.admin.* = false`
- `mchprs.plots.worldedit.bypass = false`
- `mchprs.commands.stop = false`
- `mchprs.commands.whitelist = false`
- `mchprs.commands.say = false`
- `mchprs.commands.tellraw = false`
- Editing other players' plots or unclaimed plots; changing the shared history memory budget; unlimited history.

## Budowniczy — [B]

- Database group: `builder`; yellow tag.
- Allowed: `mchprs.access.join = true`.
- Allowed: `mchprs.access.chat = true`.
- Baseline: `mchprs.* = false`, with only the join/chat exceptions above.
- Spectator access: cannot place/break blocks, change signs or containers, interact with circuits, or trigger pressure plates.
- No creative inventory edits, backend commands, WorldEdit, redstone tools, plot claims, simulation settings, or history commands.

## Gracz — [G]

- Database group: `default`; gray tag.
- Allowed: `mchprs.access.join = true`.
- Allowed: `mchprs.access.chat = true`.
- Baseline: `mchprs.* = false`, with only the join/chat exceptions above.
- Spectator access: cannot place/break blocks, change signs or containers, interact with circuits, or trigger pressure plates.
- No creative inventory edits, backend commands, WorldEdit, redstone tools, plot claims, simulation settings, or history commands.

## Inheritance and limits

- `builder` inherits `default`.
- `advanced` inherits `builder` and `default`.
- `expert` inherits `advanced`, `builder`, and `default`.
- `engineer` inherits `expert`, `advanced`, `builder`, and `default`; its 1000-tick node increases the inherited 200-tick cap.
- `admin` and `moderator` each have their own MCHPRS wildcard grant.
- The default denial remains inherited by ordinary ranks; more specific positive nodes grant their listed actions.
- History tick limits use the maximum effective positive numeric node. For a smaller custom limit, deny larger inherited numeric nodes.
- Staff have all permissions by the user-requested policy. Resource bounds and validated inputs still apply.
