use super::{Plot, PlotWorld, SelectionBounds, ToolNotice};
use crate::messages;
use crate::player::Player;
use crate::plot::worldedit::{self, AirPolicy};
use crate::world::World;
use anyhow::{bail, Context, Result};
use mchprs_blocks::blocks::Block;
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
        Self::parse_mode(args, look, false)
    }

    fn parse_mode(args: &[&str], look: BlockPos, live: bool) -> Result<Self> {
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
                        'a' | 'w' if !live => air = AirPolicy::Copy,
                        'a' | 'w' => bail!(messages::AUTO_STACK_ALWAYS_COPIES_REMOVALS),
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
            bail!(if live {
                messages::USAGE_AUTOSTACK
            } else {
                messages::USAGE_RSTACK_DIRECTION_COUNT_SPACING_E
            });
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
        if bounds.volume() * (u64::from(self.count) + 1)
            > MAX_STACK_BLOCKS.min(crate::config::CONFIG.worldedit_max_blocks)
        {
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

/// A fixed source selection and validated translations, belonging to one player
/// in one plot. Only explicit player placements/removals invoke this session.
pub(crate) struct AutoStack {
    bounds: SelectionBounds,
    offsets: Vec<BlockPos>,
}

impl AutoStack {
    fn new(bounds: SelectionBounds, destinations: &[SelectionBounds]) -> Result<Self> {
        let offsets: Vec<_> = destinations
            .iter()
            .map(|area| area.start - bounds.start)
            .collect();
        if offsets.is_empty() || offsets.contains(&BlockPos::new(0, 0, 0)) {
            bail!(messages::AUTO_STACK_NEEDS_NONZERO_COPIES_AND_SPACING);
        }
        Ok(Self { bounds, offsets })
    }

    fn destinations(&self, pos: BlockPos) -> impl Iterator<Item = BlockPos> + '_ {
        let inside = pos.min(self.bounds.start) == self.bounds.start
            && pos.max(self.bounds.end) == self.bounds.end;
        self.offsets
            .iter()
            .filter(move |_| inside)
            .map(move |&offset| pos + offset)
    }

    pub(crate) fn mirror(&self, world: &mut PlotWorld, pos: BlockPos) {
        // Snapshot once: overlapping destinations must not change the source for
        // later copies. Going through placement/destruction updates attachments,
        // redstone and block entities without triggering another auto stack.
        let block = world.get_block(pos);
        let entity = world.get_block_entity(pos).cloned();
        for destination in self.destinations(pos) {
            let old = world.get_block(destination);
            if !matches!(old, Block::Air {}) {
                crate::interaction::destroy(old, world, destination);
            }
            if !matches!(block, Block::Air {}) {
                world.delete_block_entity(destination);
                world.set_block(destination, block);
                if let Some(entity) = &entity {
                    // Clients need the block before its entity packet.
                    world.flush_block_changes();
                    world.set_block_entity(destination, entity.clone());
                }
                crate::interaction::place_in_world(block, world, destination, &None);
            }
        }
    }
}

impl Plot {
    pub(super) fn auto_stack(&mut self, player: usize, args: &[&str]) -> Result<()> {
        if args == ["off"] {
            self.players[player].redstone_tools.auto_stack = None;
            ToolNotice::AutoStackDisabled.send(&self.players[player]);
            return Ok(());
        }
        let request = RStackRequest::parse_mode(args, look_direction(&self.players[player]), true)?;
        let bounds = SelectionBounds::from_player(&self.players[player], &self.world)?;
        let destinations = request.destinations(bounds, &self.world)?;
        let session = AutoStack::new(bounds, &destinations)?;
        let player = &mut self.players[player];
        player.redstone_tools.auto_stack = Some(session);
        if matches!(request.selection, SelectionPolicy::Expand) {
            let last = destinations
                .last()
                .expect("nonempty auto stack destinations");
            player.worldedit_set_first_position(bounds.start.min(last.start));
            player.worldedit_set_second_position(bounds.end.max(last.end));
        }
        ToolNotice::AutoStackEnabled.send(player);
        Ok(())
    }

    pub(in crate::plot) fn mirror_auto_stack(&mut self, player: usize, pos: BlockPos) {
        if self.players[player].redstone_tools.auto_stack.is_none() {
            return;
        }
        if self
            .check_tool_access(player, super::ToolCommand::AutoStack)
            .is_err()
        {
            self.players[player].redstone_tools.auto_stack = None;
            ToolNotice::AutoStackDisabled.send(&self.players[player]);
            return;
        }
        if let Some(session) = &self.players[player].redstone_tools.auto_stack {
            session.mirror(&mut self.world, pos);
        }
    }

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
        worldedit::trim_history(player);

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
        direction.y = if player.pitch < 0.0 { 1 } else { -1 };
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

    fn session(world: &PlotWorld, start: BlockPos, end: BlockPos, args: &[&str]) -> AutoStack {
        let bounds = SelectionBounds::new(start, end, world).unwrap();
        let request = RStackRequest::parse_mode(args, BlockPos::new(0, 0, 1), true).unwrap();
        AutoStack::new(bounds, &request.destinations(bounds, world).unwrap()).unwrap()
    }

    #[test]
    fn auto_stack_overlaps_without_recursive_copies_and_ignores_outside_edits() {
        let mut world = super::super::tests::test_world();
        let pos = BlockPos::new(10, 30, 10);
        let stack = session(&world, pos, BlockPos::new(12, 30, 10), &["east", "2", "1"]);
        crate::interaction::place_in_world(Block::Stone {}, &mut world, pos, &None);
        stack.mirror(&mut world, pos);
        for x in 10..=12 {
            assert_eq!(world.get_block(BlockPos::new(x, 30, 10)), Block::Stone {});
        }
        assert_eq!(world.get_block(BlockPos::new(13, 30, 10)), Block::Air {});
        let outside = BlockPos::new(13, 30, 10);
        world.set_block(outside, Block::Sand {});
        stack.mirror(&mut world, outside);
        assert_eq!(world.get_block(BlockPos::new(14, 30, 10)), Block::Air {});
        crate::interaction::destroy(Block::Stone {}, &mut world, pos);
        stack.mirror(&mut world, pos);
        for x in 10..=12 {
            assert_eq!(world.get_block(BlockPos::new(x, 30, 10)), Block::Air {});
        }
    }

    #[test]
    fn auto_stack_copies_container_data_and_cleans_replaced_entities() {
        use mchprs_blocks::block_entities::{ContainerType, SignalStrength};
        use mchprs_blocks::items::ItemStack;
        let mut world = super::super::tests::test_world();
        let pos = BlockPos::new(10, 30, 10);
        let stack = session(&world, pos, pos, &["east", "2", "2"]);
        let item =
            ItemStack::container_with_ss(ContainerType::Chest, SignalStrength::new(13).unwrap());
        crate::interaction::place_in_world(
            Block::from_name("chest").unwrap(),
            &mut world,
            pos,
            &item.nbt,
        );
        stack.mirror(&mut world, pos);
        for x in [12, 14] {
            let destination = BlockPos::new(x, 30, 10);
            assert_eq!(
                crate::redstone::comparator::get_override(
                    world.get_block(destination),
                    &world,
                    destination
                ),
                13
            );
            assert_eq!(
                bincode::serialize(world.get_block_entity(destination).unwrap()).unwrap(),
                bincode::serialize(world.get_block_entity(pos).unwrap()).unwrap()
            );
        }
        crate::interaction::destroy(world.get_block(pos), &mut world, pos);
        crate::interaction::place_in_world(Block::Stone {}, &mut world, pos, &None);
        stack.mirror(&mut world, pos);
        for x in [12, 14] {
            let destination = BlockPos::new(x, 30, 10);
            assert_eq!(world.get_block(destination), Block::Stone {});
            assert!(world.get_block_entity(destination).is_none());
        }
    }

    #[test]
    fn auto_stack_updates_redstone_at_the_destinations() {
        let mut world = super::super::tests::test_world();
        let pos = BlockPos::new(10, 30, 10);
        let stack = session(&world, pos, pos, &["east", "1", "4"]);
        let lamp = BlockPos::new(15, 30, 10);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        crate::interaction::place_in_world(Block::RedstoneBlock {}, &mut world, pos, &None);
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
        stack.mirror(&mut world, pos);
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: true });
    }

    #[test]
    fn auto_stack_rejects_noop_and_out_of_bounds_configurations() {
        let world = super::super::tests::test_world();
        let pos = BlockPos::new(250, 30, 10);
        let bounds = SelectionBounds::new(pos, pos, &world).unwrap();
        for args in [["east", "0", "2"], ["east", "2", "0"]] {
            let request = RStackRequest::parse_mode(&args, BlockPos::new(0, 0, 1), true).unwrap();
            assert!(
                AutoStack::new(bounds, &request.destinations(bounds, &world).unwrap()).is_err()
            );
        }
        let request =
            RStackRequest::parse_mode(&["east", "4", "2"], BlockPos::new(0, 0, 1), true).unwrap();
        assert!(request.destinations(bounds, &world).is_err());
        for flag in ["-w", "-a"] {
            assert!(RStackRequest::parse_mode(&[flag], BlockPos::new(0, 0, 1), true).is_err());
        }
        let stack = session(&world, pos, pos, &["east", "-2", "2"]);
        assert_eq!(
            stack.destinations(pos).collect::<Vec<_>>(),
            [BlockPos::new(248, 30, 10), BlockPos::new(246, 30, 10)]
        );
    }

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
