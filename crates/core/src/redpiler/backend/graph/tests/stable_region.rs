//! Contract-scoped composition and full-state comparison on the real adder fixtures.
use super::*;
use crate::redpiler::CompilerOptions;

pub(super) fn stable_graph(id: &'static str) -> (CompileGraph, EntryState) {
    stable_graph_at_level(id, 3)
}

/// Synthesis tests select their experimental builders explicitly; public compilation keeps them disabled.
pub(super) fn experimental_pipeline(options: &CompilerOptions) -> passes::PassPipeline {
    let mut pipeline = passes::default_graph_pipeline(options);
    if options.optimization_level.is_none_or(|level| level >= 4) {
        for entry in &mut pipeline.entries {
            if matches!(
                &entry.pass,
                passes::Pass::Build(builder)
                    if matches!(builder.name(), "compile-symbolic-regions" | "compile-instant-machine" | "compile-symbolic-kernels")
            ) {
                entry.enabled = true;
            }
        }
    }
    pipeline
}

pub(super) fn stable_graph_at_level(id: &'static str, level: usize) -> (CompileGraph, EntryState) {
    let circuit = load_adder(id, ORIGIN, None);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(level),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &experimental_pipeline(&options), &monitor).unwrap();
    (prepared.graph, prepared.entry)
}

#[test]
fn stable_contract_is_explicit_and_requires_the_instant_pass_tier() {
    assert!(!CompilerOptions::default().stable_instant_inputs);
    for flags in [
        "-O3 --stable-instant-inputs",
        "-O4 --stable-instant-inputs",
        "--stable-instant-inputs --optimize",
    ] {
        assert!(CompilerOptions::parse(flags).unwrap().stable_instant_inputs);
    }
    for flags in ["--stable-instant-inputs", "-O2 --stable-instant-inputs"] {
        assert!(CompilerOptions::parse(flags).is_err());
    }
}

#[test]
fn both_adders_compose_all_their_actors_into_one_ordered_region() {
    for (id, actor_count, private_count) in [("adder_1bit", 13, 4), ("adder_11bits", 142, 43)] {
        let (graph, _) = stable_graph(id);
        eprintln!(
            "{id}: regions={}, actors={:?}, wires={:?}, proxies={}",
            graph.plans.instant.regions.len(),
            graph.plans.instant.regions.iter().map(|r| r.actors.len()).collect::<Vec<_>>(),
            graph.plans.instant.regions.iter().map(|r| r.wires.len()).collect::<Vec<_>>(),
            graph.plans.instant.proxies.len()
        );
        assert!(
            graph.plans.instant.machine.is_none(),
            "O3 composes regions without O4 compact compilation"
        );
        assert_eq!(graph.plans.instant.regions.len(), 1);
        let region = &graph.plans.instant.regions[0];
        assert!(region.stable_inputs);
        assert_eq!(region.actors.len(), actor_count);
        assert_eq!(region.contacts.len(), graph.contacts.len(), "all actor and payload contacts remain owned");
        assert!(!region.wires.is_empty());
        assert_eq!(graph.plans.instant.proxies.len(), 1);
        let proxy = &graph.plans.instant.proxies[0];
        assert_eq!(proxy.private_cells.len(), private_count);
        let mut sampled: Vec<_> = proxy
            .routes
            .iter()
            .flat_map(|route| route.steps.iter())
            .filter_map(|step| {
                if let InstantStepKind::Sample(actor) = step.kind {
                    Some(actor)
                } else {
                    None
                }
            })
            .collect();
        sampled.sort_unstable();
        sampled.dedup();
        assert_eq!(sampled, region.actors, "every actor has an explicit logical sampling instruction");
        graph.validate().unwrap();
    }
}

#[test]
fn symbolic_region_programs_cover_every_adder_actor_and_unique_reset_clock() {
    for (id, actor_count) in [("adder_1bit", 13), ("adder_11bits", 142)] {
        let (graph, _) = stable_graph_at_level(id, 4);
        assert!(graph.plans.instant.symbolic_region_declines.is_empty(), "{id} must compile completely");
        assert_eq!(graph.plans.instant.symbolic_regions.len(), 1, "{id} compiles as one shared region");
        let region = &graph.plans.instant.symbolic_regions[0];
        assert_eq!(region.region, 0);
        assert_eq!(region.actors.len(), actor_count, "{id} must represent every owned actor");
        assert!(!region.boundary.inputs.is_empty(), "{id} retains explicit external entry ports");
        assert!(
            !graph.plans.instant.regions[0].wires.is_empty(),
            "ordinary wire execution remains in Graph"
        );

        let resets: std::collections::HashSet<_> = region.resets.iter().map(|reset| reset.node).collect();
        assert_eq!(resets.len(), region.resets.len(), "shared reset clocks have one routine owner");
        assert_eq!(
            region.resets.iter().map(|reset| reset.node).collect::<Vec<_>>(),
            graph.plans.instant.regions[0].resets,
            "each original reset node has exactly one update/tick program"
        );

        let actor_ids = graph.plans.instant.regions[0].actors.clone();
        let (program, _) = graph.clone().lower().unwrap();
        assert_eq!(program.symbolic_region_counts(), (1, actor_count, resets.len(), 0));
        for actor in &actor_ids {
            assert!(program.symbolic_actor_routine(*actor).is_some(), "each actor has a direct event binding");
        }
        for reset in resets {
            assert!(program.symbolic_reset(reset).is_some(), "each reset node has a direct event binding");
        }
    }
}

