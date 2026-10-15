//! Exact power-channel equivalence, independent of prepared arithmetic protocols.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::passes::Pass;

#[test]
fn compiled_hop_observations_match_all_adder_contact_alternatives() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let world = circuit.build();
    let mut backend = GraphBackend::compile(&world, circuit.bounds(), vec![], Arc::default()).unwrap();
    for phase in 0..32 {
        for (index, piston) in backend.state.pistons.iter_mut().enumerate() {
            piston.extended = (phase >> (index % 5)) & 1 != 0;
        }
        for (index, resource) in backend.state.resources.iter_mut().enumerate() {
            let contacts = &backend.program.resources()[index].contacts;
            resource.at = contacts[(phase + index) % contacts.len()];
            resource.enabled = (phase + index) % 3 != 0;
        }
        for index in 0..backend.program.sites().len() {
            let site = SiteId::new(index as u32);
            assert_eq!(
                backend.observe_site(site),
                backend.resolve_site(site, None).map(|site| site.observed()),
                "contact snapshot at site {site:?}, phase {phase}"
            );
        }
    }
}

#[test]
fn every_instant_adder_power_channel_matches_across_strengths_contacts_and_saved_state_bits() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let world = circuit.build();
    let mut graph = passes::rich::prepare(&world, circuit.bounds(), vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph;
    // Add independently guarded, attenuated contributors so this exercises the
    // rewritten graph's conjunctions, not only the adder's unconditional paths.
    let actor = graph.pistons[0];
    let outer = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(PistonId::new(0))));
    let inner = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(PistonId::new(1))));
    let input = graph
        .power_links
        .iter()
        .position(|link| {
            link.to
                == (PowerTarget::Input {
                    node: actor.node,
                    port: InputPort::Main,
                })
                && matches!(link.from_port, PowerPort::Conducted { .. })
        })
        .unwrap();
    graph.power_links[input].guard = outer;
    graph.power_links[input].attenuation = 3;
    let cap = graph.power_links[input].from;
    let PowerPort::Conducted { include_dust } = graph.power_links[input].from_port else {
        unreachable!()
    };
    let extra_wire = graph.wires[0].node;
    let extra_site = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(extra_wire))
        .unwrap();
    graph.power_links.push(PowerLink {
        from: SiteId::new(extra_site as u32),
        from_port: PowerPort::Stored,
        to: PowerTarget::Conductor { site: cap, include_dust },
        face: BlockFace::Top,
        attenuation: 5,
        guard: inner,
    });
    let mut specialized = graph.clone();
    let mechanisms = passes::utils::instant_mechanisms::discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
    assert_eq!(mechanisms.len(), 13);
    passes::run_graph_pipeline(
        &mut specialized,
        &CompilerOptions::default(),
        &EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        },
        &passes::PassPipeline::new(vec![passes::PassEntry {
            pass: Pass::rewrite(passes::specialize_instant_pistons::SpecializeInstantPistons),
            enabled: true,
        }]),
        &TaskMonitor::default(),
    )
    .unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(program, state, 0).unwrap();
    let (program, state) = specialized.lower().unwrap();
    let mut optimized = GraphBackend::new(program, state, 0).unwrap();
    for wire_level in 0u8..16 {
        for observer_level in 0u8..16 {
            for phase in 0..4 {
                for (index, state) in exact.state.components.iter_mut().enumerate() {
                    match state {
                        ComponentState::Wire(wire) => {
                            wire.power = if index == extra_wire.index() {
                                observer_level
                            } else {
                                (wire_level + (index % 16) as u8 * (observer_level % 2)) % 16
                            }
                        }
                        ComponentState::Electrical(state) => {
                            // Include mismatched powered/output snapshots: imported history can preserve them.
                            state.powered = (index + phase) % 2 == 0;
                            state.output_strength = (observer_level + (index % 16) as u8 * (wire_level % 2)) % 16;
                        }
                        ComponentState::Specialized => {}
                    }
                }
                for (index, piston) in exact.state.pistons.iter_mut().enumerate() {
                    piston.extended = (phase >> (index % 2)) & 1 != 0;
                }
                for (index, resource) in exact.state.resources.iter_mut().enumerate() {
                    let contacts = &exact.program.resources()[index].contacts;
                    resource.at = contacts[(phase + index) % contacts.len()];
                    resource.enabled = (phase + index) % 2 == 0;
                }
                optimized.state = exact.state.clone();
                for mechanism in &mechanisms {
                    let node = exact.program.pistons()[mechanism.piston.index()].node;
                    for port in [InputPort::Main, InputPort::Quasi] {
                        assert_eq!(
                            exact.input_strength(node, port),
                            optimized.input_strength(node, port),
                            "actor {:?} {port:?}, wire={wire_level}, observer={observer_level}, phase={phase}",
                            mechanism.piston
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn compiled_adder_wire_transfers_match_general_queries_before_and_after_contact_changes() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let world = circuit.build();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &TaskMonitor::default(),
    )
    .unwrap();
    assert_eq!(prepared.graph.wires.len(), 32);
    assert!(
        prepared
            .graph
            .wires
            .iter()
            .all(|wire| wire.execution.query_facts.unwrap().transfer.is_some())
    );
    let (program, state) = prepared.graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    let plans = backend.program.wires().to_vec();
    let mut comparisons = 0;
    for strength in 0..16 {
        for phase in 0..16 {
            for (index, component) in backend.state.components.iter_mut().enumerate() {
                match component {
                    ComponentState::Wire(wire) => wire.power = (strength + index as u8 % 16) % 16,
                    ComponentState::Electrical(state) => {
                        state.powered = (index + phase) % 2 != 0;
                        state.output_strength = (strength + (index % 16) as u8 * (phase % 2) as u8) % 16;
                    }
                    ComponentState::Specialized => {}
                }
            }
            for (index, actor) in backend.state.pistons.iter_mut().enumerate() {
                actor.extended = (phase + index) & 1 != 0;
                actor.head_present = (phase + index) & 2 != 0;
            }
            for (index, resource) in backend.state.resources.iter_mut().enumerate() {
                let contacts = &backend.program.resources()[index].contacts;
                resource.at = contacts[(phase + index) % contacts.len()];
                resource.enabled = (phase + index) & 2 != 0;
            }
            for plan in &plans {
                let mut general = plan.clone();
                general.execution.query_facts = None;
                assert_eq!(backend.seed_power(plan), backend.seed_power_general(&general));
                let mut walk = WireWalk::new();
                let root = backend.component_sites[plan.node.index()].unwrap();
                for site in plan.canonical_neighbors.into_iter().chain([root]) {
                    walk.insert_site(
                        site,
                        WalkSite {
                            site,
                            observed: backend.resolve_site(site, None).unwrap().observed(),
                            visited: site == root,
                            x_bias: 0,
                            z_bias: 0,
                            layer: 0,
                            heading: None,
                            neighbor_slots: None,
                        },
                    );
                }
                // Captured contacts/strengths must not follow subsequent live changes.
                for resource in &mut backend.state.resources {
                    resource.enabled = !resource.enabled;
                }
                for wire in &plans {
                    let ComponentState::Wire(state) = &mut backend.state.components[wire.node.index()] else {
                        unreachable!()
                    };
                    state.power = 15 - state.power;
                }
                for (x_bias, z_bias) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let root_slot = walk.site_slots[&root];
                    walk.sites[root_slot].heading = None;
                    walk.sites[root_slot].x_bias = x_bias;
                    walk.sites[root_slot].z_bias = z_bias;
                    let neighbors = backend.identify_wire_neighbors(plan.node, &mut walk);
                    assert_eq!(
                        backend.turbo_wire_power(plan, &neighbors, &walk),
                        backend.turbo_wire_power_general(&general, &neighbors, &walk),
                        "wire {:?}, strength {strength}, phase {phase}",
                        plan.node
                    );
                    comparisons += 1;
                }
            }
        }
    }
    assert_eq!(comparisons, 32 * 16 * 16 * 4);
}
