use super::*;
use crate::plot::client_sync_tests::fixture;
use mchprs_blocks::blocks::RedstoneWire;
use mchprs_network::packets::serverbound::{SPlayerDigging, SUseItem};
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::read_frame;
use std::net::TcpStream;

fn equip(plot: &mut Plot) {
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: tool_item(),
        count: 1,
        nbt: None,
    });
    plot.players[0].selected_slot = 0;
    plot.players[0].redstone_tools.wire = Some(Session::new(0));
}

fn snapshot(world: &PlotWorld, start: BlockPos, end: BlockPos) -> Snapshot {
    let mut capture = Capture::new(world, start, end).unwrap();
    for _ in 0..4096 {
        if let Some(snapshot) = capture.step(world).unwrap() {
            return snapshot;
        }
    }
    panic!("bounded fixture capture did not finish");
}

fn read_through_ack(peer: &mut TcpStream, sequence: i32) -> Vec<i32> {
    let mut ids = Vec::new();
    for _ in 0..128 {
        let (id, mut frame) = read_frame(peer, false).unwrap();
        ids.push(id);
        if id == 0x04 {
            assert_eq!(frame.read_varint().unwrap(), sequence);
            return ids;
        }
    }
    panic!("missing action acknowledgement");
}

fn wait_for_preview(plot: &mut Plot) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        plot.update_wire_tools();
        let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
        if session.plan.is_some() && session.displayed {
            return;
        }
        assert!(Instant::now() < deadline,
            "Live route did not reach a buildable preview: target={:?}, notice={:?}, pending={}, capture={}, check={}, needs_search={}",
            session.target, session.notice, session.pending.is_some(), session.capture.is_some(), session.check.is_some(), session.needs_search);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn staged_route(plot: &mut Plot) -> (BlockPos, BlockPos, Block) {
    equip(plot);
    let start = BlockPos::new(28, 21, 35);
    let end = BlockPos::new(32, 21, 35);
    for x in start.x..=end.x {
        plot.world
            .set_block(BlockPos::new(x, 19, 35), Block::Stone {});
    }
    plot.world
        .set_block(start.offset(BlockFace::Bottom), Block::Stone {});
    let old = Block::RedstoneWire {
        wire: RedstoneWire::default(),
    };
    plot.world.set_block(start, old);
    plot.players[0].pitch = 90.0;
    plot.players[0].yaw = 0.0;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            0.0,
            90.0,
            Some(start),
            Plane::Horizontal
        ),
        Some(end)
    );
    let reads = snapshot(&plot.world, start, end);
    let path: Vec<_> = (start.x..=end.x)
        .map(|x| BlockPos::new(x, 21, 35))
        .collect();
    let placements = path
        .iter()
        .skip(1)
        .map(|&pos| (pos.offset(BlockFace::Bottom), Block::Glass {}))
        .chain(path.iter().skip(1).map(|&pos| (pos, old)))
        .collect();
    let plan = Plan {
        placements,
        path,
        reads: reads.clone(),
    };
    let mut session = Session::new(0);
    session.start = Some(start);
    session.target = Some(end);
    session.snapshot = Some(reads);
    session.publish(SearchResult::Found(plan));
    let mut complete = false;
    for _ in 0..session.wanted.len() {
        complete = preview::reconcile_markers(
            &plot.players[0],
            &mut session.markers,
            &session.wanted,
            128,
        );
        if complete {
            break;
        }
    }
    assert!(complete);
    session.displayed = true;
    plot.players[0].redstone_tools.wire = Some(session);
    (start, end, old)
}

#[test]
fn command_uses_empty_hotbar_slot_and_refuses_to_overwrite_full_inventory() {
    let (mut plot, _peer) = fixture(false);
    let existing = plot.players[0].inventory[36].as_ref().unwrap().item_type;
    plot.wire_tool(0, &[]).unwrap();
    assert_eq!(
        plot.players[0].inventory[36].as_ref().unwrap().item_type,
        existing
    );
    assert_eq!(plot.players[0].selected_slot, 1);
    assert_eq!(
        plot.players[0].inventory[37].as_ref().unwrap().item_type,
        tool_item()
    );
    assert!(plot.wire_held(0));
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["off"]));
    assert!(plot.players[0].redstone_tools.wire.is_none());
    for slot in 36..45 {
        plot.players[0].inventory[slot] = Some(ItemStack {
            item_type: existing,
            count: 64,
            nbt: None,
        });
    }
    assert!(plot.wire_tool(0, &[]).is_err());
    assert!(plot.players[0].inventory[36..45]
        .iter()
        .all(|item| item.as_ref().unwrap().item_type == existing));
    assert!(plot.players[0].redstone_tools.wire.is_none());
}

#[test]
fn invalid_and_offhand_use_acknowledge_without_starting_or_editing() {
    let (mut plot, mut peer) = fixture(false);
    equip(&mut plot);
    let pos = BlockPos::new(32, 20, 32);
    let before = plot.world.get_block_raw(pos);
    for (sequence, hand, yaw, pitch) in [
        (1, 1, 0.0, 90.0),
        (2, 0, f32::NAN, 90.0),
        (3, 0, 0.0, f32::INFINITY),
    ] {
        plot.handle_use_item(
            SUseItem {
                hand,
                sequence,
                yaw,
                pitch,
            },
            0,
        );
        assert_eq!(read_through_ack(&mut peer, sequence), [0x04]);
        assert!(plot.players[0]
            .redstone_tools
            .wire
            .as_ref()
            .unwrap()
            .start
            .is_none());
        assert_eq!(plot.world.get_block_raw(pos), before);
        assert!(plot.players[0].worldedit_undo.is_empty());
    }
}

