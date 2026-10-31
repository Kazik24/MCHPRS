use mchprs_blocks::{
    BlockFace, BlockPos,
    blocks::{Block, LeverFace},
};
use mchprs_core::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{CompileError, Compiler, CompilerOptions, TaskMonitor},
    world::{World, storage::Chunk},
};
use mchprs_world::TickEntry;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const ORIGIN: BlockPos = BlockPos::new(8, 8, 8);
const INPUTS: [BlockPos; 32] = [
    BlockPos::new(3, 35, 8),
    BlockPos::new(3, 35, 10),
    BlockPos::new(3, 35, 12),
    BlockPos::new(3, 35, 14),
    BlockPos::new(3, 35, 16),
    BlockPos::new(3, 35, 18),
    BlockPos::new(3, 35, 20),
    BlockPos::new(3, 35, 22),
    BlockPos::new(3, 35, 24),
    BlockPos::new(3, 35, 26),
    BlockPos::new(3, 35, 28),
    BlockPos::new(3, 35, 30),
    BlockPos::new(3, 35, 32),
    BlockPos::new(3, 35, 34),
    BlockPos::new(3, 35, 36),
    BlockPos::new(3, 35, 38),
    BlockPos::new(3, 35, 40),
    BlockPos::new(3, 35, 42),
    BlockPos::new(3, 35, 44),
    BlockPos::new(3, 35, 46),
    BlockPos::new(3, 35, 48),
    BlockPos::new(3, 35, 50),
    BlockPos::new(3, 35, 52),
    BlockPos::new(3, 35, 54),
    BlockPos::new(3, 35, 56),
    BlockPos::new(3, 35, 58),
    BlockPos::new(3, 35, 60),
    BlockPos::new(3, 35, 62),
    BlockPos::new(3, 35, 64),
    BlockPos::new(3, 35, 66),
    BlockPos::new(3, 35, 68),
    BlockPos::new(3, 35, 70),
];
const OPCODE: [BlockPos; 3] = [BlockPos::new(8, 35, 2), BlockPos::new(8, 35, 4), BlockPos::new(8, 35, 6)];
const OUTPUTS: [BlockPos; 16] = [
    BlockPos::new(170, 5, 32),
    BlockPos::new(170, 5, 36),
    BlockPos::new(170, 5, 40),
    BlockPos::new(170, 5, 44),
    BlockPos::new(170, 5, 48),
    BlockPos::new(170, 5, 52),
    BlockPos::new(170, 5, 56),
    BlockPos::new(170, 5, 60),
    BlockPos::new(170, 5, 64),
    BlockPos::new(170, 5, 68),
    BlockPos::new(170, 5, 72),
    BlockPos::new(170, 5, 76),
    BlockPos::new(170, 5, 80),
    BlockPos::new(170, 5, 84),
    BlockPos::new(170, 5, 88),
    BlockPos::new(170, 5, 92),
];
const TRIGGER: BlockPos = BlockPos::new(6, 36, 76);
const RAW_MASKS: [u64; 2] = [0x412345678, 0x558e1d715];
const WINDOW_TICKS: u32 = 512;

fn controls() -> [BlockPos; 35] {
    let mut controls = [BlockPos::new(0, 0, 0); 35];
    controls[..3].copy_from_slice(&OPCODE);
    controls[3..].copy_from_slice(&INPUTS);
    controls
}

fn control_bit(index: usize) -> u64 {
    1u64 << (34 - index)
}

fn output_word(world: &PlotWorld) -> u16 {
    OUTPUTS.iter().fold(0, |word, pos| {
        let Block::RedstoneWire { wire } = world.get_block(ORIGIN + *pos) else {
            panic!("output port changed block type at {pos:?}")
        };
        (word << 1) | u16::from(wire.power != 0)
    })
}

