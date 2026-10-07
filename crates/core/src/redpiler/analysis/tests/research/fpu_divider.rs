use super::*;

fn output(world: &PlotWorld, fixture: &Value) -> u16 {
    fixture["observations"]["output_msb_first"]
        .as_array()
        .unwrap()
        .iter()
        .fold(0, |word, pos| {
            let Block::RedstoneRepeater { repeater } =
                world.get_block(local_pos(pos) - BASE + origin(fixture))
            else {
                panic!("missing divider output repeater");
            };
            (word << 1) | u16::from(!repeater.powered)
        })
}

#[test]
fn divider_compiles_without_assumptions_and_preserves_triggered_output() {
    check_divider("fpu_divider_legacy", 16);
}

#[test]
fn fixed_divider_compiles_without_warnings_and_preserves_triggered_output() {
    check_divider("fpu_divider", 0);
}

#[test]
fn fixed_divider_logical_execution_preserves_first_response_and_skips_stopped_domains() {
    let fixture = manifest("fpu_divider");
    for optimize in [false, true] {
        let (mut world, bounds) = load(&fixture);
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant: true,
                    optimize,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap();
        assert!(compiler.warnings().is_empty());
        assert!(!compiler
            .backend
            .as_ref()
            .unwrap()
            .logical_stats()
            .is_empty());
        assert!(compiler
            .backend
            .as_ref()
            .unwrap()
            .sampled_pistons()
            .is_empty());
        let trigger = local_pos(&fixture["observations"]["trigger"][0]) - BASE + origin(&fixture);
        for _ in 0..24 {
            compiler.tick_with_world(&mut world);
        }
        compiler.on_use_block(trigger);
        let mut observed = std::collections::BTreeSet::new();
        for _ in 0..128 {
            compiler.tick_with_world(&mut world);
            compiler.flush(&mut world);
            observed.insert(output(&world, &fixture));
            assert!(world.piston_state().motions.is_empty());
            assert!(world.piston_state().events.is_empty());
        }
        assert!(
            observed.contains(&170),
            "first saved divider response missing: {observed:?}"
        );
        compiler.on_use_block(trigger);
        for _ in 0..64 {
            compiler.tick_with_world(&mut world);
        }
        compiler.flush(&mut world);
        assert_eq!(output(&world, &fixture), 0);
        // OFF runs an ordinary diode/torch clock, whose changing outputs are
        // fresh domain inputs. ON stops that clock and freezes those inputs.
        let held = compiler.backend.as_ref().unwrap().logical_stats();
        for _ in 0..64 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(
            compiler.backend.as_ref().unwrap().logical_stats(),
            held,
            "a stopped clock with stable domain inputs must reuse cached decisions"
        );
        compiler.reset(&mut world, bounds);
        assert!(world.piston_state().motions.is_empty());
        assert!(world.piston_state().events.is_empty());
    }
}

fn check_divider(name: &str, warnings: usize) {
    let fixture = manifest(name);
    for optimize in [false, true] {
        for case in fixture["cases"].as_array().unwrap() {
            let (mut compiled, bounds) = load(&fixture);
            let (mut interpreted, _) = load(&fixture);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize,
                        io_only: optimize,
                        ..Default::default()
                    },
                    Vec::new(),
                    Default::default(),
                )
                .unwrap();
            assert!(compiler.is_active());
            assert!(!compiler.current_flags().unwrap().assume_instant);
            assert_eq!(compiler.warnings().len(), warnings);
            assert_eq!(output(&compiled, &fixture), 0);
            let mut observed = std::collections::BTreeSet::new();
            let mut actual_sequence = vec![0];
            let mut reference_sequence = vec![0];
            for operation in case["steps"].as_array().unwrap() {
                if let Some(ticks) = operation["advance"].as_u64() {
                    for _ in 0..ticks {
                        compiler.tick();
                        compiler.flush(&mut compiled);
                        interpreted.tick_interpreted();
                        let actual = output(&compiled, &fixture);
                        let expected = output(&interpreted, &fixture);
                        if actual_sequence.last() != Some(&actual) {
                            actual_sequence.push(actual);
                        }
                        if reference_sequence.last() != Some(&expected) {
                            reference_sequence.push(expected);
                        }
                        observed.insert(actual);
                    }
                } else if operation["op"] == "lever" {
                    let pos = local_pos(&operation["pos"]) - BASE + origin(&fixture);
                    let Block::Lever { lever } = interpreted.get_block(pos) else {
                        panic!("missing divider input lever");
                    };
                    if lever.powered != operation["powered"].as_bool().unwrap() {
                        compiler.on_use_block(pos);
                        action(&mut interpreted, &fixture, operation);
                    }
                }
            }
            if case["id"] == "enable-cycle" {
                // Original 1 / 1.5 response: 00 followed by 10101010.
                assert_eq!(observed, [0, 170].into_iter().collect());
            }
            assert_eq!(
                actual_sequence, reference_sequence,
                "case {}, optimize {optimize}",
                case["id"]
            );
            compiler.reset(&mut compiled, bounds);
            assert!(compiler.warnings().is_empty());
            for z in (12..=40).step_by(4) {
                for x in [14, 15] {
                    let head = origin(&fixture) + BlockPos::new(x, 14, z);
                    assert_eq!(
                        compiled.get_block(head),
                        interpreted.get_block(head),
                        "head geometry after case {} at {head:?}",
                        case["id"]
                    );
                }
            }
            actual_sequence = vec![output(&compiled, &fixture)];
            reference_sequence = vec![output(&interpreted, &fixture)];
            for _ in 0..48 {
                compiled.tick_interpreted();
                interpreted.tick_interpreted();
                let actual = output(&compiled, &fixture);
                let expected = output(&interpreted, &fixture);
                if actual_sequence.last() != Some(&actual) {
                    actual_sequence.push(actual);
                }
                if reference_sequence.last() != Some(&expected) {
                    reference_sequence.push(expected);
                }
            }
            assert_eq!(
                actual_sequence, reference_sequence,
                "handoff case {}, optimize {optimize}",
                case["id"]
            );
        }
    }
}
