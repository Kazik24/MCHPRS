//! Independent boundary discovery checks on separate, fixture-derived devices.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant_regions::infer_stable_regions;
use crate::redpiler::program_graph::{
    Guard, GuardId, KernelBoundary, KernelGuardUse, KernelInput, KernelOutput, PowerLink, PowerPort, PowerTarget, SiteSource, StateTest,
};

fn independent_devices() -> (CompileGraph, NodeId) {
    let first = load_adder("adder_1bit", ORIGIN, None);
    let second = load_adder("adder_1bit", ORIGIN + BlockPos::new(48, 0, 0), None);
    let mut world = first.build();
    for (position, block, entity) in &second.cells {
        let position = second.place(*position);
        world.set_block(position, *block);
        if let Some(entity) = entity {
            world.set_block_entity(position, entity.clone());
        }
    }
    let bounds = (first.bounds().0.min(second.bounds().0), first.bounds().1.max(second.bounds().1));
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
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
    let control = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(first.trigger()))
        .unwrap();
    (prepared.graph, NodeId::new(control as u32))
}

#[test]
fn shared_external_power_does_not_merge_independent_instant_devices() {
    let (mut graph, control) = independent_devices();
    let monitor = TaskMonitor::default();
    let graph = &mut graph;
    let original = infer_stable_regions(graph, &monitor).unwrap();
    assert!(original.len() >= 2, "the separate fixtures must begin with independent owners");
    let ownership: Vec<_> = original.iter().map(|region| region.actors.clone()).collect();
    for region in &original {
        let sides: Vec<_> = region
            .actors
            .iter()
            .map(|actor| graph.pistons[actor.index()].position.x >= ORIGIN.x + 48)
            .collect();
        assert!(
            sides.iter().all(|side| *side == sides[0]),
            "a starting region must not span both fixtures"
        );
    }
    let source = graph.sites.iter().position(|site| site.source == SiteSource::Component(control)).unwrap();
    let always = graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap();
    let mut ports = Vec::new();
    for region in &original {
        let actor = region.actors[0];
        let link = graph.power_links.len() as u32;
        ports.push((actor, link));
        // Graph-level contributions isolate boundary classification from spatial wire parsing.
        graph.power_links.push(PowerLink {
            from: SiteId::new(source as u32),
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node: graph.pistons[actor.index()].node,
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(always as u32),
        });
    }
    let regions = infer_stable_regions(graph, &monitor).unwrap();
    assert_eq!(
        regions.iter().map(|region| region.actors.clone()).collect::<Vec<_>>(),
        ownership,
        "common input power is a boundary dependency and must preserve the original ownership partition"
    );
    for (actor, link) in ports {
        let region = regions.iter().find(|region| region.actors.contains(&actor)).unwrap();
        assert!(region.power_inputs.contains(&link));
        assert!(KernelBoundary::from_region(graph, region).inputs.contains(&KernelInput::Power(link)));
    }
}

fn region_owns_target(graph: &CompileGraph, region: &InstantRegion, target: PowerTarget) -> bool {
    match target {
        PowerTarget::Input { node, .. } => {
            region.resets.contains(&node)
                || region.wires.contains(&node)
                || region.actors.iter().any(|actor| graph.pistons[actor.index()].node == node)
                || region.contacts.iter().any(|contact| graph.contacts[contact.index()].node == node)
        }
        PowerTarget::Conductor { site, .. } => region.readable_sites.contains(&site),
    }
}

#[test]
fn external_power_edges_expose_their_reads_of_region_owned_guards() {
    let (mut graph, _) = independent_devices();
    let region = graph.plans.instant.regions[0].clone();
    let link = graph
        .power_links
        .iter()
        .position(|link| !region.readable_sites.contains(&link.from) && !region_owns_target(&graph, &region, link.to))
        .expect("the other device has external-to-external power contributions");
    let atom = StateTest::PistonExtended(region.actors[0]);
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(atom));
    graph.power_links[link].guard = guard;
    let boundary = KernelBoundary::from_region(&graph, &region);
    assert!(
        boundary.outputs.iter().any(|output| matches!(output,
            KernelOutput::Guard(read) if read.owner == KernelGuardUse::Power(link as u32)
                && read.guard == guard
        )),
        "a guard can observe an owned actor without either power edge endpoint being owned"
    );
    assert!(!boundary.inputs.iter().any(|input| matches!(input,
        KernelInput::Guard(read) if read.owner == KernelGuardUse::Power(link as u32)
            && read.guard == guard
    )));
}

