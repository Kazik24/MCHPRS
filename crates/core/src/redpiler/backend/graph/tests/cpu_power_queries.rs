//! Differential coverage for O6 live piston Main/Quasi power predicates.
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::CompilerOptions;
use crate::world::storage::Chunk;
use sha2::{Digest, Sha256};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);

fn rilax_bud_world() -> (PlotWorld, (BlockPos, BlockPos)) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest_path = root.join("test_data/piston-research/fixtures/rilax_memory_bank_bud.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(
        manifest["dimensions"],
        serde_json::json!([clipboard.size_x, clipboard.size_y, clipboard.size_z])
    );
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    let first = ORIGIN;
    let last = first + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1);
    paste_clipboard(
        &mut world,
        &clipboard,
        first + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    (world, (first, last))
}

fn set_test_state(backend: &mut GraphBackend, seed: u8) {
    for (index, state) in backend.state.components.iter_mut().enumerate() {
        let strength = seed.wrapping_add((index as u8).wrapping_mul(7)) % 16;
        match state {
            ComponentState::Electrical(state) => {
                state.output_strength = strength;
                state.powered = strength != 0;
            }
            ComponentState::Wire(state) => state.power = strength,
            ComponentState::Specialized => {}
        }
    }
    for (index, state) in backend.state.pistons.iter_mut().enumerate() {
        state.extended = (seed as usize + index).is_multiple_of(2);
        state.head_present = (seed as usize + index / 2).is_multiple_of(3);
    }
    for (index, state) in backend.state.resources.iter_mut().enumerate() {
        let contacts = backend.program.resources()[index].contacts.as_ref();
        if !contacts.is_empty() {
            state.at = contacts[(seed as usize + index) % contacts.len()];
        }
        state.enabled = (seed as usize + index).is_multiple_of(2);
    }
    for (index, signal) in backend.state.classic_signals.iter_mut().enumerate() {
        *signal = seed.wrapping_add(index as u8) % 16;
    }
    for (index, active) in backend.state.classic_active.iter_mut().enumerate() {
        *active = (seed as usize + index).is_multiple_of(2);
    }
}

