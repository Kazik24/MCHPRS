//! Opt-in replay of the production snapshot, including the neighboring Divider.
use super::*;
use crate::plot::{NUM_CHUNKS, PLOT_SCALE, PLOT_SECTIONS, PLOT_WIDTH, PlotWorld};
use crate::redpiler::CompilerOptions;
#[cfg(test)]
use crate::redpiler::LogicalDeviceAdmission;
use crate::world::storage::Chunk;
use mchprs_save_data::plot_data::PlotData;
use std::sync::Arc;
use std::time::Instant;

const MIN: BlockPos = BlockPos::new(-273, 50, -253);
const MAX: BlockPos = BlockPos::new(-102, 92, -147);
const TRIGGER: BlockPos = BlockPos::new(-267, 86, -177);

fn inside_fpu(p: BlockPos) -> bool {
    p.x >= MIN.x && p.x <= MAX.x && p.y >= MIN.y && p.y <= MAX.y && p.z >= MIN.z && p.z <= MAX.z
}

fn manifest_position(value: &serde_json::Value) -> BlockPos {
    let coordinates = value.as_array().expect("manifest position triple");
    assert_eq!(coordinates.len(), 3, "manifest positions use x/y/z triples");
    BlockPos::new(
        MIN.x + coordinates[0].as_i64().expect("manifest x coordinate") as i32,
        MIN.y + coordinates[1].as_i64().expect("manifest y coordinate") as i32,
        MIN.z + coordinates[2].as_i64().expect("manifest z coordinate") as i32,
    )
}

fn manifest_positions(value: &serde_json::Value) -> Vec<BlockPos> {
    value.as_array().expect("manifest position list").iter().map(manifest_position).collect()
}

fn saved_fpu_manifest() -> serde_json::Value {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("test_data/piston-research/fixtures/fpu_legal.json");
    serde_json::from_slice(&std::fs::read(path).expect("read saved FPU port manifest"))
        .expect("parse saved FPU port manifest")
}

fn sorted_positions(mut positions: Vec<BlockPos>) -> Vec<BlockPos> {
    positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    positions
}

fn known_command_witness_word(manifest: &serde_json::Value, positions: &[BlockPos]) -> u64 {
    let ports = &manifest["ports"];
    let mut assignments = manifest_positions(&ports["light_blue_operand_inputs"])
        .into_iter()
        .map(|position| (position, false))
        .chain(manifest_positions(&ports["red_operand_inputs"]).into_iter().map(|position| (position, true)))
        .collect::<Vec<_>>();
    assignments.extend(
        manifest_positions(&ports["opcode_levers_by_sign_weight_4_2_1"])
            .into_iter()
            .zip([false, true, true]),
    );
    assert_eq!(
        sorted_positions(assignments.iter().map(|(position, _)| *position).collect()),
        sorted_positions(positions.to_vec()),
        "known witness assignments must cover exactly the declared controls"
    );
    positions.iter().enumerate().fold(0u64, |word, (bit, position)| {
        let powered = assignments
            .iter()
            .find_map(|(assigned_position, powered)| (assigned_position == position).then_some(*powered))
            .expect("every witness input position has a manifest assignment");
        word | (u64::from(powered) << bit)
    })
}

fn command_block_census(backend: &GraphBackend) -> Vec<serde_json::Value> {
    backend
        .program
        .command_blocks()
        .iter()
        .map(|plan| {
            let position = backend.program.nodes()[plan.node.index()]
                .position
                .expect("saved command block has a physical position");
            let ComponentState::Electrical(state) = &backend.state.components[plan.node.index()] else {
                panic!("saved command block retains electrical state at {position:?}");
            };
            let entity = state
                .command_entity
                .as_deref()
                .expect("saved command block retains its entity");
            serde_json::json!({
                "position": position,
                "command": entity.command,
                "powered": entity.powered,
                "last_execution": entity.last_execution,
                "automatic": entity.automatic,
                "track_output": entity.track_output,
            })
        })
        .collect()
}

#[cfg(test)]
fn saved_world() -> PlotWorld {
    let path = std::env::var_os("MCHPRS_SAVED_PLOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/build-audit-tps/prod/p-1,-1"));
    saved_world_from(path)
}

fn saved_world_from(path: std::path::PathBuf) -> PlotWorld {
    saved_world_from_with_limits(path, false)
}

fn saved_world_from_with_limits(path: std::path::PathBuf, limits: bool) -> PlotWorld {
    let data = PlotData::<PLOT_SECTIONS>::load_from_file(path, false).expect("read-only saved production plot");
    assert_eq!(
        data.chunk_data.len(),
        NUM_CHUNKS,
        "build this opt-in replay with the snapshot's PLOT_SCALE (production uses 5)"
    );
    let chunks = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            Chunk::load(
                -(1 << PLOT_SCALE) + i as i32 / PLOT_WIDTH,
                -(1 << PLOT_SCALE) + i as i32 % PLOT_WIDTH,
                chunk,
            )
        })
        .collect();
    let mut world = PlotWorld::from_chunks(-1, -1, chunks, data.pending_ticks.into_iter().collect());
    *world.piston_state_mut() = data.piston_state;
    if !limits { world.disable_command_output_limits_for_replay(); }
    world
}

