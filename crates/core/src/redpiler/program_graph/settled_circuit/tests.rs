use super::*;
use crate::redpiler::program_graph::{
    BlockFace, BudMemoryRead, ContactId, GuardId, LayerExpr, LayerExprId, LayerInput, LayerInputId, LayerInputRole, LayerInputSource, LayerRoot,
    PistonId, PowerLink, PowerPort, PowerTarget, ReducedLayerProgram, ResourceId, SiteId,
};

fn input_program(input_count: usize, expressions: Vec<LayerExpr>, roots: Vec<(LayerRoot, LayerExprId)>) -> ReducedLayerProgram {
    let inputs = (0..input_count)
        .map(|index| LayerInput {
            source: LayerInputSource::SourceAbove {
                site: SiteId::new(index as u32),
                port: PowerPort::Output,
                threshold: index as u8,
            },
            role: LayerInputRole::RequestSample,
        })
        .collect::<Vec<_>>();
    ReducedLayerProgram {
        inputs: inputs.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        roots: roots.into_boxed_slice(),
    }
}

fn power_output(kind: SettledValueKind, roots: Vec<LayerExprId>) -> SettledOutput {
    SettledOutput {
        target: SettledTarget::Power(PowerTarget::Conductor {
            site: SiteId::new(0),
            include_dust: true,
        }),
        kind,
        roots: roots.into_boxed_slice(),
    }
}

fn circuit(program: ReducedLayerProgram, outputs: Vec<SettledOutput>) -> SettledCircuit {
    SettledCircuit {
        program,
        outputs: outputs.into_boxed_slice(),
        firing: None,
        retained_cells: Box::new([]),
        retained_effects: Box::new([]),
        declines: Box::new([]),
        timing: None,
    }
}

#[test]
fn strength_outputs_preserve_all_sixteen_levels_and_boolean_threshold() {
    let expressions = (0..15).map(|input| LayerExpr::Input(LayerInputId(input))).collect::<Vec<_>>();
    let roots = (0..15).map(LayerExprId).collect::<Vec<_>>();
    let program = input_program(
        15,
        expressions,
        roots.iter().enumerate().map(|(index, &root)| (LayerRoot::Output(index), root)).collect(),
    );
    let circuit = circuit(
        program,
        vec![
            power_output(SettledValueKind::Strength, roots.clone()),
            power_output(SettledValueKind::Bool, vec![roots[0]]),
        ],
    );
    let mut evaluation = SettledEvaluation::new(&circuit).unwrap();

    for strength in 0u8..=15 {
        let inputs = (0..15).map(|threshold| strength > threshold).collect::<Vec<_>>();
        evaluation.evaluate(&inputs).unwrap();
        assert_eq!(evaluation.outputs(), &[strength, u8::from(strength > 0)], "strength {strength}");
    }
}

#[test]
fn unchanged_inputs_reuse_values_without_evaluating_operations() {
    let program = input_program(
        1,
        vec![LayerExpr::Input(LayerInputId(0)), LayerExpr::Not(LayerExprId(0))],
        vec![(LayerRoot::Output(0), LayerExprId(1))],
    );
    let circuit = circuit(program, vec![power_output(SettledValueKind::Bool, vec![LayerExprId(1)])]);
    let mut evaluation = SettledEvaluation::new(&circuit).unwrap();
    assert_eq!(evaluation.evaluate(&[false]).unwrap(), 2);
    assert_eq!(evaluation.outputs(), &[1]);
    assert_eq!(evaluation.evaluate(&[false]).unwrap(), 0);
    assert_eq!(evaluation.outputs(), &[1]);
}

#[test]
fn register_result_does_not_commit_or_mutate_sampled_inputs() {
    let read = BudMemoryRead {
        storage: PistonId::new(0),
        reader: PistonId::new(1),
        resource: ResourceId::new(0),
        source: ContactId::new(0),
        link: PowerLink {
            from: SiteId::new(0),
            from_port: PowerPort::Output,
            to: PowerTarget::Conductor {
                site: SiteId::new(0),
                include_dust: true,
            },
            face: BlockFace::Bottom,
            attenuation: 0,
            guard: GuardId::new(0),
        },
    };
    let program = input_program(
        1,
        vec![LayerExpr::Input(LayerInputId(0)), LayerExpr::Not(LayerExprId(0))],
        vec![(LayerRoot::Output(0), LayerExprId(1))],
    );
    let circuit = circuit(
        program,
        vec![SettledOutput {
            target: SettledTarget::Register(read),
            kind: SettledValueKind::Bool,
            roots: vec![LayerExprId(1)].into_boxed_slice(),
        }],
    );
    let mut evaluation = SettledEvaluation::new(&circuit).unwrap();
    let mut caller_state = [false];

    evaluation.evaluate(&caller_state).unwrap();
    assert_eq!(evaluation.outputs(), &[1]);
    assert_eq!(caller_state, [false], "evaluation must not publish the proposed register write");
    evaluation.evaluate(&caller_state).unwrap();
    assert_eq!(evaluation.outputs(), &[1], "the result remains based on caller-owned old state");

    caller_state[0] = evaluation.outputs()[0] != 0;
    evaluation.evaluate(&caller_state).unwrap();
    assert_eq!(evaluation.outputs(), &[0]);
}

#[test]
fn construction_rejects_invalid_output_bindings_and_program_roots() {
    let program = input_program(1, vec![LayerExpr::Input(LayerInputId(0))], vec![]);
    let short_strength = circuit(program.clone(), vec![power_output(SettledValueKind::Strength, vec![LayerExprId(0)])]);
    assert!(SettledEvaluation::new(&short_strength).unwrap_err().contains("invalid output binding"));

    let missing_expression = circuit(program, vec![power_output(SettledValueKind::Bool, vec![LayerExprId(1)])]);
    assert!(SettledEvaluation::new(&missing_expression)
        .unwrap_err()
        .contains("invalid output binding"));

    let invalid_program_root = input_program(1, vec![LayerExpr::Input(LayerInputId(0))], vec![(LayerRoot::Output(0), LayerExprId(9))]);
    let invalid_program_root = circuit(invalid_program_root, vec![]);
    assert!(SettledEvaluation::new(&invalid_program_root)
        .unwrap_err()
        .contains("missing root expression"));
}
