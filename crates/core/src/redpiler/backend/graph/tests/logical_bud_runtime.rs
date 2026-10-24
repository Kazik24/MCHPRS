//! Differential coverage for the first automatic O6 logical BUD owner runtime.
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::CompilerOptions;
use crate::redstone;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Block, Lever, LeverFace};
use mchprs_blocks::{BlockDirection, BlockFace};
use sha2::{Digest, Sha256};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);

fn rilax_world() -> (PlotWorld, (BlockPos, BlockPos)) {
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

fn single_bud_world() -> (PlotWorld, BlockPos, BlockPos, (BlockPos, BlockPos)) {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let base = BlockPos::new(5, 20, 5);
    let lever = base.offset(BlockFace::Top).offset(BlockFace::East);
    world.set_block(
        base,
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(base.offset(BlockFace::East), Block::RedstoneBlock);
    world.set_block(lever.offset(BlockFace::East), Block::Stone {});
    world.set_block(
        base.offset(BlockFace::East).offset(BlockFace::East).offset(BlockFace::North),
        Block::RedstoneLamp { lit: false },
    );
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
        },
    );
    (world, base, lever, bounds)
}

fn set_native_bud_lever_level(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
}

#[test]
fn single_logical_bud_slice_matches_native_piston_trace_and_handoff() {
    let (mut native, base, lever, bounds) = single_bud_world();
    let (mut graph_world, _, _, _) = single_bud_world();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let mut backend = GraphBackend::compile_with_options(&graph_world, bounds, vec![], &options, monitor).unwrap();
    assert_eq!(
        backend.program.logical_bud_owners().len(),
        1,
        "the fixture should contain one admitted owner"
    );
    let actor = backend.program.logical_bud_owners()[0].actor;
    let piston_node = backend.program.pistons()[actor.index()].node;
    let lever_node = backend.positions[&lever];
    let lamp = base.offset(BlockFace::East).offset(BlockFace::East).offset(BlockFace::North);
    let (first, last) = bounds;
    let positions = (first.y..=last.y)
        .flat_map(|y| (first.z..=last.z).flat_map(move |z| (first.x..=last.x).map(move |x| BlockPos::new(x, y, z))))
        .collect::<Vec<_>>();
    backend.set_test_recording(true);

    set_native_bud_lever_level(&mut native, lever, true);
    backend.set_node(lever_node, true, 15, None, false).unwrap();
    assert!(
        backend.native_piston_trace().is_empty(),
        "a live QC level change alone must not sample the owner"
    );
    assert_eq!(backend.input_strength(piston_node, InputPort::Main), 0);
    assert!(backend.input_strength(piston_node, InputPort::Quasi) > 0);
    assert!(redstone::piston::should_piston_extend(&native, mchprs_blocks::BlockFacing::East, base));
    let native_trace = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_trace,
        "native and logical qualifying notification"
    );
    let native_repeat = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_repeat,
        "same-level repeat keeps native sample ordering"
    );
    assert_eq!(backend.future.piston_requests.len(), 1, "logical owner deduplicates repeated requests");
    assert_eq!(native.piston_state().events.len(), 1, "native piston deduplicates repeated requests");
    set_native_bud_lever_level(&mut native, lever, false);
    backend.set_node(lever_node, false, 0, None, false).unwrap();
    assert_eq!(backend.input_strength(piston_node, InputPort::Quasi), 0);
    let start = backend.native_piston_trace().len();
    let native_trace = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
    backend.tick().unwrap();
    assert_eq!(&backend.native_piston_trace()[start..], native_trace, "cancelled extension tick");
    backend.flush(&mut graph_world, false);
    assert_eq!(graph_world.get_block(lamp), native.get_block(lamp), "lamp after canceled request");
    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));

    set_native_bud_lever_level(&mut native, lever, true);
    backend.set_node(lever_node, true, 15, None, false).unwrap();
    let native_trace = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_trace,
        "native and logical accepted extension sample"
    );
    let start = backend.native_piston_trace().len();
    let native_trace = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
    backend.tick().unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_trace,
        "native piston trace at extension acceptance"
    );
    backend.flush(&mut graph_world, false);
    assert_eq!(
        graph_world.get_block(lamp),
        native.get_block(lamp),
        "lamp during pending extension publication"
    );
    assert!(backend.future.deferred_pistons.iter().any(|event| event.piston == actor));
    let native_busy_trace = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_busy_trace,
        "busy extension sampling preserves the pending publication"
    );
    for tick in 1..3 {
        let start = backend.native_piston_trace().len();
        let native_trace = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
        backend.tick().unwrap();
        assert_eq!(
            &backend.native_piston_trace()[start..],
            native_trace,
            "native piston trace at extension tick {tick}"
        );
        backend.flush(&mut graph_world, false);
        assert_eq!(graph_world.get_block(lamp), native.get_block(lamp), "lamp during extension publication");
    }
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(backend.state.pistons[actor.index()].extended);
    assert_eq!(native.get_block(lamp), Block::RedstoneLamp { lit: true });

    set_native_bud_lever_level(&mut native, lever, false);
    backend.set_node(lever_node, false, 0, None, false).unwrap();
    let native_trace = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_trace,
        "native and logical samples at retraction request"
    );
    let start = backend.native_piston_trace().len();
    let native_trace = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
    backend.tick().unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_trace,
        "native piston trace at retraction acceptance"
    );
    backend.flush(&mut graph_world, false);
    assert_eq!(
        graph_world.get_block(lamp),
        native.get_block(lamp),
        "lamp during pending retraction publication"
    );
    set_native_bud_lever_level(&mut native, lever, true);
    backend.set_node(lever_node, true, 15, None, false).unwrap();
    let native_busy_retract_trace = crate::redstone::piston::trace::capture(|| {
        let block = native.get_block(base);
        redstone::update(block, &mut native, base, None);
    });
    let start = backend.native_piston_trace().len();
    backend
        .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert_eq!(
        &backend.native_piston_trace()[start..],
        native_busy_retract_trace,
        "busy retraction sampling preserves its publication"
    );

    assert_eq!(backend.counts.piston_samples, 0, "the claimed owner must leave the physical sample path");
    assert_eq!(
        backend.counts.piston_actions, 0,
        "the claimed owner must leave physical connection mutation"
    );
    assert!(backend.counts.logical_bud_samples > 0);
    assert!(backend.counts.logical_bud_accepts > 0);
    assert!(backend.counts.logical_bud_publications > 0);
    backend.reset(&mut graph_world);
    assert_native_future_equal(&graph_world, &native, &positions, "single-owner native handoff");
    for tick in 1..=5 {
        graph_world.tick_interpreted();
        native.tick_interpreted();
        assert_native_future_equal(&graph_world, &native, &positions, &format!("native continuation tick {tick}"));
    }
}

