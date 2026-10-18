use super::*;

#[test]
fn reduction_keeps_ordered_duplicate_outputs_and_reset_acceptance_but_drops_dead_pins() {
    let mut region = super::super::tests::compile_synthetic_layer(0);
    let dead_input = LayerInputId(region.inputs.len() as u32);
    let mut inputs = region.inputs.to_vec();
    inputs.push(LayerInput {
        source: LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(99))),
        role: LayerInputRole::RequestSample,
    });
    region.inputs = inputs.into();
    let mut expressions = region.expressions.to_vec();
    expressions.push(LayerExpr::Input(dead_input));
    let dead_pin = LayerExprId(expressions.len() as u32 - 1);
    region.expressions = expressions.into();
    region.pin_values = vec![LayerPinValue {
        cell: 0,
        pin: CellPinId(0),
        value: dead_pin,
    }]
    .into();
    let accepted = region.layers[0].uses[0].accepted;
    region.outputs = vec![
        LayerOutput {
            port: 7,
            emitted_when: accepted,
        },
        LayerOutput {
            port: 7,
            emitted_when: accepted,
        },
    ]
    .into();
    let program = reduce(&region, &TaskMonitor::default()).unwrap();
    assert!(program.inputs.len() < region.inputs.len());
    assert!(program.expressions.len() < region.expressions.len());
    assert_eq!(
        program.roots.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        vec![
            LayerRoot::StationaryEntry,
            LayerRoot::Output(0),
            LayerRoot::Output(1),
            LayerRoot::Accepted { layer: 0, occurrence: 0 }
        ]
    );
    let mut scratch = vec![false; program.expressions.len()];
    program
        .evaluate(&mut scratch, |input| match input.source {
            LayerInputSource::PowerContribution { .. } => false,
            LayerInputSource::State(StateTest::ResourceEnabled(_)) => panic!("cold pin input survived"),
            _ => true,
        })
        .unwrap();
    assert!(program.roots.iter().all(|(_, root)| scratch[root.0 as usize]));
    assert!(program.evaluate(&mut [], |_| true).is_err());
}

#[test]
fn reduction_rejects_forward_references_and_checks_cancellation() {
    let monitor = TaskMonitor::default();
    let root = vec![(LayerRoot::StationaryEntry, LayerExprId(0))];
    assert!(reduce_roots(&[], &[LayerExpr::Not(LayerExprId(0))], root.clone(), &monitor).is_err());
    assert!(reduce_roots(&[], &[LayerExpr::Input(LayerInputId(0))], root.clone(), &monitor).is_err());
    assert!(reduce_roots(&[], &[], root.clone(), &monitor).is_err());
    monitor.cancel();
    assert!(
        reduce_roots(&[], &[LayerExpr::Constant(true)], root, &monitor)
            .unwrap_err()
            .contains("cancel")
    );
}

#[test]
fn generated_layer_programs_preserve_every_root_for_every_input_assignment() {
    let monitor = TaskMonitor::default();
    let inputs: Vec<_> = (0..6)
        .map(|index| LayerInput {
            source: LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(index))),
            role: LayerInputRole::RequestSample,
        })
        .collect();
    for seed in 1..=32 {
        let expressions = generated_expressions(seed);
        let roots = vec![
            (LayerRoot::StationaryEntry, LayerExprId(20)),
            (LayerRoot::Output(0), LayerExprId(65)),
            (LayerRoot::Output(1), LayerExprId(65)),
            (LayerRoot::Transfer(0), LayerExprId(71)),
            (LayerRoot::SourceAbsent(0), LayerExprId(82)),
            (LayerRoot::Accepted { layer: 0, occurrence: 0 }, LayerExprId(90)),
        ];
        let program = reduce_roots(&inputs, &expressions, roots.clone(), &monitor).unwrap();
        // Dense remapping is canonical, including when an expression is already compact.
        assert_eq!(
            program,
            reduce_roots(&program.inputs, &program.expressions, program.roots.to_vec(), &monitor).unwrap()
        );
        for mask in 0u8..64 {
            let expected = reference_values(&expressions, mask);
            let mut scratch = vec![false; program.expressions.len()];
            program
                .evaluate(&mut scratch, |input| match input.source {
                    LayerInputSource::State(StateTest::ResourceEnabled(resource)) => mask & (1 << resource.index()) != 0,
                    _ => unreachable!(),
                })
                .unwrap();
            for ((kind, reduced), (original_kind, original)) in program.roots.iter().zip(&roots) {
                assert_eq!(kind, original_kind);
                assert_eq!(
                    scratch[reduced.0 as usize], expected[original.0 as usize],
                    "seed={seed}, mask={mask}, root={kind:?}"
                );
            }
        }
    }
}

fn generated_expressions(mut seed: u32) -> Vec<LayerExpr> {
    let mut expressions: Vec<_> = (0..6).map(|index| LayerExpr::Input(LayerInputId(index))).collect();
    for _ in 6..96 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let first = LayerExprId(seed % expressions.len() as u32);
        let second = LayerExprId(seed.rotate_left(11) % expressions.len() as u32);
        let expression = match seed >> 29 {
            0 => LayerExpr::Constant(false),
            1 => LayerExpr::Constant(true),
            2 | 3 => LayerExpr::Not(first),
            4 | 5 => LayerExpr::All(vec![first, second].into()),
            _ => LayerExpr::Any(vec![first, second].into()),
        };
        expressions.push(expression);
    }
    expressions
}

fn reference_values(expressions: &[LayerExpr], mask: u8) -> Vec<bool> {
    let mut values: Vec<bool> = Vec::new();
    for expression in expressions {
        let value = match expression {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(input) => mask & (1 << input.0) != 0,
            LayerExpr::Not(input) => !values[input.0 as usize],
            LayerExpr::All(inputs) => inputs.iter().fold(true, |result, input| result & values[input.0 as usize]),
            LayerExpr::Any(inputs) => inputs.iter().fold(false, |result, input| result | values[input.0 as usize]),
        };
        values.push(value);
    }
    values
}
