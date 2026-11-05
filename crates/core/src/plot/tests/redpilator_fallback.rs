use super::*;
use mchprs_network::packets::{
    PacketDecoderExt,
    serverbound::{SPlayerBlockPlacemnt, ServerBoundPacketHandler},
};
use mchprs_network::test_support::read_frame;
use std::net::TcpStream;

fn fixture(name: &str, powered: bool) -> (Plot, TcpStream, BlockPos) {
    let (mut plot, peer) = client_sync_tests::fixture(false);
    plot.owner = Some(plot.players[0].uuid);
    plot.players[0].set_test_permissions(&["*"]);
    plot.players[0].inventory[36] = None;
    let pos = BlockPos::new(33, 21, 34);
    plot.world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    plot.world
        .set_block(pos, Block::from_name(name).unwrap().with_pressure_plate_power(powered).unwrap());
    plot.redpilator
        .compile(
            &plot.world,
            (BlockPos::new(30, 19, 30), BlockPos::new(38, 23, 38)),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap();
    plot.auto_redpilator = true;
    (plot, peer, pos)
}

fn click(plot: &mut Plot, pos: BlockPos, sequence: i32) {
    plot.handle_player_block_placement(
        SPlayerBlockPlacemnt {
            hand: 0,
            pos: pos.packed(),
            face: BlockFace::Top.get_id() as i32,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            sequence,
        },
        0,
    );
}

fn messages_through_ack(peer: &mut TcpStream, sequence: i32) -> Vec<String> {
    let mut messages = vec![];
    for _ in 0..128 {
        let (id, mut packet) = read_frame(peer, false).unwrap();
        if id == 0x72 {
            messages.push(String::from_utf8_lossy(packet.get_ref()).into_owned());
        }
        if id == 0x04 {
            assert_eq!(packet.read_varint().unwrap(), sequence);
            return messages;
        }
    }
    panic!("missing interaction acknowledgement");
}

#[test]
fn pressure_plate_noop_contact_and_empty_hand_click_keep_redpilator_active() {
    for name in ["stone_pressure_plate", "oak_pressure_plate", "polished_blackstone_pressure_plate"] {
        for powered in [false, true] {
            let (mut plot, mut peer, pos) = fixture(name, powered);
            plot.set_pressure_plate(pos, powered);
            if powered {
                plot.players[0].pos = PlayerPos::new(pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5);
                plot.players[0].on_ground = true;
                let same = plot.players[0].pos;
                plot.on_player_move(0, same, same);
            }
            click(&mut plot, pos, 1);
            assert!(
                messages_through_ack(&mut peer, 1).is_empty(),
                "no-op must not report an edit failure or fallback"
            );
            assert!(plot.redpilator.is_active(), "{name}, powered={powered}");
            assert!(plot.auto_redpilator);
            assert_eq!(plot.world.get_block(pos).pressure_plate_powered(), Some(powered));
        }
    }
}

#[test]
fn pressure_plate_contact_is_a_silent_compiled_input() {
    for initially_powered in [false, true] {
        let (mut plot, mut peer, pos) = fixture("stone_pressure_plate", initially_powered);
        plot.set_pressure_plate(pos, !initially_powered);
        plot.set_pressure_plate(pos, !initially_powered);
        assert!(plot.redpilator.is_active());
        assert!(plot.auto_redpilator, "routine player contact must not change auto-compilation policy");
        assert_eq!(plot.world.get_block(pos).pressure_plate_powered(), Some(!initially_powered));
        click(&mut plot, pos, 1);
        let messages = messages_through_ack(&mut peer, 1);
        assert!(messages.is_empty(), "ordinary circuit inputs must not report fallback");
    }
}

#[test]
fn runtime_failure_handoff_reports_once_and_returns_engine_ownership() {
    let (mut plot, mut peer, pos) = fixture("stone_pressure_plate", false);
    let reason = "Compiled execution failed at (33, 21, 34): compiled tick operation budget exceeded";
    assert!(plot.fallback_redpilator(reason));
    assert!(!plot.redpilator.is_active());
    assert!(plot.redpilator_cache.is_some(), "handoff should retain the exported checkpoint");
    assert!(plot.fallback_redpilator(reason), "already handed-off execution is a no-op");
    click(&mut plot, pos, 1);
    let messages = messages_through_ack(&mut peer, 1);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("compiled tick operation budget exceeded"));
    assert!(messages[0].contains("/hl 33 21 34"));
}

#[test]
fn explicit_redpilator_reset_does_not_claim_automatic_fallback() {
    let (mut plot, mut peer, pos) = fixture("stone_pressure_plate", false);
    plot.reset_redpilator().unwrap();
    click(&mut plot, pos, 1);
    assert!(messages_through_ack(&mut peer, 1).is_empty());
    assert!(plot.auto_redpilator);
}

#[test]
fn failed_handoff_latch_blocks_repeated_contact_and_clears_after_explicit_reset() {
    let (mut plot, mut peer, pos) = fixture("stone_pressure_plate", false);
    // Exercise retained ownership after an export failure without corrupting executor internals.
    plot.redpilator_handoff_failed = true;
    plot.auto_redpilator = false;
    plot.players[0].pos = PlayerPos::new(pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5);
    plot.players[0].on_ground = true;
    let same = plot.players[0].pos;
    for _ in 0..20 {
        plot.on_player_move(0, same, same);
        assert!(!plot.tick(), "paused failed ownership must not advance compiled work");
    }
    click(&mut plot, pos, 1);
    assert!(
        messages_through_ack(&mut peer, 1).is_empty(),
        "failed handoff must not retry or repeat diagnostics per packet"
    );
    assert!(plot.redpilator.is_active());
    assert!(plot.redpilator_cache.is_none(), "no repeated export attempt should complete");
    assert_eq!(plot.world.get_block(pos).pressure_plate_powered(), Some(false));
    plot.reset_redpilator().unwrap();
    assert!(!plot.redpilator_handoff_failed);
    plot.set_pressure_plate(pos, true);
    assert_eq!(plot.world.get_block(pos).pressure_plate_powered(), Some(true));
}

#[test]
fn holding_a_block_on_a_pressure_plate_still_uses_the_compiled_edit_lock() {
    let (mut plot, mut peer, pos) = fixture("stone_pressure_plate", false);
    plot.players[0].inventory[36] = Some(mchprs_blocks::items::ItemStack {
        item_type: mchprs_blocks::items::Item::Sandstone {},
        count: 1,
        nbt: None,
    });
    click(&mut plot, pos, 1);
    let messages = messages_through_ack(&mut peer, 1);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains(messages::PLOT_CANNOT_INTERACTED_WHILE_REDPILATOR_ACTIVE));
    assert!(plot.redpilator.is_active());
    assert_eq!(plot.world.get_block(pos.offset(BlockFace::Top)), Block::Air);
}
