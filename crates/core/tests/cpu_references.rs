#[path = "../benches/support/cpus.rs"]
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
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn pm1_sort_frozen_reference() {
    cpus::replay_with_visuals(cpus::CPUS[0], &cpus::reference(cpus::CPUS[0]), false, 0);
}

#[test]
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn anpu_pong_frozen_reference() {
    cpus::replay_with_visuals(cpus::CPUS[1], &cpus::reference(cpus::CPUS[1]), false, 0);
}

#[test]
#[ignore = "50,000-tick CPU regression; run explicitly"]
fn bubblesort_frozen_reference() {
    cpus::replay_with_visuals(cpus::CPUS[2], &cpus::reference(cpus::CPUS[2]), false, 0);
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
    assert_eq!(
        &ram_trace.last().unwrap().1[48..],
        (0..16).map(Some).collect::<Vec<_>>()
    );
    let reference = cpus::Reference {
        schema: 1,
        schematic_sha256: cpu.sha256.into(),
        checkpoints,
        chat_trace,
        ram_trace,
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .unwrap();
    serde_json::to_writer_pretty(file, &reference).unwrap();
}
