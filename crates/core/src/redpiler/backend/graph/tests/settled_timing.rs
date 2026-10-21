//! Source-derived counter timing diagnostics. Run only when investigating clock ownership.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::program_graph::{LayerExpr, LayerExprId, LayerInputSource, ReducedLayerProgram};

fn input_leaves(program: &ReducedLayerProgram, root: LayerExprId) -> Vec<LayerInputSource> {
    let mut pending = vec![root];
    let mut seen = rustc_hash::FxHashSet::default();
    let mut inputs = rustc_hash::FxHashSet::default();
    while let Some(expr) = pending.pop() {
        if !seen.insert(expr) {
            continue;
        }
        match &program.expressions[expr.0 as usize] {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                inputs.insert(program.inputs[input.0 as usize].source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    inputs.into_iter().collect()
}

fn counter_graph() -> passes::rich::PreparedGraph {
    let (world, bounds) = super::adder_tests::counter_world();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let pipeline = passes::default_graph_pipeline(&options);
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    prepared
}

#[test]
fn counter_stable_clock_equations_hide_live_reset_state() {
    use crate::redpiler::program_graph::{InputPort, PowerTarget, SiteSource};

    let prepared = counter_graph();
    let graph = &prepared.graph;
    let monitor = TaskMonitor::default();
    let observers_of = |contact| {
        graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let crate::redpiler::program_graph::CircuitNodeKind::Observer { watched, .. } = node.kind else {
                    return None;
                };
                (graph.sites[watched.index()].source == SiteSource::Contact(contact)).then_some(NodeId::new(index as u32))
            })
            .collect::<Vec<_>>()
    };

    // Identify the timing piston from its empty lane and an actual observer on its base,
    // rather than encoding fixture coordinates or piston IDs in the test.
    let clocks = graph
        .pistons
        .iter()
        .enumerate()
        .filter_map(|(index, piston)| (piston.lane == PistonLane::Empty && !observers_of(piston.base).is_empty()).then_some((index, piston)))
        .collect::<Vec<_>>();
    assert_eq!(clocks.len(), 1, "expected one empty-lane piston with a base observer");
    let (_, clock) = clocks[0];
    let clock_observers = observers_of(clock.base);
    assert!(clock_observers.iter().any(|node| graph.nodes[node.index()].output_route.is_some()));

    let query_leaves = |node, port| -> Result<Vec<LayerInputSource>, String> {
        let (program, root) =
            crate::redpiler::analysis::instant::layers::logical::diagnostic_query(graph, PowerTarget::Input { node, port }, &monitor)?;
        Ok(input_leaves(&program, root))
    };
    let all_phase_raw_leaves = |node, port| -> Result<Vec<LayerInputSource>, String> {
        let (program, root) =
            crate::redpiler::analysis::instant::layers::logical::diagnostic_all_phase_raw_query(graph, PowerTarget::Input { node, port }, &monitor)?;
        Ok(input_leaves(&program, root))
    };
    let clock_main = query_leaves(clock.node, InputPort::Main).unwrap();
    let clock_quasi = query_leaves(clock.node, InputPort::Quasi).unwrap();
    let all_phase_clock_main = all_phase_raw_leaves(clock.node, InputPort::Main).unwrap();
    assert!(!clock_main.is_empty() && clock_main.iter().all(|leaf| matches!(leaf, LayerInputSource::SourceAbove { .. })));
    assert!(clock_quasi.iter().all(|leaf| matches!(leaf, LayerInputSource::SourceAbove { .. })));
    let quasi_site = clock_quasi
        .iter()
        .find_map(|leaf| match leaf {
            LayerInputSource::SourceAbove { site, .. } => Some(*site),
            _ => None,
        })
        .expect("clock Quasi must be driven by its observed source");
    let SiteSource::Component(observer) = graph.sites[quasi_site.index()].source else {
        panic!("clock Quasi source must be an observer output")
    };
    assert!(clock_observers.contains(&observer), "clock Quasi observer must watch the clock base");
    assert!(
        graph.nodes[observer.index()]
            .output_route
            .is_none_or(|route| graph.routes[route.index()].deliveries.is_empty()),
        "clock's self-watching observer has no callback output"
    );
    assert!(
        clock_observers.iter().any(|node| graph.nodes[node.index()]
            .output_route
            .is_some_and(|route| !graph.routes[route.index()].deliveries.is_empty())),
        "base transition also owns external observer callbacks"
    );
    // Stable-frame substitution erases live reset state; it cannot certify
    // that clock timing is independent of the physical controller in every phase.
    let reset_resources: FxHashSet<_> = all_phase_clock_main
        .iter()
        .filter_map(|leaf| match leaf {
            LayerInputSource::State(StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource)) => Some(*resource),
            _ => None,
        })
        .collect();
    assert!(!reset_resources.is_empty(), "all-phase clock query must retain live reset resource state");
    let bud_resources: FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.resource).collect();
    assert!(reset_resources.is_disjoint(&bud_resources));
}

