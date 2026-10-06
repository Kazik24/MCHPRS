# Velocity / MROWW chat

Public chat uses the standalone Velocity service and MROWW's native Rust adapter
(`crates/core/src/proxy_chat.rs`). The Java plugin lives in
`plugins/network-chat/velocity`; its protocol class is included in the same JAR.
There is no Paper bridge or separate common module.

Velocity handles formatting, delivery, cooldowns, LuckPerms permission checks,
and network join/quit announcements. Commands and command output stay local.

## Build and install

Java 21 and Maven are required:

```sh
mvn -f plugins/network-chat/pom.xml package
```

Install `plugins/network-chat/velocity/target/redstonefun-chat-velocity.jar` and
LuckPerms into the standalone Velocity service's plugin directory, then restart
Velocity. This plugin requires LuckPerms. Configure LuckPerms to use your existing
permissions database and keep database credentials outside the repository.

## Backend deployment

```sh
docker --context prod compose -p mchprs -f docker-compose-prod.yml build mroww
docker --context prod compose -p mchprs -f docker-compose-prod.yml up -d mroww
```

The service, container, and image are named `mroww`, `mroww`, and `mroww:latest`.
Host port `25578` maps to container port `25565`. Velocity runs separately.

MROWW's top-level configuration:

```toml
proxy_chat = true

[velocity]
secret_file = "/run/velocity/forwarding.secret"
```

Compose mounts `/home/minecraft-dev/velocity/data/forwarding.secret` read-only
at that container path. The host file must already exist. Authenticated modern
forwarding is required. No local-chat fallback occurs when the handshake fails.

Register the backend as `mroww` in Velocity. A proxy container must use the Docker
host's reachable address with port `25578`, rather than its own `127.0.0.1`.
A proxy on `redstonefun-network` can instead use `mroww:25565`.

## Formatting and permissions

Velocity creates `plugins/redstonefun-chat/chat.properties` on first startup.
The default is `backends=mroww` with a 500 ms cooldown. Edit this UTF-8 file and
restart Velocity to change backend names, rank prefixes, or the separator.

Format: `[Rank] Nick » message`. Rank priority is admin, moderator, engineer,
expert, advanced, builder, default. LuckPerms determines inherited membership;
message text is literal gray and cannot inject colors or formatting.

Proxy permissions (global or `server=proxy` context):

- `network.chat.send`: explicit false denies public chat; undefined permits it.
- `network.chat.receive`: explicit false hides chat and join/quit announcements.
- `network.chat.cooldown.bypass`: effective true bypasses the proxy cooldown.

MROWW's `mchprs.access.chat` and backend anti-flood limits still apply. Gameplay
and command permissions retain their existing MCHPRS context.

## Protocol

The plugin channel is `redstonefun:chat`. All integers are big-endian:

- HELLO / READY: version `1`, operation `0` / `1`, 16-byte session token.
- CHAT: version `1`, operation `2`, token, positive 64-bit sequence number,
  unsigned 16-bit UTF-8 length, plain text.

Text is limited to 256 UTF-16 units / 1024 UTF-8 bytes. Invalid UTF-8, control
characters, section signs, malformed frames, stale sessions, replayed sequences,
and client-originated bridge messages are rejected. Identity comes from the
connection. Handshakes retry up to ten times. Messages use system chat.

Shared tab lists, private messages, Discord integration, and cross-server
reportable signed player chat are outside this plugin's scope.

To return to backend-local public chat, set `proxy_chat=false` and restart MROWW.
