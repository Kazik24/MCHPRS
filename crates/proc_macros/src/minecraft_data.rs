//! Generate static tables from the same pinned inputs used by the registry binary generator.
//! `include_str!` makes Cargo track input changes, including during incremental builds.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::{de::DeserializeOwned, Deserialize};
use std::collections::HashMap;
use std::convert::TryFrom;

const BLOCKS: &str = include_str!("../../../mc_data/1.21.5/blocks.json");
const LEGACY_BLOCKS: &str = include_str!("../../../mc_data/1.21.5/legacy_blocks.json");
const ITEMS: &str = include_str!("../../../mc_data/1.21.5/items.json");
const LEGACY_ITEMS: &str = include_str!("../../../mc_data/1.21.5/legacy_items.json");
const BUILTIN_IDS: &str = include_str!("../../../mc_data/1.21.5/builtin_ids.json");
const REGISTRY_DATA: &str = include_str!("../../../mc_data/1.21.5/registry_data.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    id: u32,
    name: String,
    min_state_id: u32,
    max_state_id: u32,
    default_state: u32,
    states: Vec<Property>,
    filter_light: u8,
}

#[derive(Deserialize)]
struct Property {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    num_values: usize,
    values: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: u32,
    name: String,
    stack_size: u32,
}

#[derive(Deserialize)]
struct RegistryData {
    #[serde(rename = "minecraft:worldgen/biome", deserialize_with = "ordered_keys")]
    biomes: Vec<String>,
}

// JSON object order determines the IDs sent by the Python binary generator.
// Read keys directly instead of changing serde_json's map representation for
// every crate in the workspace by enabling its preserve_order feature.
fn ordered_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    struct Keys;

    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a registry object")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut keys = Vec::new();
            while let Some((key, _)) = map.next_entry::<String, serde::de::IgnoredAny>()? {
                keys.push(key);
            }
            Ok(keys)
        }
    }

    deserializer.deserialize_map(Keys)
}

struct State {
    block_index: u16,
    properties: Vec<(String, String)>,
}

type StateKey<'a> = (&'a str, Vec<(String, String)>);

fn load<T: DeserializeOwned>(name: &str, input: &str) -> Result<T, String> {
    serde_json::from_str(input).map_err(|error| format!("invalid pinned {name}: {error}"))
}

fn states(blocks: &[Block]) -> Result<Vec<State>, String> {
    let mut states = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if block.id as usize != index
            || block.min_state_id as usize != states.len()
            || !(block.min_state_id..=block.max_state_id).contains(&block.default_state)
        {
            return Err(format!("invalid block ID/state range for {}", block.name));
        }
        let block_index = u16::try_from(index).map_err(|error| error.to_string())?;
        // Cartesian product in input order; the last property varies fastest.
        let mut combinations = vec![Vec::new()];
        for property in &block.states {
            let values = property.values.clone().unwrap_or_else(|| {
                if property.kind == "bool" {
                    vec!["true".into(), "false".into()]
                } else {
                    (0..property.num_values).map(|n| n.to_string()).collect()
                }
            });
            if values.is_empty() || values.len() != property.num_values {
                return Err(format!(
                    "invalid property {} of {}",
                    property.name, block.name
                ));
            }
            combinations = combinations
                .into_iter()
                .flat_map(|properties| {
                    values.iter().map(move |value| {
                        let mut properties = properties.clone();
                        properties.push((property.name.clone(), value.clone()));
                        properties
                    })
                })
                .collect();
        }
        if combinations.len() != (block.max_state_id - block.min_state_id + 1) as usize {
            return Err(format!(
                "property count disagrees with state range for {}",
                block.name
            ));
        }
        states.extend(combinations.into_iter().map(|properties| State {
            block_index,
            properties,
        }));
    }
    Ok(states)
}

fn renamed_block(name: &str) -> &str {
    match name {
        "grass" => "short_grass",
        name => name,
    }
}

fn renamed_item(name: &str) -> &str {
    match name {
        "scute" => "turtle_scute",
        name => renamed_block(name),
    }
}

