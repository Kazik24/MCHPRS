//! Real block layouts exercise entry alternatives, shared ownership and retained outside walks.
use super::*;
use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};

fn add_control(world: &mut PlotWorld, position: BlockPos) {
    world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        position,
        Block::Lever {
            lever: Lever {
                face: LeverFace::Floor,
                facing: BlockDirection::North,
                powered: true,
            },
        },
    );
}

fn compile_world(world: &PlotWorld) -> GraphBackend {
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    let plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert!(!plan.regions.is_empty(), "expected a compiled region: {:?}", plan.declines);
    prepared.graph.plans.instant.boundaries = Some(catalog);
    prepared.graph.plans.instant.layers = Some(plan);
    prepared.graph.validate().unwrap();
    crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    assert!(!program.instant_kernels().is_empty(), "the artifact must actually execute");
    GraphBackend::new(program, state, 0).unwrap()
}

fn check_ticks(backend: &mut GraphBackend, native: &mut PlotWorld, projected: &mut PlotWorld, label: &str) {
    let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
    for tick in 0..24 {
        backend.tick().unwrap();
        native.tick_interpreted();
        check_boundary(backend, native, projected, &positions, &format!("{label} tick={tick}")).unwrap();
    }
}

fn turn_off(backend: &mut GraphBackend, native: &mut PlotWorld, position: BlockPos) {
    backend.use_block(position).unwrap();
    native_step(native, Step::SetLever { position, powered: false });
}

#[test]
fn either_boundary_route_can_start_a_direct_instant_chain() {
    let alternate = BASE.offset(BlockFace::North);
    for order in [[CONTROL, alternate], [alternate, CONTROL]] {
        let mut world = direct_chain_world();
        add_control(&mut world, alternate);
        let mut backend = compile_world(&world);
        assert!(backend.program.instant_kernels()[0].entries.len() >= 2);
        let mut native = direct_chain_world();
        add_control(&mut native, alternate);
        turn_off(&mut backend, &mut native, order[0]);
        turn_off(&mut backend, &mut native, order[1]);
        check_ticks(&mut backend, &mut native, &mut world, "alternative entry");
        assert!(backend.execution_counts().instant_kernel_invocations > 0);
    }
}

#[test]
fn singleton_retains_its_power_output_and_outside_dust_walk() {
    let mut world = output_world();
    let mut backend = compile_world(&world);
    assert!(backend.program.instant_kernels()[0].wires.is_empty(), "the outside wire was not erased");
    let mut native = output_world();
    turn_off(&mut backend, &mut native, CONTROL);
    check_ticks(&mut backend, &mut native, &mut world, "ordinary output");
    assert!(backend.execution_counts().instant_kernel_invocations > 0);
    assert!(backend.execution_counts().wire_walks > 0, "outside propagation remains observable");
}

fn output_world() -> PlotWorld {
    let mut world = direct_cell_world();
    let output = BlockPos::new(11, 20, 8);
    world.set_block(output.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        output,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                15,
            ),
        },
    );
    world
}

#[test]
fn wired_observer_reset_remains_an_outside_clock_for_a_compiled_pull() {
    let build = || {
        let mut world = direct_cell_world();
        for position in [
            BASE.offset(BlockFace::East),
            BASE.offset(BlockFace::East).offset(BlockFace::East),
            BASE.offset(BlockFace::Top),
        ] {
            world.set_block(position, Block::Air);
        }
        world.set_block(
            BASE,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::Down,
                    sticky: true,
                    extended: true,
                },
            },
        );
        world.set_block(
            BASE.offset(BlockFace::Bottom),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::Down,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(BASE.offset(BlockFace::Bottom).offset(BlockFace::Bottom), Block::RedstoneBlock);
        world.set_block(
            BASE.offset(BlockFace::South),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::North,
                    powered: false,
                },
            },
        );
        world.set_block(BASE + BlockPos::new(0, 0, 2), Block::Stone {});
        world.set_block(BASE + BlockPos::new(0, 1, 1), Block::Stone {});
        world.set_block(
            BASE + BlockPos::new(0, 1, 2),
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            },
        );
        world
    };
    let mut world = build();
    let mut native = build();
    let mut backend = compile_world(&world);
    assert!(backend.program.instant_kernels()[0].wires.is_empty());
    turn_off(&mut backend, &mut native, CONTROL);
    check_ticks(&mut backend, &mut native, &mut world, "wired reset");
    assert!(backend.execution_counts().instant_kernel_invocations > 0);
    assert!(backend.execution_counts().wire_walks > 0, "the reset clock remains ordinary Graph work");
}

#[test]
fn shared_payload_keeps_one_token_and_the_real_fifo_winner() {
    let control = BlockPos::new(10, 20, 5);
    for controls in [vec![CONTROL], vec![control], vec![CONTROL, control], vec![control, CONTROL]] {
        let mut world = shared_world();
        let mut backend = compile_world(&world);
        assert_eq!(backend.state.resources.len(), 1);
        assert_eq!(backend.program.instant_kernels()[0].actors.len(), 2);
        let mut native = shared_world();
        for &selected in &controls {
            turn_off(&mut backend, &mut native, selected);
        }
        check_ticks(&mut backend, &mut native, &mut world, "shared token");
        if controls.len() == 1 {
            assert!(backend.execution_counts().instant_kernel_invocations > 0);
        } else {
            assert_eq!(
                backend.execution_counts().instant_kernel_invocations,
                0,
                "mixed entries keep the actual FIFO on Graph"
            );
        }
    }
}

#[test]
fn singleton_kernels_leave_unrelated_queued_actions_in_order() {
    let build = || {
        let mut world = direct_cell_world();
        for position in [
            BASE,
            BASE.offset(BlockFace::East),
            BASE.offset(BlockFace::East).offset(BlockFace::East),
            BASE.offset(BlockFace::Top),
            BASE.offset(BlockFace::Top).offset(BlockFace::Top),
            CONTROL,
            CONTROL.offset(BlockFace::Bottom),
        ] {
            let block = world.get_block(position);
            world.set_block(position + BlockPos::new(0, 0, 5), block);
        }
        world
    };
    for controls in [[CONTROL, CONTROL + BlockPos::new(0, 0, 5)], [CONTROL + BlockPos::new(0, 0, 5), CONTROL]] {
        let mut world = build();
        let mut native = build();
        let mut backend = compile_world(&world);
        assert_eq!(backend.program.instant_kernels().len(), 2);
        for control in controls {
            turn_off(&mut backend, &mut native, control);
        }
        assert_eq!(backend.future.piston_requests.len(), 2);
        check_ticks(&mut backend, &mut native, &mut world, "unrelated queued actions");
        assert_eq!(backend.execution_counts().instant_kernel_invocations, 2);
    }
}

fn shared_world() -> PlotWorld {
    let second = BlockPos::new(10, 20, 6);
    let control = BlockPos::new(10, 20, 5);
    let mut world = direct_cell_world();
    world.set_block(
        second,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        second.offset(BlockFace::South),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::South,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(
        second.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(second.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    add_control(&mut world, control);
    world
}
