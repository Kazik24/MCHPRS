# Plot Git

Plot Git saves the entire current plot: block states, supported block entities
(including inventories, signs and command blocks), scheduled ticks and piston
execution state. Commits survive server restarts. It is a native snapshot
repository; no Git executable is required.

## Start an experiment

```text
/git commit Working adder
/git branch experiment
/git checkout experiment
# Edit the build.
/git commit Smaller carry circuit
/git diff main experiment
/git checkout main
```

The first commit creates `main`. Creating a branch does not switch branches.
The active branch and working build are shared by everyone on the plot.
To revisit a commit directly, use `/git checkout a12b34cd`. This enters detached
HEAD and leaves every branch tip unchanged. Commits made there advance detached
HEAD; `/git log --all` retains them. Name that history with `/git branch revisit`,
then `/git checkout revisit` to continue on the named branch. Detached HEAD also
survives restarts, interrupted checkout recovery, and automatic work recovery.

To copy another branch's saved plot into your current branch, use:

```text
/git checkout main
/git rebase revisit
# Edit the copied build if needed.
/git commit Bring revisit into main
```

Rebase replaces the whole working plot with the source branch's tip, including
saved block data and simulation state. It leaves you on the current branch,
moves neither branch tip, and creates no commit. Your next ordinary commit saves
the result on the current branch, with its previous tip as parent. Both source
and current branch must be named branches. Unfinished work is saved in a recovery
before replacement. Copying your own branch restores its saved contents too.

Checkout pauses simulation; use `/tps 20` or the existing stepping commands to
resume. Players whose standing body would intersect the restored build are
moved above it. Checkout closes menus and clears tick history and WorldEdit undo.
Rebase uses the same restoration behavior and leaves simulation paused.

## Commands

| Command | Purpose |
| --- | --- |
| `/git`, `/git help`, `/help git` | Usage. |
| `/git status` | Active branch, build/execution changes and storage usage. |
| `/git commit <message>` | Save a complete snapshot and advance the active branch. |
| `/git log [--all] [page]` | Current ancestry, or all commits; ten entries per page. |
| `/git search [--all] [--page n] <text>` | Search commit messages. |
| `/git show <ref>` | Commit ID, author, UTC date, message and parent. |
| `/git branch` | List branches and their tips. |
| `/git branch <name> [ref]` | Create a branch, defaulting to `HEAD`. |
| `/git checkout <branch\|commit>` | Restore a branch tip or commit ID/prefix, preserving unfinished work. |
| `/git rebase <branch>` | Copy that branch's saved plot into the current working plot; edit, then commit. |
| `/git diff <from> <to>` | Prepare a comparison and show its summary. |
| `/git diff show`, `/git diff hide` | Enable/disable the prepared glow overlay. |
| `/git diff inspect <x> <y> <z> [from\|to]` | Show a changed coordinate's states and data immediately; optionally restrict data to one side. |
| `/git recoveries [page]` | List automatically saved unfinished work. |
| `/git recover <id> <new-branch>` | Put a recovery on a new branch and check it out. |

References are branch names, `HEAD`, full commit IDs or unique prefixes of at
least eight characters. Messages are limited to 256 characters, searches to 128,
and branch names to 48 ASCII letters/digits/underscores/hyphens, beginning with
a letter or digit. `HEAD` and names resembling commit IDs are reserved. Each
repository supports up to 128 branches.

## Inspect changes in the world

A comparison shows only added/removed/changed totals and **Show glow / Hide glow**
buttons in chat. Execution changes are reported separately. It never lists all
changed blocks in chat.

Click **Show glow**, then **right-click a marker with any sword in either hand**
to see that position's **From / To** block states and changed saved block data
immediately. No additional detail clicks are needed. Green means added, red
means removed, yellow means changed.
Removed positions can be inspected even when the current world is air there.
The nearest glowing marker along the aim line is selected, including through
obstructions. A successful inspection consumes the interaction and its duplicate
or offhand fallback; a miss preserves normal sword behavior.

Markers are private to the viewer and never become actual world blocks/entities.
They follow the player, showing the nearest changes in loaded chunks. Defaults
are 128 markers within 64 blocks, with 32 changes per update. The comparison
counts remain exact when fewer markers are visible. Comparisons expire after
five minutes and are removed on plot exit, checkout or permission loss. Hide
retains the prepared comparison until expiry so Show can restore it.

## Permissions

In dedicated permission mode, **`mchprs.commands.git`** is the single allow/deny
node for the whole feature, including completion, glow and sword inspection.
Backend command access (`mchprs.access.commands`) is also required.