pub fn blocks() -> Result<TokenStream, String> {
    let blocks: Vec<Block> = load("blocks.json", BLOCKS)?;
    let legacy_blocks: Vec<Block> = load("legacy_blocks.json", LEGACY_BLOCKS)?;
    let target = states(&blocks)?;
    let legacy = states(&legacy_blocks)?;
    let index: HashMap<StateKey<'_>, u32> = target
        .iter()
        .enumerate()
        .map(|(id, state)| {
            (
                (
                    blocks[state.block_index as usize].name.as_str(),
                    state.properties.clone(),
                ),
                id as u32,
            )
        })
        .collect();
    let by_name: HashMap<_, _> = blocks.iter().map(|b| (b.name.as_str(), b)).collect();

    let mut legacy_states = Vec::with_capacity(legacy.len());
    let mut reverse_states = vec![u32::MAX; target.len()];
    for (id, state) in legacy.iter().enumerate() {
        let name = renamed_block(&legacy_blocks[state.block_index as usize].name);
        let block = by_name
            .get(name)
            .ok_or_else(|| format!("missing target block {name}"))?;
        // New properties take the target default; legacy properties override it.
        let mut properties = target[block.default_state as usize].properties.clone();
        for (name, value) in &mut properties {
            if let Some((_, legacy_value)) = state.properties.iter().find(|(key, _)| key == name) {
                *value = legacy_value.clone();
            }
        }
        let target_id = *index
            .get(&(name, properties))
            .ok_or_else(|| format!("no target state for legacy block {name}, state {id}"))?;
        legacy_states.push(target_id);
        reverse_states[target_id as usize] = id as u32;
    }

    let block_indices: Vec<_> = target.iter().map(|state| state.block_index).collect();
    let mut slab_types = Vec::with_capacity(target.len());
    let mut dry_ids = Vec::with_capacity(target.len());
    for state in &target {
        let name = blocks[state.block_index as usize].name.as_str();
        let slab_type: u8 = if name.ends_with("_slab") {
            match state
                .properties
                .iter()
                .find(|(key, _)| key == "type")
                .map(|(_, v)| v.as_str())
            {
                Some("top") => 1,
                Some("bottom") => 2,
                Some("double") => 3,
                _ => return Err(format!("invalid slab type for {name}")),
            }
        } else {
            0
        };
        slab_types.push(slab_type);
        let mut properties = state.properties.clone();
        for (key, value) in &mut properties {
            if key == "waterlogged" {
                *value = "false".into();
            }
        }
        let dry_id = *index
            .get(&(name, properties))
            .ok_or_else(|| format!("missing dry state for {name}"))?;
        dry_ids.push(u16::try_from(dry_id).map_err(|error| error.to_string())?);
    }

    let items: Vec<Item> = load("items.json", ITEMS)?;
    let old_items: Vec<Item> = load("legacy_items.json", LEGACY_ITEMS)?;
    if items
        .iter()
        .enumerate()
        .any(|(index, item)| item.id as usize != index)
    {
        return Err("target item IDs must be contiguous and start at zero".into());
    }
    let item_ids: HashMap<_, _> = items.iter().map(|i| (i.name.as_str(), i.id)).collect();
    let air = *item_ids.get("air").ok_or("missing air item")?;
    // Legacy IDs start at 1; slot 0 stays air.
    let legacy_item_count = old_items
        .iter()
        .map(|i| i.id)
        .max()
        .ok_or("no legacy items")?
        + 1;
    let mut legacy_items = vec![air; legacy_item_count as usize];
    let mut reverse_items = vec![u32::MAX; items.len()];
    for item in old_items {
        let target_id = *item_ids
            .get(renamed_item(&item.name))
            .ok_or_else(|| format!("missing target item {}", item.name))?;
        legacy_items[item.id as usize] = target_id;
        reverse_items[target_id as usize] = item.id;
    }

    let block_rows = blocks.iter().map(|b| {
        let Block {
            name,
            id,
            min_state_id,
            max_state_id,
            default_state,
            ..
        } = b;
        quote! { (#name, #id, #min_state_id, #max_state_id, #default_state) }
    });
    let property_rows = target.iter().map(|state| {
        let properties = state
            .properties
            .iter()
            .map(|(key, value)| quote! { (#key, #value) });
        quote! { &[#(#properties),*] }
    });
    let light_filters = blocks.iter().map(|block| block.filter_light);
    let item_rows = items.iter().map(
        |Item {
             name, stack_size, ..
         }| quote! { (#name, #stack_size) },
    );
    let ids: serde_json::Value = load("builtin_ids.json", BUILTIN_IDS)?;
    let entity_constants = entries(&ids, "minecraft:block_entity_type")?
        .iter()
        .map(|(name, entry)| {
            let name = name
                .strip_prefix("minecraft:")
                .ok_or_else(|| format!("invalid block entity name {name}"))?;
            let ident = format_ident!("{}", name.to_ascii_uppercase());
            let id = protocol_id(entry)?;
            Ok(quote! { pub const #ident: i32 = #id; })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        pub static LEGACY_BLOCK_STATES: &[u32] = &[#(#legacy_states),*];
        pub static TARGET_TO_LEGACY: &[u32] = &[#(#reverse_states),*];
        pub static STATE_TO_BLOCK_INDEX: &[u16] = &[#(#block_indices),*];
        pub static STATE_SLAB_TYPES: &[u8] = &[#(#slab_types),*];
        pub static STATE_DRY_IDS: &[u16] = &[#(#dry_ids),*];
        pub static BLOCKS: &[(&str, u32, u32, u32, u32)] = &[#(#block_rows),*];
        pub static BLOCK_LIGHT_FILTERS: &[u8] = &[#(#light_filters),*];
        pub static STATE_PROPERTIES: &[&[(&str, &str)]] = &[#(#property_rows),*];
        pub static LEGACY_ITEMS: &[u32] = &[#(#legacy_items),*];
        pub static TARGET_TO_LEGACY_ITEMS: &[u32] = &[#(#reverse_items),*];
        pub static ITEMS: &[(&str, u32)] = &[#(#item_rows),*];
        pub mod block_entity_types {
            #(#entity_constants)*
        }
    })
}

fn entries<'a>(
    ids: &'a serde_json::Value,
    registry: &str,
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    ids[registry]["entries"]
        .as_object()
        .ok_or_else(|| format!("missing registry {registry}"))
}

fn protocol_id(entry: &serde_json::Value) -> Result<i32, String> {
    let id = entry["protocol_id"].as_i64().ok_or("missing protocol_id")?;
    i32::try_from(id).map_err(|error| error.to_string())
}

pub fn protocol() -> Result<TokenStream, String> {
    let registries: RegistryData = load("registry_data.json", REGISTRY_DATA)?;
    let plains = registries
        .biomes
        .iter()
        .position(|name| name == "plains")
        .ok_or("missing plains biome")?;
    let plains = i32::try_from(plains).map_err(|error| error.to_string())?;
    let items: Vec<Item> = load("items.json", ITEMS)?;
    let item_count = i32::try_from(items.len()).map_err(|error| error.to_string())?;
    let ids: serde_json::Value = load("builtin_ids.json", BUILTIN_IDS)?;
    let component_count = i32::try_from(entries(&ids, "minecraft:data_component_type")?.len())
        .map_err(|error| error.to_string())?;
    let entities = entries(&ids, "minecraft:entity_type")?;
    let player = protocol_id(
        entities
            .get("minecraft:player")
            .ok_or("missing player entity")?,
    )?;
    let ocelot = protocol_id(
        entities
            .get("minecraft:ocelot")
            .ok_or("missing ocelot entity")?,
    )?;
    let wolf = protocol_id(
        entities
            .get("minecraft:wolf")
            .ok_or("missing wolf entity")?,
    )?;
    let fox = protocol_id(entities.get("minecraft:fox").ok_or("missing fox entity")?)?;
    let cat = protocol_id(entities.get("minecraft:cat").ok_or("missing cat entity")?)?;
    let tags = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mc_data/1.21.5/tags.bin");
    let registries = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mc_data/1.21.5/registries.bin"
    );
    Ok(quote! {
        pub const PLAINS_BIOME: i32 = #plains;
        pub const ITEM_COUNT: i32 = #item_count;
        pub const COMPONENT_COUNT: i32 = #component_count;
        pub const PLAYER_ENTITY: i32 = #player;
        pub const OCELOT_ENTITY: i32 = #ocelot;
        pub const WOLF_ENTITY: i32 = #wolf;
        pub const FOX_ENTITY: i32 = #fox;
        pub const CAT_ENTITY: i32 = #cat;
        pub const TAGS: &[u8] = include_bytes!(#tags);
        pub const REGISTRIES: &[u8] = include_bytes!(#registries);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_ids_follow_input_order() {
        let registries: RegistryData = load(
            "registry_data.json",
            r#"{"minecraft:worldgen/biome":{"z":{},"plains":{},"a":{}}}"#,
        )
        .unwrap();
        assert_eq!(registries.biomes, ["z", "plains", "a"]);
    }
}
