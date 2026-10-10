use super::*;
use crate::player::{Gamemode, PlayerPos, SmallAnimal};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::serverbound::*;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::{connection, read_frame};
use mchprs_network::PlayerPacketSender;
use std::io::Cursor;
use std::net::TcpStream;

fn fixture(compressed: bool) -> (Plot, TcpStream, TcpStream) {
    let (mut plot, owner) = crate::plot::client_sync_tests::fixture(compressed);
    let observer = connection(compressed).unwrap();
    let mut viewer = Player::test_player(observer.player);
    viewer.uuid = 2;
    plot.world
        .packet_senders
        .push(PlayerPacketSender::new(&viewer.client));
    plot.players.push(viewer);
    (plot, owner, observer.peer)
}

fn packet(peer: &mut TcpStream, compressed: bool, expected: i32) -> Cursor<Vec<u8>> {
    let (id, body) = read_frame(peer, compressed).unwrap();
    assert_eq!(id, expected);
    body
}

fn scale(peer: &mut TcpStream, compressed: bool, entity: u32, expected: f64) {
    let mut body = packet(peer, compressed, 0x7c);
    assert_eq!(body.read_varint().unwrap(), entity as i32);
    assert_eq!(body.read_varint().unwrap(), 1);
    assert_eq!(body.read_varint().unwrap(), 24);
    assert_eq!(body.read_double().unwrap(), expected);
    assert_eq!(body.read_varint().unwrap(), 0);
    assert_eq!(body.position() as usize, body.get_ref().len());
}

fn flags(peer: &mut TcpStream, compressed: bool, expected: u8) {
    let mut body = packet(peer, compressed, 0x5c);
    body.read_varint().unwrap();
    assert_eq!(body.read_unsigned_byte().unwrap(), 0);
    assert_eq!(body.read_varint().unwrap(), 0);
    assert_eq!(body.read_unsigned_byte().unwrap(), expected);
}

fn proxy_type(peer: &mut TcpStream, compressed: bool) -> i32 {
    let mut spawn = packet(peer, compressed, 0x01);
    spawn.read_varint().unwrap();
    spawn.read_uuid().unwrap();
    spawn.read_varint().unwrap()
}

fn proxy_held_item(
    peer: &mut TcpStream,
    entity: u32,
    compressed: bool,
) -> Option<mchprs_network::packets::SlotData> {
    let mut equipment = packet(peer, compressed, 0x5f);
    assert_eq!(equipment.read_varint().unwrap(), entity as i32);
    assert_eq!(equipment.read_unsigned_byte().unwrap(), 0);
    let item = mchprs_network::packets::components::read_slot(&mut equipment).unwrap();
    assert_eq!(equipment.position() as usize, equipment.get_ref().len());
    item
}

fn enable(plot: &mut Plot, owner: &mut TcpStream, viewer: &mut TcpStream, compressed: bool) -> u32 {
    let entity = plot.players[0].entity_id;
    assert!(plot.set_small(0, true));
    let model = plot.players[0].small_model.as_ref().unwrap().entity_id;
    assert_ne!(entity, model);
    scale(owner, compressed, entity, 0.5);
    flags(owner, compressed, 0);
    packet(owner, compressed, 0x72);
    scale(viewer, compressed, entity, 0.5);
    flags(viewer, compressed, 0x20);
    let mut equipment = packet(viewer, compressed, 0x5f);
    assert_eq!(equipment.read_varint().unwrap(), entity as i32);
    for slot in 0..6 {
        assert_eq!(
            equipment.read_unsigned_byte().unwrap(),
            slot | if slot == 5 { 0 } else { 0x80 }
        );
        assert_eq!(
            equipment.read_varint().unwrap(),
            0,
            "hidden equipment must not float beside the ocelot"
        );
    }
    let mut spawn = packet(viewer, compressed, 0x01);
    assert_eq!(spawn.read_varint().unwrap(), model as i32);
    assert_eq!(
        spawn.read_uuid().unwrap(),
        plot.players[0].small_model.as_ref().unwrap().uuid
    );
    assert_eq!(
        spawn.read_varint().unwrap(),
        mchprs_network::generated::OCELOT_ENTITY
    );
    for expected in [32.5, 21.0, 35.5] {
        assert_eq!(spawn.read_double().unwrap(), expected);
    }
    packet(viewer, compressed, 0x5c);
    model
}