fn assert_native_future_equal(graph: &PlotWorld, native: &PlotWorld, positions: &[BlockPos], label: &str) {
    let native_future = super::adder_tests::future(native, positions);
    let graph_future = super::adder_tests::future(graph, positions);
    assert_eq!(
        super::adder_tests::first_difference(&native_future, &graph_future),
        None,
        "native/Graph continuation at {label}"
    );
}

#[test]
fn rilax_logical_bud_runtime_matches_physical_owner_at_every_tick_cut() {
    let (world, bounds) = rilax_world();
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
    assert!(!lowered.program.logical_bud_owners().is_empty(), "Rilax should contain a certified owner");
    let clone_lowered = || LoweredCircuit {
        program: lowered.program.clone(),
        state: lowered.state.clone(),
        logical_tick: lowered.logical_tick,
        imported_ticks: lowered.imported_ticks.clone(),
        positions: lowered.positions.clone(),
    };
    let mut logical = GraphBackend::from_lowered(clone_lowered(), &monitor).unwrap();
    let mut physical = GraphBackend::from_lowered(clone_lowered(), &monitor).unwrap();
    assert!(
        logical.logical_bud_owner_by_actor.iter().any(Option::is_some),
        "the cold owner plan should activate"
    );
    physical.disable_logical_bud_owners_for_test();
    logical.set_test_recording(true);
    physical.set_test_recording(true);

    let data = ORIGIN + BlockPos::new(14, 14, 25);
    let address = ORIGIN + BlockPos::new(19, 12, 3);
    let write_enable = ORIGIN + BlockPos::new(19, 12, 1);
    toggle_pair(&mut logical, &mut physical, data, "data input");
    tick_pair(&mut logical, &mut physical, 24, "settle data");
    toggle_pair(&mut logical, &mut physical, address, "address input");
    tick_pair(&mut logical, &mut physical, 24, "settle address");

    let owner_position = ORIGIN + BlockPos::new(0, 9, 24);
    let owner = lowered
        .program
        .logical_bud_owners()
        .iter()
        .map(|owner| owner.actor)
        .find(|&actor| lowered.program.pistons()[actor.index()].position == owner_position)
        .expect("the selected Rilax writer should have a complete logical owner plan");
    assert!(
        logical.logical_bud_owner_by_actor[owner.index()].is_some(),
        "the selected owner should activate"
    );
    let prior_accepts = logical.counts.logical_bud_accepts;
    let same_level_route = lowered.program.logical_bud_owner(owner).unwrap().sample_routes[0].route;
    logical.deliver_route(same_level_route, None).unwrap();
    physical.deliver_route(same_level_route, None).unwrap();
    assert_same_cut(&logical, &physical, "same-level owner sample notification");
    toggle_pair(&mut logical, &mut physical, write_enable, "write enable");
    for tick in 0..16 {
        if logical.counts.logical_bud_accepts > prior_accepts && logical.future.deferred_pistons.iter().any(|event| event.piston == owner) {
            break;
        }
        tick_pair(&mut logical, &mut physical, 1, &format!("request wait {tick}"));
    }
    assert!(
        logical.counts.logical_bud_accepts > prior_accepts,
        "write enable should accept a BUD action"
    );
    assert!(
        logical.future.deferred_pistons.iter().any(|event| event.piston == owner),
        "the accepted owner transition should retain its pending publication"
    );

    // Perturb inputs during the busy/deferred interval, then issue same-level updates without
    // waiting for the normal 24-tick quiet period.
    toggle_pair(&mut logical, &mut physical, data, "data update while busy");
    tick_pair(&mut logical, &mut physical, 1, "one tick after busy data update");
    toggle_pair(&mut logical, &mut physical, address, "address level change while busy");
    logical.deliver_route(same_level_route, None).unwrap();
    physical.deliver_route(same_level_route, None).unwrap();
    assert_same_cut(&logical, &physical, "same-level owner notification while busy");
    tick_pair(&mut logical, &mut physical, 2, "two ticks through publication");
    tick_pair(&mut logical, &mut physical, 21, "finish write publication");
    toggle_pair(&mut logical, &mut physical, write_enable, "write disable");
    tick_pair(&mut logical, &mut physical, 1, "retract request cut");
    toggle_pair(&mut logical, &mut physical, data, "second data update during retraction");
    tick_pair(&mut logical, &mut physical, 24, "finish retraction");

    assert!(logical.counts.logical_bud_samples > 0, "the logical sampling path must execute");
    assert!(
        logical.counts.logical_bud_accepts > 0,
        "the fixture must exercise an accepted logical transition"
    );
    assert!(
        logical.counts.logical_bud_publications > 0,
        "the fixture must exercise delayed publication"
    );
    assert!(
        logical.test_recorder.logical_bud_direct_samples.get(&owner).copied().unwrap_or_default() > 0,
        "the selected owner must use the typed owner-sample path"
    );
    assert!(
        logical
            .test_recorder
            .logical_bud_boundary_callbacks
            .get(&owner)
            .copied()
            .unwrap_or_default()
            > 0,
        "the selected owner must preserve Graph boundary callbacks"
    );
    assert!(
        logical
            .test_recorder
            .observed_operations
            .iter()
            .any(|operation| matches!(operation, GraphObservedOperation::Callback { .. })),
        "retained boundary deliveries should execute generic callbacks"
    );
    assert_eq!(physical.counts.logical_bud_accepts, 0);
    assert_eq!(
        physical.counts.piston_samples,
        logical.counts.piston_samples + logical.counts.logical_bud_samples,
        "the logical owner must replace its physical sample handling"
    );
    assert_eq!(
        physical.counts.piston_actions,
        logical.counts.piston_actions + logical.counts.logical_bud_accepts,
        "the logical owner must remove its physical piston updates while preserving other actors"
    );

    // Repeat the first accepted write from a cold copy, then hand both executors back to the
    // interpreter while the owner still has a queued two-tick publication.
    let mut handoff_logical = GraphBackend::from_lowered(clone_lowered(), &monitor).unwrap();
    let mut handoff_physical = GraphBackend::from_lowered(clone_lowered(), &monitor).unwrap();
    handoff_physical.disable_logical_bud_owners_for_test();
    let handoff_owner = lowered
        .program
        .logical_bud_owners()
        .iter()
        .map(|owner| owner.actor)
        .find(|&actor| lowered.program.pistons()[actor.index()].position == owner_position)
        .unwrap();
    toggle_pair(&mut handoff_logical, &mut handoff_physical, data, "handoff data setup");
    tick_pair(&mut handoff_logical, &mut handoff_physical, 24, "handoff data settle");
    toggle_pair(&mut handoff_logical, &mut handoff_physical, address, "handoff address setup");
    tick_pair(&mut handoff_logical, &mut handoff_physical, 24, "handoff address settle");
    let handoff_sample_route = lowered.program.logical_bud_owner(handoff_owner).unwrap().sample_routes[0].route;
    handoff_logical.deliver_route(handoff_sample_route, None).unwrap();
    handoff_physical.deliver_route(handoff_sample_route, None).unwrap();
    assert_same_cut(&handoff_logical, &handoff_physical, "handoff same-level owner notification");
    handoff_logical.use_block(write_enable).unwrap();
    handoff_physical.use_block(write_enable).unwrap();
    assert_same_cut(&handoff_logical, &handoff_physical, "handoff write enable");
    for _ in 0..16 {
        if handoff_logical.future.deferred_pistons.iter().any(|event| event.piston == handoff_owner) {
            break;
        }
        handoff_logical.tick().unwrap();
        handoff_physical.tick().unwrap();
        assert_same_cut(&handoff_logical, &handoff_physical, "handoff request wait");
    }
    let pending_owners: Vec<_> = handoff_logical
        .future
        .deferred_pistons
        .iter()
        .map(|event| lowered.program.pistons()[event.piston.index()].position)
        .collect();
    assert!(
        pending_owners.contains(&owner_position),
        "handoff should occur with owner {owner_position:?} publication pending; pending owners: {pending_owners:?}"
    );
    let (mut logical_world, logical_bounds) = rilax_world();
    let (mut physical_world, physical_bounds) = rilax_world();
    assert_eq!(logical_bounds, physical_bounds);
    handoff_logical.reset(&mut logical_world);
    handoff_physical.reset(&mut physical_world);
    assert_interpreter_handoff_equal(&logical_world, &physical_world, logical_bounds, "tick 0 after reset");
    for tick in 1..=2 {
        logical_world.tick_interpreted();
        physical_world.tick_interpreted();
        assert_interpreter_handoff_equal(&logical_world, &physical_world, logical_bounds, &format!("tick {tick} after reset"));
    }
}

