//! Validate geometry and work before capturing undo data or mutating a plot.
use super::*;
use crate::config::CONFIG;
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH};

fn volume(first: BlockPos, second: BlockPos) -> Result<u64, String> {
    [
        first.x.abs_diff(second.x),
        first.y.abs_diff(second.y),
        first.z.abs_diff(second.z),
    ]
    .into_iter()
    .try_fold(1u64, |size, axis| size.checked_mul(u64::from(axis) + 1))
    .ok_or_else(|| "Selection volume overflows".into())
}

fn region(world: &PlotWorld, first: BlockPos, second: BlockPos) -> Result<u64, String> {
    for pos in [first, second] {
        if !Plot::in_plot_bounds(world.x, world.z, pos.x, pos.z)
            || !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y)
        {
            return Err("Operation would leave the current plot or world height".into());
        }
    }
    let blocks = volume(first, second)?;
    if blocks > CONFIG.worldedit_max_blocks {
        return Err(format!(
            "WorldEdit work limit: {} blocks",
            CONFIG.worldedit_max_blocks
        ));
    }
    Ok(blocks)
}

fn clipboard_geometry(
    cb: &WorldEditClipboard,
    pos: BlockPos,
) -> Result<(BlockPos, BlockPos), String> {
    let overflow = || "Clipboard coordinates overflow".to_owned();
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
            return Err("Empty clipboard dimension".into());
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
        return Err("WorldEdit numeric arguments cannot exceed 4096".into());
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
            return Err("Clipboard offset is outside the supported coordinates".into());
        }
        let (first, second) = clipboard_geometry(cb, BlockPos::zero())?;
        let blocks = volume(first, second)?;
        if blocks > CONFIG.worldedit_max_blocks || blocks != cb.data.entries() as u64 {
            return Err(
                "Clipboard exceeds the WorldEdit work limit or has invalid dimensions".into(),
            );
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
                return Err("Stack exceeds the WorldEdit work limit".into());
            }
            let size = match direction {
                BlockFacing::East | BlockFacing::West => first.x.abs_diff(second.x) + 1,
                BlockFacing::Up | BlockFacing::Down => first.y.abs_diff(second.y) + 1,
                _ => first.z.abs_diff(second.z) + 1,
            };
            count.checked_mul(size).ok_or("Stack distance overflows")?
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
