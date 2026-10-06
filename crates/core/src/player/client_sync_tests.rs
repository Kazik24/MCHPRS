use super::*;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::{connection, read_frame};

fn xyz(pos: PlayerPos) -> [f64; 3] {
    [pos.x, pos.y, pos.z]
}

fn read_teleport(peer: &mut std::net::TcpStream, destination: PlayerPos) -> i32 {
    let (packet_id, mut frame) = read_frame(peer, false).unwrap();
    assert_eq!(packet_id, 0x41); // Minecraft 1.21.5 Synchronize Player Position.
    let id = frame.read_varint().unwrap();
    assert_eq!(
        [
            frame.read_double().unwrap(),
            frame.read_double().unwrap(),
            frame.read_double().unwrap()
        ],
        xyz(destination)
    );
    for _ in 0..3 {
        assert_eq!(frame.read_double().unwrap(), 0.0);
    }
    assert_eq!(frame.read_float().unwrap(), 35.0);
    assert_eq!(frame.read_float().unwrap(), -10.0);
    assert_eq!(PacketDecoderExt::read_int(&mut frame).unwrap(), 0); // Absolute coordinates AND rotation.
    assert_eq!(frame.position() as usize, frame.get_ref().len());
    id
}

#[test]
fn queued_movement_cannot_undo_teleports_or_release_a_newer_destination() {
    let conn = connection(false).unwrap();
    let mut peer = conn.peer;
    let mut player = Player::from_data(Default::default(), 0, "SyncTest".into(), conn.player);
    player.yaw = 35.0;
    player.pitch = -10.0;
    let original = player.pos;
    let first = PlayerPos::new(64.0, 80.0, 64.0);
    let second = PlayerPos::new(128.0, 90.0, 128.0);
    player.teleport(first);
    let first_id = read_teleport(&mut peer, first);
    assert!(player
        .accept_position(original, true, Some((180.0, 90.0)))
        .is_none());
    assert_eq!(xyz(player.pos), xyz(first));
    assert_eq!((player.yaw, player.pitch), (35.0, -10.0));

    player.teleport(second);
    let second_id = read_teleport(&mut peer, second);
    assert_ne!(first_id, second_id);
    assert!(!player.confirm_teleport(first_id));
    assert!(player.accept_position(first, false, None).is_none());
    assert_eq!(xyz(player.pos), xyz(second));
    assert!(player.confirm_teleport(second_id));
    assert!(!player.confirm_teleport(second_id));

    let next = PlayerPos::new(129.0, 90.0, 128.0);
    assert_eq!(
        xyz(player
            .accept_position(next, true, Some((45.0, 5.0)))
            .unwrap()),
        xyz(second)
    );
    assert_eq!(xyz(player.pos), xyz(next));
    assert_eq!(
        (player.yaw, player.pitch, player.on_ground),
        (45.0, 5.0, true)
    );
}
