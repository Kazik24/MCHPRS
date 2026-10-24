use super::*;
use crate::redpiler::components::{NodeState, NodeType};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use std::process::{Command, Stdio};

fn empty_graph() -> CompileGraph {
    let mut graph = super::super::tests::empty_graph(PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: mchprs_blocks::BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    });
    graph.plans.instant.boundaries = Some(BoundaryCatalog::default());
    graph
}

fn equations<'a>(graph: &'a CompileGraph, monitor: &'a TaskMonitor, defer_sources: bool) -> Equations<'a> {
    let mut equations = Equations::new(graph, monitor).unwrap();
    equations.defer_sources = defer_sources;
    equations
}

fn input(equations: &mut Equations<'_>, index: u32) -> LayerExprId {
    let input = equations
        .raw
        .input(
            LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(index))),
            LayerInputRole::RequestSample,
        )
        .unwrap();
    equations.raw.input_expr(input).unwrap()
}

fn recursive_oracle(equations: &mut Equations<'_>, id: LayerExprId) -> Result<LayerExprId, ConstructionError> {
    equations.reduced.visit()?;
    if let Some(&value) = equations.expanded.get(&id) {
        return Ok(value);
    }
    let value = match equations.raw.expression(id)?.clone() {
        LayerExpr::Constant(value) => equations.reduced.constant(value)?,
        LayerExpr::Input(input) => equations.source(equations.raw.inputs[input.0 as usize].source)?,
        LayerExpr::Not(child) => {
            let child = recursive_oracle(equations, child)?;
            equations.reduced.not(child)?
        }
        LayerExpr::All(children) | LayerExpr::Any(children) => {
            let all = matches!(equations.raw.expression(id)?, LayerExpr::All(_));
            let values = children
                .iter()
                .map(|&child| recursive_oracle(equations, child))
                .collect::<Result<Vec<_>, _>>()?;
            equations.reduced.combine(values, all)?
        }
    };
    equations.expanded.insert(id, value);
    Ok(value)
}

fn build_shared_dag(equations: &mut Equations<'_>) -> LayerExprId {
    let a = input(equations, 1);
    let b = input(equations, 2);
    let not_a = equations.raw.intern(LayerExpr::Not(a)).unwrap();
    let shared = equations.raw.intern(LayerExpr::All(vec![a, b].into_boxed_slice())).unwrap();
    let mut children = vec![not_a, shared, b];
    children.extend(vec![shared; 2048]);
    equations.raw.intern(LayerExpr::Any(children.into_boxed_slice())).unwrap()
}

#[test]
fn iterative_expand_matches_recursive_order_memo_and_accounting() {
    let graph = empty_graph();
    let monitor = TaskMonitor::default();
    let mut iterative = equations(&graph, &monitor, true);
    let mut recursive = equations(&graph, &monitor, true);
    let iterative_root = build_shared_dag(&mut iterative);
    let recursive_root = build_shared_dag(&mut recursive);

    let iterative_value = iterative.expand(iterative_root).unwrap();
    let recursive_value = recursive_oracle(&mut recursive, recursive_root).unwrap();

    assert_eq!(iterative_value, recursive_value);
    assert_eq!(iterative.raw.expressions, recursive.raw.expressions);
    assert_eq!(iterative.raw.inputs, recursive.raw.inputs);
    assert_eq!(iterative.raw.budget.work, recursive.raw.budget.work);
    assert_eq!(iterative.raw.budget.bytes, recursive.raw.budget.bytes);
    assert_eq!(iterative.reduced.expressions, recursive.reduced.expressions);
    assert_eq!(iterative.reduced.inputs, recursive.reduced.inputs);
    assert_eq!(iterative.reduced.budget.work, recursive.reduced.budget.work);
    assert_eq!(iterative.reduced.budget.bytes, recursive.reduced.budget.bytes);
    assert_eq!(iterative.expanded, recursive.expanded);

    let iterative_work = iterative.reduced.budget.work;
    let recursive_work = recursive.reduced.budget.work;
    assert_eq!(iterative.expand(iterative_root).unwrap(), iterative_value);
    assert_eq!(recursive_oracle(&mut recursive, recursive_root).unwrap(), recursive_value);
    assert_eq!(iterative.reduced.budget.work - iterative_work, 1);
    assert_eq!(recursive.reduced.budget.work - recursive_work, 1);
}

fn deep_dag(recursive: bool) {
    const DEPTH: usize = 20_000;
    assert!(std::mem::size_of::<ExpandFrame>() <= 2 * std::mem::size_of::<LayerExpr>());
    let graph = empty_graph();
    let monitor = TaskMonitor::default();
    let mut equations = equations(&graph, &monitor, true);
    let mut root = input(&mut equations, 1);
    for index in 0..DEPTH {
        root = if index % 2 == 0 {
            equations.raw.intern(LayerExpr::Not(root)).unwrap()
        } else {
            equations.raw.intern(LayerExpr::Any(vec![root].into_boxed_slice())).unwrap()
        };
    }
    if recursive {
        recursive_oracle(&mut equations, root).unwrap();
    } else {
        equations.expand(root).unwrap();
    }
    assert_eq!(equations.expanded.len(), DEPTH + 1);
}

