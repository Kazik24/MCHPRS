//! Physical Graph baseline for FPU_27 with the normal `--optimize` options.
//! This fixture test does not exercise or claim activation of the event-netlist engine.
#[path = "../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
use mchprs_blocks::blocks::Block;
use mchprs_core::{
    redpiler::{Compiler, CompilerOptions},
    world::World,
};
use std::time::Instant;

#[test]
#[ignore = "external FPU_27 schematic and expensive production admission"]
fn fpu27_isolated_default_optimize() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/piston-research/fixtures/fpu_legal.json")).unwrap()).unwrap();
    manifest["fixture"] = serde_json::json!("test_data/piston-research/FPU_27.schem");
    manifest["sha256"] = serde_json::json!("b6182c6c0541d692d635d72d23ecc8f33cef2fb6071f4ba4be66485ff6892bc1");
    manifest["origin"] = serde_json::json!([40, 30, 40]);
    let mut world = cpus::load_manifest(&manifest).unwrap();
    let origin = cpus::manifest_origin(&manifest);
    let ports = &manifest["ports"];
    let controls = ["light_blue_operand_inputs", "red_operand_inputs", "opcode_levers_by_sign_weight_4_2_1"]
        .iter()
        .flat_map(|key| ports[*key].as_array().unwrap())
        .map(|value| origin + cpus::manifest_position(value))
        .collect::<Vec<_>>();
    let outputs = ports["output_by_label_bit_15_to_0"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| origin + cpus::manifest_position(value))
        .collect::<Vec<_>>();
    let trigger = origin + cpus::manifest_position(&ports["trigger"]);
    let mut mask = controls.iter().enumerate().fold(0u64, |word, (bit, &p)| {
        word | (u64::from(matches!(world.get_block(p), Block::Lever { lever } if lever.powered)) << bit)
    });
    assert!(matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered));
    let mut compiler = Compiler::default();
    let options = CompilerOptions::parse("--optimize").unwrap();
    let compile_start = Instant::now();
    compiler
        .compile(&world, world.get_corners(), options, Vec::new(), Default::default())
        .unwrap();
    let compile_seconds = compile_start.elapsed().as_secs_f64();
    let before = compiler.graph_execution_counts().unwrap();
    let mut random = 0x19a4110b5eedu64;
    let mut checksum = 0u16;
    let frames = 32;
    let start = Instant::now();
    for frame in 0..frames {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        for (bit, &p) in controls.iter().enumerate() {
            if (random ^ mask) & (1 << bit) != 0 {
                compiler.on_use_block(p);
            }
        }
        mask = random;
        if frame == 0 {
            compiler.on_use_block(trigger);
        }
        for _ in 0..6 {
            compiler.tick_with_world(&mut world).unwrap();
        }
        compiler.flush(&mut world);
        let word = outputs.iter().fold(0u16, |word, &p| {
            (word << 1) | u16::from(matches!(world.get_block(p), Block::RedstoneWire { wire } if wire.power > 0))
        });
        checksum = checksum.rotate_left(1) ^ word;
    }
    let seconds = start.elapsed().as_secs_f64();
    let after = compiler.graph_execution_counts().unwrap();
    println!(
        "physical Graph FPU_27 --optimize baseline: compile_seconds={compile_seconds:.3} frames={frames} game_ticks={} seconds={seconds:.6} actual_game_tps={:.1} wire_walks={} piston_actions={} checksum={checksum:04x} flags={:?} (does not exercise event-netlist activation)",
        frames * 6,
        frames as f64 * 6.0 / seconds,
        after.wire_walks - before.wire_walks,
        after.piston_actions - before.piston_actions,
        compiler.current_flags()
    );
    assert_eq!(after.ticks - before.ticks, (frames * 6) as u64);
    assert!(after.wire_walks > before.wire_walks, "the physical Graph must process wire work");
}
