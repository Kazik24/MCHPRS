mod items;
mod search;
mod selection;
mod stack;

#[cfg(test)]
mod tests;

use super::{Plot, PlotWorld, PLOT_BLOCK_HEIGHT};
use crate::player::{PacketSender, Player};
use anyhow::{bail, Result};
use mchprs_blocks::BlockPos;
use search::{SearchCache, SearchKind};
use serde_json::{json, Value};

/// Session state only: none of these preferences or caches alter player saves.
#[derive(Default)]
pub(crate) struct PlayerTools {
    pub autowire: bool,
    pub selection_visible: bool,
    pub block_search: Option<SearchCache>,
    pub sign_search: Option<SearchCache>,
    selection_lines: Option<Vec<String>>,
}

#[derive(Clone, Copy)]
enum ToolCommand {
    Find,
    SignSearch,
    RStack,
    Autowire,
    Container,
    Slab,
    CurrentSelection,
}

impl ToolCommand {
    fn parse(command: &str) -> Option<Self> {
        match command {
            "//find" => Some(Self::Find),
            "//signsearch" | "//ss" => Some(Self::SignSearch),
            "//rstack" | "//rs" => Some(Self::RStack),
            "/autowire" | "/aw" => Some(Self::Autowire),
            "/container" => Some(Self::Container),
            "/slab" => Some(Self::Slab),
            "/cursel" => Some(Self::CurrentSelection),
            _ => None,
        }
    }

    fn permission(self) -> &'static str {
        match self {
            Self::Find => "redstonetools.find",
            Self::SignSearch => "redstonetools.signsearch",
            Self::RStack => "redstonetools.rstack",
            Self::Autowire => "redstonetools.autowire",
            Self::Container => "redstonetools.container",
            Self::Slab => "redstonetools.slab",
            Self::CurrentSelection => "redstonetools.cursel",
        }
    }
}

enum ToolNotice {
    Error(String),
    AutowireEnabled,
    AutowireDisabled,
    SelectionEnabled,
    SelectionDisabled,
    ItemGiven,
    Stacked(u32),
}

impl ToolNotice {
    fn send(self, player: &Player) {
        let (text, color) = match self {
            Self::Error(error) => (format!("{error} >.<"), "red"),
            Self::AutowireEnabled => ("Autowire enabled, nya~".into(), "light_purple"),
            Self::AutowireDisabled => ("Autowire disabled, nya~".into(), "light_purple"),
            Self::SelectionEnabled => ("Selection sidebar enabled, nya~".into(), "light_purple"),
            Self::SelectionDisabled => ("Selection sidebar hidden, nya~".into(), "light_purple"),
            Self::ItemGiven => ("Your item is ready, nya~".into(), "light_purple"),
            Self::Stacked(count) => (format!("Stacked {count} copies, nya~"), "light_purple"),
        };
        player.send_raw_system_message(json!({"text": text, "color": color}).to_string());
    }
}

/// Only server-generated actions use this path. Authored chat retains its existing policy.
enum ResultAction {
    Teleport(BlockPos),
    Page { kind: SearchKind, page: usize },
}

impl ResultAction {
    fn component(self, label: &str) -> Value {
        let command = match self {
            Self::Teleport(pos) => format!("/tp {} {} {}", pos.x, pos.y + 1, pos.z),
            Self::Page { kind, page } => format!("{} -p {page}", kind.command()),
        };
        json!({
            "text": label,
            "color": "aqua",
            "click_event": {"action": "run_command", "command": command},
            "hover_event": {"action": "show_text", "value": "Click, nya~"}
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SelectionBounds {
    start: BlockPos,
    end: BlockPos,
}

impl SelectionBounds {
    fn from_player(player: &Player, world: &PlotWorld) -> Result<Self> {
        let first = player.first_position.ok_or_else(|| anyhow::anyhow!("Select position 1 first"))?;
        let second = player.second_position.ok_or_else(|| anyhow::anyhow!("Select position 2 first"))?;
        Self::new(first.min(second), first.max(second), world)
    }

    fn new(start: BlockPos, end: BlockPos, world: &PlotWorld) -> Result<Self> {
        if !world.contains_position(start) || !world.contains_position(end) {
            bail!("The complete selection must be inside this plot and world height");
        }
        Ok(Self { start, end })
    }

    fn volume(self) -> u64 {
        let width = i64::from(self.end.x) - i64::from(self.start.x) + 1;
        let height = i64::from(self.end.y) - i64::from(self.start.y) + 1;
        let depth = i64::from(self.end.z) - i64::from(self.start.z) + 1;
        (width * height * depth) as u64
    }
}

impl PlotWorld {
    pub(crate) fn contains_position(&self, pos: BlockPos) -> bool {
        (0..PLOT_BLOCK_HEIGHT).contains(&pos.y)
            && Plot::in_plot_bounds(self.x, self.z, pos.x, pos.z)
    }
}

impl Plot {
    pub(super) fn handle_redstone_tools_command(
        &mut self,
        player: usize,
        command: &str,
        args: &[&str],
    ) -> bool {
        let Some(tool) = ToolCommand::parse(command) else {
            return false;
        };
        let result = self.check_tool_access(player, tool)
            .and_then(|()| self.execute_tool(player, tool, args));
        if let Err(error) = result {
            ToolNotice::Error(error.to_string()).send(&self.players[player]);
        }
        true
    }

    fn check_tool_access(&mut self, player: usize, command: ToolCommand) -> Result<()> {
        let player = &mut self.players[player];
        if !player.has_permission(command.permission()) {
            bail!("You don't have permission to use this command");
        }
        if matches!(command, ToolCommand::Find | ToolCommand::SignSearch | ToolCommand::RStack)
            && !player.has_permission("plots.worldedit.bypass")
            && self.owner != Some(player.uuid)
        {
            bail!("You can only use WorldEdit on your own plot");
        }
        Ok(())
    }

    fn execute_tool(&mut self, player: usize, command: ToolCommand, args: &[&str]) -> Result<()> {
        match command {
            ToolCommand::Find => self.search_blocks(player, args),
            ToolCommand::SignSearch => self.search_signs(player, args),
            ToolCommand::RStack => self.redstone_stack(player, args),
            ToolCommand::Container => items::give_container(&mut self.players[player], args),
            ToolCommand::Slab => items::give_slab(&mut self.players[player], args),
            ToolCommand::Autowire => {
                if !args.is_empty() {
                    bail!("Usage: /autowire (or /aw)");
                }
                let player = &mut self.players[player];
                player.redstone_tools.autowire = !player.redstone_tools.autowire;
                let notice = match player.redstone_tools.autowire {
                    true => ToolNotice::AutowireEnabled,
                    false => ToolNotice::AutowireDisabled,
                };
                notice.send(player);
                Ok(())
            }
            ToolCommand::CurrentSelection => selection::toggle(&mut self.players[player], args),
        }
    }
}