fn native_mask(world: &mut PlotWorld, previous: u64, next: u64) -> usize {
    let mut changes = 0;
    for (index, pos) in controls().into_iter().enumerate() {
        let bit = control_bit(index);
        if (previous ^ next) & bit == 0 {
            continue;
        }
        changes += 1;
        let pos = ORIGIN + pos;
        let Block::Lever { mut lever } = world.get_block(pos) else {
            unreachable!()
        };
        lever.powered = next & bit != 0;
        world.set_block(pos, Block::Lever { lever });
        mchprs_core::redstone::update_surrounding_blocks(world, pos);
        let attached = match lever.face {
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        mchprs_core::redstone::update_surrounding_blocks(world, pos.offset(attached));
    }
    changes
}

fn run_native(world: &mut PlotWorld) -> (f64, Vec<u16>, usize) {
    let mut samples = Vec::with_capacity(WINDOW_TICKS as usize);
    let mut previous = 0;
    let mut control_changes = 0;
    let started = Instant::now();
    for tick in 0..WINDOW_TICKS {
        let next = RAW_MASKS[(tick as usize / 128) % RAW_MASKS.len()];
        control_changes += native_mask(world, previous, next);
        previous = next;
        world.tick_interpreted();
        samples.push(output_word(world));
    }
    (started.elapsed().as_secs_f64(), samples, control_changes)
}

fn run_compiled(compiler: &mut Compiler, world: &mut PlotWorld, publish: bool) -> Result<(f64, f64, f64, Vec<u16>, usize), String> {
    let mut samples = Vec::with_capacity(WINDOW_TICKS as usize);
    let mut previous = 0;
    let mut control_changes = 0;
    let mut tick_only_seconds = 0.0;
    let mut publication_seconds = 0.0;
    let started = Instant::now();
    for tick in 0..WINDOW_TICKS {
        let next = RAW_MASKS[(tick as usize / 128) % RAW_MASKS.len()];
        for (index, pos) in controls().into_iter().enumerate() {
            if (previous ^ next) & control_bit(index) != 0 {
                compiler.try_on_use_block(ORIGIN + pos).map_err(|error| error.to_string())?;
                control_changes += 1;
            }
        }
        previous = next;
        let tick_started = Instant::now();
        compiler.tick().map_err(|error| error.to_string())?;
        tick_only_seconds += tick_started.elapsed().as_secs_f64();
        if publish {
            let publish_started = Instant::now();
            compiler.try_flush(world).map_err(|error| error.to_string())?;
            publication_seconds += publish_started.elapsed().as_secs_f64();
            samples.push(output_word(world));
        }
    }
    Ok((
        tick_only_seconds,
        publication_seconds,
        started.elapsed().as_secs_f64(),
        samples,
        control_changes,
    ))
}

fn load(path: &Path) -> (PlotWorld, (BlockPos, BlockPos)) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "b6182c6c0541d692d635d72d23ecc8f33cef2fb6071f4ba4be66485ff6892bc1"
    );
    let mut clip = load_schematic(File::open(path).unwrap()).unwrap();
    assert_eq!((clip.size_x, clip.size_y, clip.size_z), (171, 43, 107));
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(&mut world, &clip, ORIGIN, false);
    let bounds = (
        ORIGIN,
        ORIGIN + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    for pos in INPUTS.into_iter().chain(OPCODE) {
        assert!(
            matches!(world.get_block(ORIGIN + pos), Block::Lever { lever } if !lever.powered),
            "expected unpowered input lever at {pos:?}"
        );
    }
    assert!(matches!(world.get_block(ORIGIN + TRIGGER), Block::Lever { lever } if !lever.powered));
    for pos in OUTPUTS {
        assert!(
            matches!(world.get_block(ORIGIN + pos), Block::RedstoneWire { .. }),
            "expected dust at {pos:?}"
        );
    }
    (world, bounds)
}

