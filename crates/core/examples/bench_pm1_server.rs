//! Fixed circuit game ticks through Plot::update and the compressed TCP writer.
#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
#[path = "../test_support/pm1.rs"]
#[allow(dead_code)]
mod pm1;

use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use mchprs_core::{
    plot::benchmark,
    redpilator::{CompilerOptions, PublicationMode},
    world::World,
};
use mchprs_network::{
    PlayerPacketSender, SendStats,
    packets::{
        PacketDecoderExt,
        clientbound::{CKeepAlive, ClientBoundPacket},
        serverbound::{SKeepAlive, ServerBoundPacket},
    },
    test_support::{connection, read_frame},
};
use mchprs_save_data::plot_data::Tps;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Cursor,
    net::TcpStream,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BEGIN: i64 = i64::MIN + 1;
const END: i64 = i64::MIN + 2;

#[derive(Default, Serialize)]
struct Received {
    packets: u64,
    packet_ids: BTreeMap<i32, u64>,
    block_records: u64,
    piston_records: u64,
    piston_actions: u64,
    moving_piston_entities: u64,
    chunk_packets: u64,
    #[serde(skip_serializing)]
    commands: Vec<String>,
    #[serde(skip_serializing)]
    blocks: HashMap<BlockPos, u32>,
}

fn system_chat(frame: &mut Cursor<Vec<u8>>) -> String {
    let kind = frame.read_unsigned_byte().unwrap();
    let component = nbt::Value::from_reader(kind, frame).unwrap();
    assert!(!frame.read_bool().unwrap(), "unexpected action-bar message");
    assert_eq!(frame.position() as usize, frame.get_ref().len());
    mchprs_network::text::to_json(&component)
}

fn piston(block: Block) -> bool {
    matches!(block, Block::Piston { .. } | Block::PistonHead { .. } | Block::MovingPiston { .. })
}

fn block_record(received: &mut Received, mechanical: &mut HashSet<BlockPos>, position: BlockPos, state: u32) {
    received.block_records += 1;
    received.blocks.insert(position, state);
    let now = piston(Block::from_id(state));
    if mechanical.remove(&position) || now {
        received.piston_records += 1;
    }
    if now {
        mechanical.insert(position);
    }
}

fn drain(
    mut peer: TcpStream,
    incoming: mpsc::SyncSender<Box<dyn ServerBoundPacket>>,
    events: mpsc::Sender<Result<Option<Received>, String>>,
    mut mechanical: HashSet<BlockPos>,
    visible: HashMap<BlockPos, u32>,
) {
    let mut received = Received {
        blocks: visible,
        ..Default::default()
    };
    let mut timed = false;
    let piston_ids = [
        Block::from_name("piston").unwrap().registry_id(),
        Block::from_name("sticky_piston").unwrap().registry_id(),
    ];
    let moving_type = BlockEntity::MovingPiston(Default::default()).ty();
    loop {
        let (id, mut frame) = match read_frame(&mut peer, true) {
            Ok(frame) => frame,
            Err(error) => {
                let _ = events.send(Err(format!("client packet decode failed: {error:?}")));
                return;
            }
        };
        if id == 0x26 {
            let token = frame.read_long().unwrap();
            if token == BEGIN {
                assert!(!timed);
                timed = true;
                events.send(Ok(None)).unwrap();
                continue;
            }
            if token == END {
                assert!(timed);
                events.send(Ok(Some(received))).unwrap();
                return;
            }
            incoming.send(Box::new(SKeepAlive { id: token })).unwrap();
        }
        if !timed {
            continue;
        }
        received.packets += 1;
        *received.packet_ids.entry(id).or_default() += 1;
        match id {
            0x08 => {
                let position = BlockPos::from_packed(frame.read_position().unwrap());
                let state = frame.read_varint().unwrap() as u32;
                block_record(&mut received, &mut mechanical, position, state);
            }
            0x4d => {
                let section = frame.read_long().unwrap();
                let origin = BlockPos::new(
                    (section >> 42) as i32 * 16,
                    (section << 44 >> 44) as i32 * 16,
                    (section << 22 >> 42) as i32 * 16,
                );
                let count = frame.read_varint().unwrap();
                assert!((0..=4096).contains(&count));
                for _ in 0..count {
                    let record = frame.read_varlong().unwrap() as u64;
                    let position = origin + BlockPos::new(((record >> 8) & 15) as i32, (record & 15) as i32, ((record >> 4) & 15) as i32);
                    block_record(&mut received, &mut mechanical, position, (record >> 12) as u32);
                }
            }
            0x07 => {
                frame.read_position().unwrap();
                frame.read_unsigned_byte().unwrap();
                frame.read_unsigned_byte().unwrap();
                received.piston_actions += u64::from(piston_ids.contains(&(frame.read_varint().unwrap() as u32)));
            }
            0x06 => {
                frame.read_position().unwrap();
                received.moving_piston_entities += u64::from(frame.read_varint().unwrap() == moving_type);
            }
            0x27 => received.chunk_packets += 1,
            0x72 => received.commands.push(system_chat(&mut frame)),
            _ => {}
        }
    }
}

