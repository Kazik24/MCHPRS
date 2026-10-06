//! [Worldedit](https://github.com/EngineHub/WorldEdit) and [RedstoneTools](https://github.com/paulikauro/RedstoneTools) implementation
use crate::messages;

mod execute;
mod safety;
mod schematic;
mod schematic_paths;
pub(super) use schematic_paths::complete_names as complete_schematic_names;
#[cfg(test)]
mod stack_tests;
#[cfg(test)]
mod update_tests;

use super::{Plot, PlotWorld};
use crate::player::{PacketSender, Player, PlayerPos};
use crate::redstone;
use crate::world::storage::PalettedBitBuffer;
use crate::world::World;
use execute::*;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFacing, BlockPos};
use mchprs_utils::map;
use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

pub use schematic::*;

// Attempts to execute a worldedit command. Returns true of the command was handled.
// The command is not handled if it is not found in the worldedit commands and alias lists.
pub fn execute_command(
    plot: &mut Plot,
    player_idx: usize,
    command: &str,
    args: &mut Vec<&str>,
) -> bool {
    let player = &mut plot.players[player_idx];
    let (command_name, command) = if let Some(definition) = COMMANDS.get(command) {
        (command, definition)
    } else if let Some(command) = ALIASES.get(command) {
        let mut alias: Vec<&str> = command.split(' ').collect();
        let command = alias.remove(0);
        args.append(&mut alias);
        (command, &COMMANDS[command])
    } else {
        return false;
    };

    let allowed = if crate::permissions::dedicated_permissions() {
        player.can_edit_plot(plot.owner, (plot.world.x, plot.world.z))
    } else {
        player.has_permission("plots.worldedit.bypass")
            || plot.owner == Some(player.uuid)
            || (plot.owner.is_some()
                && super::database::is_plot_member(plot.world.x, plot.world.z, player.uuid))
    };
    if !allowed {
        player.send_no_permission_message();
        return true;
    }

    if !command.permission_node.is_empty() && !player.has_permission(command.permission_node) {
        player.send_no_permission_message();
        return true;
    }

    if command.requires_positions {
        let plot_x = plot.world.x;
        let plot_z = plot.world.z;
        if player.first_position.is_none() || player.second_position.is_none() {
            player.send_error_message(messages::SELECTION_REQUIRED);
            return true;
        }
        let first_pos = player.first_position.unwrap();
        let second_pos = player.second_position.unwrap();
        if !(0..super::PLOT_BLOCK_HEIGHT).contains(&first_pos.y)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&second_pos.y)
        {
            player.send_error_message(messages::SELECTION_OUTSIDE_WORLD_HEIGHT);
            return true;
        }
        if !Plot::in_plot_bounds(plot_x, plot_z, first_pos.x, first_pos.z) {
            player.send_system_message(messages::FIRST_POSITION_OUTSIDE_PLOT_BOUNDS);
            return true;
        }
        if !Plot::in_plot_bounds(plot_x, plot_z, second_pos.x, second_pos.z) {
            player.send_system_message(messages::SECOND_POSITION_OUTSIDE_PLOT_BOUNDS);
            return true;
        }
    }

    if command.requires_clipboard && player.worldedit_clipboard.is_none() {
        player.send_error_message(messages::CLIPBOARD_EMPTY);
        return true;
    }

    let flag_descs = command.flags;

    let mut ctx_flags = Vec::new();
    let mut arg_removal_idxs = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if arg.starts_with('-') {
            let mut with_argument = false;
            let flags = arg.chars();
            for flag in flags.skip(1) {
                if with_argument {
                    player.send_error_message(messages::FLAG_ARGUMENT_MUST_LAST_GROUPING);
                    return true;
                }
                let flag_desc = if let Some(desc) = flag_descs.iter().find(|d| d.letter == flag) {
                    desc
                } else {
                    player.send_error_message(&messages::unknown_flag(flag));
                    return true;
                };
                arg_removal_idxs.push(i);
                if flag_desc.argument_type.is_some() {
                    if i + 1 >= args.len() {
                        player.send_error_message(messages::FLAG_REQUIRES_AN_ARGUMENT);
                        return true;
                    }
                    arg_removal_idxs.push(i + 1);
                    with_argument = true;
                }
                ctx_flags.push(flag);
            }
        }
    }

    arg_removal_idxs.sort_unstable();
    arg_removal_idxs.dedup();
    for idx in arg_removal_idxs.iter().rev() {
        args.remove(*idx);
    }

    let arg_descs = command.arguments;

    if args.len() > arg_descs.len() {
        player.send_error_message(messages::TOO_MANY_ARGUMENTS);
        return true;
    }

    let mut arguments = Vec::new();
    for (i, arg_desc) in arg_descs.iter().enumerate() {
        let arg = args.get(i).copied();
        match Argument::parse(player, arg_desc, arg) {
            Ok(default_arg) => arguments.push(default_arg),
            Err(err) => {
                player.send_error_message(&err.to_string());
                return true;
            }
        }
    }
    if let Err(error) = safety::validate_request(
        &plot.world,
        &plot.players[player_idx],
        command_name,
        command,
        &arguments,
    ) {
        plot.players[player_idx].send_error_message(&error);
        return true;
    }
    if command.mutates_world {
        plot.reset_redpiler();
    }
    let previous = command
        .mutates_world
        .then(|| plot.world.set_authoritative_updates(true));
    let ctx = CommandExecuteContext {
        plot: &mut plot.world,
        player: &mut plot.players[player_idx],
        arguments,
        flags: ctx_flags,
    };
    (command.execute_fn)(ctx);
    if let Some(previous) = previous {
        plot.world.flush_block_changes();
        plot.world.set_authoritative_updates(previous);
    }
    trim_history(&mut plot.players[player_idx]);
    true
}

