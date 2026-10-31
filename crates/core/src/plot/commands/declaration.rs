use crate::messages;
use mchprs_network::packets::PacketEncoder;
use mchprs_network::packets::clientbound::{CDeclareCommands, CDeclareCommandsNode as Node, CDeclareCommandsNodeParser as Parser, ClientBoundPacket};
use once_cell::sync::Lazy;
pub static NO_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &[Node::root(&[])],
        root_index: 0,
    }
    .encode()
});

/// The `DeclareCommands` packet that is sent when the player joins.
/// This is used for command autocomplete.
pub(super) fn declared_command_nodes() -> Vec<Node<'static>> {
    let mut nodes = vec![
        // Root Node
        Node::root(&[
            1, 2, 7, 8, 10, 12, 14, 15, 16, 17, 18, 19, 20, 22, 25, 27, 29, 31, 42, 44, 48, 55, 56, 58, 60, 61, 62, 69, 70, 77, 78, 80, 86, 96, 101,
            106, 107, 108, 109, 110, 111, 113, 115, 119, 126, 130, 131, 138, 139, 140, 141, 142, 146, 153, 154, 157, 158, 167,
        ]),
        // /stop
        Node::literal("stop", &[]).executable(),
        // /plot
        Node::literal("plot", &[3, 4, 5, 6, 33, 34, 35, 36, 38, 39, 41, 53, 54, 75, 76, 120, 121, 122, 124]),
        // /plot info
        Node::literal("info", &[]).executable(),
        // /plot i
        Node::redirect("i", 3),
        // /plot claim
        Node::literal("claim", &[]).executable(),
        // /plot c
        Node::redirect("c", 5),
        // /p
        Node::redirect("p", 2),
        // /tps
        Node::literal("tps", &[9, 112]).executable(),
        // /tps [tps]
        Node::argument("tps", Parser::Integer(0, i32::MAX), &[]).executable(),
        // //pos1
        Node::literal("/pos1", &[11]).executable(),
        // //pos1 [pos]
        Node::argument("pos", Parser::BlockPos, &[]).executable(),
        // //pos2
        Node::literal("/pos2", &[13]).executable(),
        // //pos2 [pos]
        Node::argument("pos", Parser::BlockPos, &[]).executable(),
        // /1
        Node::redirect("/1", 10),
        // /2
        Node::redirect("/2", 12),
        // //copy
        Node::literal("/copy", &[]).executable(),
        // //c
        Node::redirect("/c", 16),
        // //paste
        Node::literal("/paste", &[]).executable(),
        // //p
        Node::redirect("/p", 18),
        // //set
        Node::literal("/set", &[21]),
        // //set [block]
        Node::argument("block", Parser::String(0), &[]).executable(),
        // //replace
        Node::literal("/replace", &[23]),
        // //replace [oldblock]
        Node::argument("oldblock", Parser::BlockState, &[24]),
        // //replace [oldblock] [newblock]
        Node::argument("newblock", Parser::BlockState, &[]).executable(),
        // /adv
        Node::literal("adv", &[26, 71, 73]),
        // /adv [rticks]
        Node::argument("rticks", Parser::Integer(0, 100000), &[]).executable(),
        // /speed
        Node::literal("speed", &[28]),
        // /speed [speed]
        Node::argument("speed", Parser::Float(0.0, 10.0), &[]).executable(),
        // //stack
        Node::literal("/stack", &[30]).executable(),
        // //stack [amount]
        Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
        // //undo
        Node::literal("/undo", &[]).executable(),
        // //sel
        Node::literal("/sel", &[]).executable(),
        // /p auto
        Node::literal("auto", &[]).executable(),
        // /p a
        Node::redirect("a", 5),
        // /p middle
        Node::literal("middle", &[]).executable(),
        // /p visit
        Node::literal("visit", &[37]),
        // /p visit [player]
        Node::argument("player", Parser::Entity(3), &[]).executable(),
        // /p v
        Node::redirect("v", 36),
        // /p teleport
        Node::literal("teleport", &[40]),
        // /p teleport [x, z]
        Node::argument("x, z", Parser::Vec2, &[]).executable(),
        // /p tp
        Node::redirect("tp", 39),
        // //shift
        Node::literal("/shift", &[30]).executable(),
        // //shift [amount]
        Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
        // /whitelist
        Node::literal("whitelist", &[45, 46]),
        // /whitelist add
        Node::literal("add", &[47]),
        // /whitelist remove
        Node::literal("remove", &[47]),
        // /whitelist add|remove [username]
        Node::argument("username", Parser::Entity(3), &[]).executable(),
        // /container <type> <power>
        Node::literal("container", &[49, 50, 51]),
        Node::argument("type", Parser::String(0), &[52]).suggestions("minecraft:ask_server"),
        // Preserve the existing literal alternatives and their node indices.
        Node::literal("hopper", &[52]),
        Node::literal("furnace", &[52]),
        Node::argument("power", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // /plot lock
        Node::literal("lock", &[]).executable(),
        // /plot unlock
        Node::literal("unlock", &[]).executable(),
        // //wand
        Node::literal("/wand", &[]).executable(),
        // //save
        Node::literal("/save", &[57]),
        // //save [filename]
        Node::argument("filename", Parser::String(0), &[]).executable(),
        // //load
        Node::literal("/load", &[59]),
        // //load [filename]
        Node::argument("filename", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // /toggleautorp
        Node::literal("toggleautorp", &[]).executable(),
        // /redpiler
        Node::literal("redpiler", &[63, 65, 143, 145]),
        // /rp
        Node::redirect("rp", 61),
        // /redpiler compile
        Node::literal("compile", &[129]).executable(),
        // Reserved protocol node ID; the inspection command was removed.
        Node::literal("reserved", &[]),
        // /redpiler reset
        Node::literal("reset", &[]).executable(),
        // /worldsendrate
        Node::literal("worldsendrate", &[67]).executable(),
        // /worldsendrate [rticks]
        Node::argument("hertz", Parser::Integer(0, 1000), &[]).executable(),
        // /wsr
        Node::redirect("wsr", 66),
        // /curse
        Node::literal("curse", &[]),
        // /bless
        Node::literal("bless", &[]),
        // /adv nano
        Node::literal("nano", &[72]),
        // /adv nano [nticks]
        Node::argument("nticks", Parser::Integer(0, 100000), &[]).executable(),
        // /adv pico
        Node::literal("pico", &[74]),
        // /adv pico [pticks]
        Node::argument("pticks", Parser::Integer(0, 100000), &[]).executable(),
        // /plot select
        Node::literal("select", &[]).executable(),
        // /plot sel
        Node::redirect("sel", 75),
        // /version
        Node::literal("version", &[]).executable(),
        // /say <message>, /tellraw <targets> <JSON text>
        Node::literal("say", &[79]),
        Node::argument("message", Parser::String(2), &[]).executable(),
        Node::literal("tellraw", &[81]),
        Node::argument("targets", Parser::Entity(2), &[82]),
        Node::argument("message", Parser::String(2), &[]).executable(),
        // animation preference and alias
        Node::literal("piston_anim", &[84]).executable(),
        Node::argument("mode", Parser::String(0), &[]).executable(),
        Node::redirect("bisdon_anim", 87).executable(),
        // 90–99: /help and its topic suggestions
        Node::literal("help", &[91, 92, 93, 94, 95, 96, 97, 98, 99]).executable(),
        Node::argument("topic", Parser::String(0), &[]).executable(),
        Node::literal("plots", &[]).executable(),
        Node::literal("tps", &[]).executable(),
        Node::literal("we", &[]).executable(),
        Node::literal("schematics", &[]).executable(),
        Node::literal("pistons", &[]).executable(),
        Node::literal("rewind", &[]).executable(),
        Node::literal("chat", &[]).executable(),
        Node::literal("redpiler", &[]).executable(),
        // tick history and whole-game-tick rewind
        Node::literal("rhistory", &[97, 99, 100, 103]).executable(),
        Node::literal("on", &[98]).executable(),
        Node::argument(messages::ADV_GAME_TICKS_LABEL, Parser::Integer(1, i32::MAX), &[]).executable(),
        Node::literal("off", &[]).executable(),
        Node::literal("status", &[]).executable(),
        Node::literal("rev", &[102]).executable(),
        Node::argument(messages::ADV_GAME_TICKS_LABEL, Parser::Integer(1, i32::MAX), &[]).executable(),
        // server history memory limit, in MiB
        Node::literal("limit", &[104]).executable(),
        Node::argument("MiB", Parser::Integer(0, i32::MAX), &[]).executable(),
        // flexible tool arguments, validated by the command handler
        Node::argument("arguments", Parser::String(2), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // //find
        Node::literal("/find", &[105]).executable(),
        // //signsearch
        Node::literal("/signsearch", &[105]).executable(),
        // //ss
        Node::literal("/ss", &[105]).executable(),
        // //rstack
        Node::literal("/rstack", &[105]).executable(),
        // //rs
        Node::literal("/rs", &[105]).executable(),
        // /cursel
        Node::literal("cursel", &[105]).executable(),
        // /tps timings
        Node::literal("timings", &[]).executable(),
        // //update
        Node::literal("/update", &[114]).executable(),
        // //update -p
        Node::literal("-p", &[]).executable(),
        // //invalidatecaches
        Node::literal("/invalidatecaches", &[]).executable(),
        // /screenonly [on|off]
        Node::literal("screenonly", &[117, 118]).executable(),
        Node::literal("on", &[]).executable(),
        Node::literal("off", &[]).executable(),
        // /autostack
        Node::literal("autostack", &[105]).executable(),
        // /p home, /p h, /p add <nick>, /p remove <nick>.
        Node::literal("home", &[]).executable(),
        Node::redirect("h", 120),
        Node::literal("add", &[123]),
        Node::argument("nick", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        Node::literal("remove", &[125]),
        Node::argument("nick", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // /git uses the existing greedy, server-completed arguments node.
        Node::literal("git", &[105]).executable(),
        Node::argument("options", Parser::String(2), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // Reserved protocol node IDs; the removed command nodes stay unused.
        Node::literal("reserved", &[]),
        Node::literal("reserved", &[]),
        // gamemode alias, names, IDs and legacy shortcuts.
        Node::redirect("gm", 131),
        Node::literal("gamemode", &[132, 133, 134, 135, 136, 137, 161]),
        Node::literal("creative", &[]).executable(),
        Node::literal("adventure", &[]).executable(),
        Node::literal("spectator", &[]).executable(),
        Node::literal("1", &[]).executable(),
        Node::literal("2", &[]).executable(),
        Node::literal("3", &[]).executable(),
        Node::literal("gmc", &[]).executable(),
        Node::literal("gmsp", &[]).executable(),
        // clear the WorldEdit selection.
        Node::redirect("/desel", 32).executable(),
        Node::redirect("desel", 32).executable(),
        // /set accepts the same block patterns as //set.
        Node::redirect("set", 20),
        // Redpiler subcommand aliases share arguments and suggestions.
        Node::redirect("c", 63).executable(),
        Node::redirect("i", 64).executable(),
        Node::redirect("r", 65).executable(),
        // administrator diagnostics and optional page/plot arguments.
        Node::literal("serverinfo", &[147, 149, 152]).executable(),
        Node::literal("plots", &[148]).executable(),
        Node::argument("page", Parser::Integer(1, i32::MAX), &[]).executable(),
        Node::literal("plot", &[150]).executable(),
        Node::argument("x", Parser::Integer(i32::MIN, i32::MAX), &[151]),
        Node::argument("z", Parser::Integer(i32::MIN, i32::MAX), &[]).executable(),
        Node::literal("settings", &[]).executable(),
        // wire pen, alias, and aiming modes reuse the existing off literal.
        Node::literal("wire", &[155, 156, 99]).executable(),
        Node::redirect("/wire", 153).executable(),
        Node::literal("free", &[]).executable(),
        Node::literal("plane", &[]).executable(),
        // /rv shares the server-completed git arguments.
        Node::literal("rv", &[105]).executable(),
        // /small and its explicit states.
        Node::literal("small", &[159, 160, 162, 163, 164, 165, 166]).executable(),
        Node::literal("on", &[]).executable(),
        Node::literal("off", &[]).executable(),
        // /gm cat selects creative play with the small ocelot disguise.
        Node::literal("cat", &[]).executable(),
        // saved small-mode animal choices.
        Node::literal("wolf", &[]).executable(),
        Node::literal("fox", &[]).executable(),
        Node::literal("cat", &[]).executable(),
        Node::literal("ocelot", &[]).executable(),
        Node::literal("baby", &[]).executable(),
    ];

    let visual = nodes.len();
    nodes.push(Node::literal("visual", &[]).executable());
    let pistons = nodes.len();
    nodes.push(Node::literal("pistons", &[]).executable());
    let auto = nodes.len();
    nodes.push(Node::literal("auto", &[]).executable());
    let piston_on = nodes.len();
    nodes.push(Node::literal("on", &[]).executable());
    let piston_off = nodes.len();
    nodes.push(Node::literal("off", &[]).executable());
    let rate = nodes.len();
    nodes.push(Node::literal("rate", &[]).executable());
    let hertz = nodes.len();
    nodes.push(Node::argument("hertz", Parser::Integer(0, 1000), &[]).executable());
    let displayonly = nodes.len();
    nodes.push(Node::literal("displayonly", &[]).executable());
    let display_on = nodes.len();
    nodes.push(Node::literal("on", &[]).executable());
    let display_off = nodes.len();
    nodes.push(Node::literal("off", &[]).executable());
    let visual_help = nodes.len();
    nodes.push(Node::literal("visual", &[]).executable());

    nodes[visual].children = Box::leak(vec![pistons as i32, rate as i32, displayonly as i32].into_boxed_slice());
    nodes[pistons].children = Box::leak(vec![auto as i32, piston_on as i32, piston_off as i32].into_boxed_slice());
    nodes[rate].children = Box::leak(vec![hertz as i32].into_boxed_slice());
    nodes[displayonly].children = Box::leak(vec![display_on as i32, display_off as i32].into_boxed_slice());

    // Keep the final `/help visual` topic linked if earlier command nodes move.
    let help = nodes.iter().position(|node| node.name == Some("help")).unwrap();
    let mut help_children = nodes[help].children.to_vec();
    help_children.push(visual_help as i32);
    help_children.push(nodes.len() as i32);
    nodes.push(Node::literal("waveform", &[]).executable());
    nodes[help].children = Box::leak(help_children.into_boxed_slice());

    let redbox = crate::redbox::enabled();
    let mut root_children = nodes[0].children.to_vec();
    let hl = nodes.len() as i32;
    let hl_args = hl + 1;
    nodes.push(Node::literal("hl", Box::leak(vec![hl_args].into_boxed_slice())).executable());
    nodes.push(Node::argument("arguments", Parser::String(2), &[]).executable());
    root_children.push(hl);
    let history = nodes.iter().position(|node| node.name == Some("rhistory")).unwrap();
    let mut history_children = nodes[history].children.to_vec();
    let rewind = nodes.len() as i32;
    let rewind_ticks = rewind + 1;
    nodes.push(Node::literal("rewind", Box::leak(vec![rewind_ticks].into_boxed_slice())).executable());
    nodes.push(Node::argument(messages::ADV_GAME_TICKS_LABEL, Parser::Integer(1, i32::MAX), &[]).executable());
    history_children.push(rewind);
    nodes[history].children = Box::leak(history_children.into_boxed_slice());

    if redbox {
        root_children.retain(|&id| !matches!(nodes[id as usize].name, Some("teleport" | "tp")));
    } else {
        let tp = nodes.len() as i32;
        let player = tp + 1;
        let position = tp + 2;
        nodes.push(Node::literal("tp", Box::leak(vec![player, position].into_boxed_slice())));
        nodes.push(
            Node::argument("player", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
        );
        nodes.push(Node::argument("position", Parser::Vec3, &[]).executable());
        let teleport = nodes.len() as i32;
        nodes.push(Node::redirect("teleport", tp));
        root_children.extend([tp, teleport]);

        let warp = nodes.len() as i32;
        let warp_name = warp + 1;
        nodes.push(Node::literal("warp", Box::leak(vec![warp_name].into_boxed_slice())).executable());
        nodes.push(
            Node::argument("name", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
        );
        let setwarp = nodes.len() as i32;
        let setwarp_name = setwarp + 1;
        nodes.push(Node::literal("setwarp", Box::leak(vec![setwarp_name].into_boxed_slice())));
        nodes.push(
            Node::argument("name", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
        );
        root_children.extend([warp, setwarp]);
    }
    root_children.push(super::super::waveform::command_node(&mut nodes));
    nodes[0].children = Box::leak(root_children.into_boxed_slice());
    nodes
}

pub static DECLARE_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &declared_command_nodes(),
        root_index: 0,
    }
    .encode()
});
