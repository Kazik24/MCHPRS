use crate::blocks::Block;
use crate::items::Item;
use crate::BlockFace;
use anyhow::{bail, Result};
use mchprs_utils::{map, nbt_unwrap_val};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use thin_vec::ThinVec;

/// A single item in an inventory
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub id: u32,
    pub slot: i8,
    pub count: i8,
    pub nbt: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignBlockEntity {
    pub rows: [String; 4],
    pub back_rows: [String; 4],
    pub waxed: bool,
    pub front_color: String,
    pub back_color: String,
    pub front_glow: bool,
    pub back_glow: bool,
}

impl Default for SignBlockEntity {
    fn default() -> Self {
        Self {
            rows: std::array::from_fn(|_| "{\"text\":\"\"}".into()),
            back_rows: std::array::from_fn(|_| "{\"text\":\"\"}".into()),
            waxed: false,
            front_color: "black".into(),
            back_color: "black".into(),
            front_glow: false,
            back_glow: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContainerType {
    Furnace,
    Barrel,
    Hopper,
    // Append new variants: the previous indices are part of the bincode save format.
    Chest,
}

impl FromStr for ContainerType {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "barrel" => ContainerType::Barrel,
            "furnace" => ContainerType::Furnace,
            "hopper" => ContainerType::Hopper,
            "chest" => ContainerType::Chest,
            _ => return Err(()),
        })
    }
}

impl ToString for ContainerType {
    fn to_string(&self) -> String {
        match self {
            ContainerType::Furnace => "minecraft:furnace",
            ContainerType::Barrel => "minecraft:barrel",
            ContainerType::Hopper => "minecraft:hopper",
            ContainerType::Chest => "minecraft:chest",
        }
        .to_owned()
    }
}

impl ContainerType {
    pub fn from_block(block: Block) -> Option<Self> {
        match block {
            Block::Barrel { .. } => Some(Self::Barrel),
            Block::Hopper { .. } => Some(Self::Hopper),
            Block::Furnace { .. } => Some(Self::Furnace),
            block if block.get_name() == "chest" => Some(Self::Chest),
            _ => None,
        }
    }

    pub fn num_slots(self) -> u8 {
        match self {
            ContainerType::Furnace => 3,
            ContainerType::Barrel => 27,
            ContainerType::Hopper => 5,
            ContainerType::Chest => 27,
        }
    }