#[test]
fn disguise_lifecycle_keeps_owner_interactive_and_restores_size_only_with_clearance() {
    for compressed in [false, true] {
        let (mut plot, mut owner, mut viewer) = fixture(compressed);
        let model = enable(&mut plot, &mut owner, &mut viewer, compressed);
        let inventory = plot.players[0].inventory[36].clone();
        assert_eq!(plot.players[0].scale(), 0.5);
        assert!((plot.players[0].eye_position().y - 21.81).abs() < 1e-9);
        assert!(plot.set_small(0, true));
        packet(&mut owner, compressed, 0x72);
        assert_eq!(
            plot.players[0].small_model.as_ref().unwrap().entity_id,
            model
        );

        let ceiling = BlockPos::new(32, 22, 35);
        plot.world.set_block(ceiling, Block::Stone {});
        assert!(!plot.set_small(0, false));
        packet(&mut owner, compressed, 0x72);
        assert_eq!(
            plot.players[0].small_model.as_ref().unwrap().entity_id,
            model
        );
        assert!(super::super::compass::body_clear(
            plot.players[0].pos,
            0.5,
            &|p| Some(plot.world.get_block(p))
        ));
        plot.world.set_block(ceiling, Block::Air);
        assert!(plot.set_small(0, false));
        for peer in [&mut owner, &mut viewer] {
            let mut remove = packet(peer, compressed, 0x46);
            assert_eq!(remove.read_varint().unwrap(), 1);
            assert_eq!(remove.read_varint().unwrap(), model as i32);
            scale(peer, compressed, plot.players[0].entity_id, 1.0);
            flags(peer, compressed, 0);
        }
        packet(&mut owner, compressed, 0x72);
        packet(&mut viewer, compressed, 0x5f);
        assert!(plot.players[0].small_model.is_none());
        let restored = plot.players[0].inventory[36].as_ref().unwrap();
        let inventory = inventory.unwrap();
        assert_eq!(restored.item_type, inventory.item_type);
        assert_eq!(restored.count, inventory.count);
    }
}

#[test]
fn movement_rotation_and_teleports_sync_the_model_without_echoing_it_to_owner() {
    let (mut plot, mut owner, mut viewer) = fixture(false);
    let model = enable(&mut plot, &mut owner, &mut viewer, false);
    plot.handle_player_position_and_rotation(
        SPlayerPositionAndRotation {
            x: 33.0,
            y: 21.0,
            z: 35.5,
            yaw: 90.0,
            pitch: 25.0,
            on_ground: true,
        },
        0,
    );
    packet(&mut viewer, false, 0x2f);
    packet(&mut viewer, false, 0x4c);
    plot.sync_small_models();
    let mut position = packet(&mut viewer, false, 0x76);
    assert_eq!(position.read_varint().unwrap(), model as i32);
    for expected in [33.0, 21.0, 35.5] {
        assert_eq!(position.read_double().unwrap(), expected);
    }
    for _ in 0..3 {
        assert_eq!(position.read_double().unwrap(), 0.0);
    }
    assert_eq!(position.read_float().unwrap(), 90.0);
    assert_eq!(position.read_float().unwrap(), 25.0);
    packet(&mut viewer, false, 0x4c);
    plot.sync_small_models();
    // No unchanged-pose or owner-preview packets precede the next status message.
    plot.players[0].send_system_message("owner sentinel");
    plot.players[1].send_system_message("observer sentinel");
    packet(&mut owner, false, 0x72);
    packet(&mut viewer, false, 0x72);
    plot.players[0].pos = PlayerPos::new(40.5, 25.0, 40.5);
    plot.sync_small_models();
    let mut position = packet(&mut viewer, false, 0x76);
    assert_eq!(position.read_varint().unwrap(), model as i32);
    assert_eq!(position.read_double().unwrap(), 40.5);
    assert_eq!(position.read_double().unwrap(), 25.0);
    assert_eq!(position.read_double().unwrap(), 40.5);
    packet(&mut viewer, false, 0x4c);
}