#[derive(Debug)]
struct ArgumentParseError {
    arg_type: ArgumentType,
    reason: String,
}

impl ArgumentParseError {
    fn new(arg_type: ArgumentType, reason: &str) -> ArgumentParseError {
        ArgumentParseError {
            arg_type,
            reason: String::from(reason),
        }
    }
}

impl fmt::Display for ArgumentParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&messages::argument_error(self.arg_type, &self.reason))
    }
}

impl std::error::Error for ArgumentParseError {}

type ArgumentParseResult = Result<Argument, ArgumentParseError>;

#[derive(Copy, Clone, Debug)]
enum ArgumentType {
    UnsignedInteger,
    Direction,
    Mask,
    Pattern,
    String,
    ContainerType,
}

#[derive(Debug, Clone)]
enum Argument {
    UnsignedInteger(u32),
    Direction(BlockFacing),
    Pattern(WorldEditPattern),
    Mask(WorldEditPattern),
    String(String),
    ContainerType(ContainerType),
}

impl Argument {
    fn unwrap_uint(&self) -> u32 {
        match self {
            Argument::UnsignedInteger(val) => *val,
            _ => panic!("Argument was not an UnsignedInteger"),
        }
    }

    fn unwrap_direction(&self) -> BlockFacing {
        match self {
            Argument::Direction(val) => *val,
            _ => panic!("Argument was not an Direction"),
        }
    }

    fn unwrap_pattern(&self) -> &WorldEditPattern {
        match self {
            Argument::Pattern(val) => val,
            _ => panic!("Argument was not a Pattern"),
        }
    }

    fn unwrap_mask(&self) -> &WorldEditPattern {
        match self {
            Argument::Mask(val) => val,
            _ => panic!("Argument was not a Mask"),
        }
    }

    fn unwrap_string(&self) -> &String {
        match self {
            Argument::String(val) => val,
            _ => panic!("Argument was not a String"),
        }
    }

    fn unwrap_container_type(&self) -> ContainerType {
        match self {
            Argument::ContainerType(val) => *val,
            _ => panic!("Container type must be one of [barrel, furnace, hopper]"),
        }
    }

    fn get_default(player: &Player, desc: &ArgumentDescription) -> ArgumentParseResult {
        if let Some(default) = &desc.default {
            return Ok(default.clone());
        }

        let arg_type = desc.argument_type;
        match arg_type {
            ArgumentType::Direction => Argument::parse(player, desc, Some("me")),
            ArgumentType::UnsignedInteger => Ok(Argument::UnsignedInteger(1)),
            _ => Err(ArgumentParseError::new(
                arg_type,
                messages::ARGUMENT_CANNOT_INFER,
            )),
        }
    }

    fn parse(
        player: &Player,
        desc: &ArgumentDescription,
        arg: Option<&str>,
    ) -> ArgumentParseResult {
        if arg.is_none() {
            return Argument::get_default(player, desc);
        }
        let arg = arg.unwrap();
        let arg_type = desc.argument_type;
        match arg_type {
            ArgumentType::Direction => {
                let player_facing = player.get_facing();
                Ok(Argument::Direction(match arg {
                    "me" => player_facing,
                    "u" | "up" => BlockFacing::Up,
                    "d" | "down" => BlockFacing::Down,
                    "n" | "north" => BlockFacing::North,
                    "s" | "south" => BlockFacing::South,
                    "e" | "east" => BlockFacing::East,
                    "w" | "west" => BlockFacing::West,
                    "l" | "left" => player_facing.rotate_ccw(),
                    "r" | "right" => player_facing.rotate(),
                    _ => {
                        return Err(ArgumentParseError::new(
                            arg_type,
                            messages::ARGUMENT_UNKNOWN_DIRECTION,
                        ))
                    }
                }))
            }
            ArgumentType::UnsignedInteger => match arg.parse::<u32>() {
                Ok(num) => Ok(Argument::UnsignedInteger(num)),
                Err(_) => Err(ArgumentParseError::new(
                    arg_type,
                    messages::ARGUMENT_INVALID_UINT,
                )),
            },
            ArgumentType::Pattern => match WorldEditPattern::from_str(arg) {
                Ok(pattern) => Ok(Argument::Pattern(pattern)),
                Err(err) => Err(ArgumentParseError::new(arg_type, &err.to_string())),
            },
            // Masks are net yet implemented, so in the meantime they can be treated as patterns
            ArgumentType::Mask => match WorldEditPattern::from_str(arg) {
                Ok(pattern) => Ok(Argument::Mask(pattern)),
                Err(err) => Err(ArgumentParseError::new(arg_type, &err.to_string())),
            },
            ArgumentType::String => Ok(Argument::String(arg.to_owned())),
            ArgumentType::ContainerType => match arg.parse::<ContainerType>() {
                Ok(ty) => Ok(Argument::ContainerType(ty)),
                Err(_) => Err(ArgumentParseError::new(
                    arg_type,
                    messages::ARGUMENT_INVALID_CONTAINER,
                )),
            },
        }
    }
}

struct ArgumentDescription {
    name: &'static str,
    argument_type: ArgumentType,
    description: &'static str,
    default: Option<Argument>,
}

macro_rules! argument {
    ($name:literal, $type:ident, $desc:expr) => {
        ArgumentDescription {
            name: $name,
            argument_type: ArgumentType::$type,
            description: $desc,
            default: None,
        }
    };
    ($name:literal, $type:ident, $desc:expr, $default:literal) => {
        ArgumentDescription {
            name: $name,
            argument_type: ArgumentType::$type,
            description: $desc,
            default: Some(Argument::$type($default)),
        }
    };
}

struct FlagDescription {
    letter: char,
    argument_type: Option<ArgumentType>,
    description: &'static str,
}

