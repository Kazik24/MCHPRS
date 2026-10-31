#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
fn frozen_cpu_inputs_and_initial_states() {
    for cpu in cpus::CPUS {
        let expected = cpus::reference(cpu);
        let world = cpus::load_cpu(cpu);
        assert_eq!(cpus::checkpoint(&world, 0, &[]), expected.checkpoints[0]);
    }
}

#[test]
fn bubblesort_native_initial_state_matches_reference() {
    let cpu = cpus::CPUS[2];
    assert_eq!(cpus::checkpoint(&cpus::load_cpu(cpu), 0, &[]), cpus::reference(cpu).checkpoints[0]);
}

fn replay_frozen(cpu: cpus::Cpu, redpiler: bool) {
    let expected = cpus::reference(cpu);
    let world = cpus::load_cpu(cpu);
    let (mut world, mut compiler) = if redpiler {
        let (world, compiler, _) = cpus::compile_cpu(world, cpu);
        (world, Some(compiler))
    } else {
        (world, None)
    };
    cpus::replay(&mut world, cpu, &expected, false, 0, redpiler, |world, step| match step {
        cpus::ReplayStep::Start => {
            if let Some(compiler) = &mut compiler {
                compiler.try_on_use_block(cpu.start + cpu.origin).unwrap();
            } else {
                cpus::click_cpu(world, cpu, cpu.start);
            }
        }
        cpus::ReplayStep::Tick(_) => {
            if let Some(compiler) = &mut compiler {
                compiler.tick().unwrap();
                compiler.try_flush(world).unwrap();
            } else {
                world.tick_interpreted();
            }
        }
        cpus::ReplayStep::StopTick => {
            if let Some(compiler) = &mut compiler {
                compiler.tick().unwrap();
                compiler.try_flush(world).unwrap();
            } else {
                world.tick_interpreted();
            }
        }
        cpus::ReplayStep::Stop => {
            let stop = cpu.stop.unwrap();
            if let Some(compiler) = &mut compiler {
                compiler.try_on_use_block(stop + cpu.origin).unwrap();
            } else {
                cpus::click_cpu(world, cpu, stop);
            }
        }
    });
}

