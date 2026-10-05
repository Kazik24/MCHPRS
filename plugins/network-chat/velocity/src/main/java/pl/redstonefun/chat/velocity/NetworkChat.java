package pl.redstonefun.chat.velocity;

import com.google.inject.Inject;
import com.velocitypowered.api.event.Subscribe;
import com.velocitypowered.api.event.connection.DisconnectEvent;
import com.velocitypowered.api.event.connection.PluginMessageEvent;
import com.velocitypowered.api.event.player.ServerPostConnectEvent;
import com.velocitypowered.api.event.proxy.ProxyInitializeEvent;
import com.velocitypowered.api.event.proxy.ProxyShutdownEvent;
import com.velocitypowered.api.plugin.Dependency;
import com.velocitypowered.api.plugin.Plugin;
import com.velocitypowered.api.plugin.annotation.DataDirectory;
import com.velocitypowered.api.proxy.Player;
import com.velocitypowered.api.proxy.ProxyServer;
import com.velocitypowered.api.proxy.ServerConnection;
import com.velocitypowered.api.proxy.messages.MinecraftChannelIdentifier;
import com.velocitypowered.api.scheduler.ScheduledTask;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import net.luckperms.api.LuckPerms;
import net.luckperms.api.LuckPermsProvider;
import net.luckperms.api.model.user.User;
import net.luckperms.api.query.QueryOptions;
import net.luckperms.api.util.Tristate;
import org.slf4j.Logger;
import pl.redstonefun.chat.ChatProtocol;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.Arrays;
import java.util.Properties;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

@Plugin(id = "redstonefun-chat", name = "RedstoneFun Network Chat", version = "1.0.0",
    dependencies = @Dependency(id = "luckperms"))
public final class NetworkChat {
    private static final MinecraftChannelIdentifier CHANNEL = MinecraftChannelIdentifier.from(ChatProtocol.CHANNEL);
    private static final LegacyComponentSerializer LEGACY = LegacyComponentSerializer.legacyAmpersand();
    private static final String[] RANKS = {"admin", "moderator", "engineer", "expert", "advanced", "builder", "default"};
    private final ProxyServer proxy;
    private final Logger logger;
    private final Path directory;
    private final Properties config = new Properties();
    private final ConcurrentHashMap<UUID, Session> sessions = new ConcurrentHashMap<>();
    private final Set<UUID> announced = ConcurrentHashMap.newKeySet();
    // Preserve rate limits when the player switches backend.
    private final ConcurrentHashMap<UUID, Long> lastMessages = new ConcurrentHashMap<>();
    private Set<String> backends;
    private LuckPerms permissions;
    private ScheduledTask handshakeTask;
    private long cooldownNanos;
    private volatile boolean active;

    private static final class Session {
        final ServerConnection connection;
        final UUID token = UUID.randomUUID();
        volatile boolean ready;
        int attempts;
        long lastSequence;
        Session(ServerConnection connection) { this.connection = connection; }
    }

    @Inject
    public NetworkChat(ProxyServer proxy, Logger logger, @DataDirectory Path directory) {
        this.proxy = proxy;
        this.logger = logger;
        this.directory = directory;
    }

    @Subscribe
    public void initialize(ProxyInitializeEvent event) throws IOException {
        Files.createDirectories(directory);
        Path file = directory.resolve("chat.properties");
        if (!Files.exists(file)) {
            try (var defaults = getClass().getResourceAsStream("/chat.properties")) {
                if (defaults == null) throw new IOException("Missing chat defaults");
                Files.copy(defaults, file);
            }
        }
        try (var reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) { config.load(reader); }
        backends = Set.copyOf(Arrays.stream(config.getProperty("backends", "mchprs,redstonefun").split(","))
            .map(String::trim).filter(s -> !s.isEmpty()).toList());
        long cooldown = Long.parseLong(config.getProperty("cooldown-ms", "500"));
        if (cooldown < 0 || cooldown > 60_000) throw new IOException("cooldown-ms must be 0..60000");
        cooldownNanos = Duration.ofMillis(cooldown).toNanos();
        permissions = LuckPermsProvider.get();
        proxy.getChannelRegistrar().register(CHANNEL);
        active = true;
        handshakeTask = proxy.getScheduler().buildTask(this, this::retryHandshakes)
            .repeat(Duration.ofSeconds(1)).schedule();
        logger.info("Shared chat enabled for backends {}", backends);
    }

    @Subscribe
    public void connected(ServerPostConnectEvent event) {
        if (!active) return;
        Player player = event.getPlayer();
        player.getCurrentServer().ifPresent(connection -> {
            if (!backends.contains(connection.getServerInfo().getName())) {
                sessions.remove(player.getUniqueId());
                return;
            }
            Session session = new Session(connection);
            sessions.put(player.getUniqueId(), session);
            hello(session);
        });
    }

    private void retryHandshakes() {
        sessions.values().forEach(this::hello);
    }

