//! Per-cut region execution must preserve native Graph state and host effects.
use super::*;
use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
use mchprs_blocks::blocks::{Block, Lever, LeverFace};
use mchprs_blocks::{BlockDirection, BlockFace};

const COMMAND: BlockPos = BlockPos::new(5, 20, 5);
const COMMAND_CHAIN: BlockPos = BlockPos::new(6, 20, 5);
const COMMAND_LEVER: BlockPos = BlockPos::new(5, 20, 4);

fn boundary_world() -> PlotWorld {
    let mut world = chain_world(true);
    world.set_block(COMMAND_LEVER.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        COMMAND_LEVER,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for (position, name, command, automatic) in [
        (COMMAND, "command_block", "say boundary", false),
        (COMMAND_CHAIN, "chain_command_block", r#"tellraw @a {"text":"boundary chain"}"#, true),
    ] {
        let mut block = Block::from_name(name).unwrap();
        block.set_properties(std::collections::HashMap::from([("facing", "east")]));
        world.set_block(position, block);
        world.set_block_entity(
            position,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: command.into(),
                automatic,
                ..Default::default()
            })),
        );
    }
    world
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
    crate::redstone::update_surrounding_blocks(world, position.offset(BlockFace::Bottom));
}

fn assert_clear_wires_artifact(backend: &GraphBackend) {
    assert!(
        backend.program.instant_kernels().iter().any(|kernel| {
            kernel
                .routes
                .iter()
                .any(|route| route.steps.iter().any(|step| matches!(&step.kind, KernelRouteStepKind::ClearWires(_))))
        }),
        "the admitted route must contain certified work to elide"
    );
}

fn assert_host_matches_native(left: &PlotWorld, right: &PlotWorld, positions: &[BlockPos], label: &str) {
    for &position in positions {
        assert_eq!(left.get_block(position), right.get_block(position), "{label}: block at {position:?}");
        assert_eq!(
            left.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
            right.get_block_entity(position).map(bincode::serialize).transpose().unwrap(),
            "{label}: block entity at {position:?}"
        );
    }
    assert_eq!(
        left.command_output().map(str::to_owned).collect::<Vec<_>>(),
        right.command_output().map(str::to_owned).collect::<Vec<_>>(),
        "{label}: command output history"
    );
    let left_future = super::super::super::super::adder_tests::future(left, positions);
    let right_future = super::super::super::super::adder_tests::future(right, positions);
    assert_eq!(left_future, right_future, "{label}: native scheduler, events, and piston motions");
}

fn assert_command_history_matches(left: &PlotWorld, right: &PlotWorld, label: &str) {
    assert_eq!(
        left.command_output().map(str::to_owned).collect::<Vec<_>>(),
        right.command_output().map(str::to_owned).collect::<Vec<_>>(),
        "{label}: command output history"
    );
}

fn run_handoff_phase(phase: usize) {
    let initial = boundary_world();
    let mut optimized = compile_o6_world(&initial, true, 2);
    let mut exact = compile_o6_without_pull_waves(&initial);
    let mut native = boundary_world();
    let mut optimized_world = boundary_world();
    let mut exact_world = boundary_world();
    let mut optimized_projection = boundary_world();
    let mut exact_projection = boundary_world();
    let site_positions = optimized.program.sites().iter().map(|site| site.position).collect::<Vec<_>>();
    let mut physical_positions = site_positions.clone();
    physical_positions.extend(optimized.program.contacts().iter().map(|contact| contact.position));
    physical_positions.sort_unstable_by_key(|position| (position.x, position.y, position.z));
    physical_positions.dedup();

    optimized.use_block(CONTROL).unwrap();
    exact.use_block(CONTROL).unwrap();
    set_native_lever(&mut native, CONTROL, false);
    optimized.use_block(COMMAND_LEVER).unwrap();
    exact.use_block(COMMAND_LEVER).unwrap();
    set_native_lever(&mut native, COMMAND_LEVER, true);

    for tick in 0..phase {
        optimized.tick_with_world(&mut optimized_world).unwrap();
        exact.tick_with_world(&mut exact_world).unwrap();
        native.tick_interpreted();
        assert_backend_state_equal(&optimized, &exact, &format!("phase {phase}, pre-handoff tick {tick}"));
        check_boundary(
            &optimized,
            &native,
            &mut optimized_projection,
            &site_positions,
            &format!("phase {phase}, optimized/native tick {tick}"),
        )
        .unwrap();
        check_boundary(
            &exact,
            &native,
            &mut exact_projection,
            &site_positions,
            &format!("phase {phase}, exact/native tick {tick}"),
        )
        .unwrap();
        assert_command_history_matches(&optimized_world, &native, &format!("phase {phase}, tick {tick}"));
        assert_command_history_matches(&exact_world, &native, &format!("phase {phase}, exact tick {tick}"));
    }

    assert_backend_state_equal(&optimized, &exact, &format!("phase {phase}, handoff state"));
    optimized.reset(&mut optimized_world);
    exact.reset(&mut exact_world);
    assert_host_matches_native(&optimized_world, &native, &physical_positions, &format!("phase {phase}, handoff cut"));
    assert_host_matches_native(&exact_world, &native, &physical_positions, &format!("phase {phase}, exact handoff cut"));

    for tick in 0..16 {
        optimized_world.tick_interpreted();
        exact_world.tick_interpreted();
        native.tick_interpreted();
        assert_host_matches_native(
            &optimized_world,
            &native,
            &physical_positions,
            &format!("phase {phase}, interpreted continuation tick {tick}"),
        );
        assert_host_matches_native(
            &exact_world,
            &native,
            &physical_positions,
            &format!("phase {phase}, exact continuation tick {tick}"),
        );
    }
}

