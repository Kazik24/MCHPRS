use super::ToolNotice;
use crate::messages;
use crate::player::{PacketSender, Player};
use anyhow::{bail, Result};
use mchprs_blocks::BlockPos;
use mchprs_network::packets::clientbound::{
    CDisplayScoreboard, CScoreboardObjective, CUpdateScore, ClientBoundPacket,
};
use serde_json::json;

const OBJECTIVE: &str = "rf_selection";

pub(super) fn toggle(player: &mut Player, args: &[&str]) -> Result<()> {
    if !args.is_empty() {
        bail!(messages::USAGE_CURSEL);
    }
    player.redstone_tools.selection_visible = !player.redstone_tools.selection_visible;
    if player.redstone_tools.selection_visible {
        update(player);
        ToolNotice::SelectionEnabled.send(player);
    } else {
        remove(player);
        player.send_packet(
            &CDisplayScoreboard {
                position: 1,
                score_name: "redpiler_status".into(),
            }
            .encode(),
        );
        ToolNotice::SelectionDisabled.send(player);
    }
    Ok(())
}

pub(in crate::plot) fn update(player: &mut Player) {
    if !player.redstone_tools.selection_visible {
        return;
    }
    let lines = selection_lines(player.first_position, player.second_position);
    if player.redstone_tools.selection_lines.as_ref() == Some(&lines) {
        return;
    }
    if let Some(previous) = &player.redstone_tools.selection_lines {
        for line in previous {
            send_line(player, line, 1, 0);
        }
    } else {
        player.send_packet(
            &CScoreboardObjective {
                objective_name: OBJECTIVE.into(),
                mode: 0,
                objective_value:
                    json!({"text": messages::SELECTION_SIDEBAR_TITLE, "color": "light_purple"})
                        .to_string(),
                ty: 0,
            }
            .encode(),
        );
        player.send_packet(
            &CDisplayScoreboard {
                position: 1,
                score_name: OBJECTIVE.into(),
            }
            .encode(),
        );
    }
    for (index, line) in lines.iter().enumerate() {
        send_line(player, line, 0, (lines.len() - index) as u32);
    }
    player.redstone_tools.selection_lines = Some(lines);
}

pub(in crate::plot) fn remove(player: &mut Player) {
    if player.redstone_tools.selection_lines.take().is_some() {
        player.send_packet(
            &CScoreboardObjective {
                objective_name: OBJECTIVE.into(),
                mode: 1,
                objective_value: String::new(),
                ty: 0,
            }
            .encode(),
        );
    }
}

fn send_line(player: &Player, text: &str, action: u8, value: u32) {
    player.send_packet(
        &CUpdateScore {
            entity_name: text.into(),
            action,
            objective_name: OBJECTIVE.into(),
            value,
        }
        .encode(),
    );
}

fn selection_lines(first: Option<BlockPos>, second: Option<BlockPos>) -> Vec<String> {
    let (Some(first), Some(second)) = (first, second) else {
        return vec![messages::SELECTION_SIDEBAR_INCOMPLETE.into()];
    };
    let width = u64::from(first.x.abs_diff(second.x)) + 1;
    let height = u64::from(first.y.abs_diff(second.y)) + 1;
    let depth = u64::from(first.z.abs_diff(second.z)) + 1;
    let volume = u128::from(width) * u128::from(height) * u128::from(depth);
    let color = match volume {
        0..=4096 => "§a",
        4097..=1_000_000 => "§e",
        _ => "§c",
    };
    vec![
        format!(
            "§b{}",
            messages::selection_sidebar_dimensions(width, height, depth)
        ),
        format!("{color}{}", messages::selection_sidebar_volume(volume)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_dimensions_are_inclusive_and_order_independent() {
        let first = Some(BlockPos::new(3, 4, 5));
        let second = Some(BlockPos::new(1, 2, 3));
        assert_eq!(
            selection_lines(first, second),
            selection_lines(second, first)
        );
        assert!(selection_lines(first, second)[1].contains("27"));
        assert_eq!(selection_lines(first, first)[1], "§aVolume: 1");
        assert_eq!(
            selection_lines(first, None),
            [messages::SELECTION_SIDEBAR_INCOMPLETE]
        );
        selection_lines(
            Some(BlockPos::new(i32::MIN, i32::MIN, i32::MIN)),
            Some(BlockPos::new(i32::MAX, i32::MAX, i32::MAX)),
        );
    }
}
