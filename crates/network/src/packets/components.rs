//! Protocol-770 item components. Retain the original bytes after validated parsing so
//! components that do not affect MCHPRS gameplay survive inventory/save round trips.
use super::{DecodeResult, PacketDecodeError, PacketDecoderExt, PacketEncoderExt, SlotData};
use serde_json::{Map, Value};
use std::io::{self, Cursor, Read};
use std::sync::OnceLock;

const RAW: &str = "__mchprs_components_770";
const MAX_STACK_SIZE: &str = "__mchprs_max_stack_size";

/// Decorate a generated tool item without changing its inventory component.
pub fn set_tool_display(
    item_id: i32,
    blob: &mut nbt::Blob,
    name: &str,
    lore: &str,
) -> DecodeResult<()> {
    let mut custom_name = Vec::new();
    custom_name.write_text(&serde_json::json!({"text": name, "italic": false}).to_string());
    let mut description = Vec::new();
    description.write_varint(1);
    description.write_text(
        &serde_json::json!({"text": lore, "color": "gray", "italic": false}).to_string(),
    );
    patch_components(
        item_id,
        blob,
        &[(5, custom_name), (8, description), (18, vec![1])],
    )
}

/// Protocol boundary: preserve each validated component's bytes, replacing only
/// explicitly supplied components. Prepare the complete patch before changing NBT.
fn patch_components(
    item_id: i32,
    blob: &mut nbt::Blob,
    replacements: &[(i32, Vec<u8>)],
) -> DecodeResult<()> {
    if !(0..crate::generated::ITEM_COUNT).contains(&item_id) {
        return Err(invalid("invalid item ID"));
    }
    let mut encoded = Vec::new();
    write_slot(
        &mut encoded,
        &Some(SlotData {
            item_id,
            item_count: 1,
            nbt: Some(blob.clone()),
        }),
    );
    let mut cursor = Cursor::new(encoded);
    cursor.read_varint()?;
    cursor.read_varint()?;
    let added = bounded(cursor.read_varint()?)?;
    let removed = bounded(cursor.read_varint()?)?;
    if added + removed > crate::generated::COMPONENT_COUNT as usize {
        return Err(invalid("too many components"));
    }
    let names = &schema()["types"]["SlotComponentType"][1]["mappings"];
    let mut components = std::collections::BTreeMap::new();
    for _ in 0..added {
        let id = cursor.read_varint()?;
        let name = names[id.to_string()]
            .as_str()
            .ok_or_else(|| invalid("unknown component ID"))?;
        let mut recording = Recording {
            reader: &mut cursor,
            bytes: Vec::new(),
        };
        consume(&mut recording, &component_type(name), &Map::new(), 1)?;
        components.insert(id, recording.bytes);
    }
    let mut removals = Vec::new();
    for _ in 0..removed {
        let id = cursor.read_varint()?;
        if !replacements
            .iter()
            .any(|(replacement, _)| *replacement == id)
        {
            removals.push(id);
        }
    }
    for (id, data) in replacements {
        components.insert(*id, data.clone());
    }
    let mut patch = Vec::new();
    patch.write_varint(components.len() as i32);
    patch.write_varint(removals.len() as i32);
    for (id, data) in components {
        patch.write_varint(id);
        patch.write_bytes(&data);
    }
    for id in removals {
        patch.write_varint(id);
    }
    blob.insert(
        RAW,
        nbt::Value::ByteArray(patch.into_iter().map(|byte| byte as i8).collect()),
    )?;
    Ok(())
}

fn component_type(name: &str) -> Value {
    // Corrections cross-checked with MCProtocolLib 290d84c.
    match name {
        "intangible_projectile" => Value::String("void".into()),
        "chicken/variant" => serde_json::json!([
            "container", [
                {"name": "hasHolder", "type": "bool"},
                {"name": "value", "type": ["switch", {
                    "compareTo": "hasHolder",
                    "fields": {"true": "varint", "false": "string"}
                }]}
            ]
        ]),
        _ => schema()["types"]["SlotComponent"][1][1]["type"][1]["fields"][name].clone(),
    }
}

