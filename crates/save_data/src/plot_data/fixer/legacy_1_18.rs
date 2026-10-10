//! Frozen format-1 bincode layout. Never add/reorder fields or enum variants here.
use super::legacy_1_21_5::LegacyTick;
use crate::plot_data::{ChunkData, ChunkSectionData, PlotData, PlotLoadError, Tps, WorldSendRate};
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::{
    self as current, ContainerType, InventoryEntry, MovingPistonEntity,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

#[derive(Serialize, Deserialize)]
pub struct Sign {
    pub rows: [String; 4],
}
#[derive(Serialize, Deserialize)]
pub enum Entity {
    Comparator {
        output_strength: u8,
    },
    Container {
        comparator_override: u8,
        inventory: Vec<InventoryEntry>,
        ty: ContainerType,
    },
    Sign(Box<Sign>),
    MovingPiston(MovingPistonEntity),
}
#[derive(Serialize, Deserialize)]
pub struct Chunk<const N: usize> {
    #[serde(with = "BigArray")]
    pub sections: [Option<ChunkSectionData>; N],
    pub block_entities: FxHashMap<BlockPos, Entity>,
}
#[derive(Serialize, Deserialize)]
pub struct Plot<const N: usize> {
    pub tps: Tps,
    pub world_send_rate: WorldSendRate,
    pub chunk_data: Vec<Chunk<N>>,
    pub pending_ticks: Vec<LegacyTick>,
}
pub fn state(id: u32) -> Result<u32, PlotLoadError> {
    mchprs_blocks::generated::LEGACY_BLOCK_STATES
        .get(id as usize)
        .copied()
        .ok_or_else(|| PlotLoadError::InvalidLegacyId(format!("block state {id}")))
}
pub fn item(id: u32) -> Result<u32, PlotLoadError> {
    mchprs_blocks::generated::LEGACY_ITEMS
        .get(id as usize)
        .copied()
        .ok_or_else(|| PlotLoadError::InvalidLegacyId(format!("item {id}")))
}
pub fn convert<const N: usize>(old: Plot<N>) -> Result<PlotData<N>, PlotLoadError> {
    let mut chunks = Vec::with_capacity(old.chunk_data.len());
    for chunk in old.chunk_data {
        let mut sections = chunk.sections;
        for section in sections.iter_mut().flatten() {
            if !(4..=15).contains(&section.bits_per_block) || section.entries != 4096 {
                return Err(PlotLoadError::InvalidLegacyId(
                    "invalid chunk section geometry".into(),
                ));
            }
            let bits = section.bits_per_block as usize;
            let per = 64 / bits;
            if section.data.len() != section.entries.div_ceil(per) {
                return Err(PlotLoadError::InvalidLegacyId(
                    "invalid chunk section data length".into(),
                ));
            }
            if bits >= 9 {
                let mut data = vec![0i64; section.entries.div_ceil(4)];
                for i in 0..section.entries {
                    let id = ((section.data[i / per] as u64 >> ((i % per) * bits))
                        & ((1 << bits) - 1)) as u32;
                    data[i / 4] |= (state(id)? as i64) << ((i % 4) * 15);
                }
                section.bits_per_block = 15;
                section.data = data;
            }
            // Remap even an unused direct-mode palette so no legacy IDs remain.
            for id in &mut section.palette {
                *id = state(*id as u32)? as i32;
            }
        }
        let mut entities = FxHashMap::default();
        for (pos, entity) in chunk.block_entities {
            let entity = match entity {
                Entity::Comparator { output_strength } => {
                    current::BlockEntity::Comparator { output_strength }
                }
                Entity::Container {
                    comparator_override,
                    mut inventory,
                    ty,
                } => {
                    for entry in &mut inventory {
                        entry.id = item(entry.id)?;
                    }
                    current::BlockEntity::Container {
                        comparator_override,
                        inventory: inventory.into_iter().collect(),
                        ty,
                    }
                }
                Entity::Sign(sign) => {
                    current::BlockEntity::Sign(Box::new(current::SignBlockEntity {
                        rows: sign.rows,
                        ..Default::default()
                    }))
                }
                Entity::MovingPiston(mut piston) => {
                    piston.block_state = state(piston.block_state)?;
                    current::BlockEntity::MovingPiston(piston)
                }
            };
            entities.insert(pos, entity);
        }
        chunks.push(ChunkData {
            sections,
            block_entities: entities,
        });
    }
    let mut result = PlotData {
        tps: old.tps,
        world_send_rate: old.world_send_rate,
        chunk_data: chunks,
        pending_ticks: old.pending_ticks.into_iter().map(Into::into).collect(),
        piston_state: Default::default(),
        piston_animation: Default::default(),
    };
    super::legacy_1_21_5::convert_motion(&mut result)?;
    Ok(result)
}
pub fn decode<const N: usize>(data: &[u8]) -> Result<PlotData<N>, PlotLoadError> {
    convert(bincode::deserialize::<Plot<N>>(data)?)
}

#[derive(Deserialize)]
struct PlotBeforeSendRate<const N: usize> {
    tps: Tps,
    chunk_data: Vec<Chunk<N>>,
    pending_ticks: Vec<LegacyTick>,
}
pub fn decode_v0<const N: usize>(data: &[u8]) -> Result<PlotData<N>, PlotLoadError> {
    let old: PlotBeforeSendRate<N> = bincode::deserialize(data)?;
    convert(Plot {
        tps: old.tps,
        world_send_rate: WorldSendRate::default(),
        chunk_data: old.chunk_data,
        pending_ticks: old.pending_ticks,
    })
}