macro_rules! flag {
    ($name:literal, $type:ident, $desc:expr) => {
        FlagDescription {
            letter: $name,
            argument_type: $type,
            description: $desc,
        }
    };
}

struct CommandExecuteContext<'a> {
    plot: &'a mut PlotWorld,
    player: &'a mut Player,
    arguments: Vec<Argument>,
    flags: Vec<char>,
}

impl<'a> CommandExecuteContext<'a> {
    fn has_flag(&self, c: char) -> bool {
        self.flags.contains(&c)
    }
}

struct WorldeditCommand {
    arguments: &'static [ArgumentDescription],
    flags: &'static [FlagDescription],
    requires_positions: bool,
    requires_clipboard: bool,
    execute_fn: fn(CommandExecuteContext<'_>),
    description: &'static str,
    permission_node: &'static str,
    mutates_world: bool,
}

impl Default for WorldeditCommand {
    fn default() -> Self {
        Self {
            arguments: &[],
            flags: &[],
            execute_fn: execute_unimplemented,
            description: "",
            requires_clipboard: false,
            requires_positions: false,
            permission_node: "",
            mutates_world: true,
        }
    }
}

static COMMANDS: Lazy<HashMap<&'static str, WorldeditCommand>> = Lazy::new(|| {
    map! {
        "up" => WorldeditCommand {
            execute_fn: execute_up,
            description: messages::WE_HELP_GO_UPWARDS_SOME_DISTANCE,
            arguments: &[
                argument!("distance", UnsignedInteger, messages::WE_ARGUMENT_DISTANCE_TO_GO_UPWARDS)
            ],
            permission_node: "worldedit.navigation.up",
            ..Default::default()
        },
        "ascend" => WorldeditCommand {
            execute_fn: execute_ascend,
            description: messages::WE_HELP_GO_UP_A_FLOOR,
            arguments: &[
                argument!("levels", UnsignedInteger, messages::WE_ARGUMENT_OF_LEVELS_TO_ASCEND)
            ],
            permission_node: "worldedit.navigation.ascend",
            mutates_world: false,
            ..Default::default()
        },
        "descend" => WorldeditCommand {
            execute_fn: execute_descend,
            description: messages::WE_HELP_GO_DOWN_A_FLOOR,
            arguments: &[
                argument!("levels", UnsignedInteger, messages::WE_ARGUMENT_OF_LEVELS_TO_DESCEND)
            ],
            permission_node: "worldedit.navigation.descend",
            mutates_world: false,
            ..Default::default()
        },
        "/pos1" => WorldeditCommand {
            execute_fn: execute_pos1,
            description: messages::WE_HELP_SET_POSITION_1,
            permission_node: "worldedit.selection.pos",
            mutates_world: false,
            ..Default::default()
        },
        "/pos2" => WorldeditCommand {
            execute_fn: execute_pos2,
            description: messages::WE_HELP_SET_POSITION_2,
            permission_node: "worldedit.selection.pos",
            mutates_world: false,
            ..Default::default()
        },
        "/hpos1" => WorldeditCommand {
            execute_fn: execute_hpos1,
            description: messages::WE_HELP_SET_POSITION_1_TO_TARGETED_BLOCK,
            permission_node: "worldedit.selection.hpos",
            mutates_world: false,
            ..Default::default()
        },
        "/hpos2" => WorldeditCommand {
            execute_fn: execute_hpos2,
            description: messages::WE_HELP_SET_POSITION_2_TO_TARGETED_BLOCK,
            permission_node: "worldedit.selection.hpos",
            mutates_world: false,
            ..Default::default()
        },
        "/sel" => WorldeditCommand {
            execute_fn: execute_sel,
            description: messages::WE_HELP_CHOOSE_A_REGION_SELECTOR,
            permission_node: "worldedit.selection.sel",
            mutates_world: false,
            ..Default::default()
        },
        "/set" => WorldeditCommand {
            arguments: &[
                argument!("pattern", Pattern, messages::WE_ARGUMENT_THE_PATTERN_OF_BLOCKS_TO_SET)
            ],
            requires_positions: true,
            execute_fn: execute_set,
            description: messages::WE_HELP_SETS_ALL_THE_BLOCKS_IN_THE,
            permission_node: "worldedit.region.set",
            ..Default::default()
        },
        "/replace" => WorldeditCommand {
            arguments: &[
                argument!("from", Mask, messages::WE_ARGUMENT_REPLACE_MASK),
                argument!("to", Pattern, messages::WE_ARGUMENT_THE_PATTERN_OF_BLOCKS_TO_REPLACE_WITH)
            ],
            requires_positions: true,
            execute_fn: execute_replace,
            description: messages::WE_HELP_REPLACE_ALL_BLOCKS_IN_A_SELECTION,
            permission_node: "worldedit.region.replace",
            ..Default::default()
        },
        "/copy" => WorldeditCommand {
            requires_positions: true,
            execute_fn: execute_copy,
            description: messages::WE_HELP_COPY_THE_SELECTION_TO_THE_CLIPBOARD,
            permission_node: "worldedit.clipboard.copy",
            mutates_world: false,
            ..Default::default()
        },
        "/cut" => WorldeditCommand {
            requires_positions: true,
            execute_fn: execute_cut,
            description: messages::WE_HELP_CUT_THE_SELECTION_TO_THE_CLIPBOARD,
            permission_node: "worldedit.clipboard.cut",
            ..Default::default()
        },
        "/paste" => WorldeditCommand {
            requires_clipboard: true,
            execute_fn: execute_paste,
            description: messages::WE_HELP_PASTE_THE_CLIPBOARD_S_CONTENTS,
            flags: &[
                flag!('a', None, messages::WE_HELP_SKIP_AIR_BLOCKS),
                flag!('u', None, messages::WE_HELP_ALSO_UPDATE_ALL_AFFECTED_BLOCKS),
                flag!('s', None, messages::WE_HELP_SELECT_THE_PASTED_REGION),
            ],
            permission_node: "worldedit.clipboard.paste",
            ..Default::default()
        },
        "/undo" => WorldeditCommand {
            execute_fn: execute_undo,
            description: messages::WE_HELP_UNDOES_THE_LAST_ACTION_FROM_HISTORY,
            permission_node: "worldedit.history.undo",
            ..Default::default()
        },
        "/redo" => WorldeditCommand {
            execute_fn: execute_redo,
            description: messages::WE_HELP_REDOES_THE_LAST_ACTION_FROM_HISTORY,
            permission_node: "worldedit.history.redo",
            ..Default::default()
        },
        "/stack" => WorldeditCommand {
            arguments: &[
                argument!("count", UnsignedInteger, messages::WE_ARGUMENT_OF_COPIES_TO_STACK),
                argument!("direction", Direction, messages::WE_ARGUMENT_THE_DIRECTION_TO_STACK)
            ],
            requires_positions: true,
            execute_fn: execute_stack,
            description: messages::WE_HELP_REPEAT_THE_CONTENTS_OF_THE_SELECTION,
            flags: &[
                flag!('a', None, messages::WE_HELP_IGNORE_AIR_BLOCKS)
            ],
            permission_node: "worldedit.region.stack",
            ..Default::default()
        },
        "/move" => WorldeditCommand {
            arguments: &[
                argument!("count", UnsignedInteger, messages::WE_ARGUMENT_THE_DISTANCE_TO_MOVE),
                argument!("direction", Direction, messages::WE_ARGUMENT_THE_DIRECTION_TO_MOVE)
            ],
            requires_positions: true,
            execute_fn: execute_move,
            description: messages::WE_HELP_MOVE_THE_CONTENTS_OF_THE_SELECTION,
            flags: &[
                flag!('a', None, messages::WE_HELP_IGNORE_AIR_BLOCKS),
                flag!('s', None, messages::WE_HELP_SHIFT_THE_SELECTION_TO_THE_TARGET)
            ],
            permission_node: "worldedit.region.move",
            ..Default::default()
        },
        "/count" => WorldeditCommand {
            arguments: &[
                argument!("mask", Mask, messages::WE_ARGUMENT_THE_MASK_OF_BLOCKS_TO_MATCH)
            ],
            requires_positions: true,
            execute_fn: execute_count,
            description: messages::WE_HELP_COUNTS_THE_NUMBER_OF_BLOCKS_MATCHING,
            permission_node: "worldedit.analysis.count",
            mutates_world: false,
            ..Default::default()
        },
        "/load" => WorldeditCommand {
            arguments: &[
                argument!("name", String, messages::WE_ARGUMENT_THE_FILE_NAME_OF_THE_SCHEMATIC_TO_LOAD)
            ],
            execute_fn: execute_load,
            description: messages::WE_HELP_LOADS_A_SCHEMATIC_FILE_INTO_THE,
            permission_node: "worldedit.clipboard.load",
            mutates_world: false,
            ..Default::default()
        },
        "/save" => WorldeditCommand {
            arguments: &[
                argument!("name", String, messages::WE_ARGUMENT_THE_FILE_NAME_OF_THE_SCHEMATIC_TO_SAVE)
            ],
            requires_clipboard: true,
            execute_fn: execute_save,
            description: messages::WE_HELP_SAVE_A_SCHEMATIC_FILE_FROM_THE,
            permission_node: "worldedit.clipboard.save",
            mutates_world: false,
            ..Default::default()
        },
        "/expand" => WorldeditCommand {
            arguments: &[
                argument!("amount", UnsignedInteger, messages::WE_ARGUMENT_AMOUNT_TO_EXPAND_THE_SELECTION_BY),
                argument!("direction", Direction, messages::WE_ARGUMENT_DIRECTION_TO_EXPAND)
            ],
            requires_positions: true,
            execute_fn: execute_expand,
            description: messages::WE_HELP_EXPAND_THE_SELECTION_AREA,
            permission_node: "worldedit.selection.expand",
            mutates_world: false,
            ..Default::default()
        },
        "/contract" => WorldeditCommand {
            arguments: &[
                argument!("amount", UnsignedInteger, messages::WE_ARGUMENT_AMOUNT_TO_CONTRACT_THE_SELECTION_BY),
                argument!("direction", Direction, messages::WE_ARGUMENT_DIRECTION_TO_CONTRACT)
            ],
            requires_positions: true,
            execute_fn: execute_contract,
            description: messages::WE_HELP_CONTRACT_THE_SELECTION_AREA,
            permission_node: "worldedit.selection.contract",
            mutates_world: false,
            ..Default::default()
        },
        "/shift" => WorldeditCommand {
            arguments: &[
                argument!("amount", UnsignedInteger, messages::WE_ARGUMENT_AMOUNT_TO_SHIFT_THE_SELECTION_BY),
                argument!("direction", Direction, messages::WE_ARGUMENT_DIRECTION_TO_SHIFT)
            ],
            requires_positions: true,
            execute_fn: execute_shift,
            description: messages::WE_HELP_SHIFT_THE_SELECTION_AREA,
            permission_node: "worldedit.selection.shift",
            mutates_world: false,
            ..Default::default()
        },
        "/flip" => WorldeditCommand {
            arguments: &[
                argument!("direction", Direction, messages::WE_ARGUMENT_THE_DIRECTION_TO_FLIP_DEFAULTS_TO_LOOK_DIRECTION),
            ],
            requires_clipboard: true,
            execute_fn: execute_flip,
            description: messages::WE_HELP_FLIP_THE_CONTENTS_OF_THE_CLIPBOARD,
            permission_node: "worldedit.clipboard.flip",
            mutates_world: false,
            ..Default::default()
        },
        "/rotate" => WorldeditCommand {
            arguments: &[
                argument!("rotateY", UnsignedInteger, messages::WE_ARGUMENT_ROTATION_DEGREES, 0),
            ],
            requires_clipboard: true,
            execute_fn: execute_rotate,
            description: messages::WE_HELP_ROTATE_THE_CONTENTS_OF_THE_CLIPBOARD,
            permission_node: "worldedit.clipboard.rotate",
            mutates_world: false,
            ..Default::default()
        },
        "/update" => WorldeditCommand {
            execute_fn: execute_update,
            description: messages::WE_HELP_UPDATES_ALL_BLOCKS_IN_THE_SELECTION,
            permission_node: "mchprs.we.update",
            requires_positions: false,
            flags: &[
                flag!('p', None, messages::WE_HELP_UPDATE_THE_ENTIRE_PLOT),
            ],
            ..Default::default()
        },
        "/invalidatecaches" => WorldeditCommand {
            execute_fn: execute_invalidate_caches,
            description: messages::WE_HELP_INVALIDATE_CACHES,
            permission_node: "mchprs.we.invalidatecaches",
            mutates_world: false,
            ..Default::default()
        },
        "/help" => WorldeditCommand {
            arguments: &[
                argument!("command", String, messages::WE_ARGUMENT_COMMAND_TO_RETRIEVE_HELP_FOR),
            ],
            execute_fn: execute_help,
            description: messages::WE_HELP_DISPLAYS_HELP_FOR_WORLDEDIT_COMMANDS,
            permission_node: "worldedit.help",
            mutates_world: false,
            ..Default::default()
        },
        "/wand" => WorldeditCommand {
           execute_fn: execute_wand,
           description: messages::WE_HELP_GIVES_A_WORLDEDIT_WAND,
           permission_node: "worldedit.wand",
            mutates_world: false,
           ..Default::default()
        },
        "/replacecontainer" => WorldeditCommand {
            arguments: &[
                argument!("from", ContainerType, messages::WE_ARGUMENT_THE_CONTAINER_TYPE_TO_REPLACE),
                argument!("to", ContainerType, messages::WE_ARGUMENT_THE_CONTAINER_TYPE_TO_REPLACE_WITH),
            ],
           execute_fn: execute_replace_container,
           description: messages::WE_HELP_REPLACES_ALL_CONTAINER_TYPES_IN_THE,
           permission_node: "mchprs.we.replacecontainer",
           requires_positions: true,
           ..Default::default()
        }
    }
});