#[test]
fn swap_cycles_mode_drop_changes_bend_without_losing_pen_and_dig_is_corrected() {
    let (mut plot, mut peer) = fixture(false);
    equip(&mut plot);
    plot.players[0].inventory[45] = Some(ItemStack {
        item_type: Item::Sandstone {},
        count: 32,
        nbt: None,
    });
    let pos = BlockPos::new(32, 20, 32);
    let before = plot.world.get_block_raw(pos);
    plot.handle_player_digging(
        SPlayerDigging {
            status: 6,
            pos: pos.packed(),
            face: 1,
            sequence: 1,
        },
        0,
    );
    assert_eq!(read_through_ack(&mut peer, 1), [0x04]);
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::VerticalX);
    assert!(session.prefer_x);
    assert_eq!(session.generation, 1);
    assert_eq!(
        plot.players[0].inventory[36].as_ref().unwrap().item_type,
        tool_item()
    );
    assert_eq!(plot.players[0].inventory[45].as_ref().unwrap().count, 32);
    plot.players[0].crouching = true;
    plot.handle_player_digging(
        SPlayerDigging {
            status: 6,
            pos: pos.packed(),
            face: 1,
            sequence: 2,
        },
        0,
    );
    assert_eq!(read_through_ack(&mut peer, 2), [0x04]);
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::VerticalZ);
    assert!(session.prefer_x);
    assert_eq!(session.generation, 2);
    let cancel = Arc::new(AtomicBool::new(false));
    let (_reply, receiver) = mpsc::sync_channel(1);
    let start = pos.offset(BlockFace::Top);
    let target = start + BlockPos::new(3, 1, 3);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    session.start = Some(start);
    session.target = Some(target);
    session.pending = Some(Pending {
        cancel: cancel.clone(),
        reply: receiver,
    });
    for (sequence, status, crouching, prefer_x) in [(3, 4, false, false), (4, 3, true, true)] {
        plot.players[0].crouching = crouching;
        plot.handle_player_digging(
            SPlayerDigging {
                status,
                pos: pos.packed(),
                face: 1,
                sequence,
            },
            0,
        );
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x14);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.read_short().unwrap(), 36);
        assert_eq!(frame.read_varint().unwrap(), 1);
        assert_eq!(frame.read_varint().unwrap(), tool_item().get_id() as i32);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.position() as usize, frame.get_ref().len());
        assert_eq!(read_through_ack(&mut peer, sequence), [0x04]);
        let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
        assert_eq!(session.plane, Plane::VerticalZ);
        assert_eq!(session.prefer_x, prefer_x);
        assert_eq!(session.generation, sequence as u64);
        assert_eq!(session.start, Some(start));
        assert_eq!(session.target, Some(target));
        assert!(session.needs_search);
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(plot.players[0].inventory[36].as_ref().unwrap().count, 1);
        assert_eq!(plot.players[0].inventory[45].as_ref().unwrap().count, 32);
    }
    plot.players[0].crouching = false;
    plot.handle_player_digging(
        SPlayerDigging {
            status: 0,
            pos: pos.packed(),
            face: 1,
            sequence: 5,
        },
        0,
    );
    let ids = read_through_ack(&mut peer, 5);
    assert!(ids[..ids.len() - 1].contains(&0x08));
    assert_eq!(plot.world.get_block_raw(pos), before);
    assert!(plot.players[0].worldedit_undo.is_empty());
}

#[test]
fn stopping_cancels_worker_and_destroys_only_its_markers() {
    let (mut plot, mut peer) = fixture(false);
    equip(&mut plot);
    let mut other_overlay = HashMap::new();
    assert!(preview::reconcile_markers(
        &plot.players[0],
        &mut other_overlay,
        &HashMap::from([(BlockPos::new(33, 21, 35), 4)]),
        1,
    ));
    let (packet, mut frame) = read_frame(&mut peer, false).unwrap();
    assert_eq!(packet, 0x01);
    let git_entity_id = frame.read_varint().unwrap();
    assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x5c);
    let mut session = plot.players[0].redstone_tools.wire.take().unwrap();
    let pos = BlockPos::new(32, 21, 35);
    session.wanted.insert(pos, 0);
    assert!(preview::reconcile_markers(
        &plot.players[0],
        &mut session.markers,
        &session.wanted,
        1
    ));
    let id = session.markers[&pos].0;
    let cancel = Arc::new(AtomicBool::new(false));
    let (_reply, receiver) = mpsc::sync_channel(1);
    session.pending = Some(Pending {
        cancel: cancel.clone(),
        reply: receiver,
    });
    plot.players[0].redstone_tools.wire = Some(session);
    plot.clear_wire_tool(0);
    assert!(cancel.load(Ordering::Relaxed));
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x01);
    assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x5c);
    let (packet, mut frame) = read_frame(&mut peer, false).unwrap();
    assert_eq!(packet, 0x46);
    assert_eq!(frame.read_varint().unwrap(), 1);
    assert_eq!(frame.read_varint().unwrap(), id);
    assert_ne!(id, git_entity_id);
    assert_eq!(other_overlay.len(), 1);
    preview::clear_markers(&plot.players[0], &mut other_overlay);
    let (packet, mut frame) = read_frame(&mut peer, false).unwrap();
    assert_eq!(packet, 0x46);
    assert_eq!(frame.read_varint().unwrap(), 1);
    assert_eq!(frame.read_varint().unwrap(), git_entity_id);
}

#[test]
fn older_worker_generation_cannot_publish_a_route() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let pos = BlockPos::new(32, 21, 35);
    let reads = snapshot(&plot.world, pos, pos);
    let mut session = plot.players[0].redstone_tools.wire.take().unwrap();
    session.generation = 2;
    session.snapshot = Some(reads.clone());
    let (reply, receiver) = mpsc::sync_channel(1);
    reply
        .send((
            1,
            SearchResult::Found(Plan {
                path: vec![pos],
                placements: Vec::new(),
                reads,
            }),
        ))
        .unwrap_or_else(|_| panic!("fixture receiver disconnected"));
    session.pending = Some(Pending {
        cancel: Arc::new(AtomicBool::new(false)),
        reply: receiver,
    });
    plot.players[0].redstone_tools.wire = Some(session);
    plot.update_wire_tools();
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert!(session.pending.is_none());
    assert!(session.received.is_none());
    assert!(session.plan.is_none());
    assert!(session.wanted.is_empty());
}

#[test]
fn changed_live_world_refuses_commit_without_partial_placement_or_undo() {
    let (mut plot, _peer) = fixture(false);
    let (start, end, _) = staged_route(&mut plot);
    let obstruction = BlockPos::new(30, 21, 35);
    plot.world.set_block(obstruction, Block::Glass);
    assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
    assert_eq!(plot.world.get_block(obstruction), Block::Glass);
    assert!(matches!(plot.world.get_block(end), Block::Air));
    assert!(matches!(
        plot.world.get_block(start.offset(BlockFace::East)),
        Block::Air
    ));
    assert!(plot.players[0].worldedit_undo.is_empty());
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert!(session.plan.is_none());
    assert!(!session.displayed);
}

