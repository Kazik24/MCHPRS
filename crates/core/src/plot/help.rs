//! Short tutorials for the commands players use most often.

pub(super) fn page(topic: Option<&str>) -> Option<&'static str> {
    let topic = topic
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    Some(match topic.as_str() {
        "" => "MCHPRS quick start\n\
Claim a plot with /p auto, then build your circuit. Middle-click picks a block; Ctrl-middle-click also copies its data.\n\
/help plots - Claim, visit and find your plot.\n\
/help rtps - Pause, speed up and step through a circuit.\n\
/help we - Select, copy, paste and undo.\n\
/help schematics - Load and save schematics.\n\
/help pistons - Animation and client update settings.\n\
/help rewind - tick rewind commands.\n\
/help chat - Messages and command blocks.\n\
/help redpiler - Compiled simulation.",
        "plots" | "plot" | "p" => "Plots\n\
/p auto finds and claims an empty plot. /p claim claims the plot you're standing in.\n\
/p info shows its owner; /p middle takes you to its centre.\n\
/p visit <player> [number] visits one of that player's plots.\n\
/p tp <x> <z> goes to plot coordinates, not block coordinates.\n\
/p lock keeps you in this plot; /p unlock lets you leave.\n\
/p select selects the whole plot for WorldEdit. Editing may require ownership or permission.",
        "rtps" | "ticks" | "radvance" | "radv" => "Tick control\n\
/rtps shows the current speed. /rtps 20 runs at normal game speed.\n\
/rtps 0 pauses the plot. /rtps 1000 speeds it up; /rtps unlimited runs as fast as it can.\n\
To inspect a circuit: pause it, change an input, then use /radvance 1. Repeat to watch each game tick.\n\
/radvance 10 advances ten game ticks. Two game ticks make one redstone tick. /radv is an alias.\n\
/radvance nano 1 advances a batch of work; /radvance pico 1 advances one operation. These require the interpreter: /rp reset stops compiled simulation.\n\
Use /rtps 20 to resume. Speed and stepping affect the whole plot.",
        "we" | "worldedit" => "WorldEdit\n\
Use //wand: left-click one corner and right-click the other. You can also stand at each corner and use //pos1 and //pos2.\n\
//set stone fills the selection; //replace stone glass changes only the matching blocks.\n\
//copy copies the selection relative to where you're standing. Move, then //paste to place it at the same offset.\n\
//paste -a skips air; //paste -u also updates the pasted blocks.\n\
//undo undoes your last edit; //redo brings it back. //sel clears the selection.\n\
For a command's arguments and flags, use //help paste or //help set. /help schematics covers files.",
        "schematics" | "schematic" | "load" | "save" => "Schematics\n\
//load my_circuit.schem reads a schematic into your clipboard. It doesn't place any blocks yet.\n\
Use //paste to place it where you're standing, or //paste -a to leave existing blocks where the schematic has air.\n\
To save a build: select it, //copy, then //save my_circuit.schem.\n\
Include the file extension. Names are relative to the server's schems folder; subfolders work, for example //load rf/my_circuit.schem.\n\
Sponge v2 and v3 files are supported. //save writes v3. Signs and supported command-block data are kept.\n\
Use //undo if you paste in the wrong place.",
        "pistons" | "animations" | "piston_anim" | "bisdon_anim" | "wsr" | "worldsendrate" => "Animations and client updates\n\
/piston_anim shows this plot's setting. /bisdon_anim is an alias.\n\
/piston_anim auto follows the server threshold: by default, animations turn off above 100 TPS or at unlimited speed.\n\
/piston_anim on keeps animations on. /piston_anim off shows static blocks at any speed. The choice is saved per plot.\n\
This changes what clients see; piston tick and update behaviour stays the same.\n\
/wsr shows configured and effective block-update rates. /wsr 20 sends up to 20 updates per second; /wsr 0 stops periodic block updates.\n\
Static rendering caps the send rate at 10 per second by default. A lower /wsr still applies.",
        "rewind" | "history" | "tick_rewind" | "rhistory" | "rback" => "Tick rewind (planned; these commands are not available yet)\n\
/rhistory on [ticks] will record whole game ticks, keeping 1000 by default. /rhistory on 2000 will start a fresh 2000-tick history.\n\
/rhistory will show available ticks and approximate memory use. /rhistory off will stop recording and free the history.\n\
/rback will go back one game tick; /rback 10 will go back exactly ten. A successful rewind will pause the plot.\n\
Rewind will restore the entire plot, including edits made after that tick, and clear WorldEdit undo/redo. Players and inventories won't move back.\n\
History will use the interpreter only, with no nano/pico stepping while recording. It won't survive restart.\n\
For now, use /rtps 0 and /radvance 1 to inspect a circuit going forward.",
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