pub fn max_stack_size(nbt: &Option<nbt::Blob>, default: u8) -> u8 {
    match nbt.as_ref().and_then(|blob| blob.get(MAX_STACK_SIZE)) {
        Some(nbt::Value::Int(value)) if (1..=99).contains(value) => *value as u8,
        _ => default,
    }
}
fn invalid(message: &str) -> PacketDecodeError {
    io::Error::new(io::ErrorKind::InvalidData, message).into()
}
fn bounded(n: i32) -> DecodeResult<usize> {
    if !(0..=65536).contains(&n) {
        return Err(invalid("invalid component length"));
    }
    Ok(n as usize)
}
fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../mc_data/1.21.5/protocol.json")).unwrap()
    })
}
struct Recording<'a> {
    reader: &'a mut dyn Read,
    bytes: Vec<u8>,
}
impl Read for Recording<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(b)?;
        self.bytes.extend_from_slice(&b[..n]);
        Ok(n)
    }
}
impl PacketDecoderExt for Recording<'_> {}

fn nbt_payload<T: PacketDecoderExt>(r: &mut T, tag: u8, depth: usize) -> DecodeResult<()> {
    if depth > 64 {
        return Err(invalid("NBT nesting limit"));
    }
    match tag {
        0 => (),
        1..=6 => {
            r.read_bytes([0, 1, 2, 4, 8, 4, 8][tag as usize])?;
        }
        7 | 11 | 12 => {
            let n = bounded(r.read_int()?)?;
            r.read_bytes(
                n * match tag {
                    7 => 1,
                    11 => 4,
                    _ => 8,
                },
            )?;
        }
        8 => {
            let n = r.read_unsigned_short()?;
            r.read_bytes(n as usize)?;
        }
        9 => {
            let t = r.read_unsigned_byte()?;
            let n = bounded(r.read_int()?)?;
            for _ in 0..n {
                nbt_payload(r, t, depth + 1)?;
            }
        }
        10 => {
            let mut entries = 0;
            loop {
                let t = r.read_unsigned_byte()?;
                if t == 0 {
                    break;
                }
                entries += 1;
                if entries > 65536 {
                    return Err(invalid("NBT compound too large"));
                }
                let n = r.read_unsigned_short()?;
                r.read_bytes(n as usize)?;
                nbt_payload(r, t, depth + 1)?;
            }
        }
        _ => return Err(invalid("invalid NBT tag")),
    }
    Ok(())
}
pub(super) fn anonymous_nbt<T: PacketDecoderExt>(r: &mut T) -> DecodeResult<Vec<u8>> {
    let mut rec = Recording {
        reader: r,
        bytes: vec![],
    };
    let tag = rec.read_unsigned_byte()?;
    nbt_payload(&mut rec, tag, 0)?;
    Ok(rec.bytes)
}

