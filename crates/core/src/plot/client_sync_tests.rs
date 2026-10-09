use super::*;
use crate::plot::client_test_utils::{decode_blocks, read_ack, read_blocks};
use mchprs_blocks::blocks::{
    Instrument, Lever, LeverFace, RedstonePiston, RedstonePistonHead, RedstoneWire,
    RedstoneWireSide,
};
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockDirection, BlockFacing};
use mchprs_network::packets::serverbound::*;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::{connection, read_frame};
use std::net::TcpStream;

pub(super) fn fixture(compressed: bool) -> (Plot, TcpStream) {
    let conn = connection(compressed).unwrap();
    let mut player = Player::test_player(conn.player);
    player.pos = PlayerPos::new(32.5, 21.0, 35.5);
    player.last_chunk_x = 2;
    player.last_chunk_z = 2;
    player.inventory[36] = Some(ItemStack {
        item_type: Item::Sandstone {},
        count: 64,
        nbt: None,
    });
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    );
    world.set_block(BlockPos::new(32, 20, 32), Block::Sandstone {});
    world.set_screen_only(true);
    world
        .packet_senders
        .push(PlayerPacketSender::new(&player.client));
    let (message_sender, _) = std::sync::mpsc::channel();
    let (_, priv_message_receiver) = std::sync::mpsc::channel();
    let plot = Plot {
        transient_test_fixture: true,
        world,
        players: vec![player],
        redpiler: Default::default(),
        message_receiver: bus::Bus::new(4).add_rx(),
        message_sender,
        priv_message_receiver,
        locked_players: Default::default(),
        tps: Tps::Limited(0),
        world_send_rate: WorldSendRate(20),
        piston_animation: Default::default(),
        last_update_time: Instant::now(),
        lag_time: Duration::ZERO,
        last_nspt: None,
        timings: TimingsMonitor::new(Tps::Limited(0)),
        last_player_time: Instant::now(),
        last_world_send_time: Instant::now(),
        sleep_time: Duration::ZERO,
        running: false,
        always_running: false,
        auto_redpiler: false,
        owner: Some(1),
        async_rt: Plot::create_async_rt(),
        scoreboard: Default::default(),
        last_sidebar_update: Instant::now(),
        neighbor_views: Default::default(),
        neighbor_source: None,
        git: Default::default(),
        wire_cursor: 0,
    };
    (plot, conn.peer)
}

fn placement(pos: BlockPos, sequence: i32) -> SPlayerBlockPlacemnt {
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

#[test]
fn wand_right_click_accepts_block_edges_and_extended_shapes_without_placing() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = fixture(compressed);
        let support = BlockPos::new(32, 20, 32);
        plot.players[0].inventory[36] = Some(ItemStack {
            item_type: Item::from_name("wooden_axe").unwrap(),
            count: 1,
            nbt: None,
        });
        plot.handle_player_digging(digging(support, 0, 1), 0);
        episode(&mut peer, compressed, 1);
        assert_eq!(plot.players[0].first_position, Some(support));

        for (index, cursor) in [f32::NAN, f32::INFINITY, 2.0].into_iter().enumerate() {
            let sequence = index as i32 + 2;
            let mut click = placement(support, sequence);
            click.cursor_x = cursor;
            plot.handle_player_block_placement(click, 0);
            episode(&mut peer, compressed, sequence);
            assert_eq!(plot.players[0].second_position, None);
        }
        let mut click = placement(support, 5);
        click.cursor_x = -f32::EPSILON;
        click.cursor_z = 1.0 + f32::EPSILON;
        plot.handle_player_block_placement(click, 0);
        episode(&mut peer, compressed, 5);
        assert_eq!(plot.players[0].second_position, Some(support));
        assert_eq!(plot.world.get_block(support), Block::Sandstone {});
        assert_eq!(
            plot.world.get_block(support.offset(BlockFace::Top)),
            Block::Air
        );

        let fence = support + BlockPos::new(1, 0, 0);
        let block = Block::from_name("oak_fence").unwrap();
        plot.world.set_block(fence, block);
        let mut click = placement(fence, 6);
        click.cursor_y = 1.25;
        plot.handle_player_block_placement(click, 0);
        episode(&mut peer, compressed, 6);
        assert_eq!(plot.players[0].second_position, Some(fence));
        assert_eq!(plot.world.get_block(fence), block);
    }
}