#[test]
fn committed_segment_is_one_sparse_undo_including_existing_endpoint_shape() {
    let (mut plot, _peer) = fixture(false);
    let (start, end, old) = staged_route(&mut plot);
    let untouched = BlockPos::new(33, 20, 35);
    plot.world.set_block(untouched, Block::Glass);
    // The unrelated edit changes a captured read. Refresh the fixture proof.
    let reads = snapshot(&plot.world, start, end);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    session.snapshot = Some(reads.clone());
    session.plan.as_mut().unwrap().reads = reads;
    assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
    assert!(matches!(
        plot.world.get_block(end),
        Block::RedstoneWire { .. }
    ));
    assert_ne!(plot.world.get_block(start), old);
    assert_eq!(plot.players[0].worldedit_undo.len(), 1);
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(end)
    );
    assert!(worldedit::execute_command(
        &mut plot,
        0,
        "/undo",
        &mut Vec::new()
    ));
    assert_eq!(plot.world.get_block(start), old);
    for x in start.x + 1..=end.x {
        assert!(matches!(
            plot.world.get_block(BlockPos::new(x, 21, 35)),
            Block::Air
        ));
        assert_eq!(
            plot.world.get_block(BlockPos::new(x, 20, 35)),
            Block::Air {}
        );
        assert_eq!(
            plot.world.get_block(BlockPos::new(x, 19, 35)),
            Block::Stone {}
        );
    }
    assert_eq!(plot.world.get_block(untouched), Block::Glass);
    assert_eq!(plot.players[0].worldedit_redo.len(), 1);
}

#[test]
fn partial_visibility_still_reconciles_visible_markers_and_never_allows_commit() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let visible = BlockPos::new(32, 21, 35);
    let outside = BlockPos::new(30_000_000, 21, 35);
    let mut session = plot.players[0].redstone_tools.wire.take().unwrap();
    let original = HashMap::from([(visible, 0)]);
    assert!(preview::reconcile_markers(
        &plot.players[0],
        &mut session.markers,
        &original,
        1
    ));
    let entity_id = session.markers[&visible].0;
    session.wanted = HashMap::from([(visible, 2), (outside, 2)]);
    session.displayed = true;
    plot.players[0].redstone_tools.wire = Some(session);
    plot.update_wire_tools();
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.markers[&visible], (entity_id, 2));
    assert!(!session.markers.contains_key(&outside));
    assert!(!session.displayed);
    plot.players[0]
        .redstone_tools
        .wire
        .as_mut()
        .unwrap()
        .wanted
        .remove(&visible);
    plot.update_wire_tools();
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert!(session.markers.is_empty());
    assert!(!session.displayed);
}

#[test]
fn rotating_work_cursor_services_later_players_before_earlier_heavy_previews() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let conn = mchprs_network::test_support::connection(false).unwrap();
    let _second_peer = conn.peer;
    let mut second = Player::test_player(conn.player);
    second.last_chunk_x = 2;
    second.last_chunk_z = 2;
    second.inventory[36] = Some(ItemStack {
        item_type: tool_item(),
        count: 1,
        nbt: None,
    });
    let pos = BlockPos::new(32, 21, 35);
    let mut session = Session::new(0);
    session.wanted.insert(pos, 4);
    second.redstone_tools.wire = Some(session);
    plot.players.push(second);
    // Start with the later player; an implementation which always starts at
    // index zero ignores this cursor and cannot provide round-robin fairness.
    plot.wire_cursor = 1;
    let first = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    first.wanted = (0..512)
        .map(|i| (BlockPos::new(16 + i % 32, 24, 16 + i / 32), 0))
        .collect();
    plot.update_wire_tools();
    let second = plot.players[1].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(second.markers[&pos].1, 4);
    assert!(second.displayed);
}

#[test]
fn each_construction_plane_allows_both_coordinates_and_keeps_its_anchor() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    plot.players[0].pos = PlayerPos::new(32.5, 34.0, 32.5);
    let start = BlockPos::new(40, 30, 40);
    let horizontal = [0.0, 90.0].map(|yaw| {
        aim(
            &plot.world,
            &plot.players[0],
            yaw,
            45.0,
            Some(start),
            Plane::Horizontal,
        )
        .unwrap()
    });
    assert!(horizontal.iter().all(|pos| pos.y == start.y));
    assert_ne!(horizontal[0].x, horizontal[1].x);
    assert_ne!(horizontal[0].z, horizontal[1].z);
    let vertical_x = [(0.0, 0.0), (-30.0, 0.0), (0.0, -30.0)].map(|(yaw, pitch)| {
        aim(
            &plot.world,
            &plot.players[0],
            yaw,
            pitch,
            Some(start),
            Plane::VerticalX,
        )
        .unwrap()
    });
    assert!(vertical_x.iter().all(|pos| pos.z == start.z));
    assert_ne!(vertical_x[0].x, vertical_x[1].x);
    assert_ne!(vertical_x[0].y, vertical_x[2].y);
    let vertical_z = [(-90.0, 0.0), (-60.0, 0.0), (-90.0, -30.0)].map(|(yaw, pitch)| {
        aim(
            &plot.world,
            &plot.players[0],
            yaw,
            pitch,
            Some(start),
            Plane::VerticalZ,
        )
        .unwrap()
    });
    assert!(vertical_z.iter().all(|pos| pos.x == start.x));
    assert_ne!(vertical_z[0].z, vertical_z[1].z);
    assert_ne!(vertical_z[0].y, vertical_z[2].y);
    plot.players[0].redstone_tools.wire.as_mut().unwrap().start = Some(start);
    for expected in [
        Plane::VerticalX,
        Plane::VerticalZ,
        Plane::Free,
        Plane::Horizontal,
    ] {
        assert!(plot.flip_wire_route(0, false));
        let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
        assert_eq!(session.plane, expected);
        assert_eq!(session.start, Some(start));
    }
}

#[test]
fn free_aim_accepts_different_xyz_endpoints_and_requires_a_pointed_block() {
    let (mut plot, _peer) = fixture(false);
    let start = BlockPos::new(28, 21, 35);
    let support = BlockPos::new(34, 24, 40);
    let target = support.offset(BlockFace::Top);
    plot.world.set_block(support, Block::Stone {});
    let horizontal = (2.0_f64.powi(2) + 5.0_f64.powi(2)).sqrt();
    let yaw = -(2.0_f64 / 5.0).atan().to_degrees() as f32;
    let pitch = -(1.88_f64 / horizontal).atan().to_degrees() as f32;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            yaw,
            pitch,
            Some(start),
            Plane::Free
        ),
        Some(target)
    );
    assert_ne!(start.x, target.x);
    assert_ne!(start.y, target.y);
    assert_ne!(start.z, target.z);
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            0.0,
            -90.0,
            Some(start),
            Plane::Free
        ),
        None
    );
    plot.world.set_block(
        target,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    plot.players[0].pos.y = 28.0;
    let pitch = (4.59_f64 / horizontal).atan().to_degrees() as f32;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            yaw,
            pitch,
            Some(start),
            Plane::Free
        ),
        Some(target)
    );
}

