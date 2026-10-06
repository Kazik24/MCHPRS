use super::*;
use crate::plot::client_test_utils::{read_ack, read_blocks};
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_WIDTH};
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::SignBlockEntity;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::read_frame;
use mchprs_network::{test_support::connection, BlockActionAcknowledgement, PlayerPacketSender};

#[test]
fn screen_only_paste_and_repeated_paste_publish_authoritative_nonlamp_blocks() {
    for compressed in [false, true] {
        let mut world = world();
        let source = BlockPos::new(4, 20, 4);
        let destination = BlockPos::new(32, 20, 32);
        world.set_block(source, Block::Sandstone {});
        let clipboard = create_clipboard(&mut world, source, source, source);
        let sign_source = source.offset(mchprs_blocks::BlockFace::Top);
        let sign_block = Block::from_name("oak_sign").unwrap();
        world.set_block(sign_source, sign_block);
        world.set_block_entity(
            sign_source,
            BlockEntity::Sign(Box::new(SignBlockEntity {
                rows: std::array::from_fn(|_| r#"{"text":"pasted text"}"#.into()),
                ..Default::default()
            })),
        );
        let sign_clipboard = create_clipboard(&mut world, sign_source, sign_source, sign_source);
        world.set_screen_only(true);
        let conn = connection(compressed).unwrap();
        let mut peer = conn.peer;
        world
            .packet_senders
            .push(PlayerPacketSender::new(&conn.player));

        // The second paste writes the same stored state. It must still refresh
        // a client which previously retained a stale/filtered chunk snapshot.
        for sequence in 1..=2 {
            let ack = BlockActionAcknowledgement::new(&conn.player, sequence);
            paste_clipboard(&mut world, &clipboard, destination, false);
            drop(ack);
            assert_eq!(
                read_blocks(&mut peer, compressed),
                [(destination, Block::Sandstone {}.get_id())]
            );
            read_ack(&mut peer, compressed, sequence);
            assert_eq!(world.get_block(destination), Block::Sandstone {});
        }
        // A nested caller's edit scope survives the paste's internal flushes.
        let previous = world.set_authoritative_updates(true);
        paste_clipboard(&mut world, &clipboard, destination, false);
        assert_eq!(
            read_blocks(&mut peer, compressed),
            [(destination, Block::Sandstone {}.get_id())]
        );
        let adjacent = destination.offset(mchprs_blocks::BlockFace::East);
        world.set_block(adjacent, Block::Sandstone {});
        world.flush_block_changes();
        assert_eq!(
            read_blocks(&mut peer, compressed),
            [(adjacent, Block::Sandstone {}.get_id())]
        );
        world.set_authoritative_updates(previous);
        world.set_block(adjacent, Block::Air);
        world.flush_block_changes();
        drop(BlockActionAcknowledgement::new(&conn.player, 3));
        read_ack(&mut peer, compressed, 3);

        // Entity data has a consumer boundary too: the client must know the
        // sign block before receiving its text, even with rendering filtered.
        let sign_destination = destination.offset(mchprs_blocks::BlockFace::South);
        paste_clipboard(&mut world, &sign_clipboard, sign_destination, false);
        assert_eq!(
            read_blocks(&mut peer, compressed),
            [(sign_destination, sign_block.get_id())]
        );
        let (id, mut data) = read_frame(&mut peer, compressed).unwrap();
        assert_eq!(id, 0x06); // Block Entity Data in protocol 770.
        assert_eq!(
            BlockPos::from_packed(data.read_position().unwrap()),
            sign_destination
        );
        assert_eq!(
            data.read_varint().unwrap(),
            BlockEntity::Sign(Default::default()).ty()
        );
        // Network NBT has no root name in 1.21.5; the file-NBT reader expects one.
        assert_eq!(data.read_unsigned_byte().unwrap(), 10);
        let mut named = vec![10, 0, 0];
        named.extend_from_slice(&data.get_ref()[data.position() as usize..]);
        let sign_data = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).unwrap();
        assert!(format!("{sign_data:?}").contains("pasted text"));
        assert!(matches!(
            world.get_block_entity(sign_destination),
            Some(BlockEntity::Sign(_))
        ));
    }
}

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn update_crosses_unaligned_section_boundaries_and_leaves_outside_blocks_alone() {
    let mut world = world();
    let selected = BlockPos::new(16, 16, 16);
    let outside = BlockPos::new(19, 16, 16);
    for lamp in [selected, outside] {
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        world.set_block(
            lamp.offset(mchprs_blocks::BlockFace::East),
            Block::RedstoneBlock {},
        );
    }
    // The first section is empty. Reversed selection order is also supported.
    update_selection(&mut world, selected, BlockPos::new(15, 15, 15)).unwrap();
    assert_eq!(world.get_block(selected), Block::RedstoneLamp { lit: true });
    assert_eq!(world.get_block(outside), Block::RedstoneLamp { lit: false });
}

#[test]
fn invalid_update_bounds_are_rejected_before_any_callbacks() {
    let mut world = world();
    let lamp = BlockPos::new(16, 16, 16);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        lamp.offset(mchprs_blocks::BlockFace::East),
        Block::RedstoneBlock {},
    );
    for invalid in [
        BlockPos::new(-1, 16, 16),
        BlockPos::new(256, 16, 16),
        BlockPos::new(16, -1, 16),
        BlockPos::new(16, PLOT_BLOCK_HEIGHT, 16),
    ] {
        assert!(update_selection(&mut world, lamp, invalid).is_err());
        assert!(update_selection(&mut world, invalid, lamp).is_err());
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
        assert_eq!(world.scheduler().iter_entries().count(), 0);
    }
}
