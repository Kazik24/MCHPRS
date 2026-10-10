//! Server-authored English feedback. Authored chat and diagnostics never pass
//! through a runtime text transformation.
//!
//! Fixed messages are constants; interpolation uses named, compile-time checked
//! formatting functions. Add wording here and use the corresponding entry at
//! the producer, keeping packet colors and routing at their existing call sites.

use std::fmt::{Debug, Display};
use std::time::Duration;

// Rust's format macros require literal templates. This declaration keeps the
// templates together while generating ordinary constants and small functions.
macro_rules! catalog {
    (
        fixed { $( $constant:ident = $text:literal; )* }
        formatted { $( $function:ident( $( $argument:ident: $ty:ty ),* ) = $template:literal; )* }
    ) => {
        $( pub(crate) const $constant: &str = $text; )*
        $( pub(crate) fn $function( $( $argument: $ty ),* ) -> String {
            format!($template)
        } )*
    };
}

catalog! {
    fixed {
        USAGE_SMALL = "Usage: /small [on|off|wolf|fox|cat|ocelot|baby]";
        SMALL_ENABLED = "Smol paws mode on";
        SMALL_DISABLED = "Small mode off. You're back to your normal size.";
        SMALL_NO_ROOM = "There isn't room to return to full size here. Move into an open space first.";
        HELP_SMALL = "Small mode\n/small toggles half-size movement and an animal appearance for other players.\n/small on and /small off select the mode explicitly.\n/small wolf|fox|cat|ocelot|baby changes your saved animal; ocelot is the default. Baby ocelot uses an even smaller model and hitbox.\n/gm cat enables creative play in small mode; other gamemodes restore normal size.\nYour own view stays a scaled player so the preview cannot block your clicks.\nMove into an open space before turning it off. Your mode and animal follow you between plots and are saved when you leave.";
        USAGE_SETWARP = "Usage: /setwarp <name> (creates or replaces a shared warp)";
        USAGE_WARP = "Usage: /warp [name] (omit the name to list shared warps)";
        INVALID_WARP_NAME = "Warp names must be 1-32 letters, digits, underscores or hyphens.";
        NO_WARPS = "No saved warps. Use /setwarp <name> to save your current position.";
        WARP_STORAGE_FAILED = "Could not access saved warps. Please try again.";
        HELP_WARPS = "Shared warps\n/setwarp <name> saves your exact position and facing direction for everyone.\nUsing an existing name replaces its destination.\n/warp <name> visits a saved destination, including on another plot.\n/warp lists destinations; Tab completes their names.\nNames are case-insensitive and use 1-32 letters, digits, underscores or hyphens.";
        USAGE_GIT_REBASE = "Usage: /git rebase <branch>";
        GIT_REBASE_NAMED_BRANCH_REQUIRED = "Name this detached trail first with /git branch <name>, then /git checkout <name> before rebasing, pup.";
        GIT_REBASE_SOURCE_BRANCH_REQUIRED = "These paws need an existing source branch. Use /git branch to sniff out its name :3";
        GIT_OWNER_STORAGE_LIMIT_UNAVAILABLE = "Could not sniff out the plot owner's Git storage allowance. Try again in a moment, pup.";
        WE_SELECTION_VOLUME_OVERFLOW = "This selection is beyond the volume these paws can count.";
        WE_OPERATION_OUTSIDE_PLOT = "This WorldEdit pawstep would leave the current plot or world height.";
        WE_CLIPBOARD_COORDINATES_OVERFLOW = "This clipboard reaches beyond the coordinate range of these paws.";
        WE_EMPTY_CLIPBOARD_DIMENSION = "This clipboard has an empty dimension, pup.";
        WE_NUMERIC_ARGUMENT_LIMIT = "WorldEdit counts cannot exceed 4096, pup.";
        WE_CLIPBOARD_OFFSET_OUTSIDE_RANGE = "This clipboard offset is beyond the supported pawstep coordinates.";
        WE_INVALID_CLIPBOARD_WORK = "This clipboard exceeds the WorldEdit work limit or has invalid dimensions, pup.";
        WE_STACK_WORK_LIMIT = "This stack would give your paws more blocks than the WorldEdit work limit allows.";
        WE_STACK_DISTANCE_OVERFLOW = "This stack reaches beyond the distance these paws can cover.";
        COMMAND_BLOCK_OUTPUT_LIMIT = "This command block has filled its output paws: command-block output limit reached.";
        COMMAND_BLOCK_OUTPUT_BYTE_LIMIT = "This command block has filled its output paws: command-block output byte limit reached.";
        COMMAND_BLOCK_EXECUTION_UNAVAILABLE = "Command block tricks are unavailable in this world, pup.";
        SCHEMATIC_V3_NESTED_SCHEMA_REQUIRED = "Schematic: these paws need a nested schema Compound for v3.";
        SCHEMATIC_METADATA_COMPOUND_REQUIRED = "Metadata: these paws expected a Compound.";
        SCHEMATIC_OFFSET_DISPLACEMENT_OVERFLOW = "Offset: this pawprint displacement overflows clipboard coordinates.";
        SCHEMATIC_BLOCK_CONTENT_REQUIRED = "Schematic.Blocks: these importing paws require block content.";
        SCHEMATIC_ENTITY_LIST_REQUIRED = "BlockEntities: these paws expected a List.";
        SCHEMATIC_GZIP_READ_FAILED = "Unpacking gzip schematic NBT with careful paws";
        SCHEMATIC_SCHEMA_INVALID = "Sniffing the schematic schema";
        SCHEMATIC_BLOCK_LIMIT_EXCEEDED = "dimensions: this schematic pawprint exceeds the 16,777,216 block limit.";
        SCHEMATIC_TOO_FEW_BLOCK_BYTES = "block data: this pawprint has fewer bytes than required block entries.";
        SCHEMATIC_OFFSET_X_OVERFLOW = "Schematic pawprint offset X overflow";
        SCHEMATIC_OFFSET_Y_OVERFLOW = "Schematic pawprint offset Y overflow";
        SCHEMATIC_OFFSET_Z_OVERFLOW = "Schematic pawprint offset Z overflow";
        SCHEMATIC_CLIPBOARD_VOLUME_OVERFLOW = "Schematic pawprint clipboard volume overflow";
        SCHEMATIC_INVALID_CLIPBOARD_GEOMETRY = "This schematic pawprint has invalid clipboard geometry.";
        SCHEMATIC_INVALID_BLOCK_STATE = "This schematic pawprint has an invalid clipboard block state.";
        SCHEMATIC_RELATIVE_FILENAME_REQUIRED = "Give these paws a relative schematic filename.";
        SCHEMATIC_INVALID_PATH = "That schematic path has this snoot puzzled. Use a valid relative filename.";
        SCHEMATIC_EXTENSION_REQUIRED = "Give this schematic a .schem or .schematic extension, pup.";
        SCHEMATIC_PATH_OUTSIDE_FOLDER = "That schematic trail leaves the schematic folder. Keep those paws inside it.";
        SCHEMATIC_SHARED_LIBRARY_READ_ONLY = "The rf schematic library is read-only, pup. Save your own pawprints outside rf.";
        SCHEMATIC_INVALID_ANCESTOR = "Could not follow that schematic path with these paws.";
        GIT_MEMORY_LIMIT_REACHED = "The Git pawprint RAM workspace is full (at most 1 GiB shared). Wait for comparisons to expire or leave the plot and retry.";
        GIT_WORKER_FAILED = "The Git helper tripped over its paws; unfinished checkout may need startup recovery.";
        GIT_STARTUP_RECOVERY_REQUIRED = "Git checkout needs startup recovery, pup. This plot is paused and locked; contact the server administrator.";
        GIT_CHECKOUT_IN_PROGRESS = "Switching plot pawprints. Please wait for checkout to finish.";
        GIT_FINISH_PARTIAL_TICK = "Finish this partial tick before tucking away a Git pawprint.";
        GIT_SNAPSHOT_SIZE_LIMIT = "This plot pawprint exceeds the Git snapshot size limit.";
        GIT_CAPTURE_SIZE_OVERFLOW = "This Git pawprint is too big: capture size overflow.";
        GIT_OPERATION_RUNNING = "A Git helper is already busy on this plot. Give those paws a moment.";
        GIT_WORKER_QUEUE_FULL = "The Git helpers have full paws. Try again in a moment.";
        GIT_PERMISSION_DENIED = "These paws need permission to use Git on this plot.";
        GIT_CHECKOUT_LOCKED = "Plot changes are locked, pup: checkout is in progress or needs recovery.";
        USAGE_GIT_DIFF_INSPECT = "Usage: /git diff inspect <x> <y> <z> [from|to]";
        GIT_NO_COMPARISON = "No comparison for this snoot yet. Use /git diff <from> <to>.";
        GIT_ENTITY_SIZE_LIMIT = "This pawprint has block data larger than 256 KiB. Trim that data before saving, pup.";
        GIT_NBT_RESOURCE_LIMIT = "This pawprint has item NBT beyond safe limits (64 KiB, 4096 tags, 64 levels). Existing history was preserved, pup.";
        GIT_OPERATION_STARTED = "Git helper on the trail :3";
        GIT_WORKER_DISCONNECTED = "The Git helper lost the trail: worker disconnected.";
        GIT_WORKER_DISCONNECTED_SHUTDOWN = "The Git helper lost the trail during shutdown: worker disconnected.";
        USAGE_GIT_LOG = "Usage: /git log [--all] [n] or /git log [--all] --page <page>";
        GIT_MISSING_PAGE = "Which page should this snoot fetch? Add a page number after --page.";
        GIT_SEARCH_TEXT_LIMIT = "This snoot needs 1..128 characters of search text.";
        GIT_REPOSITORY_IDENTITY_MISMATCH = "This pawprint repository has a different version or plot identity.";
        GIT_NO_COMMITS_HINT = "No saved pawprints yet. Use /git commit <message>.";
        GIT_AMBIGUOUS_COMMIT = "That commit prefix has more than one scent. Use more characters.";
        GIT_INVALID_SIZE_HEADER = "This Git pawprint has an invalid size header.";
        GIT_OBJECT_SNAPSHOT_SIZE_LIMIT = "This stored pawprint exceeds the Git snapshot size limit.";
        GIT_OVERSIZED_OBJECT = "This stored Git pawprint is oversized.";
        GIT_CHECKSUM_MISMATCH = "This stored Git pawprint has a checksum mismatch.";
        GIT_PLOT_STORAGE_FULL = "This plot has filled its Git storage quota, pup. Existing pawprint history was preserved.";
        GIT_GLOBAL_STORAGE_FULL = "The server's Git storage quota is full. No room for more pawprints.";
        GIT_STORAGE_LOCK_FAILED = "The Git helper could not lock the pawprint store.";
        GIT_COMMIT_MESSAGE_LIMIT = "Give this pawprint a commit message containing 1..256 characters.";
        GIT_NOTHING_CHANGED = "No new pawprints: nothing changed since the last commit.";
        GIT_BRANCH_NAME_RULES = "Name your branch trail with 1..20 letters, digits, _ or -; no HEAD or commit-like names.";
        GIT_BRANCH_LIMIT = "All 128 branch trails are taken, pup.";
        GIT_BRANCH_EXISTS = "That branch trail already exists. Pick another name.";
        GIT_NO_BRANCHES_HINT = "No branch trails yet. Start with /git commit <message>.";
        GIT_CREATE_MASTER_HINT = "No saved pawprints yet. Use /git commit <message> to create master.";
        GIT_INVALID_PAGE = "Pick a pawbook page between 1 and 100000.";
        GIT_NO_COMMITS = "No saved pawprints yet.";
        GIT_PREVIOUS_PAGE = "Previous";
        GIT_NEXT_PAGE = "Next";
        GIT_ROOT_COMMIT = "root";
        GIT_INVALID_BRANCH_NAME = "That branch trail needs a valid name, pup.";
        GIT_INVALID_RECOVERY_ID = "That recovery pawprint needs an 8..64 character hexadecimal ID.";
        GIT_UNKNOWN_RECOVERY_ID = "Could not sniff out one recovery with that ID. Check /git recoveries or use more characters.";
        GIT_DETACHED_LABEL = "detached HEAD";
        GIT_INVALID_COMPRESSED_SIZE = "This packed Git pawprint has an invalid size or trailing data.";
        GIT_ROLLBACK_NEEDS_RECOVERY = "Checkout stumbled and rollback needs startup recovery, pup.";
        GIT_UNSUPPORTED_SNAPSHOT = "This Git pawprint uses an unsupported snapshot or data version.";
        GIT_SNAPSHOT_PLOT_MISMATCH = "This Git pawprint belongs to another plot or plot size.";
        GIT_INVALID_ENTITY_COORDINATES = "This Git pawprint has invalid block entity coordinates.";
        GIT_CONFIGURED_SNAPSHOT_SIZE_LIMIT = "This Git pawprint exceeds the configured snapshot size limit.";
        GIT_TRUNCATED_SNAPSHOT = "This Git pawprint is incomplete: truncated snapshot.";
        GIT_DECOMPRESSION_LIMIT = "This Git pawprint exceeds the decompression limit.";
        GIT_POSITION_OUTSIDE_PLOT = "That position is outside this plot, pup.";
        GIT_DIFFERENT_PLOTS = "These pawprints belong to different plots and cannot be compared.";
        GIT_FROM_LABEL = "From";
        GIT_TO_LABEL = "To";
        GIT_EXECUTION_CHANGED_SUFFIX = "; execution changed";
        GIT_UNKNOWN_ARGUMENTS = "That Git trick is unfamiliar to these paws.";
        GIT_CHANGES = "Git pawprints: ";
        GIT_NO_CHANGES = "No changed pawprints :3";
        GIT_WORKING_BUILD = "working build";
        GIT_BRANCHES_HEADING = "Branch trails :3";
        GIT_HISTORY_TITLE = "Git pawprint history :3";
        GIT_NO_BLOCK_HISTORY = "No changes recorded for this block, pup.";
        GIT_INSPECT_TARGET_REQUIRED = "Look at a block within 10 blocks so this snoot can inspect its history.";
        GIT_POSITION_UNCHANGED = "No changed pawprints at that position in this comparison.";
        GIT_BLOCK_DATA_CHANGED = "\nSniffed out changed block data.";
        GIT_INVALID_DETAILS_SIDE = "Choose from or to so this snoot knows which details to fetch.";
        GIT_DATA_TRUNCATED = "\n[Data trimmed to fit this pawbook]";
        GIT_NO_BLOCK_DATA = "No saved block data for these paws.";
        HELP_GIT = "Plot Git pawbook :3\n/git commit <message>\n/git log [--all] [n|--page n]\n/git status\n/git diff [ref] [ref]\n/git restore <ref>\n/git inspect\n/git branch [name [ref]]\n/git checkout <branch|commit>\n/git rebase <branch>\n/git show <ref>\n/git search [--all] [--page n] <text>\n/git diff inspect <x> <y> <z> [from|to]\n/git recoveries [page]\n/git recover <id> <new-branch>";
        USAGE_PLOT_HOME = "Usage: /p home";
        PLOT_HOME_UNCLAIMED = "No plot den of your own yet. Use /p auto to claim one.";
        PLOT_MEMBER_ALREADY_ADDED = "That player is already in this plot pack.";
        PLOT_MEMBER_NOT_ADDED = "That player is not in this plot pack.";
        PLOT_MEMBER_UNKNOWN_PLAYER = "No cached pawprint for that name or UUID. They must have joined this server at least once.";
        PLOT_MEMBER_AMBIGUOUS_PLAYER = "Old cached pawprints share that nickname. Ask the target player to rejoin so these paws can refresh their identity, or use their UUID.";
        PLOT_MEMBER_IS_OWNER = "The plot owner leads this pack and cannot be added or removed as a member.";
        PLOT_CLAIMS_READ_FAILED = "Could not sniff out the plot claims. Try again in a moment.";
        PLOT_AUTO_SEARCH_FULL = "No free den in the automatic search area. Choose a plot and use /plot claim.";
        INVALID_COORDINATE = "These paws need a valid coordinate.";
        COORDINATE_OVERFLOW = "That coordinate is beyond this pawstep range.";
        USAGE_ADV = "Usage: /adv [nano|pico] <ticks>";
        ADV_GAME_TICKS_LABEL = "ticks";
        ADV_NANO_TICKS_LABEL = "nano-ticks";
        ADV_PICO_TICKS_LABEL = "pico-ticks";
        COMPASS_NO_SAFE_DESTINATION = "No safe landing spot for these paws in sight. Aim somewhere else.";
        SELECTION_OUTSIDE_WORLD_HEIGHT = "Your selection reaches beyond the world height, pup. Keep those paws inside the build range.";
        AUTHENTICATED_PROXY_REQUIRED = "This den needs the authenticated Velocity proxy. Connect through it, pup.";
        HISTORY_INVALID_MEMORY_LIMIT = "Give the pawprint store a nonnegative, representable number of MiB.";
        HISTORY_MEMORY_LIMIT_REACHED = "The tick-history pawprint store has reached its memory limit.";
        HISTORY_ALLOCATION_FAILED = "Could not make memory room for these history pawprints.";
        HISTORY_SNAPSHOT_TOO_LARGE = "This history pawprint is too large.";
        HISTORY_COMPRESSION_SIZE_OVERFLOW = "This history pawprint is too big: compression size overflow.";
        HISTORY_CHECKSUM_INVALID = "This history pawprint has an invalid checksum or size.";
        HISTORY_DECODE_SIZE_INVALID = "This history pawprint has an invalid size.";
        SCOREBOARD_ENGINE_INTERPRETER = "Engine: Interpreter";
        SCOREBOARD_ENGINE_COMPILING = "Engine: Compiling...";
        SCOREBOARD_ENGINE_REDPILER = "Engine: Redpiler ON";
        SCOREBOARD_HISTORY_OFF = "Hist: Off :3";
        SCOREBOARD_VISUAL_OFF = "Visual: OFF :3";
        SCOREBOARD_ON = "on";
        SCOREBOARD_OFF = "off";
        SCOREBOARD_GIT_LOADING = "Git: loading...";
        SCOREBOARD_GIT_NONE = "Git: none :3";
        SCOREBOARD_GIT_RESTORING = "Git: fetching...";
        SCOREBOARD_GIT_RECOVERY = "Git: recovery!";
        SCOREBOARD_GIT_HIDDEN = "Git: paws off :3";
        SIGN_FRONT_LABEL = "front";
        SIGN_BACK_LABEL = "back";
        USAGE_VISUAL = "Usage: /visual [pistons [auto|on|off] | rate [0-1000] | displayonly [on|off]]";
        SCREEN_ONLY_ON = "Screen-only updates ON: lamp pixels get booped; wire and piston animations stay still. Setting saved for this plot.";
        SCREEN_ONLY_OFF = "Screen-only updates OFF: all pending block states caught up with your paws. Setting saved for this plot.";
        DISPLAY_ONLY_BREAK_WARNING = "Display-only is enabled: block changes still happen, but may not appear to players.";
        WE_ARGUMENT_AMOUNT_TO_CONTRACT_THE_SELECTION_BY = "Amount to contract the selection by";
        WE_ARGUMENT_AMOUNT_TO_EXPAND_THE_SELECTION_BY = "Amount to expand the selection by";
        WE_ARGUMENT_ROTATION_DEGREES = "Degrees to rotate around the vertical (Y) axis";
        WE_ARGUMENT_AMOUNT_TO_SHIFT_THE_SELECTION_BY = "Amount to shift the selection by";
        WE_ARGUMENT_COMMAND_TO_RETRIEVE_HELP_FOR = "Command to retrieve help for";
        WE_ARGUMENT_DIRECTION_TO_CONTRACT = "Direction to contract";
        WE_ARGUMENT_DIRECTION_TO_EXPAND = "Direction to expand";
        WE_ARGUMENT_DIRECTION_TO_SHIFT = "Direction to shift";
        WE_ARGUMENT_DISTANCE_TO_GO_UPWARDS = "Distance to go upwards";
        WE_ARGUMENT_OF_COPIES_TO_STACK = "# of copies to stack";
        WE_ARGUMENT_OF_LEVELS_TO_ASCEND = "# of levels to ascend";
        WE_ARGUMENT_OF_LEVELS_TO_DESCEND = "# of levels to descend";
        WE_ARGUMENT_THE_CONTAINER_TYPE_TO_REPLACE = "The container type to replace";
        WE_ARGUMENT_THE_CONTAINER_TYPE_TO_REPLACE_WITH = "The container type to replace with";
        WE_ARGUMENT_THE_DIRECTION_TO_FLIP_DEFAULTS_TO_LOOK_DIRECTION = "The direction to flip, defaults to look direction";
        WE_ARGUMENT_THE_DIRECTION_TO_MOVE = "The direction to move";
        WE_ARGUMENT_THE_DIRECTION_TO_STACK = "The direction to stack";
        WE_ARGUMENT_THE_DISTANCE_TO_MOVE = "The distance to move";
        WE_ARGUMENT_THE_FILE_NAME_OF_THE_SCHEMATIC_TO_LOAD = "The file name of the schematic to load";
        WE_ARGUMENT_THE_FILE_NAME_OF_THE_SCHEMATIC_TO_SAVE = "The file name of the schematic to save";
        WE_ARGUMENT_THE_MASK_OF_BLOCKS_TO_MATCH = "The mask of blocks to match";
        WE_ARGUMENT_REPLACE_MASK = "The mask of blocks to replace";
        WE_ARGUMENT_THE_PATTERN_OF_BLOCKS_TO_REPLACE_WITH = "The pattern of blocks to replace with";
        WE_ARGUMENT_THE_PATTERN_OF_BLOCKS_TO_SET = "The pattern of blocks to set";
        ARGUMENT_CANNOT_INFER = "missing argument; no default is available";
        ARGUMENT_INVALID_CONTAINER = "error parsing container type";
        ARGUMENT_INVALID_UINT = "expected an integer from 0 through 4294967295";
        ARGUMENT_UNKNOWN_DIRECTION = "unknown direction";
        CONTAINER_INVALID_POWER = "Pup, container power must be 0..15 or lowercase a..f.";
        HELP_CHAT = "Chat and command blocks\n/say <text> sends a message to everyone.\n/tellraw @a {\"text\":\"Hello\"} sends formatted text.\nUse @a, @s or a player name; selector filters are ignored.\nCommand blocks support say and tellraw. They require creative mode and permission. Other commands are saved in schematics but do not run.";
        HELP_HISTORY = "Tick history\n/rhistory on [ticks] records ticks (default 100); /rhistory off clears it.\n/rhistory shows count and memory. /back [ticks] rewinds; resume with /tps 20.\nRewind clears WorldEdit undo/redo. History needs the interpreter and resets when the plot becomes empty, on compile or restart.\nOld ticks drop at the shared 2 GiB limit. Admins can use /rhistory limit <MiB>.";
        HELP_PISTONS = "Visual settings\n/visual shows this plot's settings.\n/visual pistons [auto|on|off] controls piston animation.\n/visual rate [0-1000] sets update speed; 0 stops periodic updates. Above the server TPS threshold (200 by default), updates cap at 10 Hz.\n/visual displayonly [on|off] sends only lamp updates. Block edits still happen, but may not appear to players.";
        HELP_PLOTS = "Plots\n/p auto claims an empty plot; /p claim claims the plot you're in.\n/p info shows the owner; /p middle goes to the centre.\n/p home visits your first plot; /p visit <player> [number] visits a plot.\n/p add <nick> and /p remove <nick> manage plot members.\n/p tp <x> <z> uses plot coordinates.\n/p lock and /p unlock control leaving. /p select selects the plot for WorldEdit.";
        HELP_QUICK_START = "MROWW — Minecraft Redstone o Wysokiej Wydajności\n/p auto claims a plot.\n/help plots - Claim and visit plots.\n/help warps - Save and visit shared destinations.\n/help tps - Control and step simulation.\n/help we - WorldEdit.\n/help tools - Redstone tools.\n/help wire - Draw redstone wires.\n/help schematics - Load and save builds.\n/help visual - Set visual representation.\n/help rewind - Tick history.\n/help git - Plot commits, branches and glowing diffs.\n/help chat - Chat and command blocks.\n/help redpiler - Compiled simulation.";
        HELP_REDPILER = "Compiled simulation\n/rp compile enables compiled mode; /rp reset returns to the interpreter.\nPiston circuits require the interpreter and cannot be compiled.\n/rp inspect checks the targeted block. /toggleautorp toggles automatic compilation.\n/rp analyze checks whether the plot can compile. It accepts the same compilation flags.\n/rp analyze --graph prepares a read-only candidate graph; optional --optimize and --io-only check optimization.\nUse /tps 0 and /adv 1 to step compiled execution. Nano/pico stepping requires /rp reset first.";
        HELP_REDSTONE_TOOLS = "Redstone tools\n//find <block> searches your selection.\n//ss <regex> searches signs; -p <page> shows more.\n//rs [direction] [count] [spacing] stacks copies; -e expands selection; -w includes air.\n/autostack [direction] [count] [spacing] [-e] automatically stacks your placements and removals in the selected region; /autostack off stops it. Leaving the plot stops it.\n/wire draws dust with a live preview; /help wire explains controls and safety limits.\n/container <type> <0..15> creates a comparator container.\n/cursel toggles the selection sidebar.";
        HELP_WIRE = "Wire pen\nHold any carrot on a stick and right-click to draw. /wire gives a named pen or resets your route.\nRight-click a block to use it as the first support, with dust directly above, or existing dust to reuse that endpoint.\nAim on the drawing plane. Right-click builds the green preview and continues from its end.\nF cycles horizontal (X/Z), vertical X (X/Y), vertical Z (Y/Z), then Free aiming. Sneak + F changes the preferred bend. The action bar shows the aiming mode.\n/wire free aims directly at blocks in any direction; /wire plane restores horizontal aiming. These commands keep your current start. F returns from Free to horizontal.\nPlanes control aiming; the router can take safe 3D detours. Vertical dust routes use staircases. Build an intermediate point, then change plane to draw in 3D.\nSneak + right-click or changing held item cancels the route. Hold the pen and right-click to start again while enabled.\n/wire off disables the pen for this login, including after item or plot changes. /wire, /wire free, or /wire plane enables it again.\nNew supports copy the starting support's passive material and color. Clicking dust samples the block below it. Active blocks or blocks with stored data fall back to glass; transparent materials may need white wool on steps.\nNo repeaters or signal-range check. Unsafe or unsupported circuit interactions are refused.\n//undo restores the last segment's geometry; it does not rewind simulation.";
        WIRE_USAGE = "Usage: /wire [free|plane|off]";
        WIRE_ENABLED = "Wire pen: RC start/build | F plane | Sneak+F bend";
        WIRE_DISABLED = "Wire pen disabled. /wire enables it.";
        WIRE_SELECT_START_FIRST = "Select a starting block first.";
        WIRE_START_SELECTED = "Start set | RC build | F plane | Sneak+F bend";
        WIRE_PLANE_HORIZONTAL = "Horizontal X/Z";
        WIRE_PLANE_VERTICAL_X = "Vertical X/Y";
        WIRE_PLANE_VERTICAL_Z = "Vertical Y/Z";
        WIRE_PLANE_FREE = "Free";
        WIRE_PENDING = "Routing...";
        WIRE_STALE = "Refreshing preview...";
        WIRE_NO_PATH = "No safe route. Try an intermediate point.";
        WIRE_BUDGET = "Route limit reached. Try a closer point.";
        WIRE_NO_TARGET = "Aim at a block or toward the drawing plane.";
        WIRE_UNSUPPORTED = "Unsafe circuit context. Try another route.";
        WIRE_ROUTE_NOT_DISPLAYED = "Wait for the green preview.";
        WIRE_FULL_HOTBAR = "Hold a carrot on a stick or free a hotbar slot for the wire pen.";
        USAGE_AUTOSTACK = "Usage: /autostack [direction] [count] [spacing] [-e], or /autostack off.";
        AUTO_STACK_ENABLED = "Enabled auto stack. From now on, blocks you place or remove in the selected region will automatically be stacked at the corresponding positions. Use /autostack off to stop.";
        AUTO_STACK_DISABLED = "Auto stack disabled.";
        AUTO_STACK_ALWAYS_COPIES_REMOVALS = "Auto stack always copies removals; omit -a and -w.";
        AUTO_STACK_NEEDS_NONZERO_COPIES_AND_SPACING = "Auto stack needs at least one copy and nonzero spacing.";
        HELP_SCHEMATICS = "Schematics\n//load <file>.schem loads a schematic; //paste places it.\nTo save: select the build, //copy, then //save <file>.schem.\nRedstoneFun schematics are in the rf/ folder.";
        HELP_TICKS = "Tick control\n/tps shows or sets speed: 20 is normal, 0 pauses, unlimited runs as fast as possible.\n/adv [count] steps game ticks. Pause first.\n/adv nano 1 steps a nanotick; /adv pico 1 steps a picotick.";
        HELP_WORLD_EDIT = "WorldEdit\nThis server supports a subset of WorldEdit. Type // and use tab completion to see available commands.\n//desel or /desel clears your selection.";
        WE_HELP_INVALIDATE_CACHES = "Clears interpreter caches for the current plot";
        INTERPRETER_CACHES_INVALIDATED = "Interpreter caches cleared for this plot.";
        UPDATE_SELECTION_OUTSIDE_HEIGHT = "Selection Y coordinates must be between 0 and 255.";
        SEARCH_NEXT_PAGE = "[Next]";
        SEARCH_PREVIOUS_PAGE = "[Previous] ";
        SEARCH_RESULT_LIMIT = " (result limit reached)";
        WE_HELP_ALSO_UPDATE_ALL_AFFECTED_BLOCKS = "Also update all affected blocks";
        WE_HELP_CHOOSE_A_REGION_SELECTOR = "Choose a region selector";
        WE_HELP_CONTRACT_THE_SELECTION_AREA = "Contract the selection area";
        WE_HELP_COPY_THE_SELECTION_TO_THE_CLIPBOARD = "Copy the selection to the clipboard";
        WE_HELP_COUNTS_THE_NUMBER_OF_BLOCKS_MATCHING = "Counts the number of blocks matching a mask";
        WE_HELP_CUT_THE_SELECTION_TO_THE_CLIPBOARD = "Cut the selection to the clipboard";
        WE_HELP_DISPLAYS_HELP_FOR_WORLDEDIT_COMMANDS = "Displays help for WorldEdit commands";
        WE_HELP_EXPAND_THE_SELECTION_AREA = "Expand the selection area";
        WE_HELP_FLIP_THE_CONTENTS_OF_THE_CLIPBOARD = "Flip the contents of the clipboard across the origin";
        WE_HELP_GIVES_A_WORLDEDIT_WAND = "Gives a WorldEdit wand";
        WE_HELP_GO_DOWN_A_FLOOR = "Go down a floor";
        WE_HELP_GO_UPWARDS_SOME_DISTANCE = "Go upwards some distance";
        WE_HELP_GO_UP_A_FLOOR = "Go up a floor";
        WE_HELP_IGNORE_AIR_BLOCKS = "Ignore air blocks";
        WE_HELP_LOADS_A_SCHEMATIC_FILE_INTO_THE = "Loads a schematic file into the clipboard";
        WE_HELP_MOVE_THE_CONTENTS_OF_THE_SELECTION = "Move the contents of the selection";
        WE_HELP_PASTE_THE_CLIPBOARD_S_CONTENTS = "Paste the clipboard's contents";
        WE_HELP_REDOES_THE_LAST_ACTION_FROM_HISTORY = "Redoes the last action (from history)";
        WE_HELP_REPEAT_THE_CONTENTS_OF_THE_SELECTION = "Repeat the contents of the selection";
        WE_HELP_REPLACES_ALL_CONTAINER_TYPES_IN_THE = "Replaces all container types in the selection";
        WE_HELP_REPLACE_ALL_BLOCKS_IN_A_SELECTION = "Replace all blocks in a selection with another";
        WE_HELP_ROTATE_THE_CONTENTS_OF_THE_CLIPBOARD = "Rotate the contents of the clipboard";
        WE_HELP_SAVE_A_SCHEMATIC_FILE_FROM_THE = "Save a schematic file from the clipboard";
        WE_HELP_SELECT_THE_PASTED_REGION = "Select the pasted region";
        WE_HELP_SETS_ALL_THE_BLOCKS_IN_THE = "Sets all the blocks in the region; accepts block names or state IDs (0 = air)";
        WE_HELP_SET_POSITION_1 = "Set position 1";
        WE_HELP_SET_POSITION_1_TO_TARGETED_BLOCK = "Set position 1 to targeted block";
        WE_HELP_SET_POSITION_2 = "Set position 2";
        WE_HELP_SET_POSITION_2_TO_TARGETED_BLOCK = "Set position 2 to targeted block";
        WE_HELP_SHIFT_THE_SELECTION_AREA = "Shift the selection area";
        WE_HELP_SHIFT_THE_SELECTION_TO_THE_TARGET = "Shift the selection to the target location";
        WE_HELP_SKIP_AIR_BLOCKS = "Skip air blocks";
        WE_HELP_UNDOES_THE_LAST_ACTION_FROM_HISTORY = "Undoes the last action (from history)";
        WE_HELP_UPDATES_ALL_BLOCKS_IN_THE_SELECTION = "Updates all blocks in the selection";
        WE_HELP_UPDATE_THE_ENTIRE_PLOT = "Update the entire plot";
        WORLD_EDIT_HELP_ARGUMENTS = "\nArguments:";
        WORLD_EDIT_HELP_FLAGS = "\nFlags:";
        WORLD_EDIT_HELP_SEPARATOR = "--------------";
        WORLD_EDIT_HELP_SEPARATOR_END = "--------------\n";
        WORLD_EDIT_HELP_USAGE = "\nUsage: ";
        AMBIGUOUS_CONTAINER_TYPE = "That container name matches more than one type, pup. Use a longer or full name.";
        BLOCK_TRACE_FAILED = "My snoot couldn't find a block in sight. Aim at one and try again.";
        CANNOT_ADVANCE_NANO_TICKS_WHILE_REDPILER = "Redpiler is active, so nano-tick pawsteps are unavailable. Run /redpiler reset to switch to the interpreter.";
        CANNOT_ADVANCE_PICO_TICKS_WHILE_REDPILER = "Can't take pico-tick pawsteps while Redpiler is active. Run /redpiler reset first.";
        CANNOT_REDO_OUTSIDE_CURRENT_PLOT = "Redo can't fetch changes from outside your current plot. Return to that plot first.";
        CANNOT_UNDO_OUTSIDE_CURRENT_PLOT = "Undo can't follow pawprints outside your current plot. Return to that plot first.";
        CLIPBOARD_EMPTY = "Nothing in the clipboard for these paws to fetch. Use //copy first.";
        CLIPBOARD_EMPTY_PASTE = "Can't paste from an empty clipboard—fetch a selection with //copy first.";
        COMMAND_BLOCK_EXECUTED = "Command executed";
        COMMAND_BLOCK_UPDATED = "Woof! Your command block learned its new trick.";
        COMMAND_NOT_FOUND = "I haven't learned that command trick. Browse /help.";
        COMMAND_PERMISSION_DENIED = "You do not have permission to use this command. Paws off for now >w<";
        COMPLETE_SELECTION_MUST_INSIDE_PLOT_WORLD = "Both selection positions must be inside this plot and within world height, pup.";
        CONNECTION_TIMEOUT = "Your connection wandered off. Timed out.";
        DISABLE_TICK_HISTORY_BEFORE_NANO_PICO = "Tick history records whole game ticks, so these paws can't advance nano/pico steps while it's enabled. Run /rhistory off first.";
        EXPECTED_TEXT_STRING_OBJECT_OR_ARRAY = "Invalid text component, pup. Use a string, object or array.";
        FINISH_PARTIAL_TICK_RADVANCE_BEFORE_USING = "A game tick is still in progress. Finish that pawstep with /adv 1 before using tick history.";
        FIRST_POSITION_OUTSIDE_PLOT_BOUNDS = "Your first selection paw landed outside this plot. Set it inside the bounds.";
        FLAG_ARGUMENT_MUST_LAST_GROUPING = "Pup, put a flag that takes an argument last in a combined group.";
        FLAG_NAME_MUST_FOLLOW = "Missing flag name after -. Give that flag its tail back.";
        FLAG_REQUIRES_AN_ARGUMENT = "This flag needs an argument, pup. Add its value after the flag.";
        HISTORY_CAPACITY_MUST_BETWEEN_GAME_TICKS = "Tick-history capacity must be 1..2147483647 game ticks. Pick how many pawprints to keep.";
        HISTORY_MEMORY_ESTIMATE_EXCEEDS_SUPPORTED_SIZE = "History memory estimate exceeds the supported size.";
        HISTORY_SIZE_OVERFLOW = "History size overflow.";
        IP_FORWARDING_REQUIRED = "If you wish to use IP forwarding, please enable it in your BungeeCord config as well!";
        INVALID_ARGUMENT_COUNT = "Wrong number of arguments for this command trick. Check its syntax.";
        INVALID_NUMBER_ARGUMENTS_TELEPORT_COMMAND = "Wrong number of arguments for this teleport trick. Use /tp <player> or /tp <x> <y> <z>.";
        INVALID_REGULAR_EXPRESSION = "Invalid regular expression; that search pattern tangled the scent trail. Check its syntax.";
        INVALID_TELEPORT_COORDINATES = "Invalid teleport coordinates. Choose finite coordinates, little floof :3";
        INVALID_X_COORDINATE = "Invalid x coordinate; these paws need a number or a relative coordinate such as ~1.";
        INVALID_Y_COORDINATE = "Couldn't parse the y coordinate, pup. Use a number or a relative coordinate such as ~1.";
        INVALID_Z_COORDINATE = "Couldn't parse the z coordinate; it has my ears tilted. Use a number or a relative coordinate such as ~1.";
        INVENTORY_FULL = "You poor overpacked pup—your inventory is full. Free a slot for the next fetch.";
        LOOK_HORIZONTALLY_BEFORE_USING_RELATIVE_DIRECTION = "Look horizontally before using a relative direction—point that snoot straight ahead.";
        MASK_PROPERTIES_MUST_END = "Close the mask's property list with ]—don't leave its tail open.";
        NO_BLOCK_SIGHT = "No block under your snoot. Aim at one first.";
        NO_FREE_SPOT_ABOVE_YOU_FOUND = "No room to perch above you. Can't ascend.";
        NO_FREE_SPOT_BELOW_YOU_FOUND = "No landing room below your paws. Can't descend.";
        ONE_COMPRESSED_SNAPSHOT_CANNOT_FIT_HISTORY = "One compressed snapshot cannot fit the history limit.";
        ONLY_TELLRAW_SAY_SUPPORTED_COMMAND_BLOCKS = "Command blocks have learned only two message tricks: /tellraw and /say.";
        PAGE_MUST_POSITIVE_INTEGER = "Pick a page number of 1 or higher, pup.";
        PAGE_NUMBERS_START_AT = "Page numbers start at 1, pup.";
        PERFORMANCE_REASONS_PLAYER_SPEED_CANNOT_HIGHER = "Flying zoomies top out at 10 for performance reasons.";
        PERMISSION_DENIED = "These paws don't have permission to perform that action.";
        PLAYER_NOT_FOUND = "No player on that scent trail. Check their name.";
        PLAYER_SAVE_COULD_NOT_LOADED_ASK = "Your player save could not be loaded. Ask the administrator to inspect the server log; your original file was preserved.";
        PLOT_ALREADY_CLAIMED = "Someone has already claimed this patch of turf.";
        PLOT_ALREADY_LOCKED = "You're already leashed to this plot.";
        PLOT_CANNOT_INTERACTED_WHILE_REDPILER_ACTIVE = "Redpiler's `--io-only` mode blocks interaction with this plot. Run `/redpiler reset` to put your paws back to work.";
        PLOT_INDEX_ZERO = "Plot indexes start at 1, pup.";
        PLOT_INVALID_ARGUMENT = "/plot doesn't know that trick. Peek at /help plots.";
        PLOT_INVALID_INDEX = "Couldn't read that plot index, pup. Use a positive integer.";
        PLOT_NOT_LOCKED = "No plot leash to unclip; you're already free to roam.";
        PLOT_PERMISSION_DENIED = "You do not have permission to change this plot. Keep your paws gentle >w<";
        PLOT_UNCLAIMED = "No paws on this plot yet";
        PLOT_UNLOCKED = "Off the plot leash! You're free to roam again ^^";
        PLOT_YOU_WERE_PREVIOUSLY_CRASHED = "The plot you were previously in has crashed!";
        REDPILER_AUTO_DISABLED = "Automatic Redpiler compilation disabled. Give /rp compile a boop when you're ready.";
        REDPILER_AUTO_ENABLED = "Automatic Redpiler compilation enabled—I'll handle that trick for you.";
        REDPILER_INVALID_ARGUMENT = "/redpiler hasn't learned that trick. Check /help redpiler.";
        REDPILER_OPTIMIZATION_HIGHLY_UNSTABLE_CAN_BREAK = "Redpiler optimization is highly unstable and can break builds. Use with caution!";
        PLOT_SIDEBAR_TITLE = "Plot monitor";
        RESULT_ACTION_HOVER = "Boop to follow this trail.";
        ROTATE_AMOUNT_MUST_MULTIPLE = "These rotation paws turn in 90-degree steps. Choose a multiple of 90.";
        RTPS_SET = "The circuit's new tick pace is set. Awoo!";
        RUN_SEARCH_FIRST = "No cached search results yet. Run a block or sign search so these paws have results to fetch.";
        SCHEMATIC_NOT_FOUND = "Couldn't fetch that schematic file. Check its path and filename.";
        SEARCH_NO_MATCHES = "You poor pup, the sniff patrol found no matches >w<";
        SECOND_POSITION_OUTSIDE_PLOT_BOUNDS = "Your second selection paw wandered outside this plot. Bring it inside the bounds.";
        SELECTION_CLEARED = "Brushed away your selection marks.";
        SELECTION_INCOMPLETE = "Your selection is incomplete, pup. Set both positions with //pos1 and //pos2.";
        SELECTION_REQUIRED = "Select a region with //pos1 and //pos2 before putting these paws to work.";
        SELECTION_SIDEBAR_DISABLED = "Tucked your selection's sidebar out of sight.";
        SELECTION_SIDEBAR_ENABLED = "Your selection's pawprints are on the sidebar now.";
        SELECTION_SIDEBAR_INCOMPLETE = "§7Set both paws: //pos1, //pos2";
        SELECTION_SIDEBAR_TITLE = "Selection pawprints";
        SELECT_POSITION_FIRST = "Plant the first selection paw with //pos1.";
        SELECT_POSITION_SECOND = "Plant the second selection paw with //pos2.";
        SERVER_CLOSED = "The server's curling up for now. See you next time!";
        SPEED_NEGATIVE = "Negative flying zoomies aren't a thing. Use /speed <0-10>.";
        SPACING_OVERFLOWS_WHEN_REVERSING_DIRECTION = "Spacing overflows when reversing direction";
        SPECIFY_NONNEGATIVE_MEMORY_LIMIT_MIB = "The tick-history memory limit must be a nonnegative number in MiB, pup.";
        SPECIFY_ONLY_ONE_DIRECTION = "Give these stacking paws only one direction per command.";
        SPECIFY_POSITIVE_HISTORY_CAPACITY_GAME_TICKS = "How long should the pawprint trail be? Choose a positive capacity in game ticks.";
        SPECIFY_POSITIVE_NUMBER_GAME_TICKS = "Those game-tick pawsteps need a positive count.";
        SPECIFY_POSITIVE_NUMBER_GAME_TICKS_REWIND = "Give me a positive number of game ticks to follow backward along the trail.";
        USAGE_SPEED = "Choose your flying zoomies with /speed <0-10>.";
        STACK_COORDINATE_OVERFLOWS = "Stack coordinate overflows";
        STACK_X_COORDINATE_OVERFLOWS = "Stack X coordinate overflows";
        STACK_Y_COORDINATE_OVERFLOWS = "Stack Y coordinate overflows";
        STACK_Z_COORDINATE_OVERFLOWS = "Stack Z coordinate overflows";
        CLIPBOARD_ROTATED_ZERO = "Rotated by 0. The clipboard mastered “stay.”";
        SUPPORTED_TARGETS_S_OR_PLAYER_NAME = "These messages can find @a, @s or a player name—choose one of those scent trails.";
        SWITCH_CREATIVE_MODE_FIRST = "These building paws need creative mode first.";
        TEXT_COMPONENT_NESTING_TOO_DEEP = "This text component is nested too deeply for these paws. Reduce its nesting.";
        THAT_PLAYER_NOT_WHITELISTED_ON_SERVER = "That player isn't in the whitelist pack, so there's no entry to remove.";
        TARGET_PLOT_NOT_LOADED = "Can't follow their trail: the target player's plot isn't loaded.";
        THERE_NOTHING_LEFT_REDO = "Redo has no later pawprints left to fetch.";
        THERE_NOTHING_LEFT_UNDO = "Undo has no earlier pawprints left to follow.";
        THERE_NO_CACHED_RESULTS_PAGINATE = "No cached search results to paginate, pup. Run a new search.";
        THESE_SEARCH_RESULTS_BELONG_ANOTHER_PLOT = "These search results belong to another plot. Send the sniff patrol out again here.";
        TICK_HISTORY_DISABLED_USE_RHISTORY_ON = "Tick history is disabled. Start recording pawprints with /rhistory on [ticks].";
        TICK_HISTORY_ONLY_AVAILABLE_DURING_INTERPRETED = "Redpiler is active, so these paws can't record tick history. Run /redpiler reset to switch to the interpreter.";
        TICK_REWIND_ONLY_AVAILABLE_DURING_INTERPRETED = "Redpiler is active, so these paws can't rewind ticks. Run /redpiler reset to switch to the interpreter.";
        TOOL_ITEM_GIVEN = "Woof, your item's fetched!";
        TOOL_PERMISSION_DENIED = "Your permissions don't include this command trick.";
        TOO_MANY_ARGUMENTS = "Too many arguments crowded into this trick. Check its syntax.";
        UNABLE_ALLOCATE_HISTORY_BUFFER = "Unable to allocate the history buffer.";
        UNABLE_PARSE_RTPS = "TPS has me chasing my tail. Use a number or unlimited.";
        UNABLE_PARSE_SEND_RATE = "Invalid world send rate, pup. Use a nonnegative integer in hertz.";
        UNABLE_PARSE_SPEED_VALUE = "Can't make a flying pace out of that value. Try /speed <0-10>.";
        UNBALANCED_MASK_PROPERTIES = "The mask has unbalanced property brackets. Pair each [ with ] so these paws can read it.";
        UNCLOSED_SELECTOR_OPTIONS = "Missing ] after the selector options. Close that bracket tail.";
        UNKNOWN_CONTAINER_TYPE_USE_CHEST_BARREL = "Can't fetch that container type. Choose chest, barrel, hopper or furnace.";
        UNKNOWN_GAMEMODE = "That's not a gamemode in my trick book. Choose creative (1), adventure (2), spectator (3), or cat.";
        UNKNOWN_HELP_TOPIC_USE_HELP_TOPICS = "That topic isn't in my pawbook. Use /help for topics, or //help <command> for WorldEdit.";
        USAGE_CONTAINER_CHEST_BARREL_HOPPER_FURNACE = "Fetch a powered stash with /container <chest|barrel|hopper|furnace> <0..15|a..f>.";
        USAGE_CURSEL = "Peek at your selection's pawprints with /cursel.";
        USAGE_FIND_MASK_OR_FIND_P = "Send the block sniff patrol with //find <mask>, or revisit //find -p <page>.";
        USAGE_HELP_TOPIC = "Looking for a pawbook? Use /help [topic].";
        USAGE_RBACK_TICKS = "Follow the tick trail backward with /back [ticks].";
        USAGE_RHISTORY_ON_TICKS_OFF_STATUS = "Tend the tick-history trail with /rhistory [on [ticks]|off|status|limit [MiB]].";
        USAGE_RSTACK_DIRECTION_COUNT_SPACING_E = "Line up your copies with //rstack [direction] [count] [spacing] [-e] [-w].";
        USAGE_SAY_MESSAGE = "Bark to the server with /say <message>.";
        USAGE_SIGNSEARCH_REGEX_OR_SIGNSEARCH_P = "Put your nose to the signs with //signsearch <regex>, or revisit //signsearch -p <page>.";
        USAGE_TELLRAW_TARGET_JSON_TEXT = "Dress your message in JSON with /tellraw <target> <JSON text>.";
        USAGE_WHITELIST_ADD_REMOVE_USERNAME = "To change who's in the pack: /whitelist [add | remove] (username).";
        USE_PROPERTY_VALUE_MASKS = "Write mask properties as property=value, pup.";
        WHITELIST_NOT_ENABLED = "The whitelist gate isn't on duty; whitelist is disabled.";
        WORLDEDIT_UNDO_REDO_CLEARED_AFTER_TICK = "Tick rewind brushed away the WorldEdit undo/redo pawprints.";
        WORLD_ALREADY_CURSED = "This world's fur is already standing on end—it's cursed >w<";
        WORLD_BEEN_BLESSED = "The world's fur lies flat again. Blessing complete ^^";
        WORLD_BEEN_CURSED_REDPILER_DISABLED_BLESS = "Rawr, the world is cursed! Redpiler disabled (/bless to undo).";
        WORLD_NOT_CURSED_CURSE_CURSE = "No curse ruffling this world's fur. (/curse to curse)";
        WORLD_SEND_RATE_CANNOT_GO_HIGHER = "World updates can't sprint past 1000 Hz.";
        WORLD_SEND_RATE_WAS_SUCCESSFULLY_SET = "World send rate set—your block updates have their new trot.";
        YOU_CAN_ONLY_USE_WORLDEDIT_ON = "WorldEdit paws may dig only in your own plot.";
        YOU_CAN_T_SET_SPEED_NAN = "NaN and -NaN aren't flying speeds these paws can use. Choose a number from 0 through 10.";
        YOU_NOT_WHITELISTED_ON_SERVER = "You are not whitelisted on this server";
    }
    formatted {
        small_animal(animal: impl Display) = "Small appearance set to {animal}.";
        warp_saved(name: impl Display) = "Saved shared warp '{name}'. Visit it with /warp {name}.";
        warp_not_found(name: impl Display) = "Warp '{name}' was not found. Use /warp to list destinations.";
        warp_teleport(name: impl Display) = "Teleporting to warp '{name}'.";
        warp_list(names: impl Display) = "Shared warps: {names}";
        git_rebased(branch: impl Display, source: impl Display, recovery: impl Display) = "Fetched {source}'s saved plot into {branch}'s working build. Branch tips unchanged; simulation paused. Edit, then /git commit <message> to tuck it away :3{recovery}";
        worldedit_work_limit(limit: u64) = "These WorldEdit paws can handle at most {limit} blocks.";
        advance_progress(advanced: u32, requested: u32, unit: impl Display) = "{advanced} of {requested} {unit}";
        plot_chat_source(x: i32, z: i32) = "Plot {x}-{z}";
        selection_sidebar_dimensions(width: u64, height: u64, depth: u64) = "Size: {width} × {height} × {depth}";
        selection_sidebar_volume(volume: u128) = "Volume: {volume}";
        worldedit_argument_default(value: impl Display) = " (defaults to {value})";
        schematic_loading_context(filename: impl Display) = "Fetching schematic ./schems/{filename}";
        schematic_expected_integer(key: impl Display) = "{key}: these paws expected Int.";
        schematic_expected_compound(key: impl Display) = "{key}: these paws expected Compound.";
        schematic_expected_vector(key: impl Display) = "{key}: these paws expected IntArray of exactly three entries.";
        schematic_unsupported_version(version: impl Display) = "Version: these paws cannot import schematic version {version}.";
        schematic_expected_dimension(key: impl Display) = "{key}: these paws expected unsigned NBT Short.";
        schematic_zero_dimension(key: impl Display) = "{key}: this pawprint has a zero dimension.";
        schematic_expected_bytes(field: impl Display) = "{field}: these paws expected ByteArray.";
        schematic_palette_integer(name: impl Display) = "Palette.{name}: these paws expected Int.";
        schematic_palette_invalid_index(name: impl Display, id: impl Display) = "Palette.{name}: this pawprint has a negative or duplicate index {id}.";
        schematic_palette_unknown_state(name: impl Display) = "Palette: this pawprint uses unsupported or invalid block state {name}.";
        schematic_truncated_varint(index: usize) = "block data: this pawprint has a truncated VarInt at entry {index}.";
        schematic_overflowing_varint(index: usize) = "block data: this pawprint has an overflowing palette VarInt at entry {index}.";
        schematic_unterminated_varint(index: usize) = "block data: this pawprint has an unterminated palette VarInt at entry {index}.";
        schematic_missing_palette_index(id: impl Display, index: usize) = "block data: this pawprint is missing palette index {id} at entry {index}.";
        schematic_trailing_block_bytes(entries: u32) = "block data: this pawprint has trailing bytes after {entries} entries.";
        schematic_entity_expected_compound(index: usize) = "BlockEntities[{index}]: these paws expected Compound.";
        schematic_entity_position_context(index: usize) = "Sniffing BlockEntities[{index}].Pos";
        schematic_entity_outside_bounds(index: usize, pos: impl Display) = "BlockEntities[{index}].Pos: {pos} is outside this schematic pawprint.";
        schematic_entity_expected_id(index: usize) = "BlockEntities[{index}].Id: these paws expected String.";
        schematic_entity_expected_data(index: usize) = "BlockEntities[{index}].Data: these paws expected Compound.";
        schematic_entity_context(index: usize, id: impl Display, pos: impl Display) = "Sniffing BlockEntities[{index}] {id} at {pos}";
        schematic_duplicate_entity_position(index: usize, pos: impl Display) = "BlockEntities[{index}]: duplicate pawprints at position {pos}.";
        schematic_items_simplified(simplified: usize, removed: usize) = "This schematic contains unsupported items/components, pup. Simplified {simplified} item stacks to plain items; removed {removed} unsupported item stacks. Custom data was discarded and vanilla stack limits applied.";
        git_detached_head(id: impl Display) = "detached HEAD [{id}]";
        schematic_ambiguous_filename(name: impl Display, example: impl Display) = "More than one schematic has the scent {name}. Use a subfolder path, such as {example}.";
        container_tool_name(kind: impl Display, power: u8) = "{kind} · power {power} :3";
        container_tool_lore(power: u8) = "Comparator signal: {power} / 15 · boop!";
        container_components_failed(error: impl Debug) = "Could not pack the container tool into these paws: {error:?}";
        scoreboard_history(ticks: impl Display, capacity: impl Display) = "Hist: {ticks}/{capacity}";
        plot_update_timings(ticks: u64, simulation: f64, flushes: u64, sections: u64, records: u64, collection: f64, enqueue: f64, rate: u32, mode: impl Display, packets: u64, bytes: u64, encoding: f64, compression: f64, writes: f64, queued: usize, coalesced: u64, failures: u64) = "Plot heartbeat pawbook: {ticks} ticks, simulation {simulation:.3}s; {flushes} visual flushes, {sections} sections, {records} block records; collection {collection:.3}s, enqueue {enqueue:.3}s. Current visual rate: {rate} Hz; mode: {mode}.\nCurrent clients (connection totals): {packets} packets, {bytes} bytes; visual encoding {encoding:.3}s, framing/compression {compression:.3}s, socket writes {writes:.3}s; {queued} queued bytes, {coalesced} coalesced blocks, {failures} send failures.";
        git_error(error: impl Display) = "Git: {error:#}";
        git_checkout_failed(error: impl Display) = "Git checkout failed: {error}";
        git_unknown_reference(reference: impl Display) = "Could not sniff out a branch or commit named {reference}.";
        git_unknown_commit(reference: impl Display) = "Could not sniff out commit {reference}.";
        git_committed(id: impl Display, branch: impl Display, message: impl Display) = "Committed {id} on {branch}: {message} :3";
        git_branch_created(name: impl Display, id: impl Display) = "New branch trail {name} at {id}. Follow it with /git checkout {name}.";
        git_status(branch: impl Display, id: impl Display, build_changed: bool, execution_changed: bool, used_mib: f64, limit_mib: f64) = "{branch} [{id}]\nBuild changes: {build_changed}; execution changes: {execution_changed}\nGit storage: {used_mib:.1} / {limit_mib:.1} MiB";
        git_history_heading(page: usize) = "Git pawprint history — page {page}";
        git_commit_details(id: impl Display, name: impl Display, date: impl Display, message: impl Display, parent: impl Display) = "{id}\n{name}, {date}\n{message}\nParent: {parent}";
        git_recovery_row(id: impl Display, date: impl Display, name: impl Display, source: impl Display) = "\n{id} {date} {name} from {source}";
        git_recoveries_heading(page: usize) = "Saved recovery pawprints — page {page}. Fetch one with /git recover <id> <new-branch>.";
        git_recovered_commit(id: impl Display) = "Recovered pawprint {id}";
        git_already_on_branch(branch: impl Display) = "Your paws are already on {branch}; unfinished work was kept.";
        git_checkout_recovery_suffix(id: impl Display) = " Recovery: {id} (see /git recoveries)";
        git_checked_out(branch: impl Display, recovery: impl Display) = "Checked out {branch}; simulation paused. Paws landed :3{recovery}";
        git_diff_reference(label: impl Display, id: impl Display) = "{label} [{id}]";
        git_status_heading(branch: impl Display, id: impl Display) = "On {branch} @ {id}";
        git_block_heading(x: i32, y: i32, z: i32) = "Block at {x}, {y}, {z}:";
        git_block_history_heading(count: usize) = "History [{count}]:";
        git_restored(id: impl Display, recovery: impl Display) = "Restored {id} (branch and HEAD untouched); simulation paused :3{recovery}";
        git_block_diff(x: i32, y: i32, z: i32, from: impl Display, to: impl Display) = "({x}, {y}, {z})\nFrom: {from}\nTo: {to}";
        git_block_data_details(side: impl Display, data: impl Display, suffix: impl Display) = "\n{side} data:\n{data}{suffix}";
        git_glow_status(shown: usize, total: u64) = "Glowing pawprints: {shown}/{total} changes nearby. Green: added; red: removed; yellow: changed.";
        plot_read_failed(error: impl Display) = "Could not sniff out your plots: {error}";
        plot_members_update_failed(error: impl Display) = "Could not update this plot pack: {error}";
        visual_setting_save_failed(error: impl Display) = "Could not tuck away the visual setting: {error}";
        plot_member_usage(command: impl Display) = "Usage: /p {command} <nick|uuid>";
        advance_tick_count_limit(limit: u32) = "These paws can advance between 0 and {limit} ticks per command.";
        plot_claim_failed(error: impl Display) = "Could not claim this plot den: {error}";
        history_limit_save_failed(error: impl Display) = "Could not tuck away the history limit: {error}";
        history_capture_workspace_failed(error: impl Display) = "No room to capture history pawprints: {error}";
        history_compression_workspace_failed(error: impl Display) = "No room to pack history pawprints: {error}";
        history_snapshot_invalid(error: impl Display) = "Could not read this history pawprint: {error}";
        history_rewind_workspace_failed(error: impl Display) = "No room to retrace history pawprints: {error}";
        history_compression_invalid(error: impl Display) = "Could not unpack this history pawprint: {error}";
        plot_member_added(name: impl Display, x: i32, z: i32) = "{name} joined the plot pack and can now build on plot ({x}, {z}) :3";
        plot_member_removed(name: impl Display, x: i32, z: i32) = "Removed {name} from the plot pack at ({x}, {z}).";
        history_free_buffers_first(used: impl Display) = "The pawprint store uses {used}. Free buffers with /rhistory off first.";
        block_debug(x: i32, y: i32, z: i32, block: impl Debug) = "Sniffing block at ({x}, {y}, {z}):\n    {block:?}";
        block_debug_power(power: impl Display) = "  Redstone power: {power}";
        block_debug_entity(entity: impl Debug) = "  Block entity:\n    {entity:?}";
        scoreboard_flag(flag: impl Display) = "Flag: {flag}";
        scoreboard_tps(actual: impl Display, target: impl Display) = "TPS: {actual}/{target}";
        scoreboard_history_memory(memory: impl Display) = "Hist mem: {memory}";
        scoreboard_visual_rate(rate: u32) = "Visual: {rate}hz";
        scoreboard_pistons(mode: impl Display, effective: impl Display) = "Pistons: {mode}/{effective}";
        scoreboard_screen_only(state: impl Display) = "Screen only: {state}";
        scoreboard_git_branch(branch: impl Display) = "Git: {branch}";
        scoreboard_git_detached(id: impl Display) = "Git: @{id}";
        plot_claim_limit(limit: impl Display) = "You can own at most {limit} plot(s).";
        argument_error(kind: impl Debug, reason: &str) = "Error parsing argument of type {kind:?}: {reason}";
        unknown_pattern_block(block: &str) = "unknown block: {block}";
        invalid_pattern(pattern: &str) = "invalid pattern: {pattern}";
        ascended(levels: impl Display) = "Hopped up {levels} levels!";
        clipboard_flipped(elapsed: Duration) = "Rolled your clipboard copy over ({elapsed:.00?}).";
        clipboard_pasted(elapsed: Duration) = "Unpacked your clipboard onto the plot ({elapsed:.00?}).";
        clipboard_rotated(elapsed: Duration) = "Gave your clipboard copy a twirl ({elapsed:.00?}).";
        copies_stacked(count: impl Display) = "Piled up {count} copies. Rawr!";
        wire_built(count: impl Display) = "Placed {count} blocks.";
        wire_status(count: impl Display) = "{count} blocks | RC build | F plane | Sneak+F bend";
        descended(levels: impl Display) = "Padded down {levels} levels.";
        duplicate_mask_property(key: impl Display) = "{key} left two pawprints in this mask. Specify that property once.";
        flying_speed(speed: impl Display, player: impl Display) = "Flying zoomies set to {speed} for {player}!";
        history_disabled(released: impl Display) = "Tick history disabled; cleared the pawprint trail and freed {released}.";
        history_disabled_for_compilation(released: impl Display) = "Tick history disabled because compiled execution is starting. Cleared the pawprint trail and released approximately {released}.";
        history_enabled(capacity: impl Display, estimated_size: impl Display) = "Recording up to {capacity} game ticks of pawprints. Estimated size: {estimated_size}.";
        history_limit_saved(limit: impl Display) = "The history den's memory allowance is now {limit}.";
        history_memory_permission(memory_permission: impl Display) = "Your paws need {memory_permission} permission to change the tick-history memory limit.";
        history_status(state: impl Display, available: impl Display, capacity: impl Display, uncompressed: impl Display, compressed: impl Display, server_used: impl Display, server_limit: impl Display) = "Tick history: {state}. Available: {available}/{capacity} game ticks.\nUncompressed: {uncompressed} | Compressed: {compressed} | Server: {server_used} / {server_limit}";
        history_stopped(error: impl Display) = "Tick history stopped: {error}";
        history_tick_limit_permission(normal_history_limit: impl Display, unlimited_history_permission: impl Display) = "Keeping more than {normal_history_limit} game ticks of pawprints requires {unlimited_history_permission} permission.";
        invalid_mask_property(key: impl Display, value: impl Display) = "Unknown block property or value: {key}={value}. These paws can't match it.";
        piston_animation(configured: impl Display, effective: impl Display) = "Piston wiggles (animation): {configured} (effective {effective}).";
        player_has_no_plots(player: impl Display) = "No plots bear {player}'s pawprint yet.";
        plot_advanced(unit: impl Display, elapsed: Duration) = "Advanced the ticks forward by {unit} ({elapsed:.00?}).";
        plot_claimed(x: impl Display, z: impl Display) = "Plot {x},{z} is your den now. Awoo!";
        plot_entered(x: impl Display, z: impl Display) = "Pounced into plot ({x}, {z}).";
        plot_index_range(count: impl Display) = "Choose one of this player's plots from 1 through {count}, pup.";
        plot_load_failed(x: impl Display, z: impl Display) = "Could not load plot {x},{z}. Please contact the server administrator.";
        plot_locked(x: impl Display, z: impl Display) = "Locked to plot ({x}, {z}). Use /p unlock to unclip the leash and roam again.";
        plot_owner(owner: impl Display) = "{owner} has their pawprint on this plot.";
        plot_rewound(ticks: impl Display, remaining: impl Display) = "Rewound {ticks} game ticks and paused the plot. {remaining} game ticks remain on the pawprint trail.";
        region_contracted(blocks: impl Display) = "Your region tucked in by {blocks} block(s).";
        region_expanded(blocks: impl Display) = "Your region stretched its paws by {blocks} block(s).";
        region_shifted(blocks: impl Display) = "Scooted your region {blocks} block(s) along.";
        rewind_available(available: impl Display) = "Only {available} game ticks are available to rewind; that's where the pawprint trail ends.";
        rtps_no_data(configured: impl Display) = "&6Haven't caught this circuit's heartbeat yet. No timings data. &a({configured})";
        rtps_report(two_seconds: f32, ten_seconds: f32, one_minute: f32, configured: impl Display) = "&6Circuit heartbeat over 2s, 10s, 1m (TPS): &a{two_seconds:.1}, {ten_seconds:.1}, {one_minute:.1} ({configured})";
        schematic_load_failed(reason: impl Display) = "Could not load schematic: {reason}";
        schematic_loaded(elapsed: Duration) = "Fetched the schematic into your clipboard ({elapsed:.00?}). Give //paste a boop to place it.";
        schematic_save_failed(reason: impl Display) = "Could not save schematic: {reason}";
        schematic_saved(elapsed: Duration) = "Tucked your schematic away successfully ({elapsed:.00?}).";
        search_page_range(pages: impl Display) = "This result trail has pages 1 through {pages}. Pick one of those.";
        search_page_usage(command: &str) = "Follow a result-page trail with {command} -p <page>.";
        search_query_limit(max_query_bytes: impl Display) = "Search text is too long for this snoot. Use at most {max_query_bytes} bytes.";
        search_results(count: impl Display, suffix: impl Display, page: impl Display, pages: impl Display) = "Sniffed out {count} matches{suffix}. Page {page}/{pages}.";
        search_selection_limit(max_scan_blocks: impl Display) = "These search paws can scan at most {max_scan_blocks} blocks. Choose a smaller selection.";
        selection_copied(elapsed: Duration) = "Selection copied to your clipboard ({elapsed:.00?}). Tucked under a paw :3";
        selection_cut(elapsed: Duration) = "Selection cut to your clipboard ({elapsed:.00?}). Paws packed :3";
        selection_first(x: impl Display, y: impl Display, z: impl Display) = "First paw set to ({x}, {y}, {z}) :3";
        selection_moved(elapsed: Duration) = "Nudged your selection into its new spot ({elapsed:.00?}).";
        selection_replaced(elapsed: Duration) = "Swapped your selection's blocks with a careful paw ({elapsed:.00?}).";
        selection_second(x: impl Display, y: impl Display, z: impl Display) = "Second paw set to ({x}, {y}, {z}) :3";
        selection_stacked(elapsed: Duration) = "Made a neat pile of your selection ({elapsed:.00?}). Stack complete.";
        selection_updated(elapsed: Duration) = "Booped the blocks in your selection to update them ({elapsed:.00?}).";
        server_history_usage(used: impl Display, limit: impl Display) = "The server's pawprint store uses {used} / {limit}.";
        stack_block_limit(max_stack_blocks: impl Display) = "This stack would exceed the {max_stack_blocks}-block limit, pup. Reduce the selection or copy count.";
        stack_copy_limit(max_copies: impl Display) = "These stacking paws can make at most {max_copies} copies per command.";
        teleport_coordinates(x: impl Display, y: impl Display, z: impl Display) = "Pouncing to ({x}, {y}, {z})!";
        teleport_player(player: impl Display) = "Following {player}'s scent—teleporting now.";
        unknown_block(name: impl Display) = "Couldn't sniff out a block named {name}.";
        unknown_block_state(id: impl Display) = "Unknown block state ID: {id}. My sniff book has no entry for it.";
        unknown_command(command: impl Display) = "{command} isn't a command trick I know.";
        unknown_direction(token: impl Display) = "{token} isn't a direction these paws recognize.";
        unknown_flag(flag: impl Display) = "Unknown flag: {flag}. Check this command's pawbook with //help <command>.";
        unknown_stack_flag(flag: impl Display) = "Unknown stacking flag: -{flag}. This trick accepts -e and -w.";
        version_mismatch(version: impl Display) = "Version mismatch, I'm on {version}!";
        whitelist_added(player: impl Display) = "{player} has a spot in the whitelist pack now.";
        whitelist_removed(player: impl Display) = "Removed {player}'s pawprint from the whitelist.";
        world_send_rate(configured: impl Display, effective: impl Display) = "World updates trot at {configured} Hz (effective {effective} Hz world send rate).";
        worldedit_completed(blocks: impl Display, elapsed: Duration) = "Operation complete: {blocks} block(s) affected ({elapsed:.00?}). pawjob done :3";
        worldedit_counted(blocks: impl Display, elapsed: Duration) = "Sniffed and counted {blocks} matching block(s) ({elapsed:.00?}).";
        worldedit_help_heading(command: impl Display) = " Pawbook for /{command} ";
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ColorCode;
    use crate::player::PacketSender;
    use mchprs_network::packets::PacketEncoder;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Capture(RefCell<Vec<PacketEncoder>>);

    impl PacketSender for Capture {
        fn send_packet(&self, packet: &PacketEncoder) {
            self.0
                .borrow_mut()
                .push(PacketEncoder::new(packet.buffer.clone(), packet.packet_id));
        }
    }

    #[test]
    fn interpolation_preserves_payloads_without_treating_them_as_templates() {
        let name = "Pup{owner}🐾";
        assert!(plot_owner(name).contains(name));
        assert!(selection_first(-123, 319, 456).contains("(-123, 319, 456)"));
        assert!(
            history_tick_limit_permission(1000, "plots.admin.rewind.unlimited").contains(
                "1000 game ticks of pawprints requires plots.admin.rewind.unlimited permission"
            )
        );
        assert!(
            worldedit_completed(17, Duration::from_millis(123))
                .contains("17 block(s) affected (123ms)")
        );
        let diagnostic = "permission denied: C:\\schems\\{reason}.schem";
        assert_eq!(
            schematic_save_failed(diagnostic),
            format!("Could not save schematic: {diagnostic}")
        );
        let commit_message = "Keep {from} -> {to} as written 🐾";
        assert!(git_committed("a12b34cd", name, commit_message).contains(commit_message));
        let data = r#"{"command":"say {player}","text":"Paw 🐾"}"#;
        assert!(git_block_data_details("from", data, "").contains(data));
    }

    #[test]
    fn feedback_keeps_colors_and_system_packet_position() {
        let capture = Capture::default();
        let success = selection_first(-1, 64, 3);
        capture.send_error_message(INVENTORY_FULL);
        capture.send_system_message(RTPS_SET);
        capture.send_color_message(ColorCode::LightPurple, &success);
        let packets = capture.0.borrow();
        assert_eq!(packets.len(), 3);
        for (packet, text, color) in [
            (&packets[0], INVENTORY_FULL, "red"),
            (&packets[1], RTPS_SET, "yellow"),
            (&packets[2], success.as_str(), "light_purple"),
        ] {
            assert_eq!(packet.packet_id, 0x72);
            // Network NBT omits the root name; restore it for the NBT reader.
            let mut named = vec![packet.buffer[0], 0, 0];
            named.extend_from_slice(&packet.buffer[1..]);
            let mut reader = std::io::Cursor::new(named);
            let component = nbt::Blob::from_reader(&mut reader).unwrap();
            assert_eq!(
                component.get("text"),
                Some(&nbt::Value::String(text.into()))
            );
            assert_eq!(
                component.get("color"),
                Some(&nbt::Value::String(color.into()))
            );
            assert_eq!(&reader.get_ref()[reader.position() as usize..], &[0]);
        }
    }

    #[test]
    fn error_coordinate_highlight_links_survive_network_nbt_encoding() {
        let capture = Capture::default();
        capture.send_error_message(
            "Redpiler: source at BlockPos { x: -20, y: 30, z: 40 } is unsupported.",
        );
        let packets = capture.0.borrow();
        let packet = &packets[0];
        let mut named = vec![packet.buffer[0], 0, 0];
        named.extend_from_slice(&packet.buffer[1..]);
        let component = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).unwrap();
        let Some(nbt::Value::List(parts)) = component.get("extra") else {
            panic!("missing components")
        };
        let nbt::Value::Compound(coordinates) = &parts[1] else {
            panic!("missing coordinates")
        };
        let Some(nbt::Value::Compound(click)) = coordinates.get("click_event") else {
            panic!("missing click event")
        };
        assert_eq!(
            click.get("action"),
            Some(&nbt::Value::String("run_command".into()))
        );
        assert_eq!(
            click.get("command"),
            Some(&nbt::Value::String(
                "/tp -19.5 30.5 40.5 --highlight-only".into()
            ))
        );
        assert_eq!(
            coordinates.get("insertion"),
            Some(&nbt::Value::String(
                "/tp -19.5 30.5 40.5 --highlight".into()
            ))
        );
    }
}
