//! Validate geometry and work before capturing undo data or mutating a plot.
use super::*;
use crate::config::CONFIG;
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH};

const HORIZONTAL_PASTE_MARGIN: i64 = 2;

fn volume(first: BlockPos, second: BlockPos) -> Result<u64, String> {
    [first.x.abs_diff(second.x), first.y.abs_diff(second.y), first.z.abs_diff(second.z)]
        .into_iter()
        .try_fold(1u64, |size, axis| size.checked_mul(u64::from(axis) + 1))
        .ok_or_else(|| messages::WE_SELECTION_VOLUME_OVERFLOW.into())
}

fn region(world: &PlotWorld, first: BlockPos, second: BlockPos) -> Result<u64, String> {
    for pos in [first, second] {
        if !Plot::in_plot_bounds(world.x, world.z, pos.x, pos.z) || !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y) {
            return Err(messages::WE_OPERATION_OUTSIDE_PLOT.into());
        }
    }
    let blocks = volume(first, second)?;
    if blocks > CONFIG.worldedit_max_blocks {
        return Err(messages::worldedit_work_limit(CONFIG.worldedit_max_blocks));
    }
    Ok(blocks)
}

fn clipboard_geometry(cb: &WorldEditClipboard, pos: BlockPos) -> Result<(BlockPos, BlockPos), String> {
    let overflow = || messages::WE_CLIPBOARD_COORDINATES_OVERFLOW.to_owned();
    let mut first = [0; 3];
    let mut second = [0; 3];
    for (i, (coordinate, offset, size)) in [
        (pos.x, cb.offset_x, cb.size_x),
        (pos.y, cb.offset_y, cb.size_y),
        (pos.z, cb.offset_z, cb.size_z),
    ]
    .into_iter()
    .enumerate()
    {
        if size == 0 {
            return Err(messages::WE_EMPTY_CLIPBOARD_DIMENSION.into());
        }
        first[i] = coordinate.checked_sub(offset).ok_or_else(overflow)?;
        second[i] = first[i]
            .checked_add(i32::try_from(size - 1).map_err(|_| overflow())?)
            .ok_or_else(overflow)?;
    }
    Ok((
        BlockPos::new(first[0], first[1], first[2]),
        BlockPos::new(second[0], second[1], second[2]),
    ))
}

fn paste_has_margin(world: &PlotWorld, first: BlockPos, last: BlockPos) -> bool {
    let width = i64::from(PLOT_BLOCK_WIDTH);
    let min_x = i64::from(world.x) * width + HORIZONTAL_PASTE_MARGIN;
    let min_z = i64::from(world.z) * width + HORIZONTAL_PASTE_MARGIN;
    let max_x = i64::from(world.x) * width + width - 1 - HORIZONTAL_PASTE_MARGIN;
    let max_z = i64::from(world.z) * width + width - 1 - HORIZONTAL_PASTE_MARGIN;
    i64::from(first.x) >= min_x && i64::from(last.x) <= max_x && i64::from(first.z) >= min_z && i64::from(last.z) <= max_z
}

