use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld, worldedit::load_schematic};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{
    BlockDirection, BlockFace, BlockFacing, BlockPos,
    block_entities::{BlockEntity, CommandBlockEntity},
    blocks::{Block, ButtonFace, Instrument, RedstonePiston, StoneButton},
};
use std::sync::Arc;
use std::time::Instant;

#[allow(dead_code)]
#[path = "../../../test_support/cpus.rs"]
mod cpus;

fn fixture() -> (PlotWorld, BlockPos, (BlockPos, BlockPos)) {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    world.disable_command_output_limits_for_replay();
    let button = BlockPos::new(15, 15, 15);
    world.set_block(button.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        button,
        Block::StoneButton {
            button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        button.offset(BlockFace::East),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(button.offset(BlockFace::East) + BlockPos::new(1, 0, 0), Block::RedstoneBlock {});
    world.set_block(
        button.offset(BlockFace::North),
        Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 1,
            powered: false,
        },
    );
    let command = button.offset(BlockFace::South);
    world.set_block(command, Block::from_name("command_block").unwrap());
    world.set_block_entity(
        command,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say checkpoint".into(),
            ..Default::default()
        })),
    );
    (world, button, (BlockPos::new(10, 10, 10), BlockPos::new(24, 20, 20)))
}

fn pending(compiler: &Compiler) -> Vec<mchprs_world::TickEntry> {
    compiler.llvm_runtime.as_ref().map_or_else(
        || compiler.runtime.as_ref().unwrap().pending_entries(),
        |native| native.pending_entries().unwrap(),
    )
}

fn checkpoint(world: &PlotWorld) -> cpus::Checkpoint {
    let chat = world.command_output().map(|message| (0, message.to_owned())).collect::<Vec<_>>();
    cpus::checkpoint(world, 0, &chat)
}

#[test]
fn executor_cache_resumes_pending_motion_and_effects_like_uninterrupted_execution() {
    for llvm in [false, true] {
        if llvm && !cfg!(any(windows, target_os = "linux")) {
            continue;
        }
        let (mut world, button, bounds) = fixture();
        let mut reference = world.activation_snapshot();
        let options = CompilerOptions {
            llvm,
            publication: Some(PublicationMode::All),
            ..Default::default()
        };
        let mut compiler = Compiler::default();
        let mut uninterrupted = Compiler::default();
        for (compiler, world) in [(&mut compiler, &world), (&mut uninterrupted, &reference)] {
            compiler.compile(world, bounds, options.clone(), vec![], Default::default()).unwrap();
            compiler.try_on_use_block(button).unwrap();
            compiler.tick().unwrap();
        }
        uninterrupted.try_flush_complete(&mut reference).unwrap();
        let expected_pending = pending(&uninterrupted);
        assert!(!expected_pending.is_empty(), "button release must survive the checkpoint");
        assert!(
            uninterrupted.runtime.as_ref().unwrap().has_motions(),
            "fixture must suspend during active piston motion"
        );
        let program = compiler.cached_program().unwrap();
        let cached = compiler
            .try_suspend(&mut world, bounds)
            .unwrap()
            .expect("ordinary reset should retain an executor");
        assert!(!compiler.is_active());
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), expected_pending);
        assert_eq!(pending(&cached), expected_pending);
        assert!(
            cached.runtime.as_ref().unwrap().notes.is_empty(),
            "suspend must acknowledge pending note effects"
        );
        assert_eq!(world.command_output().count(), 1, "suspend must publish the pending command once");
        for entry in &expected_pending {
            reference.restore_scheduled_tick(entry.clone());
        }
        assert_eq!(checkpoint(&world), checkpoint(&reference));
        let before_resume = checkpoint(&world);
        compiler.resume_cached(&world, options, Default::default(), cached).unwrap();
        assert_eq!(checkpoint(&world), before_resume, "resume must not mutate exported world state");
        assert!(compiler.stats().unwrap().cached);
        assert!(Arc::ptr_eq(&program, &compiler.cached_program().unwrap()));
        assert_eq!(pending(&compiler), expected_pending, "do not import the exported scheduler twice");
        // Plot activation transfers scheduler ownership back to the retained executor.
        world.clear_scheduled_ticks();
        reference.clear_scheduled_ticks();
        compiler.try_flush_complete(&mut world).unwrap();
        assert_eq!(world.command_output().count(), 1, "resume must not replay acknowledged effects");
        for tick in 1..=48 {
            if tick == 24 {
                assert!(matches!(world.get_block(button), Block::StoneButton { button } if !button.powered));
                compiler.try_on_use_block(button).unwrap();
                uninterrupted.try_on_use_block(button).unwrap();
            }
            compiler.tick().unwrap();
            uninterrupted.tick().unwrap();
            compiler.try_flush_complete(&mut world).unwrap();
            uninterrupted.try_flush_complete(&mut reference).unwrap();
            assert_eq!(pending(&compiler), pending(&uninterrupted), "llvm={llvm}, continuation tick {tick}");
            assert_eq!(checkpoint(&world), checkpoint(&reference), "llvm={llvm}, continuation tick {tick}");
        }
        assert_eq!(world.command_output().count(), 2, "a new input edge must still execute after resume");
    }
}