fn stable_backend(circuit: &Adder, world: &PlotWorld) -> GraphBackend {
    stable_backend_at(circuit, world, 3)
}

pub(super) fn stable_backend_at(circuit: &Adder, world: &PlotWorld, level: usize) -> GraphBackend {
    let options = CompilerOptions {
        optimization_level: Some(level),
        stable_instant_inputs: true,
        ..Default::default()
    };
    let monitor = TaskMonitor::default();
    let ticks = world.scheduler().iter_entries().collect();
    let mut prepared = passes::rich::prepare(world, circuit.bounds(), ticks, &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &experimental_pipeline(&options), &monitor).unwrap();
    prepared.finish_pipeline_statistics(&monitor);
    GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap()
}

#[test]
fn regional_adder_execution_matches_general_graph_and_interpreter_handoff() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["expectation"]["carry"].as_u64() == Some(1))
            .expect("fixture includes a carry-producing input history");
        let positions = circuit.positions();
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let (draft, _) = stable_graph_at_level(id, 4);
        let mut sorted_links = draft.power_links.clone();
        sorted_links.sort_by_key(|link| link.to);
        assert_ne!(
            draft.power_links, sorted_links,
            "{id} draft must exercise a nontrivial power-link permutation"
        );
        let (program, state) = draft.lower().unwrap();
        let mut regional = GraphBackend::new(program, state, 0).unwrap();
        let mut general = GraphBackend::compile_with_options(
            &native,
            circuit.bounds(),
            vec![],
            &CompilerOptions {
                optimization_level: Some(3),
                stable_instant_inputs: true,
                ..Default::default()
            },
            Arc::default(),
        )
        .unwrap();
        assert_eq!(
            regional.program.symbolic_region_counts().0,
            1,
            "{id} must execute one complete local region"
        );
        assert_eq!(regional.program.symbolic_region_counts().3, 0, "{id} must not hide a partial decline");
        assert!(
            regional.program.symbolic_kernels().is_empty(),
            "{id} must not depend on a whole-selection AOT cursor"
        );
        assert!(
            regional.program.instant_kernel().is_none(),
            "{id} must not depend on a finite whole-selection machine"
        );
        assert!(
            regional.program.instant_proxies().len() == 1,
            "the fixture must exercise lowered route proxies"
        );
        assert!(regional.program.power_links().windows(2).all(|pair| pair[0].to <= pair[1].to));
        for region in regional.program.symbolic_region_programs() {
            for actor in region.actors.iter() {
                for read in actor.reads.iter() {
                    if let crate::redpiler::program_graph::ActorRead::Power(link) = read {
                        assert!(
                            regional.program.power_links().contains(link),
                            "{id} actor reads retain the intended graph link after lowering sorts the shared table"
                        );
                    }
                }
            }
        }

        regional.set_test_recording(true);
        general.set_test_recording(true);
        let mut steps = case_steps(&circuit, case, false, true);
        steps.extend(std::iter::repeat_n(Step::Tick, 24));
        for (index, step) in steps.into_iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut general, step, toggle).unwrap();
            graph_step(&mut regional, step, toggle).unwrap();
            instant_proxy_tests::assert_same_memory(&general, &regional);
            assert_eq!(general.callback_order, regional.callback_order, "{id} callback order at step {index}");
            assert_eq!(
                general.observed_operations, regional.observed_operations,
                "{id} operations at step {index}"
            );
            check_boundary(
                &regional,
                &native,
                &mut projected,
                &positions,
                &format!("{id}/{} step {index}", case["id"]),
            )
            .unwrap();
        }
        assert!(
            regional.execution_counts().symbolic_region_steps > 0,
            "{id} must hit regional routines on the real input history"
        );

        let mut handed_back = circuit.build();
        regional.reset(&mut handed_back);
        for tick in 0..8 {
            assert_eq!(
                future(&native, &positions),
                future(&handed_back, &positions),
                "{id} complete state at interpreter handoff tick {tick}"
            );
            native.tick_interpreted();
            handed_back.tick_interpreted();
        }
    }
}

fn input_levels(circuit: &Adder, world: &PlotWorld) -> Vec<(BlockPos, bool)> {
    circuit
        .cells
        .iter()
        .filter_map(|(local, block, _)| {
            if !matches!(block, Block::Lever { .. }) {
                return None;
            }
            let position = circuit.place(*local);
            let Block::Lever { lever } = world.get_block(position) else {
                panic!("missing external control at {position:?}")
            };
            Some((position, lever.powered))
        })
        .collect()
}

