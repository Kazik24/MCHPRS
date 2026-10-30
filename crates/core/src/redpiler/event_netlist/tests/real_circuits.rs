//! Offline executable gates for the pinned adder and counter fixtures.
use crate::plot::{
    PLOT_WIDTH, PlotWorld,
    worldedit::{load_schematic, paste_clipboard},
};
use crate::redpiler::event_netlist::executor::EventState;
use crate::redpiler::event_netlist::scratch::{ScratchEntry, ScratchWorld, discover_watched_wire_sources, discovered_wire_strength};
use crate::redpiler::program_graph::{
    CircuitNodeKind, ComponentState, ContactId, InputPort, LoweredCircuit, PistonId, PistonLane, PowerPort, PowerTarget, ResourceId,
};
use crate::redpiler::{CompilerOptions, TaskMonitor, analysis::AnalysisLimits, passes};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);

fn fixture(id: &str) -> (PlotWorld, (BlockPos, BlockPos), Value) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest["sha256"].as_str().unwrap());
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(
        &mut world,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    (world, (ORIGIN - BlockPos::new(1, 1, 1), ORIGIN + size), manifest)
}

fn lower(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Result<LoweredCircuit, String> {
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::default();
    let prepared = passes::rich::prepare(
        world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .map_err(|error| error.to_string())?
        .lower(&monitor)
        .map_err(|error| error.to_string())
}

#[test]
fn discovered_static_wire_matches_native_piston() {
    use mchprs_blocks::{
        BlockDirection, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
    };
    let lever = ORIGIN;
    let wire = ORIGIN + BlockPos::new(1, 0, 0);
    let wire2 = ORIGIN + BlockPos::new(2, 0, 0);
    let mask = ORIGIN + BlockPos::new(1, 0, -1);
    let piston = ORIGIN + BlockPos::new(3, 0, 0);
    let bounds = (ORIGIN - BlockPos::new(3, 3, 3), ORIGIN + BlockPos::new(5, 3, 3));
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(wire2.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(mask.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::None,
                RedstoneWireSide::None,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                0,
            ),
        },
    );
    world.set_block(wire2, world.get_block(wire));
    world.set_block(mask, world.get_block(lever));
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let discovery = discover_watched_wire_sources(&world, bounds).unwrap();
    let lowered = lower(&world, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile_with_wire_discovery(&lowered, &discovery).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    assert!(discovery.sources.iter().any(|source| source.wire == wire2 && source.attenuation == 1));
    assert!(discovery.sources.iter().any(|source| source.source == mask));
    for wire in [wire, wire2] {
        let node = compiled.positions[&wire];
        let mut actual: Vec<_> = compiled.program.input_links[compiled.program.input_ranges[node.index()].clone()]
            .iter()
            .map(|&id| {
                let link = &compiled.program.links[id];
                assert!(link.condition.is_none());
                (link.source.index(), link.attenuation)
            })
            .collect();
        let mut expected: Vec<_> = discovery
            .sources
            .iter()
            .filter(|source| source.wire == wire)
            .map(|source| (compiled.positions[&source.source].index(), source.attenuation))
            .collect();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
        let expected_watchers: Vec<_> = discovery
            .watchers
            .iter()
            .filter(|watch| watch.wire == wire)
            .map(|_| compiled.program.pistons[0].input)
            .collect();
        assert_eq!(&*compiled.program.wire_watchers[node.index()], expected_watchers);
    }
    assert_eq!(event.component(compiled.positions[&mask]).unwrap().output, 0);
    // The two on sources mask each other; their individual transitions must not change dust power.
    for (source, powered, extended, actions) in [
        (lever, true, true, 1),
        (mask, true, true, 1),
        (lever, false, true, 1),
        (mask, false, false, 2),
        (lever, true, true, 3),
    ] {
        let native_before: Vec<_> = [lever, mask, wire, wire2, piston].into_iter().map(|pos| world.get_block(pos)).collect();
        let strengths_before = [wire, wire2].map(|pos| event.component(compiled.positions[&pos]).unwrap().output);
        event.set_input(compiled.positions[&source], if powered { 15 } else { 0 }).unwrap();
        if (source == mask && powered) || (source == lever && !powered) {
            assert_eq!(
                strengths_before,
                [wire, wire2].map(|pos| event.component(compiled.positions[&pos]).unwrap().output)
            );
        }
        for _ in 0..4 {
            event.tick().unwrap();
        }
        assert_eq!(
            native_before,
            [lever, mask, wire, wire2, piston]
                .into_iter()
                .map(|pos| world.get_block(pos))
                .collect::<Vec<_>>()
        );
        set_native_lever(&mut world, source, powered);
        for pos in [wire, wire2] {
            let Block::RedstoneWire { wire: native_wire } = world.get_block(pos) else {
                unreachable!()
            };
            assert_eq!(event.component(compiled.positions[&pos]).unwrap().output, native_wire.power);
        }
        for _ in 0..4 {
            world.tick_interpreted();
        }
        let Block::Piston { piston: native_piston } = world.get_block(piston) else {
            unreachable!()
        };
        assert_eq!(native_piston.extended, extended);
        let state = event.piston(PistonId::new(0)).unwrap();
        assert_eq!(state.extended, native_piston.extended);
        assert_eq!(state.head_present, extended);
        assert!(!state.pending_motion && !event.has_pending_work());
        assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
        assert_eq!(world.scheduler().iter_entries().count(), 0);
        assert_eq!(event.counts().piston_actions, actions);
        assert_eq!(event.counts().motion_completions, actions);
        assert_eq!(event.counts().resource_movements, 0);
    }
}

#[test]
fn real_fixture_inventory_reports_executable_admission() {
    for id in ["adder_1bit", "adder_11bits", "counter_basic"] {
        let (world, bounds, manifest) = fixture(id);
        let lowered = lower(&world, bounds).unwrap_or_else(|error| panic!("{id} preparation/lowering failed: {error}"));
        let mut kinds = BTreeMap::new();
        for node in lowered.program.nodes() {
            let kind = match &node.kind {
                CircuitNodeKind::Electrical(kind) => format!("{kind:?}"),
                CircuitNodeKind::Wire(_) => "Wire".into(),
                CircuitNodeKind::Piston(_) => "Piston".into(),
                CircuitNodeKind::Contact(_) => "Contact".into(),
                CircuitNodeKind::Observer { .. } => "Observer".into(),
                other => format!("{other:?}"),
            };
            *kinds.entry(kind).or_insert(0usize) += 1;
        }
        let mut lanes = BTreeMap::new();
        for piston in lowered.program.pistons() {
            let kind = match piston.lane {
                PistonLane::Empty => "empty",
                PistonLane::SingleResource { .. } => "single-resource",
            };
            *lanes.entry((piston.sticky, kind)).or_insert(0usize) += 1;
        }
        let event = crate::redpiler::event_netlist::compiler::compile(&lowered).err();
        println!(
            "{id}: dimensions={} nodes={} wires={} pistons={} resources={} contacts={} routes={} power_links={} scheduled={} kinds={kinds:?} lanes={lanes:?}",
            manifest["dimensions"],
            lowered.program.nodes().len(),
            lowered.program.wires().len(),
            lowered.program.pistons().len(),
            lowered.program.resources().len(),
            lowered.program.contacts().len(),
            lowered.program.routes().len(),
            lowered.program.power_links().len(),
            world.scheduler().iter_entries().count()
        );
        println!("{id}: event admission={event:?}");
        assert!(!lowered.program.pistons().is_empty(), "fixture must contain real piston actors");
    }
}

#[test]
#[ignore = "event compilation succeeds, but the counter release diverges at piston (56,36,44)"]
fn counter_basic_manifest_runs_through_event_program() {
    use crate::redpiler::event_netlist::compiler::compile;

    let (_, _, manifest) = fixture("counter_basic");
    let traced_pos = position(&manifest["ports"]["observations"]["memory"][0]);
    let mut executed_actions = 0;
    for case in manifest["cases"].as_array().unwrap() {
        let (mut world, bounds, _) = fixture("counter_basic");
        let lowered = lower(&world, bounds).unwrap();
        let compiled = compile(&lowered).unwrap();
        let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = position(&action["pos"]);
                    let powered = action["powered"].as_bool().unwrap();
                    let native_action_trace = if case["id"] == "release" {
                        crate::redstone::piston::trace::capture_detailed(&[traced_pos], || set_native_lever(&mut world, pos, powered))
                    } else {
                        set_native_lever(&mut world, pos, powered);
                        Vec::new()
                    };
                    event.set_input(compiled.positions[&pos], if powered { 15 } else { 0 }).unwrap();
                    executed_actions += 1;
                    if case["id"] == "release" {
                        let traced_index = lowered.program.pistons().iter().position(|piston| piston.position == traced_pos).unwrap();
                        let traced_piston = &lowered.program.pistons()[traced_index];
                        println!(
                            "counter after release action: piston={traced_index} pos={traced_pos:?} native_trace={native_action_trace:?} native_block={:?} native_events={:?} event_main={:?} event_quasi={:?} event_piston={:?} piston_ops={} requests={:?} counts={:?}",
                            world.get_block(traced_pos),
                            world
                                .piston_state()
                                .events
                                .iter()
                                .filter(|event| event.pos == traced_pos)
                                .collect::<Vec<_>>(),
                            event.input_strength_for_port_for_test(traced_piston.node, InputPort::Main),
                            event.input_strength_for_port_for_test(traced_piston.node, InputPort::Quasi),
                            event.piston(PistonId::new(traced_index as u32)),
                            event.compiled_piston_operation_count_for_test(),
                            event.pending_piston_requests_for_test(),
                            event.counts()
                        );
                        println!(
                            "counter release native scheduled={:?} piston_events={:?}",
                            world
                                .scheduler()
                                .iter_entries()
                                .filter(|entry| entry.pos == traced_pos)
                                .collect::<Vec<_>>(),
                            world
                                .piston_state()
                                .events
                                .iter()
                                .filter(|event| event.pos == traced_pos)
                                .collect::<Vec<_>>()
                        );
                    }
                }
                op => panic!("unsupported counter manifest action {op}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            world.observe_redstone_updates(true);
            let native_scheduled_before: Vec<_> = world
                .scheduler()
                .iter_entries()
                .map(|entry| {
                    let block = world.get_block(entry.pos);
                    (entry, block)
                })
                .collect();
            let scheduled_observers_touching_trace: Vec<_> = native_scheduled_before
                .iter()
                .filter_map(|(entry, block)| {
                    let Block::Observer { observer } = block else { return None };
                    let mut recipients = Vec::new();
                    crate::redstone::observer_notifications(observer.facing, entry.pos, |pos, _| {
                        if pos == traced_pos {
                            recipients.push(pos);
                        }
                    });
                    (!recipients.is_empty()).then_some((entry.pos, observer.facing, recipients))
                })
                .collect();
            let native_trace = crate::redstone::piston::trace::capture_detailed(&[traced_pos], || world.tick_interpreted());
            if !native_trace.is_empty() {
                let traced_index = lowered.program.pistons().iter().position(|piston| piston.position == traced_pos).unwrap();
                let traced_piston = &lowered.program.pistons()[traced_index];
                println!(
                    "counter trace tick={tick} native_scheduled_before={native_scheduled_before:?} native={native_trace:?} event_main={:?} event_quasi={:?} event_power={:?} event_delay={:?} event_state={:?} event_resource={:?}",
                    event.input_strength_for_port_for_test(traced_piston.node, InputPort::Main),
                    event.input_strength_for_port_for_test(traced_piston.node, InputPort::Quasi),
                    event.input_strength(traced_piston.node),
                    event.scheduled_delay_for_test(traced_piston.node),
                    event.piston(PistonId::new(traced_index as u32)),
                    lowered
                        .program
                        .resource_for_piston(PistonId::new(traced_index as u32))
                        .and_then(|resource| event.resource(resource))
                );
            }
            let native_updates = world.take_redstone_update_observations();
            let prior = event.counts();
            event.tick().unwrap();
            if case["id"] == "release" && tick == 4 {
                let node = lowered
                    .program
                    .pistons()
                    .iter()
                    .find(|piston| piston.position == traced_pos)
                    .unwrap()
                    .node;
                for &id in &compiled.program.input_links[compiled.program.input_ranges[node.index()].clone()] {
                    let link = &compiled.program.links[id];
                    let source_pos = lowered.program.nodes().get(link.source.index()).and_then(|source| source.position);
                    eprintln!(
                        "counter input source={source_pos:?} state={:?} link={link:?}",
                        event.component(link.source)
                    );
                }
            }
            let after = event.counts();
            let counts_delta = (
                after.notifications - prior.notifications,
                after.piston_requests_queued - prior.piston_requests_queued,
                after.piston_actions - prior.piston_actions,
                after.rejected_piston_actions - prior.rejected_piston_actions,
                after.scheduled_ticks - prior.scheduled_ticks,
                after.motion_completions - prior.motion_completions,
            );
            for (name, spec) in manifest["ports"]["observations"].as_object().unwrap() {
                let positions = if spec.as_array().unwrap().len() == 3 && spec[0].is_number() {
                    vec![position(spec)]
                } else {
                    spec.as_array().unwrap().iter().map(position).collect()
                };
                for pos in positions {
                    match world.get_block(pos) {
                        Block::RedstoneWire { wire } => {
                            assert_eq!(
                                event.component(compiled.positions[&pos]).unwrap().output,
                                wire.power,
                                "{} first wire divergence at tick {tick}: {name} at {pos:?}",
                                case["id"]
                            );
                        }
                        Block::RedstoneRepeater { repeater } => {
                            assert_eq!(
                                event.component(compiled.positions[&pos]).unwrap().powered,
                                repeater.powered,
                                "{} first repeater divergence at tick {tick}: {name} at {pos:?}",
                                case["id"]
                            );
                        }
                        _ => {}
                    }
                }
            }
            for (index, piston) in lowered.program.pistons().iter().enumerate() {
                let native_block = world.get_block(piston.position);
                let id = PistonId::new(index as u32);
                let state = event.piston(id).unwrap();
                let Block::Piston { piston: native } = native_block else {
                    if matches!(native_block, Block::MovingPiston { .. }) && state.pending_motion {
                        assert!(
                            world.piston_state().motions.iter().any(|motion| motion.pos == piston.position),
                            "{} event motion has no matching native motion at tick {tick}: piston {index}",
                            case["id"]
                        );
                        continue;
                    }
                    let native_motion: Vec<_> = world
                        .piston_state()
                        .motions
                        .iter()
                        .filter(|motion| {
                            motion.pos == piston.position
                                || motion.pos == lowered.program.contacts()[piston.near.index()].position
                                || motion.pos == lowered.program.contacts()[piston.far.index()].position
                        })
                        .collect();
                    let native_updates: Vec<_> = native_updates.iter().filter(|update| update.pos == piston.position).collect();
                    let resource = lowered.program.resource_for_piston(id).and_then(|resource| {
                        event.resource(resource).map(|state| {
                            let pos = lowered.program.contacts()[state.at.index()].position;
                            (pos, state, world.get_block(pos))
                        })
                    });
                    panic!(
                        "{} first missing piston base at tick {tick}: piston={index} pos={:?} block={native_block:?} entity={:?}; event={state:?} main={:?} quasi={:?} queued={:?} counts_delta={counts_delta:?} native_trace={native_trace:?} native_updates={native_updates:?} native_motion={native_motion:?} resource={resource:?} scheduler={} scheduled_observers_touching_trace={scheduled_observers_touching_trace:?} native_events={:?}",
                        case["id"],
                        piston.position,
                        world.get_block_entity(piston.position),
                        event.input_strength_for_port_for_test(piston.node, InputPort::Main),
                        event.input_strength_for_port_for_test(piston.node, InputPort::Quasi),
                        event.pending_piston_requests_for_test(),
                        world.scheduler().iter_entries().count(),
                        world
                            .piston_state()
                            .events
                            .iter()
                            .filter(|event| event.pos == piston.position)
                            .collect::<Vec<_>>(),
                    );
                };
                assert_eq!(
                    state.extended,
                    native.extended,
                    "{} first piston divergence at tick {tick}, input={:?}, main={:?} quasi={:?}, counts_delta={:?}, pending={}: piston {index} at {:?}",
                    case["id"],
                    event.input_strength(piston.node),
                    event.input_strength_for_port_for_test(piston.node, InputPort::Main),
                    event.input_strength_for_port_for_test(piston.node, InputPort::Quasi),
                    counts_delta,
                    event.has_pending_work(),
                    piston.position
                );
            }
        }
        let native_pending =
            world.scheduler().iter_entries().next().is_some() || !world.piston_state().events.is_empty() || !world.piston_state().motions.is_empty();
        assert_eq!(event.has_pending_work(), native_pending, "{} pending work", case["id"]);
    }
    assert!(executed_actions > 0);
}

#[test]
fn event_compiler_rejects_pending_entry_ticks() {
    let (mut world, bounds, _) = fixture("counter_basic");
    let position = bounds.0;
    world.schedule_tick(position, 1, mchprs_world::TickPriority::Normal);
    let lowered = lower(&world, bounds).unwrap();
    let error = match crate::redpiler::event_netlist::compiler::compile(&lowered) {
        Ok(_) => panic!("pending work was silently discarded"),
        Err(error) => error,
    };
    assert_eq!(error.position, Some(position));
    assert!(error.reason.contains("pending entry"), "{error}");
}

#[test]
fn event_torch_matches_native_delay_short_pulses_and_rearm() {
    use mchprs_blocks::{
        BlockDirection,
        blocks::{Lever, LeverFace},
    };
    for (wall, initial) in [(false, false), (true, false), (false, true), (true, true)] {
        let support = ORIGIN;
        let torch = support.offset(if wall { BlockFace::East } else { BlockFace::Top });
        let lever = support.offset(BlockFace::West);
        let bounds = (ORIGIN - BlockPos::new(3, 3, 3), ORIGIN + BlockPos::new(3, 3, 3));
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        world.set_block(support, Block::Stone {});
        world.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Wall, BlockDirection::West, initial),
            },
        );
        world.set_block(
            torch,
            if wall {
                Block::RedstoneWallTorch {
                    facing: BlockDirection::East,
                    lit: !initial,
                }
            } else {
                Block::RedstoneTorch { lit: !initial }
            },
        );
        let lowered = lower(&world, bounds).unwrap();
        let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
        let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
        let torch_node = compiled.positions[&torch];
        let lever_node = compiled.positions[&lever];
        assert_eq!(event.component(torch_node).unwrap().powered, !initial);
        for (powered, ticks) in [(!initial, 0), (initial, 2), (!initial, 2), (initial, 2), (!initial, 2), (initial, 2)] {
            let before = event.component(torch_node).unwrap().powered;
            set_native_lever(&mut world, lever, powered);
            event.set_input(lever_node, if powered { 15 } else { 0 }).unwrap();
            for step in 0..ticks {
                world.tick_interpreted();
                event.tick().unwrap();
                let lit = match world.get_block(torch) {
                    Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
                    _ => unreachable!(),
                };
                assert_eq!(event.component(torch_node).unwrap().powered, lit, "wall={wall} input={powered}");
                assert_eq!(lit, if step == 0 { before } else { !powered });
                assert_eq!(event.has_pending_work(), world.scheduler().iter_entries().count() != 0);
            }
        }
    }
}

