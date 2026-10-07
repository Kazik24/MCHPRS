use super::*;

fn output(world: &PlotWorld, fixture: &Value) -> u16 {
    fixture["observations"]["output_msb_first"]
        .as_array().unwrap().iter().fold(0, |word, pos| {
            let Block::RedstoneRepeater { repeater } = world.get_block(local_pos(pos) - BASE + origin(fixture)) else {
                panic!("missing divider output repeater");
            };
            (word << 1) | u16::from(!repeater.powered)
        })
}

#[test]
fn divider_compiles_without_assumptions_and_preserves_triggered_output() {
    let fixture = manifest("fpu_divider");
    let (mut compiled, bounds) = load(&fixture);
    let (mut interpreted, _) = load(&fixture);
    let mut compiler = Compiler::default();
    compiler.compile(&compiled, compiled.get_corners(), Default::default(), Vec::new(), Default::default()).unwrap();
    assert!(compiler.is_active());
    assert!(!compiler.current_flags().unwrap().assume_instant);
    assert_eq!(compiler.warnings().len(), 16);
    assert_eq!(output(&compiled, &fixture), 0);
    let trigger = origin(&fixture) + BlockPos::new(1,23,44);
    compiler.on_use_block(trigger);
    lever_action(&mut interpreted, trigger, false);
    let mut observed = std::collections::BTreeSet::new();
    let mut reference = std::collections::BTreeSet::new();
    for tick in 0..128 {
        compiler.tick();
        compiler.flush(&mut compiled);
        interpreted.tick_interpreted();
        observed.insert(output(&compiled, &fixture));
        reference.insert(output(&interpreted, &fixture));
        if tick < 16 { println!("divider tick {tick}: compiled {:010b}, interpreted {:010b}", output(&compiled, &fixture), output(&interpreted, &fixture)); }
    }
    assert_eq!(reference, [0, 170].into_iter().collect());
    assert_eq!(observed, reference);
    compiler.on_use_block(trigger);
    lever_action(&mut interpreted, trigger, true);
    for _ in 0..64 { compiler.tick(); compiler.flush(&mut compiled); interpreted.tick_interpreted(); }
    assert_eq!(output(&compiled, &fixture), 0);
    assert_eq!(output(&interpreted, &fixture), 0);
    compiler.reset(&mut compiled, bounds);
    for z in (12..=40).step_by(4) {
        for x in [14,15] {
            let head = origin(&fixture) + BlockPos::new(x,14,z);
            assert_eq!(compiled.get_block(head), Block::Air, "ignored head must stay absent at {head:?}");
        }
    }
}
