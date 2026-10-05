use super::clientbound::*;
use super::*;
use std::io::Cursor;

#[test]
fn clean_connection_closes_are_distinct_from_truncated_packet_frames() {
    let compression = Arc::new(AtomicBool::new(false));
    let mut state = NetworkState::Status;
    assert!(matches!(
        read_packet(&mut Cursor::new(Vec::<u8>::new()), &compression, &mut state),
        Err(PacketDecodeError::ConnectionClosed)
    ));
    // Partial length VarInt, partial body, and a complete frame with a short Ping.
    for bytes in [vec![0x80], vec![9, 1, 0], vec![2, 1, 0]] {
        let error = match read_packet(&mut Cursor::new(bytes), &compression, &mut state) {
            Err(error) => error,
            Ok(_) => panic!("truncated packet was accepted"),
        };
        match error {
            PacketDecodeError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof),
            PacketDecodeError::Packet { packet_id, source } => {
                assert_eq!(packet_id, 1);
                assert!(
                    matches!(*source, PacketDecodeError::Io(ref error) if error.kind() == io::ErrorKind::UnexpectedEof)
                );
            }
            _ => panic!("truncated frame must remain a decode error: {error:?}"),
        }
    }
}

#[test]
fn fragmented_and_interrupted_reads_preserve_frames_and_clean_closes() {
    struct Fragmented {
        data: Cursor<Vec<u8>>,
        interrupt: bool,
    }
    impl Read for Fragmented {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if std::mem::take(&mut self.interrupt) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let size = buffer.len().min(1);
            self.data.read(&mut buffer[..size])
        }
    }
    impl PacketDecoderExt for Fragmented {}
    #[derive(Default)]
    struct Handler {
        address: String,
        ping: i64,
    }
    impl ServerBoundPacketHandler for Handler {
        fn handle_handshake(&mut self, packet: SHandshake, _: usize) {
            self.address = packet.server_address;
        }
        fn handle_ping(&mut self, packet: SPing, _: usize) {
            self.ping = packet.payload;
        }
    }
    let address = "a".repeat(200); // Forces a multibyte frame length and string length.
    let mut handshake = vec![];
    handshake.write_varint(770);
    handshake.write_string(255, &address);
    handshake.write_unsigned_short(25565);
    handshake.write_varint(1);
    let mut bytes = vec![];
    PacketEncoder::new(handshake, 0)
        .write_uncompressed(&mut bytes)
        .unwrap();
    CPong { payload: 123456789 }
        .encode()
        .write_uncompressed(&mut bytes)
        .unwrap();
    let mut reader = Fragmented {
        data: Cursor::new(bytes),
        interrupt: true,
    };
    let mut state = NetworkState::Handshake;
    let compression = Arc::new(AtomicBool::new(false));
    let mut handler = Handler::default();
    read_packet(&mut reader, &compression, &mut state)
        .unwrap()
        .handle(&mut handler, 0);
    assert_eq!(state, NetworkState::Status);
    assert_eq!(handler.address, address);
    reader.interrupt = true;
    read_packet(&mut reader, &compression, &mut state)
        .unwrap()
        .handle(&mut handler, 0);
    assert_eq!(handler.ping, 123456789);
    assert!(matches!(
        read_packet(&mut reader, &compression, &mut state),
        Err(PacketDecodeError::ConnectionClosed)
    ));
}

#[test]
fn compressed_payload_errors_retain_packet_identity() {
    let mut bytes = vec![];
    PacketEncoder::new(vec![0], 1)
        .write_compressed(&mut bytes)
        .unwrap();
    let error = match read_packet(
        &mut Cursor::new(bytes),
        &Arc::new(AtomicBool::new(true)),
        &mut NetworkState::Status,
    ) {
        Err(error) => error,
        Ok(_) => panic!("short Ping was accepted"),
    };
    assert!(
        matches!(error, PacketDecodeError::Packet { packet_id: 1, source } if matches!(*source, PacketDecodeError::Io(ref error) if error.kind() == io::ErrorKind::UnexpectedEof))
    );
}

#[test]
fn forwarding_response_keeps_login_state_until_acknowledgement() {
    #[derive(Default)]
    struct Handler {
        data: Vec<u8>,
    }
    impl serverbound::ServerBoundPacketHandler for Handler {
        fn handle_login_plugin_response(
            &mut self,
            packet: serverbound::SLoginPluginResponse,
            _: usize,
        ) {
            assert_eq!(packet.message_id, 0);
            assert!(packet.successful);
            self.data = packet.data;
        }
    }
    let mut bytes = vec![2, 0, 1];
    bytes.extend_from_slice(&[7; 64]);
    let mut state = NetworkState::LoginAcknowledgement;
    let packet = read_decompressed(&mut Cursor::new(bytes), &mut state).unwrap();
    assert!(matches!(state, NetworkState::LoginAcknowledgement));
    let mut handler = Handler::default();
    packet.handle(&mut handler, 0);
    assert_eq!(handler.data, vec![7; 64]);
    assert!(serverbound::SLoginPluginResponse::decode(&mut Cursor::new(vec![0, 0, 1])).is_err());
    let mut oversized = vec![0, 1];
    oversized.extend_from_slice(&vec![0; 32769]);
    assert!(serverbound::SLoginPluginResponse::decode(&mut Cursor::new(oversized)).is_err());
}