#[test]
fn discovered_wire_tracks_event_torch_against_native() {
    use mchprs_blocks::{
        BlockDirection, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
    };
    let support = ORIGIN;
    let lever = support.offset(BlockFace::West);
    let torch = support.offset(BlockFace::Top);
    let wire = torch.offset(BlockFace::East);
    let piston = wire.offset(BlockFace::East);
    let bounds = (ORIGIN - BlockPos::new(3, 3, 3), ORIGIN + BlockPos::new(5, 4, 3));
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    world.set_block(support, Block::Stone {});
    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
        },
    );
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::None,
                RedstoneWireSide::None,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                15,
            ),
        },
    );
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let discovery = discover_watched_wire_sources(&world, bounds).unwrap();
    assert!(discovery.sources.iter().any(|source| source.wire == wire && source.source == torch));
    let lowered = lower(&world, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile_with_wire_discovery(&lowered, &discovery).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    for powered in [true, false] {
        set_native_lever(&mut world, lever, powered);
        event.set_input(compiled.positions[&lever], if powered { 15 } else { 0 }).unwrap();
        for _ in 0..4 {
            world.tick_interpreted();
            event.tick().unwrap();
            let Block::RedstoneWire { wire: native } = world.get_block(wire) else {
                unreachable!()
            };
            assert_eq!(event.component(compiled.positions[&wire]).unwrap().output, native.power);
        }
    }
}