#[test]
fn external_actor_guard_atoms_are_not_misclassified_as_owned_outputs() {
    let (mut graph, _) = independent_devices();
    let region = graph
        .plans
        .instant
        .regions
        .iter()
        .find(|region| !region.power_readers.is_empty())
        .unwrap()
        .clone();
    let external_actor = graph
        .plans
        .instant
        .regions
        .iter()
        .flat_map(|region| &region.actors)
        .copied()
        .find(|actor| !region.actors.contains(actor))
        .unwrap();
    let link = *region.power_readers.first().expect("the device has an ordinary external power consumer") as usize;
    let atom = StateTest::PistonExtended(external_actor);
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(atom));
    graph.power_links[link].guard = guard;
    let boundary = KernelBoundary::from_region(&graph, &region);
    assert!(
        !boundary.outputs.iter().any(|output| matches!(output,
            KernelOutput::Guard(read) if read.owner == KernelGuardUse::Power(link as u32)
            && read.guard == guard
        )),
        "an outside actor remains outside even when its guard controls a region output link"
    );
}

#[test]
fn internal_power_edges_keep_their_external_guard_dependencies() {
    let (mut graph, _) = independent_devices();
    let region = graph.plans.instant.regions[0].clone();
    let external_actor = graph
        .plans
        .instant
        .regions
        .iter()
        .flat_map(|region| &region.actors)
        .copied()
        .find(|actor| !region.actors.contains(actor))
        .unwrap();
    let link = graph
        .power_links
        .iter()
        .position(|link| region.readable_sites.contains(&link.from) && region_owns_target(&graph, &region, link.to))
        .expect("the device has internal power contributions");
    let owned = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(region.actors[0])));
    let external = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(external_actor)));
    let mixed = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::All(vec![owned, external].into_boxed_slice()));
    graph.power_links[link].guard = mixed;

    let boundary = KernelBoundary::from_region(&graph, &region);
    assert!(
        boundary.inputs.iter().any(|input| matches!(input,
            KernelInput::Guard(read) if read.owner == KernelGuardUse::Power(link as u32)
                && read.guard == mixed
        )),
        "internal endpoints must not hide an external state dependency"
    );
    assert!(
        !boundary.outputs.iter().any(|output| matches!(output,
            KernelOutput::Guard(read) if read.owner == KernelGuardUse::Power(link as u32)
                && read.guard == mixed
        )),
        "the guard is consumed inside this region"
    );
}

#[test]
fn comparator_overrides_expose_owned_geometry_reads_without_a_power_edge() {
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::program_graph::{CircuitNodeDraft, ComparatorInputPlan, ComparatorOverride, ComponentState, OperandView, SiteTest};
    use mchprs_blocks::blocks::ComparatorMode;

    let (mut graph, _) = independent_devices();
    let region = graph.plans.instant.regions[0].clone();
    let owned_site = graph
        .sites
        .iter()
        .position(|site| matches!(site.source, SiteSource::Contact(contact) if region.contacts.contains(&contact)))
        .unwrap();
    let external_site = graph
        .sites
        .iter()
        .enumerate()
        .find(|(index, _)| !region.readable_sites.contains(&SiteId::new(*index as u32)))
        .unwrap()
        .0;
    let comparator = NodeId::new(graph.nodes.len() as u32);
    graph.nodes.push(CircuitNodeDraft {
        position: None,
        raw_state: None,
        kind: CircuitNodeKind::Electrical(NodeType::Comparator {
            mode: ComparatorMode::Compare,
            far_input: None,
            facing_diode: false,
        }),
        state: ComponentState::Electrical(NodeState::simple(false)),
        output_route: None,
    });
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Site {
        site: SiteId::new(owned_site as u32),
        view: OperandView::Live,
        test: SiteTest::Solid,
    });
    let plan = graph.comparator_inputs.len() as u32;
    graph.comparator_inputs.push(ComparatorInputPlan {
        node: comparator,
        rear: SiteId::new(owned_site as u32),
        direct_override: None,
        far_override: Some((SiteId::new(external_site as u32), ComparatorOverride::Literal(7), guard)),
        far_threshold: 15,
    });
    assert!(
        graph.power_links.iter().all(|link| link.guard != guard),
        "this override guard must not be discoverable through power edges"
    );

    let boundary = KernelBoundary::from_region(&graph, &region);
    assert!(
        boundary.outputs.iter().any(|output| matches!(output,
            KernelOutput::Guard(read) if read.owner == KernelGuardUse::ComparatorOverride(plan)
                && read.guard == guard
        )),
        "an external comparator can observe owned contact geometry without a power-link guard"
    );
    assert!(!boundary.inputs.iter().any(|input| matches!(input,
        KernelInput::Guard(read) if read.owner == KernelGuardUse::ComparatorOverride(plan)
            && read.guard == guard
    )));
}

