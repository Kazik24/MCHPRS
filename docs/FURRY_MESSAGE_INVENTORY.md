# MCHPRS server message inventory

This inventory locates outgoing player-facing text relevant to [the furry messages and PCMD plan](FURRY_MESSAGES_AND_PCMD_PLAN.md). It covers the current working tree, including local changes, rather than only the base commit. The scope is server-to-player feedback and its producers; incoming packet parsing is listed where it leads to those replies.

All entries identify source locations and preserve current source expressions. Repeated wording remains listed at every send site. Runtime library errors have no finite phrase list, so their propagation paths are identified separately. This document does not change server behavior or message wording.

## Coverage

The scan found **182 message helper call sites across 12 files**, and **8 direct chat or disconnect packet construction sites**. These counts include shared transports and permission helper invocations; they are not counts of distinct sentences. Packet constructors can overlap helper implementations.

| Source file | Helper call sites |
| --- | ---: |
| [crates/core/src/interaction.rs](../crates/core/src/interaction.rs) | 4 |
| [crates/core/src/player.rs](../crates/core/src/player.rs) | 10 |
| [crates/core/src/plot/commands.rs](../crates/core/src/plot/commands.rs) | 82 |
| [crates/core/src/plot/containers.rs](../crates/core/src/plot/containers.rs) | 2 |
| [crates/core/src/plot/history.rs](../crates/core/src/plot/history.rs) | 2 |
| [crates/core/src/plot/mod.rs](../crates/core/src/plot/mod.rs) | 9 |
| [crates/core/src/plot/packet_handlers.rs](../crates/core/src/plot/packet_handlers.rs) | 10 |
| [crates/core/src/plot/redstone_tools/mod.rs](../crates/core/src/plot/redstone_tools/mod.rs) | 1 |
| [crates/core/src/plot/redstone_tools/search.rs](../crates/core/src/plot/redstone_tools/search.rs) | 4 |
| [crates/core/src/plot/worldedit/execute.rs](../crates/core/src/plot/worldedit/execute.rs) | 38 |
| [crates/core/src/plot/worldedit/mod.rs](../crates/core/src/plot/worldedit/mod.rs) | 12 |
| [crates/core/src/server.rs](../crates/core/src/server.rs) | 8 |

## Sending and routing

- `Player::send_error_message` and `send_system_message` use the `PacketSender` defaults: red and yellow, through `send_color_message` and `send_raw_system_message`. WorldEdit uses light purple. Add typed notices beside these entry points; keep the raw senders available for authored components.
- `Player::send_chat_message` wraps legacy chat components in a JSON `extra` array and delegates to `send_raw_chat`. RTPS reports, WorldEdit help, and plot broadcasts also use this chat path despite being server-authored.
- `plot/history.rs::record_tick` constructs a red `CChatMessage` directly, then sends the encoded packet to every plot packet sender. This is a feedback path outside the usual message helpers.
- `plot/packet_handlers.rs::handle_chat_message` sends player chat as `Message::ChatInfo` to the server thread. `server.rs::handle_message` applies `CONFIG.chat_format` and broadcasts `BroadcastMessage::Chat`; plots deliver it with `send_chat_message`. Preserve the player content.
- Player `/say` and `/tellraw`, and queued command-block messages, travel as `Message::CommandChat` to `BroadcastMessage::CommandChat`. Each plot checks `Recipient::matches` and sends the existing JSON via `send_raw_system_message`. Preserve authored content and the current sanitizer during cosmetic changes.
- Permission notices come from `send_no_permission_message`, including WorldEdit ownership, plot commands, command-block editing, interactions, and container clicks. History and RedstoneTools also return their own permission errors.
- Login rejection uses `CDisconnectLogin`; in-game kicks use `CDisconnect`. These are separate presentation surfaces from command feedback.

## Complete helper call sites

Payloads below are Rust source expressions with whitespace folded for readability. Empty permission-helper payloads share the phrase in `player.rs::send_no_permission_message`. Function names provide context; use the linked line for the precise command branch and recipients.

