# Shared RedstoneFun / MCHPRS chat

The implementation consists of a Velocity plugin, a Paper bridge plugin, and a
native MCHPRS adapter. Public chat is shared across the two backends. Velocity
owns its formatting, delivery, cooldown, permission checks, and network join/quit
announcements. Backend commands, command output, and gameplay stay local.

The Paper bridge is **inactive by default**. MCHPRS defaults to local chat.
Installing/building these components does not migrate the live Paper server.

## Deployment status (2026-10-05)

- Velocity chat plugin and LuckPerms are deployed; PostgreSQL initialization succeeded.
- Velocity uses IPv4 sockets because the host's Docker networks have no IPv6
  route. This avoids failed authentication requests to IPv6 session-server addresses;
  official-account authentication remains enabled.
- MCHPRS adapter is deployed with `proxy_chat=true`. A live player message was
  observed passing through the proxy's chat handler.
- The Paper JAR is installed in its existing plugin directory. Paper has not been
  restarted; the bridge defaults to inactive when first loaded.
- Paper's public bind, authentication, and forwarding configuration are unchanged.
  Chat across both backends requires the coordinated Paper cutover below.
- Private configuration, database, and consistent world backups are under
  `/srv/mchprs/deploy-backups/network-chat-20261005185317Z/`.
- Previous running images are tagged
  `mchprs-mchprs:before-network-chat-20261005` and
  `mchprs-velocity:before-network-chat-20261005`.

Build checks: `cargo check -p mchprs_core`, the Rust release Docker build, and both
Java plugin builds passed. No automated tests or Paper activation were run.

## Build and deployment

Java 21 and Maven are required for a native plugin build:

```sh
mvn -f plugins/network-chat/pom.xml -DskipTests package
```

The standard Docker build compiles the Java plugins and embeds the Velocity
plugin and a checksum-pinned LuckPerms release:

```sh
docker --context prod compose -p mchprs -f docker-compose-prod.yml build velocity mchprs
```

Compose does not export plugin JARs or install the Paper bridge into
RedstoneFun's plugin directory. Native builds produce the Paper JAR at
`plugins/network-chat/paper/target/redstonefun-chat-paper.jar`.

Velocity's standard image initializer copies `/plugins/*.jar` into its persistent
plugin directory. Avoid keeping older versions of the same plugin there.

### LuckPerms on Velocity

Configure `/srv/mchprs/velocity/plugins/luckperms/config.yml` to use the existing
PostgreSQL database and `luckperms_` tables. Do not use a separate local H2 database:
it would have unrelated users and ranks. Keep credentials private, outside the
repository and Docker image. Example (replace placeholders):

```yaml
server: proxy
storage-method: PostgreSQL
data:
  address: redstonefun-postgres:5432
  database: rf
  username: '<dedicated LuckPerms database account>'
  password: '<private password>'
  table-prefix: luckperms_
  pool-settings:
    maximum-pool-size: 3
    minimum-idle: 1
    properties: {}
sync-minutes: 1
messaging-service: auto
```

The proxy joins `redstonefun-network` to reach PostgreSQL. Keep the one-minute
refresh when using Paper's older LuckPerms version; do not rely on `auto` for
instant propagation across versions. If plugin messaging or Redis is configured
later, coordinate it with Paper; MCHPRS retains its own native polling. The Rust
backend does not implement LuckPerms' plugin messaging protocol.

LuckPerms 5.5.87 for Velocity is pinned by SHA-256 in the image. Paper's existing
LuckPerms plugin is not replaced by this build. Back up the shared database before
first deployment of a new LuckPerms version.

### MCHPRS configuration

In `/srv/mchprs/backend-config/Config.toml`, at the top level:

```toml
proxy_chat = true

[velocity]
secret_file = "/run/velocity/forwarding.secret"
```

`proxy_chat = true` requires authenticated modern forwarding; the server rejects
an incompatible startup configuration. Restart MCHPRS after editing this setting.
Missing bridge handshakes reject chat with feedback; there is no silent local
broadcast fallback. `mchprs.access.chat` and the existing backend rate limit still
apply. Slash commands continue through the backend command handler.

### Paper activation and proxy routing

Activation requires a planned Paper restart and a coordinated cutover:

1. Back up Paper's configuration, chat plugins, and the shared LuckPerms database.
2. Install the Paper bridge JAR and set
   `plugins/RedstoneFunChatBridge/config.yml` to `proxy-mode: true`.
3. Configure Paper for modern forwarding: `server.properties` has
   `online-mode=false`; `paper-global.yml` has `proxies.velocity.enabled=true`,
   `proxies.velocity.online-mode=true`, and the proxy's shared secret.