// The checked-in schema describes each component's actual payload; there is no
// generic component length prefix. Counts, nesting and unknown types are checked.
fn consume<T: PacketDecoderExt>(
    r: &mut T,
    ty: &Value,
    context: &Map<String, Value>,
    depth: usize,
) -> DecodeResult<Value> {
    if depth > 64 {
        return Err(invalid("component nesting limit"));
    }
    if let Some(name) = ty.as_str() {
        return Ok(match name {
            "void" => Value::Null,
            "varint" => r.read_varint()?.into(),
            "bool" => r.read_bool()?.into(),
            "u8" => r.read_unsigned_byte()?.into(),
            "i8" => r.read_byte()?.into(),
            "i32" => r.read_int()?.into(),
            "f32" => {
                r.read_float()?;
                Value::Null
            }
            "f64" => {
                r.read_double()?;
                Value::Null
            }
            "i64" | "position" => {
                r.read_long()?;
                Value::Null
            }
            "UUID" => {
                r.read_bytes(16)?;
                Value::Null
            }
            "string" => r.read_string()?.into(),
            "anonymousNbt" | "anonOptionalNbt" => {
                anonymous_nbt(r)?;
                Value::Null
            }
            "Slot" => {
                read_slot_depth(r, depth + 1, false)?;
                Value::Null
            }
            other => {
                let t = &schema()["types"][other];
                if t.is_null() || t.as_str() == Some("native") {
                    return Err(invalid("unsupported component type"));
                }
                consume(r, t, context, depth + 1)?
            }
        });
    }
    let kind = ty[0]
        .as_str()
        .ok_or_else(|| invalid("invalid component schema"))?;
    let args = &ty[1];
    match kind {
        "container" => {
            let mut fields = context.clone();
            for f in args
                .as_array()
                .ok_or_else(|| invalid("invalid container"))?
            {
                let value = consume(r, &f["type"], &fields, depth + 1)?;
                if let Some(name) = f["name"].as_str() {
                    fields.insert(name.to_owned(), value);
                }
            }
            Ok(Value::Object(fields))
        }
        "array" => {
            let count = if let Some(n) = args["count"].as_i64() {
                n as i32
            } else if let Some(field) = args["count"].as_str() {
                context[field]
                    .as_i64()
                    .ok_or_else(|| invalid("missing count"))? as i32
            } else {
                r.read_varint()?
            };
            for _ in 0..bounded(count)? {
                consume(r, &args["type"], context, depth + 1)?;
            }
            Ok(Value::Null)
        }
        "option" => {
            if r.read_bool()? {
                consume(r, args, context, depth + 1)
            } else {
                Ok(Value::Null)
            }
        }
        "mapper" => {
            let raw = consume(r, &args["type"], context, depth + 1)?;
            Ok(args["mappings"][raw.to_string()].clone())
        }
        "switch" => {
            let field = args["compareTo"]
                .as_str()
                .ok_or_else(|| invalid("missing switch field"))?;
            let value = context
                .get(field)
                .ok_or_else(|| invalid("missing component discriminator"))?;
            let key = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            let t = args["fields"]
                .get(&key)
                .or_else(|| args.get("default"))
                .ok_or_else(|| invalid("unknown component discriminator"))?;
            consume(r, t, context, depth + 1)
        }
        "registryEntryHolder" => {
            if r.read_varint()? == 0 {
                consume(r, &args["otherwise"]["type"], context, depth + 1)?;
            }
            Ok(Value::Null)
        }
        "registryEntryHolderSet" => {
            let n = r.read_varint()?;
            if n == 0 {
                consume(r, &args["base"]["type"], context, depth + 1)?;
            } else {
                for _ in 0..bounded(n - 1)? {
                    consume(r, &args["otherwise"]["type"], context, depth + 1)?;
                }
            }
            Ok(Value::Null)
        }
        _ => Err(invalid("unsupported component schema")),
    }
}

