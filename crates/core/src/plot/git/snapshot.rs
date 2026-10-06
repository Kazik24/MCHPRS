use super::super::{NUM_CHUNKS, PLOT_BLOCK_HEIGHT, PLOT_SECTIONS, PLOT_WIDTH};
use crate::messages;
use anyhow::{bail, ensure, Result};
use bincode::Options;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::BlockPos;
use mchprs_save_data::plot_data::{ChunkSectionData, PlotData, MC_DATA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub version: u32,
    pub data_version: u32,
    pub plot: (i32, i32),
    pub data: PlotData<PLOT_SECTIONS>,
}

pub(super) struct Fingerprints {
    pub content: String,
    pub execution: String,
    pub full: String,
    pub sections: Vec<[u8; 32]>,
}

pub(super) fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn state(section: Option<&ChunkSectionData>, index: usize) -> u32 {
    let Some(section) = section else {
        return 0;
    };
    let bits = section.bits_per_block as usize;
    let per = 64 / bits;
    let value = ((section.data[index / per] as u64 >> ((index % per) * bits))
        & ((1u64 << bits) - 1)) as usize;
    if bits < 9 {
        section.palette[value] as u32
    } else {
        value as u32
    }
}

pub(super) fn entity_value(entity: &BlockEntity) -> Result<Value> {
    let mut value = serde_json::to_value(entity)?;
    normalize(&mut value)?;
    Ok(value)
}