#[test]
#[ignore = "requires saved production snapshot and its PLOT_SCALE; read-only full-plot/FPU boundary replay"]
fn saved_fpu_readouts_and_neighbor_keep_their_execution_boundaries() {
    for crop in [true, false] {
        let mut world = saved_world();
        let bounds = if crop { (MIN, MAX) } else { world.get_corners() };
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            stable_instant_inputs: true,
            io_only: true,
            ..Default::default()
        };
        let monitor = Arc::new(TaskMonitor::default());
        monitor.set_budget_multiplier(8);
        let mut backend = GraphBackend::compile_with_options(&world, bounds, world.scheduler().iter_entries().collect(), &options, monitor).unwrap();
        eprintln!(
            "saved FPU crop={crop}: nodes={} pistons={} admission={:?}",
            backend.program.nodes().len(),
            backend.program.pistons().len(),
            backend.logical_device_admission()
        );
        assert_eq!(
            backend.logical_device_admission(),
            &LogicalDeviceAdmission::Active,
            "FPU must compile independently of an ordinary neighboring circuit (crop={crop})"
        );
        let (timing, clock_node) = backend
            .program
            .logical_devices()
            .iter()
            .find_map(|device| {
                let timing = device.circuit.timing.as_ref()?;
                (timing.trigger == backend.positions[&TRIGGER]).then(|| {
                    let clock = timing.actor_specs.iter().find(|actor| actor.piston == timing.clock).unwrap();
                    (timing, clock.node)
                })
            })
            .expect("the admitted clock must be certified against the manifest trigger");
        assert_eq!(backend.program.nodes()[timing.trigger.index()].position, Some(TRIGGER));
        assert!(
            !backend.logical_node_owned(timing.trigger),
            "the source trigger control must remain native Graph work"
        );
        assert!(
            !backend.logical_node_owned(clock_node),
            "the native-action clock piston remains Graph-owned"
        );
        assert!(
            backend.native_action_clock_owner.get(timing.clock.index()).is_some_and(Option::is_some),
            "the admitted frame must bind its certified clock to the native action hook"
        );

        let owned_nodes: FxHashSet<_> = backend
            .program
            .logical_devices()
            .iter()
            .flat_map(|device| device.scope.owned_nodes.iter().copied())
            .collect();
        let native_fpu_nodes = backend
            .program
            .nodes()
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                node.position
                    .filter(|position| inside_fpu(*position))
                    .filter(|_| !owned_nodes.contains(&NodeId::new(index as u32)))
                    .map(|_| NodeId::new(index as u32))
            })
            .collect::<Vec<_>>();
        let indicator_lamps = native_fpu_nodes
            .iter()
            .copied()
            .filter(|&node| matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
            .collect::<Vec<_>>();
        assert_eq!(indicator_lamps.len(), 35, "the FPU's 35 indicator lamps must remain Graph-owned");
        let indicator_positions = indicator_lamps
            .iter()
            .map(|&node| backend.program.nodes()[node.index()].position.unwrap())
            .collect::<Vec<_>>();
        let initial_indicator_blocks = indicator_positions.iter().map(|&position| world.get_block(position)).collect::<Vec<_>>();
        let fpu_controls = native_fpu_nodes
            .iter()
            .filter_map(|&node| {
                let spec = &backend.program.nodes()[node.index()];
                (spec.position != Some(TRIGGER) && matches!(spec.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
                    .then_some(spec.position.unwrap())
            })
            .collect::<Vec<_>>();
        assert!(fpu_controls.len() >= 3, "the native FPU input cone must expose changing control levers");
        let driven_fpu_controls = [fpu_controls[0], fpu_controls[fpu_controls.len() / 2], *fpu_controls.last().unwrap()];
        let observer_nodes = native_fpu_nodes
            .iter()
            .copied()
            .filter(|&node| {
                matches!(
                    backend.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
                )
            })
            .collect::<Vec<_>>();
        assert!(
            !observer_nodes.is_empty(),
            "the native FPU input cone must include its observers or compiled reset observers"
        );
        assert!(
            observer_nodes
                .iter()
                .all(|&node| matches!(backend.state.components[node.index()], ComponentState::Electrical(_))),
            "native observer/reset pulses use electrical component state"
        );
        let native_positions: FxHashSet<_> = native_fpu_nodes
            .iter()
            .filter_map(|&node| backend.program.nodes()[node.index()].position)
            .collect();

        let mut physical_world = saved_world();
        let mut physical = GraphBackend::new_with_positions(
            backend.program.clone(),
            backend.state.clone(),
            backend.future.clock.logical_tick,
            backend.positions.clone(),
        )
        .unwrap();
        let neighbors: Vec<_> = backend
            .program
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.position.is_some_and(|p| !inside_fpu(p)))
            .map(|(i, _)| i)
            .collect();
        let initial_neighbors = neighbors.iter().map(|&i| backend.state.components[i].clone()).collect::<Vec<_>>();
        let mut saw_neighbor_activity = false;
        let mut saw_indicator_transition = false;
        let mut saw_delayed_indicator_lamp_off = false;
        let mut saw_native_scheduled_work = false;
        let mut saw_observer_on = false;
        let mut saw_observer_off_after_on = false;
        let mut observer_was_on = observer_nodes
            .iter()
            .map(|&node| matches!(backend.state.components[node.index()], ComponentState::Electrical(ref state) if state.powered))
            .collect::<Vec<_>>();
        let mut indicator_was_lit = initial_indicator_blocks
            .iter()
            .map(|block| matches!(block, Block::RedstoneLamp { lit: true }))
            .collect::<Vec<_>>();
        let mut indicator_had_scheduled_tick = vec![false; indicator_positions.len()];
        let controls: Vec<_> = neighbors
            .iter()
            .filter_map(|&i| {
                matches!(backend.program.nodes()[i].kind, CircuitNodeKind::Electrical(NodeType::Lever))
                    .then_some(backend.program.nodes()[i].position.unwrap())
            })
            .collect();
        for tick in 0..48 {
            if tick == 0 || tick == 18 {
                for &control in &driven_fpu_controls {
                    backend.use_block(control).unwrap();
                    physical.use_block(control).unwrap();
                }
            }
            if tick == 0 {
                backend.use_block(TRIGGER).unwrap();
                physical.use_block(TRIGGER).unwrap();
            }
            if tick == 24 || tick == 36 {
                backend.use_block(TRIGGER).unwrap();
                physical.use_block(TRIGGER).unwrap();
            }
            if let Some(&control) = controls.get(tick / 2) {
                backend.use_block(control).unwrap();
                physical.use_block(control).unwrap();
            }
            backend.tick_with_world(&mut world).unwrap();
            physical.tick_with_world(&mut physical_world).unwrap();
            backend.flush(&mut world, false);
            physical.flush(&mut physical_world, false);
            for &i in &native_fpu_nodes {
                assert_eq!(
                    backend.state.components[i.index()],
                    physical.state.components[i.index()],
                    "native FPU input-cone node {:?} at tick {tick}",
                    backend.program.nodes()[i.index()].position
                );
            }
            let native_ticks = |graph: &GraphBackend| {
                graph
                    .scheduled_ticks()
                    .into_iter()
                    .filter(|entry| native_positions.contains(&entry.pos))
                    .collect::<Vec<_>>()
            };
            let frame_native_ticks = native_ticks(&backend);
            assert_eq!(frame_native_ticks, native_ticks(&physical), "native FPU scheduled work at tick {tick}");
            saw_native_scheduled_work |= !frame_native_ticks.is_empty();
            for (lamp_index, &position) in indicator_positions.iter().enumerate() {
                let projected = world.get_block(position);
                assert_eq!(
                    projected,
                    physical_world.get_block(position),
                    "FPU indicator lamp at {position:?}, tick {tick}"
                );
                saw_indicator_transition |= projected != initial_indicator_blocks[lamp_index];
                let lit = matches!(projected, Block::RedstoneLamp { lit: true });
                saw_delayed_indicator_lamp_off |= indicator_was_lit[lamp_index] && !lit && indicator_had_scheduled_tick[lamp_index];
                indicator_was_lit[lamp_index] = lit;
                indicator_had_scheduled_tick[lamp_index] = frame_native_ticks.iter().any(|entry| entry.pos == position);
            }
            for (observer_index, &node) in observer_nodes.iter().enumerate() {
                let on = matches!(backend.state.components[node.index()], ComponentState::Electrical(ref state) if state.powered);
                saw_observer_on |= on;
                saw_observer_off_after_on |= observer_was_on[observer_index] && !on;
                observer_was_on[observer_index] = on;
            }
            saw_neighbor_activity |= neighbors.iter().zip(&initial_neighbors).any(|(&i, initial)| {
                !matches!(backend.program.nodes()[i].kind, CircuitNodeKind::Electrical(NodeType::Lever)) && backend.state.components[i] != *initial
            });
            for &i in &neighbors {
                assert_eq!(
                    backend.state.components[i],
                    physical.state.components[i],
                    "independent component {:?} at tick {tick}",
                    backend.program.nodes()[i].position
                );
            }
            for (i, piston) in backend.program.pistons().iter().enumerate() {
                if backend.program.nodes()[piston.node.index()].position.is_some_and(|p| !inside_fpu(p)) {
                    assert_eq!(
                        backend.state.pistons[i],
                        physical.state.pistons[i],
                        "independent piston {:?} at tick {tick}",
                        backend.program.nodes()[piston.node.index()].position
                    );
                }
            }
            for (i, resource) in backend.program.resources().iter().enumerate() {
                if resource
                    .contacts
                    .iter()
                    .all(|contact| !inside_fpu(backend.program.contacts()[contact.index()].position))
                {
                    assert_eq!(
                        backend.state.resources[i], physical.state.resources[i],
                        "independent resource {i} at tick {tick}"
                    );
                }
            }
            let outside_requests = |b: &GraphBackend| {
                b.future
                    .piston_requests
                    .iter()
                    .copied()
                    .filter(|request| {
                        !inside_fpu(
                            b.program.nodes()[b.program.pistons()[request.piston.index()].node.index()]
                                .position
                                .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                outside_requests(&backend),
                outside_requests(&physical),
                "independent piston FIFO at tick {tick}"
            );
            let outside_ticks = |b: &GraphBackend| b.scheduled_ticks().into_iter().filter(|entry| !inside_fpu(entry.pos)).collect::<Vec<_>>();
            assert_eq!(
                outside_ticks(&backend),
                outside_ticks(&physical),
                "independent delayed work at tick {tick}"
            );
            for z in (-221..=-161).step_by(4) {
                assert!(matches!(world.get_block(BlockPos::new(-102, 55, z)), Block::RedstoneRepeater { .. }));
            }
        }
        assert!(saw_indicator_transition, "changing FPU inputs must exercise an indicator lamp transition");
        assert!(
            saw_delayed_indicator_lamp_off,
            "at least one native indicator lamp must turn off after its scheduled Graph tick"
        );
        assert!(
            saw_native_scheduled_work,
            "native input-cone observer/OFF work must stay on Graph's scheduler"
        );
        assert!(
            saw_observer_on && saw_observer_off_after_on,
            "observer pulses must turn on and return off while the frame is admitted"
        );
        assert!(crop || saw_neighbor_activity, "the neighboring circuit must execute real native work");
        eprintln!(
            "saved FPU crop={crop}: devices={} evaluations={} wire_walks={} native_ticks={}",
            backend.logical_devices_count(),
            backend.counts.logical_kernel_evaluations,
            backend.counts.wire_walks,
            backend.counts.scheduled_ticks
        );
        assert!(backend.counts.logical_kernel_evaluations > 0);
        if crop {
            assert!(
                backend.counts.wire_walks < physical.counts.wire_walks,
                "isolated FPU must reduce wire walks against the ordinary Graph replay: candidate={} physical={}",
                backend.counts.wire_walks,
                physical.counts.wire_walks
            );
        }
    }
}

#[test]
#[ignore = "release-only changing-input profile of the saved FPU plot and neighboring Divider"]
fn saved_fpu_neighbor_graph_fallback_profile() {
    run_saved_plot_profile(false);
}

#[test]
#[ignore = "release-only saved FPU changing-input command and six-phase native handoff oracle"]
fn saved_fpu_changing_inputs_refire_commands_with_six_handoffs() {
    run_saved_plot_profile_with_protocol(false, true);
}

pub(crate) fn run_saved_plot_profile(server: bool) {
    run_saved_plot_profile_with_protocol(server, false);
}

fn run_saved_plot_profile_with_protocol(server: bool, command_witness: bool) {
    let optimization_level = std::env::var("MCHPRS_PROFILE_OPT_LEVEL")
        .ok()
        .map(|value| value.parse::<usize>().expect("MCHPRS_PROFILE_OPT_LEVEL must be an integer"))
        .unwrap_or(6);
    assert!((3..=6).contains(&optimization_level), "saved-plot profile supports O3 through O6");
    let snapshot_path = std::env::var_os("MCHPRS_SAVED_PLOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/build-audit-tps/prod/p-1,-1-goal-start"));
    use sha2::{Digest, Sha256};
    assert_eq!(PLOT_SCALE, 5, "saved production snapshot requires PLOT_SCALE=5");
    let snapshot_hash = format!("{:x}", Sha256::digest(std::fs::read(&snapshot_path).expect("read saved snapshot for hashing")));
    assert_eq!(snapshot_hash, "426d2395869062817fa698dca1b7a101c3594728cd440389970a4e6004f06d17", "saved production snapshot hash mismatch");
    let mut world = saved_world_from_with_limits(snapshot_path.clone(), server);
    let bounds = world.get_corners();
    let options = CompilerOptions {
        optimization_level: Some(optimization_level),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        io_only: true,
        ..Default::default()
    };
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let compile_start = Instant::now();
    let ticks = world.scheduler().iter_entries().collect();
    let prepared = crate::redpiler::passes::rich::prepare(&world, bounds, ticks, &monitor, crate::redpiler::analysis::AnalysisLimits::for_budget(8))
        .expect("prepare full saved plot for fallback profile");
    let pipeline = crate::redpiler::passes::default_graph_pipeline(&options);
    let validated = prepared
        .run_passes(&options, &pipeline, &monitor)
        .expect("run full saved-plot Graph pipeline");
    for warning in monitor.warnings() {
        println!("Redpiler warning: {warning}");
    }
    let graph = validated.graph();
    let clock_census_start = Instant::now();
    let timing = crate::redpiler::analysis::instant::layers::logical::timing::candidate_clocks(graph, &monitor)
        .expect("enumerate source-qualified clock candidates")
        .into_iter()
        .filter_map(|clock| crate::redpiler::analysis::instant::layers::logical::timing::infer_for_clock(graph, clock, &monitor).ok())
        .find(|timing| graph.nodes[timing.trigger.index()].position == Some(TRIGGER))
        .expect("find the source-qualified timing island attached to the saved FPU trigger");
    let clock_census_seconds = clock_census_start.elapsed().as_secs_f64();
    let (trigger_node, clock_piston) = (timing.trigger, timing.clock);
    let lowered = validated.lower(&monitor).expect("lower Graph fallback after timing census");
    #[cfg(test)]
    let oracle_lowered = crate::redpiler::program_graph::LoweredCircuit {
        program: lowered.program.clone(),
        state: lowered.state.clone(),
        logical_tick: lowered.logical_tick,
        imported_ticks: lowered.imported_ticks.clone(),
        positions: lowered.positions.clone(),
    };
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).expect("construct full saved-plot Graph backend");
    world.clear_scheduled_ticks();
    let compile_seconds = compile_start.elapsed().as_secs_f64();
    let compilation_statistics = monitor.graph_statistics();
    assert_eq!(backend.program.nodes()[trigger_node.index()].position, Some(TRIGGER));
    let initial_clock_state = backend.state.pistons[clock_piston.index()];
    assert!(
        initial_clock_state.extended && initial_clock_state.head_present,
        "profile requires the stationary extended FPU clock"
    );
    assert_eq!(initial_clock_state.availability, PistonAvailability::Ready);

    let mut fpu_inputs = backend
        .program
        .nodes()
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let position = node.position?;
            (inside_fpu(position) && position != TRIGGER && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
                .then_some((position, NodeId::new(index as u32)))
        })
        .collect::<Vec<_>>();
    fpu_inputs.sort_unstable_by_key(|(position, _)| (position.x, position.y, position.z));
    assert_eq!(fpu_inputs.len(), 35, "profile must drive all FPU input levers in stable coordinate order");
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/fixtures/fpu_legal.json");
    let manifest_hash = format!("{:x}", Sha256::digest(std::fs::read(manifest_path).expect("read manifest for hashing")));
    assert_eq!(manifest_hash, "981109d849cadaf13a059c295a6d78444d0626a21c72683cf425eb6344b68227", "saved FPU manifest hash mismatch");
    let manifest = saved_fpu_manifest();
    let manifest_ports = &manifest["ports"];
    assert_eq!(manifest_position(&manifest_ports["trigger"]), TRIGGER, "manifest MIN offset maps its clock trigger");
    let mut declared_input_positions = [
        manifest_positions(&manifest_ports["light_blue_operand_inputs"]),
        manifest_positions(&manifest_ports["red_operand_inputs"]),
        manifest_positions(&manifest_ports["opcode_levers_by_sign_weight_4_2_1"]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    declared_input_positions = sorted_positions(declared_input_positions);
    let scanned_input_positions = sorted_positions(fpu_inputs.iter().map(|(position, _)| *position).collect());
    assert_eq!(
        declared_input_positions, scanned_input_positions,
        "the saved-plot input controls must exactly match the manifest's 32 operand and 3 opcode ports"
    );
    assert!(
        matches!(world.get_block(TRIGGER), Block::Lever { lever } if lever.powered),
        "profile starts from the saved stopped-clock entry"
    );
    let output_lamps = backend
        .program
        .nodes()
        .iter()
        .filter_map(|node| node.position.filter(|position| inside_fpu(*position)))
        .filter(|&position| matches!(world.get_block(position), Block::RedstoneLamp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(output_lamps.len(), 35, "profile must include the saved FPU's native indicator outputs");
    let readout_positions = (-221..=-161).step_by(4).map(|z| BlockPos::new(-102, 55, z)).collect::<Vec<_>>();
    assert_eq!(readout_positions.len(), 16);
    assert!(
        readout_positions
            .iter()
            .all(|&position| matches!(world.get_block(position), Block::RedstoneRepeater { .. })),
        "saved FPU's 16 readout repeaters must be present"
    );
    let terminal_dust_positions = manifest_positions(&manifest["observations"]["output"]);
    let terminal_readout_positions = sorted_positions(
        terminal_dust_positions
            .iter()
            .map(|position| position.offset(BlockFace::East))
            .collect(),
    );
    assert_eq!(terminal_dust_positions.len(), 16, "manifest declares sixteen far-side dust terminals");
    assert_eq!(
        terminal_readout_positions,
        sorted_positions(readout_positions.clone()),
        "the native readout repeaters must be directly east of the manifest's far-side dust terminals"
    );
    for &position in &terminal_dust_positions {
        assert!(
            matches!(world.get_block(position), Block::RedstoneWire { wire } if !wire.east.is_none()),
            "manifest output dust at {position:?} must connect east into its native readout repeater"
        );
        assert!(
            matches!(world.get_block(position.offset(BlockFace::East)), Block::RedstoneRepeater { .. }),
            "native readout repeater must be adjacent east of FPU terminal {position:?}"
        );
    }

    let neighbor_controls = backend
        .program
        .nodes()
        .iter()
        .filter_map(|node| node.position.filter(|position| !inside_fpu(*position)))
        .filter(|&position| matches!(world.get_block(position), Block::Lever { .. }))
        .take(24)
        .collect::<Vec<_>>();
    assert!(!neighbor_controls.is_empty(), "saved plot must expose neighboring Divider controls");

    let frames = std::env::var("MCHPRS_PROFILE_FRAMES")
        .ok()
        .map(|value| value.parse::<usize>().expect("MCHPRS_PROFILE_FRAMES must be an integer"))
        .unwrap_or(96);
    assert!(frames > 0, "profile frame count must be positive");
    if command_witness { assert!(frames >= 4, "command witness needs at least four frames"); }

    #[cfg(feature = "saved-plot-bench")]
    if server {
        return server_profile::run(world, backend, options, snapshot_path, manifest, fpu_inputs, neighbor_controls, output_lamps, readout_positions, clock_piston, frames, compile_seconds - clock_census_seconds);
    }
    #[cfg(not(feature = "saved-plot-bench"))]
    assert!(!server);

    // Exercise the fixture's known command witness during warmup, then keep the
    // measured random input sequence unchanged and outside warmup accounting.
    let mut random = 0x4d43_4850_5253_2026u64;
    let mask = (1u64 << fpu_inputs.len()) - 1;
    let entry_word = fpu_inputs.iter().enumerate().fold(0u64, |word, (bit, (position, _))| {
        word | (u64::from(matches!(world.get_block(*position), Block::Lever { lever } if lever.powered)) << bit)
    });
    let witness_word = known_command_witness_word(&manifest, &scanned_input_positions);
    let _discarded_entry_word = next_profile_word(&mut random, mask, entry_word);
    let mut previous_word = witness_word;
    let mut first_word = witness_word;
    let warmup_command_count_before = world.command_output().count();
    let warmup_command_entities_before = command_block_census(&backend);
    for (bit, (position, _)) in fpu_inputs.iter().enumerate() {
        if (entry_word ^ first_word) & (1 << bit) != 0 {
            backend.use_block(*position).unwrap();
        }
    }
    // LogicalClock::advance_sticky_observer defines powered trigger as STOP and
    // schedules the sticky retract/extend loop while it is unpowered.
    backend.use_block(TRIGGER).unwrap();
    let clock_revision_before_warmup = backend.state.pistons[clock_piston.index()].revision;
    for _ in 0..24 {
        backend.tick_with_world(&mut world).unwrap();
    }
    backend.flush(&mut world, false);
    let warmup_command_count_after = world.command_output().count();
    let warmup_command_entities_after = command_block_census(&backend);
    let warmup_command_execution_changes = warmup_command_entities_before
        .iter()
        .zip(&warmup_command_entities_after)
        .filter(|(before, after)| before["last_execution"] != after["last_execution"])
        .count();
    assert!(warmup_command_count_after > warmup_command_count_before, "the selector-011 warmup must emit real FPU commands");
    assert!(warmup_command_execution_changes > 0, "the command witness must execute native command entities");
    assert!(
        matches!(world.get_block(TRIGGER), Block::Lever { lever } if !lever.powered),
        "warmup must leave the saved FPU clock running"
    );
    let clock_revision_after_warmup = backend.state.pistons[clock_piston.index()].revision;
    assert!(
        clock_revision_after_warmup > clock_revision_before_warmup,
        "the source-qualified FPU clock must advance during warmup"
    );

    let mut saw_neighbor_activity = false;
    let neighbor_nodes = backend
        .program
        .nodes()
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.position.filter(|position| !inside_fpu(*position)).map(|_| NodeId::new(index as u32)))
        .collect::<Vec<_>>();
    let neighbor_before = neighbor_nodes
        .iter()
        .map(|node| backend.state.components[node.index()].clone())
        .collect::<Vec<_>>();
    let command_count_before = world.command_output().count();
    let command_entities_before = command_block_census(&backend);
    let counts_before = backend.execution_counts();
    let clock_revision_before = backend.state.pistons[clock_piston.index()].revision;
    let clock_state_before = backend.state.pistons[clock_piston.index()];
    let trigger_powered_before = matches!(world.get_block(TRIGGER), Block::Lever { lever } if lever.powered);
    let mut control_seconds = 0.0f64;
    let mut tick_seconds = 0.0f64;
    let mut flush_seconds = 0.0f64;
    let mut input_lever_updates = 0u64;
    let mut neighbor_lever_updates = 0u64;
    let mut output_words = FxHashSet::default();
    let mut saw_output_transition = false;
    let mut prior_output = None;
    let mut readout_words = FxHashSet::default();
    let mut readout_trace = Vec::with_capacity(frames);
    let mut saw_readout_transition = false;
    let mut prior_readout = None;
    let mut saw_readout_high = false;
    let total_start = Instant::now();

    let input_positions = fpu_inputs.iter().map(|(position, _)| *position).collect::<Vec<_>>();
    for frame in 0..frames {
        let control_start = Instant::now();
        let (word, input_updates, neighbor_updates) = if command_witness {
            let word = if frame % 2 == 0 { 0 } else { witness_word };
            drive_profile_word(word, previous_word, &input_positions, &neighbor_controls, frame, |position| backend.use_block(position).unwrap())
        } else {
            drive_profile_frame(&mut random, mask, previous_word, &input_positions, &neighbor_controls, frame, |position| backend.use_block(position).unwrap())
        };
        first_word = word;
        input_lever_updates += input_updates;
        neighbor_lever_updates += neighbor_updates;
        control_seconds += control_start.elapsed().as_secs_f64();
        previous_word = first_word;

        let tick_start = Instant::now();
        for _ in 0..6 {
            backend.tick_with_world(&mut world).unwrap();
        }
        tick_seconds += tick_start.elapsed().as_secs_f64();

        let flush_start = Instant::now();
        backend.flush(&mut world, false);
        flush_seconds += flush_start.elapsed().as_secs_f64();

        let output = output_lamps.iter().enumerate().fold(0u64, |word, (bit, position)| {
            word | (u64::from(matches!(world.get_block(*position), Block::RedstoneLamp { lit: true })) << bit)
        });
        output_words.insert(output);
        saw_output_transition |= prior_output.is_some_and(|previous| previous != output);
        prior_output = Some(output);
        let readout = readout_positions.iter().enumerate().fold(0u16, |word, (bit, position)| {
            word | (u16::from(matches!(world.get_block(*position), Block::RedstoneRepeater { repeater } if repeater.powered)) << bit)
        });
        readout_words.insert(readout);
        readout_trace.push(readout);
        saw_readout_transition |= prior_readout.is_some_and(|previous| previous != readout);
        saw_readout_high |= readout != 0;
        prior_readout = Some(readout);
        saw_neighbor_activity |= neighbor_nodes
            .iter()
            .zip(&neighbor_before)
            .any(|(node, initial)| backend.state.components[node.index()] != *initial);
    }
    let wall_seconds = total_start.elapsed().as_secs_f64();
    let counts_after = backend.execution_counts();
    let clock_revision_after = backend.state.pistons[clock_piston.index()].revision;
    let clock_state_after = backend.state.pistons[clock_piston.index()];
    let trigger_powered_after = matches!(world.get_block(TRIGGER), Block::Lever { lever } if lever.powered);
    let command_count_after = world.command_output().count();
    if command_witness {
        assert!(command_count_after > command_count_before, "changing inputs must refire commands during measurement");
    }
    let command_entities_after = command_block_census(&backend);
    let command_execution_changes = command_entities_before
        .iter()
        .zip(&command_entities_after)
        .filter(|(before, after)| before["last_execution"] != after["last_execution"])
        .count();
    let work = serde_json::json!({
        "instant_wire_node_invocations": counts_after.instant_wire_node_invocations - counts_before.instant_wire_node_invocations,
        "instant_transport_invocations": counts_after.instant_transport_invocations - counts_before.instant_transport_invocations,
        "instant_transport_wire_visits": counts_after.instant_transport_wire_visits - counts_before.instant_transport_wire_visits,
        "instant_transport_compact_captures": counts_after.instant_transport_compact_captures - counts_before.instant_transport_compact_captures,
        "instant_transition_hits": counts_after.instant_transition_hits - counts_before.instant_transition_hits,
        "instant_transition_misses": counts_after.instant_transition_misses - counts_before.instant_transition_misses,
        "instant_transition_programs": counts_after.instant_transition_programs - counts_before.instant_transition_programs,
        "instant_transition_operations": counts_after.instant_transition_operations - counts_before.instant_transition_operations,
        "instant_transition_wire_visits_elided": counts_after.instant_transition_wire_visits_elided - counts_before.instant_transition_wire_visits_elided,
        "instant_value_region_solves": counts_after.instant_value_region_solves - counts_before.instant_value_region_solves,
        "instant_value_region_cache_hits": counts_after.instant_value_region_cache_hits - counts_before.instant_value_region_cache_hits,
        "instant_value_region_inert_visits": counts_after.instant_value_region_inert_visits - counts_before.instant_value_region_inert_visits,
        "signal_channel_invocations": counts_after.signal_channel_invocations - counts_before.signal_channel_invocations,
        "signal_channel_fallbacks": counts_after.signal_channel_fallbacks - counts_before.signal_channel_fallbacks,
        "signal_channel_operations": counts_after.signal_channel_operations - counts_before.signal_channel_operations,
        "stable_logic_invocations": counts_after.stable_logic_invocations - counts_before.stable_logic_invocations,
        "stable_logic_fallbacks": counts_after.stable_logic_fallbacks - counts_before.stable_logic_fallbacks,
        "stable_logic_wire_values": counts_after.stable_logic_wire_values - counts_before.stable_logic_wire_values,
        "stable_logic_boundary_calls": counts_after.stable_logic_boundary_calls - counts_before.stable_logic_boundary_calls,
        "classic_net_invocations": counts_after.classic_net_invocations - counts_before.classic_net_invocations,
        "classic_receiver_ops": counts_after.classic_receiver_ops - counts_before.classic_receiver_ops,
        "ticks": counts_after.ticks - counts_before.ticks,
        "scheduled_ticks": counts_after.scheduled_ticks - counts_before.scheduled_ticks,
        "callbacks": counts_after.callbacks - counts_before.callbacks,
        "wire_walks": counts_after.wire_walks - counts_before.wire_walks,
        "dust_path_power_samples_elided": counts_after.dust_path_power_samples_elided - counts_before.dust_path_power_samples_elided,
        "instant_kernel_invocations": counts_after.instant_kernel_invocations - counts_before.instant_kernel_invocations,
        "instant_kernel_wire_walks_elided": counts_after.instant_kernel_wire_walks_elided - counts_before.instant_kernel_wire_walks_elided,
        "logical_kernel_evaluations": counts_after.logical_kernel_evaluations - counts_before.logical_kernel_evaluations,
        "logical_kernel_operations": counts_after.logical_kernel_operations - counts_before.logical_kernel_operations,
        "piston_samples": counts_after.piston_samples - counts_before.piston_samples,
        "piston_actions": counts_after.piston_actions - counts_before.piston_actions,
        "deferred_publications": counts_after.deferred_publications - counts_before.deferred_publications,
        "logical_bud_samples": counts_after.logical_bud_samples - counts_before.logical_bud_samples,
        "logical_bud_accepts": counts_after.logical_bud_accepts - counts_before.logical_bud_accepts,
        "logical_bud_publications": counts_after.logical_bud_publications - counts_before.logical_bud_publications,
    });
    let admission = backend.logical_device_admission();
    #[cfg(test)]
    let mut oracle_report = replay_saved_fpu_native_oracle(
        &oracle_lowered,
        snapshot_path.clone(),
        &monitor,
        &fpu_inputs.iter().map(|(position, _)| *position).collect::<Vec<_>>(),
        &neighbor_controls,
        entry_word,
        mask,
        &readout_positions,
        frames,
        0,
        command_witness,
    );
    // A frame-aligned handoff alone can hide stale intermediate piston poses.
    #[cfg(test)]
    let other_handoff_phases = (1..6)
        .map(|phase| {
            replay_saved_fpu_native_oracle(
                &oracle_lowered,
                snapshot_path.clone(),
                &monitor,
                &fpu_inputs.iter().map(|(position, _)| *position).collect::<Vec<_>>(),
                &neighbor_controls,
                entry_word,
                mask,
                &readout_positions,
                frames.min(16),
                phase,
                command_witness,
            )
        })
        .collect::<Vec<_>>();
    #[cfg(test)]
    { oracle_report["other_handoff_phases"] = serde_json::json!(other_handoff_phases); }
    #[cfg(not(test))]
    let oracle_report = serde_json::json!({"status": "not run; use the separate ignored native oracle test"});
    let mut report = serde_json::json!({
        "measurement_kind": format!("saved_plot_o{optimization_level}_changing_inputs"),
        "snapshot": snapshot_path,
        "snapshot_sha256": snapshot_hash,
        "manifest_sha256": manifest_hash,
        "measurement_scope": "direct Graph replay, not server TPS; offline command output limits disabled",
        "optimization_level": optimization_level,
        "plan_coverage": {
            "proxy_regions": backend.program.instant_proxies().len(),
            "proxy_routes": backend.program.instant_proxies().iter().map(|proxy| proxy.routes.len()).sum::<usize>(),
            "proxy_sample_steps": backend.program.instant_proxies().iter().flat_map(|proxy| proxy.routes.iter()).flat_map(|route| route.steps.iter()).filter(|step| matches!(step.kind, InstantStepKind::Sample(_))).count(),
            "proxy_reset_steps": backend.program.instant_proxies().iter().flat_map(|proxy| proxy.routes.iter()).flat_map(|route| route.steps.iter()).filter(|step| matches!(step.kind, InstantStepKind::ArmReset(_))).count(),
            "cpu_power_query_actors": backend.program.cpu_power_queries().entries.len(),
            "event_actors": backend.program.instant_kernels().iter().map(|kernel| match &kernel.program { KernelProgram::Events(entries) => entries.len(), _ => 0 }).sum::<usize>(),
            "pull_wave_actors": backend.program.instant_kernels().iter().filter(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. })).map(|kernel| kernel.actors.len()).sum::<usize>(),
        },
        "compile_seconds": compile_seconds - clock_census_seconds,
        "compilation_passes": compilation_statistics.passes.iter().map(|pass| serde_json::json!({
            "name": pass.name,
            "seconds": pass.duration.as_secs_f64(),
            "validation_seconds": pass.validation_duration.as_secs_f64(),
        })).collect::<Vec<_>>(),
        "kernel_lowering_seconds": compilation_statistics.kernel_lowering_duration.as_secs_f64(),
        "program_lowering_seconds": compilation_statistics.program_lowering_duration.as_secs_f64(),
        "compile_seconds_including_profile_clock_census": compile_seconds,
        "source_clock_census_seconds": clock_census_seconds,
        "stable_instant_inputs": true,
        "frames": frames,
        "input_levers": fpu_inputs.len(),
        "warmup_input_word": witness_word,
        "warmup_input_protocol": "manifest-derived A=0, B=0xffff, opcode sign weights 2+1 (selector 011); fixture native tellraw witness",
        "input_lever_positions": scanned_input_positions,
        "manifest_input_port_positions": declared_input_positions,
        "input_lever_updates": input_lever_updates,
        "neighbor_lever_updates": neighbor_lever_updates,
        "input_order": if command_witness { "alternating zero and manifest-derived command witness" } else { "ascending x,y,z coordinate order; raw changing masks, not an arithmetic oracle" },
        "clock_trigger_position": backend.program.nodes()[trigger_node.index()].position,
        "clock_trigger_powered_at_entry": true,
        "clock_trigger_powered_before_measurement": trigger_powered_before,
        "clock_trigger_powered_after_measurement": trigger_powered_after,
        "clock_trigger_semantics": "source-qualified sticky clock: powered means STOP; unpowered runs the retract/extend cycle",
        "clock_piston": clock_piston.index(),
        "clock_piston_state_before_measurement": format!("{clock_state_before:?}"),
        "clock_piston_state_after_measurement": format!("{clock_state_after:?}"),
        "clock_piston_revision_before_measurement": clock_revision_before,
        "clock_piston_revision_after_measurement": clock_revision_after,
        "clock_piston_revision_delta": clock_revision_after - clock_revision_before,
        "game_ticks": frames * 6,
        "scope": "full saved PlotWorld; inputs and neighbor lever actions, all Graph ticks, flushes, per-frame lamp observation, and neighbor activity checks included; commands execute natively",
        "warmup_ticks_excluded": 24,
        "logical_device_admission": format!("{admission:?}"),
        "logical_devices": backend.logical_devices_count(),
    });
    report.as_object_mut().unwrap().extend(serde_json::json!({
        "wall_seconds": wall_seconds,
        "controls_seconds": control_seconds,
        "tick_seconds": tick_seconds,
        "flush_seconds": flush_seconds,
        "game_ticks_per_second": (frames * 6) as f64 / wall_seconds,
        "native_command_outputs": command_count_after - command_count_before,
        "warmup_command_outputs": warmup_command_count_after - warmup_command_count_before,
        "warmup_command_execution_changes": warmup_command_execution_changes,
        "warmup_command_entities_before": warmup_command_entities_before,
        "warmup_command_entities_after": warmup_command_entities_after,
        "command_execution_changes": command_execution_changes,
        "command_entities_before_measurement": command_entities_before,
        "command_entities_after_measurement": command_entities_after,
        "native_oracle": oracle_report,
        "unique_indicator_words": output_words.len(),
        "indicator_transition": saw_output_transition,
        "fpu_readout_positions": readout_positions,
        "manifest_terminal_dust_positions": terminal_dust_positions,
        "terminal_to_readout_mapping": "each declared output dust terminal connects east to the saved native repeater at the corresponding z",
        "fpu_readout_words": readout_trace,
        "unique_fpu_readout_words": readout_words.len(),
        "fpu_readout_transition": saw_readout_transition,
        "fpu_readout_nonzero": saw_readout_high,
        "neighbor_activity": saw_neighbor_activity,
        "execution_counts_scope": "measured interval only: after stopped-entry input setup and 24 running warmup ticks; through all measured frames",
        "work": work,
    }).as_object().unwrap().clone());
    eprintln!("saved FPU+Divider profile: {report:#}");
    assert!(
        clock_revision_after > clock_revision_before,
        "FPU clock must perform real piston actions in the measured window"
    );
    assert!(
        saw_readout_transition && readout_words.len() > 1 && saw_readout_high,
        "profile must exercise the 16-bit FPU readout bus"
    );
    assert!(
        saw_neighbor_activity,
        "neighboring Divider must perform real work during the measured window"
    );
    assert_eq!(counts_after.ticks - counts_before.ticks, (frames * 6) as u64);
}

#[cfg(test)]
fn replay_saved_fpu_native_oracle(
    lowered: &crate::redpiler::program_graph::LoweredCircuit,
    snapshot_path: std::path::PathBuf,
    monitor: &TaskMonitor,
    input_positions: &[BlockPos],
    neighbor_controls: &[BlockPos],
    entry_word: u64,
    mask: u64,
    readout_positions: &[BlockPos],
    oracle_frames: usize,
    handoff_phase: usize,
    command_witness: bool,
) -> serde_json::Value {
    assert!(handoff_phase < 6, "handoff phase must be within the six-tick input frame");
    let mut graph_world = saved_world_from(snapshot_path.clone());
    let mut native_world = saved_world_from(snapshot_path);
    let mut graph = GraphBackend::from_lowered(
        crate::redpiler::program_graph::LoweredCircuit {
            program: lowered.program.clone(),
            state: lowered.state.clone(),
            logical_tick: lowered.logical_tick,
            imported_ticks: lowered.imported_ticks.clone(),
            positions: lowered.positions.clone(),
        },
        monitor,
    )
    .expect("rebuild saved-plot Graph for native oracle");
    // Production activation transfers scheduled work into Graph and clears the host queue.
    graph_world.clear_scheduled_ticks();
    let command_positions = graph
        .program
        .command_blocks()
        .iter()
        .filter_map(|plan| graph.program.nodes()[plan.node.index()].position)
        .collect::<Vec<_>>();
    let mut observable_positions = graph
        .program
        .nodes()
        .iter()
        .filter_map(|node| node.position)
        .filter(|&position| {
            matches!(
                graph_world.get_block(position),
                Block::RedstoneLamp { .. } | Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }
            )
        })
        .collect::<Vec<_>>();
    observable_positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    observable_positions.dedup();
    let comparator_positions = observable_positions
        .iter()
        .copied()
        .filter(|&position| matches!(graph_world.get_block(position), Block::RedstoneComparator { .. }))
        .collect::<Vec<_>>();
    let manifest = saved_fpu_manifest();
    let witness_word = known_command_witness_word(&manifest, input_positions);
    let mut random = 0x4d43_4850_5253_2026u64;
    let _discarded_entry_word = next_profile_word(&mut random, mask, entry_word);
    let mut previous_word = witness_word;
    let mut word = witness_word;
    for (bit, &position) in input_positions.iter().enumerate() {
        if (entry_word ^ word) & (1 << bit) != 0 {
            graph.use_block(position).unwrap();
            native_use_lever(&mut native_world, position);
        }
    }
    graph.use_block(TRIGGER).unwrap();
    native_use_lever(&mut native_world, TRIGGER);

    let mut ticks_compared = 0usize;
    let mut native_command_outputs = 0usize;
    let mut warmup_command_outputs = 0usize;
    let mut measured_command_outputs = 0usize;
    let initial_native_command_outputs = native_world.command_output().count();
    let mut readout_words = FxHashSet::default();
    let mut physical_positions = graph.program.nodes().iter().filter_map(|node| node.position).collect::<Vec<_>>();
    physical_positions.extend(graph.program.contacts().iter().map(|contact| contact.position));
    physical_positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    physical_positions.dedup();
    let assert_physical_state = |graph_world: &PlotWorld, native_world: &PlotWorld, cut: &str| {
        for &position in &physical_positions {
            assert_eq!(
                graph_world.get_block(position),
                native_world.get_block(position),
                "native physical block {cut}, phase {handoff_phase}, at {position:?}"
            );
            assert_eq!(
                graph_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                native_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                "native block entity {cut}, phase {handoff_phase}, at {position:?}"
            );
        }
    };
    let graph_ticks = 24 + oracle_frames * 6 + handoff_phase;
    let continuation_ticks = 16;
    for step in 0..(graph_ticks + continuation_ticks) {
        if step >= 24 && step < graph_ticks && step % 6 == 0 {
            let frame = (step - 24) / 6;
            word = if command_witness {
                if frame % 2 == 0 { 0 } else { witness_word }
            } else {
                next_profile_word(&mut random, mask, previous_word)
            };
            for (bit, &position) in input_positions.iter().enumerate() {
                if (previous_word ^ word) & (1 << bit) != 0 {
                    graph.use_block(position).unwrap();
                    native_use_lever(&mut native_world, position);
                }
            }
            for (index, &position) in neighbor_controls.iter().enumerate() {
                if frame == index * 2 {
                    graph.use_block(position).unwrap();
                    native_use_lever(&mut native_world, position);
                }
            }
            previous_word = word;
        }
        if step == graph_ticks {
            graph.reset(&mut graph_world);
            assert_physical_state(&graph_world, &native_world, "at running-clock handoff");
            assert_eq!(
                super::adder_tests::first_difference(
                    &super::adder_tests::future(&native_world, &[]),
                    &super::adder_tests::future(&graph_world, &[]),
                ),
                None,
                "native scheduler, requests, and motions at running-clock handoff"
            );
        }
        if step < graph_ticks {
            graph.tick_with_world(&mut graph_world).unwrap();
        } else {
            graph_world.tick_interpreted();
        }
        native_world.tick_interpreted();
        if step < graph_ticks {
            graph.flush(&mut graph_world, false);
        } else {
            assert_eq!(
                super::adder_tests::first_difference(
                    &super::adder_tests::future(&native_world, &[]),
                    &super::adder_tests::future(&graph_world, &[]),
                ),
                None,
                "native scheduler, requests, and motions after handoff at tick {step}"
            );
        }

        for &position in &observable_positions {
            assert_eq!(
                graph_world.get_block(position),
                native_world.get_block(position),
                "native plot lamp/repeater/comparator at tick {step}, {position:?}"
            );
        }
        for &position in &comparator_positions {
            assert_eq!(
                graph_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                native_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                "native comparator block entity at tick {step}, {position:?}"
            );
        }
        for &position in &command_positions {
            assert_eq!(
                graph_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                native_world.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
                "native command-block entity at tick {step}, {position:?}"
            );
        }
        let graph_output = graph_world.command_output().collect::<Vec<_>>();
        let native_output = native_world.command_output().collect::<Vec<_>>();
        assert_eq!(graph_output, native_output, "native command output at tick {step}");
        native_command_outputs = native_output.len() - initial_native_command_outputs;
        if step + 1 == graph_ticks { measured_command_outputs = native_command_outputs; }
        if step == 23 {
            warmup_command_outputs = native_command_outputs;
            assert!(warmup_command_outputs > 0, "native selector-011 warmup must exercise the FPU command outputs");
        }
        let readout_word = readout_positions.iter().enumerate().fold(0u16, |word, (bit, &position)| {
            word | (u16::from(matches!(graph_world.get_block(position), Block::RedstoneRepeater { repeater } if repeater.powered)) << bit)
        });
        readout_words.insert(readout_word);
        ticks_compared += 1;
    }
    assert_physical_state(&graph_world, &native_world, "after interpreter continuation");
    if command_witness {
        assert!(measured_command_outputs > warmup_command_outputs, "changing inputs must refire native commands before handoff");
    }
    serde_json::json!({
        "oracle": "native PlotWorld::tick_interpreted versus GraphBackend from the same validated program",
        "frames": oracle_frames,
        "handoff_phase_within_input_frame": handoff_phase,
        "warmup_input_word": witness_word,
        "ticks_compared": ticks_compared,
        "graph_ticks_compared": graph_ticks,
        "native_continuation_ticks": continuation_ticks,
        "physical_positions_compared_at_handoff": physical_positions.len(),
        "physical_positions_compared_after_continuation": physical_positions.len(),
        "plot_output_comparison": format!(
            "{} lamps, repeaters, and comparators compared after every game tick, including clock warmup; {} comparator block entities also compared",
            observable_positions.len(), comparator_positions.len()
        ),
        "command_effect_comparison": "all compiled command-block entities and command-output history compared after every game tick",
        "warmup_command_outputs": warmup_command_outputs,
        "command_outputs_before_handoff": measured_command_outputs,
        "command_outputs_observed": native_command_outputs,
        "distinct_readout_words": readout_words.len(),
    })
}

