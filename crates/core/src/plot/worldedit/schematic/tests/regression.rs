use super::*;
use crate::plot::worldedit::{create_clipboard, paste_clipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::{storage::Chunk, World};
use std::io::Cursor;

const ADDER: &[u8] = include_bytes!("../../../../../../../test_data/ADDER_GWIEZDNY_TEST.schem");
fn load_blob(blob: &nbt::Blob) -> Result<WorldEditClipboard> {
    let mut bytes = vec![];
    blob.to_gzip_writer(&mut bytes).unwrap();
    load_schematic(Cursor::new(bytes))
}
fn base(version: i32) -> nbt::Blob {
    nbt::Blob::from_gzip_reader(&mut Cursor::new(encode(version, vec![-128, 1]))).unwrap()
}
fn root(blob: &mut nbt::Blob) -> &mut Compound {
    if blob.content.contains_key("Schematic") {
        let Value::Compound(c) = blob.content.get_mut("Schematic").unwrap() else {
            unreachable!()
        };
        c
    } else {
        &mut blob.content
    }
}
fn blocks(blob: &mut nbt::Blob) -> &mut Compound {
    let root = root(blob);
    if matches!(root.get("Version"), Some(Value::Int(3))) {
        let Value::Compound(c) = root.get_mut("Blocks").unwrap() else {
            unreachable!()
        };
        c
    } else {
        root
    }
}
fn assert_roundtrip(cb: &WorldEditClipboard) {
    let mut bytes = vec![];
    write_schematic(&mut bytes, cb).unwrap();
    let nbt = nbt::Blob::from_gzip_reader(&mut Cursor::new(&bytes)).unwrap();
    assert!(matches!(nbt.get("Version"), Some(Value::Int(2))));
    let again = load_schematic(Cursor::new(bytes)).unwrap();
    assert_eq!(
        (cb.size_x, cb.size_y, cb.size_z),
        (again.size_x, again.size_y, again.size_z)
    );
    assert_eq!(
        (cb.offset_x, cb.offset_y, cb.offset_z),
        (again.offset_x, again.offset_y, again.offset_z)
    );
    for i in 0..cb.data.entries() {
        assert_eq!(
            cb.data.get_entry(i),
            again.data.get_entry(i),
            "state at {i}"
        );
    }
    assert_eq!(cb.block_entities.len(), again.block_entities.len());
    for (pos, entity) in &cb.block_entities {
        assert_eq!(
            entity.to_nbt(false).unwrap().content,
            again.block_entities[pos].to_nbt(false).unwrap().content
        );
    }
}
#[test]
fn actual_v3_adder_preserves_seven_signs_offsets_and_all_states() {
    let cb = load_schematic(Cursor::new(ADDER)).unwrap();
    assert_eq!((cb.size_x, cb.size_y, cb.size_z), (21, 7, 45));
    assert_eq!(cb.data.entries(), 6615);
    assert_eq!((cb.offset_x, cb.offset_y, cb.offset_z), (0, 4, 44));
    assert_eq!(cb.block_entities.len(), 7);
    for (x, y, z, label) in [
        (20, 3, 36, "O2"),
        (20, 3, 40, "O1"),
        (2, 5, 44, "TICK"),
        (3, 6, 36, "B2"),
        (3, 6, 38, "A2"),
        (3, 6, 40, "B1"),
        (3, 6, 42, "A1"),
    ] {
        let BlockEntity::Sign(sign) = &cb.block_entities[&BlockPos::new(x, y, z)] else {
            panic!("not a sign")
        };
        let text: serde_json::Value = serde_json::from_str(&sign.rows[0]).unwrap();
        assert_eq!(text.as_str().or_else(|| text["text"].as_str()), Some(label));
        for row in &sign.rows[1..] {
            let text = serde_json::from_str::<serde_json::Value>(row).unwrap();
            assert_eq!(text.as_str().or_else(|| text["text"].as_str()), Some(""));
        }
    }
    assert_roundtrip(&cb);
}
#[test]
fn all_existing_v2_fixtures_import_and_export() {
    for fixture in [
        include_bytes!("../../../../../../../test_data/UpdateTesterExtendNonInst.schem").as_slice(),
        include_bytes!("../../../../../../../test_data/UpdateTesterExtendInst.schem").as_slice(),
        include_bytes!("../../../../../../../test_data/UpdateTesterNonInst.schem").as_slice(),
        include_bytes!("../../../../../../../test_data/UpdateTesterInst.schem").as_slice(),
        include_bytes!("../../../../../../../test_data/MemCellUnalignedNanoTicks.schem").as_slice(),
    ] {
        let cb = load_schematic(Cursor::new(fixture)).unwrap();
        assert_roundtrip(&cb);
    }
    let cb = load_schematic(Cursor::new(include_bytes!(
        "../../../../../../../test_data/UpdateTesterExtendNonInst.schem"
    )))
    .unwrap();
    assert_eq!((cb.size_x, cb.size_y, cb.size_z), (7, 3, 7));
}

#[test]
fn mixed_sign_rows_paste_and_roundtrip_without_encoding_failure() {
    let cb = load_schematic(Cursor::new(include_bytes!(
        "../../../../../../../test_data/sign_mixed_text_v2.schem"
    )))
    .unwrap();
    assert_eq!((cb.size_x, cb.size_y, cb.size_z), (16, 8, 33));
    assert_eq!(cb.block_entities.len(), 2);
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    let origin = BlockPos::new(100, 60, 100);
    // set_block_entity encodes the same update packet even with no connected players.
    paste_clipboard(&mut world, &cb, origin, false);
    for (pos, entity) in &cb.block_entities {
        let at = origin + *pos - BlockPos::new(cb.offset_x, cb.offset_y, cb.offset_z);
        let Some(BlockEntity::Sign(sign)) = world.get_block_entity(at) else {
            panic!("missing pasted sign at {at:?}");
        };
        let BlockEntity::Sign(expected) = entity else {
            panic!("not a sign")
        };
        assert_eq!(sign.rows, expected.rows);
        assert_eq!(sign.back_rows, expected.back_rows);
    }
    assert_roundtrip(&cb);
}
#[test]
fn paste_capture_restore_and_reapply_keep_blocks_and_sign_positions() {
    let cb = load_schematic(Cursor::new(ADDER)).unwrap();
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    let origin = BlockPos::new(100, 30, 100);
    let start = BlockPos::new(100, 26, 56);
    let end = BlockPos::new(120, 32, 100);
    let before = create_clipboard(&mut world, origin, start, end);
    paste_clipboard(&mut world, &cb, origin, false);
    for y in 0..7 {
        for z in 0..45 {
            for x in 0..21 {
                let i = (x + z * 21 + y * 21 * 45) as usize;
                assert_eq!(
                    world.get_block_raw(start + BlockPos::new(x, y, z)),
                    cb.data.get_entry(i)
                );
            }
        }
    }
    assert!(matches!(
        world.get_block_entity(BlockPos::new(120, 29, 92)),
        Some(BlockEntity::Sign(_))
    ));
    let pasted = create_clipboard(&mut world, origin, start, end);
    paste_clipboard(&mut world, &before, origin, false);
    assert_eq!(world.get_block_raw(BlockPos::new(120, 29, 92)), 0);
    assert!(world.get_block_entity(BlockPos::new(120, 29, 92)).is_none());
    paste_clipboard(&mut world, &pasted, origin, false);
    assert!(matches!(
        world.get_block_entity(BlockPos::new(120, 29, 92)),
        Some(BlockEntity::Sign(_))
    ));
}
#[test]
fn offset_precedence_defaults_and_overflow() {
    let mut v2 = base(2);
    root(&mut v2).insert("Offset".into(), Value::IntArray(vec![1, -2, 3]));
    let cb = load_blob(&v2).unwrap();
    assert_eq!((cb.offset_x, cb.offset_y, cb.offset_z), (-1, 2, -3));
    let legacy = Compound::from([
        ("WEOffsetX".into(), Value::Int(4)),
        ("WEOffsetY".into(), Value::Int(5)),
        ("WEOffsetZ".into(), Value::Int(6)),
    ]);
    root(&mut v2).insert("Metadata".into(), Value::Compound(legacy.clone()));
    assert_eq!(load_blob(&v2).unwrap().offset_x, -4);
    let mut partial = legacy.clone();
    partial.remove("WEOffsetY");
    root(&mut v2).insert("Metadata".into(), Value::Compound(partial));
    assert!(load_blob(&v2).is_err());
    let mut v3 = base(3);
    root(&mut v3).insert("Metadata".into(), Value::Compound(legacy));
    assert_eq!(load_blob(&v3).unwrap().offset_x, 2);
    root(&mut v3).remove("Offset");
    assert_eq!(load_blob(&v3).unwrap().offset_x, 0);
    for vector in [vec![1, 2], vec![i32::MIN, 0, 0]] {
        root(&mut v3).insert("Offset".into(), Value::IntArray(vector));
        assert!(load_blob(&v3).is_err());
    }
}
#[test]
fn malformed_schema_palette_data_and_envelopes_return_errors() {
    for version in [2, 3] {
        for field in ["Version", "DataVersion", "Width", "Height", "Length"] {
            let mut blob = base(version);
            root(&mut blob).remove(field);
            assert!(load_blob(&blob).is_err(), "missing {field}");
            root(&mut blob).insert(field.into(), Value::String("wrong type".into()));
            assert!(load_blob(&blob).is_err());
        }
        for dimensions in [[0, 1, 1], [-1, -1, -1], [-1, 257, 1]] {
            let mut blob = base(version);
            for (key, n) in ["Width", "Height", "Length"].into_iter().zip(dimensions) {
                root(&mut blob).insert(key.into(), Value::Short(n));
            }
            assert!(load_blob(&blob).is_err());
        }
        let mut blob = base(version);
        let Value::Compound(palette) = blocks(&mut blob).get_mut("Palette").unwrap() else {
            unreachable!()
        };
        palette.insert("minecraft:air".into(), Value::Int(128));
        assert!(load_blob(&blob).is_err());
        for name in [
            "mod:block",
            "minecraft:missing",
            "minecraft:piston[facing=invalid]",
            "minecraft:piston[facing=east,facing=west]",
            "minecraft:stone[wat=1]",
            "minecraft:stone junk",
        ] {
            let mut blob = base(version);
            blocks(&mut blob).insert(
                "Palette".into(),
                Value::Compound(Compound::from([(name.into(), Value::Int(128))])),
            );
            assert!(load_blob(&blob).is_err(), "{name}");
        }
        let mut blob = base(version);
        blocks(&mut blob).insert("BlockEntities".into(), Value::Int(1));
        assert!(load_blob(&blob).is_err());
    }
    let mut blob = base(3);
    let Value::Compound(flat) = blob.content.remove("Schematic").unwrap() else {
        unreachable!()
    };
    assert!(load_blob(&nbt::Blob::with_content(flat)).is_err());
    assert!(load_schematic(Cursor::new(b"bad gzip")).is_err());
}
#[test]
fn spatial_order_sparse_varints_and_export_above_127() {
    let mut blob = base(3);
    root(&mut blob).insert("Width".into(), Value::Short(2));
    root(&mut blob).insert("Height".into(), Value::Short(2));
    root(&mut blob).insert("Length".into(), Value::Short(3));
    let names = [
        "minecraft:stone",
        "minecraft:glass",
        "minecraft:piston[facing=east]",
        "minecraft:observer[facing=up]",
    ];
    blocks(&mut blob).insert(
        "Palette".into(),
        Value::Compound(
            names
                .into_iter()
                .zip([0, 127, 128, 16384])
                .map(|(n, i)| (n.into(), Value::Int(i)))
                .collect(),
        ),
    );
    blocks(&mut blob).insert(
        "Data".into(),
        Value::ByteArray([0, 127, -128, 1, -128, -128, 1].repeat(3)),
    );
    let cb = load_blob(&blob).unwrap();
    for y in 0..2 {
        for z in 0..3 {
            for x in 0..2 {
                let i = x + z * 2 + y * 6;
                assert_eq!(
                    cb.data.get_entry(i),
                    parse_block(names[i % 4]).unwrap().get_id()
                );
            }
        }
    }
    let mut data = PalettedBitBuffer::new(300, 9);
    for i in 0..300 {
        data.set_entry(i, i as u32);
    }
    let cb = WorldEditClipboard {
        size_x: 300,
        size_y: 1,
        size_z: 1,
        offset_x: 0,
        offset_y: 0,
        offset_z: 0,
        data,
        block_entities: Default::default(),
    };
    assert_roundtrip(&cb);
}
#[test]
fn signs_preserve_both_sides_and_reject_malformed_messages() {
    let mut sign = Compound::from([
        ("id".into(), Value::String("minecraft:sign".into())),
        ("Text1".into(), Value::String("{\"text\":\"old\"}".into())),
    ]);
    assert!(
        matches!(BlockEntity::from_nbt(&sign).unwrap(),BlockEntity::Sign(s) if s.rows[0].contains("old"))
    );
    sign.insert(
        "front_text".into(),
        Value::Compound(Compound::from([
            (
                "messages".into(),
                Value::List(vec![Value::String("{\"text\":\"front\"}".into()); 4]),
            ),
            ("color".into(), Value::String("red".into())),
            ("has_glowing_text".into(), Value::Byte(1)),
        ])),
    );
    sign.insert(
        "back_text".into(),
        Value::Compound(Compound::from([(
            "messages".into(),
            Value::List(vec![Value::String("{\"text\":\"back\"}".into()); 4]),
        )])),
    );
    sign.insert("is_waxed".into(), Value::Byte(1));
    let parsed = BlockEntity::from_nbt(&sign).unwrap();
    assert!(
        matches!(&parsed,BlockEntity::Sign(s) if s.waxed && s.front_glow && s.front_color=="red" && s.back_rows[0].contains("back"))
    );
    assert_eq!(
        parsed.to_nbt(false).unwrap().content,
        BlockEntity::from_nbt(&parsed.to_nbt(false).unwrap().content)
            .unwrap()
            .to_nbt(false)
            .unwrap()
            .content
    );
    for invalid in [
        Value::List(vec![Value::String("A".into()); 3]),
        Value::List(vec![Value::Int(1); 4]),
        Value::Int(1),
    ] {
        let Value::Compound(front) = sign.get_mut("front_text").unwrap() else {
            unreachable!()
        };
        front.insert("messages".into(), invalid);
        assert!(BlockEntity::from_nbt(&sign).is_err());
    }
}

#[test]
fn v3_entity_envelopes_defaults_authoritative_ids_and_errors() {
    let mut blob = base(3);
    let mut envelope = Compound::from([
        ("Id".into(), Value::String("minecraft:sign".into())),
        ("Pos".into(), Value::IntArray(vec![0, 0, 0])),
    ]);
    envelope.insert(
        "Data".into(),
        Value::Compound(Compound::from([
            ("Id".into(), Value::String("wrong:entity".into())),
            ("Text1".into(), Value::String("{\"text\":\"kept\"}".into())),
        ])),
    );
    blocks(&mut blob).insert(
        "BlockEntities".into(),
        Value::List(vec![Value::Compound(envelope.clone())]),
    );
    assert!(
        matches!(load_blob(&blob).unwrap().block_entities.get(&BlockPos::new(0,0,0)),Some(BlockEntity::Sign(s)) if s.rows[0].contains("kept"))
    );
    for id in [
        "minecraft:sign",
        "minecraft:comparator",
        "minecraft:barrel",
        "minecraft:furnace",
        "minecraft:hopper",
    ] {
        envelope.remove("Data");
        envelope.insert("Id".into(), Value::String(id.into()));
        blocks(&mut blob).insert(
            "BlockEntities".into(),
            Value::List(vec![Value::Compound(envelope.clone())]),
        );
        assert_eq!(load_blob(&blob).unwrap().block_entities.len(), 1);
    }
    let state = Compound::from([
        ("Name".into(), Value::String("minecraft:piston".into())),
        (
            "Properties".into(),
            Value::Compound(Compound::from([
                ("facing".into(), Value::String("east".into())),
                ("extended".into(), Value::String("true".into())),
            ])),
        ),
    ]);
    let piston = Compound::from([
        ("blockState".into(), Value::Compound(state)),
        ("facing".into(), Value::Int(5)),
        ("extending".into(), Value::Byte(1)),
        ("source".into(), Value::Byte(0)),
        ("progress".into(), Value::Float(0.5)),
    ]);
    envelope.insert("Id".into(), Value::String("minecraft:piston".into()));
    envelope.insert("Data".into(), Value::Compound(piston));
    blocks(&mut blob).insert(
        "BlockEntities".into(),
        Value::List(vec![Value::Compound(envelope.clone())]),
    );
    assert_roundtrip(&load_blob(&blob).unwrap());
    for invalid in [Value::Int(1), Value::String("bad".into())] {
        envelope.insert("Data".into(), invalid);
        blocks(&mut blob).insert(
            "BlockEntities".into(),
            Value::List(vec![Value::Compound(envelope.clone())]),
        );
        assert!(load_blob(&blob).is_err());
    }
    envelope.remove("Data");
    envelope.insert("Id".into(), Value::String("minecraft:sign".into()));
    for position in [vec![0, 0], vec![1, 0, 0], vec![0, -1, 0]] {
        envelope.insert("Pos".into(), Value::IntArray(position));
        blocks(&mut blob).insert(
            "BlockEntities".into(),
            Value::List(vec![Value::Compound(envelope.clone())]),
        );
        assert!(load_blob(&blob).is_err());
    }
}