#[test]
fn executor_cache_excludes_update_and_rejects_expired_admission() {
    let (mut world, _, bounds) = fixture();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                update: true,
                ..Default::default()
            },
            vec![],
            Default::default(),
        )
        .unwrap();
    assert!(compiler.try_suspend(&mut world, bounds).unwrap().is_none());
    assert!(!compiler.is_active());
    assert!(compiler.try_suspend(&mut world, bounds).unwrap().is_none());

    for incompatible_update in [false, true] {
        compiler
            .compile(&world, bounds, CompilerOptions::default(), vec![], Default::default())
            .unwrap();
        let cached = compiler.try_suspend(&mut world, bounds).unwrap().unwrap();
        let before = checkpoint(&world);
        let monitor = Arc::new(TaskMonitor::with_deadline((!incompatible_update).then(Instant::now)));
        let options = CompilerOptions {
            update: incompatible_update,
            ..Default::default()
        };
        let error = compiler.resume_cached(&world, options, monitor, cached).unwrap_err();
        assert!(if incompatible_update {
            matches!(error, CompileError::Unsupported(_))
        } else {
            matches!(error, CompileError::Cancelled)
        });
        assert!(!compiler.is_active());
        assert_eq!(checkpoint(&world), before, "rejected admission must preserve the exported world");
    }
}

fn run_until_quiet(compiler: &mut Compiler, limit: u32) -> (u32, bool, std::time::Duration) {
    let mut quiet = 0;
    let mut kernel = std::time::Duration::ZERO;
    for tick in 1..=limit {
        let started = Instant::now();
        compiler.tick().unwrap();
        kernel += started.elapsed();
        let runtime = compiler.runtime.as_ref().unwrap();
        quiet = if runtime.pending_entries().is_empty() && runtime.pending_piston_event_iter().next().is_none() && !runtime.has_motions() {
            quiet + 1
        } else {
            0
        };
        if quiet == 20 {
            return (tick, true, kernel);
        }
        if tick % 10_000 == 0 {
            eprintln!("ZCore cache run: {tick}/{limit} ticks; twenty quiet ticks not reached");
        }
    }
    (limit, false, kernel)
}

