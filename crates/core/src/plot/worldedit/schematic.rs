//! Read Sponge schematic versions 2 and 3; write version 2.
//! https://github.com/SpongePowered/Schematic-Specification/blob/master/versions/schematic-2.md

use super::WorldEditClipboard;
use crate::server::MC_DATA_VERSION;
use crate::world::storage::PalettedBitBuffer;
use anyhow::{bail, Context, Result};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use once_cell::sync::Lazy;
use regex::Regex;
use rustc_hash::FxHashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

type Compound = std::collections::HashMap<String, nbt::Value>;
const MAX_BLOCKS: u32 = 16_777_216;

pub(super) fn parse_block(input: &str) -> Option<Block> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^(?:minecraft:)?([a-z_0-9]+)(?:\[([a-z_0-9]+=[a-z_0-9]+(?:,[a-z_0-9]+=[a-z_0-9]+)*)\])?$").unwrap()
    });
    let captures = RE.captures(input)?;
    let name = captures.get(1)?.as_str();
    let registry = mchprs_blocks::generated::BLOCKS
        .iter()
        .find(|b| b.0 == name)?;
    let mut properties = std::collections::HashMap::new();
    if let Some(list) = captures.get(2) {
        for pair in list.as_str().split(',') {
            let (key, value) = pair.split_once('=')?;
            if properties.insert(key, value).is_some() {
                return None;
            }
        }
    }
    // Older MCHPRS exports exposed the internal piston `sticky` flag. The block
    // name already carries that property; accept only a consistent legacy flag.
    if matches!(name, "piston" | "sticky_piston") {
        if let Some(sticky) = properties.remove("sticky") {
            if sticky
                != if name == "sticky_piston" {
                    "true"
                } else {
                    "false"
                }
            {
                return None;
            }
        }
    }
    let defaults = mchprs_blocks::generated::STATE_PROPERTIES[registry.4 as usize];
    if properties
        .keys()
        .any(|k| !defaults.iter().any(|p| p.0 == *k))
    {
        return None;
    }
    // Resolve the complete target state, retaining properties the simulation model
    // does not represent (for example waterlogging) as an opaque registry state.
    let id = (registry.2..=registry.3).find(|id| {
        mchprs_blocks::generated::STATE_PROPERTIES[*id as usize]
            .iter()
            .all(|(key, value)| {
                properties
                    .get(key)
                    .copied()
                    .unwrap_or_else(|| defaults.iter().find(|p| p.0 == *key).unwrap().1)
                    == *value
            })
    })?;
    Some(Block::from_id(id))
}
fn integer(root: &Compound, key: &str) -> Result<i32> {
    match root.get(key) {
        Some(nbt::Value::Int(n)) => Ok(*n),
        _ => bail!("{key}: expected Int"),
    }
}
fn compound<'a>(root: &'a Compound, key: &str) -> Result<&'a Compound> {
    match root.get(key) {
        Some(nbt::Value::Compound(c)) => Ok(c),
        _ => bail!("{key}: expected Compound"),
    }
}
fn vector(root: &Compound, key: &str) -> Result<[i32; 3]> {
    match root.get(key) {
        Some(nbt::Value::IntArray(v)) if v.len() == 3 => Ok([v[0], v[1], v[2]]),
        _ => bail!("{key}: expected IntArray of exactly three entries"),
    }
}
struct Schema<'a> {
    version: i32,
    data_version: i32,
    dimensions: [u32; 3],
    offset: [i32; 3],
    palette: &'a Compound,
    encoded: &'a [i8],
    entities: &'a [nbt::Value],
}
impl<'a> Schema<'a> {
    fn read(nbt: &'a nbt::Blob) -> Result<Self> {
        use nbt::Value;
        let wrapped = nbt.get("Schematic").is_some();
        let root = if wrapped {
            compound(&nbt.content, "Schematic")?
        } else {
            &nbt.content
        };
        let version = integer(root, "Version")?;
        if version != 2 && version != 3 {
            bail!("Version: unsupported schematic version {version}");
        }
        if version == 3 && !wrapped {
            bail!("Schematic: v3 requires a nested schema Compound");
        }
        let data_version = integer(root, "DataVersion")?;
        let mut dimensions = [0; 3];
        for (i, key) in ["Width", "Height", "Length"].into_iter().enumerate() {
            dimensions[i] = match root.get(key) {
                Some(Value::Short(n)) => *n as u16 as u32,
                _ => bail!("{key}: expected unsigned NBT Short"),
            };
            if dimensions[i] == 0 {
                bail!("{key}: zero dimension");
            }
        }
        let metadata = match root.get("Metadata") {
            None => None,
            Some(Value::Compound(m)) => Some(m),
            _ => bail!("Metadata: expected Compound"),
        };
        let keys = ["WEOffsetX", "WEOffsetY", "WEOffsetZ"];
        let legacy_present = metadata.is_some_and(|m| keys.iter().any(|k| m.contains_key(*k)));
        let displacement = if version == 2 && legacy_present {
            let m = metadata.unwrap();
            [
                integer(m, keys[0])?,
                integer(m, keys[1])?,
                integer(m, keys[2])?,
            ]
        } else if root.contains_key("Offset") {
            vector(root, "Offset")?
        } else {
            [0; 3]
        };
        let mut offset = [0; 3];
        for i in 0..3 {
            offset[i] = displacement[i]
                .checked_neg()
                .context("Offset: displacement overflows clipboard coordinates")?;
        }
        let container = if version == 3 {
            compound(root, "Blocks")
                .context("Schematic.Blocks: this importer requires block content")?
        } else {
            root
        };
        let palette = compound(container, "Palette")?;
        let field = if version == 3 { "Data" } else { "BlockData" };
        let encoded = match container.get(field) {
            Some(Value::ByteArray(v)) => v.as_slice(),
            _ => bail!("{field}: expected ByteArray"),
        };
        let entities = match container.get("BlockEntities") {
            None => &[][..],
            Some(Value::List(v)) => v.as_slice(),
            _ => bail!("BlockEntities: expected List"),
        };
        for key in ["Biomes", "BiomeData", "Entities"] {
            if let Some(value) = root.get(key) {
                let nonempty = match value {
                    Value::List(v) => !v.is_empty(),
                    Value::Compound(v) => !v.is_empty(),
                    Value::ByteArray(v) => !v.is_empty(),
                    _ => true,
                };
                if nonempty {
                    tracing::warn!("Sponge v{version}: ignoring unsupported {key} content");
                }
            }
        }
        Ok(Self {
            version,
            data_version,
            dimensions,
            offset,
            palette,
            encoded,
            entities,
        })
    }
}
pub fn load_schematic(mut file: impl Read) -> Result<WorldEditClipboard> {
    let nbt = nbt::Blob::from_gzip_reader(&mut file).context("reading gzip schematic NBT")?;
    let schema = Schema::read(&nbt).context("schematic schema")?;
    tracing::debug!(
        "Importing Sponge v{} with source Minecraft DataVersion {}",
        schema.version,
        schema.data_version
    );
    decode_schematic(&schema).with_context(|| {
        format!(
            "Sponge v{} DataVersion {}",
            schema.version, schema.data_version
        )
    })
}
fn decode_schematic(schema: &Schema<'_>) -> Result<WorldEditClipboard> {
    use nbt::Value;
    let [size_x, size_y, size_z] = schema.dimensions;
    let entries = size_x
        .checked_mul(size_y)
        .and_then(|n| n.checked_mul(size_z))
        .filter(|n| *n <= MAX_BLOCKS)
        .context("dimensions: schematic exceeds 16,777,216 block limit")?;
    if schema.encoded.len() < entries as usize {
        bail!("block data: fewer bytes than required block entries");
    }
    let mut palette: FxHashMap<u32, u32> = FxHashMap::default();
    for (name, value) in schema.palette {
        let Value::Int(id) = value else {
            bail!("Palette.{name}: expected Int")
        };
        if *id < 0 || palette.contains_key(&(*id as u32)) {
            bail!("Palette.{name}: negative or duplicate index {id}");
        }
        let state = parse_block(name)
            .with_context(|| format!("Palette: unsupported or invalid block state {name}"))?;
        palette.insert(*id as u32, state.get_id());
    }
    let mut data = PalettedBitBuffer::new(entries as usize, 9);
    let mut at = 0;
    for i in 0..entries as usize {
        let mut id = 0u32;
        let mut complete = false;
        for shift in 0..5 {
            let byte = *schema
                .encoded
                .get(at)
                .with_context(|| format!("block data: truncated VarInt at entry {i}"))?
                as u8;
            at += 1;
            if shift == 4 && byte & 0xf8 != 0 {
                bail!("block data: overflowing palette VarInt at entry {i}");
            }
            id |= ((byte & 127) as u32) << (shift * 7);
            if byte & 128 == 0 {
                complete = true;
                break;
            }
        }
        if !complete {
            bail!("block data: unterminated palette VarInt at entry {i}");
        }
        data.set_entry(
            i,
            *palette
                .get(&id)
                .with_context(|| format!("block data: missing palette index {id} at entry {i}"))?,
        );
    }
    if at != schema.encoded.len() {
        bail!("block data: trailing bytes after {entries} entries");
    }
    let mut parsed_block_entities = FxHashMap::default();
    let mut unsupported = std::collections::BTreeMap::<String, usize>::new();
    for (index, entry) in schema.entities.iter().enumerate() {
        let Value::Compound(envelope) = entry else {
            bail!("BlockEntities[{index}]: expected Compound")
        };
        let coords =
            vector(envelope, "Pos").with_context(|| format!("BlockEntities[{index}].Pos"))?;
        let pos = BlockPos::new(coords[0], coords[1], coords[2]);
        if !(0..size_x as i32).contains(&pos.x)
            || !(0..size_y as i32).contains(&pos.y)
            || !(0..size_z as i32).contains(&pos.z)
        {
            bail!("BlockEntities[{index}].Pos: {pos} outside schematic");
        }
        let id = match envelope.get("Id").or_else(|| envelope.get("id")) {
            Some(Value::String(n)) => n.clone(),
            _ => bail!("BlockEntities[{index}].Id: expected String"),
        };
        let mut entity = if schema.version == 3 {
            match envelope.get("Data") {
                None => Compound::new(),
                Some(Value::Compound(c)) => c.clone(),
                _ => bail!("BlockEntities[{index}].Data: expected Compound"),
            }
        } else {
            envelope.clone()
        };
        entity.remove("Id");
        entity.insert("id".into(), Value::String(id.clone()));
        if schema.data_version < 4325 {
            // Earlier versions stored JSON strings rather than NBT text components.
            for side in ["front_text", "back_text"] {
                if let Some(Value::Compound(text)) = entity.get_mut(side) {
                    if let Some(Value::List(messages)) = text.get_mut("messages") {
                        for message in messages {
                            if let Value::String(json) = message {
                                *message = mchprs_network::text::from_json(json);
                            }
                        }
                    }
                }
            }
            for key in ["CustomName", "LastOutput"] {
                if let Some(Value::String(json)) = entity.get(key) {
                    let value = mchprs_network::text::from_json(json);
                    entity.insert(key.into(), value);
                }
            }
        }
        if matches!(
            id.as_str(),
            "minecraft:comparator"
                | "minecraft:command_block"
                | "minecraft:barrel"
                | "minecraft:furnace"
                | "minecraft:hopper"
                | "minecraft:sign"
                | "minecraft:piston"
                | "minecraft:moving_piston"
        ) {
            let parsed = BlockEntity::from_nbt(&entity)
                .with_context(|| format!("BlockEntities[{index}] {id} at {pos}"))?;
            if parsed_block_entities.insert(pos, parsed).is_some() {
                bail!("BlockEntities[{index}]: duplicate position {pos}");
            }
        } else {
            *unsupported.entry(id).or_default() += 1;
        }
    }
    if !unsupported.is_empty() {
        tracing::warn!(
            "Schematic: skipped unsupported block entities: {:?}",
            unsupported
        );
    }
    let [offset_x, offset_y, offset_z] = schema.offset;
    Ok(WorldEditClipboard {
        size_x,
        size_y,
        size_z,
        offset_x,
        offset_y,
        offset_z,
        data,
        block_entities: parsed_block_entities,
    })
}