#[test]
fn both_wand_positions_need_selection_permission_without_plot_edit_access() {
    for allowed in [false, true] {
        let (mut plot, mut peer) = fixture(false);
        plot.owner = None;
        plot.players[0].set_test_permissions(if allowed {
            &["worldedit.selection.pos"]
        } else {
            &[]
        });
        assert!(!plot.players[0].can_edit_plot(plot.owner, (0, 0)));
        let support = BlockPos::new(32, 20, 32);
        plot.players[0].inventory[36] = Some(ItemStack {
            item_type: Item::from_name("wooden_axe").unwrap(),
            count: 1,
            nbt: None,
        });
        plot.handle_player_digging(digging(support, 0, 1), 0);
        episode(&mut peer, false, 1);
        plot.handle_player_block_placement(placement(support, 2), 0);
        episode(&mut peer, false, 2);
        let expected = allowed.then_some(support);
        assert_eq!(plot.players[0].first_position, expected);
        assert_eq!(plot.players[0].second_position, expected);
        assert_eq!(plot.world.get_block(support), Block::Sandstone {});
        assert_eq!(
            plot.world.get_block(support.offset(BlockFace::Top)),
            Block::Air
        );
    }
}

#[test]
fn switching_from_git_sword_does_not_swallow_wand_right_click() {
    let (mut plot, mut peer) = fixture(false);
    let support = BlockPos::new(32, 20, 32);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: Item::from_name("wooden_sword").unwrap(),
        count: 1,
        nbt: None,
    });
    plot.handle_use_item(
        SUseItem {
            hand: 0,
            sequence: 1,
            yaw: 0.0,
            pitch: 0.0,
        },
        0,
    );
    episode(&mut peer, false, 1);
    plot.players[0].inventory[36] = Some(ItemStack {
        item_type: Item::WEWand {},
        count: 1,
        nbt: None,
    });
    plot.handle_player_block_placement(placement(support, 2), 0);
    episode(&mut peer, false, 2);
    assert_eq!(plot.players[0].second_position, Some(support));
    assert_eq!(plot.world.get_block(support), Block::Sandstone {});
}