pub fn read_slot<T: PacketDecoderExt>(r: &mut T) -> DecodeResult<Option<SlotData>> {
    read_slot_depth(r, 0, false)
}
pub fn read_untrusted_slot<T: PacketDecoderExt>(r: &mut T) -> DecodeResult<Option<SlotData>> {
    read_slot_depth(r, 0, true)
}
fn item_names() -> &'static Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        serde_json::from_str::<Value>(include_str!("../../../../mc_data/1.21.5/items.json"))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_owned())
            .collect()
    })
}
fn read_slot_depth<T: PacketDecoderExt>(
    r: &mut T,
    depth: usize,
    untrusted: bool,
) -> DecodeResult<Option<SlotData>> {
    if depth > 64 {
        return Err(invalid("item nesting limit"));
    }
    let count = r.read_varint()?;
    if count == 0 {
        return Ok(None);
    }
    if !(1..=127).contains(&count) {
        return Err(invalid("invalid item count"));
    }
    let id = r.read_varint()?;
    if !(0..crate::generated::ITEM_COUNT).contains(&id) {
        return Err(invalid("invalid item ID"));
    }
    let added = bounded(r.read_varint()?)?;
    let removed = bounded(r.read_varint()?)?;
    if added + removed > crate::generated::COMPONENT_COUNT as usize {
        return Err(invalid("too many components"));
    }
    let names = &schema()["types"]["SlotComponentType"][1]["mappings"];
    let mut raw = Vec::new();
    raw.write_varint(added as i32);
    raw.write_varint(removed as i32);
    let mut nbt = nbt::Blob::new();
    let mut seen = std::collections::HashSet::new();
    let mut entity = std::collections::HashMap::new();
    for _ in 0..added {
        let component_id = r.read_varint()?;
        if !seen.insert(component_id) {
            return Err(invalid("duplicate component"));
        }
        let name = names[component_id.to_string()]
            .as_str()
            .ok_or_else(|| invalid("unknown component ID"))?;
        let component_type = component_type(name);
        let data = if untrusted {
            let n = bounded(r.read_varint()?)?;
            let data = r.read_bytes(n)?;
            let mut cursor = Cursor::new(&data);
            consume(&mut cursor, &component_type, &Map::new(), depth + 1)?;
            if cursor.position() != n as u64 {
                return Err(invalid("component payload length mismatch"));
            }
            data
        } else {
            let mut rec = Recording {
                reader: r,
                bytes: vec![],
            };
            consume(&mut rec, &component_type, &Map::new(), depth + 1)?;
            rec.bytes
        };
        raw.write_varint(component_id);
        raw.write_bytes(&data);
        if name == "max_stack_size" {
            let size = Cursor::new(&data).read_varint()?;
            if !(1..=99).contains(&size) {
                return Err(invalid("invalid maximum stack size"));
            }
            nbt.insert(MAX_STACK_SIZE, nbt::Value::Int(size))?;
        }
        if matches!(
            name,
            "custom_data" | "debug_stick_state" | "block_entity_data"
        ) && data.first() == Some(&10)
        {
            let mut named = vec![10, 0, 0];
            named.extend_from_slice(&data[1..]);
            let blob = nbt::Blob::from_reader(&mut Cursor::new(named))?;
            match name {
                "block_entity_data" => entity.extend(blob.content),
                "debug_stick_state" => {
                    nbt.insert("DebugProperty", nbt::Value::Compound(blob.content))?;
                }
                _ => {
                    nbt.insert("custom_data", nbt::Value::Compound(blob.content))?;
                }
            }
        }
        if name == "container" {
            let mut cursor = Cursor::new(&data);
            let n = bounded(cursor.read_varint()?)?;
            if n > 256 {
                return Err(invalid("container too large"));
            }
            let mut items = Vec::new();
            for slot in 0..n {
                if let Some(item) = read_slot_depth(&mut cursor, depth + 1, false)? {
                    let mut map = std::collections::HashMap::new();
                    map.insert("Slot".to_owned(), nbt::Value::Byte(slot as i8));
                    map.insert("Count".to_owned(), nbt::Value::Byte(item.item_count));
                    map.insert(
                        "id".to_owned(),
                        nbt::Value::String(format!(
                            "minecraft:{}",
                            item_names()[item.item_id as usize]
                        )),
                    );
                    if let Some(tag) = item.nbt {
                        map.insert("tag".to_owned(), nbt::Value::Compound(tag.content));
                    }
                    items.push(nbt::Value::Compound(map));
                }
            }
            entity.insert("Items".into(), nbt::Value::List(items));
        }
        if name == "block_state" {
            let mut cursor = Cursor::new(&data);
            let n = bounded(cursor.read_varint()?)?;
            let mut props = std::collections::HashMap::new();
            for _ in 0..n {
                props.insert(
                    cursor.read_string()?,
                    nbt::Value::String(cursor.read_string()?),
                );
            }
            nbt.insert("BlockStateTag", nbt::Value::Compound(props))?;
        }
    }
    for _ in 0..removed {
        let id = r.read_varint()?;
        if !(0..crate::generated::COMPONENT_COUNT).contains(&id) || !seen.insert(id) {
            return Err(invalid("invalid removed component"));
        }
        raw.write_varint(id);
    }
    if !entity.is_empty() {
        entity.entry("id".into()).or_insert_with(|| {
            nbt::Value::String(format!("minecraft:{}", item_names()[id as usize]))
        });
        nbt.insert("BlockEntityTag", nbt::Value::Compound(entity))?;
    }
    nbt.insert(
        RAW,
        nbt::Value::ByteArray(raw.iter().map(|b| *b as i8).collect()),
    )?;
    Ok(Some(SlotData {
        item_id: id,
        item_count: count as i8,
        nbt: Some(nbt),
    }))
}

