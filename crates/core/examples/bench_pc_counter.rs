//! cargo run -p mchprs_core --profile bench --locked --example bench_pc_counter
use mchprs_blocks::{
    BlockFace, BlockPos,
    blocks::{Block, LeverFace},
};
use mchprs_core::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpilator::{Compiler, CompilerOptions, PublicationMode, TaskMonitor},
    redstone,
    world::{World, storage::Chunk},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const TICKS: usize = 4096;

fn output(world: &PlotWorld) -> u16 {
    (0..16).fold(0, |word, bit| {
        let pos = ORIGIN + BlockPos::new(13, 2, 3 + bit * 4);
        let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
            panic!("missing PC counter output at {pos:?}")
        };
        (word << 1) | u16::from(!repeater.powered)
    })
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = "test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem";
    let bytes = std::fs::read(root.join(fixture)).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let reference: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/cpu-references/interpreter-20261009-pc-counter-reference.json")).unwrap())
            .unwrap();
    assert_eq!(hash, reference["schematic_sha256"].as_str().unwrap());
    let expected: Vec<u16> = serde_json::from_value(reference["per_game_tick_output_words"][0].clone()).unwrap();
    assert_eq!(expected.len(), TICKS);
    let mut clip = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!((clip.size_x, clip.size_y, clip.size_z), (24, 19, 75));
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let bounds = (ORIGIN - BlockPos::new(4, 4, 4), ORIGIN + BlockPos::new(27, 22, 78));
    let output_path = root.join("target/redpilator-bench-results/pc-counter.json");
    std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
    let mut reports = Vec::new();
    for sample in 1..=3 {
        let backends = if sample % 2 == 1 {
            ["interpreter", "rust", "llvm"]
        } else {
            ["llvm", "rust", "interpreter"]
        };
        for backend in backends {
            let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
            let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
            paste_clipboard(&mut world, &clip, ORIGIN, false);
            let mut levers = Vec::new();
            for y in 0..19 {
                for z in 0..75 {
                    for x in 0..24 {
                        let pos = ORIGIN + BlockPos::new(x, y, z);
                        if let Block::Lever { lever } = world.get_block(pos) {
                            assert!(!lever.powered, "PC counter lever must initially be off");
                            levers.push(pos);
                        }
                    }
                }
            }
            assert_eq!(levers.len(), 23);
            let mut compiler = Compiler::default();
            let compile_start = Instant::now();
            let result = if backend == "interpreter" {
                Ok(())
            } else {
                compiler.compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        llvm: backend == "llvm",
                        publication: Some(PublicationMode::All),
                        ..Default::default()
                    },
                    world.scheduler().iter_entries().collect(),
                    Arc::new(TaskMonitor::default()),
                )
            };
            let compile_seconds = compile_start.elapsed().as_secs_f64();
            if let Err(error) = result {
                reports.push(json!({"sample":sample,"backend":backend,"status":"compile_rejected","reason":error.to_string(),"compile_seconds":compile_seconds}));
            } else {
                for pos in levers {
                    if backend != "interpreter" {
                        compiler.try_on_use_block(pos).unwrap();
                        continue;
                    }
                    let Block::Lever { mut lever } = world.get_block(pos) else {
                        unreachable!()
                    };
                    lever.powered = true;
                    world.set_block(pos, Block::Lever { lever });
                    redstone::update_surrounding_blocks(&mut world, pos);
                    let face = match lever.face {
                        LeverFace::Ceiling => BlockFace::Top,
                        LeverFace::Floor => BlockFace::Bottom,
                        LeverFace::Wall => lever.facing.opposite().block_face(),
                    };
                    redstone::update_surrounding_blocks(&mut world, pos.offset(face));
                }
                if backend == "interpreter" {
                    world.tick_interpreted();
                } else {
                    compiler.tick().unwrap();
                }
                let mut elapsed = Duration::ZERO;
                let mut first_difference = None;
                for (tick, &word) in expected.iter().enumerate() {
                    let start = Instant::now();
                    if backend == "interpreter" {
                        world.tick_interpreted();
                    } else {
                        compiler.tick().unwrap();
                    }
                    elapsed += start.elapsed();
                    if backend != "interpreter" {
                        compiler.try_flush(&mut world).unwrap();
                    }
                    let actual = output(&world);
                    if actual != word && first_difference.is_none() {
                        first_difference = Some(json!({"tick":tick+1,"expected":word,"actual":actual}));
                    }
                }
                reports.push(
                    json!({"sample":sample,"backend":backend,"status":if first_difference.is_none() {"passed"} else {"diverged"},
                    "ticks":TICKS,"seconds":elapsed.as_secs_f64(),"tps":TICKS as f64/elapsed.as_secs_f64(),
                    "compile_seconds":compile_seconds,"first_difference":first_difference,"final_count":output(&world)}),
                );
            }
            println!("PC_COUNTER {}", reports.last().unwrap());
            std::fs::write(
                &output_path,
                serde_json::to_vec_pretty(&json!({
                    "fixture":fixture,"fixture_sha256":hash,"timing":"tick execution only; activation, publication and output checks excluded",
                    "validation":"all 4096 output words compared with frozen interpreter trace","reports":reports,
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }
    assert!(
        reports.iter().all(|r| r["status"] == "passed"),
        "a backend rejected the counter or differed from the frozen trace"
    );
}