#[test]
fn container_click_hashes_dispatch_and_validate_without_accepting_item_data() {
    use super::serverbound::*;
    #[derive(Default)]
    struct Handler {
        clicked: bool,
        closed: bool,
    }
    impl ServerBoundPacketHandler for Handler {
        fn handle_container_click(&mut self, p: SContainerClick, _: usize) {
            self.clicked = p.window_id == 200 && p.state_id == 9 && p.slot == 54 && p.mode == 0;
        }
        fn handle_container_close(&mut self, p: SContainerClose, _: usize) {
            self.closed = p.window_id == 200;
        }
    }
    let mut bytes = vec![0x10];
    bytes.write_varint(200);
    bytes.write_varint(9);
    bytes.write_short(54);
    bytes.write_byte(0);
    bytes.write_varint(0);
    bytes.write_varint(1);
    bytes.write_short(54);
    bytes.write_bool(true);
    bytes.write_varint(1);
    bytes.write_varint(64);
    bytes.write_varint(1);
    bytes.write_varint(1);
    PacketEncoderExt::write_int(&mut bytes, 123456);
    bytes.write_varint(0);
    bytes.write_bool(false);
    let mut handler = Handler::default();
    let mut cursor = Cursor::new(&bytes);
    read_decompressed(&mut cursor, &mut NetworkState::Play)
        .unwrap()
        .handle(&mut handler, 0);
    assert_eq!(cursor.position() as usize, bytes.len());
    bytes.pop();
    assert!(read_decompressed(&mut Cursor::new(bytes), &mut NetworkState::Play).is_err());
    let mut close = vec![0x11];
    close.write_varint(200);
    read_decompressed(&mut Cursor::new(close), &mut NetworkState::Play)
        .unwrap()
        .handle(&mut handler, 0);
    assert!(handler.clicked && handler.closed);
    let mut oversized = vec![0x10, 1, 0, 0, 0, 0, 0];
    oversized.write_varint(129);
    assert!(read_decompressed(&mut Cursor::new(oversized), &mut NetworkState::Play).is_err());
}

#[test]
fn container_ids_are_varints_in_content_slot_and_close_packets() {
    let packets = [
        CWindowItems {
            window_id: 200,
            state_id: 9,
            slot_data: vec![],
            carried_item: None,
        }
        .encode(),
        CSetSlot {
            window_id: 200,
            state_id: 9,
            slot: 0,
            slot_data: None,
        }
        .encode(),
        CCloseWindow { window_id: 200 }.encode(),
    ];
    for packet in packets {
        let mut bytes = vec![];
        packet.write_uncompressed(&mut bytes).unwrap();
        let mut cursor = Cursor::new(bytes);
        cursor.read_varint().unwrap();
        cursor.read_varint().unwrap();
        assert_eq!(cursor.read_varint().unwrap(), 200);
    }
}

#[test]
fn pick_packets_dispatch_and_reject_truncated_payloads() {
    use super::serverbound::*;
    #[derive(Default)]
    struct Handler {
        block: bool,
        entity: bool,
    }
    impl ServerBoundPacketHandler for Handler {
        fn handle_pick_item_from_block(&mut self, packet: SPickItemFromBlock, _: usize) {
            self.block = packet.include_data;
        }
        fn handle_pick_item_from_entity(&mut self, packet: SPickItemFromEntity, _: usize) {
            self.entity = packet.entity_id == 123 && !packet.include_data;
        }
    }
    let mut handler = Handler::default();
    let mut state = NetworkState::Play;
    let mut block = vec![0x22];
    block.write_long(0);
    assert!(read_decompressed(&mut Cursor::new(&block), &mut state).is_err());
    block.write_bool(true);
    read_decompressed(&mut Cursor::new(block), &mut state)
        .unwrap()
        .handle(&mut handler, 0);
    let mut entity = vec![0x23];
    entity.write_varint(123);
    entity.write_bool(false);
    read_decompressed(&mut Cursor::new(entity), &mut state)
        .unwrap()
        .handle(&mut handler, 0);
    assert!(handler.block && handler.entity);
}

#[test]
fn command_block_updates_validate_mode_flags_and_dispatch() {
    use super::serverbound::*;
    struct Handler(bool);
    impl ServerBoundPacketHandler for Handler {
        fn handle_update_command_block(&mut self, packet: SUpdateCommandBlock, _: usize) {
            self.0 = packet.command == "say test" && packet.mode == 2 && packet.flags == 1;
        }
    }
    let mut prefix = vec![0x34];
    prefix.write_long(0);
    prefix.write_string(32767, "say test");
    for (mode, flags) in [(2, 1), (3, 1), (0, 8)] {
        let mut bytes = prefix.clone();
        bytes.write_varint(mode);
        bytes.write_unsigned_byte(flags);
        let result = read_decompressed(&mut Cursor::new(bytes), &mut NetworkState::Play);
        if mode == 2 {
            let mut handler = Handler(false);
            result.unwrap().handle(&mut handler, 0);
            assert!(handler.0);
        } else {
            assert!(result.is_err());
        }
    }
    assert!(read_decompressed(&mut Cursor::new(prefix), &mut NetworkState::Play).is_err());
}

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
