use super::*;

#[test]
fn fixed_divider_rejects_unrepresented_feedback_sampling_transactionally() {
    let fixture = manifest("fpu_divider");
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (world, bounds) = load(&fixture);
            let before = compilation_fingerprint(&world, bounds);
            let mut compiler = Compiler::default();
            let error = compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("unsupported internally driven QC sampling interface"),
                "{error}"
            );
            assert!(error.contains("data source"), "{error}");
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            assert_eq!(compilation_fingerprint(&world, bounds), before);
        }
    }
}
