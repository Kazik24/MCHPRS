//! Retry settled-net contraction against RAM bits, rather than queue identity.
use super::*;
use crate::redpiler::passes::planning::PlanBuilder;
use random_fpu::Operation;

// BubbleSort's frozen RAM decoder defines all four physical memory banks.
pub(super) fn memory_probes(graph: &CompileGraph, name: &str) -> Vec<(PistonId, BlockPos)> {
    let positions: Vec<_> = if name == "cpu_bubblesort" {
        [7, 17, 27, 37]
            .into_iter()
            .flat_map(|y| (0..16).flat_map(move |row| (0..12).map(move |bit| cpus::CPUS[2].origin + BlockPos::new(175 + 4 * bit, y, 62 - 4 * row))))
            .collect()
    } else if name == "counter" {
        graph
            .plans
            .bud_memory_reads
            .iter()
            .map(|read| graph.pistons[read.storage.index()].position)
            .collect()
    } else {
        let actors: BTreeSet<_> = graph
            .plans
            .instant_events
            .iter()
            .flat_map(|kernel| kernel.actors.iter().copied())
            .collect();
        graph
            .pistons
            .iter()
            .enumerate()
            .filter(|(id, _)| !actors.contains(&PistonId::new(*id as u32)))
            .map(|(_, piston)| piston.position)
            .collect()
    };
    positions
        .into_iter()
        .map(|position| {
            let id = graph
                .pistons
                .iter()
                .position(|piston| piston.position == position)
                .expect("memory piston");
            (PistonId::new(id as u32), position)
        })
        .collect()
}

pub(super) fn native_storage_bit(world: &PlotWorld, position: BlockPos) -> Option<bool> {
    match world.get_block(position) {
        Block::Piston { piston } => Some(!piston.extended),
        Block::MovingPiston { .. } => None,
        block => panic!("native storage at {position:?}: {block:?}"),
    }
}

pub(super) fn graph_storage_bit(backend: &GraphBackend, actor: PistonId) -> Option<bool> {
    let state = backend.circuit_state().pistons[actor.index()];
    (!state.availability.base_pending()).then_some(!state.extended)
}

#[test]
#[ignore = "explicit full BubbleSort memory-only contraction experiment, including native reference"]
fn bubblesort_closed_nets_memory_only() {
    probe("cpu_bubblesort");
}

#[test]
#[ignore = "explicit seeded random FPU memory-only contraction experiment, including native reference"]
fn fpu_closed_nets_memory_only() {
    probe("fpu");
}

#[test]
#[ignore = "explicit Counter 16-cell BUD memory-only contraction experiment"]
fn counter_closed_nets_memory_only() {
    probe("counter");
}

#[test]
#[ignore = "explicit Counter memory-only probe with outside electrical consumers admitted"]
fn counter_relaxed_electrical_consumers_memory_only() {
    probe_variant("counter", true);
}

#[test]
#[ignore = "explicit relaxed electrical-boundary contraction histories for CPU, FPU, and Counter"]
fn relaxed_electrical_boundaries_memory_only() {
    for name in ["cpu_bubblesort", "fpu", "counter"] {
        probe_variant(name, true);
    }
}

