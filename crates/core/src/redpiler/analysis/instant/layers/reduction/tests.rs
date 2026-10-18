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
    monitor.cancel();
    assert!(
        reduce_roots(&[], &[LayerExpr::Constant(true)], root, &monitor)
            .unwrap_err()
            .contains("cancel")
    );
}
