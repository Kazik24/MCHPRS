use super::budget::{Budget, Bytes};
use super::{PlotWorld, PLOT_SECTIONS};
use crate::messages;
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redpiler::TickScheduler;
use crate::world::storage::Chunk;
use bincode::Options;
use mchprs_save_data::plot_data::ChunkData;
use mchprs_world::{PistonState, TickEntry};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};
use std::io::Cursor;
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct Snapshot {
    chunks: Vec<(i32, i32, ChunkData<PLOT_SECTIONS>)>,
    ticks: Vec<TickEntry>,
    piston_state: PistonState,
}

impl Snapshot {
    pub fn restore(self, world: &mut PlotWorld) {
        world.chunks = self
            .chunks
            .into_iter()
            .map(|(x, z, data)| Chunk::load(x, z, data))
            .collect();
        world.to_be_ticked = self.ticks.into_iter().collect();
        world.piston_state = self.piston_state;
        world.invalidate_interpreter_caches();
        world.reset_screen_tracking();
        world.command_messages.clear();
    }
}

struct Chunks<'a>(&'a [Chunk]);

impl Serialize for Chunks<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for chunk in self.0 {
            sequence.serialize_element(&(chunk.x, chunk.z, chunk.history_view()))?;
        }
        sequence.end()
    }
}

struct Ticks<'a>(&'a TickScheduler<ScheduledBlockTick>);

impl Serialize for Ticks<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.iter().count()))?;
        for tick in self.0.iter_entries() {
            sequence.serialize_element(&tick)?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct View<'a> {
    chunks: Chunks<'a>,
    ticks: Ticks<'a>,
    piston_state: &'a PistonState,
}

pub(super) fn capture_raw(world: &mut PlotWorld, work: &Arc<Budget>) -> Result<Bytes, String> {
    for chunk in &mut world.chunks {
        chunk.prepare_history();
    }

    let view = View {
        chunks: Chunks(&world.chunks),
        ticks: Ticks(&world.to_be_ticked),
        piston_state: &world.piston_state,
    };
    let serialized_size = bincode::serialized_size(&view).map_err(|error| error.to_string())?;
    let size = usize::try_from(serialized_size)
        .map_err(|_| messages::HISTORY_SNAPSHOT_TOO_LARGE.to_owned())?;
    let mut raw = Bytes::zeroed(work, size).map_err(messages::history_capture_workspace_failed)?;
    bincode::serialize_into(Cursor::new(&mut raw.data[..]), &view).map_err(|e| e.to_string())?;
    Ok(raw)
}

pub(super) struct Encoded {
    pub bytes: Bytes,
    pub raw_len: usize,
    pub checksum: u32,
    pub compressed: bool,
}

impl Encoded {
    pub fn encode(raw: Bytes, dict: &[u8], work: &Arc<Budget>) -> Result<Self, String> {
        let raw_len = raw.data.len();
        let checksum = crc32fast::hash(&raw.data);
        let bound = raw_len
            .checked_mul(110)
            .map(|n| n / 100)
            .and_then(|n| n.checked_add(20))
            .ok_or_else(|| messages::HISTORY_COMPRESSION_SIZE_OVERFLOW.to_owned())?;
        let mut output =
            Bytes::zeroed(work, bound).map_err(messages::history_compression_workspace_failed)?;
        let size = lz4_flex::block::compress_into_with_dict(&raw.data, &mut output.data, dict)
            .map_err(|e| e.to_string())?;

        if size < raw_len {
            output.data.truncate(size);
            Ok(Self {
                bytes: output,
                raw_len,
                checksum,
                compressed: true,
            })
        } else {
            Ok(Self {
                bytes: raw,
                raw_len,
                checksum,
                compressed: false,
            })
        }
    }
}

pub(super) struct Stored {
    pub bytes: Bytes,
    pub raw_len: usize,
    pub checksum: u32,
    pub compressed: bool,
}

impl Stored {
    pub fn decode(&self, dict: &[u8], work: &Arc<Budget>) -> Result<Snapshot, String> {
        // Keep the workspace reservation alive until deserialization finishes.
        let decompressed;
        let data = if self.compressed {
            decompressed = self.decompress(dict, work)?;
            &decompressed.data
        } else {
            &self.bytes.data
        };

        if data.len() != self.raw_len || crc32fast::hash(data) != self.checksum {
            return Err(messages::HISTORY_CHECKSUM_INVALID.into());
        }

        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .with_limit(data.len() as u64)
            .deserialize(data)
            .map_err(messages::history_snapshot_invalid)
    }

    fn decompress(&self, dict: &[u8], work: &Arc<Budget>) -> Result<Bytes, String> {
        let mut buffer =
            Bytes::zeroed(work, self.raw_len).map_err(messages::history_rewind_workspace_failed)?;
        let count =
            lz4_flex::block::decompress_into_with_dict(&self.bytes.data, &mut buffer.data, dict)
                .map_err(messages::history_compression_invalid)?;
        if count != self.raw_len {
            return Err(messages::HISTORY_DECODE_SIZE_INVALID.into());
        }

        Ok(buffer)
    }
}