fn assert_same_cut(logical: &GraphBackend, physical: &GraphBackend, label: &str) {
    assert_eq!(logical.state, physical.state, "circuit state at {label}");
    assert_eq!(logical.scheduled_ticks(), physical.scheduled_ticks(), "scheduler at {label}");
    assert_eq!(logical.future.pending, physical.future.pending, "pending identities at {label}");
    assert_eq!(logical.future.pending_nodes, physical.future.pending_nodes, "pending index at {label}");
    assert_eq!(
        logical.future.piston_requests, physical.future.piston_requests,
        "piston requests at {label}"
    );
    assert_eq!(
        logical.future.deferred_pistons, physical.future.deferred_pistons,
        "deferred events at {label}"
    );
    assert_eq!(logical.future.clock.logical_tick, physical.future.clock.logical_tick, "tick at {label}");
    assert_eq!(logical.future.clock.phase, physical.future.clock.phase, "phase at {label}");
    assert_eq!(
        logical.future.clock.scheduled_advanced, physical.future.clock.scheduled_advanced,
        "tick phase state at {label}"
    );
    assert_eq!(logical_operations(logical), logical_operations(physical), "operations at {label}");
    assert_eq!(logical.native_piston_trace(), physical.native_piston_trace(), "piston trace at {label}");
}

