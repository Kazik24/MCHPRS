use crate::{
    interaction::use_redstone,
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    world::{World, storage::Chunk},
};
use mchprs_blocks::{BlockPos, blocks::Block};
use rand::{RngExt, SeedableRng, rngs::StdRng};
use serde_json::Value;
use std::sync::Arc;

#[test]
fn redstonedb_levers_match_interpreter() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/redstonedb");
    let definitions: Value = serde_json::from_slice(&std::fs::read(root.join("definitions.json")).unwrap()).unwrap();
    let mut failures = Vec::new();
    for definition in definitions["circuits"].as_array().unwrap() {
        let name = definition["description"].as_str().unwrap_or("unnamed fixture");
        let id = definition["id"].as_str().unwrap_or("unknown");
        let result = std::panic::catch_unwind(|| {
            let mut clip = load_schematic(std::fs::File::open(root.join(definition["schematic"].as_str().unwrap())).unwrap()).unwrap();
            clip.offset_x = 0;
            clip.offset_y = 0;
            clip.offset_z = 0;
            let origin = BlockPos::new(8, 8, 8);
            let end = origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1);
            let make_world = || {
                let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
                let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
                paste_clipboard(&mut world, &clip, origin, false);
                world
            };
            let mut native = make_world();
            let compiled_world = make_world();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled_world,
                    (origin, end),
                    CompilerOptions::default(),
                    compiled_world.scheduler().iter_entries().collect(),
                    Arc::new(TaskMonitor::default()),
                )
                .unwrap();
            let levers: Vec<_> = (origin.y..=end.y)
                .flat_map(|y| (origin.z..=end.z).flat_map(move |z| (origin.x..=end.x).map(move |x| BlockPos::new(x, y, z))))
                .filter(|&pos| matches!(native.get_block(pos), Block::Lever { .. }))
                .collect();
            assert!(!levers.is_empty(), "{name}: no levers found");
            let transitions: Vec<usize> = if levers.len() <= 3 {
                (1..(1usize << levers.len()))
                    .map(|step| step.trailing_zeros() as usize)
                    .chain(std::iter::once(levers.len() - 1))
                    .collect()
            } else {
                (0..levers.len()).chain((0..levers.len()).rev()).collect()
            };
            let seed = id
                .as_bytes()
                .iter()
                .fold(0u64, |seed, byte| seed.wrapping_mul(31).wrapping_add(u64::from(*byte)));
            let mut rng = StdRng::seed_from_u64(seed);
            let mut transitions: Vec<_> = transitions.into_iter().map(|index| (index, 20)).collect();
            transitions.extend((0..32).map(|_| (rng.random_range(0..levers.len()), rng.random_range(1..=8))));
            for (step, (index, wait)) in transitions.into_iter().enumerate() {
                let lever = levers[index];
                use_redstone(native.get_block(lever), &mut native, lever, None);
                compiler.try_on_use_block(lever).unwrap();
                for tick in 1..=wait {
                    native.tick_interpreted();
                    compiler.tick().unwrap();
                    let runtime = compiler.runtime.as_ref().unwrap();
                    for &pos in &runtime.program.positions {
                        let expected = native.get_block(pos);
                        let actual = runtime.project(pos);
                        if let (Block::RedstoneWire { wire: a }, Some(Block::RedstoneWire { wire: b })) = (expected, actual) {
                            assert_eq!(
                                a.power, b.power,
                                "{name}: wire {pos:?} after {lever:?} action {step} (wait {wait}) tick {tick}"
                            );
                        } else {
                            assert_eq!(
                                Some(expected),
                                actual,
                                "{name}: block {pos:?} after {lever:?} action {step} (wait {wait}) tick {tick}"
                            );
                        }
                    }
                }
            }
        });
        if let Err(error) = result {
            let detail = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            let failure = format!("{name} ({id}, https://redstonedb.com/schematics/{id}): {detail}");
            eprintln!("FAILED: {failure}");
            failures.push(failure);
        }
    }
    assert!(
        failures.is_empty(),
        "{} RedstoneDB fixtures failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