#[test]
#[ignore = "ZCore cold compile, bounded run to quiescence, reset and warm executor admission benchmark"]
fn zcore_executor_cache_lifecycle_benchmark() {
    let cpu = cpus::CPUS[3];
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let options = CompilerOptions {
        publication: Some(PublicationMode::Io),
        ..Default::default()
    };
    let (mut world, mut compiler, cold_compile) = cpus::compile_cpu_with_options(cpus::load_cpu(cpu), cpu, options.clone());
    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
    let (load_ticks, load_settled, load_kernel) = run_until_quiet(&mut compiler, 4096);
    assert!(load_settled, "ZCore Load did not reach twenty quiet ticks");
    compiler.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
    let (run_ticks, completed, run_kernel) = run_until_quiet(&mut compiler, 250_000);
    let started = Instant::now();
    let cached = compiler.try_suspend(&mut world, bounds).unwrap().unwrap();
    let reset = started.elapsed();
    assert!(!compiler.is_active());
    let exported = checkpoint(&world);
    let mut fresh_world = world.activation_snapshot();
    let started = Instant::now();
    compiler.resume_cached(&world, options.clone(), Default::default(), cached).unwrap();
    let warm_compile = started.elapsed();
    assert!(compiler.stats().unwrap().cached);
    assert_eq!(checkpoint(&world), exported, "warm admission must preserve the full exported world");
    compiler.try_flush_complete(&mut world).unwrap();
    assert_eq!(
        checkpoint(&world),
        exported,
        "the first warm publication must reproduce the reset checkpoint"
    );
    eprintln!("ZCore admission: cold={cold_compile:?}, reset={reset:?}, warm={warm_compile:?}, completed={completed}");
    world.clear_scheduled_ticks();

    let mut fresh = Compiler::default();
    let ticks = fresh_world.scheduler().iter_entries().collect();
    let started = Instant::now();
    let fresh_result = fresh.compile(&fresh_world, bounds, options, ticks, Default::default());
    let fresh_compile = started.elapsed();
    let continuation = match fresh_result {
        Ok(()) => {
            fresh_world.clear_scheduled_ticks();
            let mut result = serde_json::json!({"ticks":64,"matched":true});
            for tick in 1..=64 {
                compiler.tick().unwrap();
                fresh.tick().unwrap();
                compiler.try_flush_complete(&mut world).unwrap();
                fresh.try_flush_complete(&mut fresh_world).unwrap();
                let (actual_pending, expected_pending) = (pending(&compiler), pending(&fresh));
                let (actual, expected) = (checkpoint(&world), checkpoint(&fresh_world));
                if actual_pending != expected_pending || actual != expected {
                    result = serde_json::json!({"ticks":tick,"matched":false,"resumed":actual,"fresh":expected,
                        "resumed_pending":actual_pending,"fresh_pending":expected_pending});
                    break;
                }
            }
            result
        }
        Err(error) => serde_json::json!({"ticks":0,"matched":null,"fresh_compile_rejection":error.to_string()}),
    };
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zcore-recovery-trials/cache-lifecycle.json");
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    let report = serde_json::json!({
        "fixture":cpu.schematic,"fixture_sha256":cpu.sha256,"backend":"rust",
        "protocol":"cold compile; Load; twenty quiet ticks; Run; twenty quiet ticks or 250000-tick cap; reset; exact executor resume",
        "cold_compile_ms":cold_compile.as_secs_f64()*1000.0,"load_ticks":load_ticks,"load_kernel_ms":load_kernel.as_secs_f64()*1000.0,
        "run_ticks":run_ticks,"run_tick_cap":250000,"completed":completed,"completion_criterion":"twenty consecutive ticks with no scheduled ticks, piston events or motion",
        "run_kernel_ms":run_kernel.as_secs_f64()*1000.0,"reset_ms":reset.as_secs_f64()*1000.0,
        "warm_compile_ms":warm_compile.as_secs_f64()*1000.0,"fresh_post_reset_compile_ms":fresh_compile.as_secs_f64()*1000.0,
        "exported_checkpoint":exported,"warm_checkpoint_equal":true,"fresh_continuation":continuation,
    });
    std::fs::write(&artifact, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!(
        "ZCore cache lifecycle: completed={completed}; cold={cold_compile:?}; warm={warm_compile:?}; {}",
        artifact.display()
    );
    assert_ne!(
        report["fresh_continuation"]["matched"].as_bool(),
        Some(false),
        "fresh continuation disagrees; see {}",
        artifact.display()
    );
}