#[test]
fn mode_commands_preserve_the_start_cancel_stale_routes_and_reaim_immediately() {
    let (mut plot, _peer) = fixture(false);
    let (start, _, _) = staged_route(&mut plot);
    let support = BlockPos::new(36, 23, 39);
    let free_target = support.offset(BlockFace::Top);
    plot.world.set_block(support, Block::Stone {});
    plot.players[0].pos.y = 26.0;
    plot.players[0].yaw = -45.0;
    plot.players[0].pitch = (4.12_f64 / (4.0_f64.powi(2) * 2.0).sqrt())
        .atan()
        .to_degrees() as f32;
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    let old_generation = session.generation;
    let old_plan = session.plan.as_ref().unwrap();
    let late_plan = Plan {
        path: old_plan.path.clone(),
        placements: old_plan.placements.clone(),
        reads: old_plan.reads.clone(),
    };
    let canceled = Arc::new(AtomicBool::new(false));
    let (reply, receiver) = mpsc::sync_channel(1);
    session.pending = Some(Pending {
        cancel: canceled.clone(),
        reply: receiver,
    });
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["free"]));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::Free);
    assert_eq!(session.start, Some(start));
    assert_eq!(session.target, Some(free_target));
    assert!(session.generation > old_generation);
    assert!(session.plan.is_none());
    assert!(!session.displayed);
    assert!(canceled.load(Ordering::Relaxed));
    // A canceled worker may finish just before it observes cancellation.
    let _ = reply.send((old_generation, SearchResult::Found(late_plan)));
    plot.update_wire_tools();
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert!(session.plan.is_none());
    assert!(session.received.is_none());
    assert!(plot.players[0].worldedit_undo.is_empty());
    let expected = aim(
        &plot.world,
        &plot.players[0],
        plot.players[0].yaw,
        plot.players[0].pitch,
        Some(start),
        Plane::Horizontal,
    );
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["plane"]));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::Horizontal);
    assert_eq!(session.start, Some(start));
    assert_eq!(session.target, expected);
    assert_ne!(session.target, Some(free_target));
    assert!(session.plan.is_none());
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["free"]));
    for expected in [
        Plane::Horizontal,
        Plane::VerticalX,
        Plane::VerticalZ,
        Plane::Free,
        Plane::Horizontal,
    ] {
        assert!(plot.flip_wire_route(0, false));
        let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
        assert_eq!(session.plane, expected);
        assert_eq!(session.start, Some(start));
    }
    assert!(plot.handle_redstone_tools_command(0, "/wire", &[]));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::Horizontal);
    assert!(session.start.is_none());
}

#[test]
fn free_command_equips_a_new_pen_without_overwriting_the_held_item() {
    let (mut plot, _peer) = fixture(false);
    let original = plot.players[0].inventory[36].as_ref().unwrap().clone();
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["free"]));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.plane, Plane::Free);
    assert!(session.start.is_none());
    assert!(plot.wire_held(0));
    let preserved = plot.players[0].inventory[36].as_ref().unwrap();
    assert_eq!(preserved.item_type, original.item_type);
    assert_eq!(preserved.count, original.count);
    assert_eq!(preserved.nbt.is_some(), original.nbt.is_some());
    assert_eq!(plot.players[0].selected_slot, 1);
}

#[test]
fn live_free_route_changes_xyz_builds_and_undoes_the_complete_segment() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let first_support = BlockPos::new(32, 20, 35);
    let last_support = BlockPos::new(36, 23, 39);
    let start = first_support.offset(BlockFace::Top);
    let end = last_support.offset(BlockFace::Top);
    for pos in [first_support, last_support] {
        plot.world.set_block(pos, Block::Stone {});
    }
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["free"]));
    plot.players[0].pos = PlayerPos::new(32.5, 26.0, 35.5);
    plot.players[0].yaw = 0.0;
    plot.players[0].pitch = 90.0;
    assert!(plot.start_wire_route(0, first_support));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(start)
    );
    plot.players[0].yaw = -45.0;
    plot.players[0].pitch = (4.12_f64 / (4.0_f64.powi(2) * 2.0).sqrt())
        .atan()
        .to_degrees() as f32;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            plot.players[0].yaw,
            plot.players[0].pitch,
            Some(start),
            Plane::Free
        ),
        Some(end)
    );
    wait_for_preview(&mut plot);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    assert_eq!(session.target, Some(end));
    let plan = session.plan.as_ref().unwrap();
    let path = plan.path.clone();
    let placements = plan.placements.clone();
    assert_eq!(path.first(), Some(&start));
    assert_eq!(path.last(), Some(&end));
    session.last_click = None;
    let (yaw, pitch) = (plot.players[0].yaw, plot.players[0].pitch);
    assert!(plot.use_wire_tool(0, 0, yaw, pitch));
    assert_eq!(plot.players[0].worldedit_undo.len(), 1);
    for pos in path {
        let block = plot.world.get_block(pos);
        assert!(matches!(block, Block::RedstoneWire { .. }));
        assert!(crate::interaction::is_valid_position(
            block,
            &plot.world,
            pos
        ));
    }
    for &(pos, block) in &placements {
        if !matches!(block, Block::RedstoneWire { .. }) {
            assert_eq!(block, Block::Stone {});
            assert_eq!(plot.world.get_block(pos), block);
        }
    }
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(end)
    );
    assert!(worldedit::execute_command(
        &mut plot,
        0,
        "/undo",
        &mut Vec::new()
    ));
    for &(pos, _) in &placements {
        assert_eq!(plot.world.get_block(pos), Block::Air {});
    }
    assert_eq!(plot.world.get_block(first_support), Block::Stone {});
    assert_eq!(plot.world.get_block(last_support), Block::Stone {});
    assert_eq!(plot.players[0].worldedit_redo.len(), 1);
}