    private void hello(Session session) {
        synchronized (session) {
            if (session.ready || session.attempts >= 10 || !current(session)) return;
            session.attempts++;
            session.connection.sendPluginMessage(CHANNEL, ChatProtocol.handshake(ChatProtocol.HELLO, session.token));
            if (session.attempts == 10) logger.warn("No chat bridge acknowledgement from {} for {}",
                session.connection.getServerInfo().getName(), session.connection.getPlayer().getUsername());
        }
    }

    private boolean current(Session session) {
        Player player = session.connection.getPlayer();
        return player.isActive() && sessions.get(player.getUniqueId()) == session
            && player.getCurrentServer().filter(connection -> connection == session.connection).isPresent();
    }

    @Subscribe
    public void message(PluginMessageEvent event) {
        if (!CHANNEL.equals(event.getIdentifier())) return;
        // Handle before checking source: never forward client-originated bridge frames.
        event.setResult(PluginMessageEvent.ForwardResult.handled());
        if (!active || !(event.getSource() instanceof ServerConnection connection)) return;
        Player player = connection.getPlayer();
        Session session = sessions.get(player.getUniqueId());
        if (session == null || connection != session.connection) return;
        final ChatProtocol.Frame frame;
        try { frame = ChatProtocol.decode(event.getData()); }
        catch (IOException ignored) { return; }
        synchronized (session) {
            if (!current(session) || !session.token.equals(frame.token())) return;
            if (frame.kind() == ChatProtocol.READY) {
                session.ready = true;
                if (announced.add(player.getUniqueId())) {
                    broadcast(LEGACY.deserialize("&8&l[&2&l+&8&l]&7 " + player.getUsername()));
                }
                return;
            }
            if (frame.kind() != ChatProtocol.CHAT || !session.ready || frame.sequence() <= session.lastSequence) return;
            session.lastSequence = frame.sequence();
            User user = permissions.getUserManager().getUser(player.getUniqueId());
            if (user == null) {
                player.sendMessage(Component.text("Chat permissions are still loading. Please try again.", NamedTextColor.RED));
                return;
            }
            QueryOptions options = permissions.getContextManager().getQueryOptions(user)
                .orElse(permissions.getContextManager().getStaticQueryOptions());
            if (check(user, options, "network.chat.send") == Tristate.FALSE) {
                player.sendMessage(Component.text("You cannot send public chat messages.", NamedTextColor.RED));
                return;
            }
            long now = System.nanoTime();
            Long previous = lastMessages.get(player.getUniqueId());
            if (previous != null && now - previous < cooldownNanos
                && check(user, options, "network.chat.cooldown.bypass") != Tristate.TRUE) {
                player.sendMessage(Component.text("Please wait before sending another message.", NamedTextColor.GRAY));
                return;
            }
            lastMessages.put(player.getUniqueId(), now);
            var inheritedGroups = user.getInheritedGroups(options);
            String group = Arrays.stream(RANKS).filter(rank -> inheritedGroups.stream()
                .anyMatch(inherited -> inherited.getName().equalsIgnoreCase(rank))).findFirst().orElse("default");
            Component formatted = LEGACY.deserialize(config.getProperty("rank." + group,
                config.getProperty("rank.default", "&8[&7G&8] &7")) + player.getUsername()
                + config.getProperty("separator", " &8» "))
                .append(Component.text(frame.text(), NamedTextColor.GRAY));
            broadcast(formatted);
            // Plain text only; control characters have already been rejected.
            logger.info("[{}] <{}> {}", connection.getServerInfo().getName(), player.getUsername(), frame.text());
        }
    }

    private static Tristate check(User user, QueryOptions options, String node) {
        return user.getCachedData().getPermissionData(options).checkPermission(node);
    }

    private void broadcast(Component message) {
        for (Player recipient : proxy.getAllPlayers()) {
            // Only bridged, authenticated game sessions receive network messages.
            Session session = sessions.get(recipient.getUniqueId());
            if (session == null || !session.ready || !current(session)) continue;
            User user = permissions.getUserManager().getUser(recipient.getUniqueId());
            if (user == null) continue;
            QueryOptions options = permissions.getContextManager().getQueryOptions(user)
                .orElse(permissions.getContextManager().getStaticQueryOptions());
            if (check(user, options, "network.chat.receive") != Tristate.FALSE) recipient.sendMessage(message);
        }
    }

    @Subscribe
    public void disconnected(DisconnectEvent event) {
        Player player = event.getPlayer();
        sessions.remove(player.getUniqueId());
        lastMessages.remove(player.getUniqueId());
        if (announced.remove(player.getUniqueId()) && active) {
            broadcast(LEGACY.deserialize("&8&l[&4&l-&8&l]&7 " + player.getUsername()));
        }
    }

    @Subscribe
    public void shutdown(ProxyShutdownEvent event) {
        active = false;
        if (handshakeTask != null) handshakeTask.cancel();
        sessions.clear();
        announced.clear();
        lastMessages.clear();
    }
}
