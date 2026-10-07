# RedstoneVC and MROWW client presentation parity

RedstoneVC uses colored chat, hover text, clipboard clicks and glowing glass
markers to present version control. MROWW already has Plot Git, but its chat
layout and interaction flow differ. The target is to bring those surfaces into
alignment while retaining MROWW's plot repository and existing safeguards.

This is the presentation reference and comparison against MROWW before the parity
changes. The implementation now follows the agreed styling and interactions;
[Plot Git](PLOT_GIT.md) describes the current commands. Backend adaptations support
working diffs, status counts, clipboard history, block history and safe restore.

## Agreed scope

- Use `/git` instead of `/rv`, including help, errors, completion and click payloads.
- Rename `switch` to `checkout`.
- Omit `init`, `use`, `list` and `tp`. One repository covers the whole current plot.
- Keep MROWW's hash lengths: eight-character display IDs and full 64-character IDs
  where the existing backend uses them. Do not adopt the plugin's 12-character IDs.
- Use `master` as the default branch.
- Keep `/git rebase` and its current working-build copy behavior.
- Keep MROWW's Git prefix and paw/snoot wording, including `:3`. Match visual
  structure, colors and interactions without requiring verbatim plugin prose.
- Keep MROWW's backend where possible. Small changes may support HID/behavior parity.
- Do not reproduce the plugin's shared diff bug. Comparisons, highlights and cleanup
  must belong to the requesting player.