#[test]
fn full_worker_queue_retains_snapshot_and_allows_later_retry() {
    let (plot, _peer) = fixture(false);
    let start = BlockPos::new(32, 21, 35);
    let end = start.offset(BlockFace::East);
    let reads = snapshot(&plot.world, start, end);
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut first = Session::new(0);
    first.snapshot = Some(reads.clone());
    first.needs_search = true;
    assert!(first.submit(&sender, start, end));
    let mut second = Session::new(0);
    second.snapshot = Some(reads);
    second.needs_search = true;
    assert!(!second.submit(&sender, start, end));
    assert!(second.snapshot.is_some());
    assert!(second.needs_search);
    assert!(second.pending.is_none());
    let queued = receiver.try_recv().unwrap();
    assert_eq!((queued.start, queued.end), (start, end));
    assert!(second.submit(&sender, start, end));
    assert!(second.pending.is_some());
    assert!(!second.needs_search);
    assert!(second.snapshot.is_some());
}

#[test]
fn completed_refusal_stays_visible_when_clicked_again() {
    let (mut plot, _peer) = fixture(false);
    let (_, _, _) = staged_route(&mut plot);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    session.plan = None;
    session.needs_search = false;
    session.notice = Some(messages::WIRE_BUDGET.into());
    assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert_eq!(session.notice.as_deref(), Some(messages::WIRE_BUDGET));
    assert!(plot.players[0].worldedit_undo.is_empty());
}

#[test]
fn cursor_movement_keeps_pending_marker_storage_bounded() {
    let mut session = Session::new(0);
    session.start = Some(BlockPos::new(20, 20, 20));
    session.target = session.start;
    session.invalidate();
    for x in 21..10_000 {
        session.retarget(Some(BlockPos::new(x, 20, 20)));
        assert_eq!(session.wanted.len(), 2);
    }
}

#[test]
fn live_aim_worker_and_preview_build_a_line_on_the_generated_floor() {
    let (mut plot, mut peer) = fixture(false);
    for x in 0..4 {
        for z in 0..5 {
            plot.world.chunks[(x * crate::plot::PLOT_WIDTH + z) as usize] =
                Plot::generate_chunk(8, x, z);
        }
    }
    equip(&mut plot);
    plot.players[0].pos = PlayerPos::new(32.5, 8.0, 32.5);
    plot.players[0].yaw = 0.0;
    plot.players[0].pitch = 90.0;
    let start = BlockPos::new(32, 8, 32);
    let end = BlockPos::new(32, 8, 47);
    let drawing_started = Instant::now();
    assert!(plot.start_wire_route(0, BlockPos::new(32, 7, 32)));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(start)
    );
    plot.players[0].pitch = (1.62_f64 / 15.0).atan().to_degrees() as f32;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            0.0,
            plot.players[0].pitch,
            Some(start),
            Plane::Horizontal,
        ),
        Some(end)
    );
    wait_for_preview(&mut plot);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    session.last_click = None;
    let pitch = plot.players[0].pitch;
    assert!(plot.use_wire_tool(0, 0, 0.0, pitch));
    for z in start.z..=end.z {
        assert!(matches!(
            plot.world.get_block(BlockPos::new(32, 8, z)),
            Block::RedstoneWire { .. }
        ));
        assert_eq!(
            plot.world.get_block(BlockPos::new(32, 7, z)),
            Block::Sandstone {}
        );
    }
    assert_eq!(plot.players[0].worldedit_undo.len(), 1);
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(end)
    );

    plot.handle_use_item(
        SUseItem {
            hand: 1,
            sequence: 987,
            yaw: 0.0,
            pitch,
        },
        0,
    );
    let mut status_packets = 0;
    for _ in 0..256 {
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        if id == 0x72 {
            // System Chat: all routine wire status uses the action bar overlay.
            assert_eq!(
                frame.get_ref().last(),
                Some(&1),
                "Wire status leaked into chat"
            );
            status_packets += 1;
        }
        if id == 0x04 {
            assert_eq!(frame.read_varint().unwrap(), 987);
            assert!(
                status_packets <= 1 + drawing_started.elapsed().as_secs() as usize,
                "Wire status must be throttled during a short drawing gesture"
            );
            assert!(worldedit::execute_command(
                &mut plot,
                0,
                "/undo",
                &mut Vec::new()
            ));
            for z in start.z..=end.z {
                assert_eq!(plot.world.get_block(BlockPos::new(32, 8, z)), Block::Air {});
                assert_eq!(
                    plot.world.get_block(BlockPos::new(32, 7, z)),
                    Block::Sandstone {}
                );
            }
            return;
        }
    }
    panic!("missing final acknowledgement after live line placement");
}

#[test]
fn live_continuation_builds_several_supported_segments_then_turns_and_undoes() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let pillar = BlockPos::new(32, 20, 32);
    let start = pillar.offset(BlockFace::Top);
    plot.world.set_block(pillar, Block::Glass {});
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 32.5);
    plot.players[0].yaw = 0.0;
    plot.players[0].pitch = 90.0;
    assert!(plot.start_wire_route(0, pillar));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(start)
    );
    let endpoints = [
        BlockPos::new(32, 21, 47),
        BlockPos::new(32, 21, 62),
        BlockPos::new(32, 21, 77),
        BlockPos::new(44, 21, 77),
    ];
    let mut placed = HashSet::new();
    for (segment, end) in endpoints.into_iter().enumerate() {
        plot.players[0].pos = PlayerPos::new(f64::from(end.x) + 0.5, 24.0, f64::from(end.z) + 0.5);
        let source = plot.players[0].redstone_tools.wire.as_ref().unwrap().start;
        assert_eq!(
            aim(
                &plot.world,
                &plot.players[0],
                0.0,
                90.0,
                source,
                Plane::Horizontal
            ),
            Some(end)
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            plot.update_wire_tools();
            let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
            if session.target == Some(end) && session.plan.is_some() && session.displayed {
                break;
            }
            assert!(Instant::now() < deadline,
                "Segment {} did not produce a buildable route: source={:?}, target={:?}, notice={:?}, pending={}, capture={}, check={}, needs_search={}",
                segment + 1, session.start, session.target, session.notice, session.pending.is_some(), session.capture.is_some(), session.check.is_some(), session.needs_search);
            std::thread::sleep(Duration::from_millis(1));
        }
        let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
        let plan = session.plan.as_ref().unwrap();
        let previous = source.unwrap();
        assert_eq!(
            plan.path.len(),
            (end.x.abs_diff(previous.x) + end.z.abs_diff(previous.z)) as usize + 1
        );
        assert!(plan.path.iter().all(|pos| pos.y == start.y));
        assert!(plan
            .placements
            .iter()
            .all(|(_, block)| matches!(block, Block::Glass {} | Block::RedstoneWire { .. })));
        placed.extend(plan.placements.iter().map(|&(pos, _)| pos));
        session.last_click = None;
        assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
        assert_eq!(plot.players[0].worldedit_undo.len(), segment + 1);
        assert_eq!(
            plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
            Some(end)
        );
        assert_eq!(plot.world.get_block(pillar), Block::Glass {});
        assert!(matches!(
            plot.world.get_block(end),
            Block::RedstoneWire { .. }
        ));
    }
    for _ in endpoints {
        assert!(worldedit::execute_command(
            &mut plot,
            0,
            "/undo",
            &mut Vec::new()
        ));
    }
    assert!(plot.players[0].worldedit_undo.is_empty());
    assert_eq!(plot.players[0].worldedit_redo.len(), endpoints.len());
    assert_eq!(plot.world.get_block(pillar), Block::Glass {});
    for pos in placed {
        assert_eq!(plot.world.get_block(pos), Block::Air {});
    }
}