fn compile(
    world: PlotWorld,
    bounds: (BlockPos, BlockPos),
    ticks: Vec<TickEntry>,
    timeout: Duration,
) -> (Compiler, PlotWorld, Result<(), CompileError>, f64, bool) {
    let monitor = Arc::new(TaskMonitor::default());
    let worker_monitor = monitor.clone();
    let started = Instant::now();
    let (compiler, world, result, timed_out) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let mut compiler = Compiler::default();
            let result = compiler.compile(&world, bounds, CompilerOptions::default(), ticks, worker_monitor);
            (compiler, world, result)
        });
        let mut timed_out = false;
        while !worker.is_finished() {
            if started.elapsed() >= timeout && !monitor.cancelled() {
                timed_out = true;
                monitor.cancel();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let (compiler, world, result) = worker.join().unwrap();
        (compiler, world, result, timed_out)
    });
    (compiler, world, result, started.elapsed().as_secs_f64(), timed_out)
}

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/FPU_27.schem");
    let mut reports = Vec::new();
    for sample in 1..=3 {
        let native_import_started = Instant::now();
        let (mut native, bounds) = load(&path);
        let native_import_seconds = native_import_started.elapsed().as_secs_f64();
        let compiler_import_started = Instant::now();
        let (mut compiler_world, _) = load(&path);
        let compiler_import_seconds = compiler_import_started.elapsed().as_secs_f64();
        let ticks: Vec<TickEntry> = compiler_world.scheduler().iter_entries().collect();
        let timeout = Duration::from_secs(
            std::env::var("MCHPRS_REDPILER_COMPILE_TIMEOUT_SECS")
                .ok()
                .and_then(|seconds| seconds.parse().ok())
                .unwrap_or(300),
        );
        let (mut compiler, mut compiler_world, result, compile_seconds, timed_out) = compile(compiler_world, bounds, ticks.clone(), timeout);
        let report = match result {
            Ok(()) => {
                let (native_seconds, native_trace, native_control_changes) = run_native(&mut native);
                let compiled = run_compiled(&mut compiler, &mut compiler_world, true);
                match compiled {
                    Ok((
                        compiled_tick_only_seconds,
                        compiled_publication_seconds,
                        compiled_tick_flush_seconds,
                        compiled_trace,
                        compiled_control_changes,
                    )) => {
                        let output_changed = native_trace.first().is_some_and(|first| native_trace.iter().any(|value| value != first));
                        let compiled_output_changed = compiled_trace
                            .first()
                            .is_some_and(|first| compiled_trace.iter().any(|value| value != first));
                        let native_active = native_control_changes > 0 || output_changed;
                        let compiled_active = compiled_control_changes > 0 || compiled_output_changed;
                        json!({
                            "sample":sample, "status":"admitted", "compile_seconds":compile_seconds,
                            "compile_timeout_seconds":timeout.as_secs(), "compile_timed_out":timed_out,
                            "native_import_seconds":native_import_seconds, "compiler_import_seconds":compiler_import_seconds,
                            "components":compiler.stats().unwrap().components,
                            "native_tick_seconds":native_seconds, "native_ticks":WINDOW_TICKS,
                            "native_tps":if native_active { Some(WINDOW_TICKS as f64 / native_seconds) } else { None },
                            "native_control_changes":native_control_changes, "native_active":native_active,
                            "compiled_tick_flush_seconds":compiled_tick_flush_seconds,
                            "compiled_tick_flush_tps":if compiled_active { Some(WINDOW_TICKS as f64 / compiled_tick_flush_seconds) } else { None },
                            "compiled_publication_seconds":compiled_publication_seconds,
                            "compiled_control_changes":compiled_control_changes, "compiled_active":compiled_active,
                            "compiled_tick_only_seconds":compiled_tick_only_seconds,
                            "compiled_tick_only_tps":if compiled_control_changes > 0 || compiled_output_changed { Some(WINDOW_TICKS as f64 / compiled_tick_only_seconds) } else { None },
                            "sampled_every_tick":true, "native_outputs":native_trace,
                            "compiled_outputs":compiled_trace,
                            "first_divergent_tick":native_trace.iter().zip(&compiled_trace).position(|(a,b)|a!=b).map(|i|i+1),
                            "output_changed":output_changed,
                            "compiled_output_changed":compiled_output_changed,
                            "falling_edge_parity":"unverified: clock coordinates not found in retained protocol",
                            "physical_mask_order":"35-bit mask, most-significant bit maps to first opcode-4 lever; opcode 4,2,1 then interleaved operand levers in manifest order; no IEEE interpretation asserted"
                        })
                    }
                    Err(error) => json!({"status":"runtime_error", "phase":"tick_or_publication", "error":error}),
                }
            }
            Err(error) => {
                let (position, block) = match &error {
                    CompileError::Unsupported(issue) => issue
                        .position
                        .map_or((None, None), |pos| (Some(pos), Some(format!("{:?}", native.get_block(pos))))),
                    _ => (None, None),
                };
                let (native_seconds, native_trace, native_control_changes) = run_native(&mut native);
                let output_changed = native_trace.first().is_some_and(|first| native_trace.iter().any(|value| value != first));
                let native_active = native_control_changes > 0 || output_changed;
                json!({
                    "sample":sample, "status":"rejected", "compile_seconds":compile_seconds,
                    "native_import_seconds":native_import_seconds, "compiler_import_seconds":compiler_import_seconds,
                    "compile_timeout_seconds":timeout.as_secs(), "compile_timed_out":timed_out,
                    "phase":"compile", "reason":error.to_string(),
                    "position":position, "block_at_position":block,
                    "native_tick_seconds":native_seconds, "native_ticks":WINDOW_TICKS,
                    "native_tps":if native_active { Some(WINDOW_TICKS as f64 / native_seconds) } else { None },
                    "native_control_changes":native_control_changes, "native_active":native_active,
                    "native_outputs":native_trace, "output_changed":output_changed,
                    "compiled_tps":null
                })
            }
        };
        reports.push(report);
        if reports.last().unwrap()["status"] != "admitted" {
            break;
        }
    }
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mair-six-build-bench/fpu.json");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&json!({"fixture_sha256":"b6182c6c0541d692d635d72d23ecc8f33cef2fb6071f4ba4be66485ff6892bc1", "dimensions":[171,43,107], "trigger_held_off":true, "raw_physical_masks":RAW_MASKS, "window_ticks":WINDOW_TICKS, "repetitions_per_mask":2, "reports":reports})).unwrap()).unwrap();
    println!("wrote {}", output.display());
}
