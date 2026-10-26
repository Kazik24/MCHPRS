//! Multiple settled devices must share the ordinary Graph scheduler without
//! taking ownership of unrelated native work.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::world::World;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{Block, LeverFace, RedstoneRepeater};
use std::sync::Arc;

const SECOND_OFFSET: BlockPos = BlockPos::new(90, 0, 0);
const THIRD_OFFSET: BlockPos = BlockPos::new(0, 0, 90);
const MANIFEST_ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const CONTROL_ORIGIN: BlockPos = BlockPos::new(160, 30, 40);

fn counter_manifest() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn manifest_position(value: &serde_json::Value, origin: BlockPos) -> BlockPos {
    origin
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
}

fn manifest_positions(value: &serde_json::Value, origin: BlockPos) -> Vec<BlockPos> {
    value.as_array().unwrap().iter().map(|entry| manifest_position(entry, origin)).collect()
}

fn copy_region(world: &mut PlotWorld, source: &PlotWorld, bounds: (BlockPos, BlockPos), offset: BlockPos) {
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                let block = source.get_block(position);
                let entity = source.get_block_entity(position).cloned();
                if block != Block::Air || entity.is_some() {
                    world.set_block(position + offset, block);
                    if let Some(entity) = entity {
                        world.set_block_entity(position + offset, entity);
                    }
                }
            }
        }
    }
}

fn combined_counter_worlds() -> (PlotWorld, (BlockPos, BlockPos)) {
    let (mut world, bounds) = super::adder_tests::counter_world();
    let (template, _) = super::adder_tests::counter_world();
    copy_region(&mut world, &template, bounds, SECOND_OFFSET);
    let full = (bounds.0.min(bounds.0 + SECOND_OFFSET), bounds.1.max(bounds.1 + SECOND_OFFSET));

    let lever = CONTROL_ORIGIN;
    let repeater = CONTROL_ORIGIN + BlockPos::new(1, 0, 0);
    let lamp = CONTROL_ORIGIN + BlockPos::new(2, 0, 0);
    for x in 0..=2 {
        world.set_block(CONTROL_ORIGIN + BlockPos::new(x, -1, 0), Block::Stone {});
    }
    world.set_block(
        lever,
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        repeater,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                delay: 1,
                facing: BlockDirection::West,
                locked: false,
                powered: false,
            },
        },
    );
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    (world, (full.0, full.1.max(lamp)))
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let support = match lever.face {
        LeverFace::Floor => mchprs_blocks::BlockFace::Bottom,
        LeverFace::Ceiling => mchprs_blocks::BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(support));
}

fn stationary_memory(world: &PlotWorld, positions: &[BlockPos]) -> Option<Vec<bool>> {
    positions
        .iter()
        .map(|&position| match world.get_block(position) {
            Block::Piston { piston } => Some(piston.extended),
            _ => None,
        })
        .collect()
}

fn published_word(world: &PlotWorld, positions: &[BlockPos]) -> Option<Vec<bool>> {
    positions
        .iter()
        .map(|&position| match world.get_block(position) {
            Block::RedstoneRepeater { repeater } => Some(!repeater.powered),
            _ => None,
        })
        .collect()
}

fn counter_publication_cut(tick: usize, last_rising_edge: Option<usize>) -> bool {
    // COUNTER publishes stable outputs every six ticks, beginning four ticks after its trigger edge.
    last_rising_edge.is_some_and(|start| tick >= start + 4 && (tick - start - 4) % 6 == 0)
}

fn outside_owners(backend: &GraphBackend, owned: &FxHashSet<NodeId>) -> Vec<(NodeId, ComponentState)> {
    backend
        .program
        .nodes()
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            let node = NodeId::new(index as u32);
            (!owned.contains(&node)).then(|| (node, backend.state.components[index].clone()))
        })
        .collect()
}

fn outside_piston_fifo(backend: &GraphBackend, owned: &FxHashSet<NodeId>) -> Vec<PistonRequest> {
    backend
        .future
        .piston_requests
        .iter()
        .copied()
        .filter(|request| {
            let node = backend.program.pistons()[request.piston.index()].node;
            !owned.contains(&node)
        })
        .collect()
}