/// Freeze external controls at the actual falling edge, before any queued action is accepted.
#[test]
fn falling_edge_adder_cycles_preserve_all_memory_and_pending_work() {
    let mut cases_checked = 0;
    let mut absorbed_callbacks = 0;
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            let positions = circuit.positions();
            let mut native = circuit.build();
            let mut scratch = circuit.build();
            let mut general = GraphBackend::compile_with_options(
                &native,
                circuit.bounds(),
                vec![],
                &CompilerOptions {
                    optimization_level: Some(3),
                    ..Default::default()
                },
                Arc::default(),
            )
            .unwrap();
            let mut composed = stable_backend(&circuit, &native);
            assert_eq!(composed.program.instant_proxies().len(), 1);
            general.set_test_recording(true);
            composed.set_test_recording(true);
            let mut steps = case_steps(&circuit, case, false, true);
            let response_start = steps.len();
            let mut frozen_inputs = None;
            steps.extend(std::iter::repeat_n(Step::Tick, 16));
            for (index, step) in steps.into_iter().enumerate() {
                let toggle = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                    Step::Tick => false,
                };
                let falling_edge = matches!(step, Step::SetLever { position, powered: false } if position == circuit.trigger());
                if falling_edge {
                    assert!(toggle, "{id}/{} must activate with a real high-to-low edge", case["id"]);
                    assert!(frozen_inputs.is_none(), "no second activation during this held-input episode");
                }
                if frozen_inputs.is_some() {
                    assert!(matches!(step, Step::Tick), "external inputs cannot change before reset");
                }
                general.clear_test_traces();
                composed.clear_test_traces();
                native_step(&mut native, step);
                graph_step(&mut general, step, toggle).unwrap();
                graph_step(&mut composed, step, toggle).unwrap();
                instant_proxy_tests::assert_same_memory(&general, &composed);
                check_boundary(&composed, &native, &mut scratch, &positions, &format!("{id}/{} step {index}", case["id"])).unwrap();
                if falling_edge {
                    frozen_inputs = Some(input_levels(&circuit, &native));
                }
                if let Some(expected) = &frozen_inputs {
                    assert_eq!(&input_levels(&circuit, &native), expected);
                    assert_eq!(&input_levels(&circuit, &scratch), expected);
                }
                if index >= response_start {
                    assert!(frozen_inputs.is_some());
                    assert_math_window(
                        &circuit,
                        &scratch,
                        expectation(&circuit, case).0,
                        expectation(&circuit, case).1,
                        index - response_start + 1,
                    );
                }
            }
            absorbed_callbacks += general.execution_counts().callbacks - composed.execution_counts().callbacks;
            cases_checked += 1;
        }
    }
    assert_eq!(cases_checked, 48);
    assert!(absorbed_callbacks > 0, "at least one exercised cycle must use the composed instructions");
}

#[test]
fn stable_composition_retains_uncertified_reset_groups() {
    use crate::redpiler::analysis::instant_regions::{infer_regions, infer_stable_regions};
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&circuit.build(), circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    assert_eq!(
        infer_stable_regions(&prepared.graph, &monitor).unwrap(),
        infer_regions(&prepared.graph, &monitor).unwrap()
    );
}

#[test]
fn composed_certificates_reject_lost_contacts_and_wrong_internal_targets() {
    let (graph, _) = stable_graph("adder_1bit");
    let mut missing_contact = graph.clone();
    missing_contact.plans.instant.regions[0].contacts.pop();
    assert!(matches!(missing_contact.validate(), Err(GraphBuildError::InvalidInstantRegion { .. })));
    let mut wrong_actor = graph;
    let step = wrong_actor.plans.instant.proxies[0]
        .routes
        .iter_mut()
        .flat_map(|route| route.steps.iter_mut())
        .find(|step| matches!(step.kind, InstantStepKind::Sample(_)))
        .unwrap();
    let InstantStepKind::Sample(actor) = step.kind else { unreachable!() };
    step.kind = InstantStepKind::Sample(PistonId::new((actor.index() as u32 + 1) % 13));
    assert!(matches!(wrong_actor.validate(), Err(GraphBuildError::InvalidInstantRegion { .. })));
}

#[test]
fn stable_region_reset_hands_back_the_same_memory_and_continuation() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["expectation"].is_object() && case["expectation"]["carry"].as_u64() == Some(1))
            .unwrap();
        for cut in [0, 1, 2, 4, 8, 16] {
            let mut native = circuit.build();
            let mut handed_back = circuit.build();
            let mut backend = stable_backend(&circuit, &native);
            let positions = circuit.positions();
            let mut steps = case_steps(&circuit, case, false, true);
            steps.extend(std::iter::repeat_n(Step::Tick, cut));
            for step in steps {
                let toggle = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                graph_step(&mut backend, step, toggle).unwrap();
            }
            backend.reset(&mut handed_back);
            for tick in 0..17 {
                assert_eq!(
                    first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
                    None,
                    "{id} cut={cut} continuation={tick}"
                );
                native.tick_interpreted();
                handed_back.tick_interpreted();
            }
        }
    }
}