4. Bind Paper to a private host interface/port, restrict access to the proxy,
   and register it as backend `redstonefun` in Velocity. A bridged container
   cannot reach a host process bound to `127.0.0.1`; use a reachable private host
   interface and Docker's `host-gateway` mapping.
5. Keep Velocity in `online-mode=true`. Move its public publication to 25565
   after Paper releases that port. Add forced-host routing for `redstonefun.pl`
   and `mroww.redstonefun.pl` and configure the default backend.
6. Audit RFCORE/rfapi/RedstoneBot chat hooks and duplicate join/quit broadcasts.
   Disable EssentialsXChat's public renderer/broadcaster for the shared-chat
   setup; keep EssentialsX itself for commands and existing mute checks.
7. Restart Paper once, then bring up the proxy/backend with normal Compose.

The bridge checks Paper's forwarding settings on startup and stays inactive if
they are inconsistent. Its event listener respects chat already cancelled by
other plugins; EssentialsX mutes are checked again on the server thread.
Messages already independently broadcast by another plugin cannot be recalled;
duplicate broadcasters must be resolved during activation.

## Formatting and permissions

Velocity creates `plugins/redstonefun-chat/chat.properties` on first startup.
Edit that UTF-8 file and restart to change allowed backend names, cooldown, rank
prefixes, or separator. Rank priority is deterministic even when LuckPerms group
weights are equal:

- Admin (`admin`): dark red bold A, red nickname.
- Moderator (`moderator`): dark green bold M, green nickname.
- Inżynier (`engineer`): cyan I and nickname.
- Ekspert (`expert`): dark purple E, light purple nickname.
- Zaawansowany (`advanced`): gold Z and nickname.
- Budowniczy (`builder`): yellow B and nickname.
- Gracz (`default`): gray G and nickname.

Format: `[Rank] Nick » message`. Brackets and separator are dark gray; authored
message text is literal gray. It is never parsed as MiniMessage or legacy colors.
Joining/leaving uses bold dark gray brackets, dark green `+` / dark red `-`, and
a gray nickname. Announcements happen once per network connection, after bridge
readiness; backend switching does not announce another join or departure.

Proxy permission nodes (set globally or in `server=proxy` context):

- `network.chat.send`: explicit false denies public chat; undefined permits it.
- `network.chat.receive`: explicit false hides chat and join/quit announcements;
  undefined permits them.
- `network.chat.cooldown.bypass`: only explicit/effective true bypasses the
  configured proxy cooldown. Backend anti-flood limits still apply.

Paper permission:

- `redstonefun.chat.send`: defaults true; false prevents submission to Velocity.
  EssentialsX mutes also prevent submission.

Mute across both backends by setting `network.chat.send=false` on the proxy.
Paper's existing Essentials mute remains a Paper-local restriction. MCHPRS build,
plot, history, and command permissions remain in `server=mchprs` context.

## Protocol and failure behavior

Dedicated channel: `redstonefun:chat`. All integers are big-endian. Frames:

- HELLO / READY: version byte `1`, operation byte `0` / `1`, 16-byte session token.
- CHAT: version `1`, operation `2`, token, positive 64-bit sequence number,
  unsigned 16-bit UTF-8 byte length, plain text bytes.

Maximum text: 256 UTF-16 units / 1024 UTF-8 bytes. Control characters, section
signs, invalid UTF-8, trailing bytes, unknown versions, stale sessions, duplicate
sequences, and client-originated channel messages are rejected. Sender identity
comes from the authenticated backend connection, never from the payload.
The proxy marks every matching channel message handled before checking its source.

Handshakes retry up to ten times, without resetting counters on retransmission.
Paper bounds pending submissions to eight per player and schedules Bukkit and
Essentials access on the server thread. No database requests block chat handlers.

Messages are delivered as Adventure system chat. The backend processes signed
client chat normally; Velocity does not cancel or modify the signed chat packet.
Cross-server Mojang-reportable signed player chat, shared tab lists, global private
messages, and Discord integration are not implemented by this public-chat bridge.

## Rollback

Restore the previous Velocity/MCHPRS images and configuration. Set MCHPRS
`proxy_chat=false` and Paper `proxy-mode=false`, restore its previous chat plugins
and public bind/authentication settings, and restart during the rollback window.
Do not expose an offline-mode backend publicly. Preserve the new database backup.

## References

- [Paper modern forwarding](https://docs.papermc.io/velocity/player-information-forwarding/)
- [Velocity plugin messaging](https://docs.papermc.io/velocity/dev/plugin-messaging/)
- [Paper chat events](https://docs.papermc.io/paper/dev/chat-events/)
- [LuckPerms configuration](https://github.com/LuckPerms/wiki/blob/master/pages/Configuration.md)