static ALIASES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    map! {
        "/desel" => "/sel",
        "desel" => "/sel",
        "set" => "/set",
        "u" => "up",
        "desc" => "descend",
        "asc" => "ascend",
        "/1" => "/pos1",
        "/2" => "/pos2",
        "/c" => "/copy",
        "/x" => "/cut",
        "/v" => "/paste",
        "/va" => "/paste -a",
        "/s" => "/stack",
        "/sa" => "/stack -a",
        "/e" => "/expand",
        "/r" => "/rotate",
        "/f" => "/flip",
        "/h1" => "/hpos1",
        "/h2" => "/hpos2",
        "/rc" => "/replacecontainer"
    }
});

#[derive(Debug, Clone)]
pub struct WorldEditPatternPart {
    pub weight: f32,
    pub block_id: u32,
}

#[derive(Clone, Debug)]
pub struct WorldEditClipboard {
    pub offset_x: i32,
    pub offset_y: i32,
    pub offset_z: i32,
    pub size_x: u32,
    pub size_y: u32,
    pub size_z: u32,
    pub data: PalettedBitBuffer,
    pub block_entities: FxHashMap<BlockPos, BlockEntity>,
}

impl WorldEditClipboard {
    /// Destination bounds for a clipboard whose geometry has already been validated.
    fn bounds_at(&self, pos: BlockPos) -> (BlockPos, BlockPos) {
        let first = BlockPos::new(
            pos.x - self.offset_x,
            pos.y - self.offset_y,
            pos.z - self.offset_z,
        );
        let second = BlockPos::new(
            first.x + self.size_x as i32 - 1,
            first.y + self.size_y as i32 - 1,
            first.z + self.size_z as i32 - 1,
        );
        (first, second)
    }
}