#[test]
fn compass_right_click_works_in_air_and_on_blocks_with_either_hand() {
    for compressed in [false, true] {
        for hand in [0, 1] {
            for on_block in [false, true] {
                let (mut plot, mut peer) = fixture(compressed);
                let target = BlockPos::new(32, 22, 39);
                plot.world.set_block(target, Block::Stone {});
                plot.players[0].inventory[if hand == 0 { 36 } else { 45 }] = Some(ItemStack {
                    item_type: Item::Compass,
                    count: 1,
                    nbt: None,
                });
                if on_block {
                    let mut click = placement(target, 1);
                    click.hand = hand;
                    plot.handle_player_block_placement(click, 0);
                } else {
                    plot.handle_use_item(
                        SUseItem {
                            hand,
                            sequence: 1,
                            yaw: 0.0,
                            pitch: 0.0,
                        },
                        0,
                    );
                }
                let (ids, _) = episode(&mut peer, compressed, 1);
                assert_eq!(ids.iter().filter(|&&id| id == 0x41).count(), 1);
                let destination = plot.players[0].pos;
                assert_eq!(
                    (destination.x, destination.y, destination.z),
                    (32.5, 23.0, 39.5)
                );
                plot.handle_use_item(
                    SUseItem {
                        hand,
                        sequence: 2,
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    0,
                );
                let (ids, _) = episode(&mut peer, compressed, 2);
                assert!(!ids.contains(&0x41), "fallback packet teleported twice");
                assert_eq!(plot.players[0].pos.y, destination.y);
                assert_eq!(plot.world.get_block(target), Block::Stone {});
                assert_eq!(
                    plot.world.get_block(target.offset(BlockFace::Top)),
                    Block::Air
                );
            }
        }
    }
}

#[test]
fn wire_input_slices_retain_unfinished_limited_tps_tick_debt() {
    let (mut plot, _peer) = fixture(false);
    // The common packet fixture drops its inbound sender; a full update needs it alive.
    let connection = connection(false).unwrap();
    let _incoming = connection.incoming;
    let _live_peer = connection.peer;
    plot.players[0].client = connection.player;
    plot.world.packet_senders.clear();
    plot.world
        .packet_senders
        .push(PlayerPacketSender::new(&plot.players[0].client));
    assert!(plot.handle_redstone_tools_command(0, "/wire", &[]));
    assert!(plot.wire_tools_active());
    plot.tps = Tps::Limited(1_000_000);
    plot.lag_time = Duration::from_secs(1);
    plot.last_nspt = Some(Duration::from_nanos(100));
    for _ in 0..2 {
        let previous_time = plot.last_update_time;
        let previous_debt = plot.lag_time;
        let previous_ticks = plot.world.update_stats.simulated_ticks;
        plot.update();
        let completed = plot.world.update_stats.simulated_ticks - previous_ticks;
        assert!((1..=50_000).contains(&completed));
        assert_eq!(
            plot.lag_time,
            previous_debt + (plot.last_update_time - previous_time)
                - Duration::from_micros(completed)
        );
        assert!(plot.lag_time >= Duration::from_millis(900));
    }
}

#[test]
fn warp_restores_exact_position_and_facing_and_transfers_out_of_locked_plots() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = fixture(compressed);
        let (sender, receiver) = std::sync::mpsc::channel();
        plot.message_sender = sender;
        let warp = database::Warp {
            pos: PlayerPos::new(48.125, 22.75, 35.25),
            yaw: 123.5,
            pitch: -20.25,
        };
        assert!(!plot.teleport_to_warp(0, warp));
        let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
        assert_eq!(id, 0x41);
        assert!(frame.read_varint().unwrap() > 0);
        assert_eq!(frame.read_double().unwrap(), warp.pos.x);
        assert_eq!(frame.read_double().unwrap(), warp.pos.y);
        assert_eq!(frame.read_double().unwrap(), warp.pos.z);
        for _ in 0..3 {
            assert_eq!(frame.read_double().unwrap(), 0.0);
        }
        assert_eq!(frame.read_float().unwrap(), warp.yaw);
        assert_eq!(frame.read_float().unwrap(), warp.pitch);
        assert!(plot.players[0].awaiting_teleport());
        assert!(receiver.try_recv().is_err());

        assert!(!plot.teleport_to_warp(
            0,
            database::Warp {
                yaw: f32::NAN,
                ..warp
            }
        ));
        assert_eq!(plot.players[0].yaw, warp.yaw);
        assert_eq!(plot.players[0].pos.x, warp.pos.x);

        plot.locked_players.insert(plot.players[0].entity_id);
        let remote = database::Warp {
            pos: PlayerPos::new(-512.125, 64.25, 1024.5),
            ..warp
        };
        assert!(plot.teleport_to_warp(0, remote));
        assert!(plot.players.is_empty());
        assert!(plot.locked_players.is_empty());
        let Message::PlayerLeavePlot(player) = receiver.try_recv().unwrap() else {
            panic!("warp must transfer through the normal plot routing");
        };
        assert_eq!(player.pos.plot_pos(), (-3, 4));
        assert_eq!(
            (player.pos.x, player.pos.y, player.pos.z),
            (remote.pos.x, remote.pos.y, remote.pos.z)
        );
        assert_eq!((player.yaw, player.pitch), (remote.yaw, remote.pitch));
    }
}

#[test]
fn redpiler_flag_suggestions_reach_the_chat_client() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = fixture(compressed);
        plot.players[0].set_test_permissions(&["mchprs.access.commands"]);
        let text = "/rp c --optimize --ass";
        plot.handle_tab_complete(
            STabComplete {
                transaction_id: 42,
                text: text.into(),
            },
            0,
        );
        let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
        assert_eq!(id, 0x0f);
        assert_eq!(frame.read_varint().unwrap(), 42);
        assert_eq!(
            frame.read_varint().unwrap(),
            text.rfind(' ').unwrap() as i32 + 1
        );
        assert_eq!(frame.read_varint().unwrap(), 5);
        assert_eq!(frame.read_varint().unwrap(), 1);
        assert_eq!(frame.read_string().unwrap(), "--assume-instant");
        assert!(!frame.read_bool().unwrap());
    }
}

fn digging(pos: BlockPos, status: i32, sequence: i32) -> SPlayerDigging {
    SPlayerDigging {
        status,
        pos: pos.packed(),
        face: 1,
        sequence,
    }
}