fn fitting_position(world: &PlotWorld, cb: &WorldEditClipboard, current: BlockPos) -> Option<BlockPos> {
    let mut coordinates = [0; 3];
    for (index, (plot, capacity, size, offset, current)) in [
        (
            i64::from(world.x) * i64::from(PLOT_BLOCK_WIDTH),
            PLOT_BLOCK_WIDTH,
            cb.size_x,
            cb.offset_x,
            current.x,
        ),
        (0, PLOT_BLOCK_HEIGHT, cb.size_y, cb.offset_y, current.y),
        (
            i64::from(world.z) * i64::from(PLOT_BLOCK_WIDTH),
            PLOT_BLOCK_WIDTH,
            cb.size_z,
            cb.offset_z,
            current.z,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if size == 0 || i64::from(size) > i64::from(capacity) {
            return None;
        }
        // The paste must fit, and its player anchor must remain inside this plot.
        let margin = if index == 1 { 0 } else { HORIZONTAL_PASTE_MARGIN };
        let low = (plot + i64::from(offset) + margin).max(plot);
        let high = (plot + i64::from(capacity) - margin - i64::from(size) + i64::from(offset)).min(plot + i64::from(capacity) - 1);
        if low > high {
            return None;
        }
        coordinates[index] = i32::try_from(i64::from(current).clamp(low, high)).ok()?;
    }
    Some(BlockPos::new(coordinates[0], coordinates[1], coordinates[2]))
}

pub(super) fn paste_placement_help(world: &PlotWorld, cb: &WorldEditClipboard, current: BlockPos) -> serde_json::Value {
    use serde_json::json;
    let mut text = messages::clipboard_dimensions(cb.size_x, cb.size_y, cb.size_z);
    if let Ok((first, last)) = clipboard_geometry(cb, current) {
        text.push_str(&format!("\n{}", messages::clipboard_paste_bounds(first, last)));
    }
    text.push('\n');
    let paste_width = PLOT_BLOCK_WIDTH - 2 * HORIZONTAL_PASTE_MARGIN as i32;
    if cb.size_x > paste_width as u32 || cb.size_y > PLOT_BLOCK_HEIGHT as u32 || cb.size_z > paste_width as u32 {
        text.push_str(&messages::clipboard_too_large(paste_width, PLOT_BLOCK_HEIGHT, paste_width));
        return json!({"text": text, "color": "yellow"});
    }
    let Some(position) = fitting_position(world, cb, current) else {
        text.push_str(messages::WE_PASTE_ORIGIN_OUTSIDE_PLOT);
        return json!({"text": text, "color": "yellow"});
    };
    if position == current {
        text.push_str(messages::WE_PASTE_FITS_HERE);
        return json!({"text": text, "color": "yellow"});
    }
    text.push_str(&messages::clipboard_stand_at(position));
    json!({"text": text, "color": "yellow", "extra": [{
        "text": messages::WE_TELEPORT_TO_FIT, "color": "aqua", "underlined": true,
        "click_event": {"action": "run_command", "command": format!("/tp {} {} {}", position.x, position.y, position.z)},
        "hover_event": {"action": "show_text", "value": {"text": messages::WE_TELEPORT_TO_FIT_TOOLTIP}}
    }]})
}

pub(super) fn validate_request(world: &PlotWorld, player: &Player, name: &str, command: &WorldeditCommand, args: &[Argument]) -> Result<(), String> {
    // Every numeric command argument stays small enough for downstream i32 math.
    if args.iter().any(|arg| matches!(arg, Argument::UnsignedInteger(n) if *n > 4096)) {
        return Err(messages::WE_NUMERIC_ARGUMENT_LIMIT.into());
    }
    let mut source = 0;
    if command.requires_positions {
        source = region(world, player.first_position.unwrap(), player.second_position.unwrap())?;
    }
    if let Some(cb) = player.worldedit_clipboard.as_ref().filter(|_| command.requires_clipboard) {
        if [cb.offset_x, cb.offset_y, cb.offset_z]
            .iter()
            .any(|offset| offset.unsigned_abs() > 30_000_000)
        {
            return Err(messages::WE_CLIPBOARD_OFFSET_OUTSIDE_RANGE.into());
        }
        let (first, second) = clipboard_geometry(cb, BlockPos::zero())?;
        let blocks = volume(first, second)?;
        if blocks > CONFIG.worldedit_max_blocks || blocks != cb.data.entries() as u64 {
            return Err(messages::WE_INVALID_CLIPBOARD_WORK.into());
        }
        if name == "/paste" {
            let (first, second) = clipboard_geometry(cb, player.pos.block_pos())?;
            region(world, first, second)?;
            if !paste_has_margin(world, first, second) {
                return Err(messages::WE_PASTE_MARGIN.into());
            }
        }
    }
    if matches!(name, "/move" | "/stack") {
        let count = args[0].unwrap_uint();
        let direction = args[1].unwrap_direction();
        let first = player.first_position.unwrap();
        let second = player.second_position.unwrap();
        let distance = if name == "/stack" {
            if source
                .checked_mul(u64::from(count) + 1)
                .is_none_or(|work| work > CONFIG.worldedit_max_blocks)
            {
                return Err(messages::WE_STACK_WORK_LIMIT.into());
            }
            let size = match direction {
                BlockFacing::East | BlockFacing::West => first.x.abs_diff(second.x) + 1,
                BlockFacing::Up | BlockFacing::Down => first.y.abs_diff(second.y) + 1,
                _ => first.z.abs_diff(second.z) + 1,
            };
            count.checked_mul(size).ok_or(messages::WE_STACK_DISTANCE_OVERFLOW)?
        } else {
            count
        };
        region(
            world,
            direction.offset_pos(first, distance as i32),
            direction.offset_pos(second, distance as i32),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{PLOT_BLOCK_WIDTH, PLOT_WIDTH};
    use crate::world::storage::Chunk;

    fn placement_clipboard() -> WorldEditClipboard {
        WorldEditClipboard {
            offset_x: 7,
            offset_y: 5,
            offset_z: -4,
            size_x: 20,
            size_y: 30,
            size_z: 40,
            data: PalettedBitBuffer::new(20 * 30 * 40, 9),
            block_entities: Default::default(),
        }
    }

    #[test]
    fn paste_teleport_honors_offsets_negative_plots_and_world_height() {
        let world = PlotWorld::from_chunks(-2, 3, vec![], Default::default());
        let cb = placement_clipboard();
        let current = BlockPos::new(-513, PLOT_BLOCK_HEIGHT - 1, 1535);
        let destination = fitting_position(&world, &cb, current).unwrap();
        assert_eq!(destination, BlockPos::new(-527, PLOT_BLOCK_HEIGHT - 25, 1536));
        let (first, last) = clipboard_geometry(&cb, destination).unwrap();
        assert!(region(&world, first, last).is_ok());
        assert!(paste_has_margin(&world, first, last));
        assert!(Plot::in_plot_bounds(world.x, world.z, destination.x, destination.z));
        let help = paste_placement_help(&world, &cb, current);
        assert_eq!(
            help["extra"][0]["click_event"]["command"],
            format!("/tp {} {} {}", destination.x, destination.y, destination.z)
        );
        assert!(paste_placement_help(&world, &cb, destination).get("extra").is_none());
    }

    #[test]
    fn paste_keeps_two_block_horizontal_margins_and_rejects_width_509() {
        let world = PlotWorld::from_chunks(-1, 2, vec![], Default::default());
        let mut cb = placement_clipboard();
        cb.offset_x = 0;
        cb.offset_y = 0;
        cb.offset_z = 0;
        cb.size_x = (PLOT_BLOCK_WIDTH - 2 * HORIZONTAL_PASTE_MARGIN as i32) as u32;
        cb.size_y = 1;
        cb.size_z = 1;
        cb.data = PalettedBitBuffer::new(508, 9);

        let destination = fitting_position(&world, &cb, BlockPos::zero()).unwrap();
        let (first, last) = clipboard_geometry(&cb, destination).unwrap();
        assert_eq!(first.x, world.x * PLOT_BLOCK_WIDTH + 2);
        assert_eq!(last.x, (world.x + 1) * PLOT_BLOCK_WIDTH - 3);
        assert!(paste_has_margin(&world, first, last));

        cb.size_x += 1;
        cb.data = PalettedBitBuffer::new(cb.size_x as usize, 9);
        assert!(fitting_position(&world, &cb, BlockPos::zero()).is_none());
        let help = paste_placement_help(&world, &cb, BlockPos::zero());
        assert!(help.get("extra").is_none());
        assert!(help["text"].as_str().unwrap().contains("508 × 256 × 508"));
    }

    #[test]
    fn paste_help_does_not_offer_teleports_for_impossible_sizes_or_origins() {
        let world = PlotWorld::from_chunks(0, 0, vec![], Default::default());
        let mut cb = placement_clipboard();
        cb.size_y = PLOT_BLOCK_HEIGHT as u32 + 17;
        assert!(fitting_position(&world, &cb, BlockPos::zero()).is_none());
        let help = paste_placement_help(&world, &cb, BlockPos::zero());
        assert!(help.get("extra").is_none());
        assert!(help["text"].as_str().unwrap().contains("cannot fit"));
        cb.size_y = 30;
        cb.offset_x = 1000;
        assert!(fitting_position(&world, &cb, BlockPos::zero()).is_none());
        assert!(paste_placement_help(&world, &cb, BlockPos::zero()).get("extra").is_none());
    }

    #[test]
    fn pm1_and_full_plot_clips_respect_the_work_limit_and_paste_margin() {
        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
        let connection = mchprs_network::test_support::connection(false).unwrap();
        let mut player = Player::test_player(connection.player);
        let cb = load_schematic(std::io::Cursor::new(include_bytes!(
            "../../../../../test_data/instant-pistons/PM1_SORT.schem"
        )))
        .unwrap();
        assert_eq!((cb.size_x, cb.size_y, cb.size_z), (235, 202, 182));
        assert_eq!(cb.data.entries(), 8_639_540);
        let pm1_work = cb.data.entries() as u64;
        player.pos = PlayerPos {
            x: f64::from(cb.offset_x) + 2.5,
            y: f64::from(cb.offset_y),
            z: f64::from(cb.offset_z) + 2.5,
        };
        player.worldedit_clipboard = Some(cb);
        let paste = &COMMANDS["/paste"];
        assert_eq!(
            validate_request(&world, &player, "/paste", paste, &[]).is_ok(),
            pm1_work <= CONFIG.worldedit_max_blocks
        );
        if pm1_work > CONFIG.worldedit_max_blocks {
            return;
        }

        player.first_position = Some(BlockPos::zero());
        player.second_position = Some(BlockPos::new(234, 201, 181));
        let copy = &COMMANDS["/copy"];
        assert!(validate_request(&world, &player, "/copy", copy, &[]).is_ok());
        execute_copy(CommandExecuteContext {
            plot: &mut world,
            player: &mut player,
            arguments: vec![],
            flags: vec![],
        });
        assert_eq!(player.worldedit_clipboard.as_ref().unwrap().data.entries(), 8_639_540);
        let clipboard = player.worldedit_clipboard.as_ref().unwrap();
        player.pos.x = f64::from(clipboard.offset_x) + 2.5;
        player.pos.z = f64::from(clipboard.offset_z) + 2.5;
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_ok());

        player.pos = PlayerPos { x: 0.0, y: 0.0, z: 0.0 };
        player.worldedit_clipboard = Some(WorldEditClipboard {
            offset_x: 0,
            offset_y: 0,
            offset_z: 0,
            size_x: PLOT_BLOCK_WIDTH as u32,
            size_y: PLOT_BLOCK_HEIGHT as u32,
            size_z: PLOT_BLOCK_WIDTH as u32,
            data: PalettedBitBuffer::new((PLOT_BLOCK_WIDTH * PLOT_BLOCK_HEIGHT * PLOT_BLOCK_WIDTH) as usize, 9),
            block_entities: Default::default(),
        });
        let full_plot_work = volume(
            BlockPos::zero(),
            BlockPos::new(PLOT_BLOCK_WIDTH - 1, PLOT_BLOCK_HEIGHT - 1, PLOT_BLOCK_WIDTH - 1),
        )
        .unwrap();
        let full_plot_paste = validate_request(&world, &player, "/paste", paste, &[]);
        if full_plot_work > CONFIG.worldedit_max_blocks {
            assert!(full_plot_paste.is_err());
            return;
        }
        assert_eq!(full_plot_paste, Err(messages::WE_PASTE_MARGIN.to_owned()));
        player.second_position = Some(BlockPos::new(PLOT_BLOCK_WIDTH - 1, PLOT_BLOCK_HEIGHT - 1, PLOT_BLOCK_WIDTH - 1));
        assert!(validate_request(&world, &player, "/copy", copy, &[]).is_ok());
        player.pos.x = 1.0;
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_err());
        player.worldedit_clipboard.as_mut().unwrap().size_x -= 1;
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_err());
        player.worldedit_clipboard.as_mut().unwrap().size_x += 2;
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_err());
    }

    #[test]
    fn malicious_clipboard_offsets_and_volumes_are_rejected() {
        assert!(volume(BlockPos::new(i32::MIN, i32::MIN, i32::MIN), BlockPos::new(i32::MAX, i32::MAX, i32::MAX)).is_err());
        let cb = WorldEditClipboard {
            offset_x: i32::MIN,
            offset_y: 0,
            offset_z: 0,
            size_x: 1,
            size_y: 1,
            size_z: 1,
            data: PalettedBitBuffer::new(1, 9),
            block_entities: Default::default(),
        };
        assert!(clipboard_geometry(&cb, BlockPos::zero()).is_err());
    }
    #[test]
    fn out_of_height_and_cross_plot_regions_are_rejected() {
        let world = PlotWorld::from_chunks(0, 0, vec![], Default::default());
        assert!(region(&world, BlockPos::zero(), BlockPos::new(1, -1, 1)).is_err());
        assert!(region(&world, BlockPos::zero(), BlockPos::new(PLOT_BLOCK_WIDTH, 1, 1)).is_err());
        assert_eq!(region(&world, BlockPos::zero(), BlockPos::new(1, 1, 1)).unwrap(), 8);
    }
}
