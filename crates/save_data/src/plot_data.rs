mod fixer;
#[cfg(test)]
mod tests;

use self::fixer::FixInfo;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::{fmt, io};
use thiserror::Error;

pub const VERSION: u32 = 5;
pub const MC_DATA_VERSION: u32 = 4325;

#[derive(Error, Debug)]
pub enum PlotLoadError {
    #[error("unmappable or invalid legacy data: {0}")]
    InvalidLegacyId(String),
    #[error("plot data deserialization error")]
    Deserialize(#[from] bincode::Error),

    #[error("invalid plot data header")]
    InvalidHeader,

    #[error("plot data version {0} too new to be loaded")]
    TooNew(u32),

    #[error("plot data version {0} failed to be converted")]
    ConversionFailed(u32),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("no migration reader for plot save version {0}; original preserved")]
    ConversionUnavailable(u32),
}

impl From<PlotSaveError> for PlotLoadError {
    fn from(e: PlotSaveError) -> Self {
        match e {
            PlotSaveError::Serialize(err) => err.into(),
            PlotSaveError::Io(err) => err.into(),
        }
    }
}

#[derive(Error, Debug)]
pub enum PlotSaveError {
    #[error("plot data serialization error")]
    Serialize(#[from] bincode::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

static PLOT_MAGIC: &[u8; 8] = b"\x86MCHPRS\x00";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct ChunkSectionData {
    pub data: Vec<i64>,
    pub palette: Vec<i32>,
    pub bits_per_block: i8,
    pub block_count: i32,
    pub entries: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkData<const NUM_SECTIONS: usize> {
    #[serde(with = "BigArray")]
    pub sections: [Option<ChunkSectionData>; NUM_SECTIONS],
    pub block_entities: FxHashMap<BlockPos, BlockEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tps {
    Limited(u32),
    Unlimited,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSendRate(pub u32);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PistonAnimation {
    #[default]
    Auto,
    On,
    Off,
}

impl fmt::Display for PistonAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        })
    }
}

impl Default for WorldSendRate {
    fn default() -> Self {
        Self(60)
    }
}

impl fmt::Display for Tps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tps::Limited(tps) => write!(f, "{}", tps),
            Tps::Unlimited => write!(f, "unlimited"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlotData<const NUM_CHUNK_SECTIONS: usize> {
    pub tps: Tps,
    pub world_send_rate: WorldSendRate,
    pub chunk_data: Vec<ChunkData<NUM_CHUNK_SECTIONS>>,
    pub pending_ticks: Vec<TickEntry>,
    pub piston_state: mchprs_world::PistonState,
    pub piston_animation: PistonAnimation,
}

impl<const NUM_CHUNK_SECTIONS: usize> PlotData<NUM_CHUNK_SECTIONS> {
    pub fn validate(&self) -> Result<(), PlotLoadError> {
        let invalid = |message: &str| PlotLoadError::InvalidLegacyId(message.into());
        let states = mchprs_blocks::generated::TARGET_TO_LEGACY.len() as u32;
        for chunk in &self.chunk_data {
            for section in chunk.sections.iter().flatten() {
                let bits = section.bits_per_block;
                if !((4..=8).contains(&bits) || bits == 15)
                    || section.entries != 4096
                    || !(0..=4096).contains(&section.block_count)
                {
                    return Err(invalid("invalid section geometry or block count"));
                }
                let per = 64 / bits as usize;
                if section.data.len() != 4096usize.div_ceil(per) {
                    return Err(invalid("invalid section data length"));
                }
                if bits < 9
                    && (section.palette.is_empty()
                        || section.palette.len() > 1usize << bits
                        || section
                            .palette
                            .iter()
                            .any(|id| *id < 0 || *id as u32 >= states))
                {
                    return Err(invalid("invalid section palette"));
                }
                for i in 0..4096 {
                    let value = ((section.data[i / per] as u64 >> ((i % per) * bits as usize))
                        & ((1 << bits) - 1)) as usize;
                    if (bits < 9 && value >= section.palette.len())
                        || (bits == 15 && value >= states as usize)
                    {
                        return Err(invalid("invalid packed section entry"));
                    }
                }
            }
            for (pos, entity) in &chunk.block_entities {
                if !(0..NUM_CHUNK_SECTIONS as i32 * 16).contains(&pos.y) {
                    return Err(invalid("invalid block entity height"));
                }
                match entity {
                    BlockEntity::CommandBlock(entity)
                        if entity.command.len() > 131068
                            || entity.command.chars().count() > 32767
                            || entity.success_count < 0 =>
                    {
                        return Err(invalid("invalid command block data"));
                    }
                    BlockEntity::MovingPiston(p) if p.block_state >= states => {
                        return Err(invalid("invalid carried piston block state"))
                    }
                    BlockEntity::Container { inventory, ty, .. } => {
                        let mut slots = std::collections::HashSet::new();
                        for entry in inventory {
                            if entry.slot < 0
                                || entry.slot as u8 >= ty.num_slots()
                                || entry.count <= 0
                                || entry.id as usize >= mchprs_blocks::generated::ITEMS.len()
                                || !slots.insert(entry.slot)
                            {
                                return Err(invalid("invalid container inventory"));
                            }
                            if let Some(nbt) = &entry.nbt {
                                nbt::Blob::from_reader(&mut std::io::Cursor::new(nbt))
                                    .map_err(|_| invalid("invalid container item NBT"))?;
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        let registry_count = mchprs_blocks::generated::BLOCKS.len() as u32;
        for tick in &self.pending_ticks {
            if tick.ticks_left >= 32
                || tick.block_type.is_some_and(|id| id >= registry_count)
                || (tick.ticks_left == 0
                    && tick.tick_priority != mchprs_world::TickPriority::NanoTick)
            {
                return Err(invalid("invalid scheduled block tick"));
            }
        }
        let state = &self.piston_state;
        if state.movement_cursor > state.movement_work.len()
            || (state.phase != mchprs_world::AdvancePhase::MovingEntities
                && (!state.movement_work.is_empty() || state.movement_cursor != 0))
        {
            return Err(invalid("invalid piston movement phase"));
        }
        let mut ids = std::collections::HashSet::new();
        let mut positions = std::collections::HashSet::new();
        for motion in &state.motions {
            if motion.identity == 0
                || motion.identity > state.next_identity
                || !ids.insert(motion.identity)
                || !positions.insert(motion.pos)
                || !motion.progress.is_finite()
                || !motion.previous_progress.is_finite()
                || !(0.0..=1.0).contains(&motion.progress)
                || !(0.0..=motion.progress).contains(&motion.previous_progress)
                || motion.last_tick > state.logical_tick
                || !(0..NUM_CHUNK_SECTIONS as i32 * 16).contains(&motion.pos.y)
            {
                return Err(invalid("invalid piston motion"));
            }
        }
        for (pos, id) in &state.movement_work {
            // A replaced/deleted entity may remain in the snapshot; its identity
            // is discarded when stepping rather than applied to the replacement.
            if *id == 0
                || *id > state.next_identity
                || !(0..NUM_CHUNK_SECTIONS as i32 * 16).contains(&pos.y)
            {
                return Err(invalid("invalid piston movement snapshot"));
            }
        }
        for (i, event) in state.events.iter().enumerate() {
            if !(0..NUM_CHUNK_SECTIONS as i32 * 16).contains(&event.pos.y)
                || state
                    .events
                    .iter()
                    .take(i)
                    .any(|previous| previous == event)
            {
                return Err(invalid("invalid or duplicate piston event"));
            }
        }
        Ok(())
    }
    pub fn load_from_file(
        path: impl AsRef<Path>,
        save_if_plot_fixed: bool,
    ) -> Result<PlotData<NUM_CHUNK_SECTIONS>, PlotLoadError> {
        let mut file = File::open(&path)?;

        let mut magic = [0; 8];
        file.read_exact(&mut magic)?;
        if &magic != PLOT_MAGIC {
            drop(file);
            return fixer::try_fix(path, FixInfo::InvalidHeader, save_if_plot_fixed)?
                .ok_or(PlotLoadError::InvalidHeader);
        }

        let version = file.read_u32::<LittleEndian>()?;
        if version < VERSION {
            drop(file);
            return fixer::try_fix(path, FixInfo::OldVersion { version }, save_if_plot_fixed)?
                .ok_or(PlotLoadError::ConversionFailed(version));
        }
        if version > VERSION {
            return Err(PlotLoadError::TooNew(version));
        }

        let data_version = file.read_u32::<LittleEndian>()?;
        if data_version != MC_DATA_VERSION {
            return Err(PlotLoadError::InvalidLegacyId(format!(
                "unsupported Minecraft data version {data_version}; expected {MC_DATA_VERSION}"
            )));
        }
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let data: Self = bincode::deserialize(&buf)?;
        data.validate()?;
        Ok(data)
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), PlotSaveError> {
        let mut bytes = Vec::new();
        bytes.write_all(PLOT_MAGIC)?;
        bytes.write_u32::<LittleEndian>(VERSION)?;
        bytes.write_u32::<LittleEndian>(MC_DATA_VERSION)?;
        bytes.write_all(&bincode::serialize(self)?)?;
        crate::atomic::write(path.as_ref(), &bytes)?;
        Ok(())
    }
}