#[test]
fn bud_noninstant_resource_admission_boundary() {
    let (world, bounds, _) = fixture("bud_noninstantinputs");
    let lowered = lower(&world, bounds).unwrap();
    let graph = &lowered.program;
    for (index, resource) in graph.resources().iter().enumerate() {
        println!(
            "resource {index}: material {:?}, contacts {:?}, initial {:?}",
            graph.profiles()[resource.material.index()].electrical,
            resource
                .contacts
                .iter()
                .map(|id| graph.contacts()[id.index()].position)
                .collect::<Vec<_>>(),
            graph.contacts()[lowered.state.resources[index].at.index()].position
        );
    }
    let discovery = discover_watched_wire_sources(&world, bounds).unwrap();
    let error = match crate::redpiler::event_netlist::compiler::compile_with_wire_discovery(&lowered, &discovery) {
        Ok(_) => panic!("movable resource was admitted"),
        Err(error) => error,
    };
    println!("BUD admission: {error}");
    assert_eq!(error.reason, "wire discovery requires one piston and static geometry");
    assert_eq!(graph.resources().len(), 1);
    assert_eq!(
        graph.resources()[0]
            .contacts
            .iter()
            .map(|id| graph.contacts()[id.index()].position)
            .collect::<Vec<_>>(),
        vec![BlockPos::new(40, 31, 43), BlockPos::new(40, 32, 43)]
    );
}