#[test]
fn status_uses_latest_action_bar_message_once_per_second_without_repeating() {
    #[derive(Default)]
    struct Viewer(std::cell::RefCell<Vec<mchprs_network::packets::PacketEncoder>>);
    impl PacketSender for Viewer {
        fn send_packet(&self, packet: &mchprs_network::packets::PacketEncoder) {
            self.0.borrow_mut().push(packet.clone());
        }
    }
    let viewer = Viewer::default();
    let mut session = Session::new(0);
    let now = Instant::now();
    session.set_status("initial");
    session.send_status(&viewer, now);
    session.set_status("obsolete");
    session.send_status(&viewer, now + Duration::from_millis(100));
    session.set_status("latest");
    session.send_status(&viewer, now + Duration::from_millis(200));
    assert_eq!(viewer.0.borrow().len(), 1);
    session.send_status(&viewer, now + NOTICE_INTERVAL);
    let packets = viewer.0.borrow();
    assert_eq!(packets.len(), 2);
    // Network NBT omits the root name; restore it for the file-NBT reader.
    let mut named = vec![10, 0, 0];
    named.extend_from_slice(&packets[1].buffer[1..packets[1].buffer.len() - 1]);
    let message = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).unwrap();
    assert_eq!(
        message.get("text"),
        Some(&nbt::Value::String(format!(
            "{} | latest",
            Plane::Horizontal.label()
        )))
    );
    assert!(packets
        .iter()
        .all(|packet| packet.packet_id == 0x72 && packet.buffer.last() == Some(&1)));
    drop(packets);
    session.send_status(&viewer, now + NOTICE_INTERVAL * 2);
    assert_eq!(viewer.0.borrow().len(), 2);
}

#[test]
fn holding_a_pen_starts_and_restarts_drawing_without_a_command() {
    let (mut plot, _peer) = fixture(false);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: tool_item(),
        count: 1,
        nbt: None,
    });
    plot.world
        .set_block(BlockPos::new(32, 20, 35), Block::Stone {});
    plot.players[0].pitch = 90.0;
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert!(plot.wire_tools_active());
    assert!(plot.start_wire_route(0, BlockPos::new(32, 20, 35)));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(BlockPos::new(32, 21, 35))
    );

    plot.handle_held_item_change(SHeldItemChange { slot: 1 }, 0);
    assert!(plot.players[0].redstone_tools.wire.is_none());
    plot.handle_held_item_change(SHeldItemChange { slot: 0 }, 0);
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert!(plot.start_wire_route(0, BlockPos::new(32, 20, 35)));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(BlockPos::new(32, 21, 35))
    );

    plot.clear_wire_tool(0);
    assert!(plot.flip_wire_route(0, false));
    let session = plot.players[0].redstone_tools.wire.as_ref().unwrap();
    assert!(session.start.is_none());
    assert_eq!(session.plane, Plane::VerticalX);
    assert!(session.prefer_x);

    plot.clear_wire_tool(0);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: Item::from_name("wooden_sword").unwrap(),
        count: 1,
        nbt: None,
    });
    assert!(!plot.use_wire_tool(0, 0, 0.0, 90.0));
    assert!(!plot.flip_wire_route(0, false));
    assert!(plot.players[0].redstone_tools.wire.is_none());
}

#[test]
fn wire_off_survives_inputs_item_changes_and_route_cleanup() {
    let (mut plot, mut peer) = fixture(false);
    equip(&mut plot);
    let support = BlockPos::new(32, 20, 35);
    plot.world.set_block(support, Block::Stone {});
    assert!(plot.start_wire_route(0, support));
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["off"]));
    assert!(plot.players[0].redstone_tools.wire_disabled);
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert!(!plot.wire_held(0));
    assert!(!plot.wire_tools_active());
    for crouching in [false, true] {
        plot.players[0].crouching = crouching;
        assert!(!plot.use_wire_tool(0, 0, 0.0, 90.0));
        assert!(!plot.flip_wire_route(0, false));
        assert!(!plot.flip_wire_route(0, true));
    }
    plot.players[0].crouching = false;
    plot.handle_use_item(
        SUseItem {
            hand: 0,
            sequence: 1,
            yaw: 0.0,
            pitch: 90.0,
        },
        0,
    );
    read_through_ack(&mut peer, 1);
    plot.handle_player_digging(
        SPlayerDigging {
            status: 6,
            pos: support.packed(),
            face: 1,
            sequence: 2,
        },
        0,
    );
    read_through_ack(&mut peer, 2);
    plot.handle_held_item_change(SHeldItemChange { slot: 1 }, 0);
    plot.handle_held_item_change(SHeldItemChange { slot: 0 }, 0);
    plot.clear_wire_tool(0);
    plot.unload_wire_chunk(0, 2, 2);
    plot.update_wire_tools();
    assert!(plot.players[0].redstone_tools.wire_disabled);
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert!(!plot.wire_held(0));
    assert!(!plot.wire_tools_active());
    assert_eq!(
        plot.players[0].inventory[36].as_ref().unwrap().item_type,
        tool_item()
    );
    assert_eq!(plot.world.get_block(support), Block::Stone {});
    assert_eq!(
        plot.world.get_block(support.offset(BlockFace::Top)),
        Block::Air {}
    );
    assert!(plot.players[0].worldedit_undo.is_empty());
    plot.players[0].inventory[36].as_mut().unwrap().count = 2;
    for (sequence, status, count) in [(3, 4, Some(1)), (4, 3, None)] {
        plot.handle_player_digging(
            SPlayerDigging {
                status,
                pos: support.packed(),
                face: 1,
                sequence,
            },
            0,
        );
        read_through_ack(&mut peer, sequence);
        assert_eq!(
            plot.players[0].inventory[36]
                .as_ref()
                .map(|item| item.count),
            count
        );
        assert!(plot.players[0].redstone_tools.wire_disabled);
        assert!(plot.players[0].redstone_tools.wire.is_none());
    }
}