#[cfg(any(test, feature = "saved-plot-bench"))]
fn native_use_lever(world: &mut PlotWorld, position: BlockPos) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("oracle input {position:?} is not a lever")
    };
    lever.powered = !lever.powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let attachment = match lever.face {
        mchprs_blocks::blocks::LeverFace::Floor => mchprs_blocks::BlockFace::Bottom,
        mchprs_blocks::blocks::LeverFace::Ceiling => mchprs_blocks::BlockFace::Top,
        mchprs_blocks::blocks::LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(attachment));
}

fn drive_profile_frame(
    random: &mut u64, mask: u64, previous: u64, inputs: &[BlockPos], neighbors: &[BlockPos],
    frame: usize, use_block: impl FnMut(BlockPos),
) -> (u64, u64, u64) {
    let word = next_profile_word(random, mask, previous);
    drive_profile_word(word, previous, inputs, neighbors, frame, use_block)
}

fn drive_profile_word(
    word: u64, previous: u64, inputs: &[BlockPos], neighbors: &[BlockPos],
    frame: usize, mut use_block: impl FnMut(BlockPos),
) -> (u64, u64, u64) {
    let mut input_updates = 0;
    let mut neighbor_updates = 0;
    for (bit, &position) in inputs.iter().enumerate() {
        if (previous ^ word) & (1 << bit) != 0 {
            use_block(position);
            input_updates += 1;
        }
    }
    for (index, &position) in neighbors.iter().enumerate() {
        if frame == index * 2 {
            use_block(position);
            neighbor_updates += 1;
        }
    }
    (word, input_updates, neighbor_updates)
}

#[cfg(feature = "saved-plot-bench")]
#[path = "saved_plot_server.rs"]
mod server_profile;

fn next_profile_word(random: &mut u64, mask: u64, previous: u64) -> u64 {
    *random ^= *random << 13;
    *random ^= *random >> 7;
    *random ^= *random << 17;
    let word = *random & mask;
    if word == previous { word ^ 1 } else { word }
}