pub(super) fn normalize(value: &mut Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(entries)) = map.get_mut("inventory") {
                entries.sort_by_key(|entry| entry.get("slot").and_then(Value::as_i64));
            }
            // Inventory NBT is saved as bytes. Parse it for semantic map equality.
            if let Some(Value::Array(bytes)) = map.get("nbt") {
                let bytes: Vec<u8> = bytes
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(256) as u8)
                    .collect();
                let blob = nbt::Blob::from_reader(&mut std::io::Cursor::new(bytes))?;
                map.insert("nbt".into(), typed_nbt(&nbt::Value::Compound(blob.content)));
            }
            for v in map.values_mut() {
                normalize(v)?;
            }
        }
        Value::Array(array) => {
            for v in array {
                normalize(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn typed_nbt(value: &nbt::Value) -> Value {
    use nbt::Value as N;
    match value {
        N::Byte(v) => serde_json::json!({"byte":v}),
        N::Short(v) => serde_json::json!({"short":v}),
        N::Int(v) => serde_json::json!({"int":v}),
        N::Long(v) => serde_json::json!({"long":v}),
        N::Float(v) => serde_json::json!({"float_bits":v.to_bits()}),
        N::Double(v) => serde_json::json!({"double_bits":v.to_bits()}),
        N::String(v) => serde_json::json!({"string":v}),
        N::ByteArray(v) => serde_json::json!({"byte_array":v}),
        N::IntArray(v) => serde_json::json!({"int_array":v}),
        N::LongArray(v) => serde_json::json!({"long_array":v}),
        N::List(v) => serde_json::json!({"list":v.iter().map(typed_nbt).collect::<Vec<_>>()}),
        N::Compound(v) => {
            serde_json::json!({"compound":v.iter().map(|(k,v)|(k.clone(),typed_nbt(v))).collect::<std::collections::BTreeMap<_,_>>()})
        }
    }
}

impl Snapshot {
    pub fn validate(&self, plot: (i32, i32)) -> Result<()> {
        ensure!(
            self.version == 1 && self.data_version == MC_DATA_VERSION,
            messages::GIT_UNSUPPORTED_SNAPSHOT
        );
        ensure!(
            self.plot == plot && self.data.chunk_data.len() == NUM_CHUNKS,
            messages::GIT_SNAPSHOT_PLOT_MISMATCH
        );
        // The NBT parser allocates from declared lengths. Scan without allocating
        // before ordinary plot validation or canonical JSON conversion.
        for chunk in &self.data.chunk_data {
            for entity in chunk.block_entities.values() {
                super::resources::check_entity(entity)?;
            }
        }
        for motion in &self.data.piston_state.motions {
            if let Some(entity) = &motion.carried_entity {
                super::resources::check_entity(entity)?;
            }
        }
        self.data.validate()?;
        for chunk in &self.data.chunk_data {
            ensure!(
                chunk
                    .block_entities
                    .keys()
                    .all(|p| (0..16).contains(&p.x) && (0..16).contains(&p.z)),
                messages::GIT_INVALID_ENTITY_COORDINATES
            );
        }
        Ok(())
    }

    pub fn fingerprints(&self) -> Result<Fingerprints> {
        self.validate(self.plot)?;
        let mut content = Sha256::new();
        content.update(self.version.to_le_bytes());
        content.update(self.data_version.to_le_bytes());
        content.update(self.plot.0.to_le_bytes());
        content.update(self.plot.1.to_le_bytes());
        let mut sections = Vec::with_capacity(NUM_CHUNKS * PLOT_SECTIONS);
        let empty_digest: [u8; 32] = Sha256::digest([0u8; 4096 * 4]).into();
        for chunk in &self.data.chunk_data {
            for sy in 0..PLOT_SECTIONS {
                if chunk.sections[sy].is_none()
                    && !chunk.block_entities.keys().any(|p| p.y / 16 == sy as i32)
                {
                    content.update(empty_digest);
                    sections.push(empty_digest);
                    continue;
                }
                let mut hash = Sha256::new();
                for index in 0..4096 {
                    hash.update(state(chunk.sections[sy].as_ref(), index).to_le_bytes());
                }
                let mut entities: Vec<_> = chunk
                    .block_entities
                    .iter()
                    .filter(|(p, _)| p.y / 16 == sy as i32)
                    .collect();
                entities.sort_by_key(|(p, _)| (p.x, p.y, p.z));
                for (pos, entity) in entities {
                    hash.update(pos.x.to_le_bytes());
                    hash.update(pos.y.to_le_bytes());
                    hash.update(pos.z.to_le_bytes());
                    let data = serde_json::to_vec(&entity_value(entity)?)?;
                    hash.update((data.len() as u64).to_le_bytes());
                    hash.update(data);
                }
                let digest: [u8; 32] = hash.finalize().into();
                content.update(digest);
                sections.push(digest);
            }
        }
        let content = hex(content.finalize());
        let execution = self.execution_hash()?;
        let full = hex(Sha256::digest(format!("{content}:{execution}")));
        Ok(Fingerprints {
            content,
            execution,
            full,
            sections,
        })
    }

    pub fn encode(&self, limit: usize) -> Result<Vec<u8>> {
        let size = bincode::serialized_size(self)? as usize;
        ensure!(size <= limit, messages::GIT_CONFIGURED_SNAPSHOT_SIZE_LIMIT);
        Ok(lz4_flex::compress_prepend_size(&bincode::serialize(self)?))
    }

    pub fn decode(bytes: &[u8], plot: (i32, i32), limit: usize) -> Result<Self> {
        let size = bytes
            .get(..4)
            .ok_or_else(|| anyhow::anyhow!(messages::GIT_TRUNCATED_SNAPSHOT))?;
        let size = u32::from_le_bytes(size.try_into()?) as usize;
        ensure!(size <= limit, messages::GIT_DECOMPRESSION_LIMIT);
        ensure!(
            bytes.len() <= compressed_bound(size),
            messages::GIT_OVERSIZED_OBJECT
        );
        let data = lz4_flex::decompress_size_prepended(bytes)?;
        ensure!(data.len() == size, messages::GIT_INVALID_COMPRESSED_SIZE);
        // Validate the fixed bincode header's chunk count before serde allocates
        // a Vec of large chunk structs from an untrusted declared length.
        let tps = data
            .get(16..20)
            .ok_or_else(|| anyhow::anyhow!(messages::GIT_TRUNCATED_SNAPSHOT))?;
        let offset = match u32::from_le_bytes(tps.try_into()?) {
            0 => 28, // Limited(u32), then world_send_rate(u32).
            1 => 24, // Unlimited, then world_send_rate(u32).
            _ => bail!(messages::GIT_UNSUPPORTED_SNAPSHOT),
        };
        let chunks = data
            .get(offset..offset + 8)
            .ok_or_else(|| anyhow::anyhow!(messages::GIT_TRUNCATED_SNAPSHOT))?;
        ensure!(
            u64::from_le_bytes(chunks.try_into()?) == NUM_CHUNKS as u64,
            messages::GIT_SNAPSHOT_PLOT_MISMATCH
        );
        let snapshot: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(size as u64)
            .reject_trailing_bytes()
            .deserialize(&data)?;
        snapshot.validate(plot)?;
        Ok(snapshot)
    }

    fn execution_hash(&self) -> Result<String> {
        let state = &self.data.piston_state;
        let mut writer = HashWriter(Sha256::new());
        // Preserve the original canonical JSON bytes and stored object IDs.
        // Only one sequence entry is converted to JSON at a time; object keys
        // follow serde_json::Value's alphabetical order.
        writer.write_all(b"[")?;
        canonical_sequence(&mut writer, &self.data.pending_ticks)?;
        writer.write_all(b",{\"events\":")?;
        canonical_sequence(&mut writer, &state.events)?;
        writer.write_all(b",\"logical_tick\":")?;
        serde_json::to_writer(&mut writer, &state.logical_tick)?;
        writer.write_all(b",\"motions\":")?;
        canonical_sequence(&mut writer, &state.motions)?;
        writer.write_all(b",\"movement_cursor\":")?;
        serde_json::to_writer(&mut writer, &state.movement_cursor)?;
        writer.write_all(b",\"movement_work\":")?;
        canonical_sequence(&mut writer, &state.movement_work)?;
        writer.write_all(b",\"next_identity\":")?;
        serde_json::to_writer(&mut writer, &state.next_identity)?;
        writer.write_all(b",\"phase\":")?;
        serde_json::to_writer(&mut writer, &state.phase)?;
        writer.write_all(b",\"scheduled_advanced\":")?;
        serde_json::to_writer(&mut writer, &state.scheduled_advanced)?;
        writer.write_all(b"}]")?;
        Ok(hex(writer.0.finalize()))
    }

    fn location(&self, pos: BlockPos) -> Option<(usize, BlockPos)> {
        let x = i64::from(pos.x) - i64::from(self.plot.0) * i64::from(PLOT_WIDTH * 16);
        let z = i64::from(pos.z) - i64::from(self.plot.1) * i64::from(PLOT_WIDTH * 16);
        if !(0..i64::from(PLOT_WIDTH * 16)).contains(&x)
            || !(0..i64::from(PLOT_WIDTH * 16)).contains(&z)
            || !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y)
        {
            return None;
        }
        let (x, z) = (x as i32, z as i32);
        Some((
            ((x / 16) * PLOT_WIDTH + z / 16) as usize,
            BlockPos::new(x & 15, pos.y, z & 15),
        ))
    }

    pub fn block(&self, pos: BlockPos) -> u32 {
        let Some((chunk, local)) = self.location(pos) else {
            return 0;
        };
        state(
            self.data.chunk_data[chunk].sections[(local.y / 16) as usize].as_ref(),
            ((local.y & 15) * 256 + local.z * 16 + local.x) as usize,
        )
    }

    pub fn entity(&self, pos: BlockPos) -> Result<Option<Value>> {
        let Some((chunk, local)) = self.location(pos) else {
            bail!(messages::GIT_POSITION_OUTSIDE_PLOT);
        };
        self.data.chunk_data[chunk]
            .block_entities
            .get(&local)
            .map(entity_value)
            .transpose()
    }
}

pub(super) fn compressed_bound(size: usize) -> usize {
    lz4_flex::block::get_maximum_output_size(size).saturating_add(4)
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn canonical_sequence(
    writer: &mut impl Write,
    values: impl IntoIterator<Item = impl Serialize>,
) -> Result<()> {
    writer.write_all(b"[")?;
    for (index, value) in values.into_iter().enumerate() {
        if index != 0 {
            writer.write_all(b",")?;
        }
        let mut value = serde_json::to_value(value)?;
        normalize(&mut value)?;
        serde_json::to_writer(&mut *writer, &value)?;
    }
    writer.write_all(b"]")?;
    Ok(())
}
