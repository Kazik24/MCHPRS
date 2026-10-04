mod completion;
mod items;
mod search;
pub(super) mod selection;
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
    Container,
    CurrentSelection,
}

impl ToolCommand {
    fn parse(command: &str) -> Option<Self> {
        match command {
            "//find" => Some(Self::Find),
            "//signsearch" | "//ss" => Some(Self::SignSearch),
            "//rstack" | "//rs" => Some(Self::RStack),
            "/container" => Some(Self::Container),
            "/cursel" => Some(Self::CurrentSelection),
            _ => None,
        }
    }

    fn permission(self) -> &'static str {
        match self {
            Self::Find => "redstonetools.find",
            Self::SignSearch => "redstonetools.signsearch",
            Self::RStack => "redstonetools.rstack",
            Self::Container => "redstonetools.container",
            Self::CurrentSelection => "redstonetools.cursel",
        }
    }

    fn help_topic(command: &str, args: &[&str]) -> Option<Self> {
        if command != "//help" {
            return None;
        }
        let [topic] = args else {
            return None;
        };
        let topic = topic.trim_start_matches('/');
        Self::parse(&format!("//{topic}")).or_else(|| Self::parse(&format!("/{topic}")))
    }
}

enum ToolNotice {
    Error(String),
    SelectionEnabled,
    SelectionDisabled,
    ItemGiven,
    Stacked(u32),
}

impl ToolNotice {
    fn send(self, player: &Player) {
        let (text, color) = match self {
            Self::Error(error) => (format!("{error} >.<"), "red"),
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
        let first = player
            .first_position
            .ok_or_else(|| anyhow::anyhow!("Select position 1 first"))?;
        let second = player
            .second_position
            .ok_or_else(|| anyhow::anyhow!("Select position 2 first"))?;
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
        if let Some(tool) = ToolCommand::help_topic(command, args) {
            match self.check_tool_access(player, tool) {
                Ok(()) => self.players[player].send_system_message(
                    super::help::page(Some("tools")).expect("tools help page"),
                ),
                Err(error) => {
                    ToolNotice::Error(error.to_string()).send(&self.players[player]);
                }
            }
            return true;
        }
        let Some(tool) = ToolCommand::parse(command) else {
            return false;
        };
        let result = self
            .check_tool_access(player, tool)
            .and_then(|()| self.execute_tool(player, tool, args));
        if let Err(error) = result {
            ToolNotice::Error(error.to_string()).send(&self.players[player]);
        }
        true
    }

    fn check_tool_access(&self, player: usize, command: ToolCommand) -> Result<()> {
        let player = &self.players[player];
        if !player.has_permission(command.permission()) {
            bail!("You don't have permission to use this command");
        }
        if matches!(
            command,
            ToolCommand::Find | ToolCommand::SignSearch | ToolCommand::RStack
        ) && !player.has_permission("plots.worldedit.bypass")
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
            ToolCommand::CurrentSelection => selection::toggle(&mut self.players[player], args),
        }
    }
}
