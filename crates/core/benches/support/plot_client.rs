//! Real plot updates and a compressed TCP receiver, paired with the same core execution.
use crate::cpu_support::*;
use anyhow::{Result, ensure};
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::world::World;
use mchprs_core::{
    plot::{PlotWorld, benchmark},
    redpiler::{Compiler, CompilerOptions},
};
use mchprs_network::{
    PlayerPacketSender,
    packets::{
        PacketDecoderExt,
        clientbound::{CKeepAlive, ClientBoundPacket},
        serverbound::SKeepAlive,
    },
    test_support::{connection, read_frame},
};
use serde_json::json;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const END: i64 = i64::MIN;

fn decode<T>(result: mchprs_network::packets::DecodeResult<T>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("packet decode failed: {error:?}"))
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str())
}

fn world(cpu: Cpu) -> PlotWorld {
    let mut world = load_cpu(cpu);
    if cpu.name == "cpu_bubblesort" {
        let sign = cpu.origin + BlockPos::new(186, 2, 184);
        assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
        assert!(world.set_block(sign, Block::Air));
    }
    world.flush_block_changes();
    world
}

fn compile(world: &PlotWorld, level: &str) -> Result<Compiler> {
    let mut compiler = Compiler::default();
    let mut options = CompilerOptions::parse(&format!("-O{level}")).map_err(anyhow::Error::msg)?;
    options.budget_multiplier = 8;
    compiler.compile(
        world,
        world.get_corners(),
        options,
        world.scheduler().iter_entries().collect(),
        Default::default(),
    )?;
    Ok(compiler)
}