#[test]
fn each_explicit_wire_command_reenables_drawing_after_off() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    plot.world
        .set_block(BlockPos::new(32, 20, 35), Block::Stone {});
    for slot in 37..45 {
        plot.players[0].inventory[slot] = Some(ItemStack {
            item_type: Item::Sandstone {},
            count: 64,
            nbt: None,
        });
    }
    for (args, mode) in [
        (&[][..], Plane::Horizontal),
        (&["free"][..], Plane::Free),
        (&["plane"][..], Plane::Horizontal),
    ] {
        assert!(plot.handle_redstone_tools_command(0, "/wire", &["off"]));
        assert!(!plot.wire_held(0));
        assert!(plot.handle_redstone_tools_command(0, "/wire", args));
        assert!(!plot.players[0].redstone_tools.wire_disabled);
        assert!(plot.wire_held(0));
        assert!(plot.wire_tools_active());
        assert_eq!(
            plot.players[0].redstone_tools.wire.as_ref().unwrap().plane,
            mode
        );
        assert!(plot.start_wire_route(0, BlockPos::new(32, 20, 35)));
        assert_eq!(
            plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
            Some(BlockPos::new(32, 21, 35))
        );
        assert!(plot.players[0].worldedit_undo.is_empty());
    }
}

#[test]
fn failed_wire_enable_keeps_off_and_off_requires_no_permission() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    plot.players[0].deny_test_permissions();
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["off"]));
    for args in [&[][..], &["free"][..], &["plane"][..]] {
        assert!(plot.handle_redstone_tools_command(0, "/wire", args));
        assert!(plot.players[0].redstone_tools.wire_disabled);
        assert!(plot.players[0].redstone_tools.wire.is_none());
        assert!(!plot.wire_held(0));
        assert!(!plot.wire_tools_active());
    }
    let (mut plot, _peer) = fixture(false);
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["off"]));
    plot.players[0].gamemode = Gamemode::Adventure;
    assert!(plot.handle_redstone_tools_command(0, "/wire", &["free"]));
    assert!(plot.players[0].redstone_tools.wire_disabled);
    plot.players[0].gamemode = Gamemode::Creative;
    for slot in 36..45 {
        plot.players[0].inventory[slot] = Some(ItemStack {
            item_type: Item::Sandstone {},
            count: 64,
            nbt: None,
        });
    }
    for args in [&[][..], &["free"][..], &["plane"][..]] {
        assert!(plot.handle_redstone_tools_command(0, "/wire", args));
        assert!(plot.players[0].redstone_tools.wire_disabled);
        assert!(plot.players[0].redstone_tools.wire.is_none());
        assert!(!plot.wire_tools_active());
    }
}

#[test]
fn clicking_existing_dust_preserves_its_support_and_copies_the_support_color() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let start = BlockPos::new(32, 21, 35);
    let end = BlockPos::new(37, 21, 35);
    let support = Block::Wool {
        color: mchprs_blocks::BlockColorVariant::Blue,
    };
    let old_wire = Block::RedstoneWire {
        wire: RedstoneWire::default(),
    };
    plot.world
        .set_block(start.offset(BlockFace::Bottom), support);
    plot.world.set_block(start, old_wire);
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 35.5);
    plot.players[0].yaw = 0.0;
    plot.players[0].pitch = 90.0;
    assert!(plot.start_wire_route(0, start));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(start)
    );
    assert_eq!(plot.world.get_block(start), old_wire);
    assert_eq!(
        plot.world.get_block(start.offset(BlockFace::Bottom)),
        support
    );
    assert!(plot.players[0].worldedit_undo.is_empty());
    plot.players[0].pos.x = f64::from(end.x) + 0.5;
    wait_for_preview(&mut plot);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    assert_eq!(session.target, Some(end));
    assert!(session
        .plan
        .as_ref()
        .unwrap()
        .placements
        .iter()
        .all(|&(_, block)| block == support || matches!(block, Block::RedstoneWire { .. })));
    session.last_click = None;
    assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
    for x in start.x..=end.x {
        assert_eq!(plot.world.get_block(BlockPos::new(x, 20, 35)), support);
        assert!(matches!(
            plot.world.get_block(BlockPos::new(x, 21, 35)),
            Block::RedstoneWire { .. }
        ));
    }
    assert!(worldedit::execute_command(
        &mut plot,
        0,
        "/undo",
        &mut Vec::new()
    ));
    assert_eq!(plot.world.get_block(start), old_wire);
    assert_eq!(
        plot.world.get_block(start.offset(BlockFace::Bottom)),
        support
    );
    for x in start.x + 1..=end.x {
        assert_eq!(
            plot.world.get_block(BlockPos::new(x, 20, 35)),
            Block::Air {}
        );
        assert_eq!(
            plot.world.get_block(BlockPos::new(x, 21, 35)),
            Block::Air {}
        );
    }
}

#[test]
fn left_click_selects_support_and_right_click_without_a_start_does_not() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let support = BlockPos::new(32, 20, 35);
    plot.world.set_block(support, Block::Stone {});
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 35.5);

    plot.handle_player_digging(
        SPlayerDigging {
            status: 0,
            pos: support.packed(),
            face: 1,
            sequence: 0,
        },
        0,
    );
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(support.offset(BlockFace::Top))
    );
    assert_eq!(plot.world.get_block(support), Block::Stone {});
    assert!(plot.players[0].worldedit_undo.is_empty());

    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let wire = support.offset(BlockFace::Top);
    plot.world.set_block(support, Block::Stone {});
    plot.world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 35.5);
    plot.handle_player_digging(
        SPlayerDigging {
            status: 0,
            pos: wire.packed(),
            face: 1,
            sequence: 0,
        },
        0,
    );
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(wire)
    );
    assert_eq!(plot.world.get_block(support), Block::Stone {});
    assert!(matches!(
        plot.world.get_block(wire),
        Block::RedstoneWire { .. }
    ));

    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    plot.world.set_block(support, Block::Stone {});
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 35.5);
    assert!(plot.use_wire_tool(0, 0, 0.0, 90.0));
    assert!(plot.players[0]
        .redstone_tools
        .wire
        .as_ref()
        .unwrap()
        .start
        .is_none());
}