#[test]
fn regional_bud_memory_changes_only_after_the_independent_update() {
    let circuit = super::symbolic_kernel_tests::load_mechanism("bud_instantmemorycellobserverupdate");
    let positions = circuit.positions();
    let data = circuit.place(local(&circuit.ports["inputs"]["data"]));
    let update = circuit.place(local(&circuit.ports["inputs"]["update"]));
    let memory = circuit.place(local(&circuit.ports["observations"]["memory"]));
    let mut saw_action = false;
    let mut sampled_memory = Vec::new();
    for powered in [false, true] {
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let mut backend = GraphBackend::compile_with_options(
            &native,
            circuit.bounds(),
            vec![],
            &CompilerOptions {
                optimization_level: Some(4),
                stable_instant_inputs: true,
                ..Default::default()
            },
            Arc::default(),
        )
        .unwrap();
        assert!(backend.program.instant_kernel().is_none());
        assert!(backend.program.symbolic_kernels().is_empty());
        assert!(backend.program.symbolic_region_counts().1 > 0);
        for region in backend.program.symbolic_region_programs() {
            assert!(
                region
                    .actors
                    .iter()
                    .all(|actor| backend.program.pistons()[actor.actor.index()].position != memory),
                "the BUD memory must remain an ordinary Graph component outside the instant program"
            );
        }
        let initial_memory = native.get_block(memory);
        let steps = std::iter::once(Step::SetLever { position: data, powered })
            .chain(std::iter::repeat_n(Step::Tick, 32))
            .chain(std::iter::once(Step::SetLever {
                position: update,
                powered: false,
            }))
            .chain(std::iter::repeat_n(Step::Tick, 36));
        for (index, step) in steps.enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut backend, step, toggle).unwrap();
            check_boundary(
                &backend,
                &native,
                &mut projected,
                &positions,
                &format!("BUD data={powered} event={index}"),
            )
            .unwrap();
            if index <= 32 {
                assert_eq!(native.get_block(memory), initial_memory, "QC data alone must not sample memory");
                assert_eq!(projected.get_block(memory), initial_memory);
            }
        }
        assert!(
            backend.execution_counts().symbolic_region_steps > 0,
            "the qualifying update must execute regional routines"
        );
        saw_action |= backend.execution_counts().piston_actions > 0;
        sampled_memory.push(projected.get_block(memory));
        backend.reset(&mut projected);
        for tick in 0..8 {
            native.tick_interpreted();
            projected.tick_interpreted();
            assert_eq!(
                future(&projected, &positions),
                future(&native, &positions),
                "BUD handoff data={powered} tick={tick}"
            );
        }
    }
    assert!(saw_action, "at least one sampled data value must change the memory mechanism");
    assert_ne!(
        sampled_memory[0], sampled_memory[1],
        "the update must actually sample the two distinct QC data values"
    );
}

/// An event VM alone passes functional comparisons; this also requires propagation to disappear.
#[test]
fn compiled_adder_regions_replace_internal_propagation_and_preserve_outputs() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["expectation"]["carry"].as_u64() == Some(1))
            .expect("the fixture has a carry-producing transaction");
        let positions = circuit.positions();
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let compile = |level| {
            GraphBackend::compile_with_options(
                &native,
                circuit.bounds(),
                vec![],
                &CompilerOptions {
                    optimization_level: Some(level),
                    stable_instant_inputs: true,
                    ..Default::default()
                },
                Arc::default(),
            )
            .unwrap()
        };
        let mut general = compile(3);
        let mut compiled = compile(4);
        let coverage = compiled.program.symbolic_region_counts();
        assert_eq!(coverage.0, 1, "{id} needs a complete regional program; coverage={coverage:?}");
        assert_eq!(coverage.3, 0, "{id} must not pass through a regional compilation decline");
        assert!(
            compiled.program.symbolic_region_programs().all(|region| region.event_kernel.is_some()),
            "{id} must use complete propagation plans, not the old actor-predicate VM"
        );
        assert!(
            compiled.program.instant_kernel().is_none(),
            "a regional kernel must not use a global finite machine"
        );
        assert!(
            compiled.program.symbolic_kernels().is_empty(),
            "a regional kernel must not take over the whole selection"
        );
        let mut steps = case_steps(&circuit, case, false, true);
        let response_start = steps.len();
        steps.extend(std::iter::repeat_n(Step::Tick, 24));
        for (index, step) in steps.into_iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut general, step, toggle).unwrap();
            graph_step(&mut compiled, step, toggle).unwrap();
            check_boundary(&compiled, &native, &mut projected, &positions, &format!("compiled {id} step {index}")).unwrap();
            if index >= response_start {
                let (sum, carry) = expectation(&circuit, case);
                assert_math_window(&circuit, &projected, sum, carry, index - response_start + 1);
            }
        }
        let before = general.execution_counts();
        let after = compiled.execution_counts();
        eprintln!("{id} work: general={before:?}; compiled={after:?}");
        assert!(before.wire_walks > 0 && before.callbacks > 0, "the baseline must exercise propagation");
        assert!(
            after.wire_walks * 4 <= before.wire_walks,
            "{id} must eliminate most Turbo walks, rather than evaluate extra predicates around them"
        );
        assert!(
            after.callbacks * 2 <= before.callbacks,
            "{id} must absorb internal callback delivery, rather than replay the physical routes"
        );
        compiled.reset(&mut projected);
        for tick in 0..8 {
            assert_eq!(future(&native, &positions), future(&projected, &positions), "{id} handoff tick {tick}");
            native.tick_interpreted();
            projected.tick_interpreted();
        }
    }
}