pub fn write_slot<T: PacketEncoderExt>(w: &mut T, slot: &Option<SlotData>) {
    let Some(slot) = slot else {
        w.write_varint(0);
        return;
    };
    w.write_varint(slot.item_count as i32);
    w.write_varint(slot.item_id);
    if let Some(blob) = &slot.nbt {
        if let Some(nbt::Value::ByteArray(data)) = blob.get(RAW) {
            w.write_bytes(&data.iter().map(|b| *b as u8).collect::<Vec<_>>());
            return;
        }
        let mut components = Vec::new();
        let mut count = 0;
        if let Some(nbt::Value::Compound(tag)) = blob.get("BlockEntityTag") {
            let mut entity = tag.clone();
            if let Some(value) = entity.remove("Id") {
                entity.entry("id".into()).or_insert(value);
            }
            entity.entry("id".into()).or_insert_with(|| {
                nbt::Value::String(format!("minecraft:{}", item_names()[slot.item_id as usize]))
            });
            if let Some(nbt::Value::List(items)) = entity.remove("Items") {
                let mut inventory = std::collections::BTreeMap::new();
                for value in items {
                    let nbt::Value::Compound(item) = value else {
                        continue;
                    };
                    let index = match item.get("Slot") {
                        Some(nbt::Value::Byte(n)) if *n >= 0 => *n as usize,
                        _ => continue,
                    };
                    let name = match item.get("id") {
                        Some(nbt::Value::String(n)) => n.trim_start_matches("minecraft:"),
                        _ => continue,
                    };
                    let Some(id) = item_names().iter().position(|n| n == name) else {
                        continue;
                    };
                    let count = match item.get("Count").or_else(|| item.get("count")) {
                        Some(nbt::Value::Byte(n)) => *n,
                        Some(nbt::Value::Int(n)) => *n as i8,
                        _ => continue,
                    };
                    let nbt = match item.get("tag") {
                        Some(nbt::Value::Compound(c)) => Some(nbt::Blob::with_content(c.clone())),
                        _ => None,
                    };
                    inventory.insert(
                        index,
                        Some(SlotData {
                            item_id: id as i32,
                            item_count: count,
                            nbt,
                        }),
                    );
                }
                let length = inventory.keys().last().map_or(0, |i| i + 1);
                components.write_varint(66);
                components.write_varint(length as i32);
                for i in 0..length {
                    write_slot(&mut components, &inventory.remove(&i).flatten());
                }
                count += 1;
            }
            components.write_varint(51);
            components.write_nbt_blob(&nbt::Blob::with_content(entity));
            count += 1;
        } else {
            components.write_varint(0);
            components.write_nbt_blob(blob);
            count += 1;
        }
        w.write_varint(count);
        w.write_varint(0);
        w.write_bytes(&components);
    } else {
        w.write_varint(0);
        w.write_varint(0);
    }
}

#[cfg(test)]
mod tool_component_tests {
    use super::*;

    #[test]
    fn tool_display_preserves_existing_components_and_custom_data() {
        let item_id = item_names()
            .iter()
            .position(|name| name == "oak_slab")
            .unwrap() as i32;
        let mut raw = Vec::new();
        raw.write_varint(3);
        raw.write_varint(0);
        raw.write_varint(4); // unbreakable, unit component
        raw.write_varint(1); // maximum stack size
        raw.write_varint(16);
        raw.write_varint(0); // authored custom data
        raw.write_nbt_blob(&nbt::Blob::with_content(std::collections::HashMap::from([
            ("owner".into(), nbt::Value::String("kitten".into())),
        ])));
        let mut slot = vec![8];
        slot.write_varint(item_id);
        slot.write_bytes(&raw);
        let mut decoded = read_slot(&mut Cursor::new(slot)).unwrap().unwrap();
        let blob = decoded.nbt.as_mut().unwrap();
        set_tool_display(item_id, blob, "Top slab", "Test lore").unwrap();
        let mut bytes = Vec::new();
        write_slot(&mut bytes, &Some(decoded));
        let restored = read_slot(&mut Cursor::new(&bytes)).unwrap().unwrap();
        assert_eq!(restored.item_count, 8);
        assert_eq!(max_stack_size(&restored.nbt, 64), 16);
        let blob = restored.nbt.as_ref().unwrap();
        let Some(nbt::Value::Compound(data)) = blob.get("custom_data") else {
            panic!("tool display must retain custom data");
        };
        assert_eq!(
            data.get("owner"),
            Some(&nbt::Value::String("kitten".into()))
        );
        let mut again = Vec::new();
        write_slot(&mut again, &Some(restored));
        assert_eq!(bytes, again);
    }
}
