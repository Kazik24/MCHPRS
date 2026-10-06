//! Short tutorials for the commands players use most often.

use crate::messages;

pub(super) fn page(topic: Option<&str>) -> Option<&'static str> {
    let topic = topic
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    Some(match topic.as_str() {
        "" => messages::HELP_QUICK_START,
        "plots" | "plot" | "p" => messages::HELP_PLOTS,
        "tps" | "adv" | "rtps" | "ticks" | "radvance" | "radv" => messages::HELP_TICKS,
        "we" | "worldedit" => messages::HELP_WORLD_EDIT,
        "tools" | "redstonetools" | "find" | "signsearch" | "ss" | "rstack" | "rs"
        | "autostack" | "container" | "cursel" => messages::HELP_REDSTONE_TOOLS,
        "schematics" | "schematic" | "load" | "save" => messages::HELP_SCHEMATICS,
        "screenonly" | "screens" => messages::HELP_SCREEN_ONLY,
        "pistons" | "animations" | "piston_anim" | "bisdon_anim" | "wsr" | "worldsendrate" => {
            messages::HELP_PISTONS
        }
        "back" | "rewind" | "history" | "tick_rewind" | "rhistory" | "rback" => {
            messages::HELP_HISTORY
        }
        "chat" | "commands" | "commandblocks" | "command_blocks" | "say" | "tellraw" => {
            messages::HELP_CHAT
        }
        "redpiler" | "rp" => messages::HELP_REDPILER,
        "git" => super::git::HELP,
        _ => return None,
    })
}