#[derive(Clone, Debug)]
pub struct WorldEditUndo {
    clipboards: Vec<WorldEditClipboard>,
    pos: BlockPos,
    plot_x: i32,
    plot_z: i32,
}

impl WorldEditUndo {
    /// Capture all destination regions before applying undo or redo.
    fn capture_inverse(&self, plot: &mut PlotWorld) -> Self {
        Self {
            clipboards: self
                .clipboards
                .iter()
                .map(|clipboard| {
                    let (first, second) = clipboard.bounds_at(self.pos);
                    create_clipboard(plot, self.pos, first, second)
                })
                .collect(),
            pos: self.pos,
            plot_x: self.plot_x,
            plot_z: self.plot_z,
        }
    }
}

/// Bound retained undo/redo across repeated commands, including redstone stacks.
pub(in crate::plot) fn trim_history(player: &mut Player) {
    let entries = |history: &[WorldEditUndo]| -> u64 {
        history
            .iter()
            .flat_map(|undo| &undo.clipboards)
            .map(|cb| cb.data.entries() as u64)
            .sum()
    };
    let mut blocks = entries(&player.worldedit_undo) + entries(&player.worldedit_redo);
    while blocks > crate::config::CONFIG.worldedit_history_blocks {
        let history = if !player.worldedit_undo.is_empty() {
            &mut player.worldedit_undo
        } else {
            &mut player.worldedit_redo
        };
        let removed = history.remove(0);
        blocks -= removed
            .clipboards
            .iter()
            .map(|cb| cb.data.entries() as u64)
            .sum::<u64>();
    }
}

#[derive(Debug)]
pub enum PatternParseError {
    UnknownBlock(String),
    InvalidPattern(String),
}

