# Authenticated MCHPRS behind Velocity

Velocity is the public entry point on port 25562. MCHPRS has no published port;
it accepts gameplay connections only after verifying Velocity's modern
forwarding HMAC. The proxy must have `online-mode = true` and
`player-info-forwarding-mode = "modern"`. Keep Paper RedstoneFun on its existing
port and configuration.

Add the following to the private runtime MCHPRS config, alongside its existing
LuckPerms configuration:

```toml
bungeecord = false

[velocity]
secret_file = "/run/velocity/forwarding.secret"
```

The secret file must match Velocity's `forwarding.secret` and contain at least
32 bytes. The standard proxy container generates it on first startup and keeps
it under `/srv/mchprs/velocity/`. Keep it outside Git and Docker images. An
unreadable or short secret prevents MCHPRS from starting. Legacy BungeeCord and
modern Velocity forwarding cannot both be enabled. If Velocity is enabled but
LuckPerms is missing, permission checks deny access instead of granting every
command.

MCHPRS requests forwarding version 1 and verifies the SHA-256 HMAC before
reading the forwarded IP, account UUID, username and profile properties.
Missing forwarding, incorrect signatures, mismatched usernames, malformed
profiles and unsupported versions are rejected. Signed `textures` properties
are preserved in login and player-list packets, including broadcasts to players
already on other plots. This supports account skins and capes; it does not add
player chat signature enforcement.

## Deployment

From the repository, deploy directly to the existing `prod` Docker context:

```sh
docker --context prod compose -p mchprs -f docker-compose-prod.yml up -d --build
```

This is the complete update command. Docker transfers build inputs, builds both
images on the remote host, and starts the services. Python and copied deployment
scripts are not required. The `prod` context points to `ssh://lord225@37.27.71.203`.

`docker-compose-prod.yml` publishes only Velocity on 25562. Its dedicated
backend network is internal. MCHPRS also joins the existing database network
for the read-only LuckPerms reader. Velocity has a separate network for internet
access to Minecraft authentication services.

Private runtime files on `urmom` live under
`/srv/mchprs/`. `docker/Dockerfile-velocity` pins the proxy base
image digest and official JAR checksum. `docker/velocity.toml` controls the public
listing, online authentication and forwarding; the normal image initializer
synchronizes it on startup. MCHPRS waits for the proxy's health check, ensuring
the forwarding secret exists before the backend starts. Runtime MCHPRS config is
mounted as a directory so atomic configuration writes continue working.

The existing private backend configuration, including read-only LuckPerms
credentials, stays in `/srv/mchprs/backend-config/Config.toml`. It is excluded
from image builds. `docker/Config.toml` is the tracked public image default, so
building does not depend on an ignored local `Config.toml`.

The public listing is dark red and bold `RedstoneFUN.pl`, gray `1.21.5`, and red
`Szybki Serwer Redstone!` on the second line. It advertises 100 players.

Player files and plot ownership are keyed by UUID. Existing offline player data
remains preserved when authenticated users join with their account UUIDs.
Linking old ownership or inventories requires verified account ownership; do not
automatically convert existing user records from usernames.

## Checks

```text
cargo test -p mchprs_core -p mchprs_network --lib --locked
```

Before deployment, run protocol checks against a temporary world with a separate
test forwarding secret. Verify successful signed forwarding, skin properties,
missing/forged forwarding rejection, and ordinary-user denial of administration.
Exercise a genuine Velocity instance without online authentication only in an
isolated staging network, then discard that configuration. Production must
require account authentication. Verify the public proxy rejects unauthenticated
clients and that the backend has no public port mapping.

## Deployment record: 2026-10-05

The documented Compose command deployed both services successfully on `urmom`.
Running it again reused both containers with cached builds. A clean temporary
proxy container generated its own secret and became healthy without a backend;
the temporary container and volume were then removed. No Python deployment
helpers remain in the repository or `/srv/mchprs/deployment/`.

The deployed backend image is
`sha256:19a8be8ffb52c2b166231adb10d0a4efb9f6a0e2a7687c442f95f837bc8a836e`;
the proxy image is
`sha256:9054d6d1c0acf3001580cb65abf00b88d32ef41520e6465f3d8b78213bfc2efa`.
A restart-only Compose configuration is stored at
`/srv/mchprs/deployment/docker-compose.yml`. Source updates use the repository
command above, which also builds the images.

The stopped-backend data backup is
`/srv/mchprs/deploy-backups/velocity-20261005T122455Z/data.tar.gz`, with its checksum
and original container/configuration metadata alongside it. Keep authenticated
forwarding enabled when restoring world data.

Validation passed: 172 core and 20 network unit tests; isolated protocol checks
for signed forwarding, skin properties, spoofing rejection and rank enforcement;
and the public listing/account-authentication check after the final Compose
deployment. Paper's original process and three configuration hashes were
unchanged. No successful login using an official account or visual in-game skin
check was performed during that initial deployment; the skin properties were
checked at the packet level.

The subsequent rank deployment uses dedicated `mchprs.*` LuckPerms nodes and
the RedstoneFun join/chat format. See [MCHPRS_PERMISSIONS.md](MCHPRS_PERMISSIONS.md)
for the current policy and permission catalog. A live authenticated Lord225
session was observed using plot commands after that deployment. Visual skin and
chat formatting still require in-game confirmation.