pub fn save_schematic(file_name: &str, clipboard: &WorldEditClipboard) -> Result<()> {
    let path = super::schematic_paths::save_path(Path::new("./schems"), file_name)?;
    fs::create_dir_all(path.parent().unwrap())?;

    let file = File::create(path)?;
    write_schematic(file, clipboard)
}
fn write_schematic(mut file: impl std::io::Write, clipboard: &WorldEditClipboard) -> Result<()> {
    let size_x = clipboard.size_x;
    let size_y = clipboard.size_y;
    let size_z = clipboard.size_z;
    let offset_x = clipboard
        .offset_x
        .checked_neg()
        .context("offset X overflow")?;
    let offset_y = clipboard
        .offset_y
        .checked_neg()
        .context("offset Y overflow")?;
    let offset_z = clipboard
        .offset_z
        .checked_neg()
        .context("offset Z overflow")?;
    let blocks = &clipboard.data;
    let volume = size_x
        .checked_mul(size_y)
        .and_then(|n| n.checked_mul(size_z))
        .context("clipboard volume overflow")?;
    if [size_x, size_y, size_z]
        .iter()
        .any(|n| *n == 0 || *n > u16::MAX as u32)
        || volume > MAX_BLOCKS
        || blocks.entries() != volume as usize
    {
        bail!("invalid clipboard geometry");
    }

    let mut data = Vec::new();
    let mut pallette = Vec::new();
    for y_offset in (0..size_y).map(|y| y * size_z * size_x) {
        for z_offset in (0..size_z).map(|z| z * size_x) {
            for x in 0..size_x {
                let entry = blocks.get_entry((y_offset + z_offset + x) as usize);
                let block = Block::from_id(entry);

                let name = format!("minecraft:{}", block.get_name());
                let props = mchprs_blocks::generated::STATE_PROPERTIES
                    .get(entry as usize)
                    .context("invalid clipboard block state")?;
                let full_name = if !props.is_empty() {
                    let props_strs: Vec<String> = props
                        .iter()
                        .map(|(name, val)| format!("{}={}", name, val))
                        .collect();
                    format!("{}[{}]", name, props_strs.join(","))
                } else {
                    name
                };
                let mut idx = if let Some(idx) = pallette.iter().position(|s| *s == full_name) {
                    idx
                } else {
                    let idx = pallette.len();
                    pallette.push(full_name);
                    idx
                };

                loop {
                    let mut temp = (idx & 0b0111_1111) as u8;
                    idx >>= 7;
                    if idx != 0 {
                        temp |= 0b1000_0000;
                    }
                    data.push(temp as i8);
                    if idx == 0 {
                        break;
                    }
                }
            }
        }
    }

    let mut encoded_pallete = nbt::Blob::new();
    for (i, entry) in pallette.iter().enumerate() {
        encoded_pallete.insert(entry, i as i32)?;
    }

    let mut block_entities = Vec::new();
    for (pos, block_entity) in &clipboard.block_entities {
        if let Some(mut blob) = block_entity.to_nbt(false) {
            if let Some(id) = blob.content.remove("id") {
                blob.insert("Id", id)?;
            }
            blob.insert("Pos", nbt::Value::IntArray(vec![pos.x, pos.y, pos.z]))?;
            block_entities.push(nbt::Value::Compound(blob.content));
        }
    }

    // The serde path turns nested NBT arrays into Lists, losing item components.
    // Write the typed NBT tree directly so every nested tag retains its type.
    let mut schematic = nbt::Blob::new();
    schematic.insert("Width", size_x as i16)?;
    schematic.insert("Length", size_z as i16)?;
    schematic.insert("Height", size_y as i16)?;
    schematic.insert("BlockData", nbt::Value::ByteArray(data))?;
    schematic.insert("BlockEntities", nbt::Value::List(block_entities))?;
    schematic.insert("Palette", nbt::Value::Compound(encoded_pallete.content))?;
    schematic.insert("PaletteMax", pallette.len() as i32)?;
    schematic.insert(
        "Metadata",
        nbt::Value::Compound(Compound::from([
            ("WEOffsetX".into(), nbt::Value::Int(offset_x)),
            ("WEOffsetY".into(), nbt::Value::Int(offset_y)),
            ("WEOffsetZ".into(), nbt::Value::Int(offset_z)),
        ])),
    )?;
    schematic.insert("Version", 2i32)?;
    schematic.insert("DataVersion", MC_DATA_VERSION)?;
    schematic.to_gzip_writer(&mut file)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    mod regression;
    use super::*;
    use nbt::Value;
    fn encode(version: i32, encoded: Vec<i8>) -> Vec<u8> {
        let mut root = nbt::Blob::new();
        for key in ["Width", "Height", "Length"] {
            root.insert(key, 1i16).unwrap();
        }
        root.insert("Version", version).unwrap();
        root.insert("DataVersion", 4325i32).unwrap();
        let mut palette = std::collections::HashMap::new();
        palette.insert(
            "minecraft:piston[facing=east,extended=true]".into(),
            Value::Int(128),
        );
        if version == 2 {
            root.insert("Palette", Value::Compound(palette)).unwrap();
            root.insert("BlockData", Value::ByteArray(encoded)).unwrap();
        } else {
            let mut blocks = std::collections::HashMap::new();
            blocks.insert("Palette".into(), Value::Compound(palette));
            blocks.insert("Data".into(), Value::ByteArray(encoded));
            root.insert("Blocks", Value::Compound(blocks)).unwrap();
            root.insert("Offset", Value::IntArray(vec![-2, 3, -4]))
                .unwrap();
            root = nbt::Blob::with_content(std::collections::HashMap::from([(
                "Schematic".into(),
                Value::Compound(root.content),
            )]));
        }
        let mut bytes = vec![];
        root.to_gzip_writer(&mut bytes).unwrap();
        bytes
    }
    #[test]
    fn sponge_v2_and_v3_preserve_target_piston_properties() {
        for version in [2, 3] {
            let schematic =
                load_schematic(std::io::Cursor::new(encode(version, vec![-128, 1]))).unwrap();
            let block = Block::from_id(schematic.data.get_entry(0));
            assert_eq!(block.get_name(), "piston");
            assert_eq!(block.properties().get("facing").unwrap(), "east");
            assert_eq!(block.properties().get("extended").unwrap(), "true");
            if version == 3 {
                assert_eq!(
                    (schematic.offset_x, schematic.offset_y, schematic.offset_z),
                    (2, -3, 4)
                );
            }
            for invalid in [vec![-128], vec![0], vec![-128, 1, 0], vec![-1; 5]] {
                assert!(load_schematic(std::io::Cursor::new(encode(version, invalid))).is_err());
            }
        }
    }
}