impl fmt::Display for PatternParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternParseError::UnknownBlock(block) => {
                f.write_str(&messages::unknown_pattern_block(block))
            }
            PatternParseError::InvalidPattern(pattern) => {
                f.write_str(&messages::invalid_pattern(pattern))
            }
        }
    }
}

pub type PatternParseResult<T> = std::result::Result<T, PatternParseError>;

#[derive(Debug, Clone)]
pub struct WorldEditPattern {
    pub parts: Vec<WorldEditPatternPart>,
}

impl FromStr for WorldEditPattern {
    type Err = PatternParseError;

    fn from_str(pattern_str: &str) -> PatternParseResult<WorldEditPattern> {
        let mut pattern = WorldEditPattern { parts: Vec::new() };
        let mut depth = 0;
        for part in pattern_str.split(|c| {
            match c {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            c == ',' && depth == 0
        }) {
            static RE: Lazy<Regex> = Lazy::new(|| {
                Regex::new(r"^(([0-9]+(\.[0-9]+)?)%)?(=)?([0-9]+|(minecraft:)?[a-zA-Z_]+)(:([0-9]+)|\[(([a-zA-Z_]+=[a-zA-Z0-9]+,?)+?)\])?((\|([^|]*?)){1,4})?$").unwrap()
            });

            let pattern_match = RE
                .captures(part)
                .ok_or_else(|| PatternParseError::InvalidPattern(part.to_owned()))?;

            let block_name = pattern_match.get(5).unwrap().as_str();
            let mut block = if pattern_match.get(4).is_some()
                || block_name.bytes().all(|byte| byte.is_ascii_digit())
            {
                let id = block_name
                    .parse::<u32>()
                    .map_err(|_| PatternParseError::InvalidPattern(part.to_owned()))?;
                if id as usize >= mchprs_blocks::generated::STATE_PROPERTIES.len() {
                    return Err(PatternParseError::UnknownBlock(part.to_owned()));
                }
                Block::from_id(id)
            } else {
                let block_name = block_name.trim_start_matches("minecraft:");
                Block::from_name(block_name)
                    .ok_or_else(|| PatternParseError::UnknownBlock(part.to_owned()))?
            };

            if let Some(props) = pattern_match.get(9) {
                block =
                    schematic::parse_block(&format!("{}[{}]", block.get_name(), props.as_str()))
                        .ok_or_else(|| PatternParseError::InvalidPattern(part.to_owned()))?;
            }

            let weight = pattern_match
                .get(2)
                .map_or("100", |m| m.as_str())
                .parse::<f32>()
                .map_err(|_| PatternParseError::InvalidPattern(part.to_owned()))?
                / 100.0;
            if !weight.is_finite() || pattern.parts.len() >= 256 {
                return Err(PatternParseError::InvalidPattern(part.to_owned()));
            }

            pattern.parts.push(WorldEditPatternPart {
                weight,
                block_id: block.get_id(),
            });
        }

        let weight: f32 = pattern.parts.iter().map(|part| part.weight).sum();
        if !weight.is_finite() || weight <= 0.0 {
            return Err(PatternParseError::InvalidPattern(pattern_str.to_owned()));
        }
        Ok(pattern)
    }
}

impl WorldEditPattern {
    pub fn matches(&self, block: Block) -> bool {
        let block_id = block.get_id();
        self.parts.iter().any(|part| part.block_id == block_id)
    }

    pub fn pick(&self) -> Block {
        let mut weight_sum = 0.0;
        for part in &self.parts {
            weight_sum += part.weight;
        }
        if !weight_sum.is_finite() || weight_sum <= 0.0 {
            return Block::Air {};
        }

        let mut rng = rand::thread_rng();
        let mut random = rng.gen_range(0.0..weight_sum);

        let mut selected = &WorldEditPatternPart {
            block_id: 0,
            weight: 0.0,
        };

        for part in &self.parts {
            if part.weight <= 0.0 {
                continue;
            }
            random -= part.weight;
            if random <= 0.0 {
                selected = part;
                break;
            }
        }

        Block::from_id(selected.block_id)
    }
}

#[test]
fn container_patterns_apply_properties_and_split_only_between_blocks() {
    let pattern = WorldEditPattern::from_str(
        "25%hopper[facing=east,enabled=false],75%furnace[facing=west,lit=true]",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pattern.parts.len(), 2);
    assert_eq!(pattern.parts[0].block_id, 10043);
    assert_eq!(pattern.parts[1].block_id, 4362);
    assert_eq!(pattern.parts[0].weight, 0.25);
    assert_eq!(pattern.parts[1].weight, 0.75);
    for invalid in ["hopper[facing=up]", "furnace[lit=maybe]", "cake[bites=7]"] {
        assert!(WorldEditPattern::from_str(invalid).is_err());
    }
}

#[test]
fn numeric_patterns_match_names_and_explicit_state_ids() {
    for name in ["air", "stone", "glass", "redstone_block"] {
        let named: WorldEditPattern = name.parse().unwrap();
        let id = named.parts[0].block_id;
        for input in [id.to_string(), format!("={id}")] {
            let numeric: WorldEditPattern = input.parse().unwrap();
            assert_eq!(numeric.parts[0].block_id, id);
            assert_eq!(numeric.pick(), named.pick());
        }
    }
    assert_eq!(
        "0".parse::<WorldEditPattern>().unwrap().pick(),
        Block::Air {}
    );
    let mixed: WorldEditPattern = "25%0,75%stone".parse().unwrap();
    assert_eq!(mixed.parts[0].block_id, 0);
    assert_eq!(mixed.parts[0].weight, 0.25);
    for input in [
        "4294967296".to_owned(),
        "4294967295".to_owned(),
        "-1".to_owned(),
        mchprs_blocks::generated::STATE_PROPERTIES.len().to_string(),
    ] {
        assert!(input.parse::<WorldEditPattern>().is_err(), "{input}");
    }
}

#[test]
fn invalid_pattern_weights_and_numeric_ids_do_not_panic() {
    for pattern in [
        "0%stone",
        "0%stone,0%glass",
        "=4294967296",
        "99999999999999999999999999999999999999999999999999999999%stone",
    ] {
        assert!(pattern.parse::<WorldEditPattern>().is_err(), "{pattern}");
    }
    let pattern: WorldEditPattern = "0%stone,100%glass".parse().unwrap();
    for _ in 0..10 {
        assert_eq!(pattern.pick(), Block::Glass {});
    }
}

struct WorldEditOperation {
    blocks_updated: usize,
    x_range: RangeInclusive<i32>,
    y_range: RangeInclusive<i32>,
    z_range: RangeInclusive<i32>,
}

impl WorldEditOperation {
    fn new(first_pos: BlockPos, second_pos: BlockPos) -> WorldEditOperation {
        let start_pos = first_pos.min(second_pos);
        let end_pos = first_pos.max(second_pos);

        let x_range = start_pos.x..=end_pos.x;
        let y_range = start_pos.y..=end_pos.y;
        let z_range = start_pos.z..=end_pos.z;

        WorldEditOperation {
            blocks_updated: 0,
            x_range,
            y_range,
            z_range,
        }
    }

