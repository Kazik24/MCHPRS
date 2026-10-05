//! Frozen format-3 payload. Do not add/reorder its fields.
use super::super::{ChunkData, PlotData, PlotLoadError, Tps, WorldSendRate};
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyTick {
    pub ticks_left: u32,
    pub tick_priority: TickPriority,
    pub pos: BlockPos,
}
impl From<LegacyTick> for TickEntry {
    fn from(tick: LegacyTick) -> Self {
        Self {
            pos: tick.pos,
            ticks_left: tick.ticks_left,
            tick_priority: tick.tick_priority,
            block_type: None,
        }
    }
}
#[derive(Serialize, Deserialize)]
pub struct Plot<const N: usize> {
    pub tps: Tps,
    pub world_send_rate: WorldSendRate,
    pub chunk_data: Vec<ChunkData<N>>,
    pub pending_ticks: Vec<LegacyTick>,
}
pub fn decode<const N: usize>(data: &[u8]) -> Result<PlotData<N>, PlotLoadError> {
    let old: Plot<N> = bincode::deserialize(data)?;
    let mut plot = PlotData {
        tps: old.tps,
        world_send_rate: old.world_send_rate,
        chunk_data: old.chunk_data,
        pending_ticks: old.pending_ticks.into_iter().map(Into::into).collect(),
        piston_state: Default::default(),
        piston_animation: Default::default(),
    };
    convert_motion(&mut plot)?;
    Ok(plot)
}

