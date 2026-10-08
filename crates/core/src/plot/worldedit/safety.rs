//! Validate geometry and work before capturing undo data or mutating a plot.
use super::*;
use crate::config::CONFIG;
use crate::plot::PLOT_BLOCK_HEIGHT;

fn volume(first: BlockPos, second: BlockPos) -> Result<u64, String> {
    [
        first.x.abs_diff(second.x),
        first.y.abs_diff(second.y),
        first.z.abs_diff(second.z),
    ]
    .into_iter()
    .try_fold(1u64, |size, axis| size.checked_mul(u64::from(axis) + 1))
    .ok_or_else(|| messages::WE_SELECTION_VOLUME_OVERFLOW.into())
}

fn region(world: &PlotWorld, first: BlockPos, second: BlockPos) -> Result<u64, String> {
    for pos in [first, second] {
        if !Plot::in_plot_bounds(world.x, world.z, pos.x, pos.z)
            || !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y)
        {
            return Err(messages::WE_OPERATION_OUTSIDE_PLOT.into());
        }
    }
    let blocks = volume(first, second)?;
    if blocks > CONFIG.worldedit_max_blocks {
        return Err(messages::worldedit_work_limit(CONFIG.worldedit_max_blocks));
    }
    Ok(blocks)
}

fn clipboard_geometry(
    cb: &WorldEditClipboard,
    pos: BlockPos,
) -> Result<(BlockPos, BlockPos), String> {
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

pub(super) fn validate_request(
    world: &PlotWorld,
    player: &Player,
    name: &str,
    command: &WorldeditCommand,
    args: &[Argument],
) -> Result<(), String> {
    // Every numeric command argument stays small enough for downstream i32 math.
    if args
        .iter()
        .any(|arg| matches!(arg, Argument::UnsignedInteger(n) if *n > 4096))
    {
        return Err(messages::WE_NUMERIC_ARGUMENT_LIMIT.into());
    }
    let mut source = 0;
    if command.requires_positions {
        source = region(
            world,
            player.first_position.unwrap(),
            player.second_position.unwrap(),
        )?;
    }
    if let Some(cb) = player
        .worldedit_clipboard
        .as_ref()
        .filter(|_| command.requires_clipboard)
    {
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
            count
                .checked_mul(size)
                .ok_or(messages::WE_STACK_DISTANCE_OVERFLOW)?
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
    #[test]
    fn pm1_sort_and_full_plot_clipboards_fit_the_work_limit() {
        let chunks = (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
        let connection = mchprs_network::test_support::connection(false).unwrap();
        let mut player = Player::test_player(connection.player);
        let cb = load_schematic(std::io::Cursor::new(include_bytes!(
            "../../../../../test_data/PM1_SORT.schem"
        )))
        .unwrap();
        assert_eq!((cb.size_x, cb.size_y, cb.size_z), (235, 202, 182));
        assert_eq!(cb.data.entries(), 8_639_540);
        player.pos = PlayerPos {
            x: f64::from(cb.offset_x),
            y: f64::from(cb.offset_y),
            z: f64::from(cb.offset_z),
        };
        player.worldedit_clipboard = Some(cb);
        let paste = &COMMANDS["/paste"];
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_ok());

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
        assert_eq!(
            player.worldedit_clipboard.as_ref().unwrap().data.entries(),
            8_639_540
        );
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_ok());

        player.pos = PlayerPos {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        player.worldedit_clipboard = Some(WorldEditClipboard {
            offset_x: 0,
            offset_y: 0,
            offset_z: 0,
            size_x: PLOT_BLOCK_WIDTH as u32,
            size_y: PLOT_BLOCK_HEIGHT as u32,
            size_z: PLOT_BLOCK_WIDTH as u32,
            data: PalettedBitBuffer::new(
                (PLOT_BLOCK_WIDTH * PLOT_BLOCK_HEIGHT * PLOT_BLOCK_WIDTH) as usize,
                9,
            ),
            block_entities: Default::default(),
        });
        assert!(validate_request(&world, &player, "/paste", paste, &[]).is_ok());
        player.second_position = Some(BlockPos::new(
            PLOT_BLOCK_WIDTH - 1,
            PLOT_BLOCK_HEIGHT - 1,
            PLOT_BLOCK_WIDTH - 1,
        ));
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
        assert!(volume(
            BlockPos::new(i32::MIN, i32::MIN, i32::MIN),
            BlockPos::new(i32::MAX, i32::MAX, i32::MAX)
        )
        .is_err());
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
        assert!(region(
            &world,
            BlockPos::zero(),
            BlockPos::new(PLOT_BLOCK_WIDTH, 1, 1)
        )
        .is_err());
        assert_eq!(
            region(&world, BlockPos::zero(), BlockPos::new(1, 1, 1)).unwrap(),
            8
        );
    }
}