Ownership or membership is required for reading history. Commit and branch
creation additionally require existing plot edit access. Checkout/recovery/rebase are
restricted to the plot owner with plot edit access. `mchprs.plots.admin.git` explicitly overrides these
plot restrictions, while the Git allow/deny node still applies.

Use the existing LuckPerms installation to grant or deny access, for example:

```text
/lp group expert permission set mchprs.commands.git true server=mchprs
/lp user PlayerName permission set mchprs.commands.git false server=mchprs
```

Missing nodes deny access on LuckPerms deployments; exact denials also override
an inherited admin wildcard. Permission refresh follows the existing cache.
Legacy LuckPerms mode uses `commands.git` and `plots.admin.git`; standalone
servers retain the usual permissive command fallback and plot access checks.

### Rank storage allowances

Disk history is limited per plot using its **owner's** effective rank allowance,
even when a member or administrator operates on the plot. The default allowance
is **100 MiB**; grant **1 GiB** to selected ranks with numeric permission nodes:

```text
/lp group default permission set mchprs.git.storage.100 true server=mchprs
/lp group engineer permission set mchprs.git.storage.1024 true server=mchprs
```

The deployed RedstoneFun policy grants Git to Expert `[E]` and higher. Expert
gets 100 MiB per plot; Engineer `[I]`, Moderator, and Admin get 1 GiB. Lower
ranks have no Git grant. Explicit user overrides still apply.

These are configuration examples; apply them to your chosen groups. Values are
MiB, and the largest effective positive grant wins. Exact denials and expiry
apply. Storage grants do not grant `/git` access, and wildcard grants alone do
not select a numeric allowance. Legacy mode uses `git.storage.<MiB>`.

`git_default_plot_storage_mib` sets the fallback when no numeric grant applies,
including standalone servers and unowned plots. `git_plot_storage_mib` is the
server ceiling for every plot, so a larger rank grant is clamped to that value.
The owner's online permission cache is reused; offline owners are resolved on
a background Git worker. If that lookup fails, the operation stops without
changing the repository. Ownership changes use the new owner's allowance.

`/git status` shows the applicable disk quota. A rank downgrade preserves
existing commits and recoveries; writes requiring additional space are rejected
until the allowance is raised. The global disk limit still applies.

## Storage and recovery

Each plot has `world/plot-git/p<X>,<Z>/repository.sqlite`. Snapshots use LZ4
compression. Decompression validates the declared size and snapshot checksums.
Compressed snapshots,
commits, branches and recovery metadata share SQLite transactions with full
synchronization. Snapshot hashes use canonical states/data rather than palette
layout, inventory order or NBT compound ordering; identical snapshots share an
object. Execution order remains significant. Version and checksum validation
reject corrupt or incompatible snapshots.

Checkout and rebase save unfinished work before replacing the plot. `/git recoveries`
shows the recovery IDs. A durable checkout record reconciles the atomic ordinary
plot save with the active branch on restart. Recoverable failures roll back;
if rollback cannot finish, the plot stays paused and locked until startup recovery.
Back up **both** `world/plots` and `world/plot-git` while the server is stopped.

The owning plot thread captures consistent state at a tick boundary. Redpiler
is reset to export authoritative blocks/ticks. Hashing, compression, repository
access and diff scans run on two background workers with a bounded queue. One
Git operation runs per plot at a time; edits after a commit capture are subsequent
working changes. Checkout temporarily locks world mutations and ordinary saves.

`Config.toml` adds these defaults automatically:

```toml
git_plot_storage_mib = 1024
git_default_plot_storage_mib = 100
git_total_storage_mib = 16384
git_work_memory_mib = 1024
git_snapshot_max_mib = 128
git_marker_limit = 128
git_marker_radius = 64
git_session_seconds = 300
```

Quotas count compressed objects, recoveries and repository metadata; every write
checks database growth even when it reuses a snapshot. Global admission accounts
for repository file sizes and staging headroom. Work memory
uses conservative reservations for captures, decoded comparisons and retained
sessions. The server-wide Git **RAM** workspace is capped at **1 GiB**, including
retained comparisons; `git_work_memory_mib` can lower this cap, but values above
1024 are clamped. This reservation budget is separate from rank disk quotas and
is not a measurement of the server's total RAM usage. Quota rejection preserves existing
history; history is never pruned automatically. Radius is capped at 128 blocks,
marker count at 512, session duration at 10–3600 seconds. Operations whose
conservative memory reservations do not fit are rejected before replacing the
plot or history.

Merges, remotes, branch deletion and partial restoration are
outside this release. Glow metadata is checked against the bundled 1.21.5
protocol; final appearance and client performance need an in-game client check.
