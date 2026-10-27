use super::*;
use crate::plot::benchmark;
use mchprs_network::{
    PlayerPacketSender,
    packets::{
        PacketDecoderExt,
        clientbound::{CKeepAlive, ClientBoundPacket},
        serverbound::SKeepAlive,
    },
    test_support::{connection, read_frame},
};
use mchprs_save_data::plot_data::Tps;

const END: i64 = i64::MIN;

pub(super) fn run(
    world: PlotWorld,
    backend: GraphBackend,
    options: CompilerOptions,
    snapshot: std::path::PathBuf,
    manifest: serde_json::Value,
    inputs: Vec<(BlockPos, NodeId)>,
    neighbors: Vec<BlockPos>,
    lamps: Vec<BlockPos>,
    readouts: Vec<BlockPos>,
    clock: PistonId,
    frames: usize,
    compile_seconds: f64,
) {
    let positions = inputs.iter().map(|(position, _)| *position).collect::<Vec<_>>();
    let witness = known_command_witness_word(&manifest, &positions);
    let mask = (1u64 << positions.len()) - 1;
    let entry = positions.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        word | (u64::from(matches!(world.get_block(position), Block::Lever { lever } if lever.powered)) << bit)
    });
    let neighbor_nodes = backend
        .program
        .nodes()
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.position.filter(|position| !inside_fpu(*position)).map(|_| NodeId::new(index as u32)))
        .collect::<Vec<_>>();
    let lamp_nodes = lamps.iter().map(|position| backend.positions[position]).collect::<Vec<_>>();
    let readout_nodes = readouts.iter().map(|position| backend.positions[position]).collect::<Vec<_>>();
    let compiler = crate::redpiler::Compiler {
        backend: Some(Box::new(backend)),
        options,
        statistics: None,
    };
    let conn = connection(true).expect("create compressed benchmark client");
    conn.peer.set_read_timeout(Some(std::time::Duration::from_secs(600))).unwrap();
    let sender = PlayerPacketSender::new(&conn.player);
    let mut player = benchmark::player(conn.player);
    player.pos = crate::player::PlayerPos::new(-256.5, 86.0, -200.5);
    player.last_chunk_x = -17;
    player.last_chunk_z = -13;
    let mut plot = benchmark::with_client(world, player, Tps::Unlimited);
    plot.redpiler = compiler;
    let receiver = std::thread::spawn(move || {
        let mut peer = conn.peer;
        let mut chat = Vec::new();
        let mut packets = 0u64;
        loop {
            let (id, mut frame) = read_frame(&mut peer, true).expect("read benchmark client packet");
            packets += 1;
            match id {
                0x26 => {
                    let id = frame.read_long().expect("decode keepalive");
                    if id == END {
                        break;
                    }
                    conn.incoming.send(Box::new(SKeepAlive { id })).expect("send keepalive response");
                }
                0x72 => {
                    let bytes = &frame.get_ref()[frame.position() as usize..];
                    let component = match bytes.first() {
                        Some(10) => {
                            let mut named = vec![10, 0, 0];
                            named.extend_from_slice(&bytes[1..]);
                            let blob = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).expect("decode system chat NBT");
                            nbt::Value::Compound(blob.content)
                        }
                        Some(8) => {
                            let length = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
                            nbt::Value::String(std::str::from_utf8(&bytes[3..3 + length]).expect("decode chat string").to_owned())
                        }
                        _ => panic!("unsupported system chat root"),
                    };
                    chat.push(mchprs_network::text::to_json(&component));
                }
                _ => {}
            }
        }
        (packets, chat)
    });

    for (bit, &position) in positions.iter().enumerate() {
        if (entry ^ witness) & (1 << bit) != 0 {
            plot.redpiler.on_use_block(position);
        }
    }
    plot.redpiler.on_use_block(TRIGGER);
    let clock_at_entry = plot.redpiler.backend.as_ref().unwrap().state.pistons[clock.index()].revision;
    benchmark::run_ticks(&mut plot, 24);
    let clock_before = plot.redpiler.backend.as_ref().unwrap().state.pistons[clock.index()].revision;
    assert!(clock_before > clock_at_entry, "FPU clock must advance during warmup");
    let neighbor_before = neighbor_nodes
        .iter()
        .map(|node| plot.redpiler.backend.as_ref().unwrap().state.components[node.index()].clone())
        .collect::<Vec<_>>();
    let counts_before = plot.redpiler.graph_execution_counts().unwrap();
    benchmark::reset_timings(&mut plot);
    let mut random = 0x4d43_4850_5253_2026u64;
    let _ = next_profile_word(&mut random, mask, entry);
    let mut previous = witness;
    let mut observations = Vec::with_capacity(frames);
    let mut input_updates = 0;
    let mut neighbor_updates = 0;
    let mut neighbor_activity = false;
    let start = Instant::now();
    for frame in 0..frames {
        let (word, changed, neighbor_changed) = drive_profile_frame(&mut random, mask, previous, &positions, &neighbors, frame, |position| {
            plot.redpiler.on_use_block(position)
        });
        previous = word;
        input_updates += changed;
        neighbor_updates += neighbor_changed;
        benchmark::run_ticks(&mut plot, 6);
        let backend = plot.redpiler.backend.as_ref().unwrap();
        observations.push(observe_graph(backend, &lamp_nodes, &readout_nodes));
        neighbor_activity |= neighbor_nodes
            .iter()
            .zip(&neighbor_before)
            .any(|(node, initial)| backend.state.components[node.index()] != *initial);
    }
    let wall = start.elapsed();
    let counts = plot.redpiler.graph_execution_counts().unwrap();
    let clock_after = plot.redpiler.backend.as_ref().unwrap().state.pistons[clock.index()].revision;
    assert!(clock_after > clock_before, "FPU clock must advance during measurement");
    let phases = benchmark::timings(&plot);
    assert_eq!(counts.ticks - counts_before.ticks, (frames * 6) as u64);
    assert!(neighbor_activity, "neighboring Divider must execute work");
    let delivery_start = Instant::now();
    benchmark::publish(&mut plot);
    sender.send_packet(&CKeepAlive { id: END }.encode());
    let (packets, chat) = receiver.join().expect("benchmark client receiver");
    let delivery = delivery_start.elapsed();
    let network = sender.send_stats();
    assert_eq!(network.failures, 0, "client delivery failed");
    assert_eq!(network.queued_bytes, 0, "client did not drain output");

    // The native replay runs after the timed server loop.
    let mut native = saved_world_from(snapshot.clone());
    for (bit, &position) in positions.iter().enumerate() {
        if (entry ^ witness) & (1 << bit) != 0 {
            native_use_lever(&mut native, position);
        }
    }
    native_use_lever(&mut native, TRIGGER);
    for _ in 0..24 {
        native.tick_interpreted();
    }
    let warmup_commands = native.command_output().count();
    assert_eq!(warmup_commands, 8, "native command witness must execute");
    let mut random = 0x4d43_4850_5253_2026u64;
    let _ = next_profile_word(&mut random, mask, entry);
    let mut previous = witness;
    for (frame, expected) in observations.iter().enumerate() {
        let (word, _, _) = drive_profile_frame(&mut random, mask, previous, &positions, &neighbors, frame, |position| {
            native_use_lever(&mut native, position)
        });
        previous = word;
        for _ in 0..6 {
            native.tick_interpreted();
        }
        assert_eq!(*expected, observe(&native, &lamps, &readouts), "native output at frame {frame}");
    }
    let native_chat = native.command_output().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(chat, native_chat, "ordered client command messages must match the native replay");
    let unique_readouts = observations.iter().map(|(_, readout)| *readout).collect::<FxHashSet<_>>();
    assert!(unique_readouts.len() > 1 && unique_readouts.iter().any(|word| *word != 0));
    let report = serde_json::json!({
        "measurement_kind": "saved_plot_server_bounded_batches",
        "scope": "one compressed TCP client, actual Plot::update and Compiler tick/flush, bounded to six ticks per input frame, non-mutating Graph output observations each frame; login and initial chunks excluded",
        "snapshot": snapshot, "snapshot_sha256": "426d2395869062817fa698dca1b7a101c3594728cd440389970a4e6004f06d17",
        "manifest_sha256": "981109d849cadaf13a059c295a6d78444d0626a21c72683cf425eb6344b68227",
        "command_output_limits": "normal server limits enabled; native replay disables limits and retains its command queue", "compile_seconds": compile_seconds,
        "compiler_options": format!("{:?}", plot.redpiler.current_flags().unwrap()),
        "frames": frames, "game_ticks": frames * 6, "wall_seconds": wall.as_secs_f64(),
        "game_ticks_per_second": (frames * 6) as f64 / wall.as_secs_f64(),
        "final_delivery_seconds": delivery.as_secs_f64(),
        "including_final_delivery_tps": (frames * 6) as f64 / (wall + delivery).as_secs_f64(), "phases": phases,
        "input_updates": input_updates, "neighbor_updates": neighbor_updates, "clock_revision_delta": clock_after - clock_before,
        "client_packets": packets, "client_command_messages": chat.len(), "client_bytes": network.bytes,
        "native_parity": { "scope": "all 35 indicators and 16 readouts each frame, ordered client command text; command emission tick timing not compared; separate full physical six-phase oracle required", "warmup_commands": warmup_commands, "command_messages": native_chat.len() },
        "work": { "ticks": counts.ticks - counts_before.ticks, "logical_evaluations": counts.logical_kernel_evaluations - counts_before.logical_kernel_evaluations,
            "instant_kernel_invocations": counts.instant_kernel_invocations - counts_before.instant_kernel_invocations,
            "instant_kernel_wire_walks_elided": counts.instant_kernel_wire_walks_elided - counts_before.instant_kernel_wire_walks_elided,
            "instant_transition_hits": counts.instant_transition_hits - counts_before.instant_transition_hits,
            "instant_transition_misses": counts.instant_transition_misses - counts_before.instant_transition_misses,
            "instant_transition_programs": counts.instant_transition_programs - counts_before.instant_transition_programs,
            "instant_transition_operations": counts.instant_transition_operations - counts_before.instant_transition_operations,
            "instant_transition_wire_visits_elided": counts.instant_transition_wire_visits_elided - counts_before.instant_transition_wire_visits_elided,
            "callbacks": counts.callbacks - counts_before.callbacks, "wire_walks": counts.wire_walks - counts_before.wire_walks,
            "piston_actions": counts.piston_actions - counts_before.piston_actions },
    });
    eprintln!("saved FPU+Divider server profile: {report:#}");
}

fn observe(world: &PlotWorld, lamps: &[BlockPos], readouts: &[BlockPos]) -> (u64, u16) {
    let lamps = lamps.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        word | (u64::from(matches!(world.get_block(position), Block::RedstoneLamp { lit: true })) << bit)
    });
    let readouts = readouts.iter().enumerate().fold(0u16, |word, (bit, &position)| {
        word | (u16::from(matches!(world.get_block(position), Block::RedstoneRepeater { repeater } if repeater.powered)) << bit)
    });
    (lamps, readouts)
}

fn observe_graph(backend: &GraphBackend, lamps: &[NodeId], readouts: &[NodeId]) -> (u64, u16) {
    let lamps = lamps.iter().enumerate().fold(0u64, |word, (bit, &node)| {
        let Block::RedstoneLamp { lit } = backend.node_block(node) else {
            panic!("expected lamp")
        };
        word | (u64::from(lit) << bit)
    });
    let readouts = readouts.iter().enumerate().fold(0u16, |word, (bit, &node)| {
        let Block::RedstoneRepeater { repeater } = backend.node_block(node) else {
            panic!("expected repeater")
        };
        word | (u16::from(repeater.powered) << bit)
    });
    (lamps, readouts)
}