pub fn main(args: &[String]) -> Result<()> {
    let name = option(args, "--cpu").unwrap_or("cpu_bubblesort");
    let cpu = *CPUS
        .iter()
        .find(|cpu| cpu.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown CPU {name}"))?;
    let seconds: f64 = option(args, "--seconds").unwrap_or("3").parse()?;
    ensure!(seconds.is_finite() && seconds > 0.0 && seconds <= 30.0, "--seconds must be in (0, 30]");
    let iterations: usize = option(args, "--iterations").unwrap_or("1").parse()?;
    ensure!(iterations > 0, "--iterations must be positive");
    let level = option(args, "--graph-tier").unwrap_or("5");
    let screen_only = args.iter().any(|arg| arg == "--screen-only");
    let mut samples = Vec::new();
    for sample in 0..iterations {
        let mut live_world = world(cpu);
        live_world.set_screen_only(screen_only);
        let live = compile(&live_world, level)?;
        let mut core_world = world(cpu);
        let mut core = compile(&core_world, level)?;
        let conn = connection(true)?;
        let sender = PlayerPacketSender::new(&conn.player);
        let player = benchmark::player(conn.player);
        let mut plot = benchmark::with_client(live_world, player, mchprs_save_data::plot_data::Tps::Unlimited);
        plot.redpiler = live;
        let receiver = std::thread::spawn(move || -> Result<_> {
            let mut peer = conn.peer;
            let mut blocks = HashMap::new();
            let mut packets = 0u64;
            loop {
                let (id, mut frame) = decode(read_frame(&mut peer, true))?;
                packets += 1;
                match id {
                    0x26 => {
                        let id = decode(frame.read_long())?;
                        if id == END {
                            return Ok((packets, blocks));
                        }
                        conn.incoming
                            .send(Box::new(SKeepAlive { id }))
                            .map_err(|_| anyhow::anyhow!("plot inbound queue closed"))?;
                    }
                    0x08 => {
                        let pos = BlockPos::from_packed(decode(frame.read_position())?);
                        blocks.insert(pos, decode(frame.read_varint())? as u32);
                    }
                    0x4d => {
                        let section = decode(frame.read_long())?;
                        let origin = BlockPos::new(
                            (section >> 42) as i32 * 16,
                            (section << 44 >> 44) as i32 * 16,
                            (section << 22 >> 42) as i32 * 16,
                        );
                        let count = decode(frame.read_varint())?;
                        ensure!((0..=4096).contains(&count), "invalid section update count");
                        for _ in 0..count {
                            let record = decode(frame.read_varlong())? as u64;
                            let pos = origin + BlockPos::new(((record >> 8) & 15) as i32, (record & 15) as i32, ((record >> 4) & 15) as i32);
                            blocks.insert(pos, (record >> 12) as u32);
                        }
                    }
                    _ => {}
                }
            }
        });
        plot.redpiler.on_use_block(cpu.origin + cpu.start);
        core.on_use_block(cpu.origin + cpu.start);
        let wall = benchmark::run_for(&mut plot, Duration::from_secs_f64(seconds));
        let phases = benchmark::timings(&plot);
        let counts = plot.redpiler.graph_execution_counts().unwrap();
        ensure!(
            counts.ticks > 0 && counts.ticks <= u64::from(cpu.active_ticks),
            "window exceeded the active circuit episode; shorten --seconds"
        );
        let delivery_started = Instant::now();
        benchmark::publish(&mut plot);
        sender.send_packet(&CKeepAlive { id: END }.encode());
        let (received_packets, received_blocks) = receiver.join().map_err(|_| anyhow::anyhow!("client receiver panicked"))??;
        let delivery = delivery_started.elapsed();
        let network = sender.send_stats();
        ensure!(network.failures == 0 && network.queued_bytes == 0, "client did not keep up");
        ensure!(!received_blocks.is_empty(), "client received no circuit block updates");
        for (&position, &id) in &received_blocks {
            ensure!(plot.world.get_block_raw(position) == id, "client state differs at {position}");
        }
        let started = Instant::now();
        for _ in 0..counts.ticks {
            core.tick_with_world(&mut core_world)?;
        }
        let core_time = started.elapsed();
        ensure!(core.graph_execution_counts() == Some(counts), "plot loop changed compiled execution");
        core.reset(&mut core_world, plot.world.get_corners());
        plot.redpiler.reset(&mut plot.world, core_world.get_corners());
        let actual = checkpoint(&plot.world, counts.ticks as u32, &[]);
        let expected = checkpoint(&core_world, counts.ticks as u32, &[]);
        ensure!(actual == expected, "plot/core state or future diverged after interpreter handoff");
        let report = json!({
            "sample": sample + 1, "game_ticks": counts.ticks,
            "plot_seconds": wall.as_secs_f64(), "plot_tps": counts.ticks as f64 / wall.as_secs_f64(),
            "core_seconds": core_time.as_secs_f64(), "core_tps": counts.ticks as f64 / core_time.as_secs_f64(),
            "final_delivery_seconds": delivery.as_secs_f64(),
            "including_final_delivery_tps": counts.ticks as f64 / (wall + delivery).as_secs_f64(),
            "phases": phases,
            "client": {"received_packets": received_packets, "updated_positions": received_blocks.len(),
                "bytes": network.bytes, "encode_seconds": network.encode_ns as f64 / 1e9,
                "compression_seconds": network.compress_ns as f64 / 1e9, "write_seconds": network.write_ns as f64 / 1e9,
                "coalesced_blocks": network.coalesced_blocks, "failures": network.failures},
            "checks": "same engine work, full world and future after handoff, received block states, ordered TCP end barrier",
        });
        println!(
            "{name} O{level} sample {}: plot {:.1} TPS, core {:.1} TPS; {}",
            sample + 1,
            report["plot_tps"],
            report["core_tps"],
            report["phases"]
        );
        samples.push(report);
    }
    let report = json!({"schema": 1, "cpu": name, "tier": level, "clients": 1, "compressed": true,
        "scope": "production plot updates and incremental TCP delivery; excludes login, initial chunks and Minecraft rendering",
        "samples": samples});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(option(args, "--output").unwrap_or("target/plot-client.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!("Report: {}", path.display());
    Ok(())
}
