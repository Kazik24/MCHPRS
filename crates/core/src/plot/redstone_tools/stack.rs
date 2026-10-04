use super::{Plot, PlotWorld, SelectionBounds, ToolNotice};
use crate::messages;
use crate::player::Player;
use crate::plot::worldedit::{self, AirPolicy};
use anyhow::{bail, Context, Result};
use mchprs_blocks::{BlockFacing, BlockPos};

const MAX_COPIES: u32 = 4096;
const MAX_STACK_BLOCKS: u64 = 16_777_216;

#[derive(Clone, Copy, Debug)]
enum SelectionPolicy {
    Keep,
    Expand,
}

#[derive(Debug)]
struct RStackRequest {
    count: u32,
    spacing: i32,
    direction: BlockPos,
    air: AirPolicy,
    selection: SelectionPolicy,
}

impl RStackRequest {
    fn parse(args: &[&str], look: BlockPos) -> Result<Self> {
        let mut numbers = Vec::new();
        let mut direction = None;
        let mut air = AirPolicy::Ignore;
        let mut selection = SelectionPolicy::Keep;
        for &argument in args {
            if let Ok(number) = argument.parse::<i32>() {
                numbers.push(number);
                continue;
            }
            if argument.starts_with('-') {
                if argument == "-" {
                    bail!(messages::FLAG_NAME_MUST_FOLLOW);
                }
                for flag in argument[1..].chars() {
                    match flag {
                        'a' | 'w' => air = AirPolicy::Copy,
                        'e' => selection = SelectionPolicy::Expand,
                        _ => bail!(messages::unknown_stack_flag(flag)),
                    }
                }
                continue;
            }
            if direction.is_some() {
                bail!(messages::SPECIFY_ONLY_ONE_DIRECTION);
            }
            direction = Some(parse_direction(argument, look)?);
        }
        if numbers.len() > 2 {
            bail!(messages::USAGE_RSTACK_DIRECTION_COUNT_SPACING_E);
        }
        let count = numbers.first().copied().unwrap_or(1);
        let mut spacing = numbers.get(1).copied().unwrap_or(2);
        if count < 0 {
            spacing = spacing
                .checked_neg()
                .context(messages::SPACING_OVERFLOWS_WHEN_REVERSING_DIRECTION)?;
        }
        let count = count.unsigned_abs();
        if count > MAX_COPIES {
            bail!(messages::stack_copy_limit(MAX_COPIES));
        }
        Ok(Self {
            count,
            spacing,
            direction: direction.unwrap_or(look),
            air,
            selection,
        })
    }

    fn destinations(
        &self,
        bounds: SelectionBounds,
        world: &PlotWorld,
    ) -> Result<Vec<SelectionBounds>> {
        if bounds.volume() * u64::from(self.count) > MAX_STACK_BLOCKS {
            bail!(messages::stack_block_limit(MAX_STACK_BLOCKS));
        }
        let mut destinations = Vec::new();
        for index in 1..=self.count {
            let distance = i64::from(self.spacing) * i64::from(index);
            let start = translated(bounds.start, self.direction, distance)?;
            let end = translated(bounds.end, self.direction, distance)?;
            destinations.push(SelectionBounds::new(start, end, world)?);
        }
        Ok(destinations)
    }
}

impl Plot {
    pub(super) fn redstone_stack(&mut self, player: usize, args: &[&str]) -> Result<()> {
        let look = look_direction(&self.players[player]);
        let request = RStackRequest::parse(args, look)?;
        let bounds = SelectionBounds::from_player(&self.players[player], &self.world)?;
        let destinations = request.destinations(bounds, &self.world)?;
        if destinations.is_empty() {
            ToolNotice::Stacked(0).send(&self.players[player]);
            return Ok(());
        }

        self.reset_redpiler();
        let destinations: Vec<_> = destinations
            .iter()
            .map(|area| (area.start, area.end))
            .collect();
        let undo = worldedit::stack_prepared(
            &mut self.world,
            bounds.start,
            bounds.end,
            &destinations,
            request.air,
        );
        let player = &mut self.players[player];
        player.worldedit_undo.push(undo);
        player.worldedit_redo.clear();

        if matches!(request.selection, SelectionPolicy::Expand) {
            let last = destinations
                .last()
                .expect("nonempty validated destinations");
            player.worldedit_set_first_position(bounds.start.min(last.0));
            player.worldedit_set_second_position(bounds.end.max(last.1));
        }
        ToolNotice::Stacked(request.count).send(player);
        Ok(())
    }
}

fn translated(position: BlockPos, direction: BlockPos, distance: i64) -> Result<BlockPos> {
    let x = translated_axis(position.x, direction.x, distance)?;
    let y = translated_axis(position.y, direction.y, distance)?;
    let z = translated_axis(position.z, direction.z, distance)?;
    Ok(BlockPos::new(
        i32::try_from(x).context(messages::STACK_X_COORDINATE_OVERFLOWS)?,
        i32::try_from(y).context(messages::STACK_Y_COORDINATE_OVERFLOWS)?,
        i32::try_from(z).context(messages::STACK_Z_COORDINATE_OVERFLOWS)?,
    ))
}

