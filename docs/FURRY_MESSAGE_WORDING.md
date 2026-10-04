# Furry message wording proposal

This is a presentation-only proposal for the existing messages in [the inventory](FURRY_MESSAGE_INVENTORY.md), following [the style plan](FURRY_MESSAGES_PLAN.md). These are proposed English replacements, not runtime changes. Repeated messages share one proposed phrase; the inventory retains every send site.

The character lives in the verbs and imagery: fetching selections, following tick trails, pouncing to coordinates, and clipping plot leashes. Avoid a stock suffix, a universal prefix, or a mascot name attached to every reply. Emoticons are occasional accents, not mandatory punctuation.

Use named notice payloads for the values shown here. Keep the original colors and recipients. Render each notice once; when a typed error already has cute wording, do not also decorate it with the generic error wrapper. Technical diagnostics and authored content remain exact.

## Proposed feedback

| Source | Current message or template | Proposed wording | Review |
| --- | --- | --- | --- |
| [player.rs:373](../crates/core/src/player.rs#L373) | `Timed out.` | `Your connection wandered off. Timed out.` |  |
| [player.rs:441](../crates/core/src/player.rs#L441) | `We just saved you from a game crash, don't try it again!` | `Invalid teleport coordinates. Choose finite coordinates, little floof :3` | ok |
| [player.rs:479](../crates/core/src/player.rs#L479) | `You do not have permission to perform this action.` | `These paws don't have permission to perform that action.` |  |
| [player.rs:488](../crates/core/src/player.rs#L488) | `First position set to ({}, {}, {})` | `First paw set to ({}, {}, {}) :3` | ok |
| [player.rs:497](../crates/core/src/player.rs#L497) | `Second position set to ({}, {}, {})` | `Second paw set to ({}, {}, {}) :3` | ok |
| [commands.rs:51](../crates/core/src/plot/commands.rs#L51) | `Invalid argument for /plot` | `/plot doesn't know that trick. Peek at /help plots.` |  |
| [commands.rs:63](../crates/core/src/plot/commands.rs#L63) | `Plot owner is: {}` | `{} has their pawprint on this plot.` |  |
| [commands.rs:68](../crates/core/src/plot/commands.rs#L68) | `Plot is not owned by anyone.` | `No paws on this plot yet` | ok |
| [commands.rs:73](../crates/core/src/plot/commands.rs#L73) | `Plot is already claimed!` | `Someone has already claimed this patch of turf.` |  |
| [commands.rs:95](../crates/core/src/plot/commands.rs#L95) | `Invalid number of arguments!` | `That command needs a different handful of arguments. Check its syntax.` |  |
| [commands.rs:103](../crates/core/src/plot/commands.rs#L103) | `Plot index starts at 1` | `The first pawprint is plot 1; plot indexes start there.` |  |
| [commands.rs:107](../crates/core/src/plot/commands.rs#L107) | `Unable to parse index` | `That plot index has me puzzled. Give me a positive integer.` |  |
| [commands.rs:122](../crates/core/src/plot/commands.rs#L122) | `Plot range (1, {}).` | `Your plot trail runs from index 1 through {}.` |  |
| [commands.rs:126](../crates/core/src/plot/commands.rs#L126) | `{} does not own any plots.` | `No plots bear {}'s pawprint yet.` |  |
| [commands.rs:140](../crates/core/src/plot/commands.rs#L140) | `Unable to parse x coordinate!` | `That x coordinate has my ears tilted. Check the number or relative coordinate.` |  |
| [commands.rs:146](../crates/core/src/plot/commands.rs#L146) | `Unable to parse z coordinate!` | `That z coordinate lost the trail. Check the number or relative coordinate.` |  |
| [commands.rs:160](../crates/core/src/plot/commands.rs#L160) | `You are already locked to this plot.` | `You're already leashed to this plot.` |  |
| [commands.rs:170](../crates/core/src/plot/commands.rs#L170) | `You are now unlocked.` | `Off the plot leash! You're free to roam again ^^` |  |
| [commands.rs:172](../crates/core/src/plot/commands.rs#L172) | `You are not locked to this plot.` | `No plot leash to unclip; you're already free to roam.` |  |
| [commands.rs:208](../crates/core/src/plot/commands.rs#L208) | `Trace failed` | `My snoot couldn't find a block in sight. Aim at one and try again.` |  |
| [commands.rs:216](../crates/core/src/plot/commands.rs#L216) | `Invalid argument for /redpiler` | `/redpiler hasn't learned that trick. Check /help redpiler.` |  |
| [commands.rs:257](../crates/core/src/plot/commands.rs#L257) | `Usage: /help [topic]` | `Looking for a pawbook? Use /help [topic].` |  |
| [commands.rs:261](../crates/core/src/plot/commands.rs#L261) | `Unknown help topic. Use /help for topics, or //help <command> for WorldEdit.` | `That topic isn't in my pawbook. Use /help for topics, or //help <command> for WorldEdit.` |  |
| [commands.rs:278](../crates/core/src/plot/commands.rs#L278) | `Usage: /piston_anim [auto\|on\|off]` | `Pick how the pistons wiggle: /piston_anim [auto\|on\|off].` |  |
| [commands.rs:284](../crates/core/src/plot/commands.rs#L284) | `Piston animation: {} (effective {})` | `Piston wiggles: {} (effective {}).` |  |
| [commands.rs:358](../crates/core/src/plot/commands.rs#L358) | `Usage: /whitelist [add \| remove] (username)` | `To change who's in the pack: /whitelist [add \| remove] (username).` |  |
| [commands.rs:366](../crates/core/src/plot/commands.rs#L366) | `&6RTPS from last 10s, 1m, 5m, 15m: &a{:.1}, {:.1}, {:.1}, {:.1} ({})` | `&6Circuit heartbeat over 10s, 1m, 5m, 15m (RTPS): &a{:.1}, {:.1}, {:.1}, {:.1} ({})` |  |
| [commands.rs:374](../crates/core/src/plot/commands.rs#L374) | `&6No timings data. &a({})` | `&6Haven't caught this circuit's heartbeat yet. No timings data. &a({})` |  |
| [commands.rs:391](../crates/core/src/plot/commands.rs#L391) | `Unable to parse rtps!` | `RTPS has me chasing my tail. Use a number or unlimited.` |  |
| [commands.rs:400](../crates/core/src/plot/commands.rs#L400) | `The rtps was successfully set.` | `The circuit's new tick pace is set. Awoo!` |  |
| [commands.rs:414](../crates/core/src/plot/commands.rs#L414) | `Please specify a number of ticks to advance.` | `How many game-tick pawsteps should I take? Specify a tick count.` |  |
| [commands.rs:421](../crates/core/src/plot/commands.rs#L421) | `Please specify a number of nano-ticks to advance.` | `Tiny steps need a count: specify how many nano-ticks to advance.` |  |
| [commands.rs:427](../crates/core/src/plot/commands.rs#L427) | `Unable to parse nano-ticks!` | `Unable to parse those nano-ticks!` |  |
| [commands.rs:431](../crates/core/src/plot/commands.rs#L431) | `Cannot advance nano-ticks while redpiler is active!` | `Redpiler has the leash; nano-ticks need the interpreter. Run /redpiler reset first.` |  |
| [commands.rs:437](../crates/core/src/plot/commands.rs#L437) | `Disable tick history before nano/pico advancement.` | `Tick history follows whole pawprints. Run /rhistory off before nano/pico advancement.` |  |
| [commands.rs:447](../crates/core/src/plot/commands.rs#L447) | `Please specify a number of pico-ticks to advance.` | `Give these tiny toe beans a pico-tick count to advance.` |  |
| [commands.rs:453](../crates/core/src/plot/commands.rs#L453) | `Unable to parse pico-ticks!` | `Those pico-ticks won't sit still as a number. Use a nonnegative integer.` |  |
| [commands.rs:457](../crates/core/src/plot/commands.rs#L457) | `Cannot advance pico-ticks while redpiler is active!` | `Can't tiptoe through pico-ticks with redpiler active. Run /redpiler reset first.` |  |
| [commands.rs:473](../crates/core/src/plot/commands.rs#L473) | `Unable to parse ticks!` | `That tick count isn't a pawstep I can follow. Use a nonnegative integer.` |  |
| [commands.rs:483](../crates/core/src/plot/commands.rs#L483) | `Plot has been advanced by {unit} ({:.00?})` | `Trotted the plot forward by {unit} ({:.00?}).` |  |
| [commands.rs:492](../crates/core/src/plot/commands.rs#L492) | `Automatic redpiler compilation has been enabled.` | `Redpiler can chase compilation automatically now.` |  |
| [commands.rs:495](../crates/core/src/plot/commands.rs#L495) | `Automatic redpiler compilation has been disabled.` | `Redpiler will wait for your paw before compiling.` |  |
| [commands.rs:513](../crates/core/src/plot/commands.rs#L513) | `Unable to parse y coordinate!` | `Can't follow that y coordinate. Check the number or relative coordinate.` |  |
| [commands.rs:523](../crates/core/src/plot/commands.rs#L523) | `Teleporting to ({}, {}, {})` | `Pouncing to ({}, {}, {})!` |  |
| [commands.rs:527](../crates/core/src/plot/commands.rs#L527) | `Teleporting to {}` | `Following {}'s scent—teleporting now.` |  |
| [commands.rs:536](../crates/core/src/plot/commands.rs#L536) | `Invalid number of arguments for teleport command!` | `That teleport trail is incomplete. Use /tp <player> or /tp <x> <y> <z>.` |  |
| [commands.rs:560](../crates/core/src/plot/commands.rs#L560) | `/speed <0-10>` | `Choose your flying zoomies with /speed <0-10>.` |  |
| [commands.rs:566](../crates/core/src/plot/commands.rs#L566) | `Silly child, you can't have a negative flyspeed!` | `Negative flying zoomies aren't a thing. Use /speed <0-10>.` |  |
| [commands.rs:570](../crates/core/src/plot/commands.rs#L570) | `For performance reasons player speed cannot be higher than 10.` | `Flying zoomies top out at 10 for performance reasons.` |  |
| [commands.rs:577](../crates/core/src/plot/commands.rs#L577) | `You can't set your speed to NaN or -NaN.` | `NaN and -NaN aren't flying speeds these paws can use. Choose a number from 0 through 10.` |  |
| [commands.rs:583](../crates/core/src/plot/commands.rs#L583) | `Set flying speed to {} for {}` | `Flying zoomies set to {} for {}!` |  |
| [commands.rs:588](../crates/core/src/plot/commands.rs#L588) | `Unable to parse speed value` | `Can't make a flying pace out of that value. Try /speed <0-10>.` |  |
| [commands.rs:603](../crates/core/src/plot/commands.rs#L603) | `Unknown gamemode` | `That's not a gamemode in my trick book. Choose creative or spectator.` |  |
| [commands.rs:611](../crates/core/src/plot/commands.rs#L611) | `World send rate: {} Hz (effective {} Hz)` | `Your view gets fresh pawprints at {} Hz (effective {} Hz world send rate).` |  |
| [commands.rs:619](../crates/core/src/plot/commands.rs#L619) | `Usage: /worldsendrate <hertz>` | `Set the world-update trot with /worldsendrate <hertz>.` |  |
| [commands.rs:624](../crates/core/src/plot/commands.rs#L624) | `Unable to parse send rate!` | `Can't count that world-update trot. Use a nonnegative integer in hertz.` |  |
| [commands.rs:629](../crates/core/src/plot/commands.rs#L629) | `The world send rate cannot go higher than 1000!` | `World updates can't sprint past 1000 Hz.` |  |
| [commands.rs:636](../crates/core/src/plot/commands.rs#L636) | `The world send rate was successfully set.` | `Your view's world-update trot is set.` |  |
| [commands.rs:640](../crates/core/src/plot/commands.rs#L640) | `The world is already cursed.` | `This world's fur is already standing on end—it's cursed >w<` |  |
| [commands.rs:644](../crates/core/src/plot/commands.rs#L644) | `The world has been cursed. Redpiler disabled (/bless to undo)` | `Rawr, the world is cursed! Redpiler disabled (/bless to undo).` |  |
| [commands.rs:653](../crates/core/src/plot/commands.rs#L653) | `The world has been blessed.` | `The world's fur lies flat again. Blessing complete ^^` |  |
| [commands.rs:656](../crates/core/src/plot/commands.rs#L656) | `The world is not cursed. (/curse to curse)` | `No curse ruffling this world's fur. (/curse to curse)` |  |
| [commands.rs:660](../crates/core/src/plot/commands.rs#L660) | `Command not found!` | `I haven't learned that command trick. Browse /help.` |  |
| [history.rs:424](../crates/core/src/plot/history.rs#L424) | `WorldEdit undo/redo cleared after tick rewind.` | `Tick rewind brushed away the WorldEdit undo/redo pawprints.` |  |
| [history.rs:449](../crates/core/src/plot/history.rs#L449) | `Plot rewound by {ticks} game ticks and paused. {} game ticks remain in history.` | `Backtracked {ticks} game ticks and parked the plot. {} game ticks of pawprints remain in history.` |  |
| [mod.rs:866](../crates/core/src/plot/mod.rs#L866) | `Entering plot ({}, {})` | `Padding into plot ({}, {}).` |  |
| [mod.rs:969](../crates/core/src/plot/mod.rs#L969) | `This plot contains pistons, observers or command blocks and runs with the interpreter to preserve their behavior.` | `The interpreter is keeping a careful paw on this plot's pistons, observers or command blocks to preserve their behavior.` |  |
| [mod.rs:977](../crates/core/src/plot/mod.rs#L977) | `Tick history disabled because compiled execution is starting. Released approximately {}.` | `Compiled execution is taking over, so the tick-history pawprints were packed away. Released approximately {}.` |  |
| [mod.rs:1097](../crates/core/src/plot/mod.rs#L1097) | `Claimed plot {},{}` | `Plot {},{} is your den now. Awoo!` |  |
| [mod.rs:1194](../crates/core/src/plot/mod.rs#L1194) | `Server closed` | `The server's curling up for now. See you next time!` |  |
| [packet_handlers.rs:135](../crates/core/src/plot/packet_handlers.rs#L135) | `Command block updated.` | `Your command block has learned its new trick.` |  |
| [packet_handlers.rs:344](../crates/core/src/plot/packet_handlers.rs#L344) | `Can't interact with blocks outside of plot` | `Your paws can't reach across this plot's interaction boundary.` |  |
| [packet_handlers.rs:623](../crates/core/src/plot/packet_handlers.rs#L623) | `Can't break blocks outside of plot` | `Digging paws have to stay inside this plot.` |  |
| [search.rs:100](../crates/core/src/plot/redstone_tools/search.rs#L100) | `No matches found, nya~` | `No matches—the sniff patrol came back empty-pawed >w<` |  |
| [search.rs:111](../crates/core/src/plot/redstone_tools/search.rs#L111) | `{} matches{suffix}; page {page}/{}, nya~` | `Sniffed out {} matches{suffix}. Page {page}/{}.` |  |
| [execute.rs:71](../crates/core/src/plot/worldedit/execute.rs#L71) | `Operation completed: {} block(s) affected ({:.00?})` | `Operation complete: {} block(s) affected ({:.00?}). pawjob done :3` | ok |
| [execute.rs:134](../crates/core/src/plot/worldedit/execute.rs#L134) | `Counted {} block(s) ({:.00?})` | `Sniffed and counted {} matching block(s) ({:.00?}).` |  |
| [execute.rs:153](../crates/core/src/plot/worldedit/execute.rs#L153) | `Your selection was copied. ({:.00?})` | `Selection copied to your clipboard ({:.00?}). Tucked under a paw :3` | oi |
| [execute.rs:172](../crates/core/src/plot/worldedit/execute.rs#L172) | `Your selection was cut. ({:.00?})` | `Selection cut to your clipboard ({:.00?}). Paws packed :3` | ok |
| [execute.rs:222](../crates/core/src/plot/worldedit/execute.rs#L222) | `Your selection was moved. ({:.00?})` | `Nudged your selection into its new spot ({:.00?}).` |  |
| [execute.rs:253](../crates/core/src/plot/worldedit/execute.rs#L253) | `Your clipboard was pasted. ({:.00?})` | `Unpacked your clipboard onto the plot ({:.00?}).` |  |
| [execute.rs:258](../crates/core/src/plot/worldedit/execute.rs#L258) | `Your clipboard is empty!` | `Can't paste from an empty clipboard—fetch a selection with //copy first.` |  |
| [execute.rs:278](../crates/core/src/plot/worldedit/execute.rs#L278) | `The schematic was loaded to your clipboard. Use //paste to place it. ({:.00?})` | `Fetched the schematic into your clipboard ({:.00?}). Give //paste a boop to place it.` |  |
| [execute.rs:311](../crates/core/src/plot/worldedit/execute.rs#L311) | `The schematic was saved sucessfuly. ({:.00?})` | `Tucked your schematic away successfully ({:.00?}).` |  |
| [execute.rs:358](../crates/core/src/plot/worldedit/execute.rs#L358) | `Your selection was stacked. ({:.00?})` | `Made a neat pile of your selection ({:.00?}). Stack complete.` |  |
| [execute.rs:367](../crates/core/src/plot/worldedit/execute.rs#L367) | `There is nothing left to undo.` | `Undo has no earlier pawprints left to follow.` |  |
| [execute.rs:373](../crates/core/src/plot/worldedit/execute.rs#L373) | `Cannot undo outside of your current plot.` | `Undo can't follow pawprints outside your current plot. Return to that plot first.` |  |
| [execute.rs:405](../crates/core/src/plot/worldedit/execute.rs#L405) | `There is nothing left to redo.` | `Redo has no later pawprints left to fetch.` |  |
| [execute.rs:411](../crates/core/src/plot/worldedit/execute.rs#L411) | `Cannot redo outside of your current plot.` | `Redo can't fetch changes from outside your current plot. Return to that plot first.` |  |
| [execute.rs:444](../crates/core/src/plot/worldedit/execute.rs#L444) | `Selection cleared.` | `Brushed away your selection marks.` |  |
| [execute.rs:468](../crates/core/src/plot/worldedit/execute.rs#L468) | `No block in sight!` | `No block under your snoot. Aim at one first.` |  |
| [execute.rs:497](../crates/core/src/plot/worldedit/execute.rs#L497) | `Region expanded {} block(s).` | `Your region stretched its paws by {} block(s).` |  |
| [execute.rs:511](../crates/core/src/plot/worldedit/execute.rs#L511) | `Region contracted {} block(s).` | `Your region tucked in by {} block(s).` |  |
| [execute.rs:543](../crates/core/src/plot/worldedit/execute.rs#L543) | `Region shifted {} block(s).` | `Scooted your region {} block(s) along.` |  |
| [execute.rs:621](../crates/core/src/plot/worldedit/execute.rs#L621) | `The clipboard copy has been flipped. ({:.00?})` | `Rolled your clipboard copy over ({:.00?}).` |  |
| [execute.rs:633](../crates/core/src/plot/worldedit/execute.rs#L633) | `Successfully rotated by 0! That took a lot of work.` | `Rotated by 0. The clipboard mastered “stay.”` |  |
| [execute.rs:641](../crates/core/src/plot/worldedit/execute.rs#L641) | `Rotate amount must be a multiple of 90.` | `These rotation paws turn in 90-degree steps. Choose a multiple of 90.` |  |
| [execute.rs:727](../crates/core/src/plot/worldedit/execute.rs#L727) | `The clipboard copy has been rotated. ({:.00?})` | `Gave your clipboard copy a twirl ({:.00?}).` |  |
| [execute.rs:750](../crates/core/src/plot/worldedit/execute.rs#L750) | `Unknown command: {}` | `{} isn't a command trick I know.` |  |
| [execute.rs:901](../crates/core/src/plot/worldedit/execute.rs#L901) | `No free spot above you found.` | `No room to perch above you. Can't ascend.` |  |
| [execute.rs:906](../crates/core/src/plot/worldedit/execute.rs#L906) | `Ascended {} levels.` | `Hopped up {} levels!` |  |
| [execute.rs:937](../crates/core/src/plot/worldedit/execute.rs#L937) | `No free spot below you found.` | `No landing room below your paws. Can't descend.` |  |
| [execute.rs:942](../crates/core/src/plot/worldedit/execute.rs#L942) | `Descended {} levels.` | `Padded down {} levels.` |  |
| [execute.rs:958](../crates/core/src/plot/worldedit/execute.rs#L958) | `Your selection is incomplete.` | `One pawprint's missing from your selection. Set both positions with //pos1 and //pos2.` |  |
| [execute.rs:965](../crates/core/src/plot/worldedit/execute.rs#L965) | `Your selection was updated sucessfully. ({:.00?})` | `Booped the blocks in your selection to update them ({:.00?}).` |  |
| [execute.rs:1035](../crates/core/src/plot/worldedit/execute.rs#L1035) | `Your selection was replaced sucessfully. ({:.00?})` | `Swapped your selection's blocks with a careful paw ({:.00?}).` |  |
| [mod.rs:72](../crates/core/src/plot/worldedit/mod.rs#L72) | `Make a region selection first.` | `Mark your patch with //pos1 and //pos2 before putting these paws to work.` |  |
| [mod.rs:78](../crates/core/src/plot/worldedit/mod.rs#L78) | `First position is outside plot bounds!` | `Your first selection paw landed outside this plot. Set it inside the bounds.` |  |
| [mod.rs:82](../crates/core/src/plot/worldedit/mod.rs#L82) | `Second position is outside plot bounds!` | `Your second selection paw wandered outside this plot. Bring it inside the bounds.` |  |
| [mod.rs:88](../crates/core/src/plot/worldedit/mod.rs#L88) | `Your clipboard is empty. Use //copy first.` | `Nothing in the clipboard for these paws to fetch. Use //copy first.` |  |
| [mod.rs:102](../crates/core/src/plot/worldedit/mod.rs#L102) | `Flag with argument must be last in grouping` | `A flag carrying an argument needs the tail end of its group.` |  |
| [mod.rs:108](../crates/core/src/plot/worldedit/mod.rs#L108) | `Unknown flag: {}` | `{} isn't a flag in this trick's pawbook.` |  |
| [mod.rs:114](../crates/core/src/plot/worldedit/mod.rs#L114) | `Flag requires an argument` | `This flag's missing its treat—give it an argument.` |  |
| [mod.rs:134](../crates/core/src/plot/worldedit/mod.rs#L134) | `Too many arguments.` | `Too many arguments crowded into this trick. Check its syntax.` |  |
| [server.rs:566](../crates/core/src/server.rs#L566) | `Their plot wasn't loaded. How did this happen??` | `Can't follow their trail: the target player's plot isn't loaded.` |  |
| [server.rs:580](../crates/core/src/server.rs#L580) | `Player not found!` | `No player on that scent trail. Check their name.` |  |
| [server.rs:603](../crates/core/src/server.rs#L603) | `Whitelist is not enabled!` | `The whitelist gate isn't on duty; whitelist is disabled.` |  |
| [server.rs:626](../crates/core/src/server.rs#L626) | `That player is not whitelisted on this server.` | `That player isn't in the whitelist pack, so there's no entry to remove.` |  |
| [chat_commands.rs:29](../crates/core/src/chat_commands.rs#L29) | `Text component nesting is too deep` | `This text-component burrow is too deep. Reduce the nesting.` |  |
| [chat_commands.rs:36](../crates/core/src/chat_commands.rs#L36) | `Expected a text string, object or array` | `These text paws need a string, object or array to work with.` |  |
| [chat_commands.rs:134](../crates/core/src/chat_commands.rs#L134) | `Unclosed selector options` | `Your selector's tail bracket wandered off. Close its options with ].` |  |
| [chat_commands.rs:143](../crates/core/src/chat_commands.rs#L143) | `Supported targets: @a, @s or a player name` | `These messages can find @a, @s or a player name—choose one of those scent trails.` |  |
| [chat_commands.rs:167](../crates/core/src/chat_commands.rs#L167) | `Usage: /tellraw <target> <JSON text>` | `Dress your message in JSON with /tellraw <target> <JSON text>.` |  |
| [chat_commands.rs:173](../crates/core/src/chat_commands.rs#L173) | `Usage: /say <message>` | `Bark to the server with /say <message>.` |  |
| [chat_commands.rs:174](../crates/core/src/chat_commands.rs#L174) | `Only /tellraw and /say are supported in command blocks` | `Command blocks have learned only two message tricks: /tellraw and /say.` |  |
| [history.rs:28](../crates/core/src/plot/history.rs#L28) | `More than {NORMAL_HISTORY_LIMIT} game ticks requires {UNLIMITED_HISTORY_PERMISSION} permission.` | `Keeping more than {NORMAL_HISTORY_LIMIT} game ticks of pawprints requires {UNLIMITED_HISTORY_PERMISSION} permission.` |  |
| [history.rs:99](../crates/core/src/plot/history.rs#L99) | `Tick history: {}. Available: {}/{} game ticks.\nUncompressed: {} \| Compressed: {} \| Server: {} / {}` | `Tick-history trail: {}. Pawprints available: {}/{} game ticks.\nUncompressed: {} \| Compressed: {} \| Server: {} / {}` |  |
| [history.rs:113](../crates/core/src/plot/history.rs#L113) | `History capacity must be between 1 and 2147483647 game ticks.` | `The history den holds 1 through 2147483647 game ticks. Choose a capacity in that range.` |  |
| [history.rs:176](../crates/core/src/plot/history.rs#L176) | `Tick history is disabled. Use /rhistory on [ticks].` | `No tick pawprints are being recorded. Start the trail with /rhistory on [ticks].` |  |
| [history.rs:179](../crates/core/src/plot/history.rs#L179) | `Specify a positive number of game ticks to rewind.` | `Give me a positive number of game ticks to follow backward along the trail.` |  |
| [history.rs:183](../crates/core/src/plot/history.rs#L183) | `Only {} game ticks are available to rewind.` | `The rewind trail runs back only {} game ticks.` |  |
| [history.rs:246](../crates/core/src/plot/history.rs#L246) | `Finish the partial tick with /radvance 1 before using tick history.` | `That tick still has a paw in the air. Finish it with /radvance 1 before using tick history.` |  |
| [history.rs:317](../crates/core/src/plot/history.rs#L317) | `You do not have permission to use this command.` | `You do not have permission to use this command. Paws off for now >w<` | ok |
| [history.rs:327](../crates/core/src/plot/history.rs#L327) | `You do not have permission to change this plot.` | `You do not have permission to change this plot. Keep your paws gentle >w<` | ok |
| [history.rs:348](../crates/core/src/plot/history.rs#L348) | `Server history: {} / {}` | `The server's pawprint store uses {} / {}.` |  |
| [history.rs:355](../crates/core/src/plot/history.rs#L355) | `Requires {MEMORY_PERMISSION} permission.` | `This memory-den adjustment needs {MEMORY_PERMISSION} permission.` |  |
| [history.rs:359](../crates/core/src/plot/history.rs#L359) | `Specify a nonnegative memory limit in MiB.` | `Give the history den a nonnegative memory allowance in MiB.` |  |
| [history.rs:365](../crates/core/src/plot/history.rs#L365) | `History limit saved: {}.` | `The history den's memory allowance is now {}.` |  |
| [history.rs:370](../crates/core/src/plot/history.rs#L370) | `History disabled. Released {}.` | `Stopped recording tick pawprints and freed {}.` |  |
| [history.rs:377](../crates/core/src/plot/history.rs#L377) | `Tick history is only available during interpreted execution.` | `Tick history needs the interpreter to track every pawprint. Run /redpiler reset first.` |  |
| [history.rs:382](../crates/core/src/plot/history.rs#L382) | `Specify a positive history capacity in game ticks.` | `How long should the pawprint trail be? Choose a positive capacity in game ticks.` |  |
| [history.rs:391](../crates/core/src/plot/history.rs#L391) | `History enabled: up to {capacity} game ticks. Estimated size: {}.` | `Recording up to {capacity} game ticks of pawprints. Estimated size: {}.` |  |
| [history.rs:395](../crates/core/src/plot/history.rs#L395) | `Usage: /rhistory [on [ticks]\|off\|status\|limit [MiB]]` | `Tend the tick-history trail with /rhistory [on [ticks]\|off\|status\|limit [MiB]].` |  |
| [history.rs:405](../crates/core/src/plot/history.rs#L405) | `Specify a positive number of game ticks.` | `Those game-tick pawsteps need a positive count.` |  |
| [history.rs:406](../crates/core/src/plot/history.rs#L406) | `Usage: /rback [ticks]` | `Follow the tick trail backward with /rback [ticks].` |  |
| [history.rs:409](../crates/core/src/plot/history.rs#L409) | `Tick rewind is only available during interpreted execution.` | `Rewind can follow only the interpreter's pawprints. Run /redpiler reset first.` |  |
| [items.rs:24](../crates/core/src/plot/redstone_tools/items.rs#L24) | `Power must be 0..15 or lowercase a..f` | `These spark paws take 0..15 or lowercase a..f as power.` |  |
| [items.rs:27](../crates/core/src/plot/redstone_tools/items.rs#L27) | `Power must be between 0 and 15` | `Keep that comparator spark between 0 and 15.` |  |
| [items.rs:36](../crates/core/src/plot/redstone_tools/items.rs#L36) | `Usage: /container <chest\|barrel\|hopper\|furnace> <0..15\|a..f>` | `Fetch a powered stash with /container <chest\|barrel\|hopper\|furnace> <0..15\|a..f>.` |  |
| [items.rs:58](../crates/core/src/plot/redstone_tools/items.rs#L58) | `Usage: /slab [slab_type]` | `Fetch a top slab with /slab [slab_type].` |  |
| [items.rs:77](../crates/core/src/plot/redstone_tools/items.rs#L77) | `Unknown slab type` | `Can't sniff out that slab type. Check its name.` |  |
| [items.rs:79](../crates/core/src/plot/redstone_tools/items.rs#L79) | `This item is not a slab` | `This item won't do the slab trick; choose a slab.` |  |
| [items.rs:117](../crates/core/src/plot/redstone_tools/items.rs#L117) | `Unknown container type; use chest, barrel, hopper or furnace` | `Can't fetch that container type. Choose chest, barrel, hopper or furnace.` |  |
| [items.rs:119](../crates/core/src/plot/redstone_tools/items.rs#L119) | `Ambiguous container type` | `That container name follows more than one scent. Use a longer or full name.` |  |
| [items.rs:126](../crates/core/src/plot/redstone_tools/items.rs#L126) | `Switch to creative mode first` | `These building paws need creative mode first.` |  |
| [items.rs:138](../crates/core/src/plot/redstone_tools/items.rs#L138) | `Your inventory is full; free a slot first` | `Your inventory's stuffed like a treat pouch. Free a slot first.` |  |
| [mod.rs:72](../crates/core/src/plot/redstone_tools/mod.rs#L72) | `{error} >.<` | `{error}` |  |
| [mod.rs:73](../crates/core/src/plot/redstone_tools/mod.rs#L73) | `Selection sidebar enabled, nya~` | `Your selection's pawprints are on the sidebar now.` |  |
| [mod.rs:74](../crates/core/src/plot/redstone_tools/mod.rs#L74) | `Selection sidebar hidden, nya~` | `Tucked your selection's sidebar out of sight.` |  |
| [mod.rs:75](../crates/core/src/plot/redstone_tools/mod.rs#L75) | `Your item is ready, nya~` | `Fetched your item!` |  |
| [mod.rs:76](../crates/core/src/plot/redstone_tools/mod.rs#L76) | `Stacked {count} copies, nya~` | `Piled up {count} copies. Rawr!` |  |
| [mod.rs:98](../crates/core/src/plot/redstone_tools/mod.rs#L98) | `Click, nya~` | `Boop to follow this trail.` |  |
| [mod.rs:111](../crates/core/src/plot/redstone_tools/mod.rs#L111) | `Select position 1 first` | `Plant the first selection paw with //pos1.` |  |
| [mod.rs:112](../crates/core/src/plot/redstone_tools/mod.rs#L112) | `Select position 2 first` | `Plant the second selection paw with //pos2.` |  |
| [mod.rs:118](../crates/core/src/plot/redstone_tools/mod.rs#L118) | `The complete selection must be inside this plot and world height` | `Your whole selection needs its paws inside this plot and within world height.` |  |
| [mod.rs:175](../crates/core/src/plot/redstone_tools/mod.rs#L175) | `You don't have permission to use this command` | `Your permissions don't include this command trick.` |  |
| [mod.rs:181](../crates/core/src/plot/redstone_tools/mod.rs#L181) | `You can only use WorldEdit on your own plot` | `WorldEdit paws may dig only in your own plot.` |  |
| [search.rs:91](../crates/core/src/plot/redstone_tools/search.rs#L91) | `Page must be between 1 and {}` | `This result trail has pages 1 through {}. Pick one of those.` |  |
| [search.rs:165](../crates/core/src/plot/redstone_tools/search.rs#L165) | `Unbalanced mask properties` | `Those mask-property brackets aren't paired. Give each paw its partner.` |  |
| [search.rs:177](../crates/core/src/plot/redstone_tools/search.rs#L177) | `Unknown block state ID: {id}` | `Block state ID {id} isn't in my sniff book.` |  |
| [search.rs:184](../crates/core/src/plot/redstone_tools/search.rs#L184) | `Mask properties must end with ]` | `The mask's property tail needs a closing ].` |  |
| [search.rs:192](../crates/core/src/plot/redstone_tools/search.rs#L192) | `Unknown block: {name}` | `Couldn't sniff out a block named {name}.` |  |
| [search.rs:196](../crates/core/src/plot/redstone_tools/search.rs#L196) | `Use property=value in masks` | `Pair each mask property's paws as property=value.` |  |
| [search.rs:198](../crates/core/src/plot/redstone_tools/search.rs#L198) | `Duplicate mask property: {key}` | `{key} left two pawprints in this mask. Specify that property once.` |  |
| [search.rs:201](../crates/core/src/plot/redstone_tools/search.rs#L201) | `Unknown property or value: {key}={value}` | `Can't match these block-state paws: {key}={value} is an unknown property or value.` |  |
| [search.rs:226](../crates/core/src/plot/redstone_tools/search.rs#L226) | `Usage: //find <mask> or //find -p <page>` | `Send the block sniff patrol with //find <mask>, or revisit //find -p <page>.` |  |
| [search.rs:243](../crates/core/src/plot/redstone_tools/search.rs#L243) | `Usage: //signsearch <regex> or //signsearch -p <page>` | `Put your nose to the signs with //signsearch <regex>, or revisit //signsearch -p <page>.` |  |
| [search.rs:251](../crates/core/src/plot/redstone_tools/search.rs#L251) | `Invalid regular expression` | `That regex tangled the scent trail. Check the expression.` |  |
| [search.rs:282](../crates/core/src/plot/redstone_tools/search.rs#L282) | `Usage: {} -p <page>` | `Follow a result-page trail with {} -p <page>.` |  |
| [search.rs:284](../crates/core/src/plot/redstone_tools/search.rs#L284) | `Page must be a positive integer` | `Result pages need a paw-sitive integer. Choose 1 or higher.` |  |
| [search.rs:291](../crates/core/src/plot/redstone_tools/search.rs#L291) | `Run a search first` | `Nothing to follow yet—send a search out to sniff first.` |  |
| [search.rs:293](../crates/core/src/plot/redstone_tools/search.rs#L293) | `These search results belong to another plot; run a new search` | `These pawprints lead to another plot. Run a fresh search here.` |  |
| [search.rs:296](../crates/core/src/plot/redstone_tools/search.rs#L296) | `Page numbers start at 1` | `The page trail starts at 1.` |  |
| [search.rs:304](../crates/core/src/plot/redstone_tools/search.rs#L304) | `Search selections may contain at most {MAX_SCAN_BLOCKS} blocks` | `The sniff patrol can cover at most {MAX_SCAN_BLOCKS} blocks per selection.` |  |
| [search.rs:337](../crates/core/src/plot/redstone_tools/search.rs#L337) | `Queries may contain at most {MAX_QUERY_BYTES} bytes` | `Your sniff request needs to fit within {MAX_QUERY_BYTES} bytes.` |  |
| [selection.rs:14](../crates/core/src/plot/redstone_tools/selection.rs#L14) | `Usage: /cursel` | `Peek at your selection's pawprints with /cursel.` |  |
| [selection.rs:47](../crates/core/src/plot/redstone_tools/selection.rs#L47) | `Selection, nya~` | `Selection pawprints` |  |
| [selection.rs:85](../crates/core/src/plot/redstone_tools/selection.rs#L85) | `§7Select both positions` | `§7Plant both paws: //pos1, //pos2` |  |
| [stack.rs:47](../crates/core/src/plot/redstone_tools/stack.rs#L47) | `Unknown rstack flag: -{flag}` | `-{flag} isn't a flag this stacking trick knows.` |  |
| [stack.rs:53](../crates/core/src/plot/redstone_tools/stack.rs#L53) | `Specify only one direction` | `These stacking paws need one direction, not a tug-of-war.` |  |
| [stack.rs:58](../crates/core/src/plot/redstone_tools/stack.rs#L58) | `Usage: //rstack [direction] [count] [spacing] [-e] [-w]` | `Line up your copies with //rstack [direction] [count] [spacing] [-e] [-w].` |  |
| [stack.rs:67](../crates/core/src/plot/redstone_tools/stack.rs#L67) | `Stack count may not exceed {MAX_COPIES}` | `This stacking trick can carry at most {MAX_COPIES} copies.` |  |
| [stack.rs:80](../crates/core/src/plot/redstone_tools/stack.rs#L80) | `A stack operation may copy at most {MAX_STACK_BLOCKS} blocks` | `One stacking trip can carry at most {MAX_STACK_BLOCKS} blocks.` |  |
| [stack.rs:197](../crates/core/src/plot/redstone_tools/stack.rs#L197) | `Unknown direction: {token}` | `{token} isn't a direction these paws recognize.` |  |
| [stack.rs:201](../crates/core/src/plot/redstone_tools/stack.rs#L201) | `Look horizontally before using a relative direction` | `Level your snoot before choosing a relative direction.` |  |
| [commands.rs:156](../crates/core/src/plot/commands.rs#L156) | `Locked to plot ({}, {}). Use '/p unlock' to unlock.` | `Leashed to plot ({}, {}). Use '/p unlock' when you're ready to roam.` |  |
| [execute.rs:286](../crates/core/src/plot/worldedit/execute.rs#L286) | `The specified schematic file could not be found.` | `Couldn't fetch that schematic file. Check its path and filename.` |  |
| [packet_handlers.rs:22](../crates/core/src/plot/packet_handlers.rs#L22) | ``This plot cannot be interacted with while redpiler is active with `--io-only`. To stop redpiler, run `/redpiler reset`.`` | ``Redpiler's `--io-only` mode has the interaction leash. Run `/redpiler reset` to put your paws back on this plot.`` |  |
| [server.rs:593](../crates/core/src/server.rs#L593) | `{} was sucessfully added to the whitelist.` | `{} has a spot in the whitelist pack now.` |  |
| [server.rs:613](../crates/core/src/server.rs#L613) | `{} was sucessfully removed from the whitelist.` | `Removed {}'s pawprint from the whitelist.` |  |

## Preserved messages and data

These retain their current wording. Serious failures need their actual reason; numerical and syntax fragments keep their exact values.

| Source | Preserved message or template |
| --- | --- |
| [mod.rs:1590](../crates/core/src/plot/mod.rs#L1590) | `The plot you were previously in has crashed!` |
| [execute.rs:294](../crates/core/src/plot/worldedit/execute.rs#L294) | `Could not load schematic: {}` |
| [execute.rs:320](../crates/core/src/plot/worldedit/execute.rs#L320) | `Could not save schematic: {}` |
| [server.rs:523](../crates/core/src/server.rs#L523) | `Could not load plot {},{}. Please contact the server administrator.` |
| [player.rs:321](../crates/core/src/player.rs#L321) | `Your player save could not be loaded. Ask the administrator to inspect the server log; your original file was preserved.` |
| [server.rs:327](../crates/core/src/server.rs#L327) | `You are not whitelisted on this server` |
| [server.rs:730](../crates/core/src/server.rs#L730) | `Version mismatch, I'm on {}!` |
| [server.rs:742](../crates/core/src/server.rs#L742) | `If you wish to use IP forwarding, please enable it in your BungeeCord config as well!` |
| [chat_commands.rs:162](../crates/core/src/chat_commands.rs#L162) | `[{source}] {args}` |
| [history.rs:117](../crates/core/src/plot/history.rs#L117) | `History size overflow.` |
| [history.rs:122](../crates/core/src/plot/history.rs#L122) | `Unable to allocate the history buffer.` |
| [history.rs:233](../crates/core/src/plot/history.rs#L233) | `One compressed snapshot cannot fit the history limit.` |
| [history.rs:238](../crates/core/src/plot/history.rs#L238) | `History memory estimate exceeds the supported size.` |
| [history.rs:263](../crates/core/src/plot/history.rs#L263) | `Tick history stopped: {error}` |
| [history.rs:298](../crates/core/src/plot/history.rs#L298) | `{:.2} GiB` |
| [history.rs:300](../crates/core/src/plot/history.rs#L300) | `{:.2} MiB` |
| [history.rs:302](../crates/core/src/plot/history.rs#L302) | `{:.2} KiB` |
| [history.rs:304](../crates/core/src/plot/history.rs#L304) | `{bytes} B` |
| [items.rs:45](../crates/core/src/plot/redstone_tools/items.rs#L45) | `{} · power {}` |
| [items.rs:46](../crates/core/src/plot/redstone_tools/items.rs#L46) | `Comparator signal: {} / 15` |
| [items.rs:47](../crates/core/src/plot/redstone_tools/items.rs#L47) | `Cannot prepare container components: {error:?}` |
| [items.rs:86](../crates/core/src/plot/redstone_tools/items.rs#L86) | `Top {name}` |
| [items.rs:87](../crates/core/src/plot/redstone_tools/items.rs#L87) | `Places top slabs; click an existing top slab to place beneath it.` |
| [items.rs:88](../crates/core/src/plot/redstone_tools/items.rs#L88) | `Cannot prepare slab display: {error:?}` |
| [items.rs:98](../crates/core/src/plot/redstone_tools/items.rs#L98) | `Cannot prepare slab components: {error:?}` |
| [mod.rs:91](../crates/core/src/plot/redstone_tools/mod.rs#L91) | `/tp {} {} {}` |
| [mod.rs:92](../crates/core/src/plot/redstone_tools/mod.rs#L92) | `{} -p {page}` |
| [search.rs:108](../crates/core/src/plot/redstone_tools/search.rs#L108) | ` (result limit reached)` |
| [search.rs:118](../crates/core/src/plot/redstone_tools/search.rs#L118) | `({}, {}, {})` |
| [search.rs:121](../crates/core/src/plot/redstone_tools/search.rs#L121) | `\n  {} {}: ` |
| [search.rs:129](../crates/core/src/plot/redstone_tools/search.rs#L129) | `[Previous] ` |
| [selection.rs:97](../crates/core/src/plot/redstone_tools/selection.rs#L97) | `§bSize: {width} × {height} × {depth}` |
| [selection.rs:98](../crates/core/src/plot/redstone_tools/selection.rs#L98) | `{color}Volume: {volume}` |
| [stack.rs:63](../crates/core/src/plot/redstone_tools/stack.rs#L63) | `Spacing overflows when reversing direction` |
| [stack.rs:132](../crates/core/src/plot/redstone_tools/stack.rs#L132) | `Stack X coordinate overflows` |
| [stack.rs:133](../crates/core/src/plot/redstone_tools/stack.rs#L133) | `Stack Y coordinate overflows` |
| [stack.rs:134](../crates/core/src/plot/redstone_tools/stack.rs#L134) | `Stack Z coordinate overflows` |
| [stack.rs:141](../crates/core/src/plot/redstone_tools/stack.rs#L141) | `Stack coordinate overflows` |

## Help and other text

| Surface | Proposed presentation | Preserved content |
| --- | --- | --- |
| `/help` | Use distinct page titles: “First pawsteps”, “Your patch of turf”, “Circuit heartbeat”, “Building tricks”, “Redstone sniff tools”, “Schematic stash”, “Piston wiggles”, “Following the tick trail”, “Barks and messages”, and “Redpiler tricks”. | Keep all ten help-page command examples and explanations. |
| WorldEdit help heading | Change ` Help for /{} ` to ` Pawbook for /{} `. | Keep the displayed command, separators, argument defaults, descriptions, flag letters, and usage layout. |
| WorldEdit argument errors | Keep `Error parsing argument of type {:?}: {}` and the reason. | Preserve parser type and diagnostic details. |
| Redpiler sidebar title | `Redpiler watch` | Keep state labels, option names, colors, and scoreboard identity. |
| Selection sidebar | `Selection pawprints`; `Plant both paws: //pos1, //pos2` for incomplete selection. | Keep dimension and volume numbers and colors. |
| Server-authored tool item labels | Keep current generated names and lore for this feedback rollout. | Preserve block names, comparator strengths, and placement instructions. |
| Debug inspection | Keep coordinates, block and entity dumps, and face-power values. | Do not cute-transform debug payloads. |
| Player chat, `/say`, `/tellraw`, signs, authored names and lore | Keep exact text. | Preserve authored components, URLs, and current sanitation behavior. |
| Existing teleport and page buttons | Keep coordinate and page labels; propose `Boop to follow this trail.` only for the hover label. | Preserve exact action commands and routing. |
| Server-list description and container titles | Keep administrator MOTD and Minecraft translation keys. | Preserve configuration and localization. |
| Library and storage diagnostics | Keep actual OS, parser, NBT, compression, and serialization reasons, with existing server-authored failure wrappers. | Preserve paths, names, numbers, and permissions. |

Entries marked “ok” and existing review notes were retained.

All curly-brace payload placeholders in each proposed replacement match its original template. New command guidance uses existing commands. The in-progress RedstoneTools source can change after this snapshot; review newly added phrases before implementing any replacements.
