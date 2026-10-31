use super::*;

fn test_uuid() -> u128 {
    0x1234_5678_9abc_def0_1122_3344_5566_7788
}

#[test]
fn state_uses_java_data_output_stream_layout() {
    let position = Position {
        world: "plot_1_-2".into(),
        x: 32.5,
        y: 64.0,
        z: -8.25,
        yaw: 90.0,
        pitch: -45.0,
    };
    let data = encode_player_state(test_uuid(), &position);
    assert_eq!(&data[..2], &[0x00, 0x05]);
    assert_eq!(&data[2..7], b"STATE");
    let text = HyphenatedUUID(test_uuid()).to_string();
    assert_eq!(&data[7..9], &(text.len() as u16).to_be_bytes());
    assert_eq!(&data[9..9 + text.len()], text.as_bytes());
    let mut rest = &data[9 + text.len()..];
    assert_eq!(read_utf(&mut rest).unwrap(), "plot_1_-2");
    assert_eq!(read_f64(&mut rest).unwrap(), 32.5);
    assert_eq!(read_f64(&mut rest).unwrap(), 64.0);
    assert_eq!(read_f64(&mut rest).unwrap(), -8.25);
    assert_eq!(read_f32(&mut rest).unwrap(), 90.0);
    assert_eq!(read_f32(&mut rest).unwrap(), -45.0);
    assert!(rest.is_empty());
}

#[test]
fn teleport_and_gamemode_round_trip() {
    let teleport = encode(TELEPORT, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        write_position(
            data,
            &Position {
                world: "world".into(),
                x: 1.5,
                y: 2.5,
                z: 3.5,
                yaw: 10.0,
                pitch: 20.0,
            },
        );
    });
    let Incoming::Teleport { uuid, position } = decode(&teleport).unwrap() else {
        panic!("expected a teleport");
    };
    assert_eq!(uuid, self::test_uuid());
    assert_eq!(position.world, "world");
    assert_eq!(position.player_pos(), PlayerPos::new(1.5, 2.5, 3.5));

    let gamemode = encode(GAMEMODE, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        data.extend_from_slice(&2_i32.to_be_bytes());
    });
    let Incoming::Gamemode { uuid, mode } = decode(&gamemode).unwrap() else {
        panic!("expected a gamemode");
    };
    assert_eq!(uuid, self::test_uuid());
    assert_eq!(mode, 2);
    assert_eq!(gamemode_from_id(mode), Some(Gamemode::Adventure));
    assert_eq!(gamemode_from_id(0), Some(Gamemode::Survival));
    assert_eq!(Gamemode::Survival.get_id(), 0);
    assert_eq!(gamemode_from_id(9), None);
}

#[test]
fn permissions_round_trip() {
    let request = encode_permissions_request(test_uuid());
    assert_eq!(&request[..2], &(("PERMISSIONS_REQUEST".len() as u16).to_be_bytes()));

    let payload = encode(PERMISSIONS, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        write_utf(data, "vip");
        write_utf(data, "[VIP] ");
        write_utf(data, "");
        data.extend_from_slice(&3_i32.to_be_bytes());
        for (node, value) in [("mchprs.build", 1u8), ("mchprs.*", 1u8), ("mchprs.admin", 0u8)] {
            write_utf(data, node);
            data.push(value);
        }
    });
    let Incoming::Permissions {
        uuid,
        group,
        prefix,
        suffix,
        permissions,
    } = decode(&payload).unwrap()
    else {
        panic!("expected permissions");
    };
    assert_eq!(uuid, test_uuid());
    assert_eq!(group, "vip");
    assert_eq!(prefix, "[VIP] ");
    assert_eq!(suffix, "");
    assert_eq!(
        permissions,
        vec![("mchprs.build".into(), true), ("mchprs.*".into(), true), ("mchprs.admin".into(), false),]
    );

    // A truncated count or a missing boolean is rejected.
    for length in 0..payload.len() {
        assert!(decode(&payload[..length]).is_err(), "prefix {length}");
    }
}

#[test]
fn powertool_messages_round_trip() {
    let held = encode_held_item(test_uuid(), "wooden_axe");
    assert_eq!(&held[..2], &(("HELD_ITEM".len() as u16).to_be_bytes()));

    let click = encode_pt_click(test_uuid(), 0, "wooden_axe");
    // Rebuilt the way the proxy reads it: uuid, int button, item.
    let rebuilt = encode(PT_CLICK, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        data.extend_from_slice(&0_i32.to_be_bytes());
        write_utf(data, "wooden_axe");
    });
    assert_eq!(click, rebuilt);

    let run = encode(PT_RUN, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        write_utf(data, "warp a1");
    });
    let Incoming::PtRun { uuid, command } = decode(&run).unwrap() else {
        panic!("expected a powertool command");
    };
    assert_eq!(uuid, test_uuid());
    assert_eq!(command, "warp a1");

    for invalid in ["", "/plot", "line\nfeed", &"x".repeat(1025)] {
        let payload = encode(PT_RUN, |data| {
            write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
            write_utf(data, invalid);
        });
        assert!(decode(&payload).is_err(), "{invalid:?}");
    }
}

#[test]
fn modified_utf8_handles_polish_text_and_null() {
    let mut data = Vec::new();
    write_utf(&mut data, "żółw\0");
    let mut bytes = &data[..];
    assert_eq!(read_utf(&mut bytes).unwrap(), "żółw\0");
    assert!(bytes.is_empty());

    for malformed in [
        &[0x00, 0x01, 0x00][..],
        &[0x00, 0x02, 0xc0, 0xaf][..],
        &[0x00, 0x03, 0xe0, 0x80, 0x80][..],
        &[0x00, 0x02, 0xc3, 0x77][..],
        &[0x00, 0x03, 0xed, 0xa0, 0xbd][..],
    ] {
        assert!(read_utf(&mut &malformed[..]).is_err(), "{malformed:?}");
    }
}

#[test]
fn malformed_payloads_are_rejected() {
    let valid = encode(TELEPORT, |data| {
        write_utf(data, &HyphenatedUUID(test_uuid()).to_string());
        write_position(
            data,
            &Position {
                world: "w".into(),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
    });
    for length in 0..valid.len() {
        assert!(decode(&valid[..length]).is_err(), "prefix {length}");
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode(&trailing).is_err());
    let unknown = encode("NOPE", |data| write_utf(data, &HyphenatedUUID(test_uuid()).to_string()));
    assert!(decode(&unknown).is_err());
}