    fn update_block(&mut self) {
        self.blocks_updated += 1;
    }

    fn blocks_updated(&self) -> usize {
        self.blocks_updated
    }

    fn x_range(&self) -> RangeInclusive<i32> {
        self.x_range.clone()
    }
    fn y_range(&self) -> RangeInclusive<i32> {
        self.y_range.clone()
    }
    fn z_range(&self) -> RangeInclusive<i32> {
        self.z_range.clone()
    }
}

pub fn ray_trace_block(
    world: &impl World,
    mut pos: PlayerPos,
    start_pitch: f64,
    start_yaw: f64,
    max_distance: f64,
) -> Option<BlockPos> {
    let check_distance = 0.2;

    // Player view height
    pos.y += 1.65;
    let rot_x = (start_yaw + 90.0) % 360.0;
    let rot_y = -start_pitch;
    let h = check_distance * rot_y.to_radians().cos();

    let offset_x = h * rot_x.to_radians().cos();
    let offset_y = check_distance * rot_y.to_radians().sin();
    let offset_z = h * rot_x.to_radians().sin();

    let mut current_distance = 0.0;

    while current_distance < max_distance {
        let block_pos = pos.block_pos();
        let block = world.get_block(block_pos);

        if !matches!(block, Block::Air) {
            return Some(block_pos);
        }

        pos.x += offset_x;
        pos.y += offset_y;
        pos.z += offset_z;
        current_distance += check_distance;
    }

    None
}

fn worldedit_start_operation(player: &mut Player) -> WorldEditOperation {
    let first_pos = player.first_position.unwrap();
    let second_pos = player.second_position.unwrap();
    WorldEditOperation::new(first_pos, second_pos)
}

fn create_clipboard(
    plot: &mut PlotWorld,
    origin: BlockPos,
    first_pos: BlockPos,
    second_pos: BlockPos,
) -> WorldEditClipboard {
    let start_pos = first_pos.min(second_pos);
    let end_pos = first_pos.max(second_pos);
    let size_x = (end_pos.x - start_pos.x) as u32 + 1;
    let size_y = (end_pos.y - start_pos.y) as u32 + 1;
    let size_z = (end_pos.z - start_pos.z) as u32 + 1;
    let offset = origin - start_pos;
    let mut cb = WorldEditClipboard {
        offset_x: offset.x,
        offset_y: offset.y,
        offset_z: offset.z,
        size_x,
        size_y,
        size_z,
        data: PalettedBitBuffer::new((size_x * size_y * size_z) as usize, 9),
        block_entities: FxHashMap::default(),
    };
    let mut i = 0;
    for y in start_pos.y..=end_pos.y {
        for z in start_pos.z..=end_pos.z {
            for x in start_pos.x..=end_pos.x {
                let pos = BlockPos::new(x, y, z);
                let id = plot.get_block_raw(pos);
                let block = plot.get_block(BlockPos::new(x, y, z));
                if block.has_block_entity() {
                    if let Some(block_entity) = plot.get_block_entity(pos) {
                        cb.block_entities
                            .insert(pos - start_pos, block_entity.clone());
                    }
                }
                cb.data.set_entry(i, id);
                i += 1;
            }
        }
    }
    cb
}

fn clear_area(plot: &mut PlotWorld, first_pos: BlockPos, second_pos: BlockPos) {
    let start_pos = first_pos.min(second_pos);
    let end_pos = first_pos.max(second_pos);
    for y in start_pos.y..=end_pos.y {
        for z in start_pos.z..=end_pos.z {
            for x in start_pos.x..=end_pos.x {
                plot.set_block_raw(BlockPos::new(x, y, z), 0);
            }
        }
    }
    // Send modified chunks
    for chunk_x in (start_pos.x >> 4)..=(end_pos.x >> 4) {
        for chunk_z in (start_pos.z >> 4)..=(end_pos.z >> 4) {
            if let Some(chunk) = plot.get_chunk(chunk_x, chunk_z) {
                let chunk_data = chunk.encode_packet();
                for player in &mut plot.packet_senders {
                    player.send_packet(&chunk_data);
                }
            }
        }
    }
}

pub fn paste_clipboard(
    plot: &mut PlotWorld,
    cb: &WorldEditClipboard,
    pos: BlockPos,
    ignore_air: bool,
) {
    let previous = plot.set_authoritative_updates(true);
    let offset_x = pos.x - cb.offset_x;
    let offset_y = pos.y - cb.offset_y;
    let offset_z = pos.z - cb.offset_z;
    let mut i = 0;
    // This can be made better, but right now it's not D:
    let x_range = offset_x..offset_x + cb.size_x as i32;
    let y_range = offset_y..offset_y + cb.size_y as i32;
    let z_range = offset_z..offset_z + cb.size_z as i32;

    let entries = cb.data.entries();
    // I have no clue if these clones are going to cost anything noticeable.
    'top_loop: for y in y_range {
        for z in z_range.clone() {
            for x in x_range.clone() {
                if i >= entries {
                    break 'top_loop;
                }
                let entry = cb.data.get_entry(i);
                i += 1;
                if ignore_air && entry == 0 {
                    continue;
                }
                let block_pos = BlockPos::new(x, y, z);
                plot.delete_block_entity(block_pos);
                plot.set_block_raw(block_pos, entry);
            }
        }
    }