#[test]
fn deep_dag_expands_on_an_explicit_small_stack() {
    const CHILD: &str = "MCHPRS_EXPAND_SMALL_STACK_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let recursive = std::env::var_os("MCHPRS_EXPAND_RECURSIVE_CHILD").is_some();
        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || deep_dag(recursive))
            .unwrap()
            .join()
            .unwrap();
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .arg("expansion_tests::deep_dag_expands_on_an_explicit_small_stack")
        .env(CHILD, "1")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "small-stack expansion subprocess failed: {status}");
}

#[test]
#[ignore = "manual child-process probe for the recursive oracle's stack failure"]
fn recursive_oracle_stack_failure_probe() {
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("expansion_tests::deep_dag_expands_on_an_explicit_small_stack")
        .env("MCHPRS_EXPAND_SMALL_STACK_CHILD", "1")
        .env("MCHPRS_EXPAND_RECURSIVE_CHILD", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let windows_stack_overflow = output.status.code() == Some(0xc00000fdu32 as i32);
    assert!(
        stderr.contains("overflowed its stack") || stderr.contains("stack overflow") || windows_stack_overflow,
        "recursive child did not report stack overflow: status={}, stderr={stderr}",
        output.status
    );
}

#[test]
fn source_query_reentry_can_grow_raw_arena_while_expanding() {
    let mut graph = empty_graph();
    graph.nodes = vec![
        CircuitNodeDraft {
            position: None,
            raw_state: Some(Block::RedstoneLamp { lit: false }.get_id()),
            kind: CircuitNodeKind::Electrical(NodeType::Lamp),
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        },
        CircuitNodeDraft {
            position: None,
            raw_state: Some(Block::Lever { lever: Default::default() }.get_id()),
            kind: CircuitNodeKind::Electrical(NodeType::Lever),
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        },
        CircuitNodeDraft {
            position: None,
            raw_state: Some(Block::Lever { lever: Default::default() }.get_id()),
            kind: CircuitNodeKind::Electrical(NodeType::Lever),
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        },
    ];
    graph.sites = vec![
        SiteSpec {
            position: BlockPos::new(0, 0, 0),
            source: SiteSource::Component(NodeId::new(0)),
            source_strength: 0,
        },
        SiteSpec {
            position: BlockPos::new(1, 0, 0),
            source: SiteSource::Component(NodeId::new(1)),
            source_strength: 0,
        },
        SiteSpec {
            position: BlockPos::new(2, 0, 0),
            source: SiteSource::Component(NodeId::new(2)),
            source_strength: 0,
        },
    ];
    graph.power_links = vec![PowerLink {
        from: SiteId::new(1),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: mchprs_blocks::BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    }];
    let monitor = TaskMonitor::default();
    let mut iterative = equations(&graph, &monitor, false);
    let mut recursive = equations(&graph, &monitor, false);
    let lamp_source = LayerInputSource::SourceAbove {
        site: SiteId::new(0),
        port: PowerPort::Output,
        threshold: 0,
    };
    let other_source = LayerInputSource::SourceAbove {
        site: SiteId::new(2),
        port: PowerPort::Output,
        threshold: 0,
    };
    let lamp_input = iterative.raw.input(lamp_source, LayerInputRole::RequestSample).unwrap();
    let lamp = iterative.raw.input_expr(lamp_input).unwrap();
    let other_input = iterative.raw.input(other_source, LayerInputRole::RequestSample).unwrap();
    let other = iterative.raw.input_expr(other_input).unwrap();
    let iterative_root = iterative.raw.intern(LayerExpr::All(vec![lamp, other].into_boxed_slice())).unwrap();
    let lamp_input = recursive.raw.input(lamp_source, LayerInputRole::RequestSample).unwrap();
    let lamp = recursive.raw.input_expr(lamp_input).unwrap();
    let other_input = recursive.raw.input(other_source, LayerInputRole::RequestSample).unwrap();
    let other = recursive.raw.input_expr(other_input).unwrap();
    let recursive_root = recursive.raw.intern(LayerExpr::All(vec![lamp, other].into_boxed_slice())).unwrap();
    let initial_raw_len = iterative.raw.expressions.len();

    let iterative_value = iterative.expand(iterative_root).unwrap();
    let recursive_value = recursive_oracle(&mut recursive, recursive_root).unwrap();

    assert!(
        iterative.raw.expressions.len() > initial_raw_len,
        "nested query should append raw expressions"
    );
    assert_eq!(iterative.raw.expressions, recursive.raw.expressions);
    assert_eq!(iterative.raw.inputs, recursive.raw.inputs);
    assert_eq!(iterative.raw.budget.work, recursive.raw.budget.work);
    assert_eq!(iterative.raw.budget.bytes, recursive.raw.budget.bytes);
    assert_eq!(iterative.reduced.expressions, recursive.reduced.expressions);
    assert_eq!(iterative.reduced.inputs, recursive.reduced.inputs);
    assert_eq!(iterative.reduced.budget.work, recursive.reduced.budget.work);
    assert_eq!(iterative.reduced.budget.bytes, recursive.reduced.budget.bytes);
    assert_eq!(iterative.expanded, recursive.expanded);
    assert_eq!(iterative_value, recursive_value);
}

#[test]
fn expand_preserves_missing_reference_cancellation_and_budget_errors() {
    let graph = empty_graph();
    let monitor = TaskMonitor::default();
    let mut missing = equations(&graph, &monitor, true);
    assert!(matches!(
        missing.expand(LayerExprId(u32::MAX)),
        Err(ConstructionError::Invalid(message)) if message == "layer expression references a missing root"
    ));

    let mut missing_input = equations(&graph, &monitor, true);
    let invalid_input = LayerExprId(missing_input.raw.expressions.len() as u32);
    missing_input.raw.expressions.push(LayerExpr::Input(LayerInputId(u32::MAX)));
    assert!(matches!(
        missing_input.expand(invalid_input),
        Err(ConstructionError::Invalid(message)) if message == "logical expression references a missing input"
    ));

    let mut forward = equations(&graph, &monitor, true);
    let child = LayerExprId(forward.raw.expressions.len() as u32 + 1);
    let parent = forward.raw.expressions.len() as u32;
    forward.raw.expressions.push(LayerExpr::Not(child));
    forward.raw.expressions.push(LayerExpr::Constant(true));
    assert!(matches!(
        forward.expand(LayerExprId(parent)),
        Err(ConstructionError::Invalid(message)) if message == "logical expression references a non-prior root"
    ));

    let iterative_cancel_monitor = TaskMonitor::default();
    let recursive_cancel_monitor = TaskMonitor::default();
    let mut iterative_cancelled = equations(&graph, &iterative_cancel_monitor, true);
    let mut recursive_cancelled = equations(&graph, &recursive_cancel_monitor, true);
    let iterative_root = build_shared_dag(&mut iterative_cancelled);
    let recursive_root = build_shared_dag(&mut recursive_cancelled);
    iterative_cancel_monitor.cancel();
    recursive_cancel_monitor.cancel();
    let iterative_error = iterative_cancelled.expand(iterative_root).unwrap_err();
    let recursive_error = recursive_oracle(&mut recursive_cancelled, recursive_root).unwrap_err();
    assert!(matches!(&iterative_error, ConstructionError::Invalid(message) if message.contains("cancelled")));
    assert_eq!(iterative_error, recursive_error);
    assert_eq!(iterative_cancelled.reduced.budget.work, recursive_cancelled.reduced.budget.work);
    assert_eq!(iterative_cancelled.expanded, recursive_cancelled.expanded);

    let iterative_budget_monitor = TaskMonitor::default();
    let recursive_budget_monitor = TaskMonitor::default();
    let mut iterative_budgeted = equations(&graph, &iterative_budget_monitor, true);
    let mut recursive_budgeted = equations(&graph, &recursive_budget_monitor, true);
    let build_budget_root = |equations: &mut Equations<'_>| {
        let input = input(equations, 3);
        equations
            .raw
            .intern(LayerExpr::Any(vec![LayerExprId(0), input].into_boxed_slice()))
            .unwrap()
    };
    let iterative_root = build_budget_root(&mut iterative_budgeted);
    let recursive_root = build_budget_root(&mut recursive_budgeted);
    iterative_budgeted.reduced.budget.work = MAX_LAYER_WORK - 3;
    recursive_budgeted.reduced.budget.work = MAX_LAYER_WORK - 3;
    let iterative_error = iterative_budgeted.expand(iterative_root).unwrap_err();
    let recursive_error = recursive_oracle(&mut recursive_budgeted, recursive_root).unwrap_err();
    assert_eq!(iterative_error, ConstructionError::BudgetExceeded);
    assert_eq!(iterative_error, recursive_error);
    assert_eq!(iterative_budgeted.expanded, recursive_budgeted.expanded);
    assert_eq!(iterative_budgeted.reduced.expressions, recursive_budgeted.reduced.expressions);
    assert_eq!(iterative_budgeted.reduced.inputs, recursive_budgeted.reduced.inputs);
    assert_eq!(iterative_budgeted.reduced.budget.work, recursive_budgeted.reduced.budget.work);
}