#[test]
fn regional_external_power_reads_stay_dynamic_without_rebinding_internal_writes() {
    use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, RegionRead};
    use mchprs_redpiler_opt::{Limits, symbolic::Signal};

    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let region = &graph.plans.instant.regions[0];
    let (site, source) = graph
        .sites
        .iter()
        .enumerate()
        .find_map(|(index, site)| match site.source {
            SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) => {
                Some((SiteId::new(index as u32), node))
            }
            _ => None,
        })
        .expect("the adder has an external component source");
    let mut compiler = SymbolicCompiler::new_region(&graph, region, Limits::default()).unwrap();
    let stored = compiler.site_power(site, PowerPort::Stored).unwrap();
    assert_eq!(stored, compiler.site_power(site, PowerPort::Output).unwrap());
    let circuit = compiler.logic.circuit(&stored.bits()).unwrap();
    let plan = mchprs_redpiler_opt::optimize(&circuit, Limits::default(), || false).unwrap();
    let mut scratch = vec![false; plan.scratch_len()];
    let mut roots = [false; 4];
    // The compiled source must follow all signal values after preparation, including analog values.
    for strength in 0u8..16 {
        let inputs: Vec<_> = plan
            .inputs()
            .iter()
            .map(|&input| match compiler.region_reads[input] {
                RegionRead::ComponentSignal { node, bit } if node == source => strength & (1 << bit) != 0,
                _ => false,
            })
            .collect();
        plan.evaluate(&inputs, &mut scratch, &mut roots).unwrap();
        let actual = roots.iter().enumerate().fold(0u8, |value, (bit, &set)| value | (u8::from(set) << bit));
        assert_eq!(actual, strength, "external power must not freeze at the schematic value");
    }

    let wire = region.wires[0];
    let written = Signal::literal(7).unwrap();
    compiler.state.components[wire.index()].signal = written;
    compiler.state_written();
    compiler.symbolize_component(wire).unwrap();
    assert_eq!(
        compiler.state.components[wire.index()].signal,
        written,
        "a subsequent read must keep the sequential wire write instead of restoring its entry input"
    );
}

#[test]
fn compiled_adders_preserve_every_pinned_arithmetic_history_and_safe_handoff() {
    let mut checked = 0;
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let positions = circuit.positions();
        let (draft, _) = stable_region_tests::stable_graph_at_level(id, 4);
        let (program, initial_state) = draft.lower().unwrap();
        assert_eq!(program.symbolic_region_counts().0, 1, "{id} must use the regional compiler");
        assert_eq!(program.symbolic_region_counts().3, 0, "{id} must not pass through a compilation decline");
        assert!(program.symbolic_region_programs().all(|region| region.event_kernel.is_some()));
        assert!(program.instant_kernel().is_none() && program.symbolic_kernels().is_empty());
        // Compile once; every prepared case executes the same immutable program from fresh state.
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            let mut native = circuit.build();
            let mut projected = circuit.build();
            let mut backend = GraphBackend::new(program.clone(), initial_state.clone(), 0).unwrap();
            let mut steps = case_steps(&circuit, case, false, true);
            let response_start = steps.len();
            steps.extend(std::iter::repeat_n(Step::Tick, 24));
            for (index, step) in steps.into_iter().enumerate() {
                let toggle = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                graph_step(&mut backend, step, toggle).unwrap();
                check_boundary(
                    &backend,
                    &native,
                    &mut projected,
                    &positions,
                    &format!("{id}/{} event {index}", case["id"]),
                )
                .unwrap();
                if index >= response_start {
                    let (sum, carry) = expectation(&circuit, case);
                    assert_math_window(&circuit, &projected, sum, carry, index - response_start + 1);
                }
            }
            assert!(
                backend.execution_counts().symbolic_region_steps > 0,
                "{id}/{} must exercise compiled execution",
                case["id"]
            );
            backend.reset(&mut projected);
            for tick in 0..8 {
                assert_eq!(
                    future(&native, &positions),
                    future(&projected, &positions),
                    "{id}/{} handoff {tick}",
                    case["id"]
                );
                native.tick_interpreted();
                projected.tick_interpreted();
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 48, "all pinned cases must execute, rather than an empty or partial filter");
}
