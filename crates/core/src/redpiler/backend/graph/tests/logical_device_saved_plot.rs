//! Opt-in replay of the production snapshot, including the neighboring Divider.
use super::*;
use crate::plot::{NUM_CHUNKS, PLOT_SCALE, PLOT_SECTIONS, PLOT_WIDTH, PlotWorld};
use crate::redpiler::{CompilerOptions, LogicalDeviceAdmission};
use crate::world::storage::Chunk;
use mchprs_save_data::plot_data::PlotData;
use std::sync::Arc;

const MIN: BlockPos = BlockPos::new(-273, 50, -253);
const MAX: BlockPos = BlockPos::new(-102, 92, -147);
const TRIGGER: BlockPos = BlockPos::new(-267, 86, -177);

fn inside_fpu(p: BlockPos) -> bool {
    p.x >= MIN.x && p.x <= MAX.x && p.y >= MIN.y && p.y <= MAX.y && p.z >= MIN.z && p.z <= MAX.z
}

fn saved_world() -> PlotWorld {
    let path = std::env::var_os("MCHPRS_SAVED_PLOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/build-audit-tps/prod/p-1,-1"));
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
    world.disable_command_output_limits_for_replay();
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
            backend.logical_node_owned(clock_node),
            "the certified logical clock piston must belong to the admitted device"
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