The reference [RedstoneVC repository](https://github.com/Brzechuu/RedstoneVC) is
cloned at `scraps/RedstoneVC`, revision
`ad473302ab8ff43858a8d51f0702e804a11a0f13`. The MROWW comparison uses revision
`5c15013215793a222639ebd4d3c0514835ae41a7`. All retained command spellings below
are normalized to `/git`; the reference currently exposes them under `/rv`.

## Presentation conventions

| Surface | RedstoneVC reference | MROWW presentation target |
| --- | --- | --- |
| Successful mutation | Green chat. | Green result with existing Git/paw wording. |
| Help and neutral feedback | Gray chat. | Gray help, empty results and no-op feedback. |
| Usage and failure | Red chat. | Red feedback; keep the current Git prefix and helpful paw/snoot wording. |
| Change summary | Gray label, green `+N`, red `-N`, yellow `~N`. | Same component structure and colors. |
| History row | Yellow ID, colored branch decorations, white message. | Same structure with MROWW's eight-character displayed ID. |
| Inspection headings | Aqua. | Aqua coordinates and history headings. |
| Inspection old/new states | Red minus/pale-red state; green plus/pale-green state. | Same colors and ordering. |
| Command transport | Chat commands and server completion. | Existing MROWW command transport with matching suggestions. |
| GUI | No inventory menu, action bar, boss bar or sidebar. | No new GUI needed; keep existing MROWW features and style related output consistently. |

Most current MROWW Git text replies use the shared yellow system-message helper,
including successes, help and status. Changing the prose alone cannot match the
reference colors; those replies need the appropriate color or a structured chat
component. Existing red errors already use the correct broad color category.

Sources: [reference commands](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/RedstoneVC.kt),
[MROWW message templates](../crates/core/src/messages.rs),
[MROWW chat delivery](../crates/core/src/player.rs).

## Command summary

| MROWW command | Reference presentation and interaction | MROWW before parity changes |
| --- | --- | --- |
| `/git` | Separate gray usage lines, no click actions. | Yellow descriptive help with additional features. |
| `/git commit <message>` | Green ID/branch/message confirmation; gray unchanged feedback. | Yellow result with a pawprint line; unchanged feedback is a red Git error. |
| `/git log [n]` | Styled rows; author/date/ID hover; click copies ID; `n` counts entries. | Aqua rows with date/author inline; click runs show; the number selects a page. |
| `/git status` | Gray branch/HEAD heading and colored `+N -N ~N` totals against the live build. | Build/execution booleans and storage usage. |
| `/git diff [ref] [ref]` | Zero/one/two-ref modes, colored totals, immediate glow. | Two-ref mode only; structured links and explicit Show glow button. |
| `/git inspect` | Crosshair block history, aqua headings, white target marker, colored old/new revisions. | Missing. Sword/diff inspection reports a comparison, not block history. |
| `/git restore <ref>` | Green restore confirmation; replaces working blocks while keeping branch and HEAD. | Missing. Checkout detaches on commit refs; rebase accepts only named branches. |
| `/git branch [name]` | Gray heading; green current branch, white others, yellow arrows/IDs; green creation result. | Yellow rows without heading and a different row layout. |
| `/git checkout <branch>` | Reference switch UI: green success, gray already-current feedback, red dirty-work rejection. | Automatic dirty-work recovery, paused simulation and plot-wide announcement. |
| `/git rebase <branch>` | No plugin counterpart; retained MROWW command. | Keep its existing semantics and apply the shared chat styling. |

### Help

The reference prints usage lines in this order: `init`, `list`, `use`, `tp`,
`commit`, `log`, `status`, `diff`, `restore`, `inspect`, `branch`, `switch`.
Each line is gray, without a tooltip or click event. With no arguments, help does
not require an active repository. Unknown commands with an active repository
produce a red error followed by the gray usage list.

After applying the agreed exclusions, the reference portion becomes:

```text
/git commit <message>
/git log [n]
/git status
/git diff [ref] [ref]
/git restore <ref>
/git inspect
/git branch [name]
/git checkout <branch>
/git rebase <branch>
```

Keep MROWW's `/git help` and `/help git` entry points and its other supported Git
commands. List those commands in the same gray style. Remove all instructions to
select a WorldEdit region, initialize a project, choose a repository or teleport
to a named repository. A prefix or paw-themed heading may stay.

### Commit

The player types a message; the reference joins the remaining arguments into one
message. A successful commit prints one green line with ID, branch and message.
If nothing changed, it gives gray neutral feedback. Missing/blank messages get
red usage feedback. Committing advances the active branch; there is no staging UI.

MROWW should keep its eight-character visible ID and current snapshot behavior.
Present its confirmation in green, retaining paw wording where desired; treat
an unchanged commit as gray neutral feedback. Its validation and saved data can
stay as they are. The agreed default branch is `master`, matching the plugin.

Example layout, with each color applied to the complete line:

```text
[green] Git: Committed a12b34cd on master: Working adder :3
[gray]  Git: No new pawprints; nothing to commit.
[red]   Git: Usage: /git commit <message>
```

### Log

The reference walks current HEAD ancestry newest first. It defaults to ten
entries. Its optional integer means an entry count, clamped to at least one;
invalid integers fall back to ten. This differs from MROWW's numeric page argument.

A row has a yellow ID and punctuation, aqua `HEAD`, green branch names and a white
message. All branch names pointing to a commit appear in alphabetical order;
current HEAD is decorated as `HEAD -> branch`. Author and date are in a hover
card, rather than in the visible row. The date format is `yyyy-MM-dd HH:mm` in
the JVM's default timezone. Clicking anywhere on the row copies the commit ID.
An empty history produces gray feedback.

Target row structure with MROWW IDs:

```text
[yellow] a12b34cd ([aqua] HEAD[yellow] -> [green] master[yellow])  [white] Working adder
```

Use MROWW's full commit ID as the clipboard payload, while displaying eight
characters. A hover card can show author, date and that full ID. Match the timezone
used by the plugin deployment when comparing dates; the current MROWW UTC date
is a visible difference. Keep `/git show` available explicitly, but the reference
row click is a clipboard action, not a show-command action.

For behavior compatibility, resolve the numeric argument as a count. MROWW's
pagination and `--all` are existing extensions; they may remain through explicit
options or additional controls, but the same bare integer cannot mean both count
and page. This is a small command/query adaptation required by the reference flow.

### Status

The reference prints a gray `On <branch> @ <id>` heading, then a gray summary
label with green `+N`, red `-N` and yellow `~N`. Counts compare the saved HEAD with
the live working build; zeros are still shown. It does not show markers. Before
the first commit it gives a gray branch/no-commits message.

MROWW should retain its branch and eight-character ID, but report numeric totals
in the reference component layout. Existing execution-change and storage/quota
information can remain on separate secondary lines, with existing wording. If
needed, computing working-change counts is a narrow backend addition; storage,
permissions and commit behavior need not change.

### Diff

| Input | Reference behavior |
| --- | --- |
| `/git diff` | Compare HEAD with the live working build. |
| `/git diff <ref>` | Compare that saved reference with the live working build. |
| `/git diff <from> <to>` | Compare the first saved reference with the second. |

An unchanged comparison gives gray feedback and no glow. A changed comparison
immediately shows green/red/yellow markers and the colored `+N -N ~N` summary.
It never floods chat with block rows. The reference renders at most 3,000 markers;
when capped, it adds a gray notice, but the counts still cover every change.
Reference lookup failures give red feedback.

MROWW currently requires two saved refs and then a Show glow click. To match the
reference flow, add the missing working-build modes and enable the requesting
player's glow automatically for nonempty results. Reuse its existing comparison
and marker facilities. Keep full IDs internally and MROWW reference validation;
there is no reason to copy the plugin's ambiguous-prefix lookup.

MROWW's show/hide helpers and source/destination links can remain as secondary
controls. Use a gray summary label and individually colored counts instead of
coloring the whole summary one color. Keep execution-change information separate.
An empty diff should use neutral feedback without inviting a meaningless Show
click. Marker caps and proximity selection remain an existing visible difference;
do not raise whole-plot rendering limits blindly to reproduce 3,000 entities.

The shared-state bug is excluded explicitly; see **Diff ownership** below.

### Inspect

The reference uses a typed command while the player looks at a real non-air block
within ten blocks. No held item or prepared diff is required. An out-of-range or
invalid target produces red feedback. It briefly surrounds the target with a
white glowing glass display for 60 server ticks, nominally three seconds.

Its chat result is:

1. Aqua world-coordinate heading.
2. Gray current block state, with `minecraft:` removed but state properties kept.
3. Aqua history heading with the number of recorded revisions, or an aqua
   no-recorded-changes message.
4. For each revision, newest first: yellow ID/date/author, red minus with pale-red
   old state, green plus with pale-green new state.

The pale colors are `#FF9C9C` for old states and `#9CFF9C` for new states. IDs use
MROWW's eight-character display width. The reference scans up to 200 ancestor
commits, compares each block against its parent and omits the root commit's
initial placement; it does not include commit messages in the revision rows.

MROWW's existing sword interaction and `/git diff inspect` show From/To states
and saved data for a prepared comparison. They should remain useful extensions,
but neither is a substitute for the crosshair block-history UI. Implementing that
UI needs a limited historical-block lookup against the existing repository.
The white target display should also be owned by the requesting player.

### Restore

The reference restores a chosen snapshot into the live working region, including
air, while leaving both branch and HEAD unchanged. It gives a green confirmation
containing the ID and a branch-unchanged note. Missing/unknown refs produce red
feedback. This allows the player to revisit saved contents as working changes
and then commit them on the current branch.

MROWW has no direct equivalent. Commit checkout enters detached HEAD. Rebase
keeps the current branch/tip but accepts only named source/current branches.
A `/git restore <ref>` UI needs a narrow adaptation of existing restoration
facilities to preserve HEAD for arbitrary accepted refs. Keep the current safety
and recovery behavior; do not copy the reference's unprotected overwrite of dirty
work. Show a green result with the eight-character ID, existing paw wording and
any recovery/paused-simulation notice that actually applies.

### Branch

Listing branches gives a gray heading followed by alphabetically sorted rows:
a green star and green name for the current branch, white names otherwise,
and yellow arrows/IDs. Rows have no click or hover actions.

```text
[gray]  Branches:
[green] * master[yellow] -> a12b34cd
[green]   [white] experiment[yellow] -> e56f78ab
```

Creating a branch prints a green name/ID confirmation and does not switch to it.
Invalid names, duplicates and missing commits are red errors. The reference's
empty listing has only its heading; MROWW's helpful empty-state hint can stay in
gray with its paw wording.

Match this component layout with MROWW's ID width. Keep MROWW's optional source
ref, stricter name validation and branch limits. Its existing checkout hint may
remain, spelled `/git checkout`.

### Checkout

This is the renamed reference `switch` command. It switches to a named branch
and updates the live build to that branch's saved state. Its success is green;
already-current feedback is gray; missing/unknown branches are red. The plugin
rejects dirty-work switching with a red commit-or-restore hint.

Keep MROWW's automatic dirty-work recovery, commit checkout/detached HEAD support,
paused simulation and other restoration safeguards. Those notices must describe
MROWW's real behavior rather than copying a false dirty-work rejection. Present
success in green and already-current feedback in gray, with the current prefix
and paw wording. Keep any plot-wide notification, but give the initiating player
a clearly styled result as well.

### Rebase

Keep `/git rebase <branch>`. It copies the source branch's saved plot into the
current working build, leaves both branch tips unchanged and creates no commit.
The player edits if needed, then commits. MROWW preserves unfinished work before
replacement and leaves simulation paused.

There is no reference plugin UI for rebase. Fit it into the same visual language:
green success, red usage/failure, gray secondary instructions. Retain the current
source/current branch names, commit-next instruction, recovery details and paw
wording. Do not reinterpret this command as ordinary Git history rewriting.

## Omitted reference commands

These still explain the plugin's original presentation, but must not appear in
MROWW Git help, completion or hints.

| Command | Reference UI and action |
| --- | --- |
| `init <name>` | Green name/world/dimensions confirmation after saving the WorldEdit selection as the repository region. Red usage, duplicate-name or missing-selection errors. |
| `use <name>` | Green repository/world confirmation; selects the player's named repository without teleporting or modifying blocks. |
| `list` | Gray heading; active name/star green, inactive names white, world aqua, coordinates yellow, dimensions gray. Hover gives a green teleport hint; click runs `tp`, not `use`. Empty list is gray. |
| `tp <name>` | Teleports above the named region's center and confirms in green. Does not select the repository. Missing/unknown names and unavailable world are red errors. |

## World markers and HID

| Property | Reference | MROWW before parity changes | Presentation target |
| --- | --- | --- | --- |
| Material | Lime/red/yellow stained glass; inspect uses white. | Lime/red/yellow stained glass. | Match materials and add private white inspect highlight. |
| Shape | Scale 1.01, centered offset -0.005. | Same scale and offset. | Already aligned. |
| Glow colors | Green `#39FF14`, red `#FF2D2D`, yellow `#FFE23D`, inspect white. | `#55FF55`, `#FF5555`, `#FFFF55`. | Use the reference RGB values. |
| Lighting | Block/sky brightness 15/15. | No explicit brightness override. | Match full brightness. |
| Diff activation | Immediate for nonempty diff. | Explicit Show glow. | Automatic activation; retain show/hide as helpers. |
| Diff lifetime | 900 server ticks, about 45 seconds at 20 TPS. | Default five minutes of wall-clock time. | Match the nominal 45-second visible duration; keep cleanup independent of paused plot simulation. |
| Inspect lifetime | 60 server ticks, about three seconds. | No white history-inspect marker. | About three seconds. |
| Marker set | First 3,000 changes. | Nearest 128 by default within 64 blocks and loaded view; batched updates. | Retain safe whole-plot bounds and disclose visible subset with neutral feedback. |
| Ownership | Shared command fields and world entities. | Viewer-specific sessions and packets. | Keep viewer-specific ownership; the shared plugin behavior is a bug. |
| Typed inspection | Real block under crosshair, ten-block range, no tool. | Missing historical inspection. | Add this flow through `/git inspect`. |
| Sword inspection | No plugin sword handler. | Right-click a diff marker using any sword, either hand, including removed markers. | Retain as an extension; do not substitute it for block history. |

The plugin has no custom mouse bindings. Its two special chat actions are
repository teleport rows, which are omitted, and clipboard history rows, which
are retained. The WorldEdit wand is only for `init`, so MROWW Git needs no wand
selection UI. No new inventory menu or tool item is needed.

Sources: [reference highlights](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/Highlights.kt),
[MROWW marker renderer](../crates/core/src/plot/git/visuals.rs),
[MROWW sword and session handling](../crates/core/src/plot/git/mod.rs),
[MROWW interaction handlers](../crates/core/src/plot/packet_handlers.rs).

## Diff ownership

The reported bug is visible in the reference code: `RvCommand` owns one
`diffHighlights` list for all players, and every player's command calls
`clearDiffHighlights()` before dispatch. Thus player B's `status`, `log`, failed
command or new diff removes player A's markers. A subsequent diff replaces the
single shared list. The real Bukkit display entities also have normal world
visibility rather than viewer-specific ownership. The separate inspection
highlight field is shared too.

Do not reproduce this. Keep MROWW's sessions keyed by player UUID and send marker
packets only to their owner. A player's diff, hide, timeout, plot exit, restore
cleanup or permission loss must never replace another player's comparison or
arbitrarily remove another player's markers. A plot restore legitimately
invalidates comparisons for everyone because the shared build changed; that is
different from one player issuing an unrelated command.

If matching the reference's next-command cleanup, apply it only to the initiating
player's overlay. Keep helper interactions such as show/hide, comparison inspect
and sword inspection usable without clearing their own prerequisite comparison.
Scope any new white inspect marker and asynchronous inspection reply to the
requesting player as well. Retain permission checks on all viewer interactions.

## Completion

The reference filters suggestions case-insensitively and by argument position.
After omitting repository management and normalizing names, the retained behavior
is:

| Position | Suggestions |
| --- | --- |
| First argument | Retained Git subcommands, including rebase and supported MROWW extensions. |
| `checkout` target | Existing branch names; keep commit refs if exposing MROWW detached checkout. |
| First or second `diff` ref | `HEAD`, branch names and recent commit IDs. |
| `restore` ref | `HEAD`, branch names and recent commit IDs. |
| `rebase` source | Existing branch names, consistent with current MROWW behavior. |
| Commit message, new branch name, log count, inspect | No unrelated branch suggestions. |

Reference commit suggestions include the newest fifty IDs in current ancestry.
Use MROWW IDs; suggestion filtering should match the accepted spelling/case rules.
Current MROWW completion generally combines `HEAD` and cached branches across
argument positions and does not suggest commit IDs. It can suggest branch names
where a subcommand or message belongs. Tighten these positions rather than adding
a new completion framework. Keep permission filtering.

## Existing MROWW extensions

Keep rebase explicitly. Existing `help`, `show`, `search`, `log --all`, recoveries,
recovery, diff helpers, sword inspection, storage information and sidebar are not
reasons to redesign the backend. Apply the common presentation style to their
chat output and preserve their functional safeguards.

The sidebar has no reference counterpart. Keep its existing branch/detached,
loading, restoring and recovery information; paw wording is allowed. Worker
progress can remain but should use neutral styling rather than looking like the
final success. Recovery notices should be clear secondary output. Stronger
validation and permission errors should stay accurate rather than copying the
reference's backend weaknesses.

## Acceptance checks

| Check | Expected result |
| --- | --- |
| Help and completion | `/git` and `checkout` everywhere; no init/use/list/tp; rebase present. |
| Success, no-op, failure | Green, gray and red respectively; existing prefix/paw wording remains allowed. |
| IDs | Eight-character display; clipboard copies the full MROWW commit ID. |
| Log row | Yellow ID, aqua HEAD, green branches, white message; hover metadata; clipboard click. |
| Status and diff | Colored `+N -N ~N`, correct comparison direction, no block-list chat flood. |
| Working diffs | Zero/one/two refs support the reference user flow. |
| Markers | Matching glass geometry, RGB and full brightness; automatic diff glow and timed cleanup. |
| Inspect | Ten-block crosshair flow, private white marker, aqua headings and colored historical states. |
| Restore | Working contents change while branch and HEAD stay unchanged; existing recovery remains intact. |
| Rebase | Existing working-copy behavior remains, with green success and clear secondary instructions. |
| Player A diff, player B status/log/invalid command | A's markers and comparison remain intact. |
| Player A and B create different diffs | Each sees their own markers and summary; neither steals the other's session. |
| Player B hides/expires/leaves plot | Only B's private comparison is cleaned up. |
| Display limits | Counts remain exact even when only nearby/capped markers are visible. |

These are source-backed presentation targets, not completed client tests. The
plugin targets Paper 26.2 while MROWW's metadata targets its bundled 1.21.5
protocol, so final appearance needs a client check on the actual deployments.

## Source map

| Source | What it establishes |
| --- | --- |
| [RedstoneVC.kt](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/RedstoneVC.kt) | All command output, click/hover events, completion, crosshair inspect and shared diff bug. |
| [Highlights.kt](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/Highlights.kt) | Glass materials, RGB, shape, brightness, display visibility and timers. |
| [Storage.kt](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/Storage.kt) | Reference ID width, branch defaults and ancestry traversal. |
| [WorldEditBridge.kt](https://github.com/Brzechuu/RedstoneVC/blob/ad473302ab8ff43858a8d51f0702e804a11a0f13/src/main/kotlin/pl/redstonefun/redstonevc/WorldEditBridge.kt) | Region capture and restoration behavior. |
| [Git module](../crates/core/src/plot/git/mod.rs) | MROWW command dispatch, permissions, per-player sessions, completion and sword interaction. |
| [Repository](../crates/core/src/plot/git/repository.rs) | MROWW history, status, branch, checkout, restore helpers, recoveries and ID use. |
| [Diff](../crates/core/src/plot/git/diff.rs) | MROWW comparison summary, counts and From/To inspection. |
| [Visuals](../crates/core/src/plot/git/visuals.rs) | MROWW marker packets, metadata, bounds and updates. |
| [Messages](../crates/core/src/messages.rs) | Current MROWW Git wording and templates. |
| [Plot Git](PLOT_GIT.md) | Existing MROWW user behavior and safeguards. |