#[test]
fn new_viewers_receive_the_disguise_and_sneaking_keeps_invisibility() {
    let (mut plot, mut owner, mut viewer) = fixture(false);
    let model = enable(&mut plot, &mut owner, &mut viewer, false);
    plot.handle_entity_action(
        SEntityAction {
            entity_id: plot.players[0].entity_id as i32,
            action_id: 0,
            jump_boost: 0,
        },
        0,
    );
    flags(&mut owner, false, 0x02);
    flags(&mut viewer, false, 0x22);
    assert!((plot.players[0].eye_position().y - 21.635).abs() < 1e-9);
    let conn = connection(false).unwrap();
    let newcomer = Player::test_player(conn.player);
    let mut peer = conn.peer;
    Plot::spawn_player(&newcomer, &plot.players[0]);
    packet(&mut peer, false, 0x01);
    flags(&mut peer, false, 0x22);
    scale(&mut peer, false, plot.players[0].entity_id, 0.5);
    packet(&mut peer, false, 0x5f);
    let mut spawn = packet(&mut peer, false, 0x01);
    assert_eq!(spawn.read_varint().unwrap(), model as i32);
    assert_eq!(
        spawn.read_uuid().unwrap(),
        plot.players[0].small_model.as_ref().unwrap().uuid
    );
    assert_eq!(
        spawn.read_varint().unwrap(),
        mchprs_network::generated::OCELOT_ENTITY
    );
    packet(&mut peer, false, 0x5c);
    let departed = plot.leave_plot(plot.players[0].uuid);
    assert!(departed.small_model.is_some());
    for entity in [departed.entity_id, model] {
        let mut destroy = packet(&mut viewer, false, 0x46);
        assert_eq!(destroy.read_varint().unwrap(), 1);
        assert_eq!(destroy.read_varint().unwrap(), entity as i32);
    }
}

#[test]
fn small_players_place_break_and_use_worldedit_through_normal_handlers() {
    let (mut plot, mut owner) = crate::plot::client_sync_tests::fixture(false);
    assert!(plot.set_small(0, true));
    for id in [0x7c, 0x5c, 0x72] {
        packet(&mut owner, false, id);
    }
    let support = BlockPos::new(32, 20, 32);
    let placed = support.offset(BlockFace::Top);
    plot.handle_player_block_placement(
        SPlayerBlockPlacemnt {
            hand: 0,
            pos: support.packed(),
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            sequence: 1,
        },
        0,
    );
    assert_eq!(plot.world.get_block(placed), Block::Sandstone {});
    plot.handle_player_digging(
        SPlayerDigging {
            status: 0,
            pos: placed.packed(),
            face: 1,
            sequence: 2,
        },
        0,
    );
    assert_eq!(plot.world.get_block(placed), Block::Air);
    let mut args = Vec::new();
    assert!(crate::plot::worldedit::execute_command(
        &mut plot, 0, "/wand", &mut args
    ));
    assert!(matches!(
        plot.players[0].inventory[36].as_ref().unwrap().item_type,
        mchprs_blocks::items::Item::WEWand {}
    ));
    plot.handle_player_block_placement(
        SPlayerBlockPlacemnt {
            hand: 0,
            pos: support.packed(),
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            sequence: 3,
        },
        0,
    );
    assert_eq!(plot.players[0].second_position, Some(support));
    assert!(plot.players[0].small_model.is_some());
}

#[test]
fn cat_gamemode_enables_creative_and_normal_gamemode_restores_size() {
    let (mut plot, _peer) = crate::plot::client_sync_tests::fixture(false);
    plot.players[0].gamemode = Gamemode::Spectator;
    assert!(!plot.handle_command(0, "/gm", vec!["cat"]));
    assert!(matches!(plot.players[0].gamemode, Gamemode::Creative));
    assert!(plot.players[0].small_model.is_some());
    plot.world
        .set_block(BlockPos::new(32, 22, 35), Block::Stone {});
    plot.handle_command(0, "/gm", vec!["spectator"]);
    assert!(matches!(plot.players[0].gamemode, Gamemode::Creative));
    assert!(plot.players[0].small_model.is_some());
    plot.world.set_block(BlockPos::new(32, 22, 35), Block::Air);
    plot.handle_command(0, "/gm", vec!["creative"]);
    assert!(plot.players[0].small_model.is_none());
    plot.handle_command(0, "/small", vec![]);
    assert!(plot.players[0].small_model.is_some());
    plot.handle_command(0, "/small", vec![]);
    assert!(plot.players[0].small_model.is_none());
    plot.handle_command(0, "/small", vec!["on"]);
    assert!(plot.players[0].small_model.is_some());
    plot.handle_command(0, "/small", vec!["off"]);
    assert!(plot.players[0].small_model.is_none());
}

