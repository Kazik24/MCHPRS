use super::*;
use mchprs_blocks::block_entities::SignBlockEntity;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_network::packets::serverbound::{
    SPlayerBlockPlacemnt, SUpdateSign, ServerBoundPacketHandler,
};
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::read_frame;
use std::net::TcpStream;

fn click(pos: BlockPos, sequence: i32) -> SPlayerBlockPlacemnt {
    SPlayerBlockPlacemnt {
        hand: 0,
        pos: pos.packed(),
        face: BlockFace::Top.get_id() as i32,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        sequence,
    }
}

fn editor_packets(peer: &mut TcpStream, compressed: bool, sequence: i32) -> Vec<(BlockPos, bool)> {
    let mut editors = Vec::new();
    for _ in 0..128 {
        let (id, mut frame) = read_frame(peer, compressed).unwrap();
        match id {
            0x35 => {
                editors.push((
                    BlockPos::from_packed(frame.read_position().unwrap()),
                    frame.read_bool().unwrap(),
                ));
                assert_eq!(frame.position() as usize, frame.get_ref().len());
            }
            0x04 => {
                assert_eq!(frame.read_varint().unwrap(), sequence);
                return editors;
            }
            _ => {}
        }
    }
    panic!("sign click was not acknowledged");
}

#[test]
fn right_click_reopens_both_sign_sides_and_preserves_other_text_and_style() {
    for compressed in [false, true] {
        for (name, property, value, front_dx, front_dz) in [
            ("oak_sign", "rotation", "0", 0.0, 2.0),
            ("spruce_sign", "rotation", "4", -2.0, 0.0),
            ("cherry_sign", "rotation", "8", 0.0, -2.0),
            ("bamboo_sign", "rotation", "12", 2.0, 0.0),
            ("oak_sign", "rotation", "2", -2.0, 2.0),
            ("oak_wall_sign", "facing", "north", 0.0, -2.0),
            ("cherry_wall_sign", "facing", "east", 2.0, 0.0),
            ("bamboo_wall_sign", "facing", "south", 0.0, 2.0),
            ("spruce_wall_sign", "facing", "west", -2.0, 0.0),
        ] {
            let (mut plot, mut peer) = client_sync_tests::fixture(compressed);
            plot.world.packet_senders.clear();
            let pos = BlockPos::new(32, 21, 32);
            let mut block = Block::from_name(name).unwrap();
            block.set_properties(std::collections::HashMap::from([(property, value)]));
            plot.world.set_block(pos, block);
            let original = SignBlockEntity {
                rows: std::array::from_fn(|_| r#"{"text":"front"}"#.into()),
                back_rows: std::array::from_fn(|_| r#"{"text":"back"}"#.into()),
                front_color: "red".into(),
                back_color: "blue".into(),
                front_glow: true,
                back_glow: true,
                ..Default::default()
            };
            plot.world
                .set_block_entity(pos, BlockEntity::Sign(Box::new(original.clone())));
            for (sequence, front) in [(1, true), (2, false)] {
                let side = if front { 1.0 } else { -1.0 };
                plot.players[0].pos = PlayerPos::new(
                    pos.x as f64 + 0.5 + side * front_dx,
                    pos.y as f64,
                    pos.z as f64 + 0.5 + side * front_dz,
                );
                plot.players[0].inventory[36] = if front {
                    None
                } else {
                    Some(ItemStack {
                        item_type: Item::Sandstone {},
                        count: 1,
                        nbt: None,
                    })
                };
                plot.handle_player_block_placement(click(pos, sequence), 0);
                assert_eq!(
                    editor_packets(&mut peer, compressed, sequence),
                    [(pos, front)],
                    "{name}"
                );
                plot.handle_update_sign(
                    SUpdateSign {
                        pos: pos.packed(),
                        front,
                        lines: std::array::from_fn(|_| format!("edited {front}")),
                    },
                    0,
                );
                let Some(BlockEntity::Sign(sign)) = plot.world.get_block_entity(pos) else {
                    panic!("sign disappeared")
                };
                let rows = std::array::from_fn::<_, 4, _>(|_| {
                    serde_json::json!({ "text": format!("edited {front}") }).to_string()
                });
                if front {
                    assert_eq!(sign.rows, rows);
                    assert_eq!(sign.back_rows, original.back_rows);
                } else {
                    assert_eq!(sign.back_rows, rows);
                    assert!(sign.rows.iter().all(|row| row.contains("edited true")));
                }
                assert_eq!(sign.front_color, original.front_color);
                assert_eq!(sign.back_color, original.back_color);
                assert!(sign.front_glow && sign.back_glow);
                assert_eq!(plot.world.get_block(pos.offset(BlockFace::Top)), Block::Air);
            }
        }
    }
}

#[test]
fn sign_editor_respects_wax_permissions_and_reach() {
    for scenario in ["waxed", "permission", "reach", "sneaking"] {
        let (mut plot, mut peer) = client_sync_tests::fixture(false);
        plot.world.packet_senders.clear();
        let pos = BlockPos::new(32, 21, 32);
        plot.world
            .set_block(pos, Block::from_name("oak_sign").unwrap());
        plot.world.set_block_entity(
            pos,
            BlockEntity::Sign(Box::new(SignBlockEntity {
                waxed: scenario == "waxed",
                ..Default::default()
            })),
        );
        plot.players[0].inventory[36] = None;
        if scenario == "permission" {
            plot.owner = None;
            plot.players[0].deny_test_permissions();
        }
        if scenario == "reach" {
            plot.players[0].pos.x += 20.0;
        }
        plot.players[0].crouching = scenario == "sneaking";
        plot.handle_player_block_placement(click(pos, 1), 0);
        assert!(editor_packets(&mut peer, false, 1).is_empty(), "{scenario}");
    }
}

#[test]
fn sign_placement_sends_the_block_before_opening_the_editor() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = client_sync_tests::fixture(compressed);
        let support = BlockPos::new(32, 20, 32);
        let pos = support.offset(BlockFace::Top);
        plot.players[0].inventory[36] = Some(ItemStack {
            item_type: Item::from_name("cherry_sign").unwrap(),
            count: 1,
            nbt: None,
        });
        plot.handle_player_block_placement(click(support, 1), 0);
        let mut saw_sign = false;
        let mut saw_editor = false;
        loop {
            let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
            match id {
                0x08 | 0x4d => {
                    saw_sign |= client_test_utils::decode_blocks(id, &mut frame)
                        .into_iter()
                        .any(|(p, state)| p == pos && Block::from_id(state).is_sign());
                }
                0x35 => {
                    assert!(saw_sign, "editor arrived before its sign block");
                    assert_eq!(BlockPos::from_packed(frame.read_position().unwrap()), pos);
                    assert!(frame.read_bool().unwrap());
                    saw_editor = true;
                }
                0x04 => {
                    assert_eq!(frame.read_varint().unwrap(), 1);
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_editor);
    }
}

fn world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

#[test]
fn placing_signs_creates_editable_entities_and_preserves_text_on_rotation() {
    let mut world = world();
    let pos = BlockPos::new(31, 20, 47);
    for face in BlockFace::values() {
        world.set_block(pos.offset(face), Block::Stone {});
    }
    for name in [
        "oak_sign",
        "spruce_wall_sign",
        "cherry_sign",
        "bamboo_wall_sign",
    ] {
        let mut block = Block::from_name(name).unwrap();
        crate::interaction::place_in_world(block, &mut world, pos, &None);
        assert!(
            matches!(world.get_block_entity(pos), Some(BlockEntity::Sign(_))),
            "{name}"
        );

        let sign = SignBlockEntity {
            rows: std::array::from_fn(|_| r#"{"text":"front"}"#.into()),
            back_rows: std::array::from_fn(|_| r#"{"text":"back"}"#.into()),
            ..Default::default()
        };
        let entity = BlockEntity::Sign(Box::new(sign));
        world.set_block_entity(pos, entity.clone());
        block.rotate(mchprs_blocks::blocks::RotateAmt::Rotate90);
        world.set_block(pos, block);
        assert_eq!(
            world
                .get_block_entity(pos)
                .unwrap()
                .to_nbt(false)
                .unwrap()
                .content,
            entity.to_nbt(false).unwrap().content
        );

        world.set_block(pos, Block::Stone {});
        assert!(world.get_block_entity(pos).is_none());
    }
}
