use super::clientbound::*;
use super::*;
use std::io::Cursor;

#[test]
fn integer_boundaries_and_malformed_lengths() {
    for n in [0, 1, 127, 128, 16384, i32::MAX, i32::MIN, -1] {
        let mut data = vec![];
        data.write_varint(n);
        assert_eq!(Cursor::new(data).read_varint().unwrap(), n);
    }
    for n in [0, 127, 128, i64::MAX, i64::MIN, -1] {
        let mut data = vec![];
        data.write_varlong(n);
        assert_eq!(Cursor::new(data).read_varlong().unwrap(), n);
    }
    assert!(Cursor::new(vec![0x80; 6]).read_varint().is_err());
    assert!(Cursor::new(vec![0xff; 5]).read_string().is_err());
    assert!(Cursor::new(vec![0x80]).read_varint().is_err());
}
#[test]
fn login_waits_for_acknowledgements() {
    let mut state = NetworkState::Login;
    let mut packet = vec![0];
    packet.write_string(16, "TestPlayer");
    packet.write_uuid(1);
    read_decompressed(&mut Cursor::new(packet), &mut state).unwrap();
    assert!(state == NetworkState::LoginAcknowledgement);
    assert!(read_decompressed(&mut Cursor::new(vec![0x32]), &mut state).is_err());
    read_decompressed(&mut Cursor::new(vec![3]), &mut state).unwrap();
    assert!(state == NetworkState::Configuration);
    read_decompressed(&mut Cursor::new(vec![7, 0]), &mut state).unwrap();
    assert!(state == NetworkState::ConfigurationFinish);
    read_decompressed(&mut Cursor::new(vec![3]), &mut state).unwrap();
    assert!(state == NetworkState::Play);
}
#[test]
fn structured_component_patch_survives_untrusted_to_trusted_conversion() {
    // A creative item with unbreakable (unit), max stack size, custom name NBT,
    // and an explicit removal. Untrusted patches carry per-component lengths.
    let mut packet = vec![];
    packet.write_varint(1);
    packet.write_varint(1);
    packet.write_varint(3);
    packet.write_varint(1);
    packet.write_varint(4);
    packet.write_varint(0);
    packet.write_varint(1);
    packet.write_varint(1);
    packet.write_varint(16);
    packet.write_varint(5);
    packet.write_varint(4);
    packet.extend_from_slice(&[8, 0, 1, b'A']);
    packet.write_varint(3);
    let slot = components::read_untrusted_slot(&mut Cursor::new(packet)).unwrap();
    let mut trusted = vec![];
    trusted.write_slot_data(&slot);
    assert_eq!(trusted, vec![1, 1, 3, 1, 4, 1, 16, 5, 8, 0, 1, b'A', 3]);
    let decoded = components::read_slot(&mut Cursor::new(&trusted)).unwrap();
    let mut result = vec![];
    result.write_slot_data(&decoded);
    assert_eq!(result, trusted);
}
#[test]
fn nested_container_patch_and_invalid_components() {
    let bytes = vec![1, 1, 1, 0, 66, 2, 0, 2, 1, 1, 0, 4];
    let slot = components::read_slot(&mut Cursor::new(&bytes)).unwrap();
    let mut encoded = vec![];
    encoded.write_slot_data(&slot);
    assert_eq!(encoded, bytes);
    let entity = slot.unwrap().nbt.unwrap();
    assert!(matches!(
        entity.get("BlockEntityTag"),
        Some(nbt::Value::Compound(_))
    ));
    assert!(components::read_slot(&mut Cursor::new(vec![1, 1, 1, 0, 127])).is_err());
    assert!(components::read_slot(&mut Cursor::new(vec![1, 1, 1, 0, 5, 8])).is_err());
    assert!(components::read_untrusted_slot(&mut Cursor::new(vec![1, 1, 1, 0, 4, 1, 0])).is_err());
}
#[test]
fn target_nbt_and_position_packet_layouts() {
    let mut nbt = vec![];
    nbt.write_nbt_blob(&nbt::Blob::new());
    assert_eq!(nbt, vec![10, 0]);
    let packet = CPlayerPositionAndLook {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        yaw: 90.0,
        pitch: 0.0,
        flags: 0,
        teleport_id: 128,
        dismount_vehicle: false,
    }
    .encode();
    let mut cursor = Cursor::new(&packet.buffer);
    assert_eq!(cursor.read_varint().unwrap(), 128);
    assert_eq!(cursor.read_double().unwrap(), 1.0);
    assert_eq!(packet.buffer.len(), 2 + 6 * 8 + 2 * 4 + 4);
    for coords in [(-1, 0, -1), (-33554432, -2048, 33554431), (123, 255, -456)] {
        assert_eq!(
            PackedPos::new(coords.0, coords.1, coords.2).coords(),
            coords
        );
    }
}
#[test]
fn compressed_status_framing_round_trips() {
    for size in [0, 254, 255, 256, 2000] {
        let mut data = vec![];
        PacketEncoder::new(vec![0; size], 0)
            .write_compressed(&mut data)
            .unwrap();
        let mut state = NetworkState::Status;
        read_packet(
            &mut Cursor::new(data),
            &Arc::new(AtomicBool::new(true)),
            &mut state,
        )
        .unwrap();
    }
}