#[test]
fn pull_region_preserves_native_fifo_and_command_history_each_tick() {
    let initial = boundary_world();
    let mut optimized = compile_o6_world(&initial, true, 2);
    assert_clear_wires_artifact(&optimized);
    let mut exact = compile_o6_without_pull_waves(&initial);
    let mut native = boundary_world();
    let mut optimized_world = boundary_world();
    let mut exact_world = boundary_world();
    let mut optimized_projection = boundary_world();
    let mut exact_projection = boundary_world();
    let positions = optimized.program.sites().iter().map(|site| site.position).collect::<Vec<_>>();
    let piston_source = CONTROL;

    // Start the same piston retraction in the optimized, exact Graph, and interpreter runs.
    optimized.use_block(piston_source).unwrap();
    exact.use_block(piston_source).unwrap();
    set_native_lever(&mut native, piston_source, false);
    assert!(optimized.future.piston_requests.front().is_some_and(|request| request.source.is_some()));
    assert_eq!(optimized.future.piston_requests, exact.future.piston_requests);

    // A separate native command chain proves that ordered host effects continue to run.
    optimized.use_block(COMMAND_LEVER).unwrap();
    exact.use_block(COMMAND_LEVER).unwrap();
    set_native_lever(&mut native, COMMAND_LEVER, true);

    for tick in 0..32 {
        optimized.tick_with_world(&mut optimized_world).unwrap();
        exact.tick_with_world(&mut exact_world).unwrap();
        native.tick_interpreted();

        assert_backend_state_equal(&optimized, &exact, &format!("region tick {tick}"));
        check_boundary(
            &optimized,
            &native,
            &mut optimized_projection,
            &positions,
            &format!("optimized/native tick {tick}"),
        )
        .unwrap();
        check_boundary(&exact, &native, &mut exact_projection, &positions, &format!("exact/native tick {tick}")).unwrap();

        let native_output = native.command_output().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            optimized_world.command_output().map(str::to_owned).collect::<Vec<_>>(),
            native_output,
            "optimized command output at tick {tick}"
        );
        assert_eq!(
            exact_world.command_output().map(str::to_owned).collect::<Vec<_>>(),
            native_output,
            "exact command output at tick {tick}"
        );
    }

    let output = native.command_output().map(str::to_owned).collect::<Vec<_>>();
    assert!(!output.is_empty(), "the host-effect assertion requires real command output");
    assert!(output.iter().any(|line| line.contains("[@] boundary")), "say output: {output:?}");
    assert!(output.iter().any(|line| line.contains("boundary chain")), "tellraw output: {output:?}");

    let optimized_counts = optimized.execution_counts();
    let exact_counts = exact.execution_counts();
    assert!(optimized_counts.instant_kernel_invocations > 0, "the compiled region must execute");
    assert!(
        optimized_counts.instant_kernel_wire_walks_elided > 0,
        "the compiled route must execute its certified ClearWires step: {optimized_counts:?}"
    );
    assert!(
        optimized_counts.callbacks + optimized_counts.wire_walks < exact_counts.callbacks + exact_counts.wire_walks,
        "the compiled region must remove Graph work: optimized={optimized_counts:?}, exact={exact_counts:?}"
    );
}

#[test]
fn region_handoff_matches_native_at_each_of_six_tick_offsets() {
    for phase in 0..6 {
        run_handoff_phase(phase);
    }
}