#[test]
fn bud_resource_wire_arrangements_are_discovered_from_scratch() {
    use crate::redpiler::event_netlist::scratch::discover_resource_wire_arrangements;
    let (world, bounds, manifest) = fixture("bud_noninstantinputs");
    let lowered = lower(&world, bounds).unwrap();
    let piston = &lowered.program.pistons()[0];
    let near = lowered.program.contacts()[piston.near.index()].position;
    let far = lowered.program.contacts()[piston.far.index()].position;
    let capture = (bounds.0 - BlockPos::new(3, 3, 3), bounds.1 + BlockPos::new(3, 3, 3));
    let entry = || ScratchEntry {
        scheduled: world.scheduler().iter_entries().collect(),
        piston: world.piston_state().clone(),
    };
    let receiver = position(&manifest["ports"]["observations"]["repeater"]);
    let arrangements = discover_resource_wire_arrangements(&world, capture, bounds, entry(), piston.position, near, far, receiver).unwrap();
    println!(
        "near sources: {:?}\nfar sources: {:?}\nmotion sources: {:?}",
        arrangements.at_near.sources, arrangements.at_far.sources, arrangements.in_motion.sources
    );
    assert_eq!(arrangements.at_near.watchers, arrangements.at_far.watchers);
    assert_eq!(arrangements.in_motion.watchers, arrangements.at_far.watchers);
    assert!(
        arrangements
            .in_motion
            .sources
            .iter()
            .all(|source| source.source != near && source.source != far)
    );
    assert_eq!(arrangements.receiver.map(|input| input.strength), [0, 15, 0]);
    assert!(
        arrangements
            .receiver
            .iter()
            .all(|input| input.source == far && input.direction == BlockFace::North && input.attenuation == 0)
    );
    assert!(discover_resource_wire_arrangements(&world, capture, bounds, entry(), piston.position, far, near, receiver).is_err());
}

#[test]
fn bud_movement_scratch_records_receiver_delivery_order() {
    let (host, bounds, manifest) = fixture("bud_noninstantinputs");
    let capture = (bounds.0 - BlockPos::new(3, 3, 3), bounds.1 + BlockPos::new(3, 3, 3));
    let entry = ScratchEntry {
        scheduled: host.scheduler().iter_entries().collect(),
        piston: host.piston_state().clone(),
    };
    let mut scratch = ScratchWorld::from_world(&host, capture, entry).unwrap();
    let data = position(&manifest["ports"]["inputs"]["data"]);
    let update = position(&manifest["ports"]["inputs"]["update"]);
    let repeater = position(&manifest["ports"]["observations"]["repeater"]);
    set_native_lever(scratch.world_mut(), data, true);
    for _ in 0..32 {
        scratch.world_mut().tick_interpreted();
    }
    scratch.world_mut().observe_redstone_updates(true);
    set_native_lever(scratch.world_mut(), update, true);
    let mut delivered = Vec::new();
    for tick in 0..8 {
        scratch.world_mut().tick_interpreted();
        delivered.extend(
            scratch
                .world_mut()
                .take_redstone_update_observations()
                .into_iter()
                .filter(|event| event.pos == repeater)
                .map(|event| (tick, event.direction)),
        );
        scratch.world_mut().observe_redstone_updates(true);
    }
    scratch.finish_probe().unwrap();
    println!("BUD repeater deliveries: {delivered:?}");
    assert_eq!(delivered, vec![(1, Some(BlockFace::North)), (5, Some(BlockFace::North))]);
}

#[test]
fn bud_scratch_identifies_movement_callback_phases() {
    use mchprs_world::AdvancePhase;
    let (host, bounds, manifest) = fixture("bud_noninstantinputs");
    let capture = (bounds.0 - BlockPos::new(3, 3, 3), bounds.1 + BlockPos::new(3, 3, 3));
    let entry = ScratchEntry {
        scheduled: host.scheduler().iter_entries().collect(),
        piston: host.piston_state().clone(),
    };
    let mut scratch = ScratchWorld::from_world(&host, capture, entry).unwrap();
    let data = position(&manifest["ports"]["inputs"]["data"]);
    let update = position(&manifest["ports"]["inputs"]["update"]);
    let repeater = position(&manifest["ports"]["observations"]["repeater"]);
    for (data_value, update_value) in [(true, true), (false, false)] {
        set_native_lever(scratch.world_mut(), data, data_value);
        for _ in 0..32 {
            scratch.world_mut().tick_interpreted();
        }
        set_native_lever(scratch.world_mut(), update, update_value);
        let start = scratch.world().piston_state().logical_tick;
        let mut delivered = Vec::new();
        for _ in 0..10000 {
            let before: Vec<_> = scratch.world().scheduler().iter_entries().map(|entry| entry.pos).collect();
            scratch.world_mut().observe_redstone_updates(true);
            scratch.world_mut().picotick_advance(1);
            let state = scratch.world().piston_state();
            let tick = state.logical_tick - start;
            let phase = state.phase;
            let observed = scratch.world_mut().take_redstone_update_observations();
            if phase == AdvancePhase::ScheduledTicks && observed.iter().any(|event| event.pos == repeater) {
                let after: Vec<_> = scratch.world().scheduler().iter_entries().map(|entry| entry.pos).collect();
                assert_eq!(
                    before.iter().copied().filter(|pos| !after.contains(pos)).collect::<Vec<_>>(),
                    vec![repeater]
                );
            }
            delivered.extend(
                observed
                    .into_iter()
                    .filter(|event| event.pos == repeater)
                    .map(|event| (tick, phase, event.direction)),
            );
            if tick >= 8 && phase == AdvancePhase::BetweenTicks {
                break;
            }
        }
        println!("update={update_value} deliveries={delivered:?}");
        assert_eq!(
            delivered,
            if update_value {
                vec![
                    (2, AdvancePhase::PistonEvents, Some(BlockFace::North)),
                    (6, AdvancePhase::ScheduledTicks, Some(BlockFace::North)),
                ]
            } else {
                vec![
                    (4, AdvancePhase::MovingEntities, Some(BlockFace::North)),
                    (8, AdvancePhase::ScheduledTicks, Some(BlockFace::North)),
                ]
            }
        );
        for _ in 0..24 {
            scratch.world_mut().tick_interpreted();
        }
    }
    scratch.finish_probe().unwrap();
}

