use crate::items::Item;
use mchprs_utils::{map, nbt_unwrap_val};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// A single item in an inventory
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub id: u32,
    pub slot: i8,
    pub count: i8,
    pub nbt: Option<Vec<u8>>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SignBlockEntity {
    pub front_rows: [String; 4],
    pub back_rows: [String; 4],
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
            ContainerType::Furnace => 14,
            ContainerType::Barrel => 2,
            ContainerType::Hopper => 16,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockEntity {
    Comparator {
        output_strength: u8,
    },
    Container {
        comparator_override: u8,
        inventory: Vec<InventoryEntry>,
        ty: ContainerType,
    },
    Sign(Box<SignBlockEntity>),
}

impl BlockEntity {
    /// The protocol id for the block entity
    pub fn ty(&self) -> i32 {
        match self {
            BlockEntity::Comparator { .. } => 18,
            BlockEntity::Container { ty, .. } => match ty {
                ContainerType::Furnace => 0,
                ContainerType::Barrel => 26,
                ContainerType::Hopper => 17,
            },
            BlockEntity::Sign(_) => 7,
        }
    }

    fn load_container(slots_nbt: &[nbt::Value], ty: ContainerType) -> Option<BlockEntity> {
        use nbt::Value;
        let num_slots = ty.num_slots();
        let mut fullness_sum: f32 = 0.0;
        let mut inventory = Vec::new();
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

    pub fn from_nbt(id: &str, nbt: &HashMap<String, nbt::Value>) -> Option<BlockEntity> {
        use nbt::Value;
        match id.trim_start_matches("minecraft:") {
            "comparator" => Some(BlockEntity::Comparator {
                output_strength: *nbt_unwrap_val!(&nbt["OutputSignal"], Value::Int) as u8,
            }),
            "furnace" => BlockEntity::load_container(
                nbt_unwrap_val!(&nbt["Items"], Value::List),
                ContainerType::Furnace,
            ),
            "barrel" => BlockEntity::load_container(
                nbt_unwrap_val!(&nbt["Items"], Value::List),
                ContainerType::Barrel,
            ),
            "hopper" => BlockEntity::load_container(
                nbt_unwrap_val!(&nbt["Items"], Value::List),
                ContainerType::Hopper,
            ),
            "sign" => {
                let sign = if nbt.contains_key("Text1") {
                    // This is the pre-1.20 encoding
                    SignBlockEntity {
                        front_rows: [
                            // This cloning is really dumb
                            nbt_unwrap_val!(nbt["Text1"].clone(), Value::String),
                            nbt_unwrap_val!(nbt["Text2"].clone(), Value::String),
                            nbt_unwrap_val!(nbt["Text3"].clone(), Value::String),
                            nbt_unwrap_val!(nbt["Text4"].clone(), Value::String),
                        ],
                        back_rows: Default::default(),
                    }
                } else {
                    let get_side = |side| {
                        let messages =
                            nbt_unwrap_val!(&nbt[side], Value::Compound).get("messages")?;
                        let mut messages = nbt_unwrap_val!(messages, Value::List).iter().cloned();
                        Some([
                            nbt_unwrap_val!(messages.next()?, Value::String),
                            nbt_unwrap_val!(messages.next()?, Value::String),
                            nbt_unwrap_val!(messages.next()?, Value::String),
                            nbt_unwrap_val!(messages.next()?, Value::String),
                        ])
                    };
                    SignBlockEntity {
                        front_rows: get_side("front_text")?,
                        back_rows: get_side("back_text")?,
                    }
                };
                Some(BlockEntity::Sign(Box::new(sign)))
            }
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
                let front = sign.front_rows.iter().map(|str| Value::String(str.clone()));
                let back = sign.front_rows.iter().map(|str| Value::String(str.clone()));
                nbt::Blob::with_content(map! {
                    "is_waxed" => Value::Byte(0),
                    "front_text" => Value::Compound(map! {
                        "has_glowing_text" => Value::Byte(0),
                        "color" => Value::String("black".into()),
                        "messages" => Value::List(front.collect())
                    }),
                    "back_text" => Value::Compound(map! {
                        "has_glowing_text" => Value::Byte(0),
                        "color" => Value::String("black".into()),
                        "messages" => Value::List(back.collect())
                    }),
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
        }
    }
}