    // Send block changes before we send block entity data, otherwise it'll be ignored
    plot.flush_block_changes();

    for (pos, block_entity) in &cb.block_entities {
        let new_pos = BlockPos {
            x: pos.x + offset_x,
            y: pos.y + offset_y,
            z: pos.z + offset_z,
        };
        plot.set_block_entity(new_pos, block_entity.clone());
        if plot.get_block(new_pos).is_command_block() {
            redstone::command_block::update(plot, new_pos);
        }
    }
    plot.flush_block_changes();
    plot.set_authoritative_updates(previous);
}

#[derive(Clone, Copy, Debug)]
pub(super) enum AirPolicy {
    Ignore,
    Copy,
}

/// All destinations must be validated before calling. Snapshot every target before
/// the first paste so overlapping copies remain reversible in a single undo.
pub(super) fn stack_prepared(
    plot: &mut PlotWorld,
    start: BlockPos,
    end: BlockPos,
    destinations: &[(BlockPos, BlockPos)],
    air: AirPolicy,
) -> WorldEditUndo {
    let source = create_clipboard(plot, start, start, end);
    let clipboards = destinations
        .iter()
        .map(|&(first, second)| create_clipboard(plot, start, first, second))
        .collect();
    let undo = WorldEditUndo {
        clipboards,
        pos: start,
        plot_x: plot.x,
        plot_z: plot.z,
    };
    for &(first, _) in destinations {
        paste_clipboard(plot, &source, first, matches!(air, AirPolicy::Ignore));
    }
    undo
}

fn capture_undo(
    plot: &mut PlotWorld,
    player: &mut Player,
    first_pos: BlockPos,
    second_pos: BlockPos,
) {
    let origin = first_pos.min(second_pos);
    let cb = create_clipboard(plot, origin, first_pos, second_pos);
    let undo = WorldEditUndo {
        clipboards: vec![cb],
        pos: origin,
        plot_x: plot.x,
        plot_z: plot.z,
    };

    player.worldedit_undo.push(undo);
    player.worldedit_redo.clear();
}

fn expand_selection(player: &mut Player, amount: BlockPos, contract: bool) {
    let mut p1 = player.first_position.unwrap();
    let mut p2 = player.second_position.unwrap();

    fn get_pos_axis(pos: &mut BlockPos, axis: u8) -> &mut i32 {
        match axis {
            0 => &mut pos.x,
            1 => &mut pos.y,
            2 => &mut pos.z,
            _ => unreachable!(),
        }
    }

    let mut expand_axis = |axis: u8| {
        let amount = *get_pos_axis(&mut amount.clone(), axis);
        let p1 = get_pos_axis(&mut p1, axis);
        let p2 = get_pos_axis(&mut p2, axis);
        #[allow(clippy::comparison_chain)]
        if amount > 0 {
            if (p1 > p2) ^ contract {
                *p1 += amount;
            } else {
                *p2 += amount;
            }
        } else if amount < 0 {
            if (p1 < p2) ^ contract {
                *p1 += amount;
            } else {
                *p2 += amount;
            }
        }
    };

    for axis in 0..=2 {
        expand_axis(axis);
    }

    if Some(p1) != player.first_position {
        player.worldedit_set_first_position(p1);
    }
    if Some(p2) != player.second_position {
        player.worldedit_set_second_position(p2);
    }
}

fn update(plot: &mut PlotWorld, first_pos: BlockPos, second_pos: BlockPos) {
    // Pasted clipboards can overlap the plot boundary; update only their placed part.
    let (plot_min, plot_max) = plot.get_corners();
    let first = first_pos.min(second_pos).max(plot_min);
    let second = first_pos.max(second_pos).min(plot_max);
    if first.x <= second.x && first.y <= second.y && first.z <= second.z {
        update_selection(plot, first, second).expect("clipped update bounds");
    }
}

fn update_selection(
    plot: &mut PlotWorld,
    first_pos: BlockPos,
    second_pos: BlockPos,
) -> Result<(), &'static str> {
    if !Plot::in_plot_bounds(plot.x, plot.z, first_pos.x, first_pos.z) {
        return Err(messages::FIRST_POSITION_OUTSIDE_PLOT_BOUNDS);
    }
    if !Plot::in_plot_bounds(plot.x, plot.z, second_pos.x, second_pos.z) {
        return Err(messages::SECOND_POSITION_OUTSIDE_PLOT_BOUNDS);
    }
    if !(0..super::PLOT_BLOCK_HEIGHT).contains(&first_pos.y)
        || !(0..super::PLOT_BLOCK_HEIGHT).contains(&second_pos.y)
    {
        return Err(messages::UPDATE_SELECTION_OUTSIDE_HEIGHT);
    }
    crate::world::for_each_block_mut_optimized(plot, first_pos, second_pos, |plot, pos| {
        redstone::update(plot.get_block(pos), plot, pos, None);
    });
    Ok(())
}