fn bud_movement_callbacks(
    host: &PlotWorld,
    bounds: (BlockPos, BlockPos),
    manifest: &Value,
) -> (Vec<BlockPos>, Vec<BlockPos>, Vec<crate::world::PistonNotificationObservation>) {
    use mchprs_world::AdvancePhase;
    let capture = (bounds.0 - BlockPos::new(3, 3, 3), bounds.1 + BlockPos::new(3, 3, 3));
    let entry = ScratchEntry {
        scheduled: host.scheduler().iter_entries().collect(),
        piston: host.piston_state().clone(),
    };
    let mut scratch = ScratchWorld::from_world(host, capture, entry).unwrap();
    let data = position(&manifest["ports"]["inputs"]["data"]);
    let update = position(&manifest["ports"]["inputs"]["update"]);
    let receiver = position(&manifest["ports"]["observations"]["repeater"]);
    let mut phases = Vec::new();
    let mut piston_notifications = Vec::new();
    for (data_value, update_value) in [(true, true), (false, false)] {
        set_native_lever(scratch.world_mut(), data, data_value);
        for _ in 0..32 {
            scratch.world_mut().tick_interpreted();
        }
        set_native_lever(scratch.world_mut(), update, update_value);
        let start = scratch.world().piston_state().logical_tick;
        let mut observed = Vec::new();
        for _ in 0..10000 {
            scratch.world_mut().observe_redstone_updates(true);
            scratch.world_mut().observe_piston_notifications(true);
            scratch.world_mut().picotick_advance(1);
            let phase = scratch.world().piston_state().phase;
            let tick = scratch.world().piston_state().logical_tick - start;
            let updates = scratch.world_mut().take_redstone_update_observations();
            observed.extend(updates.iter().filter(|event| event.pos == receiver).map(|event| (phase, event.direction)));
            piston_notifications.extend(scratch.world_mut().take_piston_notification_observations());
            if tick >= 8 && phase == AdvancePhase::BetweenTicks {
                break;
            }
        }
        phases.push(observed);
        for _ in 0..24 {
            scratch.world_mut().tick_interpreted();
        }
    }
    scratch.finish_probe().unwrap();
    let selected = |events: &[(AdvancePhase, Option<BlockFace>)], phase| {
        events
            .iter()
            .filter(|(seen, direction)| *seen == phase && *direction == Some(BlockFace::North))
            .map(|_| receiver)
            .collect()
    };
    (
        selected(&phases[0], AdvancePhase::PistonEvents),
        selected(&phases[1], AdvancePhase::MovingEntities),
        piston_notifications,
    )
}

#[test]
fn bud_discovered_resource_store_hold_resample_matches_native() {
    use crate::redpiler::event_netlist::{
        compiler::{ResourceDiscovery, compile_with_resource_discovery},
        scratch::discover_resource_wire_arrangements,
    };
    let mut movements = 0;
    for fixture_id in ["bud_noninstantinputs", "bud_pistonupdate"] {
        let (_, _, manifest_list) = fixture(fixture_id);
        for selected in manifest_list["cases"].as_array().unwrap() {
            let (mut world, bounds, manifest) = fixture(fixture_id);
            let lowered = lower(&world, bounds).unwrap();
            let actors = super::super::scratch::map_piston_resources(&world, &lowered);
            assert_eq!(actors.pistons.len(), lowered.program.pistons().len());
            assert_eq!(actors.resources.len(), lowered.program.resources().len());
            assert!(actors.resources.iter().all(|resource| !resource.acting_pistons.is_empty()));
            let [resource_actor] = actors.resources[0].acting_pistons.as_slice() else {
                panic!("unsupported shared resource actor group")
            };
            let piston = &lowered.program.pistons()[resource_actor.index()];
            let near = lowered.program.contacts()[piston.near.index()].position;
            let far = lowered.program.contacts()[piston.far.index()].position;
            let receiver = position(&manifest["ports"]["observations"]["repeater"]);
            let capture = (bounds.0 - BlockPos::new(3, 3, 3), bounds.1 + BlockPos::new(3, 3, 3));
            let entry = ScratchEntry {
                scheduled: world.scheduler().iter_entries().collect(),
                piston: world.piston_state().clone(),
            };
            let arrangements = discover_resource_wire_arrangements(&world, capture, bounds, entry, piston.position, near, far, receiver).unwrap();
            let (departure_far, arrival_far, piston_notifications) = bud_movement_callbacks(&world, bounds, &manifest);
            let compiled = compile_with_resource_discovery(
                &lowered,
                &ResourceDiscovery {
                    arrangements: &arrangements,
                    actors: &actors,
                    receiver,
                    departure_far: &departure_far,
                    arrival_far: &arrival_far,
                    piston_notifications: &piston_notifications,
                },
            )
            .unwrap();
            for actor in &actors.pistons {
                assert_eq!(compiled.positions[&actor.position], lowered.program.pistons()[actor.id.index()].node);
            }
            let receiver_node = compiled.positions[&receiver];
            let receiver_links = &compiled.program.input_links[compiled.program.input_ranges[receiver_node.index()].clone()];
            assert_eq!(receiver_links.len(), 1);
            assert_eq!(compiled.program.links[receiver_links[0]].attenuation, 0);
            let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
            let case = selected;
            let rounds = if case["id"] == "store-hold-resample" { 2 } else { 1 };
            for _ in 0..rounds {
                let mut last_was_data = false;
                let mut prior_movements = event.counts().resource_movements;
                for action in case["actions"].as_array().unwrap() {
                    match action["op"].as_str().unwrap() {
                        "lever" => {
                            let pos = position(&action["pos"]);
                            let powered = action["powered"].as_bool().unwrap();
                            last_was_data = pos == position(&manifest["ports"]["inputs"]["data"]);
                            prior_movements = event.counts().resource_movements;
                            set_native_lever(&mut world, pos, powered);
                            event.set_input(compiled.positions[&pos], if powered { 15 } else { 0 }).unwrap();
                        }
                        "wait_ready" | "wait" => {
                            for _ in 0..action["ticks"].as_u64().unwrap() {
                                world.tick_interpreted();
                                event.tick().unwrap();
                            }
                            check_bud_probe(&world, &lowered, &compiled, &event, &manifest);
                            check_bud_wires(&world, &compiled, &event, &manifest);
                            if last_was_data {
                                assert_eq!(event.counts().resource_movements, prior_movements, "{} data-only hold", case["id"]);
                            }
                            assert!(!event.has_pending_work());
                            assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
                            assert_eq!(world.scheduler().iter_entries().count(), 0);
                        }
                        op => panic!("unexpected action {op}"),
                    }
                }
                for _ in 0..case["ticks"].as_u64().unwrap() {
                    world.tick_interpreted();
                    event.tick().unwrap();
                }
                check_bud_probe(&world, &lowered, &compiled, &event, &manifest);
                check_bud_wires(&world, &compiled, &event, &manifest);
                if last_was_data {
                    assert_eq!(event.counts().resource_movements, prior_movements, "{} data-only hold", case["id"]);
                }
                assert!(!event.has_pending_work());
                assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
                assert_eq!(world.scheduler().iter_entries().count(), 0);
            }
            if case["id"] == "store-hold-resample" {
                assert_eq!(event.counts().resource_movements, 4);
            }
            movements += event.counts().resource_movements;
        }
    }
    assert!(movements > 0);
}

