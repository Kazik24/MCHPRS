# LuckPerms integration

MCHPRS reads an existing SQL-backed LuckPerms database. PostgreSQL, MySQL and
MariaDB are supported. It issues SELECT statements in read-only transactions;
it does not create/migrate tables, update users, or change permission nodes.
Use the existing LuckPerms plugin to manage permissions. Players must reconnect
to refresh permission changes.

## PostgreSQL configuration

Append this table to the runtime `Config.toml`, replacing the connection details:

```toml
[luckperms]
storage = "postgres"
host = "redstonefun-postgres"
port = 5432
db_name = "rf"
username = "mchprs_luckperms_ro"
password = "REPLACE_WITH_PRIVATE_READ_ONLY_PASSWORD"
table_prefix = "luckperms_"
server_context = "global"
world_context = "redstoneplots"
plotsquared_compat = true
```

Protect the config file and backups containing credentials. Never commit the
actual password. PostgreSQL connections use SCRAM/password authentication without
TLS, so use a private Docker network, SSH tunnel, or equivalent trusted transport.
The production Compose file joins the existing external `redstonefun-network`;
PostgreSQL does not need an additional published port.

The reader needs CONNECT to the database, USAGE on the schema, and SELECT on
`luckperms_user_permissions` and `luckperms_group_permissions`. SELECT on
`luckperms_players` is also used by the optional integration test. Do not grant
INSERT, UPDATE, DELETE, TRUNCATE, CREATE, role membership, or administrative rights.
Set `default_transaction_read_only=on` for this login as additional protection.
The server validates the connection and tables before accepting players. A
permission lookup failure during login produces an empty permission cache, rather
than granting the permissive behavior used when LuckPerms is disabled.

## Account identity

LuckPerms keys users by UUID. A database populated by an online-mode Minecraft
server contains authenticated account UUIDs. MCHPRS's default offline login uses
different UUIDs, so it cannot find those users' existing ranks. Merely looking up
an account UUID from an unverified username would allow rank impersonation.

MCHPRS currently accepts account UUIDs through BungeeCord forwarding. If using
`bungeecord = true`, only a trusted authenticating proxy may reach the backend;
the public must not have direct access to its port. The forwarding format is not
authenticated by MCHPRS itself. No UUID conversion or username-based permission
mapping should be applied to the shared database.

## Supported permission data

- Direct user permissions and recursive group inheritance, with cycle protection.
- Implicit `default` group for users without an applicable group membership.
- Boolean PostgreSQL values and numeric MySQL/MariaDB values.
- Exact permission names and dot-separated wildcard prefixes.
- Explicit denials, deterministic precedence, and group weights.
- `global` and configured server/world scopes, plus matching `server` and `world`
  JSON contexts. Unsupported dynamic contexts are excluded.
- Expired nodes are excluded. Group membership changes require reconnecting.
- Prefixes, suffixes and weights are metadata, not permission grants.

This is a basic reader, not the complete LuckPerms engine. Direct user nodes take
precedence over inherited nodes. Within that class, scoped and more specific
nodes precede broader nodes, then nearer groups and their weights resolve ties.
An otherwise equal denial wins. Existing administrator wildcard grants still
apply; an ordinary permission pack never implies administrative rights.

## PlotSquared compatibility

Set `plotsquared_compat = true` to interpret the basic permission pack found in
RedstoneFun's PlotSquared 7.3.11 `plugin.yml`. This is local to MCHPRS and does not
expand or rewrite the shared LuckPerms rows.

| MCHPRS permission | Existing PlotSquared/WorldEdit permission |
| --- | --- |
| `plots.info`, `plots.claim`, `plots.auto`, `plots.visit`, `plots.middle` | Child of `plots.permpack.basic` |
| `plots.lock` | `plots.flag`, also provided by the basic pack |
| `plots.select` | `worldedit.selection.pos` |
| `commands.rhistory`, `commands.rback` | `plots.set`, also provided by the basic pack |

An explicit MCHPRS permission grant or denial takes precedence over compatibility
fallbacks. Plot ownership checks still apply to edits and rewind operations.
The basic pack does not grant WorldEdit, other-plot access, history administrator
permissions, `/stop`, or `/whitelist`. Those last two commands require
`minecraft.command.stop` and `minecraft.command.whitelist` respectively.

## Integrity checks and tests

Before connecting to an existing database, create and validate a consistent
`pg_dump --format=custom` backup, back up the runtime configuration and deployment
image reference, and record ordered row hashes/counts for every LuckPerms table.
After creating the read-only login and after testing/deployment, compare the hashes
and verify that the login has no write or schema-creation privileges.

Run permission unit tests with:

```text
cargo test -p mchprs_core --lib permissions::tests --locked
```

For the optional RedstoneFun read-only integration test, set
`MCHPRS_LUCKPERMS_TEST_CONFIG` to a private TOML file containing the above table,
then run:

```text
cargo test -p mchprs_core --lib permissions::tests::live_postgres_read_only_permissions --locked -- --ignored --nocapture
```

This test checks read-only access and the existing default, builder and staff
rank results without issuing data mutations. Only use it with the matching
RedstoneFun rank layout; it is not a schema-independent migration utility.

The protocol smoke runner uses a separate temporary world on localhost:25588.
Set `MCHPRS_LUCKPERMS_TEST_ACCOUNTS` to a private JSON file mapping `default`,
`builder`, and `admin` to existing UUIDs, then run
`python tools/run_luckperms_smoke.py` (`py` on Windows). It simulates trusted proxy
forwarding only within that isolated server, and verifies default plot/rewind
access, scoped builder WorldEdit, administrator commands, and denied escalation.
It does not modify LuckPerms data. Plot ownership is refreshed after claims and
on entry so ordinary owners can use these permissions immediately.

## RedstoneFun setup record: 2026-10-04

The existing backend is PostgreSQL 16.1, database `rf`, schema `public`, prefix
`luckperms_`. A new `mchprs_luckperms_ro` login has SELECT on the two permission
tables and the players table only. Authentication, read-only transactions, no
table-write privileges and no schema/role administration were verified.

A validated full custom-format dump, original MCHPRS config, deployment image
reference, integrity snapshots and private staged configuration are stored on
the RedstoneFun host in:

```text
/home/lord225/mchprs-backups/luckperms-20261004T184711Z/
```

The backup `rf.dump` is 34,555 bytes; SHA-256:
`379790ff57f51f255550d543b287feec1e900eca2e26399ec597b92798148110`.
Ordered row hashes/counts were identical before setup and after reading all 252
accounts. The six tables contain 252 actions, 43 user nodes, 94 group nodes, 252
players, seven groups and zero tracks. No existing permission data or table
definitions were changed.

Validation passed: 161 workspace unit tests, the live 252-account read-only test,
and protocol permission checks against both the local build and the production
Docker image. The isolated production-image server stopped cleanly and its test
container was removed. The staged image is
`sha256:f78a33775f35f3a125017340a371084a601112eee2bfcda0fe61334e98418501`.

**Activation is pending the authentication choice.** The live MCHPRS config and
running container remain unchanged. None of the 252 stored account UUIDs matches
its corresponding offline UUID. Enabling the reader without verified identity
would therefore apply default permissions instead of existing player ranks.
The private `luckperms.toml` is staged beside the backup, and the production image
has been built. Only activate after choosing an authenticated login route; never
solve this by copying user/group rows or trusting a submitted account name.
