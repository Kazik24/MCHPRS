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