/// Read through the ACK, retaining all authoritative block updates and packet
/// order. A missing correction/ACK fails within the socket's bounded timeout.
fn episode(
    peer: &mut TcpStream,
    compressed: bool,
    sequence: i32,
) -> (Vec<i32>, Vec<(BlockPos, u32)>) {
    let mut ids = Vec::new();
    let mut blocks = Vec::new();
    for _ in 0..128 {
        let (id, mut frame) = read_frame(peer, compressed).unwrap();
        ids.push(id);
        match id {
            0x04 => {
                assert_eq!(frame.read_varint().unwrap(), sequence);
                return (ids, blocks);
            }
            0x08 | 0x4d => blocks.extend(decode_blocks(id, &mut frame)),
            _ => {}
        }
    }
    panic!("action acknowledgement did not arrive within 128 packets");
}

fn compiled_bud_fixture(
    compressed: bool,
    io_only: bool,
) -> (Plot, TcpStream, BlockPos, BlockPos, BlockPos, BlockPos) {
    let (mut plot, peer) = fixture(compressed);
    let senders = std::mem::take(&mut plot.world.packet_senders);
    plot.world.set_screen_only(false);
    // Base and head straddle a section boundary to exercise packet collection.
    let cell = BlockPos::new(47, 32, 40);
    let piston = RedstonePiston {
        facing: BlockFacing::Down,
        sticky: true,
        extended: true,
    };
    plot.world.set_block(cell, Block::Piston { piston });
    plot.world.set_block(
        cell.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: RedstonePistonHead::from(piston),
        },
    );
    plot.world
        .set_block(cell + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
    let data_wire = cell + BlockPos::new(0, 3, 0);
    plot.world
        .set_block(data_wire.offset(BlockFace::Bottom), Block::Stone {});
    plot.world.set_block(
        data_wire,
        Block::RedstoneWire {
            wire: RedstoneWire {
                north: RedstoneWireSide::None,
                south: RedstoneWireSide::None,
                east: RedstoneWireSide::Side,
                west: RedstoneWireSide::Side,
                power: 0,
            },
        },
    );
    let data = data_wire.offset(BlockFace::East);
    let generator = cell + BlockPos::new(2, 0, 0);
    plot.world.set_block(
        generator,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::West,
                sticky: false,
                extended: false,
            },
        },
    );
    let sample = generator.offset(BlockFace::South);
    let note = cell + BlockPos::new(0, -3, 0);
    plot.world.set_block(
        note,
        Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 0,
            powered: true,
        },
    );
    let note_control = note.offset(BlockFace::East);
    for pos in [data, sample, note_control] {
        plot.world
            .set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        plot.world.set_block(
            pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
    }
    plot.redpiler
        .compile(
            &plot.world,
            plot.world.get_corners(),
            CompilerOptions {
                optimize: true,
                io_only,
                ..Default::default()
            },
            vec![],
            Default::default(),
        )
        .unwrap();
    plot.redpiler.flush(&mut plot.world);
    plot.world.flush_block_changes();
    plot.world.sounds.clear();
    plot.world.packet_senders = senders;
    (plot, peer, cell, data, sample, note_control)
}

#[test]
fn paused_advance_delivers_committed_bud_geometry_and_preserves_suppression() {
    for compressed in [false, true] {
        for io_only in [false, true] {
            for screen_only in [false, true] {
                let (mut plot, mut peer, cell, data, sample, _) =
                    compiled_bud_fixture(compressed, io_only);
                plot.world.set_screen_only(screen_only);
                let near = cell.offset(BlockFace::Bottom);
                let far = near.offset(BlockFace::Bottom);
                let old = [cell, near, far].map(|pos| (pos, plot.world.get_block_raw(pos)));
                plot.redpiler.on_use_block(sample);
                assert_eq!(plot.world.get_block_raw(cell), old[0].1);
                assert!(!plot.handle_command(0, "/adv", vec!["1"]));
                drop(mchprs_network::BlockActionAcknowledgement::new(
                    &plot.players[0].client,
                    1,
                ));
                let (_, blocks) = episode(&mut peer, compressed, 1);
                for pos in [cell, near, far] {
                    assert_eq!(
                        blocks.iter().any(|&(changed, _)| changed == pos),
                        !io_only && !screen_only,
                    );
                }
                if !io_only {
                    assert!(
                        matches!(plot.world.get_block(cell), Block::Piston { piston } if !piston.extended)
                    );
                    assert_eq!(plot.world.get_block(near), Block::RedstoneBlock);
                    assert_eq!(plot.world.get_block(far), Block::Air);
                    if screen_only {
                        plot.world.set_screen_only(false);
                        drop(mchprs_network::BlockActionAcknowledgement::new(
                            &plot.players[0].client,
                            2,
                        ));
                        let (_, blocks) = episode(&mut peer, compressed, 2);
                        for pos in [cell, near, far] {
                            assert!(blocks.contains(&(pos, plot.world.get_block_raw(pos))));
                        }
                    }
                } else {
                    assert_eq!(
                        [cell, near, far].map(|pos| (pos, plot.world.get_block_raw(pos))),
                        old,
                    );
                }
                plot.redpiler.on_use_block(data);
                plot.redpiler.on_use_block(sample);
                plot.handle_command(0, "/adv", vec!["1"]);
                drop(mchprs_network::BlockActionAcknowledgement::new(
                    &plot.players[0].client,
                    3,
                ));
                let (_, blocks) = episode(&mut peer, compressed, 3);
                if !io_only {
                    for &(pos, state) in &old {
                        assert!(blocks.contains(&(pos, state)));
                        assert_eq!(plot.world.get_block_raw(pos), state);
                    }
                }
            }
        }
    }
}