#[test]
#[ignore = "compile-only census before deciding whether relaxed large-device replays add useful coverage"]
fn relaxed_electrical_boundary_admission_delta() {
    let mut rows = Vec::new();
    for name in ["cpu_bubblesort", "fpu", "counter"] {
        let fixture = if name == "counter" {
            let path = root().join("test_data/instant-pistons-io/fixtures/counter_basic.json");
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        } else {
            manifest("fpu_legal")
        };
        let (world, bounds) = if name == "cpu_bubblesort" {
            let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
            (world, bounds)
        } else {
            load(&fixture)
        };
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let options = CompilerOptions {
            optimization_level: Some(5),
            stable_instant_inputs: true,
            budget_multiplier: 8,
            ..Default::default()
        };
        let ticks = world.scheduler().iter_entries().collect();
        let mut prepared = passes::rich::prepare(&world, bounds, ticks, &monitor, AnalysisLimits::for_budget(8)).unwrap();
        let mut pipeline = passes::default_graph_pipeline(&options);
        pipeline.entries.retain(|entry| {
            !matches!(&entry.pass, passes::Pass::Build(builder)
                if matches!(builder.name(), "unsafe-abstract-instant-nets" | "experimental-compile-instant-wire-nodes"))
        });
        passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        let graph = &prepared.graph;
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        let strict = census_closed_instant_nets(graph, catalog, &monitor).unwrap();
        let relaxed = crate::redpiler::program_graph::census_instant_nets(graph, catalog, &monitor, true).unwrap();
        let multi = |census: &crate::redpiler::program_graph::ClosedInstantNetCensus| {
            (
                census.closed.iter().filter(|net| net.wires > 1).count(),
                census.closed.iter().filter(|net| net.wires > 1).map(|net| net.wires).sum::<usize>(),
            )
        };
        let before = multi(&strict);
        let after = multi(&relaxed);
        rows.push(json!({"device":name,"strict_multi_wire_nets":before.0,"strict_wires":before.1,
            "relaxed_multi_wire_nets":after.0,"relaxed_wires":after.1,"added_nets":after.0.saturating_sub(before.0),
            "added_wires":after.1.saturating_sub(before.1),"strict_declines":strict.declines,"relaxed_declines":relaxed.declines}));
        println!("{name}: strict {:?} -> relaxed {:?} multi-wire nets/wires", before, after);
    }
    let report = json!({"relaxation":"outside_electrical_consumer only; signal sources, callback receivers, execution ownership, and reset wires stay strict","devices":rows});
    let directory = root().join("target/instant-wire-nodes/memory-only");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("admission-delta.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

fn probe(name: &str) {
    probe_variant(name, false);
}

fn probe_variant(name: &str, relax_electrical_boundaries: bool) {
    let fixture = if name == "counter" {
        let path = root().join("test_data/instant-pistons-io/fixtures/counter_basic.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    } else {
        manifest("fpu_legal")
    };
    let (mut native, bounds) = if name == "cpu_bubblesort" {
        let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
        (world, bounds)
    } else {
        load(&fixture)
    };
    native.disable_command_output_limits_for_replay();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &native,
        bounds,
        native.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline.entries.retain(|entry| {
        !matches!(&entry.pass, passes::Pass::Build(builder)
            if matches!(builder.name(), "unsafe-abstract-instant-nets" | "experimental-compile-instant-wire-nodes"))
    });
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    if name == "counter" {
        passes::compile_bud_memory_reads::CompileBudMemoryReads
            .run(&mut prepared.graph, &passes::PassContext::new(&monitor, &options, &prepared.entry))
            .unwrap();
        assert_eq!(prepared.graph.plans.bud_memory_reads.len(), 16);
    }
    let graph = &prepared.graph;
    let memories = memory_probes(graph, name);
    assert!(!memories.is_empty(), "the experiment must observe actual storage pistons");
    let census = crate::redpiler::program_graph::census_instant_nets(
        graph,
        graph.plans.instant.boundaries.as_ref().unwrap(),
        &monitor,
        relax_electrical_boundaries,
    )
    .unwrap();
    let mut contracted = graph.clone();
    let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
    if relax_electrical_boundaries {
        crate::redpiler::program_graph::with_relaxed_instant_net_electrical_boundaries(|| {
            passes::abstract_instant_nets::AbstractInstantNets.run(&mut contracted, &context).unwrap();
        });
    } else {
        passes::abstract_instant_nets::AbstractInstantNets.run(&mut contracted, &context).unwrap();
    }
    let contracted_wires = contracted.wires.iter().filter(|wire| wire.execution.abstract_instant_net).count();
    assert!(contracted_wires > 0);
    let (program, state) = if relax_electrical_boundaries {
        crate::redpiler::program_graph::with_relaxed_instant_net_electrical_boundaries(|| contracted.lower_with_monitor(&monitor).unwrap())
    } else {
        contracted.lower_with_monitor(&monitor).unwrap()
    };
    let baseline = prepared.lower(&monitor).unwrap();
    let candidate = LoweredCircuit {
        program,
        state,
        logical_tick: baseline.logical_tick,
        positions: baseline.positions.clone(),
        imported_ticks: baseline.imported_ticks.clone(),
    };
    let mut baseline = GraphBackend::from_lowered(baseline, &monitor).unwrap();
    let mut candidate = GraphBackend::from_lowered(candidate, &monitor).unwrap();
    baseline.set_test_recording(false);
    candidate.set_test_recording(false);
    let mut base_host = empty();
    let mut candidate_host = empty();
    base_host.disable_command_output_limits_for_replay();
    candidate_host.disable_command_output_limits_for_replay();
    let operations = if name == "cpu_bubblesort" {
        let trigger = cpus::CPUS[2].origin + cpus::CPUS[2].start;
        cpus::click_cpu(&mut native, cpus::CPUS[2], cpus::CPUS[2].start);
        baseline.use_block(trigger).unwrap();
        candidate.use_block(trigger).unwrap();
        vec![Operation::Tick; 10_036]
    } else if name == "counter" {
        let trigger = local_pos(&fixture["ports"]["inputs"]["trigger"]) - BASE + origin(&fixture);
        std::iter::repeat_n(Operation::Tick, 24)
            .chain(std::iter::once(Operation::Lever(trigger, true)))
            .chain(std::iter::repeat_n(Operation::Tick, 102))
            .collect()
    } else {
        random_fpu::operations(&fixture).0
    };
    let mut controls = std::collections::HashMap::new();
    for operation in &operations {
        if let Operation::Lever(position, _) = *operation {
            let Block::Lever { lever } = native.get_block(position) else {
                panic!("missing input lever at {position:?}")
            };
            controls.insert(position, lever.powered);
        }
    }
    let mut tick = 0usize;
    let mut mismatched_ticks = 0usize;
    let mut mismatched_64_tick_checkpoints = 0usize;
    let mut first_memory_difference = Value::Null;
    let mut first_resource_difference = Value::Null;
    let mut last_memory_difference_tick = None;
    let mut maximum_differing_bits = 0;
    let mut previous_bits: Vec<_> = memories.iter().map(|&(actor, _)| graph_storage_bit(&baseline, actor)).collect();
    let mut unavailable_samples = 0usize;
    let mut availability_difference_samples = 0usize;
    let mut first_availability_difference = Value::Null;
    let mut native_bit_transitions = 0usize;
    let mut changed_cells = vec![false; memories.len()];
    let mut baseline_seconds = 0.0;
    let mut contracted_seconds = 0.0;
    for operation in operations {
        match operation {
            Operation::Lever(position, powered) => {
                lever_action(&mut native, position, powered);
                if controls.insert(position, powered) != Some(powered) {
                    baseline.use_block(position).unwrap();
                    candidate.use_block(position).unwrap();
                }
                continue;
            }
            Operation::Tick => {
                native.tick_interpreted();
                let start = std::time::Instant::now();
                baseline.tick_with_world(&mut base_host).unwrap();
                baseline_seconds += start.elapsed().as_secs_f64();
                let start = std::time::Instant::now();
                candidate.tick_with_world(&mut candidate_host).unwrap();
                contracted_seconds += start.elapsed().as_secs_f64();
                tick += 1;
            }
        }
        let mut differing_bits = Vec::new();
        for (index, &(actor, position)) in memories.iter().enumerate() {
            let expected = native_storage_bit(&native, position);
            assert_eq!(
                graph_storage_bit(&baseline, actor),
                expected,
                "unchanged baseline differs from native at {name} tick {tick}, storage piston {index}"
            );
            let actual = graph_storage_bit(&candidate, actor);
            if expected.is_some() != actual.is_some() {
                availability_difference_samples += 1;
                if first_availability_difference.is_null() {
                    first_availability_difference = json!({"tick":tick,"position":position,"native":expected,"contracted":actual});
                }
            }
            if let Some(expected_bit) = expected {
                if previous_bits[index].is_some_and(|previous| previous != expected_bit) {
                    native_bit_transitions += 1;
                    changed_cells[index] = true;
                }
                previous_bits[index] = Some(expected_bit);
                if let Some(actual_bit) = actual {
                    if expected_bit != actual_bit {
                        differing_bits.push(json!({"memory":index,"actor":actor.index(),
                            "position":position,"native_bit":expected_bit,"contracted_bit":actual_bit}));
                    }
                } else {
                    unavailable_samples += 1;
                }
            } else {
                unavailable_samples += 1;
            }
        }
        if first_resource_difference.is_null() {
            first_resource_difference = baseline
                .circuit_state()
                .resources
                .iter()
                .zip(&candidate.circuit_state().resources)
                .enumerate()
                .find(|(_, (left, right))| left.enabled != right.enabled || left.at != right.at)
                .map(|(id, (left, right))| json!({"tick":tick,"resource":id,"baseline":format!("{left:?}"),"contracted":format!("{right:?}")}))
                .unwrap_or(Value::Null);
        }
        if !differing_bits.is_empty() {
            mismatched_ticks += 1;
            last_memory_difference_tick = Some(tick);
            maximum_differing_bits = maximum_differing_bits.max(differing_bits.len());
            if tick % 64 == 0 {
                mismatched_64_tick_checkpoints += 1;
            }
            if first_memory_difference.is_null() {
                first_memory_difference = json!({"tick":tick,"bits":differing_bits});
                println!("{name}: first storage bit difference at tick {tick}: {first_memory_difference}");
            }
        }
        if tick % 1024 == 0 {
            println!("{name}: {tick} ticks, {mismatched_ticks} ticks with differing storage bits");
        }
    }
    let mut ram = Value::Null;
    if name == "cpu_bubblesort" {
        project(&baseline, &mut base_host);
        project(&candidate, &mut candidate_host);
        let words = |world: &PlotWorld| {
            (0..16)
                .map(|row| {
                    (0..12).try_fold(0u16, |word, bit| {
                        let value = native_storage_bit(world, cpus::CPUS[2].origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row))?;
                        Some(word | (u16::from(value) << bit))
                    })
                })
                .collect::<Vec<_>>()
        };
        let expected = words(&native);
        assert_eq!(expected, (0..16).map(Some).collect::<Vec<Option<u16>>>(), "native complete sort");
        assert_eq!(words(&base_host), expected, "unchanged O5 complete sort");
        ram = json!({"native":expected,"contracted":words(&candidate_host),"matches":words(&candidate_host)==expected});
    }
    let report = json!({"device":name,"admission":"closed".to_owned()+if relax_electrical_boundaries { "+relaxed-electrical-consumers" } else { "" },"relaxed_electrical_consumer_boundary":relax_electrical_boundaries,"ticks":tick,"observed_storage_pistons":memories.len(),
        "storage_scope":if name == "cpu_bubblesort" { "768 documented RAM bits in four banks" } else if name == "counter" { "16 independently certified upper-storage BUD pistons" } else { "all ordinary pistons outside the instant kernel; storage candidates, not a certified FPU memory map" },
        "unavailable_storage_samples":unavailable_samples,"availability_difference_samples":availability_difference_samples,
        "first_availability_difference":first_availability_difference,"native_bit_transitions":native_bit_transitions,"active_storage_pistons":changed_cells.iter().filter(|&&changed|changed).count(),
        "admitted_multi_wire_nets":census.closed.iter().filter(|net|net.wires>1).count(),
        "admitted_multi_wire_members":census.closed.iter().filter(|net|net.wires>1).map(|net|net.wires).sum::<usize>(),
        "declined_nets_and_wires":census.declines,"contracted_wires":contracted_wires,
        "definition":"not extended when a piston base is present; moving bases are unavailable, as in the frozen RAM decoder; only simultaneously readable bits compared",
        "excluded":"wire strengths, revisions, pending work, and availability-only differences do not count as wrong memory bits",
        "native_reference":"every stored bit checked every tick against unchanged O5 execution",
        "first_memory_difference":first_memory_difference,
        "first_any_resource_difference":first_resource_difference,"mismatched_memory_ticks":mismatched_ticks,
        "last_memory_difference_tick":last_memory_difference_tick,"maximum_differing_bits":maximum_differing_bits,
        "mismatched_64_tick_checkpoints":mismatched_64_tick_checkpoints,
        "final_memory_matches":last_memory_difference_tick!=Some(tick),"final_ram":ram,
        "memory_equivalent_for_tested_history":mismatched_ticks==0,
        "diagnostic_tick_seconds":{"baseline":baseline_seconds,"contracted":contracted_seconds,"clean_paired_benchmark":false},
        "baseline_counts":format!("{:?}",baseline.execution_counts()),"contracted_counts":format!("{:?}",candidate.execution_counts())});
    let directory = root().join("target/instant-wire-nodes/memory-only");
    std::fs::create_dir_all(&directory).unwrap();
    let suffix = if relax_electrical_boundaries {
        "_relaxed_electrical_consumers"
    } else {
        ""
    };
    std::fs::write(
        directory.join(format!("{name}{suffix}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{name}: {tick} ticks; {mismatched_ticks} with differing storage bits; {}",
        report["diagnostic_tick_seconds"]
    );
}
