//! ANPU admission and visible-output checks against the unchanged Pong fixture.
use super::*;

#[test]
#[ignore = "ANPU admission matrix; opt-in larger fixture analysis"]
fn anpu_current_compile_admission() {
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
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
            let error = result.unwrap_err().to_string();
            println!("ANPU budget={budget_multiplier}, assume_instant={assume_instant}: {error}");
            if budget_multiplier == 1 {
                assert!(error.contains("dependency budget"), "{error}");
            } else if assume_instant {
                assert!(error.contains("logical piston admission failed"), "{error}");
            } else {
                assert!(error.contains("ordinary non-instant movement"), "{error}");
            }
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            assert_eq!(
                cpus::checkpoint(&world, 0, &[]),
                before,
                "admission must preserve physical state and scheduled work"
            );
        }
    }
    println!("ANPU admission: all four configurations rejected transactionally");
}

#[test]
fn anpu_rejects_mechanisms_without_certified_instant_boundaries() {
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    // A ready mechanism in the unchanged fixture has no recognized instant
    // response/reset contract. A larger analysis budget cannot certify it.
    for assume_instant in [false, true] {
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    budget_multiplier: 8,
                    ..Default::default()
                },
                ticks.clone(),
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("logical piston admission failed"), "{error}");
        assert!(
            error.contains("BlockPos"),
            "the ambiguous boundary must be located: {error}"
        );
        assert!(
            error.contains("sampling")
                || error.contains("response")
                || error.contains("owner")
                || error.contains("cycle"),
            "{error}"
        );
        assert!(
            !error.contains("--assume-instant"),
            "unrelated flag advice must not obscure the failure reason: {error}"
        );
        assert!(!compiler.is_active());
        assert!(compiler.current_flags().is_none());
        assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    }
}