fn toggle_pair(logical: &mut GraphBackend, physical: &mut GraphBackend, position: BlockPos, label: &str) {
    logical.use_block(position).unwrap();
    physical.use_block(position).unwrap();
    assert_same_cut(logical, physical, label);
}

fn tick_pair(logical: &mut GraphBackend, physical: &mut GraphBackend, count: usize, label: &str) {
    for tick in 0..count {
        logical.tick().unwrap();
        physical.tick().unwrap();
        assert_same_cut(logical, physical, &format!("{label}, tick {tick}"));
    }
}

fn logical_operations(backend: &GraphBackend) -> Vec<GraphObservedOperation> {
    let owned_callback_targets: FxHashSet<_> = backend
        .program
        .logical_bud_owners()
        .iter()
        .flat_map(|owner| owner.sample_routes.iter())
        .filter_map(|occurrence| {
            let delivery = backend.program.routes()[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            let DeliveryKind::Update { direction } = delivery.kind else {
                return None;
            };
            Some((backend.program.sites()[delivery.target.index()].position, direction))
        })
        .collect();
    let operations = &backend.test_recorder.observed_operations;
    operations
        .iter()
        .filter_map(|operation| {
            let absorbed_owner_sample = matches!(operation,
                GraphObservedOperation::Callback { target, direction }
                    if owned_callback_targets.contains(&(*target, *direction))
            );
            (!absorbed_owner_sample).then(|| operation.clone())
        })
        .collect()
}

fn assert_interpreter_handoff_equal(logical: &PlotWorld, physical: &PlotWorld, bounds: (BlockPos, BlockPos), label: &str) {
    let logical_state = logical.piston_state();
    let physical_state = physical.piston_state();
    assert_eq!(logical_state.logical_tick, physical_state.logical_tick, "logical tick at {label}");
    assert_eq!(logical_state.phase, physical_state.phase, "phase at {label}");
    assert_eq!(
        logical_state.scheduled_advanced, physical_state.scheduled_advanced,
        "schedule phase at {label}"
    );
    assert_eq!(logical_state.events, physical_state.events, "native piston events at {label}");
    assert_eq!(
        logical_state.next_identity, physical_state.next_identity,
        "motion identity sequence at {label}"
    );
    assert_eq!(
        logical_state.movement_cursor, physical_state.movement_cursor,
        "movement cursor at {label}"
    );
    assert_eq!(logical_state.movement_work, physical_state.movement_work, "movement work at {label}");
    let summarize_motions = |world: &PlotWorld| {
        world
            .piston_state()
            .motions
            .iter()
            .map(|motion| {
                (
                    motion.pos,
                    motion.identity,
                    motion.progress.to_bits(),
                    motion.previous_progress.to_bits(),
                    motion.last_tick,
                    format!("{:?}", motion.carried_entity),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(summarize_motions(logical), summarize_motions(physical), "motions at {label}");
    assert_eq!(
        logical.scheduler().iter_entries().collect::<Vec<_>>(),
        physical.scheduler().iter_entries().collect::<Vec<_>>(),
        "native scheduler at {label}"
    );
    let (first, last) = bounds;
    for y in first.y..=last.y {
        for z in first.z..=last.z {
            for x in first.x..=last.x {
                let position = BlockPos::new(x, y, z);
                assert_eq!(
                    logical.get_block(position),
                    physical.get_block(position),
                    "block at {position:?}, {label}"
                );
                assert_eq!(
                    format!("{:?}", logical.get_block_entity(position)),
                    format!("{:?}", physical.get_block_entity(position)),
                    "block entity at {position:?}, {label}"
                );
            }
        }
    }
}