#[test]
fn noncreative_pen_use_and_plane_switch_do_not_activate_or_edit() {
    let (mut plot, _peer) = fixture(false);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: tool_item(),
        count: 1,
        nbt: None,
    });
    let platform = BlockPos::new(32, 20, 35);
    plot.world.set_block(platform, Block::Stone {});
    for gamemode in [Gamemode::Adventure, Gamemode::Spectator] {
        plot.players[0].gamemode = gamemode;
        assert!(plot.start_wire_route(0, platform));
        assert!(plot.flip_wire_route(0, false));
        assert!(plot.flip_wire_route(0, true));
        assert!(plot.players[0].redstone_tools.wire.is_none());
        assert!(plot.players[0].worldedit_undo.is_empty());
        assert_eq!(plot.world.get_block(platform), Block::Stone {});
        assert_eq!(
            plot.world.get_block(platform.offset(BlockFace::Top)),
            Block::Air {}
        );
    }
}

#[test]
fn denied_pen_use_and_plane_switch_do_not_activate_or_edit() {
    let (mut plot, _peer) = fixture(false);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: tool_item(),
        count: 1,
        nbt: None,
    });
    plot.players[0].deny_test_permissions();
    let platform = BlockPos::new(32, 20, 35);
    plot.world.set_block(platform, Block::Stone {});
    assert!(plot.start_wire_route(0, platform));
    assert!(plot.flip_wire_route(0, false));
    assert!(plot.flip_wire_route(0, true));
    assert!(plot.players[0].redstone_tools.wire.is_none());
    assert!(plot.players[0].worldedit_undo.is_empty());
    assert_eq!(plot.world.get_block(platform), Block::Stone {});
    assert_eq!(
        plot.world.get_block(platform.offset(BlockFace::Top)),
        Block::Air {}
    );
}

#[test]
fn cursor_skips_space_above_thin_dust_but_can_select_the_dust_itself() {
    let (mut plot, _peer) = fixture(false);
    let wire = BlockPos::new(32, 22, 36);
    let farther_platform = BlockPos::new(32, 22, 40);
    plot.world
        .set_block(wire.offset(BlockFace::Bottom), Block::Glass {});
    plot.world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    plot.world.set_block(farther_platform, Block::Stone {});
    plot.players[0].pos = PlayerPos::new(32.5, 21.0, 35.5);
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            0.0,
            0.0,
            None,
            Plane::Horizontal
        ),
        Some(farther_platform.offset(BlockFace::Top)),
    );
    plot.players[0].pos.y = 20.4;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            0.0,
            0.0,
            None,
            Plane::Horizontal
        ),
        Some(wire),
    );
}

#[test]
fn live_elevated_route_copies_stone_supports_and_undo_restores_both_platforms() {
    let (mut plot, _peer) = fixture(false);
    equip(&mut plot);
    let lower_platform = BlockPos::new(32, 20, 35);
    let upper_platform = BlockPos::new(36, 23, 35);
    for pos in [lower_platform, upper_platform] {
        plot.world.set_block(pos, Block::Stone {});
    }
    plot.players[0].pos = PlayerPos::new(32.5, 24.0, 31.5);
    plot.players[0].yaw = 0.0;
    plot.players[0].pitch = (4.62_f64 / 4.0).atan().to_degrees() as f32;
    let start = lower_platform.offset(BlockFace::Top);
    let end = upper_platform.offset(BlockFace::Top);
    assert!(plot.start_wire_route(0, lower_platform));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().start,
        Some(start)
    );
    assert!(plot.flip_wire_route(0, false));
    assert_eq!(
        plot.players[0].redstone_tools.wire.as_ref().unwrap().plane,
        Plane::VerticalX
    );
    plot.players[0].yaw = -45.0;
    plot.players[0].pitch = (1.62_f64 / (4.0_f64 * 4.0 * 2.0).sqrt())
        .atan()
        .to_degrees() as f32;
    assert_eq!(
        aim(
            &plot.world,
            &plot.players[0],
            plot.players[0].yaw,
            plot.players[0].pitch,
            Some(start),
            Plane::VerticalX,
        ),
        Some(end)
    );
    wait_for_preview(&mut plot);
    let session = plot.players[0].redstone_tools.wire.as_mut().unwrap();
    let plan = session.plan.as_ref().unwrap();
    let path = plan.path.clone();
    let placements = plan.placements.clone();
    assert!(placements
        .iter()
        .any(|(_, block)| matches!(block, Block::Stone {})));
    assert!(placements
        .iter()
        .all(|(_, block)| matches!(block, Block::Stone {} | Block::RedstoneWire { .. })));
    assert!(path.windows(2).any(|pair| pair[0].y != pair[1].y));
    session.last_click = None;
    let (yaw, pitch) = (plot.players[0].yaw, plot.players[0].pitch);
    assert!(plot.use_wire_tool(0, 0, yaw, pitch));
    assert_eq!(plot.players[0].worldedit_undo.len(), 1);
    for &(pos, block) in &placements {
        if matches!(block, Block::RedstoneWire { .. }) {
            assert!(matches!(
                plot.world.get_block(pos),
                Block::RedstoneWire { .. }
            ));
            assert!(crate::interaction::is_valid_position(
                plot.world.get_block(pos),
                &plot.world,
                pos
            ));
        } else {
            assert_eq!(plot.world.get_block(pos), block);
        }
    }
    for pair in path.windows(2).filter(|pair| pair[0].y != pair[1].y) {
        let higher = if pair[0].y > pair[1].y {
            pair[0]
        } else {
            pair[1]
        };
        let support = higher.offset(BlockFace::Bottom);
        if !placements.iter().any(|&(pos, _)| pos == support) {
            continue;
        }
        assert_eq!(plot.world.get_block(support), Block::Stone {});
    }
    assert_eq!(plot.world.get_block(lower_platform), Block::Stone {});
    assert_eq!(plot.world.get_block(upper_platform), Block::Stone {});
    assert!(worldedit::execute_command(
        &mut plot,
        0,
        "/undo",
        &mut Vec::new()
    ));
    for &(pos, _) in &placements {
        assert_eq!(plot.world.get_block(pos), Block::Air {});
    }
    assert_eq!(plot.world.get_block(lower_platform), Block::Stone {});
    assert_eq!(plot.world.get_block(upper_platform), Block::Stone {});
    assert_eq!(plot.players[0].worldedit_redo.len(), 1);
}