fn outside_deferred_pistons(backend: &GraphBackend, owned: &FxHashSet<NodeId>) -> Vec<DeferredPistonEvent> {
    backend
        .future
        .deferred_pistons
        .iter()
        .copied()
        .filter(|event| {
            let node = backend.program.pistons()[event.piston.index()].node;
            !owned.contains(&node)
        })
        .collect()
}

fn outside_scheduled(backend: &GraphBackend, owned: &FxHashSet<NodeId>) -> Vec<TickEntry> {
    backend
        .scheduled_ticks()
        .into_iter()
        .filter(|entry| !backend.positions.get(&entry.pos).is_some_and(|node| owned.contains(node)))
        .collect()
}

#[test]
fn two_counter_islands_coexist_with_an_ordinary_scheduled_repeater() {
    let (mut candidate_world, bounds) = combined_counter_worlds();
    let (mut native_world, native_bounds) = combined_counter_worlds();
    let (mut physical_world, physical_bounds) = combined_counter_worlds();
    assert_eq!(native_bounds, bounds);
    assert_eq!(physical_bounds, bounds);
    let manifest = counter_manifest();
    let trigger_a = manifest_position(&manifest["ports"]["inputs"]["trigger"], MANIFEST_ORIGIN);
    let trigger_b = trigger_a + SECOND_OFFSET;
    let memory_a = manifest_positions(&manifest["ports"]["observations"]["memory"], MANIFEST_ORIGIN);
    let memory_b = memory_a.iter().map(|position| *position + SECOND_OFFSET).collect::<Vec<_>>();
    let output_a = manifest_positions(&manifest["ports"]["observations"]["repeater"], MANIFEST_ORIGIN);
    let output_b = output_a.iter().map(|position| *position + SECOND_OFFSET).collect::<Vec<_>>();
    let lever_c = CONTROL_ORIGIN;
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let ticks = candidate_world.scheduler().iter_entries().collect();
    let mut backend = GraphBackend::compile_with_options(&candidate_world, bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
    assert_eq!(
        backend.program.logical_devices().len(),
        2,
        "the two fixture clocks form separate certified islands"
    );
    assert_eq!(backend.logical_devices.len(), 2, "both certified islands must be installed");

    let mut physical = GraphBackend::new_with_positions(
        backend.program.clone(),
        backend.state.clone(),
        backend.future.clock.logical_tick,
        backend.positions.clone(),
    )
    .unwrap();
    let owned = backend
        .program
        .logical_devices()
        .iter()
        .flat_map(|device| device.scope.owned_nodes.iter().copied())
        .collect::<FxHashSet<_>>();
    let control_node = *backend.positions.get(&lever_c).expect("ordinary lever is in the compiled graph");
    assert!(
        !owned.contains(&control_node),
        "the independent control remains outside both device scopes"
    );

    let repeater_position = CONTROL_ORIGIN + BlockPos::new(1, 0, 0);
    let mut saw_repeater_powered = false;
    let mut last_rising_edge = [None, None];
    let mut trigger_powered = [false, false];
    let mut output_publications = [0, 0];
    for tick in 0..72 {
        for (position, powered) in [
            (trigger_a, (tick < 3 || (tick >= 34 && tick < 51))),
            (trigger_b, (tick >= 7 && tick < 29) || tick >= 40),
            (lever_c, (2..8).contains(&tick) || (29..35).contains(&tick) || (47..53).contains(&tick)),
        ] {
            let current = match native_world.get_block(position) {
                Block::Lever { lever } => lever.powered,
                other => panic!("expected lever at {position:?}, found {other:?}"),
            };
            if current != powered {
                if powered {
                    if position == trigger_a {
                        last_rising_edge[0] = Some(tick);
                    } else if position == trigger_b {
                        last_rising_edge[1] = Some(tick);
                    }
                }
                set_native_lever(&mut native_world, position, powered);
                backend.use_block(position).unwrap();
                physical.use_block(position).unwrap();
                if position == trigger_a {
                    trigger_powered[0] = powered;
                } else if position == trigger_b {
                    trigger_powered[1] = powered;
                }
            }
        }

        backend.tick_with_world(&mut candidate_world).unwrap();
        backend.flush(&mut candidate_world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        native_world.tick_interpreted();

        assert_eq!(
            outside_owners(&backend, &owned),
            outside_owners(&physical, &owned),
            "ordinary Graph state at tick {tick}"
        );
        assert_eq!(
            outside_piston_fifo(&backend, &owned),
            outside_piston_fifo(&physical, &owned),
            "ordinary piston FIFO at tick {tick}"
        );
        assert_eq!(
            outside_deferred_pistons(&backend, &owned),
            outside_deferred_pistons(&physical, &owned),
            "ordinary deferred piston work at tick {tick}"
        );
        assert_eq!(
            outside_scheduled(&backend, &owned),
            outside_scheduled(&physical, &owned),
            "ordinary scheduled work at tick {tick}"
        );

        for (index, (label, memory, outputs)) in [("first", &memory_a, &output_a), ("second", &memory_b, &output_b)]
            .into_iter()
            .enumerate()
        {
            if let (Some(native), Some(projected)) = (stationary_memory(&native_world, memory), stationary_memory(&candidate_world, memory)) {
                assert_eq!(projected, native, "{label} stable memory at tick {tick}");
            }
            if trigger_powered[index] && counter_publication_cut(tick, last_rising_edge[index]) {
                assert_eq!(
                    published_word(&candidate_world, outputs),
                    published_word(&native_world, outputs),
                    "{label} output bank at publication tick {tick}"
                );
                output_publications[index] += 1;
            }
        }
        assert_eq!(
            candidate_world.get_block(CONTROL_ORIGIN + BlockPos::new(2, 0, 0)),
            native_world.get_block(CONTROL_ORIGIN + BlockPos::new(2, 0, 0)),
            "ordinary lamp follows the repeater through native scheduling"
        );
        let Block::RedstoneRepeater { repeater } = candidate_world.get_block(repeater_position) else {
            panic!("ordinary repeater disappeared");
        };
        saw_repeater_powered |= repeater.powered;
    }
    assert!(
        output_publications.iter().all(|count| *count > 0),
        "both counters must reach native publication cuts: {output_publications:?}"
    );
    assert!(saw_repeater_powered, "the independent repeater must receive input and turn on");

    let mut piston_positions = backend
        .program
        .pistons()
        .iter()
        .map(|piston| backend.program.nodes()[piston.node.index()].position.unwrap())
        .collect::<Vec<_>>();
    piston_positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    piston_positions.dedup();

    // Hand off from a stopped logical cut after the COUNTER's supported 24-tick OFF drain.
    set_native_lever(&mut native_world, trigger_b, false);
    backend.use_block(trigger_b).unwrap();
    physical.use_block(trigger_b).unwrap();
    for tick in 0..24 {
        backend.tick_with_world(&mut candidate_world).unwrap();
        backend.flush(&mut candidate_world, false);
        physical.tick_with_world(&mut physical_world).unwrap();
        physical.flush(&mut physical_world, false);
        native_world.tick_interpreted();
        for (label, memory) in [("first", &memory_a), ("second", &memory_b)] {
            if let (Some(native), Some(projected)) = (stationary_memory(&native_world, memory), stationary_memory(&candidate_world, memory)) {
                assert_eq!(projected, native, "{label} stopped memory at handoff prelude tick {tick}");
            }
        }
    }
    assert_eq!(
        stationary_memory(&candidate_world, &memory_a).expect("first counter is stationary after OFF drain"),
        stationary_memory(&native_world, &memory_a).expect("native first counter is stationary after OFF drain"),
        "first counter memory after the supported OFF drain"
    );
    assert_eq!(
        stationary_memory(&candidate_world, &memory_b).expect("second counter is stationary after OFF drain"),
        stationary_memory(&native_world, &memory_b).expect("native second counter is stationary after OFF drain"),
        "second counter memory after the supported OFF drain"
    );
    assert_eq!(
        published_word(&candidate_world, &output_a),
        published_word(&native_world, &output_a),
        "first counter output after the supported OFF drain"
    );
    assert_eq!(
        published_word(&candidate_world, &output_b),
        published_word(&native_world, &output_b),
        "second counter output after the supported OFF drain"
    );
    backend.reset(&mut candidate_world);
    for tick in 0..8 {
        native_world.tick_interpreted();
        candidate_world.tick_interpreted();
        assert_eq!(
            super::adder_tests::future(&native_world, &piston_positions),
            super::adder_tests::future(&candidate_world, &piston_positions),
            "native continuation handoff at tick {tick}"
        );
    }
}

#[test]
fn unrelated_preexisting_repeater_tick_does_not_block_counter_admission() {
    let (mut candidate_world, bounds) = combined_counter_worlds();
    let (mut native_world, native_bounds) = combined_counter_worlds();
    assert_eq!(native_bounds, bounds);
    let lever = CONTROL_ORIGIN;
    let repeater = CONTROL_ORIGIN + BlockPos::new(1, 0, 0);
    set_native_lever(&mut candidate_world, lever, true);
    set_native_lever(&mut native_world, lever, true);
    assert!(
        candidate_world.scheduler().iter_entries().any(|entry| entry.pos == repeater),
        "the ordinary repeater has queued work before Graph compilation"
    );

    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let ticks = candidate_world.scheduler().iter_entries().collect();
    let mut backend = GraphBackend::compile_with_options(&candidate_world, bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
    assert_eq!(
        backend.logical_devices.len(),
        2,
        "unrelated scheduled work must not decline either counter"
    );
    assert!(backend.scheduled_ticks().iter().any(|entry| entry.pos == repeater));

    let mut saw_powered = false;
    for tick in 0..4 {
        backend.tick_with_world(&mut candidate_world).unwrap();
        backend.flush(&mut candidate_world, false);
        native_world.tick_interpreted();
        let Block::RedstoneRepeater { repeater: graph } = candidate_world.get_block(repeater) else {
            panic!("ordinary repeater disappeared");
        };
        let Block::RedstoneRepeater { repeater: native } = native_world.get_block(repeater) else {
            panic!("native repeater disappeared");
        };
        assert_eq!(graph.powered, native.powered, "preexisting outside tick at {tick}");
        saw_powered |= graph.powered;
    }
    assert!(saw_powered, "the unrelated repeater must execute its queued tick");
}

#[test]
fn pending_work_inside_a_counter_scope_prevents_activation() {
    let (mut world, bounds) = super::adder_tests::counter_world();
    let trigger = manifest_position(&counter_manifest()["ports"]["inputs"]["trigger"], MANIFEST_ORIGIN);
    set_native_lever(&mut world, trigger, true);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    assert!(!ticks.is_empty(), "triggering the controller must create native pending work");

    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let backend = GraphBackend::compile_with_options(&world, bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
    assert!(
        backend.logical_devices.is_empty(),
        "a controller with its own pending event is not a stable activation entry"
    );
}

#[test]
fn one_malformed_controller_does_not_prevent_other_counter_islands_from_installing() {
    let (mut world, bounds) = combined_counter_worlds();
    let (template, template_bounds) = super::adder_tests::counter_world();
    copy_region(&mut world, &template, template_bounds, THIRD_OFFSET);
    let full_bounds = (bounds.0.min(bounds.0 + THIRD_OFFSET), bounds.1.max(bounds.1 + THIRD_OFFSET));

    let malformed_trigger = manifest_position(&counter_manifest()["ports"]["inputs"]["trigger"], MANIFEST_ORIGIN) + THIRD_OFFSET;
    world.set_block(malformed_trigger, Block::Stone {});

    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let ticks = world.scheduler().iter_entries().collect();
    let backend = GraphBackend::compile_with_options(&world, full_bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
    assert_eq!(
        backend.program.logical_devices().len(),
        2,
        "the malformed third controller is declined locally"
    );
    assert_eq!(backend.logical_devices.len(), 2, "both valid controllers still install");
    assert!(
        !backend.positions.contains_key(&malformed_trigger),
        "the unsupported trigger should remain on ordinary Graph"
    );
}