#[test]
fn bud_note_obstruction_uses_committed_memory_with_deferred_or_suppressed_display() {
    for io_only in [false, true] {
        for frequent in [false, true] {
            let (mut plot, _peer, cell, data, sample, note_control) =
                compiled_bud_fixture(false, io_only);
            let ready = |plot: &mut Plot| {
                // Complete the piston movement and reset before the next input.
                // Pure ticks keep note events deferred until an explicit flush.
                for _ in 0..6 {
                    plot.redpiler.tick();
                    if frequent {
                        plot.redpiler.flush(&mut plot.world);
                    }
                }
            };
            plot.redpiler.on_use_block(sample);
            ready(&mut plot);
            let far = cell + BlockPos::new(0, -2, 0);
            assert_eq!(
                plot.world.get_block(far),
                if frequent && !io_only {
                    Block::Air
                } else {
                    Block::RedstoneBlock
                },
                "deferred or suppressed geometry must not decide note eligibility",
            );
            plot.redpiler.on_use_block(note_control);
            if frequent {
                plot.redpiler.flush(&mut plot.world);
                assert_eq!(plot.world.sounds.len(), 1, "committed far cell is empty");
            } else {
                assert!(plot.world.sounds.is_empty(), "note playback stays deferred");
            }
            ready(&mut plot);
            plot.redpiler.on_use_block(note_control);
            ready(&mut plot);
            plot.redpiler.on_use_block(data);
            // The new QC data must settle before the separate update samples it.
            ready(&mut plot);
            plot.redpiler.on_use_block(sample);
            ready(&mut plot);
            plot.redpiler.flush(&mut plot.world);
            assert_eq!(
                plot.world.sounds.len(),
                1,
                "only the rise while the committed far cell was empty can play",
            );
            assert_eq!(plot.world.get_block(far), Block::RedstoneBlock);
            plot.redpiler.flush(&mut plot.world);
            assert_eq!(
                plot.world.sounds.len(),
                1,
                "a flush must not replay the note"
            );
        }
    }
}

#[test]
fn actual_placement_and_break_handlers_publish_edits_in_screen_mode() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = fixture(compressed);
        let viewer = connection(compressed).unwrap();
        let mut viewer_peer = viewer.peer;
        let mut viewer_player = Player::test_player(viewer.player);
        viewer_player.uuid = 2;
        plot.world
            .packet_senders
            .push(PlayerPacketSender::new(&viewer_player.client));
        plot.players.push(viewer_player);
        let support = BlockPos::new(32, 20, 32);
        let placed = support.offset(BlockFace::Top);
        plot.handle_player_block_placement(placement(support, 1), 0);
        assert_eq!(plot.world.get_block(placed), Block::Sandstone {});
        let (ids, blocks) = episode(&mut peer, compressed, 1);
        assert!(
            ids.contains(&0x4d),
            "other viewers also need the placement delta"
        );
        assert!(blocks.contains(&(placed, Block::Sandstone {}.get_id())));
        assert_eq!(ids.last(), Some(&0x04));
        assert_eq!(
            read_blocks(&mut viewer_peer, compressed),
            [(placed, Block::Sandstone {}.get_id())]
        );

        plot.handle_player_digging(digging(placed, 0, 2), 0);
        assert_eq!(plot.world.get_block(placed), Block::Air);
        let (ids, blocks) = episode(&mut peer, compressed, 2);
        assert!(ids.contains(&0x4d));
        assert!(blocks.contains(&(placed, Block::Air.get_id())));
        assert_eq!(
            read_blocks(&mut viewer_peer, compressed),
            [(placed, Block::Air.get_id())]
        );
        assert!(plot.world.screen_only());
    }
}

