use crate::cpus;
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::{plot::PlotWorld, world::World};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const SHA256: &str = "6957e4f9537d519e2133f0168fd25837a084a10e54d96166c72fd3dcf3bb8939";
pub type Bounds = (BlockPos, BlockPos);

#[derive(Deserialize)]
pub struct Pair {
    lower: [i32; 3],
    upper: [i32; 3],
}

#[derive(Deserialize)]
pub struct Protocol {
    pub origin: [i32; 3],
    pub start_local: [i32; 3],
    pub ticks: u32,
    pub lamp_positions: Vec<[i32; 3]>,
    pub ram_pairs: Vec<Pair>,
}

pub fn protocol(path: &Path) -> Protocol {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn pos([x, y, z]: [i32; 3]) -> BlockPos {
    BlockPos::new(x, y, z)
}

pub fn cpu(protocol: &Protocol) -> cpus::Cpu {
    cpus::Cpu {
        name: "pm1_redpilator",
        schematic: "pm1_redpilator.schem",
        sha256: SHA256,
        start: pos(protocol.start_local),
        stop: None,
        origin: pos(protocol.origin),
        active_ticks: protocol.ticks,
        screen: Some(cpus::Screen {
            x: 214,
            top_y: 103,
            bottom_y: 72,
            left_z: 78,
            right_z: 109,
            sparse: false,
        }),
    }
}

pub fn load(protocol: &Protocol) -> (PlotWorld, Bounds) {
    let cpu = cpu(protocol);
    let world = cpus::load_cpu(cpu);
    let bounds = (cpu.origin, cpu.origin + BlockPos::new(221, 233, 176));
    assert!(matches!(world.get_block(cpu.origin + cpu.start), Block::StoneButton { button } if !button.powered));
    (world, bounds)
}

pub fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty()
}

pub fn outputs(world: &PlotWorld, protocol: &Protocol) -> Value {
    let origin = pos(protocol.origin);
    let lamps: Vec<_> = protocol
        .lamp_positions
        .iter()
        .map(|&local| {
            let Block::RedstoneLamp { lit } = world.get_block(origin + pos(local)) else {
                panic!("missing output lamp at {local:?}")
            };
            lit
        })
        .collect();
    let ram: Vec<_> = protocol
        .ram_pairs
        .iter()
        .map(
            |pair| match (world.get_block(origin + pos(pair.lower)), world.get_block(origin + pos(pair.upper))) {
                (Block::RedstoneBlock, _) => Some(0u8),
                (_, Block::RedstoneBlock) => Some(1u8),
                _ => None,
            },
        )
        .collect();
    let screen = cpus::screen(world, cpu(protocol));
    json!({"lamps":lamps,"ram_pairs":ram,"screen":screen,"screen_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&screen).unwrap()))})
}