#[test]
#[ignore = "prints the counter's logical and callback clock cone; diagnostic only"]
fn counter_clock_source_and_callback_cone() {
    use crate::redpiler::program_graph::{InputPort, PowerTarget, SettledTarget, SiteSource};

    let prepared = counter_graph();
    let monitor = TaskMonitor::default();
    let graph = &prepared.graph;
    let settled = crate::redpiler::analysis::instant::layers::logical::compile_settled(graph, &monitor).unwrap();

    println!(
        "counter timing diagnostic: nodes={} pistons={} resources={} inputs={} expressions={} outputs={} declines={:?}",
        graph.nodes.len(),
        graph.pistons.len(),
        graph.resources.len(),
        settled.program.inputs.len(),
        settled.program.expressions.len(),
        settled.outputs.len(),
        settled.declines
    );
    println!("entry scheduler boundary: {:#?}", prepared.entry);

    let piston_at = |x, y, z| {
        graph
            .pistons
            .iter()
            .enumerate()
            .find(|(_, piston)| (piston.position.x, piston.position.y, piston.position.z) == (x, y, z))
            .unwrap_or_else(|| panic!("counter fixture lacks piston at ({x},{y},{z})"))
    };
    let (clock_index, clock) = piston_at(42, 41, 59);
    let (aux_index, auxiliary) = piston_at(42, 42, 48);
    println!(
        "all-phase power/callback controller support candidate: {:?}",
        crate::redpiler::analysis::instant::layers::logical::controller_support(graph, PistonId::new(clock_index as u32), &monitor)
    );
    let clock_node = clock.node;
    let aux_node = auxiliary.node;
    println!(
        "clock piston: id={clock_index} spec={clock:?} state={:?}",
        graph.initial_pistons[clock_index]
    );
    println!(
        "auxiliary resource piston: id={aux_index} spec={auxiliary:?} state={:?}",
        graph.initial_pistons[aux_index]
    );

    for node in [clock_node, aux_node] {
        println!(
            "node {} {:?} at {:?}",
            node.index(),
            graph.nodes[node.index()].kind,
            graph.nodes[node.index()].position
        );
        for link in graph
            .power_links
            .iter()
            .filter(|link| matches!(link.to, PowerTarget::Input { node: target, .. } if target == node))
        {
            let source = graph.sites[link.from.index()];
            println!("  input link {link:?}; source={source:?}");
        }
        for route in graph.routes.iter().filter(|route| route.source == node) {
            println!("  route {}: {:?}", route.source.index(), route.deliveries);
        }
    }

    if let PistonLane::SingleResource { far } = auxiliary.lane {
        for (resource_index, resource) in graph
            .resources
            .iter()
            .enumerate()
            .filter(|(_, resource)| resource.contacts.contains(&far))
        {
            println!(
                "auxiliary far resource {resource_index}: spec={resource:?} state={:?}",
                graph.initial_resources[resource_index]
            );
            let contact = &graph.contacts[far.index()];
            println!("  far contact={contact:?}");
            for variant in contact.variants.iter() {
                println!("  variant guard {:?}: {:?}", variant.when, graph.guards[variant.when.index()]);
            }
        }
    }

    for (index, output) in settled.outputs.iter().enumerate() {
        let SettledTarget::Power(PowerTarget::Input { node, .. }) = output.target else {
            continue;
        };
        if node != clock_node && node != aux_node {
            continue;
        }
        let dependencies: Vec<_> = output.roots.iter().flat_map(|root| input_leaves(&settled.program, *root)).collect();
        println!(
            "settled output {index} target={:?} kind={:?} roots={:?} leaves={dependencies:?}",
            output.target, output.kind, output.roots
        );
    }

    for (node, port) in [
        (aux_node, InputPort::Main),
        (aux_node, InputPort::Quasi),
        (clock_node, InputPort::Main),
        (clock_node, InputPort::Quasi),
    ] {
        let target = PowerTarget::Input { node, port };
        match crate::redpiler::analysis::instant::layers::logical::diagnostic_query(graph, target, &monitor) {
            Ok((program, root)) => println!(
                "independent controller query {target:?}: leaves={:?} root={root:?}",
                input_leaves(&program, root)
            ),
            Err(error) => println!("independent controller query {target:?} declined: {error}"),
        }
    }

    let reset_nodes: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let watched = match node.kind {
                crate::redpiler::program_graph::CircuitNodeKind::BuiltinReset { watched, .. } => watched,
                _ => return None,
            };
            (graph.sites[watched.index()].source == SiteSource::Contact(auxiliary.base)).then_some(NodeId::new(index as u32))
        })
        .collect();
    println!("auxiliary built-in reset clocks: {reset_nodes:?}");
    for node in graph.nodes.iter().enumerate().filter_map(|(index, node)| {
        let crate::redpiler::program_graph::CircuitNodeKind::Observer { watched, facing } = node.kind else {
            return None;
        };
        let source = graph.sites[watched.index()].source;
        (source == SiteSource::Contact(clock.base) || source == SiteSource::Contact(auxiliary.base)).then_some((
            NodeId::new(index as u32),
            watched,
            facing,
            node.position,
        ))
    }) {
        println!("observer on clock/aux base: {node:?}");
        if let Some(route) = graph.nodes[node.0.index()].output_route {
            println!("  output route {:?}: {:?}", route, graph.routes[route.index()].deliveries);
        }
    }

    let mut local_nodes = rustc_hash::FxHashSet::default();
    local_nodes.insert(clock_node);
    local_nodes.insert(aux_node);
    local_nodes.extend(reset_nodes.iter().copied());
    local_nodes.extend(graph.nodes.iter().enumerate().filter_map(|(index, node)| {
        let crate::redpiler::program_graph::CircuitNodeKind::Observer { watched, .. } = node.kind else {
            return None;
        };
        matches!(graph.sites[watched.index()].source, SiteSource::Contact(contact) if contact == clock.base || contact == auxiliary.base)
            .then_some(NodeId::new(index as u32))
    }));
    let lowered = prepared.lower(&monitor).unwrap();
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    let trigger = backend
        .program
        .nodes()
        .iter()
        .find_map(|node| {
            matches!(node.kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever))
                .then_some(node.position)
                .flatten()
        })
        .expect("counter trigger");
    backend.use_block(trigger).unwrap();
    for step in 0..24 {
        backend.tick().unwrap();
        let tick = backend.future.clock.logical_tick;
        let pending: Vec<_> = backend
            .future
            .scheduler
            .iter()
            .filter(|(event, _, _)| event.node.is_some_and(|node| local_nodes.contains(&node)))
            .map(|(event, delay, priority)| (event.position, delay, priority, event.node))
            .collect();
        let local_requests: Vec<_> = backend
            .future
            .piston_requests
            .iter()
            .filter(|request| request.piston.index() == clock_index || request.piston.index() == aux_index)
            .collect();
        let local_deferred: Vec<_> = backend
            .future
            .deferred_pistons
            .iter()
            .filter(|event| event.piston.index() == clock_index || event.piston.index() == aux_index)
            .collect();
        println!(
            "physical controller tick {tick} step={step}: clock={:?} auxiliary={:?} reset_clocks={:?} requests={local_requests:?} deferred={local_deferred:?} scheduled={pending:?}",
            backend.state.pistons[clock_index],
            backend.state.pistons[aux_index],
            reset_nodes
                .iter()
                .map(|node| (node, &backend.state.components[node.index()]))
                .collect::<Vec<_>>(),
        );
    }
}