#[test]
fn swords_inspect_without_a_diff_and_never_break_blocks() {
    for compressed in [false, true] {
        for hand in [0, 1] {
            let (mut plot, mut peer) = fixture(compressed);
            let support = BlockPos::new(32, 20, 32);
            for (index, material) in ["wooden", "stone", "iron", "golden", "diamond", "netherite"]
                .into_iter()
                .enumerate()
            {
                plot.players[0].inventory[36] = Some(ItemStack {
                    item_type: Item::from_name(&format!("{material}_sword")).unwrap(),
                    count: 1,
                    nbt: None,
                });
                let sequence = index as i32 + 1;
                plot.handle_player_digging(digging(support, 0, sequence), 0);
                assert_eq!(plot.world.get_block(support), Block::Sandstone {});
                let (ids, blocks) = episode(&mut peer, compressed, sequence);
                assert_eq!(ids, [0x08, 0x04]);
                assert_eq!(blocks, [(support, Block::Sandstone {}.get_id())]);
            }
            if hand == 1 {
                plot.players[0].inventory[45] = plot.players[0].inventory[36].take();
            }
            // Aim into empty space: inspect must report its own target error,
            // without requiring a diff or starting a repository worker.
            plot.handle_use_item(
                SUseItem {
                    hand,
                    sequence: 7,
                    yaw: 0.0,
                    pitch: 0.0,
                },
                0,
            );
            let (id, frame) = read_frame(&mut peer, compressed).unwrap();
            assert_eq!(id, 0x72);
            let payload = &frame.get_ref()[frame.position() as usize..];
            let mut named = vec![payload[0], 0, 0];
            named.extend_from_slice(&payload[1..]);
            let component = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).unwrap();
            let text = mchprs_network::text::to_json(&nbt::Value::Compound(component.content));
            assert!(text.contains(messages::GIT_INSPECT_TARGET_REQUIRED));
            read_ack(&mut peer, compressed, 7);
            // A duplicate use-on-block packet must not interact with the block.
            let mut duplicate = placement(support, 8);
            duplicate.hand = hand;
            plot.handle_player_block_placement(duplicate, 0);
            let (ids, _) = episode(&mut peer, compressed, 8);
            assert!(!ids.contains(&0x72));
            let mut fallback = placement(support, 9);
            fallback.hand = 1 - hand;
            plot.handle_player_block_placement(fallback, 0);
            let (ids, _) = episode(&mut peer, compressed, 9);
            assert!(!ids.contains(&0x72));
            assert_eq!(plot.world.get_block(support), Block::Sandstone {});
            assert_eq!(
                plot.world.get_block(support.offset(BlockFace::Top)),
                Block::Air
            );
        }
    }
}

#[test]
fn rejected_placement_and_dig_abort_finish_correct_prediction_before_ack() {
    for compressed in [false, true] {
        let (mut plot, mut peer) = fixture(compressed);
        let support = BlockPos::new(32, 20, 32);
        let mut invalid = placement(support, 1);
        invalid.cursor_x = f32::NAN;
        plot.handle_player_block_placement(invalid, 0);
        let (ids, blocks) = episode(&mut peer, compressed, 1);
        assert_eq!(ids, [0x08, 0x08, 0x04]);
        assert_eq!(
            blocks,
            [
                (support, Block::Sandstone {}.get_id()),
                (support.offset(BlockFace::Top), Block::Air.get_id()),
            ]
        );
        for status in [1, 2] {
            plot.handle_player_digging(digging(support, status, status + 1), 0);
            let (ids, blocks) = episode(&mut peer, compressed, status + 1);
            assert_eq!(ids, [0x08, 0x04]);
            assert_eq!(blocks, [(support, Block::Sandstone {}.get_id())]);
            assert_eq!(plot.world.get_block(support), Block::Sandstone {});
        }
    }
}

