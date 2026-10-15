//! Counterexamples to erasing recognized reset notifications without a replacement.
use super::*;
use rustc_hash::FxHashSet;

fn reset_erasure_backend(circuit: &Adder, world: &PlotWorld, erase: bool) -> (GraphBackend, usize) {
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(3),
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let mechanisms = passes::utils::instant_mechanisms::discover_mechanisms(&prepared.graph, &monitor).unwrap();
    assert_eq!(mechanisms.len(), 13);
    let observers: FxHashSet<_> = mechanisms.iter().map(|mechanism| mechanism.observer).collect();
    assert_eq!(observers.len(), 9);
    // This deliberately invalid rewrite must not retain executable certificates for the old rows.
    prepared.graph.instant_proxies.clear();
    prepared.graph.instant_regions.clear();
    let mut removed = 0;
    if erase {
        for route in &mut prepared.graph.routes {
            let old = std::mem::take(&mut route.deliveries);
            route.deliveries = old
                .into_vec()
                .into_iter()
                .filter(|delivery| {
                    let receiver = delivery.receiver.or_else(|| match prepared.graph.sites[delivery.target.index()].source {
                        SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                        _ => None,
                    });
                    let reset = matches!(delivery.kind, DeliveryKind::Update { .. })
                        && matches!(receiver, Some(CallbackReceiver::Component(node)) if observers.contains(&node));
                    removed += usize::from(reset);
                    !reset
                })
                .collect();
        }
        prepared
            .graph
            .links
            .retain(|link| !matches!(link.kind, LinkKind::Update { .. }) || !observers.contains(&link.to));
        assert!(removed > 0);
    }
    // Both controls use linear selection so route-offset invalidation cannot explain a difference.
    prepared.graph.turbo_callback_indices.clear();
    // Structural validity cannot establish semantic equivalence of a rewrite.
    prepared.graph.validate().unwrap();
    let lowered = prepared.lower(&monitor).unwrap();
    (GraphBackend::from_lowered(lowered, &monitor).unwrap(), removed)
}

#[test]
fn deleting_recognized_adder_reset_notifications_changes_actor_state_and_future_work() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let mut divergent_cases = 0;
    let mut output_divergences = 0;
    let mut hidden_future_divergences = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let mut native = circuit.build();
        let (mut exact, _) = reset_erasure_backend(&circuit, &native, false);
        let (mut erased, removed) = reset_erasure_backend(&circuit, &native, true);
        let mut exact_projection = circuit.build();
        let mut erased_projection = circuit.build();
        let mut steps = case_steps(&circuit, case, false, true);
        steps.extend(std::iter::repeat_n(Step::Tick, 32));
        let mut first_actor_difference = None;
        let mut first_future_difference = None;
        let mut first_output_difference = None;
        let positions = circuit.positions();
        let output = circuit.port_positions("observations", "sum_payload")[0];
        for (index, &step) in steps.iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => {
                    matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered)
                }
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut exact, step, toggle).unwrap();
            graph_step(&mut erased, step, toggle).unwrap();
            check_boundary(&exact, &native, &mut exact_projection, &positions, "unmodified control").unwrap();
            project(&erased, &mut erased_projection);
            if exact.state.pistons != erased.state.pistons || exact.state.resources != erased.state.resources {
                first_actor_difference.get_or_insert(index);
            }
            if future(&exact_projection, &positions).scheduled != future(&erased_projection, &positions).scheduled {
                first_future_difference.get_or_insert(index);
            }
            if exact_projection.get_block(output) != erased_projection.get_block(output) {
                first_output_difference.get_or_insert((index, exact_projection.get_block(output), erased_projection.get_block(output)));
            }
        }
        println!(
            "{}: removed {removed} reset deliveries; first future difference {first_future_difference:?}, actor/resource difference {first_actor_difference:?}, sum-cell difference {first_output_difference:?}",
            case["id"]
        );
        divergent_cases += usize::from(first_actor_difference.is_some() && first_future_difference.is_some());
        output_divergences += usize::from(first_output_difference.is_some());
        hidden_future_divergences += usize::from(first_future_difference.is_some() && first_output_difference.is_none());
    }
    assert_eq!(
        divergent_cases, 7,
        "the all-zero history stays quiet; the other histories need reset behavior"
    );
    assert_eq!(output_divergences, 5, "sum-cell counterexamples must survive the full O3 pipeline");
    assert_eq!(hidden_future_divergences, 2, "a matching sum alone cannot certify reset erasure");
}
