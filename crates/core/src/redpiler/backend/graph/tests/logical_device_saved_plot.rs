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
        if crop {
            assert_eq!(
                backend.logical_device_admission(),
                &LogicalDeviceAdmission::Active,
                "terminal readout repeaters must not hide the isolated FPU's computation"
            );
        } else if backend.logical_device.is_none() {
            assert!(
                matches!(backend.logical_device_admission(), LogicalDeviceAdmission::Declined(_)),
                "a mixed plot must explain why it keeps general Graph execution"
            );
            continue;
        }

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
        let controls: Vec<_> = neighbors
            .iter()
            .filter_map(|&i| {
                matches!(backend.program.nodes()[i].kind, CircuitNodeKind::Electrical(NodeType::Lever))
                    .then_some(backend.program.nodes()[i].position.unwrap())
            })
            .collect();
        backend.use_block(TRIGGER).unwrap();
        physical.use_block(TRIGGER).unwrap();
        for tick in 0..48 {
            if let Some(&control) = controls.get(tick / 2) {
                backend.use_block(control).unwrap();
                physical.use_block(control).unwrap();
            }
            backend.tick_with_world(&mut world).unwrap();
            physical.tick_with_world(&mut physical_world).unwrap();
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
            backend.flush(&mut world, false);
            for z in (-221..=-161).step_by(4) {
                assert!(matches!(world.get_block(BlockPos::new(-102, 55, z)), Block::RedstoneRepeater { .. }));
            }
        }
        assert!(backend.counts.logical_kernel_evaluations > 0);
        if crop {
            assert_eq!(backend.counts.wire_walks, 0, "isolated FPU must execute without bulk wire walks");
        }
    }
}
