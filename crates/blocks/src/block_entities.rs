use crate::blocks::Block;
use crate::items::Item;
use crate::BlockFace;
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContainerType {
    Furnace,
    Barrel,
    Hopper,
}

impl FromStr for ContainerType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "barrel" => ContainerType::Barrel,
            "furnace" => ContainerType::Furnace,
            "hopper" => ContainerType::Hopper,
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
        }
        .to_owned()
    }
}

impl ContainerType {
    pub fn num_slots(self) -> u8 {
        match self {
            ContainerType::Furnace => 3,
            ContainerType::Barrel => 27,
            ContainerType::Hopper => 5,
        }
    }

    pub fn window_type(self) -> u8 {
        // https://wiki.vg/Inventory
        match self {
            ContainerType::Furnace => 13,
            ContainerType::Barrel => 2,
            ContainerType::Hopper => 15,
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
    /// Negative values can be used to increase the time until transformation.
    pub progress: u8, // (0 => 0.0, 255 => 1.0, linear interpolation)
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
    pub const ID: &'static str = "minecraft:moving_piston";
    pub const MAX_PROGRESS: u8 = u8::MAX;
    pub fn get_progress(&self) -> f32 {
        self.progress as f32 / Self::MAX_PROGRESS as f32
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
}

impl BlockEntity {
    /// The protocol id for the block entity
    pub fn ty(&self) -> i32 {
        match self {
            BlockEntity::Comparator { .. } => 17,
            BlockEntity::Container { ty, .. } => match ty {
                ContainerType::Furnace => 0,
                ContainerType::Barrel => 25,
                ContainerType::Hopper => 16,
            },
            BlockEntity::Sign(_) => 7,
            BlockEntity::MovingPiston(_) => 67, //idk if this is correct, found it at https://github.com/PrismarineJS/minecraft-data/tree/master
        }
    }

    fn load_container(slots_nbt: &[nbt::Value], ty: ContainerType) -> Option<BlockEntity> {
        use nbt::Value;
        let num_slots = ty.num_slots();
        let mut fullness_sum: f32 = 0.0;
        let mut inventory = ThinVec::with_capacity(slots_nbt.len());
        for item in slots_nbt {
            let item_compound = nbt_unwrap_val!(item, Value::Compound);
            let count = nbt_unwrap_val!(item_compound["Count"], Value::Byte);
            let slot = nbt_unwrap_val!(item_compound["Slot"], Value::Byte);
            let namespaced_name = nbt_unwrap_val!(
                item_compound
                    .get("Id")
                    .or_else(|| item_compound.get("id"))?,
                Value::String
            );
            let item_type = Item::from_name(namespaced_name.split(':').last()?);

            let mut blob = nbt::Blob::new();
            for (k, v) in item_compound {
                blob.insert(k, v.clone()).unwrap();
            }
            let mut data = Vec::new();
            blob.to_writer(&mut data).unwrap();

            let tag = match item_compound.get("tag") {
                Some(nbt::Value::Compound(map)) => {
                    let mut blob = nbt::Blob::new();
                    for (k, v) in map {
                        blob.insert(k, v.clone()).unwrap();
                    }

                    let mut data = Vec::new();
                    blob.to_writer(&mut data).unwrap();
                    Some(data)
                }
                _ => None,
            };
            inventory.push(InventoryEntry {
                slot,
                count,
                id: item_type.unwrap_or(Item::Redstone {}).get_id(),
                nbt: tag,
            });

            fullness_sum += count as f32 / item_type.map_or(64, Item::max_stack_size) as f32;
        }
        Some(BlockEntity::Container {
            comparator_override: (if fullness_sum > 0.0 { 1.0 } else { 0.0 }
                + (fullness_sum / num_slots as f32) * 14.0)
                .floor() as u8,
            inventory,
            ty,
        })
    }

    pub fn from_nbt(nbt: &HashMap<String, nbt::Value>) -> Option<BlockEntity> {
        use nbt::Value;
        let id = nbt_unwrap_val!(&nbt.get("Id").or_else(|| nbt.get("id"))?, Value::String);
        match id.as_ref() {
            "minecraft:comparator" => Some(BlockEntity::Comparator {
                output_strength: *nbt_unwrap_val!(nbt.get("OutputSignal")?, Value::Int) as u8,
            }),
            "minecraft:furnace" => BlockEntity::load_container(
                nbt_unwrap_val!(nbt.get("Items")?, Value::List),
                ContainerType::Furnace,
            ),
            "minecraft:barrel" => BlockEntity::load_container(
                nbt_unwrap_val!(nbt.get("Items")?, Value::List),
                ContainerType::Barrel,
            ),
            "minecraft:hopper" => BlockEntity::load_container(
                nbt_unwrap_val!(nbt.get("Items")?, Value::List),
                ContainerType::Hopper,
            ),
            "minecraft:sign" => Some({
                BlockEntity::Sign(Box::new(SignBlockEntity {
                    rows: [
                        // This cloning is really dumb
                        nbt_unwrap_val!(nbt.get("Text1")?.clone(), Value::String),
                        nbt_unwrap_val!(nbt.get("Text2")?.clone(), Value::String),
                        nbt_unwrap_val!(nbt.get("Text3")?.clone(), Value::String),
                        nbt_unwrap_val!(nbt.get("Text4")?.clone(), Value::String),
                    ],
                }))
            }),
            MovingPistonEntity::ID => Some({
                let block_state = nbt_unwrap_val!(nbt.get("blockState")?, Value::Compound);
                let block_state = nbt_unwrap_val!(block_state.get("Name")?, Value::String);
                //todo properties of blocks (low priority)
                let block_state = Block::from_name(block_state)?.get_id();
                BlockEntity::MovingPiston(MovingPistonEntity {
                    block_state,
                    extending: *nbt_unwrap_val!(nbt.get("extending")?, Value::Byte) != 0,
                    facing: BlockFace::try_from_id(
                        *nbt_unwrap_val!(nbt.get("facing")?, Value::Int) as u32,
                    )?, //todo add error with info
                    progress: MovingPistonEntity::progress_to_u8(*nbt_unwrap_val!(
                        nbt.get("progress")?,
                        Value::Float
                    )),
                    source: *nbt_unwrap_val!(nbt.get("source")?, Value::Byte) != 0,
                })
            }),
            _ => None,
        }
    }

    pub fn to_nbt(&self, sign_only: bool) -> Option<nbt::Blob> {
        if sign_only && !matches!(self, BlockEntity::Sign(_)) {
            return None;
        }

        use nbt::Value;
        match self {
            BlockEntity::Sign(sign) => Some({
                let [r1, r2, r3, r4] = sign.rows.clone();
                nbt::Blob::with_content(map! {
                    "Text1" => Value::String(r1),
                    "Text2" => Value::String(r2),
                    "Text3" => Value::String(r3),
                    "Text4" => Value::String(r4),
                    "id" => Value::String("minecraft:sign".to_owned())
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
                    let nbt = map! {
                        "Count" => nbt::Value::Byte(entry.count),
                        "id" => nbt::Value::String("minecraft:".to_string() + Item::from_id(entry.id).get_name()),
                        "Slot" => nbt::Value::Byte(entry.slot)
                    };
                    // TODO: item nbt data in containers
                    // if let Some(tag) = &entry.nbt {
                    //     let blob = nbt::Blob::from_reader(&mut Cursor::new(tag)).unwrap();
                    // }
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
                    //todo properties of blocks (low priority)
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