#[test]
fn void_recovery_fences_stale_movement_and_preserves_coordinate_validation() {
    for compressed in [false, true] {
        for y in [-2047.9898847094473, -0.1] {
            let (mut plot, mut peer) = fixture(compressed);
            let original = PlayerPos::new(32.5, y, 35.5);
            plot.players[0].teleport(original);
            let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
            assert_eq!(id, 0x41);
            let old_id = frame.read_varint().unwrap();

            plot.players[0].update();
            let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
            assert_eq!(id, 0x41);
            let rescue_id = frame.read_varint().unwrap();
            assert_ne!(rescue_id, old_id);
            assert_eq!(
                [
                    frame.read_double().unwrap(),
                    frame.read_double().unwrap(),
                    frame.read_double().unwrap(),
                ],
                [original.x, 128.0, original.z]
            );
            plot.handle_teleport_confirm(STeleportConfirm { id: old_id }, 0);
            assert!(plot.players[0].awaiting_teleport());

            let falling = SPlayerPosition {
                x: original.x,
                y: -2048.22041207836,
                z: original.z,
                on_ground: false,
            };
            plot.handle_player_position(falling, 0);
            plot.handle_player_position_and_rotation(
                SPlayerPositionAndRotation {
                    x: original.x,
                    y: -2048.22041207836,
                    z: original.z,
                    yaw: 45.0,
                    pitch: 10.0,
                    on_ground: false,
                },
                0,
            );
            assert_eq!(plot.players[0].pos.y, 128.0);
            assert!(plot.players[0].client.alive());
            drop(mchprs_network::BlockActionAcknowledgement::new(
                &plot.players[0].client,
                1,
            ));
            read_ack(&mut peer, compressed, 1);

            plot.handle_teleport_confirm(STeleportConfirm { id: rescue_id }, 0);
            assert!(!plot.players[0].awaiting_teleport());
            plot.handle_player_position(
                SPlayerPosition {
                    x: original.x,
                    y: -2048.22041207836,
                    z: original.z,
                    on_ground: false,
                },
                0,
            );
            assert!(!plot.players[0].client.alive());
        }
    }
}

#[test]
fn teleport_fences_movement_and_edits_until_destination_view_is_queued() {
    let compressed = true;
    let (mut plot, mut peer) = fixture(compressed);
    let original = plot.players[0].pos;
    let destination = PlayerPos::new(48.5, 21.0, 35.5);
    plot.players[0].teleport(destination);
    let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
    assert_eq!(id, 0x41);
    let teleport_id = frame.read_varint().unwrap();
    plot.handle_player_position(
        SPlayerPosition {
            x: original.x,
            y: original.y,
            z: original.z,
            on_ground: true,
        },
        0,
    );
    assert_eq!(plot.players[0].pos.x, destination.x);
    let support = BlockPos::new(32, 20, 32);
    plot.handle_player_digging(digging(support, 0, 1), 0);
    assert_eq!(plot.world.get_block(support), Block::Sandstone {});
    let (ids, _) = episode(&mut peer, compressed, 1);
    assert_eq!(ids, [0x08, 0x04]);

    plot.handle_teleport_confirm(
        STeleportConfirm {
            id: teleport_id - 1,
        },
        0,
    );
    assert!(plot.players[0].awaiting_teleport());
    drop(mchprs_network::BlockActionAcknowledgement::new(
        &plot.players[0].client,
        2,
    ));
    read_ack(&mut peer, compressed, 2); // Wrong confirmation sends no view/chunk work.
    plot.handle_teleport_confirm(STeleportConfirm { id: teleport_id }, 0);
    assert!(!plot.players[0].awaiting_teleport());
    assert_eq!(
        (plot.players[0].last_chunk_x, plot.players[0].last_chunk_z),
        (3, 2)
    );
    // The next edit in the SAME drained packet batch must follow the chunk loads.
    let target = BlockPos::new(48, 20, 32);
    plot.handle_player_digging(digging(target, 2, 3), 0);
    let (ids, blocks) = episode(&mut peer, compressed, 3);
    assert_eq!(ids[0], 0x57); // Set Center Chunk in protocol 770.
    assert!(ids.contains(&0x27));
    assert_eq!(ids[ids.len() - 2..], [0x08, 0x04]);
    assert_eq!(blocks, [(target, Block::Air.get_id())]);
}