### crates/core/src/interaction.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [interaction.rs:25](../crates/core/src/interaction.rs#L25) | `on_use / send_color_message` | `ColorCode::DarkAqua, format_args!("Block at ({}, {}, {}):\n    {block:?}", pos.x, pos.y, pos.z),` | Preserve diagnostic values |
| [interaction.rs:37](../crates/core/src/interaction.rs#L37) | `on_use / send_color_message` | `ColorCode::Gold, format_args!("  Redstone power: {power_desc}"),` | Preserve diagnostic values |
| [interaction.rs:43](../crates/core/src/interaction.rs#L43) | `on_use / send_color_message` | `ColorCode::Aqua, format_args!("  Block entity:\n    {entity:?}"),` | Preserve diagnostic values |
| [interaction.rs:146](../crates/core/src/interaction.rs#L146) | `on_use / send_no_permission_message` | `shared permission notice` | Candidate notice |

### crates/core/src/player.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [player.rs:371](../crates/core/src/player.rs#L371) | `update / kick` | `json!({ "text": "Timed out." }).to_string()` | Disconnect notice |
| [player.rs:439](../crates/core/src/player.rs#L439) | `teleport / send_error_message` | `"We just saved you from a game crash, don't try it again!"` | Candidate notice |
| [player.rs:473](../crates/core/src/player.rs#L473) | `send_chat_message / send_raw_chat` | `sender, json` | Shared transport |
| [player.rs:477](../crates/core/src/player.rs#L477) | `send_no_permission_message / send_error_message` | `"You do not have permission to perform this action."` | Candidate notice |
| [player.rs:482](../crates/core/src/player.rs#L482) | `send_worldedit_message / send_color_message` | `ColorCode::LightPurple, message` | Shared transport |
| [player.rs:486](../crates/core/src/player.rs#L486) | `worldedit_set_first_position / send_worldedit_message` | `&format!( "First position set to ({}, {}, {})", pos.x, pos.y, pos.z )` | Candidate notice |
| [player.rs:495](../crates/core/src/player.rs#L495) | `worldedit_set_second_position / send_worldedit_message` | `&format!( "Second position set to ({}, {}, {})", pos.x, pos.y, pos.z )` | Candidate notice |
| [player.rs:678](../crates/core/src/player.rs#L678) | `send_error_message / send_color_message` | `ColorCode::Red, message` | Shared transport |
| [player.rs:683](../crates/core/src/player.rs#L683) | `send_system_message / send_color_message` | `ColorCode::Yellow, message` | Shared transport |
| [player.rs:688](../crates/core/src/player.rs#L688) | `send_color_message / send_raw_system_message` | `json!({ "text": message.to_string(), "color": col }) .to_string(),` | Shared transport |

### crates/core/src/plot/commands.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [commands.rs:52](../crates/core/src/plot/commands.rs#L52) | `handle_plot_command / send_error_message` | `"Invalid argument for /plot"` | Candidate notice |
| [commands.rs:57](../crates/core/src/plot/commands.rs#L57) | `handle_plot_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [commands.rs:64](../crates/core/src/plot/commands.rs#L64) | `handle_plot_command / send_system_message` | `&format!( "Plot owner is: {}", database::get_cached_username(owner.clone()).unwrap_or(owner) )` | Candidate notice |
| [commands.rs:69](../crates/core/src/plot/commands.rs#L69) | `handle_plot_command / send_system_message` | `"Plot is not owned by anyone."` | Candidate notice |
| [commands.rs:74](../crates/core/src/plot/commands.rs#L74) | `handle_plot_command / send_system_message` | `"Plot is already claimed!"` | Candidate notice |
| [commands.rs:96](../crates/core/src/plot/commands.rs#L96) | `handle_plot_command / send_error_message` | `"Invalid number of arguments!"` | Candidate notice |
| [commands.rs:104](../crates/core/src/plot/commands.rs#L104) | `handle_plot_command / send_error_message` | `"Plot index starts at 1"` | Candidate notice |
| [commands.rs:108](../crates/core/src/plot/commands.rs#L108) | `handle_plot_command / send_error_message` | `"Unable to parse index"` | Candidate notice |
| [commands.rs:123](../crates/core/src/plot/commands.rs#L123) | `handle_plot_command / send_system_message` | `&format!("Plot range (1, {}).", plots.len())` | Candidate notice |
| [commands.rs:127](../crates/core/src/plot/commands.rs#L127) | `handle_plot_command / send_system_message` | `&format!("{} does not own any plots.", args[0])` | Candidate notice |
| [commands.rs:132](../crates/core/src/plot/commands.rs#L132) | `handle_plot_command / send_error_message` | `"Invalid number of arguments!"` | Candidate notice |
| [commands.rs:141](../crates/core/src/plot/commands.rs#L141) | `handle_plot_command / send_error_message` | `"Unable to parse x coordinate!"` | Candidate notice |
| [commands.rs:147](../crates/core/src/plot/commands.rs#L147) | `handle_plot_command / send_error_message` | `"Unable to parse z coordinate!"` | Candidate notice |
| [commands.rs:158](../crates/core/src/plot/commands.rs#L158) | `handle_plot_command / send_system_message` | `&res` | Candidate notice |
| [commands.rs:161](../crates/core/src/plot/commands.rs#L161) | `handle_plot_command / send_system_message` | `"You are already locked to this plot."` | Candidate notice |
| [commands.rs:171](../crates/core/src/plot/commands.rs#L171) | `handle_plot_command / send_system_message` | `"You are now unlocked."` | Candidate notice |
| [commands.rs:173](../crates/core/src/plot/commands.rs#L173) | `handle_plot_command / send_system_message` | `"You are not locked to this plot."` | Candidate notice |
| [commands.rs:176](../crates/core/src/plot/commands.rs#L176) | `handle_plot_command / send_error_message` | `"Invalid argument for /plot"` | Candidate notice |
| [commands.rs:191](../crates/core/src/plot/commands.rs#L191) | `handle_redpiler_command / send_system_message` | `msg` | Candidate notice |
| [commands.rs:209](../crates/core/src/plot/commands.rs#L209) | `handle_redpiler_command / send_error_message` | `"Trace failed"` | Candidate notice |
| [commands.rs:217](../crates/core/src/plot/commands.rs#L217) | `handle_redpiler_command / send_error_message` | `"Invalid argument for /redpiler"` | Candidate notice |
| [commands.rs:241](../crates/core/src/plot/commands.rs#L241) | `handle_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [commands.rs:254](../crates/core/src/plot/commands.rs#L254) | `handle_command / send_error_message` | `"Usage: /help [topic]"` | Candidate notice |
| [commands.rs:256](../crates/core/src/plot/commands.rs#L256) | `handle_command / send_system_message` | `page` | Candidate notice |
| [commands.rs:258](../crates/core/src/plot/commands.rs#L258) | `handle_command / send_error_message` | `"Unknown help topic. Use /help for topics, or //help <command> for WorldEdit.",` | Candidate notice |
| [commands.rs:265](../crates/core/src/plot/commands.rs#L265) | `handle_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [commands.rs:275](../crates/core/src/plot/commands.rs#L275) | `handle_command / send_error_message` | `"Usage: /piston_anim [auto\|on\|off]"` | Candidate notice |
| [commands.rs:281](../crates/core/src/plot/commands.rs#L281) | `handle_command / send_system_message` | `&format!( "Piston animation: {} (effective {})", self.piston_animation, if self.world.fast_rendering { "off" } else { "on" } )` | Candidate notice |
| [commands.rs:298](../crates/core/src/plot/commands.rs#L298) | `handle_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [commands.rs:312](../crates/core/src/plot/commands.rs#L312) | `handle_command / send_error_message` | `&error` | Candidate notice |
| [commands.rs:316](../crates/core/src/plot/commands.rs#L316) | `handle_command / send_system_message` | `&crate::server::version_string()` | Candidate notice |
| [commands.rs:355](../crates/core/src/plot/commands.rs#L355) | `handle_command / send_error_message` | `"Usage: /whitelist [add \| remove] (username)"` | Candidate notice |
| [commands.rs:363](../crates/core/src/plot/commands.rs#L363) | `handle_command / send_chat_message` | `0, &ChatComponent::from_legacy_text(&format!( "&6RTPS from last 10s, 1m, 5m, 15m: &a{:.1}, {:.1}, {:.1}, {:.1} ({})", report.ten_s, report.one_m, report.five_m, report.fifteen_m, self.tps )),` | Candidate notice |
| [commands.rs:371](../crates/core/src/plot/commands.rs#L371) | `handle_command / send_chat_message` | `0, &ChatComponent::from_legacy_text(&format!( "&6No timings data. &a({})", self.tps )),` | Candidate notice |
| [commands.rs:388](../crates/core/src/plot/commands.rs#L388) | `handle_command / send_error_message` | `"Unable to parse rtps!"` | Candidate notice |
| [commands.rs:397](../crates/core/src/plot/commands.rs#L397) | `handle_command / send_system_message` | `"The rtps was successfully set."` | Candidate notice |
| [commands.rs:400](../crates/core/src/plot/commands.rs#L400) | `handle_command / send_system_message` | `&message` | Candidate notice |
| [commands.rs:401](../crates/core/src/plot/commands.rs#L401) | `handle_command / send_error_message` | `&error` | Candidate notice |
| [commands.rs:405](../crates/core/src/plot/commands.rs#L405) | `handle_command / send_error_message` | `&error` | Candidate notice |
| [commands.rs:411](../crates/core/src/plot/commands.rs#L411) | `handle_command / send_error_message` | `"Please specify a number of ticks to advance."` | Candidate notice |
| [commands.rs:418](../crates/core/src/plot/commands.rs#L418) | `handle_command / send_error_message` | `"Please specify a number of nano-ticks to advance.",` | Candidate notice |
| [commands.rs:424](../crates/core/src/plot/commands.rs#L424) | `handle_command / send_error_message` | `"Unable to parse nano-ticks!"` | Candidate notice |
| [commands.rs:428](../crates/core/src/plot/commands.rs#L428) | `handle_command / send_error_message` | `"Cannot advance nano-ticks while redpiler is active!",` | Candidate notice |
| [commands.rs:434](../crates/core/src/plot/commands.rs#L434) | `handle_command / send_error_message` | `"Disable tick history before nano/pico advancement.",` | Candidate notice |
| [commands.rs:444](../crates/core/src/plot/commands.rs#L444) | `handle_command / send_error_message` | `"Please specify a number of pico-ticks to advance.",` | Candidate notice |
| [commands.rs:450](../crates/core/src/plot/commands.rs#L450) | `handle_command / send_error_message` | `"Unable to parse pico-ticks!"` | Candidate notice |
| [commands.rs:454](../crates/core/src/plot/commands.rs#L454) | `handle_command / send_error_message` | `"Cannot advance pico-ticks while redpiler is active!",` | Candidate notice |
| [commands.rs:460](../crates/core/src/plot/commands.rs#L460) | `handle_command / send_error_message` | `"Disable tick history before nano/pico advancement.",` | Candidate notice |
| [commands.rs:470](../crates/core/src/plot/commands.rs#L470) | `handle_command / send_error_message` | `"Unable to parse ticks!"` | Candidate notice |
| [commands.rs:480](../crates/core/src/plot/commands.rs#L480) | `handle_command / send_system_message` | `&format!( "Plot has been advanced by {unit} ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [commands.rs:489](../crates/core/src/plot/commands.rs#L489) | `handle_command / send_system_message` | `"Automatic redpiler compilation has been enabled."` | Candidate notice |
| [commands.rs:492](../crates/core/src/plot/commands.rs#L492) | `handle_command / send_system_message` | `"Automatic redpiler compilation has been disabled."` | Candidate notice |
| [commands.rs:504](../crates/core/src/plot/commands.rs#L504) | `handle_command / send_error_message` | `"Unable to parse x coordinate!"` | Candidate notice |
| [commands.rs:510](../crates/core/src/plot/commands.rs#L510) | `handle_command / send_error_message` | `"Unable to parse y coordinate!"` | Candidate notice |
| [commands.rs:516](../crates/core/src/plot/commands.rs#L516) | `handle_command / send_error_message` | `"Unable to parse z coordinate!"` | Candidate notice |
| [commands.rs:520](../crates/core/src/plot/commands.rs#L520) | `handle_command / send_system_message` | `&format!("Teleporting to ({}, {}, {})", x, y, z)` | Candidate notice |
| [commands.rs:524](../crates/core/src/plot/commands.rs#L524) | `handle_command / send_system_message` | `&format!("Teleporting to {}", args[0])` | Candidate notice |
| [commands.rs:533](../crates/core/src/plot/commands.rs#L533) | `handle_command / send_error_message` | `"Invalid number of arguments for teleport command!"` | Candidate notice |
| [commands.rs:541](../crates/core/src/plot/commands.rs#L541) | `handle_command / send_error_message` | `"Invalid number of arguments!"` | Candidate notice |
| [commands.rs:549](../crates/core/src/plot/commands.rs#L549) | `handle_command / send_error_message` | `"Invalid number of arguments!"` | Candidate notice |
| [commands.rs:557](../crates/core/src/plot/commands.rs#L557) | `handle_command / send_error_message` | `"/speed <0-10>"` | Candidate notice |
| [commands.rs:563](../crates/core/src/plot/commands.rs#L563) | `handle_command / send_error_message` | `"Silly child, you can't have a negative flyspeed!"` | Candidate notice |
| [commands.rs:567](../crates/core/src/plot/commands.rs#L567) | `handle_command / send_error_message` | `"For performance reasons player speed cannot be higher than 10.",` | Candidate notice |
| [commands.rs:574](../crates/core/src/plot/commands.rs#L574) | `handle_command / send_error_message` | `"You can't set your speed to NaN or -NaN."` | Candidate notice |
| [commands.rs:580](../crates/core/src/plot/commands.rs#L580) | `handle_command / send_system_message` | `&format!( "Set flying speed to {} for {}", speed_arg, username )` | Candidate notice |
| [commands.rs:585](../crates/core/src/plot/commands.rs#L585) | `handle_command / send_error_message` | `"Unable to parse speed value"` | Candidate notice |
| [commands.rs:592](../crates/core/src/plot/commands.rs#L592) | `handle_command / send_error_message` | `"Invalid number of arguments!"` | Candidate notice |
| [commands.rs:600](../crates/core/src/plot/commands.rs#L600) | `handle_command / send_error_message` | `"Unknown gamemode"` | Candidate notice |
| [commands.rs:608](../crates/core/src/plot/commands.rs#L608) | `handle_command / send_error_message` | `"Usage: /container [type] [power]"` | Candidate notice |
| [commands.rs:615](../crates/core/src/plot/commands.rs#L615) | `handle_command / send_error_message` | `"Unable to parse power!"` | Candidate notice |
| [commands.rs:622](../crates/core/src/plot/commands.rs#L622) | `handle_command / send_error_message` | `"Container type must be one of [barrel, furnace, hopper]",` | Candidate notice |
| [commands.rs:630](../crates/core/src/plot/commands.rs#L630) | `handle_command / send_error_message` | `"Container power must be greater than 0 and lower than 15!",` | Candidate notice |
| [commands.rs:642](../crates/core/src/plot/commands.rs#L642) | `handle_command / send_system_message` | `&format!( "World send rate: {} Hz (effective {} Hz)", self.world_send_rate.0, self.effective_send_rate() )` | Candidate notice |
| [commands.rs:650](../crates/core/src/plot/commands.rs#L650) | `handle_command / send_error_message` | `"Usage: /worldsendrate <hertz>"` | Candidate notice |
| [commands.rs:655](../crates/core/src/plot/commands.rs#L655) | `handle_command / send_error_message` | `"Unable to parse send rate!"` | Candidate notice |
| [commands.rs:660](../crates/core/src/plot/commands.rs#L660) | `handle_command / send_error_message` | `"The world send rate cannot go higher than 1000!"` | Candidate notice |
| [commands.rs:667](../crates/core/src/plot/commands.rs#L667) | `handle_command / send_system_message` | `"The world send rate was successfully set."` | Candidate notice |
| [commands.rs:671](../crates/core/src/plot/commands.rs#L671) | `handle_command / send_system_message` | `"The world is already cursed."` | Candidate notice |
| [commands.rs:675](../crates/core/src/plot/commands.rs#L675) | `handle_command / send_system_message` | `"The world has been cursed. Redpiler disabled (/bless to undo)",` | Candidate notice |
| [commands.rs:684](../crates/core/src/plot/commands.rs#L684) | `handle_command / send_system_message` | `"The world has been blessed."` | Candidate notice |
| [commands.rs:687](../crates/core/src/plot/commands.rs#L687) | `handle_command / send_system_message` | `"The world is not cursed. (/curse to curse)"` | Candidate notice |
| [commands.rs:691](../crates/core/src/plot/commands.rs#L691) | `handle_command / send_error_message` | `"Command not found!"` | Candidate notice |

### crates/core/src/plot/containers.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [containers.rs:89](../crates/core/src/plot/containers.rs#L89) | `click_open_container / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [containers.rs:99](../crates/core/src/plot/containers.rs#L99) | `click_open_container / send_error_message` | `super::packet_handlers::ERROR_IO_ONLY` | Candidate notice |

### crates/core/src/plot/history.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [history.rs:424](../crates/core/src/plot/history.rs#L424) | `rewind_plot / send_system_message` | `"WorldEdit undo/redo cleared after tick rewind."` | Candidate notice |
| [history.rs:449](../crates/core/src/plot/history.rs#L449) | `rewind_plot / broadcast_plot_chat_message` | `&format!( "Plot rewound by {ticks} game ticks and paused. {} game ticks remain in history.", self.world.history.len() )` | Candidate notice |

### crates/core/src/plot/mod.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [mod.rs:714](../crates/core/src/plot/mod.rs#L714) | `broadcast_plot_chat_message / send_chat_message` | `0, &ChatComponent::from_legacy_text(message)` | Shared transport |
| [mod.rs:865](../crates/core/src/plot/mod.rs#L865) | `enter_plot / send_system_message` | `&format!( "Entering plot ({}, {})", self.world.x, self.world.z )` | Candidate notice |
| [mod.rs:968](../crates/core/src/plot/mod.rs#L968) | `start_redpiler / send_system_message` | `"This plot contains pistons, observers or command blocks and runs with the interpreter to preserve their behavior."` | Candidate notice |
| [mod.rs:976](../crates/core/src/plot/mod.rs#L976) | `start_redpiler / broadcast_plot_chat_message` | `&format!( "Tick history disabled because compiled execution is starting. Released approximately {}.", history::format_memory(bytes) )` | Candidate notice |
| [mod.rs:1095](../crates/core/src/plot/mod.rs#L1095) | `claim_plot / send_system_message` | `&format!("Claimed plot {},{}", plot_x, plot_z)` | Candidate notice |
| [mod.rs:1158](../crates/core/src/plot/mod.rs#L1158) | `handle_messages / send_raw_system_message` | `command.message.clone()` | Preserve authored text |
| [mod.rs:1164](../crates/core/src/plot/mod.rs#L1164) | `handle_messages / send_chat_message` | `sender, &message` | Preserve player chat |
| [mod.rs:1192](../crates/core/src/plot/mod.rs#L1192) | `handle_messages / kick` | `json!({ "text": "Server closed" }) .to_string(),` | Disconnect notice |
| [mod.rs:1585](../crates/core/src/plot/mod.rs#L1585) | `drop / send_error_message` | `"The plot you were previously in has crashed!"` | Candidate notice |

### crates/core/src/plot/packet_handlers.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [packet_handlers.rs:95](../crates/core/src/plot/packet_handlers.rs#L95) | `handle_update_command_block / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [packet_handlers.rs:135](../crates/core/src/plot/packet_handlers.rs#L135) | `handle_update_command_block / send_system_message` | `"Command block updated."` | Candidate notice |
| [packet_handlers.rs:336](../crates/core/src/plot/packet_handlers.rs#L336) | `handle_player_block_placement / send_system_message` | `"Can't interact with blocks outside of plot"` | Candidate notice |
| [packet_handlers.rs:360](../crates/core/src/plot/packet_handlers.rs#L360) | `handle_player_block_placement / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [packet_handlers.rs:365](../crates/core/src/plot/packet_handlers.rs#L365) | `handle_player_block_placement / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [packet_handlers.rs:379](../crates/core/src/plot/packet_handlers.rs#L379) | `handle_player_block_placement / send_error_message` | `ERROR_IO_ONLY` | Candidate notice |
| [packet_handlers.rs:615](../crates/core/src/plot/packet_handlers.rs#L615) | `handle_player_digging / send_system_message` | `"Can't break blocks outside of plot"` | Candidate notice |
| [packet_handlers.rs:640](../crates/core/src/plot/packet_handlers.rs#L640) | `handle_player_digging / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [packet_handlers.rs:645](../crates/core/src/plot/packet_handlers.rs#L645) | `handle_player_digging / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [packet_handlers.rs:652](../crates/core/src/plot/packet_handlers.rs#L652) | `handle_player_digging / send_error_message` | `ERROR_IO_ONLY` | Candidate notice |

### crates/core/src/plot/redstone_tools/mod.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [mod.rs:85](../crates/core/src/plot/redstone_tools/mod.rs#L85) | `send / send_raw_system_message` | `json!({"text": text, "color": color}).to_string()` | Candidate notice |

### crates/core/src/plot/redstone_tools/search.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [search.rs:100](../crates/core/src/plot/redstone_tools/search.rs#L100) | `display / send_raw_system_message` | `json!({ "text": "No matches found, nya~", "color": "light_purple" }).to_string()` | Candidate notice |
| [search.rs:111](../crates/core/src/plot/redstone_tools/search.rs#L111) | `display / send_raw_system_message` | `json!({ "text": format!("{} matches{suffix}; page {page}/{}, nya~", self.hits.len(), self.page_count()), "color": "light_purple" }).to_string()` | Candidate notice |
| [search.rs:124](../crates/core/src/plot/redstone_tools/search.rs#L124) | `display / send_raw_system_message` | `json!({"text": "", "extra": parts}).to_string()` | Candidate notice |
| [search.rs:135](../crates/core/src/plot/redstone_tools/search.rs#L135) | `display / send_raw_system_message` | `json!({"text": "", "extra": pages}).to_string()` | Candidate notice |

### crates/core/src/plot/worldedit/execute.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [execute.rs:71](../crates/core/src/plot/worldedit/execute.rs#L71) | `execute_set / send_worldedit_message` | `&format!( "Operation completed: {} block(s) affected ({:.00?})", blocks_updated, start_time.elapsed() )` | Candidate notice |
| [execute.rs:109](../crates/core/src/plot/worldedit/execute.rs#L109) | `execute_replace / send_worldedit_message` | `&format!( "Operation completed: {} block(s) affected ({:.00?})", blocks_updated, start_time.elapsed() )` | Candidate notice |
| [execute.rs:134](../crates/core/src/plot/worldedit/execute.rs#L134) | `execute_count / send_worldedit_message` | `&format!( "Counted {} block(s) ({:.00?})", blocks_counted, start_time.elapsed() )` | Candidate notice |
| [execute.rs:153](../crates/core/src/plot/worldedit/execute.rs#L153) | `execute_copy / send_worldedit_message` | `&format!( "Your selection was copied. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:172](../crates/core/src/plot/worldedit/execute.rs#L172) | `execute_cut / send_worldedit_message` | `&format!( "Your selection was cut. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:222](../crates/core/src/plot/worldedit/execute.rs#L222) | `execute_move / send_worldedit_message` | `&format!( "Your selection was moved. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:253](../crates/core/src/plot/worldedit/execute.rs#L253) | `execute_paste / send_worldedit_message` | `&format!( "Your clipboard was pasted. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:258](../crates/core/src/plot/worldedit/execute.rs#L258) | `execute_paste / send_system_message` | `"Your clipboard is empty!"` | Candidate notice |
| [execute.rs:278](../crates/core/src/plot/worldedit/execute.rs#L278) | `execute_load / send_worldedit_message` | `&format!( "The schematic was loaded to your clipboard. Use //paste to place it. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:287](../crates/core/src/plot/worldedit/execute.rs#L287) | `execute_load / send_error_message` | `msg` | Candidate notice |
| [execute.rs:294](../crates/core/src/plot/worldedit/execute.rs#L294) | `execute_load / send_error_message` | `&format!("Could not load schematic: {}", e.root_cause())` | Candidate notice |
| [execute.rs:311](../crates/core/src/plot/worldedit/execute.rs#L311) | `execute_save / send_worldedit_message` | `&format!( "The schematic was saved sucessfuly. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:320](../crates/core/src/plot/worldedit/execute.rs#L320) | `execute_save / send_error_message` | `&format!("Could not save schematic: {}", err.root_cause())` | Candidate notice |
| [execute.rs:358](../crates/core/src/plot/worldedit/execute.rs#L358) | `execute_stack / send_worldedit_message` | `&format!( "Your selection was stacked. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:367](../crates/core/src/plot/worldedit/execute.rs#L367) | `execute_undo / send_error_message` | `"There is nothing left to undo."` | Candidate notice |
| [execute.rs:373](../crates/core/src/plot/worldedit/execute.rs#L373) | `execute_undo / send_error_message` | `"Cannot undo outside of your current plot."` | Candidate notice |
| [execute.rs:405](../crates/core/src/plot/worldedit/execute.rs#L405) | `execute_redo / send_error_message` | `"There is nothing left to redo."` | Candidate notice |
| [execute.rs:411](../crates/core/src/plot/worldedit/execute.rs#L411) | `execute_redo / send_error_message` | `"Cannot redo outside of your current plot."` | Candidate notice |
| [execute.rs:444](../crates/core/src/plot/worldedit/execute.rs#L444) | `execute_sel / send_worldedit_message` | `"Selection cleared."` | Candidate notice |
| [execute.rs:468](../crates/core/src/plot/worldedit/execute.rs#L468) | `execute_hpos1 / send_error_message` | `"No block in sight!"` | Candidate notice |
| [execute.rs:482](../crates/core/src/plot/worldedit/execute.rs#L482) | `execute_hpos2 / send_error_message` | `"No block in sight!"` | Candidate notice |
| [execute.rs:497](../crates/core/src/plot/worldedit/execute.rs#L497) | `execute_expand / send_worldedit_message` | `&format!("Region expanded {} block(s).", amount)` | Candidate notice |
| [execute.rs:511](../crates/core/src/plot/worldedit/execute.rs#L511) | `execute_contract / send_worldedit_message` | `&format!("Region contracted {} block(s).", amount)` | Candidate notice |
| [execute.rs:543](../crates/core/src/plot/worldedit/execute.rs#L543) | `execute_shift / send_worldedit_message` | `&format!("Region shifted {} block(s).", amount)` | Candidate notice |
| [execute.rs:621](../crates/core/src/plot/worldedit/execute.rs#L621) | `execute_flip / send_worldedit_message` | `&format!( "The clipboard copy has been flipped. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:633](../crates/core/src/plot/worldedit/execute.rs#L633) | `execute_rotate / send_worldedit_message` | `"Successfully rotated by 0! That took a lot of work."` | Candidate notice |
| [execute.rs:641](../crates/core/src/plot/worldedit/execute.rs#L641) | `execute_rotate / send_error_message` | `"Rotate amount must be a multiple of 90."` | Candidate notice |
| [execute.rs:727](../crates/core/src/plot/worldedit/execute.rs#L727) | `execute_rotate / send_worldedit_message` | `&format!( "The clipboard copy has been rotated. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:750](../crates/core/src/plot/worldedit/execute.rs#L750) | `execute_help / send_error_message` | `&format!("Unknown command: {}", command_name)` | Candidate notice |
| [execute.rs:855](../crates/core/src/plot/worldedit/execute.rs#L855) | `execute_help / send_chat_message` | `0, &message` | Candidate notice |
| [execute.rs:901](../crates/core/src/plot/worldedit/execute.rs#L901) | `execute_ascend / send_error_message` | `"No free spot above you found."` | Candidate notice |
| [execute.rs:906](../crates/core/src/plot/worldedit/execute.rs#L906) | `execute_ascend / send_worldedit_message` | `&format!("Ascended {} levels.", initial_levels - levels)` | Candidate notice |
| [execute.rs:937](../crates/core/src/plot/worldedit/execute.rs#L937) | `execute_descend / send_error_message` | `"No free spot below you found."` | Candidate notice |
| [execute.rs:942](../crates/core/src/plot/worldedit/execute.rs#L942) | `execute_descend / send_worldedit_message` | `&format!("Descended {} levels.", initial_levels - levels)` | Candidate notice |
| [execute.rs:986](../crates/core/src/plot/worldedit/execute.rs#L986) | `execute_rstack / send_worldedit_message` | `&format!( "Your selection was stacked successfully. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:1004](../crates/core/src/plot/worldedit/execute.rs#L1004) | `execute_update / send_error_message` | `"Your selection is incomplete."` | Candidate notice |
| [execute.rs:1011](../crates/core/src/plot/worldedit/execute.rs#L1011) | `execute_update / send_worldedit_message` | `&format!( "Your selection was updated sucessfully. ({:.00?})", start_time.elapsed() )` | Candidate notice |
| [execute.rs:1083](../crates/core/src/plot/worldedit/execute.rs#L1083) | `execute_replace_container / send_worldedit_message` | `&format!( "Your selection was replaced sucessfully. ({:.00?})", start_time.elapsed() )` | Candidate notice |

### crates/core/src/plot/worldedit/mod.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [mod.rs:53](../crates/core/src/plot/worldedit/mod.rs#L53) | `execute_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [mod.rs:58](../crates/core/src/plot/worldedit/mod.rs#L58) | `execute_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [mod.rs:64](../crates/core/src/plot/worldedit/mod.rs#L64) | `execute_command / send_no_permission_message` | `shared permission notice` | Candidate notice |
| [mod.rs:72](../crates/core/src/plot/worldedit/mod.rs#L72) | `execute_command / send_error_message` | `"Make a region selection first."` | Candidate notice |
| [mod.rs:78](../crates/core/src/plot/worldedit/mod.rs#L78) | `execute_command / send_system_message` | `"First position is outside plot bounds!"` | Candidate notice |
| [mod.rs:82](../crates/core/src/plot/worldedit/mod.rs#L82) | `execute_command / send_system_message` | `"Second position is outside plot bounds!"` | Candidate notice |
| [mod.rs:88](../crates/core/src/plot/worldedit/mod.rs#L88) | `execute_command / send_error_message` | `"Your clipboard is empty. Use //copy first."` | Candidate notice |
| [mod.rs:102](../crates/core/src/plot/worldedit/mod.rs#L102) | `execute_command / send_error_message` | `"Flag with argument must be last in grouping"` | Candidate notice |
| [mod.rs:108](../crates/core/src/plot/worldedit/mod.rs#L108) | `execute_command / send_error_message` | `&format!("Unknown flag: {}", flag)` | Candidate notice |
| [mod.rs:114](../crates/core/src/plot/worldedit/mod.rs#L114) | `execute_command / send_error_message` | `"Flag requires an argument"` | Candidate notice |
| [mod.rs:134](../crates/core/src/plot/worldedit/mod.rs#L134) | `execute_command / send_error_message` | `"Too many arguments."` | Candidate notice |
| [mod.rs:144](../crates/core/src/plot/worldedit/mod.rs#L144) | `execute_command / send_error_message` | `&err.to_string()` | Candidate notice |

### crates/core/src/server.rs

| Location | Function and sender | Current payload | Treatment |
| --- | --- | --- | --- |
| [server.rs:523](../crates/core/src/server.rs#L523) | `handle_message / kick` | `json!({"text": format!("Could not load plot {},{}. Please contact the server administrator.", x, z)}).to_string()` | Disconnect notice |
| [server.rs:566](../crates/core/src/server.rs#L566) | `handle_message / send_system_message` | `"Their plot wasn't loaded. How did this happen??"` | Candidate notice |
| [server.rs:580](../crates/core/src/server.rs#L580) | `handle_message / send_system_message` | `"Player not found!"` | Candidate notice |
| [server.rs:594](../crates/core/src/server.rs#L594) | `handle_message / send_system_message` | `&msg` | Candidate notice |
| [server.rs:603](../crates/core/src/server.rs#L603) | `handle_message / send_error_message` | `"Whitelist is not enabled!"` | Candidate notice |
| [server.rs:616](../crates/core/src/server.rs#L616) | `handle_message / send_system_message` | `&msg` | Candidate notice |
| [server.rs:626](../crates/core/src/server.rs#L626) | `handle_message / send_error_message` | `"That player is not whitelisted on this server."` | Candidate notice |
| [server.rs:629](../crates/core/src/server.rs#L629) | `handle_message / send_error_message` | `"Whitelist is not enabled!"` | Candidate notice |

## Direct packet construction

| Location | Packet | Current fields |
| --- | --- | --- |
| [player.rs:319](../crates/core/src/player.rs#L319) | `CDisconnect` | `reason:json!({"text":"Your player save could not be loaded. Ask the administrator to inspect the server log; your original file was preserved."}).to_string()` |
| [player.rs:461](../crates/core/src/player.rs#L461) | `CChatMessage` | `message, sender, position: 0,` |
| [player.rs:514](../crates/core/src/player.rs#L514) | `CDisconnect` | `reason` |
| [player.rs:667](../crates/core/src/player.rs#L667) | `CChatMessage` | `message, sender: 0, position: 1,` |
| [history.rs:269](../crates/core/src/plot/history.rs#L269) | `CChatMessage` | `message, position: 1, sender: 0,` |
| [server.rs:327](../crates/core/src/server.rs#L327) | `CDisconnectLogin` | `reason: json!({ "text": "You are not whitelisted on this server" }) .to_string(),` |
| [server.rs:730](../crates/core/src/server.rs#L730) | `CDisconnectLogin` | `reason: json!({ "text": format!("Version mismatch, I'm on {}!", MC_VERSION) }) .to_string(),` |
| [server.rs:742](../crates/core/src/server.rs#L742) | `CDisconnectLogin` | `reason: json!({ "text": "If you wish to use IP forwarding, please enable it in your BungeeCord config as well!" }) .to_string(),` |

## Text produced before sending

An expression such as `send_error_message(&error)` is only the final send. The following catalog traces local phrases that feed it, help-page bodies, and rich-text builders. Strings are shown as source literals so escape sequences, placeholders, and command syntax remain visible. Empty strings, identifiers, parser tokens, and constant style names are omitted.

### History and rewind

`control_history` returns success or error strings to `/rhistory` in `plot/commands.rs`. `rewind_plot` returns errors to `/rback`; successful rewind and undo clearing are listed in the send-site table. `record_tick` broadcasts recording errors. Budget and codec errors propagate through these paths.

#### crates/core/src/plot/history.rs

| Location | Current literal or template |
| --- | --- |
| [history.rs:28](../crates/core/src/plot/history.rs#L28) | `"More than {NORMAL_HISTORY_LIMIT} game ticks requires {UNLIMITED_HISTORY_PERMISSION} permission."` |
| [history.rs:99](../crates/core/src/plot/history.rs#L99) | `"Tick history: {}. Available: {}/{} game ticks.\nUncompressed: {} \| Compressed: {} \| Server: {} / {}"` |
| [history.rs:113](../crates/core/src/plot/history.rs#L113) | `"History capacity must be between 1 and 2147483647 game ticks."` |
| [history.rs:117](../crates/core/src/plot/history.rs#L117) | `"History size overflow."` |
| [history.rs:122](../crates/core/src/plot/history.rs#L122) | `"Unable to allocate the history buffer."` |
| [history.rs:176](../crates/core/src/plot/history.rs#L176) | `"Tick history is disabled. Use /rhistory on [ticks]."` |
| [history.rs:179](../crates/core/src/plot/history.rs#L179) | `"Specify a positive number of game ticks to rewind."` |
| [history.rs:183](../crates/core/src/plot/history.rs#L183) | `"Only {} game ticks are available to rewind."` |
| [history.rs:231](../crates/core/src/plot/history.rs#L231) | `"History size overflow."` |
| [history.rs:233](../crates/core/src/plot/history.rs#L233) | `"One compressed snapshot cannot fit the history limit."` |
| [history.rs:238](../crates/core/src/plot/history.rs#L238) | `"History memory estimate exceeds the supported size."` |
| [history.rs:246](../crates/core/src/plot/history.rs#L246) | `"Finish the partial tick with /radvance 1 before using tick history."` |
| [history.rs:263](../crates/core/src/plot/history.rs#L263) | `"Tick history stopped: {error}"` |
| [history.rs:265](../crates/core/src/plot/history.rs#L265) | `"Tick history stopped: {error}"` |
| [history.rs:298](../crates/core/src/plot/history.rs#L298) | `"{:.2} GiB"` |
| [history.rs:300](../crates/core/src/plot/history.rs#L300) | `"{:.2} MiB"` |
| [history.rs:302](../crates/core/src/plot/history.rs#L302) | `"{:.2} KiB"` |
| [history.rs:304](../crates/core/src/plot/history.rs#L304) | `"{bytes} B"` |
| [history.rs:317](../crates/core/src/plot/history.rs#L317) | `"You do not have permission to use this command."` |
| [history.rs:327](../crates/core/src/plot/history.rs#L327) | `"You do not have permission to change this plot."` |
| [history.rs:348](../crates/core/src/plot/history.rs#L348) | `"Server history: {} / {}"` |
| [history.rs:355](../crates/core/src/plot/history.rs#L355) | `"Requires {MEMORY_PERMISSION} permission."` |
| [history.rs:359](../crates/core/src/plot/history.rs#L359) | `"Specify a nonnegative memory limit in MiB."` |
| [history.rs:365](../crates/core/src/plot/history.rs#L365) | `"History limit saved: {}."` |
| [history.rs:370](../crates/core/src/plot/history.rs#L370) | `"History disabled. Released {}."` |
| [history.rs:377](../crates/core/src/plot/history.rs#L377) | `"Tick history is only available during interpreted execution."` |
| [history.rs:382](../crates/core/src/plot/history.rs#L382) | `"Specify a positive history capacity in game ticks."` |
| [history.rs:391](../crates/core/src/plot/history.rs#L391) | `"History enabled: up to {capacity} game ticks. Estimated size: {}."` |
| [history.rs:395](../crates/core/src/plot/history.rs#L395) | `"Usage: /rhistory [on [ticks]\|off\|status\|limit [MiB]]"` |
| [history.rs:405](../crates/core/src/plot/history.rs#L405) | `"Specify a positive number of game ticks."` |
| [history.rs:406](../crates/core/src/plot/history.rs#L406) | `"Usage: /rback [ticks]"` |
| [history.rs:409](../crates/core/src/plot/history.rs#L409) | `"Tick rewind is only available during interpreted execution."` |
| [history.rs:424](../crates/core/src/plot/history.rs#L424) | `"WorldEdit undo/redo cleared after tick rewind."` |
| [history.rs:450](../crates/core/src/plot/history.rs#L450) | `"Plot rewound by {ticks} game ticks and paused. {} game ticks remain in history."` |

#### crates/core/src/plot/history/budget.rs

| Location | Current literal or template |
| --- | --- |
| [budget.rs:22](../crates/core/src/plot/history/budget.rs#L22) | `"Memory limit must be a nonnegative, representable number of MiB."` |
| [budget.rs:52](../crates/core/src/plot/history/budget.rs#L52) | `"Tick-history memory limit reached."` |
| [budget.rs:54](../crates/core/src/plot/history/budget.rs#L54) | `"Tick-history memory limit reached."` |
| [budget.rs:72](../crates/core/src/plot/history/budget.rs#L72) | `"History uses {}. Free buffers with /rhistory off first."` |
| [budget.rs:105](../crates/core/src/plot/history/budget.rs#L105) | `"Unable to allocate history bytes."` |

#### crates/core/src/plot/history/codec.rs

| Location | Current literal or template |
| --- | --- |
| [codec.rs:76](../crates/core/src/plot/history/codec.rs#L76) | `"Snapshot is too large."` |
| [codec.rs:78](../crates/core/src/plot/history/codec.rs#L78) | `"History capture workspace: {e}"` |
| [codec.rs:98](../crates/core/src/plot/history/codec.rs#L98) | `"Compression size overflow."` |
| [codec.rs:100](../crates/core/src/plot/history/codec.rs#L100) | `"History compression workspace: {e}"` |
| [codec.rs:142](../crates/core/src/plot/history/codec.rs#L142) | `"Invalid history checksum or size."` |
| [codec.rs:150](../crates/core/src/plot/history/codec.rs#L150) | `"Invalid history snapshot: {e}"` |
| [codec.rs:155](../crates/core/src/plot/history/codec.rs#L155) | `"History rewind workspace: {error}"` |
| [codec.rs:158](../crates/core/src/plot/history/codec.rs#L158) | `"Invalid history compression: {error}"` |
| [codec.rs:160](../crates/core/src/plot/history/codec.rs#L160) | `"Invalid history size."` |

#### crates/core/src/config.rs

| Location | Current literal or template |
| --- | --- |
| [config.rs:18](../crates/core/src/config.rs#L18) | `"Cannot save history limit: {e}"` |

### Say and Tellraw

Parser and selector errors feed the player command error sender. Command blocks store the same errors in `last_output`. The `/say` template `[{source}] {args}` is authored-message framing, not a notice to decorate. Unsupported click and hover fields are discarded without a user-facing sanitation notice today.

#### crates/core/src/chat_commands.rs

| Location | Current literal or template |
| --- | --- |
| [chat_commands.rs:29](../crates/core/src/chat_commands.rs#L29) | `"Text component nesting is too deep"` |
| [chat_commands.rs:36](../crates/core/src/chat_commands.rs#L36) | `"Expected a text string, object or array"` |
| [chat_commands.rs:134](../crates/core/src/chat_commands.rs#L134) | `"Unclosed selector options"` |
| [chat_commands.rs:143](../crates/core/src/chat_commands.rs#L143) | `"Supported targets: @a, @s or a player name"` |
| [chat_commands.rs:162](../crates/core/src/chat_commands.rs#L162) | `"[{source}] {args}"` |
| [chat_commands.rs:167](../crates/core/src/chat_commands.rs#L167) | `"Usage: /tellraw <target> <JSON text>"` |
| [chat_commands.rs:173](../crates/core/src/chat_commands.rs#L173) | `"Usage: /say <message>"` |
| [chat_commands.rs:174](../crates/core/src/chat_commands.rs#L174) | `"Only /tellraw and /say are supported in command blocks"` |

### General command payload variables

These producers resolve variables used by send sites and compose the version reply.

#### crates/core/src/plot/commands.rs

| Location | Current literal or template |
| --- | --- |
| [commands.rs:157](../crates/core/src/plot/commands.rs#L157) | `"Locked to plot ({}, {}). Use '/p unlock' to unlock."` |
| [commands.rs:189](../crates/core/src/plot/commands.rs#L189) | `"Redpiler optimization is highly unstable and can break builds. Use with caution!"` |

#### crates/core/src/server.rs

| Location | Current literal or template |
| --- | --- |
| [server.rs:36](../crates/core/src/server.rs#L36) | `"MCHPRS {} (Minecraft {}, protocol {})"` |
| [server.rs:593](../crates/core/src/server.rs#L593) | `"{} was sucessfully added to the whitelist."` |
| [server.rs:613](../crates/core/src/server.rs#L613) | `"{} was sucessfully removed from the whitelist."` |

#### crates/core/src/plot/packet_handlers.rs

| Location | Current literal or template |
| --- | --- |
| [packet_handlers.rs:22](../crates/core/src/plot/packet_handlers.rs#L22) | ``"This plot cannot be interacted with while redpiler is active with `--io-only`. To stop redpiler, run `/redpiler reset`."`` |

### General help pages

`plot/help.rs::page` supplies the nine help bodies to `/help`; the help topic tokens themselves should stay exact.

#### crates/core/src/plot/help.rs

| Location | Current literal or template |
| --- | --- |
| [help.rs:9](../crates/core/src/plot/help.rs#L9) | `"MCHPRS quick start\nClaim a plot with /p auto.\n/help plots - Claim, visit and find your plot.\n/help rtps - Pause, speed up and step through a circuit.\n/help we - Select, copy, paste and undo.\n/help schematics - Load and save schematics.\n/help pistons - Animation and client update settings.\n/help rewind - tick rewind commands.\n/help chat - Messages and command blocks.\n/help redpiler - Compiled simulation."` |
| [help.rs:19](../crates/core/src/plot/help.rs#L19) | `"Plots\n/p auto claim an empty plot. /p claim claims the plot you're standing in.\n/p info shows its owner; /p middle takes you to its centre.\n/p visit <player> [number] visits one of that player's plots.\n/p tp <x> <z> goes to plot coordinates, not block coordinates.\n/p lock keeps you in this plot; /p unlock lets you leave.\n/p select selects the whole plot for WorldEdit. Editing may require ownership or permission."` |
| [help.rs:26](../crates/core/src/plot/help.rs#L26) | `"Tick control\n/rtps shows the current speed. /rtps 20 runs at normal game speed.\n/rtps 0 pauses the tickrate. /rtps 1000 speeds it up; /rtps unlimited runs as fast as it can.\n/radvance Advances one game tick if rtps is 0. /radv is an alias.\n/radvance 10 advances ten game ticks. \n/radvance nano 1 advances a batch of work (One \"Nanotick\"; /radvance pico 1 advances one operation (One \"Picotick\")\n"` |
| [help.rs:33](../crates/core/src/plot/help.rs#L33) | `"WorldEdit\nThis server supports subset of WorldEdit commands, check autofill with // to check what is available.\n\n"` |
| [help.rs:36](../crates/core/src/plot/help.rs#L36) | `"Schematics\n//load my_schematic.schem reads a schematic into your clipboard. \nUse //paste to place it where you're standing\nTo save a build: select it, //copy, then //save my_schematic.schem.\nYou can load schematics from redstonefun server under rf/ folder.  \n"` |
| [help.rs:42](../crates/core/src/plot/help.rs#L42) | `"Animations and client updates\n/piston_anim shows this plot's setting. /bisdon_anim is an alias.\n/piston_anim auto follows the server threshold: by default, animations turn off above 100 TPS.\n/piston_anim on keeps animations on. /piston_anim off shows static blocks at any speed.\nThis affects only clients animations, piston tick and update behaviour stays the same.\n/wsr shows configured and effective block-update rates. /wsr 20 sends up to 20 updates per second; /wsr 0 stops periodic block updates.\nStatic rendering caps the send rate at 10 per second by default. A lower /wsr still applies."` |
| [help.rs:49](../crates/core/src/plot/help.rs#L49) | `"Tick rewind\n/rhistory on [ticks] starts recording; the default is 100 ticks.\n/rhistory shows ticks and compressed/uncompressed sizes. /rhistory off frees the buffer.\n/rback rewinds one tick; /rback 10 rewinds ten. Rewind pauses the plot and clears WorldEdit undo/redo. Use /rtps 20 to resume.\nAll plots share a 2 GiB memory limit by default. Older ticks drop when memory fills. Admins can use /rhistory limit <MiB> to change it.\nMore than 1000 ticks requires plots.admin.rewind.unlimited; changing memory requires plots.admin.rewind.memory. Grant these through LuckPerms.\nHistory needs the interpreter and whole ticks. Compilation or restart clears it.\nTry /rtps 0, /rhistory on, /radvance 10, then /rback 5."` |
| [help.rs:57](../crates/core/src/plot/help.rs#L57) | `"Messages and command blocks\n/say Hello everyone sends a message to all players.\n/tellraw @a {\"text\":\"Hello\",\"color\":\"gold\",\"bold\":true} sends formatted text.\nUse @a for everyone, @s for yourself, or a player name. Selector filters are ignored. Text, colours, basic styles and extra components work; scoreboard text and click/hover actions are ignored.\nPlace a command block, open it and enter say or tellraw. Impulse blocks run on a redstone pulse; repeating and chain blocks are also supported. Editing requires creative mode and permission.\nCommand blocks can use @a or player names; @s has no player there. Other commands are kept in schematics but don't run."` |
| [help.rs:63](../crates/core/src/plot/help.rs#L63) | `"Redpiler\n/rp compile compiles a circuit for faster simulation. /rp reset returns to the interpreter.\n/rp inspect inspects the block you're looking at. /toggleautorp toggles automatic compilation for this plot.\nPlots with pistons, observers or command blocks stay on the interpreter to preserve their behaviour.\nUse the interpreter for nano/pico stepping. Start with /rtps 0, then /rp reset and /radvance pico 1."` |

### WorldEdit arguments and rich help

`ArgumentParseError::fmt` wraps reasons from argument/default parsing, including `PatternParseError::fmt`; native integer parse errors are also forwarded. `execute_help` combines component labels, command descriptions, argument descriptions, flag descriptions, and computed defaults before its single chat send. The metadata catalog below includes descriptions passed through `arg!` and `flag!`.

#### crates/core/src/plot/worldedit/mod.rs

| Location | Current literal or template |
| --- | --- |
| [mod.rs:181](../crates/core/src/plot/worldedit/mod.rs#L181) | `"Error parsing argument of type {:?}: {}"` |
| [mod.rs:277](../crates/core/src/plot/worldedit/mod.rs#L277) | `"argument can't be inferred"` |
| [mod.rs:305](../crates/core/src/plot/worldedit/mod.rs#L305) | `"unknown direction"` |
| [mod.rs:310](../crates/core/src/plot/worldedit/mod.rs#L310) | `"error parsing uint"` |
| [mod.rs:357](../crates/core/src/plot/worldedit/mod.rs#L357) | `"unknown direction"` |
| [mod.rs:367](../crates/core/src/plot/worldedit/mod.rs#L367) | `"error parsing container type"` |
| [mod.rs:459](../crates/core/src/plot/worldedit/mod.rs#L459) | `"Go upwards some distance"` |
| [mod.rs:461](../crates/core/src/plot/worldedit/mod.rs#L461) | `"Distance to go upwards"` |
| [mod.rs:468](../crates/core/src/plot/worldedit/mod.rs#L468) | `"Go up a floor"` |
| [mod.rs:470](../crates/core/src/plot/worldedit/mod.rs#L470) | `"# of levels to ascend"` |
| [mod.rs:478](../crates/core/src/plot/worldedit/mod.rs#L478) | `"Go down a floor"` |
| [mod.rs:480](../crates/core/src/plot/worldedit/mod.rs#L480) | `"# of levels to descend"` |
| [mod.rs:488](../crates/core/src/plot/worldedit/mod.rs#L488) | `"Set position 1"` |
| [mod.rs:495](../crates/core/src/plot/worldedit/mod.rs#L495) | `"Set position 2"` |
| [mod.rs:502](../crates/core/src/plot/worldedit/mod.rs#L502) | `"Set position 1 to targeted block"` |
| [mod.rs:509](../crates/core/src/plot/worldedit/mod.rs#L509) | `"Set position 2 to targeted block"` |
| [mod.rs:516](../crates/core/src/plot/worldedit/mod.rs#L516) | `"Choose a region selector"` |
| [mod.rs:522](../crates/core/src/plot/worldedit/mod.rs#L522) | `"The pattern of blocks to set"` |
| [mod.rs:526](../crates/core/src/plot/worldedit/mod.rs#L526) | `"Sets all the blocks in the region"` |
| [mod.rs:532](../crates/core/src/plot/worldedit/mod.rs#L532) | `"The mask representng blocks to replace"` |
| [mod.rs:533](../crates/core/src/plot/worldedit/mod.rs#L533) | `"The pattern of blocks to replace with"` |
| [mod.rs:537](../crates/core/src/plot/worldedit/mod.rs#L537) | `"Replace all blocks in a selection with another"` |
| [mod.rs:544](../crates/core/src/plot/worldedit/mod.rs#L544) | `"Copy the selection to the clipboard"` |
| [mod.rs:552](../crates/core/src/plot/worldedit/mod.rs#L552) | `"Cut the selection to the clipboard"` |
| [mod.rs:559](../crates/core/src/plot/worldedit/mod.rs#L559) | `"Paste the clipboard's contents"` |
| [mod.rs:561](../crates/core/src/plot/worldedit/mod.rs#L561) | `"Skip air blocks"` |
| [mod.rs:562](../crates/core/src/plot/worldedit/mod.rs#L562) | `"Also update all affected blocks"` |
| [mod.rs:563](../crates/core/src/plot/worldedit/mod.rs#L563) | `"Select the pasted region"` |
| [mod.rs:570](../crates/core/src/plot/worldedit/mod.rs#L570) | `"Undoes the last action (from history)"` |
| [mod.rs:576](../crates/core/src/plot/worldedit/mod.rs#L576) | `"Redoes the last action (from history)"` |
| [mod.rs:582](../crates/core/src/plot/worldedit/mod.rs#L582) | `"# of copies to stack"` |
| [mod.rs:583](../crates/core/src/plot/worldedit/mod.rs#L583) | `"The direction to stack"` |
| [mod.rs:587](../crates/core/src/plot/worldedit/mod.rs#L587) | `"Repeat the contents of the selection"` |
| [mod.rs:589](../crates/core/src/plot/worldedit/mod.rs#L589) | `"Ignore air blocks"` |
| [mod.rs:596](../crates/core/src/plot/worldedit/mod.rs#L596) | `"The distance to move"` |
| [mod.rs:597](../crates/core/src/plot/worldedit/mod.rs#L597) | `"The direction to move"` |
| [mod.rs:601](../crates/core/src/plot/worldedit/mod.rs#L601) | `"Move the contents of the selection"` |
| [mod.rs:603](../crates/core/src/plot/worldedit/mod.rs#L603) | `"Ignore air blocks"` |
| [mod.rs:604](../crates/core/src/plot/worldedit/mod.rs#L604) | `"Shift the selection to the target location"` |
| [mod.rs:611](../crates/core/src/plot/worldedit/mod.rs#L611) | `"The mask of blocks to match"` |
| [mod.rs:615](../crates/core/src/plot/worldedit/mod.rs#L615) | `"Counts the number of blocks matching a mask"` |
| [mod.rs:622](../crates/core/src/plot/worldedit/mod.rs#L622) | `"The file name of the schematic to load"` |
| [mod.rs:625](../crates/core/src/plot/worldedit/mod.rs#L625) | `"Loads a schematic file into the clipboard"` |
| [mod.rs:632](../crates/core/src/plot/worldedit/mod.rs#L632) | `"The file name of the schematic to save"` |
| [mod.rs:636](../crates/core/src/plot/worldedit/mod.rs#L636) | `"Save a schematic file from the clipboard"` |
| [mod.rs:643](../crates/core/src/plot/worldedit/mod.rs#L643) | `"Amount to expand the selection by"` |
| [mod.rs:644](../crates/core/src/plot/worldedit/mod.rs#L644) | `"Direction to expand"` |
| [mod.rs:648](../crates/core/src/plot/worldedit/mod.rs#L648) | `"Expand the selection area"` |
| [mod.rs:655](../crates/core/src/plot/worldedit/mod.rs#L655) | `"Amount to contract the selection by"` |
| [mod.rs:656](../crates/core/src/plot/worldedit/mod.rs#L656) | `"Direction to contract"` |
| [mod.rs:660](../crates/core/src/plot/worldedit/mod.rs#L660) | `"Contract the selection area"` |
| [mod.rs:667](../crates/core/src/plot/worldedit/mod.rs#L667) | `"Amount to shift the selection by"` |
| [mod.rs:668](../crates/core/src/plot/worldedit/mod.rs#L668) | `"Direction to shift"` |
| [mod.rs:672](../crates/core/src/plot/worldedit/mod.rs#L672) | `"Shift the selection area"` |
| [mod.rs:679](../crates/core/src/plot/worldedit/mod.rs#L679) | `"The direction to flip, defaults to look direction"` |
| [mod.rs:683](../crates/core/src/plot/worldedit/mod.rs#L683) | `"Flip the contents of the clipboard across the origin"` |
| [mod.rs:689](../crates/core/src/plot/worldedit/mod.rs#L689) | `"Amount to rotate on the x-axis"` |
| [mod.rs:693](../crates/core/src/plot/worldedit/mod.rs#L693) | `"Rotate the contents of the clipboard"` |
| [mod.rs:699](../crates/core/src/plot/worldedit/mod.rs#L699) | `"# of copies to stack"` |
| [mod.rs:700](../crates/core/src/plot/worldedit/mod.rs#L700) | `"The spacing between each selection"` |
| [mod.rs:701](../crates/core/src/plot/worldedit/mod.rs#L701) | `"The direction to stack"` |
| [mod.rs:705](../crates/core/src/plot/worldedit/mod.rs#L705) | `"Include air blocks"` |
| [mod.rs:706](../crates/core/src/plot/worldedit/mod.rs#L706) | `"Expand selection"` |
| [mod.rs:709](../crates/core/src/plot/worldedit/mod.rs#L709) | `"Like //stack but allows the stacked copies to overlap, supports more directions, and more flags"` |
| [mod.rs:715](../crates/core/src/plot/worldedit/mod.rs#L715) | `"Updates all blocks in the selection"` |
| [mod.rs:719](../crates/core/src/plot/worldedit/mod.rs#L719) | `"Update the entire plot"` |
| [mod.rs:725](../crates/core/src/plot/worldedit/mod.rs#L725) | `"Command to retrieve help for"` |
| [mod.rs:728](../crates/core/src/plot/worldedit/mod.rs#L728) | `"Displays help for WorldEdit commands"` |
| [mod.rs:735](../crates/core/src/plot/worldedit/mod.rs#L735) | `"Gives a WorldEdit wand"` |
| [mod.rs:742](../crates/core/src/plot/worldedit/mod.rs#L742) | `"The container type to replace"` |
| [mod.rs:743](../crates/core/src/plot/worldedit/mod.rs#L743) | `"The container type to replace with"` |
| [mod.rs:746](../crates/core/src/plot/worldedit/mod.rs#L746) | `"Replaces all container types in the selection"` |
| [mod.rs:811](../crates/core/src/plot/worldedit/mod.rs#L811) | `"unknown block: {}"` |
| [mod.rs:812](../crates/core/src/plot/worldedit/mod.rs#L812) | `"invalid pattern: {}"` |

#### crates/core/src/plot/worldedit/execute.rs

| Location | Current literal or template |
| --- | --- |
| [execute.rs:286](../crates/core/src/plot/worldedit/execute.rs#L286) | `"The specified schematic file could not be found."` |
| [execute.rs:750](../crates/core/src/plot/worldedit/execute.rs#L750) | `"Unknown command: {}"` |
| [execute.rs:760](../crates/core/src/plot/worldedit/execute.rs#L760) | `" Help for /{} "` |
| [execute.rs:768](../crates/core/src/plot/worldedit/execute.rs#L768) | `"\nUsage: "` |
| [execute.rs:778](../crates/core/src/plot/worldedit/execute.rs#L778) | `" ["` |
| [execute.rs:798](../crates/core/src/plot/worldedit/execute.rs#L798) | `"\n  ["` |
| [execute.rs:823](../crates/core/src/plot/worldedit/execute.rs#L823) | `" (defaults to {})"` |
| [execute.rs:830](../crates/core/src/plot/worldedit/execute.rs#L830) | `": {}"` |
| [execute.rs:845](../crates/core/src/plot/worldedit/execute.rs#L845) | `"\n  -{}"` |
| [execute.rs:848](../crates/core/src/plot/worldedit/execute.rs#L848) | `": {}"` |

### Schematic diagnostics

Load and save send `Could not load schematic: {root_cause}` or `Could not save schematic: {root_cause}`. File-not-found receives its own fixed reply. Local validators, schematic parsing, and imported block entities can supply the root cause. Some context strings below appear only in detailed logs when a deeper error is present; they are listed to distinguish diagnostics from the notice wrapper. OS, gzip, NBT, and serialization errors remain dynamic diagnostics.

#### crates/core/src/plot/worldedit/schematic_paths.rs

| Location | Error or context producer |
| --- | --- |
| [schematic_paths.rs:8](../crates/core/src/plot/worldedit/schematic_paths.rs#L8) | `ensure!(!name.starts_with('/') && !name.ends_with('/'), "Use a relative schematic filename.")` |
| [schematic_paths.rs:17](../crates/core/src/plot/worldedit/schematic_paths.rs#L17) | `ensure!(part != ".." && !part.ends_with(['.', ' ']) && !part .chars() .any(\|c\| c.is_control() \|\| ":*?\"<>\|".contains(c)), "Invalid schematic path.")` |
| [schematic_paths.rs:28](../crates/core/src/plot/worldedit/schematic_paths.rs#L28) | `ensure!(extension.eq_ignore_ascii_case("schem") \|\| extension.eq_ignore_ascii_case("schematic"), "Include a .schem or .schematic extension.")` |
| [schematic_paths.rs:45](../crates/core/src/plot/worldedit/schematic_paths.rs#L45) | `ensure!(resolved.starts_with(root), "Schematic path leaves the schematic folder.")` |
| [schematic_paths.rs:84](../crates/core/src/plot/worldedit/schematic_paths.rs#L84) | `bail!("More than one schematic named {name}. Use a subfolder path, such as {}.", matches[0].strip_prefix(&root)?.display())` |
| [schematic_paths.rs:93](../crates/core/src/plot/worldedit/schematic_paths.rs#L93) | `ensure!(!in_rf(&relative), "The rf schematic folder is read-only. Save outside rf.")` |
| [schematic_paths.rs:109](../crates/core/src/plot/worldedit/schematic_paths.rs#L109) | `.context("Invalid schematic path")` |
| [schematic_paths.rs:112](../crates/core/src/plot/worldedit/schematic_paths.rs#L112) | `ensure!(ancestor.pop(), "Invalid schematic path")` |
| [schematic_paths.rs:121](../crates/core/src/plot/worldedit/schematic_paths.rs#L121) | `ensure!(!in_rf(resolved.strip_prefix(&root)?), "The rf schematic folder is read-only. Save outside rf.")` |
| [schematic_paths.rs:127](../crates/core/src/plot/worldedit/schematic_paths.rs#L127) | `ensure!(!resolved.starts_with(shared), "The rf schematic folder is read-only. Save outside rf.")` |

#### crates/core/src/plot/worldedit/schematic.rs

| Location | Error or context producer |
| --- | --- |
| [schematic.rs:79](../crates/core/src/plot/worldedit/schematic.rs#L79) | `bail!("{key}: expected Int")` |
| [schematic.rs:85](../crates/core/src/plot/worldedit/schematic.rs#L85) | `bail!("{key}: expected Compound")` |
| [schematic.rs:91](../crates/core/src/plot/worldedit/schematic.rs#L91) | `bail!("{key}: expected IntArray of exactly three entries")` |
| [schematic.rs:114](../crates/core/src/plot/worldedit/schematic.rs#L114) | `bail!("Version: unsupported schematic version {version}")` |
| [schematic.rs:117](../crates/core/src/plot/worldedit/schematic.rs#L117) | `bail!("Schematic: v3 requires a nested schema Compound")` |
| [schematic.rs:124](../crates/core/src/plot/worldedit/schematic.rs#L124) | `bail!("{key}: expected unsigned NBT Short")` |
| [schematic.rs:127](../crates/core/src/plot/worldedit/schematic.rs#L127) | `bail!("{key}: zero dimension")` |
| [schematic.rs:133](../crates/core/src/plot/worldedit/schematic.rs#L133) | `bail!("Metadata: expected Compound")` |
| [schematic.rs:153](../crates/core/src/plot/worldedit/schematic.rs#L153) | `.context("Offset: displacement overflows clipboard coordinates")` |
| [schematic.rs:157](../crates/core/src/plot/worldedit/schematic.rs#L157) | `.context("Schematic.Blocks: this importer requires block content")` |
| [schematic.rs:165](../crates/core/src/plot/worldedit/schematic.rs#L165) | `bail!("{field}: expected ByteArray")` |
| [schematic.rs:170](../crates/core/src/plot/worldedit/schematic.rs#L170) | `bail!("BlockEntities: expected List")` |
| [schematic.rs:197](../crates/core/src/plot/worldedit/schematic.rs#L197) | `.context("reading gzip schematic NBT")` |
| [schematic.rs:198](../crates/core/src/plot/worldedit/schematic.rs#L198) | `.context("schematic schema")` |
| [schematic.rs:204](../crates/core/src/plot/worldedit/schematic.rs#L204) | `.with_context(\|\| { format!( "Sponge v{} DataVersion {}", schema.version, schema.data_version ) })` |
| [schematic.rs:218](../crates/core/src/plot/worldedit/schematic.rs#L218) | `.context("dimensions: schematic exceeds 16,777,216 block limit")` |
| [schematic.rs:220](../crates/core/src/plot/worldedit/schematic.rs#L220) | `bail!("block data: fewer bytes than required block entries")` |
| [schematic.rs:225](../crates/core/src/plot/worldedit/schematic.rs#L225) | `bail!("Palette.{name}: expected Int")` |
| [schematic.rs:228](../crates/core/src/plot/worldedit/schematic.rs#L228) | `bail!("Palette.{name}: negative or duplicate index {id}")` |
| [schematic.rs:231](../crates/core/src/plot/worldedit/schematic.rs#L231) | `.with_context(\|\| format!("Palette: unsupported or invalid block state {name}"))` |
| [schematic.rs:243](../crates/core/src/plot/worldedit/schematic.rs#L243) | `.with_context(\|\| format!("block data: truncated VarInt at entry {i}"))` |
| [schematic.rs:247](../crates/core/src/plot/worldedit/schematic.rs#L247) | `bail!("block data: overflowing palette VarInt at entry {i}")` |
| [schematic.rs:256](../crates/core/src/plot/worldedit/schematic.rs#L256) | `bail!("block data: unterminated palette VarInt at entry {i}")` |
| [schematic.rs:262](../crates/core/src/plot/worldedit/schematic.rs#L262) | `.with_context(\|\| format!("block data: missing palette index {id} at entry {i}"))` |
| [schematic.rs:266](../crates/core/src/plot/worldedit/schematic.rs#L266) | `bail!("block data: trailing bytes after {entries} entries")` |
| [schematic.rs:272](../crates/core/src/plot/worldedit/schematic.rs#L272) | `bail!("BlockEntities[{index}]: expected Compound")` |
| [schematic.rs:275](../crates/core/src/plot/worldedit/schematic.rs#L275) | `.with_context(\|\| format!("BlockEntities[{index}].Pos"))` |
| [schematic.rs:281](../crates/core/src/plot/worldedit/schematic.rs#L281) | `bail!("BlockEntities[{index}].Pos: {pos} outside schematic")` |
| [schematic.rs:285](../crates/core/src/plot/worldedit/schematic.rs#L285) | `bail!("BlockEntities[{index}].Id: expected String")` |
| [schematic.rs:291](../crates/core/src/plot/worldedit/schematic.rs#L291) | `bail!("BlockEntities[{index}].Data: expected Compound")` |
| [schematic.rs:330](../crates/core/src/plot/worldedit/schematic.rs#L330) | `.with_context(\|\| format!("BlockEntities[{index}] {id} at {pos}"))` |
| [schematic.rs:332](../crates/core/src/plot/worldedit/schematic.rs#L332) | `bail!("BlockEntities[{index}]: duplicate position {pos}")` |
| [schematic.rs:371](../crates/core/src/plot/worldedit/schematic.rs#L371) | `.context("offset X overflow")` |
| [schematic.rs:375](../crates/core/src/plot/worldedit/schematic.rs#L375) | `.context("offset Y overflow")` |
| [schematic.rs:379](../crates/core/src/plot/worldedit/schematic.rs#L379) | `.context("offset Z overflow")` |
| [schematic.rs:384](../crates/core/src/plot/worldedit/schematic.rs#L384) | `.context("clipboard volume overflow")` |
| [schematic.rs:391](../crates/core/src/plot/worldedit/schematic.rs#L391) | `bail!("invalid clipboard geometry")` |
| [schematic.rs:405](../crates/core/src/plot/worldedit/schematic.rs#L405) | `.context("invalid clipboard block state")` |

#### crates/blocks/src/block_entities.rs

| Location | Error or context producer |
| --- | --- |
| [block_entities.rs:189](../crates/blocks/src/block_entities.rs#L189) | `bail!("{key}: expected String")` |
| [block_entities.rs:197](../crates/blocks/src/block_entities.rs#L197) | `bail!("{key}: expected boolean Byte")` |
| [block_entities.rs:202](../crates/blocks/src/block_entities.rs#L202) | `bail!("Command: too long")` |
| [block_entities.rs:207](../crates/blocks/src/block_entities.rs#L207) | `bail!("SuccessCount: expected nonnegative Int")` |
| [block_entities.rs:212](../crates/blocks/src/block_entities.rs#L212) | `bail!("LastExecution: expected Long")` |
| [block_entities.rs:220](../crates/blocks/src/block_entities.rs#L220) | `bail!("{key}: expected text component")` |
| [block_entities.rs:305](../crates/blocks/src/block_entities.rs#L305) | `bail!("invalid container item count")` |
| [block_entities.rs:309](../crates/blocks/src/block_entities.rs#L309) | `bail!("modern persisted inventory components are not supported by this schematic importer")` |
| [block_entities.rs:314](../crates/blocks/src/block_entities.rs#L314) | `bail!("invalid container slot or count")` |
| [block_entities.rs:324](../crates/blocks/src/block_entities.rs#L324) | `anyhow!("Item compound id missing namespace")` |
| [block_entities.rs:351](../crates/blocks/src/block_entities.rs#L351) | `anyhow!("unknown item {namespaced_name}")` |
| [block_entities.rs:379](../crates/blocks/src/block_entities.rs#L379) | `bail!("OutputSignal: expected strength 0..15")` |
| [block_entities.rs:392](../crates/blocks/src/block_entities.rs#L392) | `bail!("Items: expected List")` |
| [block_entities.rs:416](../crates/blocks/src/block_entities.rs#L416) | `bail!("{side}.messages: expected List")` |
| [block_entities.rs:419](../crates/blocks/src/block_entities.rs#L419) | `bail!("{side}.messages: expected exactly four text components")` |
| [block_entities.rs:426](../crates/blocks/src/block_entities.rs#L426) | `bail!("{side}.messages: invalid text component")` |
| [block_entities.rs:434](../crates/blocks/src/block_entities.rs#L434) | `bail!("{side}.color: expected String")` |
| [block_entities.rs:439](../crates/blocks/src/block_entities.rs#L439) | `bail!("{side}.has_glowing_text: expected boolean Byte")` |
| [block_entities.rs:450](../crates/blocks/src/block_entities.rs#L450) | `bail!("Text{}: expected String", i + 1)` |
| [block_entities.rs:455](../crates/blocks/src/block_entities.rs#L455) | `bail!("{side}: expected Compound")` |
| [block_entities.rs:461](../crates/blocks/src/block_entities.rs#L461) | `bail!("is_waxed: expected boolean Byte")` |
| [block_entities.rs:475](../crates/blocks/src/block_entities.rs#L475) | `anyhow!("unknown carried block {name}")` |
| [block_entities.rs:495](../crates/blocks/src/block_entities.rs#L495) | `anyhow!("Unknown block face in moving piston block entity: {facing}")` |
| [block_entities.rs:509](../crates/blocks/src/block_entities.rs#L509) | `bail!("Invalid moving piston progress: {progress}")` |
| [block_entities.rs:524](../crates/blocks/src/block_entities.rs#L524) | `bail!("Unknown block entity id: {}", id)` |

### RedstoneTools notices and results

The working tree contains an in-progress `plot/redstone_tools` module. Its `ToolNotice::send` already renders fixed feline success wording and appends ` >.<` to error text; its trusted result-action builder includes a hover label. The inventory below reflects files present at scan time. Files being added by other work can introduce further messages after this snapshot.

#### crates/core/src/plot/redstone_tools/mod.rs

| Location | Current literal or template |
| --- | --- |
| [mod.rs:77](../crates/core/src/plot/redstone_tools/mod.rs#L77) | `"{error} >.<"` |
| [mod.rs:78](../crates/core/src/plot/redstone_tools/mod.rs#L78) | `"Autowire enabled, nya~"` |
| [mod.rs:79](../crates/core/src/plot/redstone_tools/mod.rs#L79) | `"Autowire disabled, nya~"` |
| [mod.rs:80](../crates/core/src/plot/redstone_tools/mod.rs#L80) | `"Selection sidebar enabled, nya~"` |
| [mod.rs:81](../crates/core/src/plot/redstone_tools/mod.rs#L81) | `"Selection sidebar hidden, nya~"` |
| [mod.rs:82](../crates/core/src/plot/redstone_tools/mod.rs#L82) | `"Your item is ready, nya~"` |
| [mod.rs:83](../crates/core/src/plot/redstone_tools/mod.rs#L83) | `"Stacked {count} copies, nya~"` |
| [mod.rs:98](../crates/core/src/plot/redstone_tools/mod.rs#L98) | `"/tp {} {} {}"` |
| [mod.rs:99](../crates/core/src/plot/redstone_tools/mod.rs#L99) | `"{} -p {page}"` |
| [mod.rs:105](../crates/core/src/plot/redstone_tools/mod.rs#L105) | `"Click, nya~"` |
| [mod.rs:118](../crates/core/src/plot/redstone_tools/mod.rs#L118) | `"Select position 1 first"` |
| [mod.rs:119](../crates/core/src/plot/redstone_tools/mod.rs#L119) | `"Select position 2 first"` |
| [mod.rs:125](../crates/core/src/plot/redstone_tools/mod.rs#L125) | `"The complete selection must be inside this plot and world height"` |
| [mod.rs:166](../crates/core/src/plot/redstone_tools/mod.rs#L166) | `"You don't have permission to use this command"` |
| [mod.rs:172](../crates/core/src/plot/redstone_tools/mod.rs#L172) | `"You can only use WorldEdit on your own plot"` |
| [mod.rs:186](../crates/core/src/plot/redstone_tools/mod.rs#L186) | `"Usage: /autowire (or /aw)"` |

#### crates/core/src/plot/redstone_tools/search.rs

| Location | Current literal or template |
| --- | --- |
| [search.rs:91](../crates/core/src/plot/redstone_tools/search.rs#L91) | `"Page must be between 1 and {}"` |
| [search.rs:101](../crates/core/src/plot/redstone_tools/search.rs#L101) | `"No matches found, nya~"` |
| [search.rs:108](../crates/core/src/plot/redstone_tools/search.rs#L108) | `" (result limit reached)"` |
| [search.rs:112](../crates/core/src/plot/redstone_tools/search.rs#L112) | `"{} matches{suffix}; page {page}/{}, nya~"` |
| [search.rs:118](../crates/core/src/plot/redstone_tools/search.rs#L118) | `"({}, {}, {})"` |
| [search.rs:121](../crates/core/src/plot/redstone_tools/search.rs#L121) | `"\n  {} {}: "` |
| [search.rs:129](../crates/core/src/plot/redstone_tools/search.rs#L129) | `"[Previous] "` |
| [search.rs:165](../crates/core/src/plot/redstone_tools/search.rs#L165) | `"Unbalanced mask properties"` |
| [search.rs:177](../crates/core/src/plot/redstone_tools/search.rs#L177) | `"Unknown block state ID: {id}"` |
| [search.rs:184](../crates/core/src/plot/redstone_tools/search.rs#L184) | `"Mask properties must end with ]"` |
| [search.rs:192](../crates/core/src/plot/redstone_tools/search.rs#L192) | `"Unknown block: {name}"` |
| [search.rs:196](../crates/core/src/plot/redstone_tools/search.rs#L196) | `"Use property=value in masks"` |
| [search.rs:198](../crates/core/src/plot/redstone_tools/search.rs#L198) | `"Duplicate mask property: {key}"` |
| [search.rs:201](../crates/core/src/plot/redstone_tools/search.rs#L201) | `"Unknown property or value: {key}={value}"` |
| [search.rs:226](../crates/core/src/plot/redstone_tools/search.rs#L226) | `"Usage: //find <mask> or //find -p <page>"` |
| [search.rs:243](../crates/core/src/plot/redstone_tools/search.rs#L243) | `"Usage: //signsearch <regex> or //signsearch -p <page>"` |
| [search.rs:251](../crates/core/src/plot/redstone_tools/search.rs#L251) | `"Invalid regular expression"` |
| [search.rs:282](../crates/core/src/plot/redstone_tools/search.rs#L282) | `"Usage: {} -p <page>"` |
| [search.rs:284](../crates/core/src/plot/redstone_tools/search.rs#L284) | `"Page must be a positive integer"` |
| [search.rs:291](../crates/core/src/plot/redstone_tools/search.rs#L291) | `"Run a search first"` |
| [search.rs:293](../crates/core/src/plot/redstone_tools/search.rs#L293) | `"These search results belong to another plot; run a new search"` |
| [search.rs:296](../crates/core/src/plot/redstone_tools/search.rs#L296) | `"Page numbers start at 1"` |
| [search.rs:304](../crates/core/src/plot/redstone_tools/search.rs#L304) | `"Search selections may contain at most {MAX_SCAN_BLOCKS} blocks"` |
| [search.rs:337](../crates/core/src/plot/redstone_tools/search.rs#L337) | `"Queries may contain at most {MAX_QUERY_BYTES} bytes"` |

#### crates/core/src/plot/redstone_tools/selection.rs

| Location | Current literal or template |
| --- | --- |
| [selection.rs:14](../crates/core/src/plot/redstone_tools/selection.rs#L14) | `"Usage: /cursel"` |
| [selection.rs:47](../crates/core/src/plot/redstone_tools/selection.rs#L47) | `"Selection, nya~"` |
| [selection.rs:85](../crates/core/src/plot/redstone_tools/selection.rs#L85) | `"§7Select both positions"` |
| [selection.rs:97](../crates/core/src/plot/redstone_tools/selection.rs#L97) | `"§bSize: {width} × {height} × {depth}"` |
| [selection.rs:98](../crates/core/src/plot/redstone_tools/selection.rs#L98) | `"{color}Volume: {volume}"` |

#### crates/core/src/plot/redstone_tools/stack.rs

| Location | Current literal or template |
| --- | --- |
| [stack.rs:47](../crates/core/src/plot/redstone_tools/stack.rs#L47) | `"Unknown rstack flag: -{flag}"` |
| [stack.rs:53](../crates/core/src/plot/redstone_tools/stack.rs#L53) | `"Specify only one direction"` |
| [stack.rs:58](../crates/core/src/plot/redstone_tools/stack.rs#L58) | `"Usage: //rstack [direction] [count] [spacing] [-e] [-w]"` |
| [stack.rs:63](../crates/core/src/plot/redstone_tools/stack.rs#L63) | `"Spacing overflows when reversing direction"` |
| [stack.rs:67](../crates/core/src/plot/redstone_tools/stack.rs#L67) | `"Stack count may not exceed {MAX_COPIES}"` |
| [stack.rs:80](../crates/core/src/plot/redstone_tools/stack.rs#L80) | `"A stack operation may copy at most {MAX_STACK_BLOCKS} blocks"` |
| [stack.rs:118](../crates/core/src/plot/redstone_tools/stack.rs#L118) | `"nonempty validated destinations"` |
| [stack.rs:132](../crates/core/src/plot/redstone_tools/stack.rs#L132) | `"Stack X coordinate overflows"` |
| [stack.rs:133](../crates/core/src/plot/redstone_tools/stack.rs#L133) | `"Stack Y coordinate overflows"` |
| [stack.rs:134](../crates/core/src/plot/redstone_tools/stack.rs#L134) | `"Stack Z coordinate overflows"` |
| [stack.rs:191](../crates/core/src/plot/redstone_tools/stack.rs#L191) | `"Unknown direction: {token}"` |
| [stack.rs:195](../crates/core/src/plot/redstone_tools/stack.rs#L195) | `"Look horizontally before using a relative direction"` |

## Other visible text and preserved content

| Source | Surface | Treatment |
| --- | --- | --- |
| [interaction.rs:25](../crates/core/src/interaction.rs#L25) | Stick inspection reports coordinates, block debug data, redstone power by face, and block-entity debug data. | Preserve diagnostic payloads; a future notice renderer can change surrounding labels separately. |
| [redstone/command_block.rs:101](../crates/core/src/redstone/command_block.rs#L101) | `Command executed` and parser errors are stored as JSON `last_output`. | Command-block GUI feedback, not direct chat. Preserve authored commands and custom names. |
| [blocks/block_entities.rs:250](../crates/blocks/src/block_entities.rs#L250) | `last_output` is emitted as `LastOutput` NBT. | Reaches clients through block-entity packets and chunk data; any tone change belongs at its producer. |
| [player.rs:567](../crates/core/src/player.rs#L567) | Container titles use `container.barrel`, `container.furnace`, and `container.hopper` translations. | Keep Minecraft translation identifiers. |
| [plot/scoreboard.rs](../crates/core/src/plot/scoreboard.rs) | `Redpiler Status`, `Stopped`, `Compiling`, `Running`, `Flags:`, `- optimize`, `- export`, `- io only`, `- update`. | Separate scoreboard presentation; preserve option meaning. |
| [server.rs:757](../crates/core/src/server.rs#L757) | Server-list description uses `CONFIG.motd`. | Administrator-authored configuration; preserve. |
| [plot/packet_handlers.rs:58](../crates/core/src/plot/packet_handlers.rs#L58) | Schematic completion returns paths with `tooltip: None`. | Paths and command suggestions are syntax, not tone candidates. |
| [chat.rs](../crates/core/src/chat.rs) | Legacy formatting and detected URL `open_url` components. | Shared serialization; preserve player content and URLs. |
| [network/text.rs](../crates/network/src/text.rs), [network/packets/clientbound.rs:220](../crates/network/src/packets/clientbound.rs#L220) | JSON/NBT text conversion and chat packet encoding. | Transport rather than phrase ownership; keep protocol behavior. |
| [network/packets/components.rs](../crates/network/src/packets/components.rs), [blocks/items.rs](../crates/blocks/src/items.rs) | Item patches and display metadata. | Preserve authored item names, lore, and data; no PCMD item-message producer is present. |
| [blocks/block_entities.rs](../crates/blocks/src/block_entities.rs) | Sign lines and imported custom names. | Authored text delivered in world data; preserve. |

## Findings for the plan

- A global change to `send_raw_system_message`, `send_raw_chat`, or `send_chat_message` would also rewrite authored content. Convert notice producers selectively.
- The complete existing feedback scope includes general commands, server-thread whitelist and teleport replies, WorldEdit parsing and results, interactions, tick history, plot lifecycle, and login/disconnect surfaces.
- `/container` currently validates power `1..=15`; its existing error says greater than 0 and lower than 15 even though 15 is accepted. The plan example suggesting 0 through 15 is not a cosmetic replacement of this behavior. RedstoneTools has an in-progress replacement, so verify its validation before adopting wording.
- Native PCMD creation, sanitation-count feedback, placement, ownership/removal, and copy-to-clipboard messages have no existing send sites. They remain proposed features. The authored `/say` and `/tellraw` route and command-block output are the current integration points.
- Keep OS errors, parse details, paths, numbers, coordinates, permission tokens, and command syntax exact when moving wrappers into typed notices. Logging calls are not player feedback unless the same text is explicitly sent.

## Rechecking coverage

The inventory was generated from a repository-wide Rust search and balanced extraction of complete calls. The supporting producer tables follow the variable/error paths reviewed above. No compilation or runtime tests were needed for this documentation-only audit. Source line numbers describe this working-tree snapshot and will move as code changes. The RedstoneTools module was being added during the audit; refresh these tables after that work settles.

```powershell
rg -n --glob '*.rs' 'send_error_message|send_system_message|send_worldedit_message|send_no_permission_message|send_color_message|send_raw_system_message|send_raw_chat|send_chat_message|broadcast_chat_message|broadcast_plot_chat_message|\.kick\(' .
rg -n --glob '*.rs' 'CChatMessage\s*\{|CDisconnectLogin\s*\{|CDisconnect\s*\{|last_output|ChatComponentBuilder::new|window_title|objective_value|tooltip|CONFIG\.motd' .
```