fn check_bud_wires(world: &PlotWorld, compiled: &crate::redpiler::event_netlist::compiler::CompiledEvent, event: &EventState, manifest: &Value) {
    for (name, spec) in manifest["ports"]["observations"].as_object().unwrap() {
        let pos = position(spec);
        if let Block::RedstoneWire { wire } = world.get_block(pos) {
            assert_eq!(event.component(compiled.positions[&pos]).unwrap().output, wire.power, "{name}");
        }
    }
}

#[test]
fn bud_noninstant_event_store_hold_resample_matches_native() {
    let (mut world, bounds, manifest) = fixture("bud_noninstantinputs");
    let lowered = lower(&world, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "store-hold-resample")
        .unwrap();
    let mut stage = 0;
    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let powered = action["powered"].as_bool().unwrap();
                set_native_lever(&mut world, pos, powered);
                event.set_input(compiled.positions[&pos], if powered { 15 } else { 0 }).unwrap();
            }
            "wait_ready" | "wait" => {
                for _ in 0..action["ticks"].as_u64().unwrap() {
                    world.tick_interpreted();
                    event.tick().unwrap();
                }
                check_bud_probe(&world, &lowered, &compiled, &event, &manifest);
                assert_eq!(event.piston(PistonId::new(0)).unwrap().extended, [true, false, false][stage]);
                assert_eq!(event.counts().resource_movements, [0, 1, 1][stage]);
                assert!(!event.has_pending_work());
                stage += 1;
            }
            op => panic!("unexpected action {op}"),
        }
    }
    for _ in 0..case["ticks"].as_u64().unwrap() {
        world.tick_interpreted();
        event.tick().unwrap();
    }
    check_bud_probe(&world, &lowered, &compiled, &event, &manifest);
    assert_eq!(stage, 3);
    assert!(event.piston(PistonId::new(0)).unwrap().extended);
    assert!(!event.has_pending_work());
    assert_eq!(event.counts().resource_movements, 2);
}

fn check_bud_probe(
    world: &PlotWorld,
    lowered: &LoweredCircuit,
    compiled: &crate::redpiler::event_netlist::compiler::CompiledEvent,
    event: &EventState,
    manifest: &Value,
) {
    for (index, spec) in lowered.program.pistons().iter().enumerate() {
        let Block::Piston { piston: native } = world.get_block(spec.position) else {
            panic!("missing piston at {:?}", spec.position)
        };
        assert_eq!(
            event.piston(PistonId::new(index as u32)).unwrap().extended,
            native.extended,
            "piston {index} at {:?} after settling",
            spec.position
        );
    }
    let resource = event.resource(ResourceId::new(0)).unwrap();
    let resource_pos = lowered.program.contacts()[resource.at.index()].position;
    assert!(
        resource.enabled && matches!(world.get_block(resource_pos), Block::RedstoneBlock),
        "payload at {resource_pos:?}"
    );
    let repeater_pos = position(&manifest["ports"]["observations"]["repeater"]);
    let Block::RedstoneRepeater { repeater } = world.get_block(repeater_pos) else {
        panic!("missing repeater")
    };
    assert_eq!(
        event.component(compiled.positions[&repeater_pos]).unwrap().powered,
        repeater.powered,
        "consumer after settling"
    );
}

#[test]
fn compiled_event_pistons_keep_quasi_power_during_trigger_departures() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redstone::piston::trace::{self, Stage};

    let (mut scratch, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&scratch, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    let mut graph = GraphBackend::compile(&scratch, bounds, scratch.scheduler().iter_entries().collect(), Default::default()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let quasi_powered = [6usize, 11];
    let piston_positions = [lowered.program.pistons()[11].position, lowered.program.pistons()[9].position];

    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                let node = compiled.positions[&pos];
                let native_trace = if pos == trigger {
                    Some(trace::capture_detailed(&piston_positions, || {
                        set_native_lever(&mut scratch, pos, value);
                        scratch.tick_interpreted();
                    }))
                } else {
                    set_native_lever(&mut scratch, pos, value);
                    None
                };
                let graph_powered = matches!(&graph.circuit_state().components[node.index()], ComponentState::Electrical(state) if state.powered);
                if graph_powered != value {
                    graph.use_block(pos).unwrap();
                }
                event.set_input(node, if value { 15 } else { 0 }).unwrap();
                if pos == trigger {
                    let native_trace = native_trace.unwrap();
                    assert!(
                        native_trace
                            .iter()
                            .any(|entry| entry.pos == piston_positions[0] && entry.stage == Stage::Sample && entry.powered)
                    );
                    assert!(
                        !native_trace
                            .iter()
                            .any(|entry| entry.pos == piston_positions[0] && entry.stage == Stage::Request)
                    );
                    assert!(
                        !native_trace.iter().any(|entry| entry.pos == piston_positions[1]),
                        "Piston 9 must not update without resource 8 departure"
                    );
                    for index in quasi_powered {
                        let piston = PistonId::new(index as u32);
                        let graph_input = lowered.program.pistons()[index].node;
                        let event_input = compiled.program.pistons[index].input;
                        assert_eq!(graph.input_strength_for_test(graph_input, InputPort::Main), 0);
                        assert!(graph.input_strength_for_test(graph_input, InputPort::Quasi) > 0);
                        assert!(event.input_strength(event_input).unwrap() > 0, "{piston:?} must include quasi power");
                    }
                    graph.tick().unwrap();
                    event.tick().unwrap();
                    for index in quasi_powered {
                        assert!(event.piston(PistonId::new(index as u32)).unwrap().extended);
                    }
                    assert_eq!(event.resource(ResourceId::new(8)).unwrap().at, ContactId::new(32));
                    assert!(event.resource(ResourceId::new(8)).unwrap().enabled);
                    let graph_resources = graph.circuit_state().resources.clone();
                    let mut native_applied = Vec::new();
                    let piston_positions = lowered.program.pistons().iter().map(|piston| piston.position).collect::<Vec<_>>();
                    for _ in 1..case["ticks"].as_u64().unwrap() {
                        graph.tick().unwrap();
                        event.tick().unwrap();
                        native_applied.extend(
                            trace::capture_detailed(&piston_positions, || scratch.tick_interpreted())
                                .into_iter()
                                .filter(|entry| entry.stage == Stage::Applied),
                        );
                    }
                    assert_eq!(
                        graph.circuit_state().resources,
                        graph_resources,
                        "Graph resource arrangement over 0+0 output window"
                    );
                    assert!(
                        native_applied.is_empty(),
                        "native piston movement over 0+0 output window: {native_applied:?}"
                    );
                    assert_eq!(event.counts().resource_movements, 0, "EventProgram movement count for 0+0");
                    return;
                }
            }
            "wait" | "wait_ready" => {
                for _ in 0..action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    event.tick().unwrap();
                    scratch.tick_interpreted();
                }
            }
            op => panic!("unexpected manifest action {op}"),
        }
    }
    panic!("fixture case did not reach its trigger");
}

