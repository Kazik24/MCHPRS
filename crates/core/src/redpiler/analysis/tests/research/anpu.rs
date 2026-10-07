//! ANPU admission and visible-output checks against the unchanged Pong fixture.
use super::*;

#[test]
#[ignore = "ANPU admission matrix; opt-in larger fixture analysis"]
fn anpu_current_compile_admission() {
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let mut accepted = 0;
    for budget_multiplier in [1, 8] {
        for assume_instant in [false, true] {
            let mut compiler = Compiler::default();
            let result = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    budget_multiplier,
                    ..Default::default()
                },
                ticks.clone(),
                Default::default(),
            );
            match result {
                Ok(()) => {
                    accepted += 1;
                    assert!(compiler.is_active());
                    println!("ANPU budget={budget_multiplier}, assume_instant={assume_instant}: compiled; output comparison is a separate acceptance check");
                }
                Err(error) => {
                    println!("ANPU budget={budget_multiplier}, assume_instant={assume_instant}: {error}");
                    assert!(!compiler.is_active());
                    assert!(compiler.current_flags().is_none());
                }
            }
            assert_eq!(
                cpus::checkpoint(&world, 0, &[]),
                before,
                "admission must preserve physical state and scheduled work"
            );
        }
    }
    println!("ANPU admission: {accepted}/4 accepted; no compiled execution was compared by this probe");
}

#[test]
#[ignore = "ANPU compiled acceptance; requires actual admission and a 50,000-tick comparison"]
fn anpu_compiles_and_preserves_interpreted_screen() {
    let cpu = cpus::CPUS[1];
    let frozen = cpus::reference(cpu);
    let frames: Vec<cpus::ScreenFrame> = serde_json::from_slice(
        &std::fs::read(root().join("test_data/cpu-references/anpu_screen.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(frames.len(), 14);
    assert_eq!(frames.last().unwrap().tick, 3422);

    for assume_instant in [false, true] {
        let mut compiled = cpus::load_cpu(cpu);
        let mut interpreted = cpus::load_cpu(cpu);
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &compiled,
                compiled.get_corners(),
                CompilerOptions {
                    assume_instant,
                    budget_multiplier: 8,
                    ..Default::default()
                },
                compiled.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap_or_else(|error| {
                panic!("ANPU cannot enter compiled output comparison; assume_instant={assume_instant}: {error}")
            });

        let mut actual_frames = Vec::new();
        let mut expected_frames = Vec::new();
        let mut actual_chat = Vec::new();
        let mut expected_chat = Vec::new();
        cpus::collect_screen(&compiled, 0, &mut actual_frames);
        cpus::collect_screen(&interpreted, 0, &mut expected_frames);
        assert_eq!(cpus::checkpoint(&interpreted, 0, &[]), frozen.checkpoints[0]);
        compiler.on_use_block(cpu.origin + cpu.start);
        cpus::click_cpu(&mut interpreted, cpu, cpu.start);
        for tick in 1..=50_000 {
            compiler.tick_with_world(&mut compiled);
            compiler.flush(&mut compiled);
            interpreted.tick_interpreted();
            assert_eq!(
                cpus::screen(&compiled),
                cpus::screen(&interpreted),
                "ANPU screen at tick {tick}, assume_instant={assume_instant}"
            );
            cpus::collect_screen(&compiled, tick, &mut actual_frames);
            cpus::collect_screen(&interpreted, tick, &mut expected_frames);
            cpus::collect_chat(&compiled, tick, &mut actual_chat);
            cpus::collect_chat(&interpreted, tick, &mut expected_chat);
            assert_eq!(actual_chat, expected_chat, "ANPU command output at tick {tick}");
            if let Some(expected) = frozen.checkpoints.iter().find(|sample| sample.tick == tick) {
                assert_eq!(cpus::checkpoint(&interpreted, tick, &expected_chat), *expected);
            }
        }
        assert_eq!(expected_frames, frames, "unchanged interpreted screen reference");
        assert_eq!(actual_frames, frames, "compiled screen reference");
        assert_eq!(expected_chat, frozen.chat_trace);
        assert_eq!(actual_chat, frozen.chat_trace);
    }
}