#[test]
fn small_animal_commands_spawn_generated_models_and_update_fox_equipment() {
    let (mut plot, mut owner, mut viewer) = fixture(false);
    enable(&mut plot, &mut owner, &mut viewer, false);
    plot.players[0].inventory[37] = Some(ItemStack {
        item_type: Item::Stick {},
        count: 1,
        nbt: None,
    });

    for (name, animal, expected_entity) in [
        (
            "wolf",
            SmallAnimal::Wolf,
            mchprs_network::generated::WOLF_ENTITY,
        ),
        (
            "fox",
            SmallAnimal::Fox,
            mchprs_network::generated::FOX_ENTITY,
        ),
        (
            "cat",
            SmallAnimal::Cat,
            mchprs_network::generated::CAT_ENTITY,
        ),
        (
            "ocelot",
            SmallAnimal::Ocelot,
            mchprs_network::generated::OCELOT_ENTITY,
        ),
    ] {
        assert!(!plot.handle_command(0, "/small", vec![name]));
        assert_eq!(plot.players[0].small_animal, animal);
        packet(&mut owner, false, 0x46);
        packet(&mut owner, false, 0x72);
        packet(&mut viewer, false, 0x46);
        let model = plot.players[0].small_model.as_ref().unwrap().entity_id;
        assert_eq!(proxy_type(&mut viewer, false), expected_entity);
        packet(&mut viewer, false, 0x5c);
        if animal == SmallAnimal::Fox {
            let item = proxy_held_item(&mut viewer, model, false).unwrap();
            assert_eq!(item.item_id, Item::Sandstone {}.get_id() as i32);
            assert_eq!(item.item_count, 64);

            plot.sync_small_models();
            packet(&mut viewer, false, 0x76);
            packet(&mut viewer, false, 0x4c);
            proxy_held_item(&mut viewer, model, false);
            plot.players[0].selected_slot = 1;
            plot.sync_small_models();
            let item = proxy_held_item(&mut viewer, model, false).unwrap();
            assert_eq!(item.item_id, Item::Stick {}.get_id() as i32);
        }
    }
}

#[test]
fn baby_ocelot_uses_its_model_and_quarter_size_hitbox() {
    let (mut plot, mut owner, mut viewer) = fixture(false);
    enable(&mut plot, &mut owner, &mut viewer, false);

    assert!(!plot.handle_command(0, "/small", vec!["baby"]));
    assert_eq!(plot.players[0].small_animal, SmallAnimal::BabyOcelot);
    assert_eq!(plot.players[0].scale(), 0.25);
    assert!((plot.players[0].eye_position().y - 21.405).abs() < 1e-9);

    for peer in [&mut owner, &mut viewer] {
        let mut destroy = packet(peer, false, 0x46);
        assert_eq!(destroy.read_varint().unwrap(), 1);
        destroy.read_varint().unwrap();
        scale(peer, false, plot.players[0].entity_id, 0.25);
    }
    packet(&mut owner, false, 0x72);
    assert_eq!(
        proxy_type(&mut viewer, false),
        mchprs_network::generated::OCELOT_ENTITY
    );

    let mut metadata = packet(&mut viewer, false, 0x5c);
    assert_eq!(
        metadata.read_varint().unwrap(),
        plot.players[0].small_model.as_ref().unwrap().entity_id as i32
    );
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 5);
    assert_eq!(metadata.read_varint().unwrap(), 8);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 1);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 16);
    assert_eq!(metadata.read_varint().unwrap(), 8);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 1);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 0xff);

    let roof = |pos: BlockPos| {
        Some(if pos.y == 21 {
            Block::Stone {}
        } else {
            Block::Air {}
        })
    };
    let feet = PlayerPos::new(32.5, 20.5, 35.5);
    assert!(!super::super::compass::body_clear(feet, 0.5, &roof));
    assert!(super::super::compass::body_clear(feet, 0.25, &roof));

    assert!(!plot.handle_command(0, "/small", vec!["ocelot"]));
    assert_eq!(plot.players[0].small_animal, SmallAnimal::Ocelot);
    assert_eq!(plot.players[0].scale(), 0.5);
    for peer in [&mut owner, &mut viewer] {
        let mut destroy = packet(peer, false, 0x46);
        assert_eq!(destroy.read_varint().unwrap(), 1);
        destroy.read_varint().unwrap();
        scale(peer, false, plot.players[0].entity_id, 0.5);
    }
    packet(&mut owner, false, 0x72);
    proxy_type(&mut viewer, false);
    let mut metadata = packet(&mut viewer, false, 0x5c);
    metadata.read_varint().unwrap();
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 5);
    assert_eq!(metadata.read_varint().unwrap(), 8);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 1);
    assert_eq!(metadata.read_unsigned_byte().unwrap(), 0xff);
}