fn translated_axis(position: i32, direction: i32, distance: i64) -> Result<i64> {
    i64::from(direction)
        .checked_mul(distance)
        .and_then(|offset| i64::from(position).checked_add(offset))
        .context(messages::STACK_COORDINATE_OVERFLOWS)
}

fn look_direction(player: &Player) -> BlockPos {
    if player.pitch.abs() > 67.5 {
        return match player.get_facing() {
            BlockFacing::Up => BlockPos::new(0, 1, 0),
            _ => BlockPos::new(0, -1, 0),
        };
    }
    let yaw = f64::from(player.yaw).to_radians();
    let x = -yaw.sin();
    let z = yaw.cos();
    let mut direction = BlockPos::new(0, 0, 0);
    if x.abs() >= 0.3826834323650898 {
        direction.x = x.signum() as i32;
    }
    if z.abs() >= 0.3826834323650898 {
        direction.z = z.signum() as i32;
    }
    if player.pitch.abs() > 22.5 {
        direction.y = match player.pitch < 0.0 {
            true => 1,
            false => -1,
        };
    }
    direction
}

fn parse_direction(token: &str, look: BlockPos) -> Result<BlockPos> {
    let token = token.to_ascii_lowercase();
    let horizontal_look = BlockPos::new(look.x, 0, look.z);
    let base = match token.as_str() {
        "me" => return Ok(look),
        "u" | "up" => return Ok(BlockPos::new(0, 1, 0)),
        "d" | "down" => return Ok(BlockPos::new(0, -1, 0)),
        other => other,
    };
    let (base, vertical) = match base.chars().last() {
        Some('u') => (&base[..base.len() - 1], 1),
        Some('d') if base != "forward" && base != "backward" => (&base[..base.len() - 1], -1),
        _ => (base, 0),
    };
    let mut direction = match base {
        "n" | "north" => BlockPos::new(0, 0, -1),
        "s" | "south" => BlockPos::new(0, 0, 1),
        "e" | "east" => BlockPos::new(1, 0, 0),
        "w" | "west" => BlockPos::new(-1, 0, 0),
        "ne" | "northeast" => BlockPos::new(1, 0, -1),
        "nw" | "northwest" => BlockPos::new(-1, 0, -1),
        "se" | "southeast" => BlockPos::new(1, 0, 1),
        "sw" | "southwest" => BlockPos::new(-1, 0, 1),
        "f" | "forward" => horizontal_look,
        "b" | "back" | "backward" => horizontal_look * -1,
        "l" | "left" => BlockPos::new(horizontal_look.z, 0, -horizontal_look.x),
        "r" | "right" => BlockPos::new(-horizontal_look.z, 0, horizontal_look.x),
        _ => bail!(messages::unknown_direction(token)),
    };
    direction.y = vertical;
    if direction == BlockPos::new(0, 0, 0) {
        bail!(messages::LOOK_HORIZONTALLY_BEFORE_USING_RELATIVE_DIRECTION);
    }
    Ok(direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_arguments_are_flexible_signed_and_strict() {
        let look = BlockPos::new(0, 0, 1);
        let request = RStackRequest::parse(&["-ew", "neu", "-3", "-2"], look).unwrap();
        assert_eq!(request.count, 3);
        assert_eq!(request.spacing, 2);
        assert_eq!(request.direction, BlockPos::new(1, 1, -1));
        assert!(matches!(request.air, AirPolicy::Copy));
        let defaults = RStackRequest::parse(&[], look).unwrap();
        assert_eq!(defaults.count, 1);
        assert_eq!(defaults.spacing, 2);
        for args in [
            vec!["sideways"],
            vec!["1", "2", "3"],
            vec!["-q"],
            vec!["-1", "-2147483648"],
            vec!["2147483648"],
        ] {
            assert!(RStackRequest::parse(&args, look).is_err(), "{args:?}");
        }
        assert!(translated(BlockPos::new(i32::MAX, 1, 1), look, i64::MAX).is_err());
    }

    #[test]
    fn every_destination_is_checked_before_mutation() {
        let world = super::super::tests::test_world();
        let start = BlockPos::new(250, 10, 10);
        let bounds = SelectionBounds::new(start, start, &world).unwrap();
        let request = RStackRequest::parse(&["east", "4", "2"], BlockPos::new(0, 0, 1)).unwrap();
        assert!(request.destinations(bounds, &world).is_err());
        let valid = RStackRequest::parse(&["-2", "2", "east"], BlockPos::new(0, 0, 1)).unwrap();
        let destinations = valid.destinations(bounds, &world).unwrap();
        assert_eq!(destinations[0].start.x, 248);
        assert_eq!(destinations[1].start.x, 246);
    }
}
