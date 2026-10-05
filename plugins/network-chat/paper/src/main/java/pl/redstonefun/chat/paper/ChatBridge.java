package pl.redstonefun.chat.paper;

import com.earth2me.essentials.Essentials;
import io.papermc.paper.event.player.AsyncChatEvent;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.configuration.file.YamlConfiguration;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.plugin.messaging.PluginMessageListener;
import pl.redstonefun.chat.ChatProtocol;

import java.io.File;
import java.io.IOException;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Predicate;

public final class ChatBridge extends JavaPlugin implements Listener, PluginMessageListener {
    private final ConcurrentHashMap<UUID, Session> sessions = new ConcurrentHashMap<>();
    private final ConcurrentHashMap<UUID, Long> lastFeedback = new ConcurrentHashMap<>();
    private volatile boolean enabled;
    private Predicate<Player> muted = player -> false;

    // Keep the optional Essentials classes out of the main plugin's field types.
    private static final class EssentialsMute {
        static Predicate<Player> hook(org.bukkit.plugin.Plugin plugin) {
            return plugin instanceof Essentials essentials
                ? player -> essentials.getUser(player).isMuted() : player -> false;
        }
    }

    private static final class Session {
        final Player player;
        final UUID token;
        final AtomicInteger pending = new AtomicInteger();
        long sequence;
        Session(Player player, UUID token) { this.player = player; this.token = token; }
    }

    @Override
    public void onEnable() {
        saveDefaultConfig();
        enabled = getConfig().getBoolean("proxy-mode", false);
        if (!enabled) {
            getLogger().info("Bridge installed but inactive (proxy-mode: false).");
            return;
        }
        var paper = YamlConfiguration.loadConfiguration(new File("config/paper-global.yml"));
        if (Bukkit.getOnlineMode() || !paper.getBoolean("proxies.velocity.enabled")
            || !paper.getBoolean("proxies.velocity.online-mode")
            || paper.getString("proxies.velocity.secret", "").trim().length() < 32) {
            enabled = false;
            getLogger().severe("Bridge requires Paper offline-mode with authenticated Velocity modern forwarding. Bridge disabled.");
            return;
        }
        var plugin = Bukkit.getPluginManager().getPlugin("Essentials");
        if (plugin != null && plugin.isEnabled()) muted = EssentialsMute.hook(plugin);
        getServer().getMessenger().registerOutgoingPluginChannel(this, ChatProtocol.CHANNEL);
        getServer().getMessenger().registerIncomingPluginChannel(this, ChatProtocol.CHANNEL, this);
        getServer().getPluginManager().registerEvents(this, this);
        getLogger().info("Shared chat bridge active; Velocity owns public chat and join/quit announcements.");
    }

    @Override
    public void onPluginMessageReceived(String channel, Player player, byte[] bytes) {
        if (!enabled || !ChatProtocol.CHANNEL.equals(channel)) return;
        try {
            var frame = ChatProtocol.decode(bytes);
            if (frame.kind() != ChatProtocol.HELLO) return;
            sessions.compute(player.getUniqueId(), (id, previous) ->
                previous != null && previous.player == player && previous.token.equals(frame.token())
                    ? previous : new Session(player, frame.token()));
            player.sendPluginMessage(this, channel, ChatProtocol.handshake(ChatProtocol.READY, frame.token()));
        } catch (IOException ignored) {
            // Ignore malformed or unsupported protocol frames without allocating queued work.
        }
    }

    @EventHandler(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    public void chat(AsyncChatEvent event) {
        if (!enabled) return;
        event.setCancelled(true);
        Player player = event.getPlayer();
        Session session = sessions.get(player.getUniqueId());
        // Preserve modifications from earlier moderation listeners, without copying the renderer.
        String text = PlainTextComponentSerializer.plainText().serialize(event.message());
        if (session == null || session.player != player || !ChatProtocol.validText(text)) {
            feedback(player, "Shared chat is unavailable or the message is invalid. Please try again.");
            return;
        }
        // Bound work queued from the async chat thread onto the server thread.
        if (session.pending.incrementAndGet() > 8) {
            session.pending.decrementAndGet();
            return;
        }
        Runnable send = () -> {
            try {
                if (!player.isOnline() || sessions.get(player.getUniqueId()) != session) return;
                // Bukkit and Essentials access stays on the server thread.
                if (!player.hasPermission("redstonefun.chat.send")
                    || muted.test(player)) {
                    player.sendMessage(Component.text("You cannot send public chat messages.", NamedTextColor.RED));
                    return;
                }
                if (session.sequence == Long.MAX_VALUE) return;
                player.sendPluginMessage(this, ChatProtocol.CHANNEL,
                    ChatProtocol.chat(session.token, ++session.sequence, text));
            } finally {
                session.pending.decrementAndGet();
            }
        };
        if (event.isAsynchronous()) getServer().getScheduler().runTask(this, send);
        else send.run();
    }

    private void feedback(Player player, String text) {
        long now = System.nanoTime();
        // A missing handshake must not turn a chat flood into unbounded scheduler work.
        final boolean[] allowed = {false};
        lastFeedback.compute(player.getUniqueId(), (id, previous) -> {
            if (previous == null || now - previous >= 1_000_000_000L) {
                allowed[0] = true;
                return now;
            }
            return previous;
        });
        if (!allowed[0]) return;
        // Schedule all Bukkit calls, including feedback, from async chat safely.
        getServer().getScheduler().runTask(this, () -> {
            if (player.isOnline()) player.sendMessage(Component.text(text, NamedTextColor.RED));
        });
    }

    @EventHandler(priority = EventPriority.HIGHEST)
    public void joined(PlayerJoinEvent event) {
        if (enabled) {
            event.joinMessage(null);
        }
    }

    @EventHandler(priority = EventPriority.HIGHEST)
    public void quit(PlayerQuitEvent event) {
        if (enabled) {
            sessions.remove(event.getPlayer().getUniqueId());
            lastFeedback.remove(event.getPlayer().getUniqueId());
            event.quitMessage(null);
        }
    }

    @Override
    public void onDisable() {
        enabled = false;
        sessions.clear();
        lastFeedback.clear();
        getServer().getMessenger().unregisterIncomingPluginChannel(this);
        getServer().getMessenger().unregisterOutgoingPluginChannel(this);
    }
}