#[test]
#[ignore = "50,000-tick compiled CPU admission gate; run explicitly"]
fn compiled_bubblesort_load_reset_start_checkpoints() {
    use mchprs_core::redpiler::{Compiler, CompilerOptions, TaskMonitor};
    use mchprs_core::world::World;
    use std::sync::Arc;

    let cpu = cpus::CPUS[2];
    let world = cpus::load_cpu(cpu);
    let pos = cpu.start + cpu.origin;
    let state = world.get_block(pos);
    println!("CPU_BUBBLESORT start control {pos:?}: {state:?}");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = mchprs_core::plot::worldedit::load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + mchprs_blocks::BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let mut inventory = std::collections::BTreeMap::new();
    mchprs_core::world::for_each_block_optimized(&world, bounds.0, bounds.1, |at| {
        *inventory.entry(world.get_block(at).get_name()).or_insert(0usize) += 1;
    });
    println!("CPU_BUBBLESORT block inventory: {inventory:?}");
    let timeout = std::time::Duration::from_secs(
        std::env::var("MCHPRS_REDPILER_COMPILE_TIMEOUT_SECS")
            .ok()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(300),
    );
    let monitor = Arc::new(TaskMonitor::default());
    let worker_monitor = monitor.clone();
    let ticks = world.scheduler().iter_entries().collect();
    let compile_started = std::time::Instant::now();
    let mut cancellation_requested = None;
    let (world, mut compiler, error, timed_out) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let mut compiler = Compiler::default();
            let result = compiler.compile(&world, bounds, CompilerOptions::default(), ticks, worker_monitor.clone());
            (world, compiler, result)
        });
        let mut timed_out = false;
        while !worker.is_finished() {
            if compile_started.elapsed() >= timeout && !monitor.cancelled() {
                timed_out = true;
                cancellation_requested = Some(std::time::Instant::now());
                monitor.cancel();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let (world, compiler, error) = worker.join().unwrap();
        (world, compiler, error, timed_out)
    });
    if timed_out {
        println!("CPU_BUBBLESORT compile timed out after {timeout:?}; cancellation latency {:?}", cancellation_requested.unwrap().elapsed());
    }
    if let Err(error) = error {
        let rejected = match &error {
            mchprs_core::redpiler::CompileError::Unsupported(scratch) => scratch.position,
            _ => None,
        };
        println!(
            "CPU_BUBBLESORT compile rejection: start={pos:?} ({state:?}), bounds={bounds:?}, blocked={rejected:?} ({})",
            rejected.map_or_else(|| "<none>".to_owned(), |at| format!("{:?}", world.get_block(at)))
        );
        panic!("CPU_BUBBLESORT admission failed");
    }
    println!("CPU_BUBBLESORT compiled: {:?}", compiler.stats().unwrap());
    let mut world = world;
    let expected = cpus::reference(cpu);
    assert_eq!(cpus::checkpoint(&world, 0, &[]), expected.checkpoints[0]);
    let mut native = cpus::load_cpu(cpu);
    cpus::click_cpu(&mut native, cpu, cpu.start);
    compiler.try_on_use_block(pos).unwrap();
    let mut chat = Vec::new();
    let mut ram = Vec::new();
    cpus::collect_ram(&world, cpu, 0, &mut ram);
    let mut checkpoint_index = 1;
    for tick in 1..=50_000 {
        compiler.tick().unwrap();
        compiler.try_flush(&mut world).unwrap();
        if tick <= 10 {
            native.tick_interpreted();
            let mut differences = Vec::new();
            mchprs_core::world::for_each_block_optimized(&native, bounds.0, bounds.1, |at| {
                let expected = native.get_block(at);
                let actual = world.get_block(at);
                if expected != actual && differences.len() < 20 {
                    differences.push((at, expected, actual));
                }
            });
            if !differences.is_empty() {
                eprintln!("native/compiled block differences at tick {tick}: {differences:?}");
            }
            let mut native_ram = Vec::new();
            let mut compiled_ram = Vec::new();
            cpus::collect_ram(&native, cpu, tick, &mut native_ram);
            cpus::collect_ram(&world, cpu, tick, &mut compiled_ram);
            assert_eq!(compiled_ram, native_ram, "native/compiled RAM at tick {tick}");
        }
        cpus::collect_chat(&world, tick, &mut chat);
        cpus::collect_ram(&world, cpu, tick, &mut ram);
        if cpus::CHECKPOINTS.contains(&tick) {
            let actual = cpus::checkpoint(&world, tick, &chat);
            let reference = &expected.checkpoints[checkpoint_index];
            eprintln!("tick {tick} published scheduler={} native scheduler={}", actual.scheduler, reference.scheduler);
            assert_eq!(
                (&actual.blocks, &actual.entities, &actual.pistons, &actual.chat, actual.messages, actual.lit_lamps),
                (&reference.blocks, &reference.entities, &reference.pistons, &reference.chat, reference.messages, reference.lit_lamps),
                "compiled CPU_BUBBLESORT checkpoint at tick {tick}"
            );
            checkpoint_index += 1;
        }
    }
    assert_eq!(ram, expected.ram_trace, "compiled CPU_BUBBLESORT per-tick RAM transitions");
    assert_eq!(chat, expected.chat_trace, "compiled CPU_BUBBLESORT command output");
}

#[test]
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn pm1_sort_frozen_reference() {
    replay_frozen(cpus::CPUS[0], false);
}

#[test]
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn anpu_pong_frozen_reference() {
    replay_frozen(cpus::CPUS[1], false);
}

#[test]
#[ignore = "50,000-tick CPU regression; run explicitly"]
fn bubblesort_frozen_reference() {
    replay_frozen(cpus::CPUS[2], false);
}

#[test]
#[ignore = "50,000-tick interpreter baseline; run explicitly"]
fn zcore12b_sierpinski_frozen_reference() {
    replay_frozen(cpus::CPUS[3], false);
}

#[test]
#[ignore = "full CPU Redpiler compile plus 50,000-tick replay; run explicitly"]
fn zcore12b_sierpinski_redpiler_matches_interpreter_reference() {
    replay_frozen(cpus::CPUS[3], true);
}

#[test]
#[ignore = "new-file BubbleSort baseline capture; MCHPRS_PISTON_RESEARCH_OUTPUT required"]
fn capture_bubblesort_reference() {
    let destination = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    let mut checkpoints = vec![cpus::checkpoint(&world, 0, &[])];
    let mut chat_trace = Vec::new();
    let mut ram_trace = Vec::new();
    cpus::collect_ram(&world, cpu, 0, &mut ram_trace);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    for tick in 1..=50_000 {
        world.tick_interpreted();
        cpus::collect_chat(&world, tick, &mut chat_trace);
        cpus::collect_ram(&world, cpu, tick, &mut ram_trace);
        if cpus::CHECKPOINTS.contains(&tick) {
            checkpoints.push(cpus::checkpoint(&world, tick, &chat_trace));
        }
    }
    assert_eq!(&ram_trace.last().unwrap().1[48..], (0..16).map(Some).collect::<Vec<_>>());
    let reference = cpus::Reference {
        schema: 1,
        schematic_sha256: cpu.sha256.into(),
        checkpoints,
        chat_trace,
        ram_trace,
    };
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(destination).unwrap();
    serde_json::to_writer_pretty(file, &reference).unwrap();
}
