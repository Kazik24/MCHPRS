mod regression;
use super::*;
use nbt::Value;
#[test]
fn full_production_arena_schematic_fits_the_block_limit() {
    let palette = Compound::from([("minecraft:air".into(), Value::Int(0))]);
    let encoded = vec![0; 512 * 256 * 512];
    let mut schema = Schema {
        version: 2,
        data_version: 4325,
        dimensions: [512, 256, 512],
        offset: [0; 3],
        palette: &palette,
        encoded: &encoded,
        entities: &[],
    };
    assert_eq!(decode_schematic(&schema).unwrap().0.data.entries(), 67_108_864);
    schema.dimensions[1] += 1;
    assert!(decode_schematic(&schema).is_err());
}

fn encode(version: i32, encoded: Vec<i8>) -> Vec<u8> {
    let mut root = nbt::Blob::new();
    for key in ["Width", "Height", "Length"] {
        root.insert(key, 1i16).unwrap();
    }
    root.insert("Version", version).unwrap();
    root.insert("DataVersion", 4325i32).unwrap();
    let mut palette = std::collections::HashMap::new();
    palette.insert("minecraft:piston[facing=east,extended=true]".into(), Value::Int(128));
    if version == 2 {
        root.insert("Palette", Value::Compound(palette)).unwrap();
        root.insert("BlockData", Value::ByteArray(encoded)).unwrap();
    } else {
        let mut blocks = std::collections::HashMap::new();
        blocks.insert("Palette".into(), Value::Compound(palette));
        blocks.insert("Data".into(), Value::ByteArray(encoded));
        root.insert("Blocks", Value::Compound(blocks)).unwrap();
        root.insert("Offset", Value::IntArray(vec![-2, 3, -4])).unwrap();
        root = nbt::Blob::with_content(std::collections::HashMap::from([("Schematic".into(), Value::Compound(root.content))]));
    }
    let mut bytes = vec![];
    root.to_gzip_writer(&mut bytes).unwrap();
    bytes
}
#[test]
fn sponge_v2_and_v3_preserve_target_piston_properties() {
    for version in [2, 3] {
        let schematic = load_schematic(std::io::Cursor::new(encode(version, vec![-128, 1]))).unwrap();
        let block = Block::from_id(schematic.data.get_entry(0));
        assert_eq!(block.get_name(), "piston");
        assert_eq!(block.properties().get("facing").unwrap(), "east");
        assert_eq!(block.properties().get("extended").unwrap(), "true");
        if version == 3 {
            assert_eq!((schematic.offset_x, schematic.offset_y, schematic.offset_z), (2, -3, 4));
        }
        for invalid in [vec![-128], vec![0], vec![-128, 1, 0], vec![-1; 5]] {
            assert!(load_schematic(std::io::Cursor::new(encode(version, invalid))).is_err());
        }
    }
}

#[test]
fn nbt_preflight_rejects_hostile_lengths_and_nesting() {
    // Root compound name followed by an int array with a negative element count.
    let negative_array = [10, 0, 0, 11, 0, 1, b'x', 0xff, 0xff, 0xff, 0xff];
    assert!(preflight_nbt(&negative_array).is_err());
    let huge_list = [10, 0, 0, 9, 0, 1, b'x', 1, 0, 0x00, 0x0f, 0x42, 0x41];
    assert!(preflight_nbt(&huge_list).is_err());
    // Root compound containing itself more than the admitted nesting depth.
    let mut nested = vec![10, 0, 0];
    for _ in 0..MAX_NBT_DEPTH + 1 {
        nested.extend([10, 0, 0]);
    }
    nested.extend([0; MAX_NBT_DEPTH + 2]);
    assert!(preflight_nbt(&nested).is_err());
}

#[test]
fn schematic_loader_rejects_malformed_nbt_before_deserialization() {
    fn gzip(nbt: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(nbt).unwrap();
        encoder.finish().unwrap()
    }

    let negative_array = [10, 0, 0, 11, 0, 1, b'x', 0xff, 0xff, 0xff, 0xff];
    let negative_list = [10, 0, 0, 9, 0, 1, b'x', 1, 0xff, 0xff, 0xff, 0xff];
    let list_with_end_items = [10, 0, 0, 9, 0, 1, b'x', 0, 0, 0, 0, 1];
    let truncated_scalar = [10, 0, 0, 3, 0, 0, 0, 0, 0];
    let oversized_array = [10, 0, 0, 7, 0, 1, b'x', 16, 0, 0, 1]; // MAX_NBT_BYTES + 1
    let mut too_deep = vec![10, 0, 0];
    for _ in 0..MAX_NBT_DEPTH + 1 {
        too_deep.extend([10, 0, 0]);
    }
    too_deep.extend([0; MAX_NBT_DEPTH + 2]);
    for bytes in [
        negative_array.as_slice(),
        &negative_list,
        &list_with_end_items,
        &truncated_scalar,
        &oversized_array,
        &too_deep,
    ] {
        assert!(load_schematic_with_warnings(Cursor::new(gzip(bytes))).is_err());
    }
    assert!(read_gzip_nbt(Cursor::new(gzip(&[1, 2, 3])), 2).is_err());
}
