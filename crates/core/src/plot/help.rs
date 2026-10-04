//! Short tutorials for the commands players use most often.

pub(super) fn page(topic: Option<&str>) -> Option<&'static str> {
    let topic = topic
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    Some(match topic.as_str() {
        "" => "MCHPRS quick start\n\
Claim a plot with /p auto.\n\
/help plots - Claim, visit and find your plot.\n\
/help rtps - Pause, speed up and step through a circuit.\n\
/help we - Select, copy, paste and undo.\n\
/help schematics - Load and save schematics.\n\
/help pistons - Animation and client update settings.\n\
/help rewind - tick rewind commands.\n\
/help chat - Messages and command blocks.\n\
/help redpiler - Compiled simulation.",
        "plots" | "plot" | "p" => "Plots\n\
/p auto claim an empty plot. /p claim claims the plot you're standing in.\n\
/p info shows its owner; /p middle takes you to its centre.\n\
/p visit <player> [number] visits one of that player's plots.\n\
/p tp <x> <z> goes to plot coordinates, not block coordinates.\n\
/p lock keeps you in this plot; /p unlock lets you leave.\n\
/p select selects the whole plot for WorldEdit. Editing may require ownership or permission.",
        "rtps" | "ticks" | "radvance" | "radv" => "Tick control\n\
/rtps shows the current speed. /rtps 20 runs at normal game speed.\n\
/rtps 0 pauses the tickrate. /rtps 1000 speeds it up; /rtps unlimited runs as fast as it can.\n\
/radvance Advances one game tick if rtps is 0. /radv is an alias.\n\
/radvance 10 advances ten game ticks. \n\
/radvance nano 1 advances a batch of work (One \"Nanotick\"; /radvance pico 1 advances one operation (One \"Picotick\")\n\
",
        "we" | "worldedit" => "WorldEdit\n\
This server supports subset of WorldEdit commands, check autofill with // to check what is available.\n
",
        "schematics" | "schematic" | "load" | "save" => "Schematics\n\
//load my_schematic.schem reads a schematic into your clipboard. \n\
Use //paste to place it where you're standing\n\
To save a build: select it, //copy, then //save my_schematic.schem.\n\
You can load schematics from redstonefun server under rf/ folder.  \n\
",
        "pistons" | "animations" | "piston_anim" | "bisdon_anim" | "wsr" | "worldsendrate" => "Animations and client updates\n\
/piston_anim shows this plot's setting. /bisdon_anim is an alias.\n\
/piston_anim auto follows the server threshold: by default, animations turn off above 100 TPS.\n\
/piston_anim on keeps animations on. /piston_anim off shows static blocks at any speed.\n\
This affects only clients animations, piston tick and update behaviour stays the same.\n\
/wsr shows configured and effective block-update rates. /wsr 20 sends up to 20 updates per second; /wsr 0 stops periodic block updates.\n\
Static rendering caps the send rate at 10 per second by default. A lower /wsr still applies.",
        "rewind" | "history" | "tick_rewind" | "rhistory" | "rback" => "Tick rewind\n\
/rhistory on [ticks] starts recording; the default is 100 ticks.\n\
/rhistory shows ticks and compressed/uncompressed sizes. /rhistory off frees the buffer.\n\
/rback rewinds one tick; /rback 10 rewinds ten. Rewind pauses the plot and clears WorldEdit undo/redo. Use /rtps 20 to resume.\n\
All plots share a 2 GiB memory limit by default. Older ticks drop when memory fills. Admins can use /rhistory limit <MiB> to change it.\n\
More than 1000 ticks requires plots.admin.rewind.unlimited; changing memory requires plots.admin.rewind.memory. Grant these through LuckPerms.\n\
History needs the interpreter and whole ticks. Compilation or restart clears it.\n\
Try /rtps 0, /rhistory on, /radvance 10, then /rback 5.",
        "chat" | "commands" | "commandblocks" | "command_blocks" | "say" | "tellraw" => "Messages and command blocks\n\
/say Hello everyone sends a message to all players.\n\
/tellraw @a {\"text\":\"Hello\",\"color\":\"gold\",\"bold\":true} sends formatted text.\n\
Use @a for everyone, @s for yourself, or a player name. Selector filters are ignored. Text, colours, basic styles and extra components work; scoreboard text and click/hover actions are ignored.\n\
Place a command block, open it and enter say or tellraw. Impulse blocks run on a redstone pulse; repeating and chain blocks are also supported. Editing requires creative mode and permission.\n\
Command blocks can use @a or player names; @s has no player there. Other commands are kept in schematics but don't run.",
        "redpiler" | "rp" => "Redpiler\n\
/rp compile compiles a circuit for faster simulation. /rp reset returns to the interpreter.\n\
/rp inspect inspects the block you're looking at. /toggleautorp toggles automatic compilation for this plot.\n\
Plots with pistons, observers or command blocks stay on the interpreter to preserve their behaviour.\n\
Use the interpreter for nano/pico stepping. Start with /rtps 0, then /rp reset and /radvance pico 1.",
        _ => return None,
    })
}