    pub fn window_type(self) -> u8 {
        // Minecraft 1.21.5 minecraft:menu registry (official generated report).
        match self {
            ContainerType::Furnace => 14,
            ContainerType::Barrel => 2,
            ContainerType::Hopper => 16,
            ContainerType::Chest => 2,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MovingPistonEntity {
    /// true if the piston is extending instead of withdrawing
    pub extending: bool,
    /// Direction that the piston pushes
    pub facing: BlockFace,
    /// How far the block has been moved. Starts at 0.0, and increments by 0.5 each tick.
    /// If the value is 1.0 or higher at the start of a tick (before incrementing), then the block transforms into the stored blockState.
    /// Serialized/interpolated progress. Exact simulation progress is stored in PistonMotion.
    pub progress: u8, // 0 => 0.0, 127 => 0.5, 255 => 1.0
    /// true if the block represents the piston head itself, false if it represents a block being pushed.
    pub source: bool,
    /// The moving block represented by this block entity.
    pub block_state: u32,
}
impl Default for MovingPistonEntity {
    fn default() -> Self {
        Self {
            extending: false,
            facing: BlockFace::Bottom,
            progress: 0,
            source: false,
            block_state: 0,
        }
    }
}

impl MovingPistonEntity {
    pub const ID: &'static str = "minecraft:piston";
    pub const MAX_PROGRESS: u8 = u8::MAX;
    pub fn get_progress(&self) -> f32 {
        // The legacy byte layout is retained; the simulation also stores exact
        // current/previous progress. Preserve the exact half-step on the wire.
        if self.progress == 127 {
            0.5
        } else {
            self.progress as f32 / Self::MAX_PROGRESS as f32
        }
    }
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = Self::progress_to_u8(progress);
    }
    pub fn progress_to_u8(progress: f32) -> u8 {
        let prog = progress * Self::MAX_PROGRESS as f32;
        prog.clamp(0.0, Self::MAX_PROGRESS as f32) as u8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandBlockEntity {
    pub command: String,
    pub custom_name: String,
    pub success_count: i32,
    pub track_output: bool,
    pub last_output: Option<String>,
    pub powered: bool,
    pub automatic: bool,
    pub condition_met: bool,
    pub last_execution: i64,
    pub update_last_execution: bool,
}

impl Default for CommandBlockEntity {
    fn default() -> Self {
        Self {
            command: String::new(),
            custom_name: "\"@\"".into(),
            success_count: 0,
            track_output: true,
            last_output: None,
            powered: false,
            automatic: false,
            condition_met: false,
            last_execution: -1,
            update_last_execution: true,
        }
    }
}

impl CommandBlockEntity {
    pub fn from_nbt(data: &HashMap<String, nbt::Value>) -> Result<Self> {
        let string = |key: &str, default: &str| -> Result<String> {
            match data.get(key) {
                None => Ok(default.into()),
                Some(nbt::Value::String(s)) => Ok(s.clone()),
                _ => bail!("{key}: expected String"),
            }
        };
        let boolean = |key: &str, default: bool| -> Result<bool> {
            match data.get(key) {
                None => Ok(default),
                Some(nbt::Value::Byte(0)) => Ok(false),
                Some(nbt::Value::Byte(1)) => Ok(true),
                _ => bail!("{key}: expected boolean Byte"),
            }
        };
        let command = string("Command", "")?;
        if command.len() > 131068 || command.chars().count() > 32767 {
            bail!("Command: too long");
        }
        let success_count = match data.get("SuccessCount") {
            None => 0,
            Some(nbt::Value::Int(n)) if *n >= 0 => *n,
            _ => bail!("SuccessCount: expected nonnegative Int"),
        };
        let last_execution = match data.get("LastExecution") {
            None => -1,
            Some(nbt::Value::Long(n)) => *n,
            _ => bail!("LastExecution: expected Long"),
        };
        let text = |key: &str| -> Result<Option<String>> {
            match data.get(key) {
                None => Ok(None),
                Some(
                    value @ (nbt::Value::String(_) | nbt::Value::Compound(_) | nbt::Value::List(_)),
                ) => Ok(Some(mchprs_network::text::to_json(value))),
                _ => bail!("{key}: expected text component"),
            }
        };
        Ok(Self {
            command,
            success_count,
            last_execution,
            custom_name: text("CustomName")?.unwrap_or_else(|| "\"@\"".into()),
            last_output: text("LastOutput")?,
            track_output: boolean("TrackOutput", true)?,
            powered: boolean("powered", false)?,
            automatic: boolean("auto", false)?,
            condition_met: boolean("conditionMet", false)?,
            update_last_execution: boolean("UpdateLastExecution", true)?,
        })
    }

    fn to_nbt(&self) -> nbt::Blob {
        use nbt::Value;
        let mut result = nbt::Blob::with_content(map! {
            "id" => Value::String("minecraft:command_block".into()),
            "Command" => Value::String(self.command.clone()),
            "CustomName" => mchprs_network::text::from_json(&self.custom_name),
            "SuccessCount" => Value::Int(self.success_count),
            "TrackOutput" => Value::Byte(self.track_output as i8),
            "powered" => Value::Byte(self.powered as i8), "auto" => Value::Byte(self.automatic as i8),
            "conditionMet" => Value::Byte(self.condition_met as i8),
            "LastExecution" => Value::Long(self.last_execution),
            "UpdateLastExecution" => Value::Byte(self.update_last_execution as i8)
        });
        if let Some(output) = &self.last_output {
            result
                .insert("LastOutput", mchprs_network::text::from_json(output))
                .unwrap();
        }
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockEntity {
    Comparator {
        output_strength: u8,
    },
    Container {
        comparator_override: u8,
        inventory: ThinVec<InventoryEntry>, //reducing size of overall BlockEntity (to 16 bytes from 32 bytes when using Vec)
        ty: ContainerType,
    },
    Sign(Box<SignBlockEntity>),
    MovingPiston(MovingPistonEntity),
    // Append variants to retain the format-4 enum discriminants during migration.
    CommandBlock(Box<CommandBlockEntity>),
}

impl BlockEntity {
    /// The protocol id for the block entity
    pub fn ty(&self) -> i32 {
        match self {
            BlockEntity::Comparator { .. } => crate::generated::block_entity_types::COMPARATOR,
            BlockEntity::Container { ty, .. } => match ty {
                ContainerType::Furnace => crate::generated::block_entity_types::FURNACE,
                ContainerType::Barrel => crate::generated::block_entity_types::BARREL,
                ContainerType::Hopper => crate::generated::block_entity_types::HOPPER,
                ContainerType::Chest => crate::generated::block_entity_types::CHEST,
            },
            BlockEntity::Sign(_) => crate::generated::block_entity_types::SIGN,
            BlockEntity::MovingPiston(_) => crate::generated::block_entity_types::PISTON,
            BlockEntity::CommandBlock(_) => crate::generated::block_entity_types::COMMAND_BLOCK,
        }
    }

    fn load_container(slots_nbt: &[nbt::Value], ty: ContainerType) -> Result<BlockEntity> {
        use nbt::Value;
        let num_slots = ty.num_slots();
        let mut seen_slots = std::collections::HashSet::new();
        let mut fullness_sum: f32 = 0.0;
        let mut inventory = ThinVec::with_capacity(slots_nbt.len());
        for item in slots_nbt {
            let item_compound = nbt_unwrap_val!(Some(item), Value::Compound);
            let count = match item_compound
                .get("count")
                .or_else(|| item_compound.get("Count"))
            {
                Some(Value::Byte(n)) => *n,
                Some(Value::Int(n)) if (1..=127).contains(n) => *n as i8,
                _ => bail!("invalid container item count"),
            };
            if let Some(value) = item_compound.get("components") {
                if !matches!(value,Value::Compound(c) if c.is_empty()) {
                    bail!("modern persisted inventory components are not supported by this schematic importer");
                }
            }
            let slot = *nbt_unwrap_val!(item_compound.get("Slot"), Value::Byte);
            if slot < 0 || slot as u8 >= num_slots || !seen_slots.insert(slot) || count <= 0 {
                bail!("invalid container slot or count");
            }
            let namespaced_name = nbt_unwrap_val!(
                item_compound.get("Id").or_else(|| item_compound.get("id")),
                Value::String
            );
            let item_type = Item::from_name(
                namespaced_name
                    .split(':')
                    .last()
                    .ok_or(anyhow::anyhow!("Item compound id missing namespace"))?,
            );

            let tag_blob = match item_compound.get("tag") {
                Some(nbt::Value::Compound(map)) => {
                    let mut blob = nbt::Blob::new();
                    for (k, v) in map {
                        blob.insert(k, v.clone()).unwrap();
                    }

                    Some(blob)
                }
                _ => None,
            };
            let max_stack = mchprs_network::packets::components::max_stack_size(
                &tag_blob,
                item_type.map_or(64, Item::max_stack_size) as u8,
            );
            let tag = tag_blob.map(|blob| {
                let mut data = Vec::new();
                blob.to_writer(&mut data).expect("validated inventory tag");
                data
            });
            inventory.push(InventoryEntry {
                slot,
                count,
                id: item_type
                    .ok_or_else(|| anyhow::anyhow!("unknown item {namespaced_name}"))?
                    .get_id(),
                nbt: tag,
            });

            fullness_sum += count as f32 / max_stack as f32;
        }
        Ok(BlockEntity::Container {
            comparator_override: (if fullness_sum > 0.0 { 1.0 } else { 0.0 }
                + (fullness_sum / num_slots as f32) * 14.0)
                .floor()
                .min(15.0) as u8,
            inventory,
            ty,
        })
    }

    pub fn from_nbt(nbt: &HashMap<String, nbt::Value>) -> anyhow::Result<BlockEntity> {
        use nbt::Value;
        let id = nbt_unwrap_val!(nbt.get("Id").or_else(|| nbt.get("id")), Value::String);
        match id.as_ref() {
            "minecraft:command_block" => Ok(BlockEntity::CommandBlock(Box::new(
                CommandBlockEntity::from_nbt(nbt)?,
            ))),
            "minecraft:comparator" => {
                let output_strength = match nbt.get("OutputSignal") {
                    None => 0,
                    Some(Value::Int(n)) if (0..=15).contains(n) => *n as u8,
                    _ => bail!("OutputSignal: expected strength 0..15"),
                };
                Ok(BlockEntity::Comparator { output_strength })
            }
            "minecraft:furnace" | "minecraft:barrel" | "minecraft:hopper" | "minecraft:chest" => {
                let ty = match id.as_str() {
                    "minecraft:furnace" => ContainerType::Furnace,
                    "minecraft:hopper" => ContainerType::Hopper,
                    "minecraft:chest" => ContainerType::Chest,
                    _ => ContainerType::Barrel,
                };
                let items = match nbt.get("Items") {
                    None => &[][..],
                    Some(Value::List(v)) => v.as_slice(),
                    _ => bail!("Items: expected List"),
                };
                BlockEntity::load_container(items, ty)
            }
            "minecraft:sign" => Ok({
                let mut sign = SignBlockEntity::default();
                for (side, rows, color, glow) in [
                    (
                        "front_text",
                        &mut sign.rows,
                        &mut sign.front_color,
                        &mut sign.front_glow,
                    ),
                    (
                        "back_text",
                        &mut sign.back_rows,
                        &mut sign.back_color,
                        &mut sign.back_glow,
                    ),
                ] {
                    match nbt.get(side) {
                        Some(Value::Compound(text)) => {
                            if let Some(messages) = text.get("messages") {
                                let Value::List(messages) = messages else {
                                    bail!("{side}.messages: expected List")
                                };
                                if messages.len() != 4 {
                                    bail!("{side}.messages: expected exactly four text components");
                                }
                                for (dst, src) in rows.iter_mut().zip(messages) {
                                    if !matches!(
                                        src,
                                        Value::String(_) | Value::Compound(_) | Value::List(_)
                                    ) {
                                        bail!("{side}.messages: invalid text component");
                                    }
                                    *dst = mchprs_network::text::to_json(src);
                                }
                            }
                            match text.get("color") {
                                None => (),
                                Some(Value::String(c)) => *color = c.clone(),
                                _ => bail!("{side}.color: expected String"),
                            }
                            match text.get("has_glowing_text") {
                                None => (),
                                Some(Value::Byte(b)) if *b == 0 || *b == 1 => *glow = *b != 0,
                                _ => bail!("{side}.has_glowing_text: expected boolean Byte"),
                            }
                        }
                        None if side == "front_text" => {
                            for (i, row) in rows.iter_mut().enumerate() {
                                match nbt
                                    .get(&format!("Text{}", i + 1))
                                    .or_else(|| nbt.get(&format!("text{}", i + 1)))
                                {
                                    None => (),
                                    Some(Value::String(s)) => *row = s.clone(),
                                    _ => bail!("Text{}: expected String", i + 1),
                                }
                            }
                        }
                        None => (),
                        _ => bail!("{side}: expected Compound"),
                    }
                }
                sign.waxed = match nbt.get("is_waxed") {
                    None => false,
                    Some(Value::Byte(b)) if *b == 0 || *b == 1 => *b != 0,
                    _ => bail!("is_waxed: expected boolean Byte"),
                };
                BlockEntity::Sign(Box::new(sign))
            }),
            MovingPistonEntity::ID | "minecraft:moving_piston" => Ok({
                let state = nbt_unwrap_val!(
                    nbt.get("BlockState").or_else(|| nbt.get("blockState")),
                    Value::Compound
                );
                let name = nbt_unwrap_val!(
                    state.get("Name").or_else(|| state.get("name")),
                    Value::String
                );
                let mut block = Block::from_name(name.trim_start_matches("minecraft:"))
                    .ok_or_else(|| anyhow::anyhow!("unknown carried block {name}"))?;
                if let Some(Value::Compound(props)) = state.get("Properties") {
                    block.set_properties(
                        props
                            .iter()
                            .filter_map(|(k, v)| {
                                if let Value::String(s) = v {
                                    Some((k.as_str(), s.as_str()))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    );
                }
                let block_state = block.get_id();

                let facing =
                    *nbt_unwrap_val!(nbt.get("Facing").or_else(|| nbt.get("facing")), Value::Int)
                        as u32;
                let facing = BlockFace::try_from_id(facing).ok_or(anyhow::anyhow!(
                    "Unknown block face in moving piston block entity: {facing}"
                ))?;

                let extending = *nbt_unwrap_val!(
                    nbt.get("Extending").or_else(|| nbt.get("extending")),
                    Value::Byte
                ) != 0;

                let progress = *nbt_unwrap_val!(
                    nbt.get("Progress").or_else(|| nbt.get("progress")),
                    Value::Float
                );
                if !progress.is_finite() || !(0.0..=1.0).contains(&progress) {
                    bail!("Invalid moving piston progress: {progress}");
                }
                let progress = MovingPistonEntity::progress_to_u8(progress);
                let source =
                    *nbt_unwrap_val!(nbt.get("Source").or_else(|| nbt.get("source")), Value::Byte)
                        != 0;

                BlockEntity::MovingPiston(MovingPistonEntity {
                    block_state,
                    extending,
                    facing,
                    progress,
                    source,
                })
            }),
            _ => bail!("Unknown block entity id: {}", id),
        }
    }

    pub fn to_nbt(&self, sign_only: bool) -> Option<nbt::Blob> {
        if sign_only && !matches!(self, BlockEntity::Sign(_)) {
            return None;
        }

        use nbt::Value;
        match self {
            BlockEntity::CommandBlock(entity) => Some(entity.to_nbt()),
            BlockEntity::Sign(sign) => Some({
                let text = |rows: &[String; 4], color: &str, glow: bool| {
                    Value::Compound(map! {
                        "messages"=>mchprs_network::text::list(rows.iter().map(|s|mchprs_network::text::from_json(s)).collect()),
                        "color"=>Value::String(color.into()),"has_glowing_text"=>Value::Byte(glow as i8)
                    })
                };
                nbt::Blob::with_content(map! {
                    "front_text"=>text(&sign.rows,&sign.front_color,sign.front_glow),
                    "back_text"=>text(&sign.back_rows,&sign.back_color,sign.back_glow),
                    "is_waxed"=>Value::Byte(sign.waxed as i8),"id"=>Value::String("minecraft:sign".into())
                })
            }),
            BlockEntity::Comparator { output_strength } => Some({
                nbt::Blob::with_content(map! {
                    "OutputSignal" => Value::Int(*output_strength as i32),
                    "id" => Value::String("minecraft:comparator".to_owned())
                })
            }),
            BlockEntity::Container { inventory, ty, .. } => Some({
                let mut items = Vec::new();
                for entry in inventory {
                    let mut nbt = map! {
                        "count" => nbt::Value::Int(entry.count as i32),
                        "id" => nbt::Value::String(format!("minecraft:{}",Item::from_id(entry.id).get_name())),
                        "Slot" => nbt::Value::Byte(entry.slot)
                    };
                    // Preserve tags/components used by inventory entries across schematic round trips.
                    if let Some(tag) = &entry.nbt {
                        match nbt::Blob::from_reader(&mut std::io::Cursor::new(tag)) {
                            Ok(blob) => {
                                nbt.insert("tag".into(), Value::Compound(blob.content));
                            }
                            Err(_) => return None,
                        }
                    }
                    items.push(nbt::Value::Compound(nbt));
                }
                nbt::Blob::with_content(map! {
                    "id" => Value::String(ty.to_string()),
                    "Items" => Value::List(items)
                })
            }),
            BlockEntity::MovingPiston(mp) => Some({
                let block_state = map! {
                    "Name" => Value::String(format!("minecraft:{}",Block::from_id(mp.block_state).get_name())),
                    "Properties" => Value::Compound(crate::generated::STATE_PROPERTIES[mp.block_state as usize].iter().map(|(k,v)|((*k).to_owned(),Value::String((*v).to_owned()))).collect()),
                };
                nbt::Blob::with_content(map! {
                    "id" => Value::String(MovingPistonEntity::ID.into()),
                    "blockState" => Value::Compound(block_state),
                    "extending" => Value::Byte(mp.extending as i8),
                    "facing" => Value::Int(mp.facing.get_id() as i32),
                    "progress" => Value::Float(mp.get_progress()),
                    "source" => Value::Byte(mp.source as i8),
                })
            }),
        }
    }
}