/// Legacy fork motion stored a payload in the moving head, rather than at its
/// destination. Convert that representation without overwriting occupied cells.
/// Missing historical timing restarts that old motion at its recorded progress.
pub fn convert_motion<const N: usize>(plot: &mut PlotData<N>) -> Result<(), PlotLoadError> {
    plot.validate()?;
    let width = (plot.chunk_data.len() as f64).sqrt() as usize;
    if width == 0 || width * width != plot.chunk_data.len() {
        return Ok(());
    }
    let mut old = Vec::new();
    for (i, chunk) in plot.chunk_data.iter().enumerate() {
        for (local, entity) in &chunk.block_entities {
            if let BlockEntity::MovingPiston(entity) = entity {
                let pos = BlockPos::new(
                    (i / width) as i32 * 16 + local.x,
                    local.y,
                    (i % width) as i32 * 16 + local.z,
                );
                if entity.source
                    && matches!(read(plot, width, pos), Some(Block::MovingPiston { .. }))
                {
                    old.push((pos, *entity));
                }
            }
        }
    }
    for (head, entity) in old {
        if matches!(
            Block::from_id(entity.block_state),
            Block::PistonHead { .. } | Block::Piston { .. }
        ) {
            continue;
        }
        let base = head.offset(entity.facing.opposite());
        let Some(Block::Piston { piston }) = read(plot, width, base) else {
            return Err(PlotLoadError::InvalidLegacyId(
                "legacy piston motion has no matching base; cannot safely convert".into(),
            ));
        };
        if BlockFace::from(piston.facing) != entity.facing {
            return Err(PlotLoadError::ConversionFailed(3));
        }
        if entity.extending {
            let destination = head.offset(entity.facing);
            if entity.block_state != 0 {
                if read(plot, width, destination) != Some(Block::Air) {
                    return Err(PlotLoadError::InvalidLegacyId("legacy pushed destination is occupied/outside the plot; original preserved".into()));
                }
                write(
                    plot,
                    width,
                    destination,
                    Block::MovingPiston {
                        moving: piston.into(),
                    },
                )?;
                set_entity(
                    plot,
                    width,
                    destination,
                    Some(MovingPistonEntity {
                        source: false,
                        ..entity
                    }),
                );
            }
            set_entity(
                plot,
                width,
                head,
                Some(MovingPistonEntity {
                    block_state: Block::PistonHead {
                        head: piston.into(),
                    }
                    .get_id(),
                    ..entity
                }),
            );
            write(
                plot,
                width,
                base,
                Block::Piston {
                    piston: piston.extend(true),
                },
            )?;
        } else {
            write(
                plot,
                width,
                base,
                Block::MovingPiston {
                    moving: piston.into(),
                },
            )?;
            set_entity(
                plot,
                width,
                base,
                Some(MovingPistonEntity {
                    source: true,
                    block_state: Block::Piston {
                        piston: piston.extend(false),
                    }
                    .get_id(),
                    ..entity
                }),
            );
            if entity.block_state == 0 {
                write(plot, width, head, Block::Air)?;
                set_entity(plot, width, head, None);
            } else {
                set_entity(
                    plot,
                    width,
                    head,
                    Some(MovingPistonEntity {
                        source: false,
                        ..entity
                    }),
                );
            }
        }
    }
    // Obsolete base locks become immediate rechecks; moving-entity ticks are
    // handled by their own phase, never by dispatching a position-only tick.
    for tick in &mut plot.pending_ticks {
        // Pending positions are absolute; chunk layout is relative. Bind the
        // expected type at runtime. Any legacy piston tick will only recheck,
        // and cannot execute movement completion.
        tick.block_type = None;
    }
    Ok(())
}
fn location<const N: usize>(
    _plot: &PlotData<N>,
    width: usize,
    pos: BlockPos,
) -> Option<(usize, usize, usize)> {
    if pos.x < 0
        || pos.z < 0
        || pos.x >= (width * 16) as i32
        || pos.z >= (width * 16) as i32
        || !(0..N as i32 * 16).contains(&pos.y)
    {
        return None;
    }
    Some((
        (pos.x as usize / 16) * width + pos.z as usize / 16,
        pos.y as usize / 16,
        (pos.x as usize % 16) + (pos.z as usize % 16) * 16 + (pos.y as usize % 16) * 256,
    ))
}
fn read<const N: usize>(plot: &PlotData<N>, width: usize, pos: BlockPos) -> Option<Block> {
    let (chunk, section, entry) = location(plot, width, pos)?;
    let Some(section) = &plot.chunk_data[chunk].sections[section] else {
        return Some(Block::Air);
    };
    let bits = section.bits_per_block as usize;
    if !(4..=15).contains(&bits) {
        return None;
    }
    let per = 64 / bits;
    let packed = ((*section.data.get(entry / per)? as u64 >> (entry % per * bits))
        & ((1 << bits) - 1)) as u32;
    let id = if bits < 9 {
        *section.palette.get(packed as usize)? as u32
    } else {
        packed
    };
    Some(Block::from_id(id))
}
fn write<const N: usize>(
    plot: &mut PlotData<N>,
    width: usize,
    pos: BlockPos,
    block: Block,
) -> Result<(), PlotLoadError> {
    let (chunk, section, entry) =
        location(plot, width, pos).ok_or(PlotLoadError::ConversionFailed(3))?;
    let mut ids = Vec::with_capacity(4096);
    for i in 0..4096 {
        let at = BlockPos::new(
            pos.x.div_euclid(16) * 16 + (i % 16) as i32,
            pos.y.div_euclid(16) * 16 + (i / 256) as i32,
            pos.z.div_euclid(16) * 16 + (i / 16 % 16) as i32,
        );
        ids.push(
            read(plot, width, at)
                .ok_or(PlotLoadError::ConversionFailed(3))?
                .get_id(),
        );
    }
    ids[entry] = block.get_id();
    let mut data = vec![0i64; 1024];
    for (i, id) in ids.iter().enumerate() {
        data[i / 4] |= (*id as i64) << (i % 4 * 15);
    }
    plot.chunk_data[chunk].sections[section] = Some(super::super::ChunkSectionData {
        data,
        palette: vec![],
        bits_per_block: 15,
        block_count: ids.iter().filter(|&&id| id != 0).count() as i32,
        entries: 4096,
    });
    Ok(())
}
fn set_entity<const N: usize>(
    plot: &mut PlotData<N>,
    width: usize,
    pos: BlockPos,
    entity: Option<MovingPistonEntity>,
) {
    let (index, _, _) = location(plot, width, pos).unwrap();
    let local = BlockPos::new(pos.x % 16, pos.y, pos.z % 16);
    if let Some(entity) = entity {
        plot.chunk_data[index]
            .block_entities
            .insert(local, BlockEntity::MovingPiston(entity));
    } else {
        plot.chunk_data[index].block_entities.remove(&local);
    }
}