fn network(before: SendStats, after: SendStats) -> Value {
    json!({"packets":after.packets-before.packets,"bytes":after.bytes-before.bytes,
        "encode_seconds":(after.encode_ns-before.encode_ns) as f64/1e9,
        "compress_seconds":(after.compress_ns-before.compress_ns) as f64/1e9,
        "write_seconds":(after.write_ns-before.write_ns) as f64/1e9,
        "coalesced_blocks":after.coalesced_blocks-before.coalesced_blocks,
        "queued_bytes_end":after.queued_bytes,"failures":after.failures-before.failures,
        "included_end_barrier_packets":1})
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let protocol = (args[0] != "zcore").then(|| pm1::protocol(&PathBuf::from(&args[0])));
    let output = PathBuf::from(&args[1]);
    let backend = &args[2];
    assert!(["llvm", "rust"].contains(&backend.as_str()));
    let ticks: u32 = args
        .get(3)
        .map_or(protocol.as_ref().map_or(50_000, |p| p.ticks), |value| value.parse().unwrap());
    let samples: u32 = args.get(4).map_or(3, |value| value.parse().unwrap());
    assert!(ticks > 0 && samples > 0);
    let relaxed = args.iter().any(|arg| arg == "--relaxed");
    let reference: Option<Value> = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--reference="))
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
    let selected = args.iter().find_map(|arg| arg.strip_prefix("--publication="));
    assert!(selected.is_none_or(|modes| modes.split(',').all(|mode| ["none", "io", "redstone"].contains(&mode))));
    let hz: u32 = args.iter().find_map(|arg| arg.strip_prefix("--hz=")).map_or(10, |v| v.parse().unwrap());
    assert!((1..=1000).contains(&hz));
    let selected_batch: Option<u32> = args.iter().find_map(|arg| arg.strip_prefix("--batch=")).map(|v| v.parse().unwrap());
    assert!(selected_batch.is_none_or(|n| [16, 32, 64].contains(&n)));
    let cpu = protocol.as_ref().map_or(cpus::CPUS[3], pm1::cpu);
    let load = || {
        let (mut world, bounds) = if let Some(p) = &protocol {
            pm1::load(p)
        } else {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
            let clip = mchprs_core::plot::worldedit::load_schematic(std::fs::File::open(path).unwrap()).unwrap();
            (
                cpus::load_cpu(cpu),
                (
                    cpu.origin,
                    cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
                ),
            )
        };
        benchmark::live_command_output(&mut world);
        (world, bounds)
    };
    let (initial, bounds) = load();
    let mut mechanical = HashSet::new();
    let mut io_positions = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                if piston(initial.get_block(position)) {
                    mechanical.insert(position);
                }
                let block = initial.get_block(position);
                if matches!(block, Block::Lever { .. } | Block::StoneButton { .. } | Block::RedstoneLamp { .. })
                    || block.is_copper_bulb()
                    || block.pressure_plate_powered().is_some()
                {
                    io_positions.push(position);
                }
            }
        }
    }
    let flags = format!(
        "{} {} --publish-io",
        if backend == "llvm" { "--llvm" } else { "" },
        if relaxed { "--relaxed" } else { "" }
    );
    let (_, seed, compile_elapsed) = cpus::compile_cpu_with_options(initial, cpu, CompilerOptions::parse(&flags).unwrap());
    if let Some(cpu) = args.iter().find_map(|arg| arg.strip_prefix("--affinity=")) {
        benchmark::pin_cpu(cpu.parse().unwrap());
    }
    let mut rows = Vec::new();
    let mut expected_output = None;
    for sample in 1..=samples {
        let mut modes = vec!["none", "io", "redstone"];
        if sample % 2 == 0 {
            modes.reverse();
        }
        let mut batches = if backend == "llvm" { vec![16, 32, 64] } else { vec![16] };
        if sample % 2 == 0 {
            batches.reverse();
        }
        for batch in batches.into_iter().filter(|n| selected_batch.is_none_or(|v| v == *n)) {
            for mode in modes
                .iter()
                .copied()
                .filter(|mode| selected.is_none_or(|selected| selected.split(',').any(|value| value == *mode)))
            {
                let (mut world, bounds) = load();
                let mut compiler = seed
                    .benchmark_restart(&world, bounds, world.scheduler().iter_entries().collect())
                    .unwrap();
                if protocol.is_none() {
                    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
                    for _ in 0..413 {
                        compiler.tick().unwrap();
                    }
                    compiler.try_flush(&mut world).unwrap();
                }
                world.flush_block_changes();
                let connection = connection(true).unwrap();
                connection.peer.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
                let sender = PlayerPacketSender::new(&connection.player);
                let (events, messages) = mpsc::channel();
                let positions = mechanical.clone();
                let visible = io_positions.iter().map(|&p| (p, world.get_block_raw(p))).collect();
                let reader = thread::spawn(move || drain(connection.peer, connection.incoming, events, positions, visible));
                let mut plot = benchmark::with_client(world, benchmark::player(connection.player), Tps::Limited(0));
                plot.redpilator = compiler;
                benchmark::publication(
                    &mut plot,
                    match mode {
                        "io" => Some(PublicationMode::Io),
                        "redstone" => Some(PublicationMode::Redstone),
                        _ => None,
                    },
                    hz,
                );
                sender.send_packet(&CKeepAlive { id: BEGIN }.encode());
                assert!(messages.recv_timeout(Duration::from_secs(30)).unwrap().unwrap().is_none());
                while sender.send_stats().packets == 0 {
                    thread::yield_now();
                }
                let before = sender.send_stats();
                plot.redpilator
                    .try_on_use_block(cpu.origin + if protocol.is_some() { cpu.start } else { BlockPos::new(207, 10, 154) })
                    .unwrap();
                let started = Instant::now();
                let loop_elapsed = benchmark::run_ticks_batched(&mut plot, ticks, batch);
                let command_drain = benchmark::drain_commands(&mut plot);
                sender.send_packet(&CKeepAlive { id: END }.encode());
                let received = messages.recv_timeout(Duration::from_secs(30)).unwrap().unwrap().unwrap();
                while sender.send_stats().packets < before.packets + received.packets + 1 {
                    thread::yield_now();
                }
                let elapsed = started.elapsed();
                let after = sender.send_stats();
                reader.join().unwrap();
                assert_eq!(after.failures, before.failures, "network writer failed");
                assert_eq!(after.queued_bytes, 0);
                assert_eq!(
                    received.piston_records + received.piston_actions + received.moving_piston_entities + received.chunk_packets,
                    0,
                    "excluded piston presentation reached the client"
                );
                if mode == "none" {
                    assert_eq!(received.block_records, 0);
                }
                for (&position, &state) in &received.blocks {
                    assert_eq!(plot.world.get_block_raw(position), state, "client differs from latest published world");
                }
                let timings = benchmark::timings(&plot);
                assert_eq!(timings["ticks"], ticks);
                benchmark::materialize(&mut plot, bounds);
                let observed = if let Some(p) = &protocol {
                    pm1::outputs(&plot.world, p)
                } else {
                    let screen = cpus::screen(&plot.world, cpu);
                    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&screen).unwrap()));
                    if ticks == 50_000 {
                        assert_eq!(hash, "6597ffbdb45a34e2570052f1835b9a9807e4ac9853cf41966a3ca2af2551cdfc");
                    }
                    json!({"screen_sha256":hash})
                };
                let commands = received.commands.clone();
                if let Some(reference) = &reference {
                    assert_eq!(reference["fixture_sha256"], cpu.sha256);
                    assert_eq!(reference["ticks"], ticks);
                    assert_eq!(reference["outputs"], observed, "server-path lamp or physical RAM output differs");
                    assert_eq!(reference["commands"], json!(commands), "server-path command output differs");
                }
                let result = (observed.clone(), commands.clone());
                if let Some(expected) = &expected_output {
                    assert_eq!(&result, expected, "fresh engine/publication modes differ");
                } else {
                    expected_output = Some(result);
                }
                let row = json!({"cpu":cpu.name,"backend":backend,"relaxed":relaxed,"sample":sample,"publication":mode,"batch":batch,
                "ticks":ticks,"preparation_ticks":if protocol.is_some(){0}else{413},"elapsed_ms":elapsed.as_secs_f64()*1000.0,
                "tps":ticks as f64/elapsed.as_secs_f64(),"plot_loop_ms":loop_elapsed.as_secs_f64()*1000.0,
                "writer_drain_ms":elapsed.saturating_sub(loop_elapsed).as_secs_f64()*1000.0,
                "command_delivery_drain_ms":command_drain.as_secs_f64()*1000.0,"live_command_limits":true,
                "compile_ms":compile_elapsed.as_secs_f64()*1000.0,"configured_send_rate_hz":if mode=="none" {0} else {hz},
                "observed_publication_rate_hz":timings["publications"].as_u64().unwrap() as f64/loop_elapsed.as_secs_f64(),
                "client_matches_published_world":true,"io_components":io_positions.len(),
                "plot_timings":timings,"network":network(before,after),"received":received,
                "outputs":observed,"commands":commands,"reference_matched":reference.is_some()});
                println!(
                    "PM1_SERVER_RESULT cpu={} backend={backend} publication={mode} batch={batch} sample={sample} ticks={ticks} tps={:.1}",
                    cpu.name,
                    row["tps"].as_f64().unwrap()
                );
                rows.push(row);
                std::fs::write(&output, serde_json::to_vec_pretty(&json!({"fixture_sha256":cpu.sha256,
                "scope":"Fixed game ticks via actual Plot update; normal wall-clock visual cadence; one real compressed TCP client. Elapsed includes final ordered writer barrier/drain; complete-state verification is untimed.",
                "samples":rows})).unwrap()).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_network::packets::clientbound::CChatMessage;

    #[test]
    fn command_trace_decodes_actual_nbt_system_chat() {
        let message = json!({"text":"","extra":[{"text":"[PM-1] "},{"text":"shut","color":"gold"}]}).to_string();
        let encoded = CChatMessage {
            message: message.clone(),
            position: 1,
            sender: 0,
        }
        .encode();
        assert_eq!(encoded.packet_id, 0x72);
        assert_eq!(system_chat(&mut Cursor::new(encoded.buffer)), message);
    }
}