fn set_native_lever(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::{Block, LeverFace};

    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("expected lever at {pos:?}, found {:?}", world.get_block(pos));
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(pos, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, pos);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, pos.offset(support));
}

fn position(value: &Value) -> BlockPos {
    ORIGIN
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
}

#[test]
fn adder1_prepared_0_1_0_world_discovery_diagnostic() {
    let (host, bounds, manifest) = fixture("adder_1bit");
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-1-0")
        .unwrap();
    let input_actions = &case["actions"].as_array().unwrap()[..4];
    let all_actions = case["actions"].as_array().unwrap();
    let (min, max) = bounds;
    let capture = (min - BlockPos::new(2, 2, 2), max + BlockPos::new(2, 2, 2));
    let entry = || ScratchEntry {
        scheduled: host.scheduler().iter_entries().collect(),
        piston: host.piston_state().clone(),
    };
    let apply = |scratch: &mut ScratchWorld, actions: &[Value], observe: bool| {
        scratch.world_mut().observe_redstone_updates(observe);
        for action in actions {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = position(&action["pos"]);
                    let Block::Lever { mut lever } = scratch.world().get_block(pos) else {
                        panic!("expected lever at {pos:?}")
                    };
                    lever.powered = action["powered"].as_bool().unwrap();
                    scratch.world_mut().set_block(pos, Block::Lever { lever });
                    crate::redstone::update_surrounding_blocks(scratch.world_mut(), pos);
                    let attachment = match lever.face {
                        mchprs_blocks::blocks::LeverFace::Ceiling => BlockFace::Top,
                        mchprs_blocks::blocks::LeverFace::Floor => BlockFace::Bottom,
                        mchprs_blocks::blocks::LeverFace::Wall => lever.facing.opposite().block_face(),
                    };
                    crate::redstone::update_surrounding_blocks(scratch.world_mut(), pos.offset(attachment));
                }
                "wait_ready" | "wait" => {
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        scratch.world_mut().tick_interpreted();
                    }
                }
                op => panic!("unexpected operation {op}"),
            }
        }
    };

    let mut preparation = ScratchWorld::from_world(&host, capture, entry()).unwrap();
    apply(&mut preparation, input_actions, false);
    preparation.finish_probe().unwrap();
    let plan = discover_watched_wire_sources(preparation.world(), capture).unwrap();
    let predicted_before: Vec<_> = plan
        .watchers
        .iter()
        .map(|watch| (watch.wire, discovered_wire_strength(preparation.world(), &plan, watch.wire)))
        .collect();
    preparation.finish_probe().unwrap();

    let mut transition = ScratchWorld::from_world(&host, capture, entry()).unwrap();
    apply(&mut transition, input_actions, false);
    transition.finish_probe().unwrap();
    transition.world_mut().observe_redstone_updates(true);
    apply(&mut transition, &all_actions[4..], true);
    transition.finish_probe().unwrap();
    let after: Vec<_> = plan
        .watchers
        .iter()
        .map(|watch| {
            let Block::RedstoneWire { wire } = transition.world().get_block(watch.wire) else {
                unreachable!()
            };
            (watch.wire, wire.power)
        })
        .collect();
    let predicted_after: Vec<_> = plan
        .watchers
        .iter()
        .map(|watch| (watch.wire, discovered_wire_strength(transition.world(), &plan, watch.wire)))
        .collect();
    transition.finish_probe().unwrap();
    let callbacks = transition.world_mut().take_redstone_update_observations();
    let piston_callbacks: Vec<_> = callbacks
        .into_iter()
        .filter(|event| matches!(event.block, Block::Piston { .. }))
        .collect();
    let before: Vec<_> = predicted_before
        .iter()
        .map(|&(wire, _)| {
            let Block::RedstoneWire { wire: state } = preparation.world().get_block(wire) else {
                unreachable!()
            };
            (wire, state.power)
        })
        .collect();
    assert_eq!(predicted_before, before, "discovered wire strength differs from saved interpreter state");
    assert_eq!(predicted_after, after, "discovered wire strength differs from interpreter after trigger");
    let changed: Vec<_> = before
        .iter()
        .zip(&after)
        .filter(|((before_wire, before_power), (after_wire, after_power))| before_wire == after_wire && before_power != after_power)
        .map(|((wire, before), (_, after))| (*wire, *before, *after))
        .collect();
    assert!(!plan.watchers.is_empty(), "no nearby watched wires discovered");
    assert!(plan.sources.iter().all(|source| source.attenuation <= 14));
    assert!(
        piston_callbacks
            .iter()
            .any(|event| plan.watchers.iter().any(|watch| watch.piston == event.pos)),
        "no delivered update reached a discovered watched piston: {piston_callbacks:?}"
    );
    assert!(
        plan.watchers.iter().any(|watch| {
            changed.iter().any(|(wire, _, _)| *wire == watch.wire) && piston_callbacks.iter().any(|event| event.pos == watch.piston)
        }),
        "no changed watched wire and delivered piston callback coincide; changed={changed:?} callbacks={piston_callbacks:?}"
    );
    let changed_evidence: Vec<_> = changed
        .iter()
        .map(|(wire, before, after)| {
            (
                *wire,
                *before,
                *after,
                plan.sources.iter().filter(|source| source.wire == *wire).copied().collect::<Vec<_>>(),
                plan.watchers
                    .iter()
                    .filter(|watch| watch.wire == *wire)
                    .map(|watch| watch.piston)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert!(
        changed_evidence.iter().all(|(_, _, _, sources, _)| !sources.is_empty()),
        "changed wire has no discovered electrical source: {changed_evidence:?}"
    );
    println!(
        "ADDER1BIT prepared-0-1-0 watchers={} sources={} changed_wire_evidence={changed_evidence:?} delivered_piston_callbacks={}",
        plan.watchers.len(),
        plan.sources.len(),
        piston_callbacks.len()
    );
}

#[test]
fn offline_adder1_compiled_event_program_arithmetic() {
    let (_, _, manifest) = fixture("adder_1bit");
    let mut resource_movements = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let (world, bounds, _) = fixture("adder_1bit");
        let lowered = lower(&world, bounds).unwrap();
        let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered)
            .unwrap_or_else(|error| panic!("{} event compilation failed: {error}", case["id"]));
        assert!(!compiled.program.links.is_empty(), "compiled electrical links required");
        let sum = compiled.positions[&position(&manifest["ports"]["observations"]["sum_repeater"])];
        let carry = compiled.positions[&position(&manifest["ports"]["observations"]["carry_repeater"])];
        let carry_links = compiled.program.input_links[compiled.program.input_ranges[carry.index()].clone()].to_vec();
        assert!(!carry_links.is_empty(), "compiled carry dependencies are required");
        let carry_target = PowerTarget::Input {
            node: carry,
            port: InputPort::Main,
        };
        let graph_links = lowered.program.power_links_to(carry_target);
        println!(
            "{} carry Graph links={} fixed query={:?} predicates={:?} event guard={:?}",
            case["id"],
            graph_links.len(),
            lowered.program.fixed_signal_query(carry_target),
            graph_links
                .iter()
                .map(|link| (link.from, link.from_port, link.attenuation, &lowered.program.guards()[link.guard.index()]))
                .collect::<Vec<_>>(),
            carry_links
                .iter()
                .map(|&link| compiled.program.links[link].condition.as_ref())
                .collect::<Vec<_>>()
        );
        for link in graph_links {
            let site = &lowered.program.sites()[link.from.index()];
            println!("{} carry source site {site:?}", case["id"]);
            if let crate::redpiler::program_graph::SiteSource::Component(node) = site.source {
                if matches!(lowered.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
                    println!(
                        "{} carry source wire {:?} query={:?} has links {:?}",
                        case["id"],
                        node,
                        lowered.program.fixed_signal_query(PowerTarget::Input { node, port: InputPort::Main }),
                        lowered.program.power_links_to(PowerTarget::Input { node, port: InputPort::Main })
                    );
                    for wire_link in lowered.program.power_links_to(PowerTarget::Input { node, port: InputPort::Main }) {
                        let wire_site = &lowered.program.sites()[wire_link.from.index()];
                        println!("{} carry wire link site={wire_site:?}", case["id"]);
                        if let PowerPort::Conducted { include_dust } = wire_link.from_port {
                            for input in lowered.program.power_links_to(PowerTarget::Conductor {
                                site: wire_link.from,
                                include_dust,
                            }) {
                                println!(
                                    "{} carry wire conductor input={input:?} site={:?} guard={:?}",
                                    case["id"],
                                    lowered.program.sites()[input.from.index()],
                                    lowered.program.guards()[input.guard.index()]
                                );
                            }
                        }
                    }
                }
            }
        }
        let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
        let event_resource1_owners = compiled
            .program
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(index, piston)| (piston.resource == Some(ResourceId::new(1))).then_some((PistonId::new(index as u32), piston.input)))
            .collect::<Vec<_>>();
        let p0_input = lowered.program.pistons()[event_resource1_owners[0].0.index()].node;
        let p0_event_links = compiled.program.input_links[compiled.program.input_ranges[p0_input.index()].clone()].to_vec();
        println!(
            "{} carry compiled sources: {:?}",
            case["id"],
            carry_links
                .iter()
                .map(|&link| {
                    let source = compiled.program.links[link].source;
                    (
                        link,
                        source,
                        lowered.program.nodes().get(source.index()).map(|node| &node.kind),
                        lowered.state.components.get(source.index()),
                        compiled.program.components[source.index()].kind,
                        event.component(source),
                        compiled.program.links[link].condition.as_ref(),
                    )
                })
                .collect::<Vec<_>>()
        );
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let node = compiled.positions[&position(&action["pos"])];
                    if node == compiled.positions[&position(&manifest["ports"]["inputs"]["trigger"])] {
                        println!(
                            "{} P0 before trigger: strength={:?} links={:?}",
                            case["id"],
                            event.input_strength(p0_input),
                            p0_event_links
                                .iter()
                                .map(|&link| {
                                    let source = compiled.program.links[link].source;
                                    (
                                        link,
                                        source,
                                        compiled.program.components[source.index()].kind,
                                        event.component(source),
                                        event.link_state_for_test(link),
                                        &compiled.program.links[link].condition,
                                    )
                                })
                                .collect::<Vec<_>>()
                        );
                    }
                    event.set_input(node, if action["powered"].as_bool().unwrap() { 15 } else { 0 }).unwrap();
                    if node == compiled.positions[&position(&manifest["ports"]["inputs"]["trigger"])] {
                        let wire = compiled.positions[&BlockPos::new(40, 34, 41)];
                        println!(
                            "{} wire89 after trigger={:?} input={:?} ops={:?} links={:?}",
                            case["id"],
                            event.component(wire),
                            event.input_strength(wire),
                            compiled.program.source_ranges[wire.index()]
                                .clone()
                                .map(|index| compiled.program.operations[index])
                                .collect::<Vec<_>>(),
                            compiled.program.input_links[compiled.program.input_ranges[wire.index()].clone()]
                                .iter()
                                .map(|&link| (link, &compiled.program.links[link], event.link_state_for_test(link)))
                                .collect::<Vec<_>>()
                        );
                        println!(
                            "{} P0 after trigger: strength={:?} links={:?}",
                            case["id"],
                            event.input_strength(p0_input),
                            p0_event_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>()
                        );
                        println!(
                            "{} carry at event entry: links={:?}, pending deadline={:?}, resource1={:?}, owners={:?}, notifications={}",
                            case["id"],
                            carry_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>(),
                            event.scheduled_delay_for_test(carry),
                            event.resource(ResourceId::new(1)),
                            event_resource1_owners
                                .iter()
                                .map(|&(piston, input)| {
                                    (
                                        piston,
                                        lowered.program.nodes()[input.index()].position,
                                        event.input_strength(input),
                                        event.piston(piston),
                                    )
                                })
                                .collect::<Vec<_>>(),
                            event.counts().notifications
                        );
                    }
                }
                "wait" | "wait_ready" => {
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        event.tick().unwrap();
                    }
                }
                op => panic!("unexpected manifest action {op}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            let before = event.counts();
            event.tick().unwrap();
            let after = event.counts();
            if after.resource_movements != before.resource_movements || after.motion_completions != before.motion_completions {
                println!(
                    "{} tick={tick} {} carry links={:?}, pending deadline={:?}, resource1={:?}, owners={:?}, notifications += {}",
                    case["id"],
                    if after.resource_movements != before.resource_movements {
                        "departure"
                    } else {
                        "arrival"
                    },
                    carry_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>(),
                    event.scheduled_delay_for_test(carry),
                    event.resource(ResourceId::new(1)),
                    event_resource1_owners
                        .iter()
                        .map(|&(piston, input)| {
                            (
                                piston,
                                lowered.program.nodes()[input.index()].position,
                                event.input_strength(input),
                                event.piston(piston),
                            )
                        })
                        .collect::<Vec<_>>(),
                    after.notifications - before.notifications
                );
            }
            if (3..=7).contains(&tick) {
                assert_eq!(
                    u64::from(!event.component(sum).unwrap().powered),
                    case["expectation"]["sum"].as_u64().unwrap(),
                    "{} raw sum repeater tick {tick}",
                    case["id"]
                );
            }
            if (5..=9).contains(&tick) {
                assert_eq!(
                    u64::from(!event.component(carry).unwrap().powered),
                    case["expectation"]["carry"].as_u64().unwrap(),
                    "{} raw carry repeater tick {tick}",
                    case["id"]
                );
            }
        }
        let counts = event.counts();
        assert!(counts.notifications > 0, "{} compiled notifications", case["id"]);
        resource_movements += counts.resource_movements;
    }
    assert!(resource_movements > 0, "arithmetic vectors must exercise compiled resource movement");
}