#[test]
fn rilax_o6_main_quasi_power_queries_match_recursive_reads_at_each_cut() {
    let (world, bounds) = rilax_bud_world();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
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
    let lowered = prepared.lower(&monitor).unwrap();
    let program = lowered.program.clone();
    let state = lowered.state.clone();
    let entries = program.cpu_power_queries().entries.clone();
    assert!(!entries.is_empty(), "O6 should publish some live piston power queries");
    let reader_position = ORIGIN + BlockPos::new(0, 5, 24);
    let writer_position = ORIGIN + BlockPos::new(0, 9, 24);
    let actor_at = |position| {
        program
            .pistons()
            .iter()
            .position(|spec| spec.position == position)
            .map(|index| PistonId::new(index as u32))
            .expect("the Rilax sample BUD reader/writer are compiled actors")
    };
    let reader = actor_at(reader_position);
    let writer = actor_at(writer_position);
    let reader_node = program.pistons()[reader.index()].node;
    let reader_entry = program.cpu_power_query_entry(reader).expect("O6 query for the selected read gate");
    let power_plan = program.cpu_power_queries();
    let quasi_target = PowerTarget::Input {
        node: reader_node,
        port: InputPort::Quasi,
    };
    let (resource, source) = program
        .power_links_to(quasi_target)
        .iter()
        .filter_map(|link| match program.sites()[link.from.index()].source {
            SiteSource::Contact(contact) => Some(contact),
            _ => None,
        })
        .flat_map(|contact| program.resources_at_contact(contact).iter().map(move |&resource| (resource, contact)))
        .find(|(resource, _)| program.resource_for_piston(writer) == Some(*resource))
        .expect("the lower Quasi source is the payload owned by its upper BUD writer");
    assert!(program.resources_at_contact(source).contains(&resource));
    assert!(
        entries.iter().any(|entry| {
            [entry.main, entry.quasi].into_iter().any(|predicate| {
                power_plan.packed.reads[predicate.read_start as usize..predicate.read_start as usize + predicate.read_len as usize]
                    .iter()
                    .any(|read| !matches!(read, CpuRead::Input { .. }))
            })
        }),
        "the global power plan should lower ordinary roots to live leaves, not only fallback inputs"
    );

    let mut compiled = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
    let mut recursive = GraphBackend::new(program, state, 0).unwrap();
    recursive.use_general_cpu_power_queries();

    // These are separate live samples, so changing a BUD payload or wire strength
    // between them also checks the request/acceptance use of the same predicate.
    for seed in 0..12u8 {
        set_test_state(&mut compiled, seed);
        set_test_state(&mut recursive, seed);
        for entry in &entries {
            let node = compiled.program.pistons()[entry.actor.index()].node;
            assert_eq!(
                compiled.cpu_power_predicate(entry.main),
                recursive.input_strength(node, InputPort::Main) > 0,
                "Main actor {:?} at cut {seed}",
                entry.actor
            );
            assert_eq!(
                compiled.cpu_power_predicate(entry.quasi),
                recursive.input_strength(node, InputPort::Quasi) > 0,
                "Quasi actor {:?} at cut {seed}",
                entry.actor
            );
            assert_eq!(
                compiled.piston_powered(entry.actor),
                recursive.piston_powered(entry.actor),
                "combined actor {:?} at cut {seed}",
                entry.actor
            );
        }

        let mut enabled_results = [false; 2];
        for (index, enabled) in [false, true].into_iter().enumerate() {
            for backend in [&mut compiled, &mut recursive] {
                backend.state.resources[resource.index()].at = source;
                backend.state.resources[resource.index()].enabled = enabled;
            }
            let planned_quasi = compiled.cpu_power_predicate(reader_entry.quasi);
            assert_eq!(
                planned_quasi,
                recursive.input_strength(reader_node, InputPort::Quasi) > 0,
                "ordinary BUD Quasi actor {reader:?}, enabled={enabled}"
            );
            enabled_results[index] = planned_quasi;
        }
        assert_ne!(enabled_results[0], enabled_results[1], "the live payload flip must reach a later sample");
    }

    let cloned_lowered = || LoweredCircuit {
        program: lowered.program.clone(),
        state: lowered.state.clone(),
        logical_tick: lowered.logical_tick,
        imported_ticks: lowered.imported_ticks.clone(),
        positions: lowered.positions.clone(),
    };
    let mut planned = GraphBackend::from_lowered(cloned_lowered(), &monitor).unwrap();
    let mut general = GraphBackend::from_lowered(cloned_lowered(), &monitor).unwrap();
    general.use_general_cpu_power_queries();
    planned.set_test_recording(true);
    general.set_test_recording(true);
    assert!(planned.fixed_classic.is_none(), "the sample must exercise the dynamic graph query path");
    assert!(
        planned.logical_device.is_none(),
        "the ordinary Rilax memory graph must use native scheduler cuts"
    );
    let write_enable = ORIGIN + BlockPos::new(19, 12, 1);
    planned.use_block(write_enable).unwrap();
    general.use_block(write_enable).unwrap();
    assert_backend_power_state_equal(&planned, &general, "write-enable request cut");
    for tick in 0..36 {
        planned.tick().unwrap();
        general.tick().unwrap();
        assert_backend_power_state_equal(&planned, &general, &format!("write-enable tick {tick}"));
    }
    planned.use_block(write_enable).unwrap();
    general.use_block(write_enable).unwrap();
    assert_backend_power_state_equal(&planned, &general, "write-enable off cut");
    for tick in 0..36 {
        planned.tick().unwrap();
        general.tick().unwrap();
        assert_backend_power_state_equal(&planned, &general, &format!("write-disable tick {tick}"));
    }
    let trace = planned.native_piston_trace();
    assert!(
        trace
            .iter()
            .any(|entry| matches!(entry.operation, crate::redstone::piston::trace::Operation::Sample { .. }))
    );
    assert!(
        trace
            .iter()
            .any(|entry| matches!(entry.operation, crate::redstone::piston::trace::Operation::Applied(_)))
    );
}

fn assert_backend_power_state_equal(planned: &GraphBackend, general: &GraphBackend, cut: &str) {
    assert_eq!(planned.state, general.state, "state at {cut}");
    assert_eq!(planned.scheduled_ticks(), general.scheduled_ticks(), "scheduler at {cut}");
    assert_eq!(planned.future.pending, general.future.pending, "pending identities at {cut}");
    assert_eq!(planned.future.pending_nodes, general.future.pending_nodes, "pending index at {cut}");
    assert_eq!(planned.future.piston_requests, general.future.piston_requests, "requests at {cut}");
    assert_eq!(
        planned.future.deferred_pistons, general.future.deferred_pistons,
        "deferred actions at {cut}"
    );
    assert_eq!(planned.future.clock.logical_tick, general.future.clock.logical_tick, "clock at {cut}");
    assert_eq!(
        planned.test_recorder.observed_operations, general.test_recorder.observed_operations,
        "sample/request/accept trace at {cut}"
    );
    assert_eq!(
        planned.native_piston_trace(),
        general.native_piston_trace(),
        "native-format piston trace at {cut}"
    );
}
