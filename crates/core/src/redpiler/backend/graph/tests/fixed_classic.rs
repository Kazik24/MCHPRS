//! Whole fixed-circuit lowering: all wires execute typed instructions, without Graph dust fallback.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::{
    BlockDirection,
    blocks::{ButtonFace, RedstoneComparator},
};
use serde_json::json;
use std::time::{Duration, Instant};

const A: BlockPos = BlockPos::new(5, 20, 5);
const B: BlockPos = BlockPos::new(8, 20, 5);
const RECEIVER: BlockPos = BlockPos::new(6, 20, 6);
const WIRES: [BlockPos; 3] = [BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5), BlockPos::new(7, 20, 6)];

fn analog_world() -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for (position, facing) in [(A, BlockDirection::West), (B, BlockDirection::East), (RECEIVER, BlockDirection::North)] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(facing, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
    }
    for position in WIRES {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    for position in WIRES {
        let wire = crate::redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
    }
    world
}

pub(super) fn compile(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> GraphBackend {
    compile_mode(world, bounds, false)
}

fn compile_mode(world: &PlotWorld, bounds: (BlockPos, BlockPos), fixed_classic: bool) -> GraphBackend {
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    GraphBackend::compile_with_options(
        world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            budget_multiplier: 8,
            classic_redstone: fixed_classic,
            ..Default::default()
        },
        monitor,
    )
    .unwrap()
}

#[test]
fn fixed_classic_pipeline_publishes_and_executes_whole_program() {
    let world = analog_world();
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    let mut reference = compile(&world, bounds);
    let mut candidate = compile_mode(&world, bounds, true);
    let fixed = candidate.program.fixed_classic().expect("pass must publish its executable program");
    assert!(Arc::ptr_eq(fixed, candidate.fixed_classic.as_ref().unwrap()));
    assert_eq!(&**fixed, &FixedClassicProgram::compile(&reference.program).unwrap());
    let coverage = candidate.execution_coverage();
    assert_eq!(coverage.fixed_classic_wires, coverage.wire_nodes);
    assert_eq!(coverage.fixed_classic_nodes, coverage.component_nodes);
    assert_eq!(coverage.general_graph_nodes(), 0);
    for strength in [15, 3, 0, 8, 0] {
        for engine in [&mut reference, &mut candidate] {
            let node = engine.positions[&A];
            engine.set_node(node, strength > 0, strength, None, false).unwrap();
            engine.fire_route(node).unwrap();
        }
        for tick in 0..4 {
            assert_equivalent(&reference, &candidate, tick);
            reference.tick().unwrap();
            candidate.tick().unwrap();
        }
    }
}

pub(super) fn attach(reference: &GraphBackend) -> (GraphBackend, Duration) {
    let started = Instant::now();
    let program = Arc::new(FixedClassicProgram::compile(&reference.program).unwrap());
    let elapsed = started.elapsed();
    assert_eq!(program.wires.len(), reference.program.wires().len());
    assert_eq!(program.nodes.len(), reference.program.nodes().len());
    assert_eq!(program.routes.len(), reference.program.routes().len());
    let mut candidate = GraphBackend::new(reference.program.clone(), reference.state.clone(), reference.future.clock.logical_tick).unwrap();
    candidate.future.scheduler = reference.future.scheduler.clone();
    candidate.future.pending.clone_from(&reference.future.pending);
    candidate.future.pending_nodes.clone_from(&reference.future.pending_nodes);
    candidate.future.clock = reference.future.clock;
    candidate.fixed_classic = Some(program);
    (candidate, elapsed)
}

pub(super) fn assert_equivalent(reference: &GraphBackend, candidate: &GraphBackend, tick: usize) {
    if reference.state != candidate.state {
        if let Some(index) = reference
            .state
            .components
            .iter()
            .zip(&candidate.state.components)
            .position(|(a, b)| a != b)
        {
            panic!(
                "state at tick {tick}, node {index} {:?}: {:?} vs {:?}",
                reference.program.nodes()[index].position,
                reference.state.components[index],
                candidate.state.components[index]
            );
        }
        panic!("non-component state at tick {tick}");
    }
    let left = reference.scheduled_ticks();
    let right = candidate.scheduled_ticks();
    if left != right {
        let first = left.iter().zip(&right).position(|(a, b)| a != b).unwrap_or(left.len().min(right.len()));
        panic!(
            "future at tick {tick}, ordinal {first}: {:?} vs {:?}; lengths {} / {}",
            left.get(first),
            right.get(first),
            left.len(),
            right.len()
        );
    }
    assert!(reference.future.pending == candidate.future.pending, "pending map at tick {tick}");
    assert!(
        reference.future.pending_nodes == candidate.future.pending_nodes,
        "pending multiplicity at tick {tick}"
    );
    assert_eq!(reference.future.clock.logical_tick, candidate.future.clock.logical_tick);
    assert_eq!(reference.future.clock.phase, candidate.future.clock.phase);
    assert_eq!(reference.future.clock.scheduled_advanced, candidate.future.clock.scheduled_advanced);
    assert_eq!(candidate.counts.wire_walks, 0, "general dust fallback at tick {tick}");
    assert!(candidate.wire_walks.is_empty(), "general walk scratch allocated at tick {tick}");
}

#[test]
fn fixed_classic_multiple_analog_sources_main_side_and_reset() {
    let world = analog_world();
    let mut reference = compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)));
    let (mut candidate, _) = attach(&reference);
    let mut boundary = 0;
    for a in 0..16u8 {
        for b in [0, 15, 3, 12, 0] {
            for (position, strength) in [(A, a), (B, b)] {
                for engine in [&mut reference, &mut candidate] {
                    let node = engine.positions[&position];
                    engine.set_node(node, strength > 0, strength, None, false).unwrap();
                    engine.fire_route(node).unwrap();
                }
                assert_equivalent(&reference, &candidate, boundary);
                boundary += 1;
            }
            for _ in 0..3 {
                reference.tick().unwrap();
                candidate.tick().unwrap();
                assert_equivalent(&reference, &candidate, boundary);
                boundary += 1;
            }
        }
    }
    assert!(candidate.fixed_classic_counts.seed_calls > 0);
    assert!(candidate.fixed_classic_counts.wire_steps > 0);
    let mut left = analog_world();
    let mut right = analog_world();
    reference.reset(&mut left);
    candidate.reset(&mut right);
    for _ in 0..16 {
        left.tick_interpreted();
        right.tick_interpreted();
        for node in reference.program.nodes() {
            let position = node.position.unwrap();
            assert_eq!(left.get_block(position), right.get_block(position));
            assert_eq!(
                serde_json::to_value(left.get_block_entity(position)).unwrap(),
                serde_json::to_value(right.get_block_entity(position)).unwrap()
            );
        }
        assert_eq!(
            left.scheduler().iter_entries().collect::<Vec<_>>(),
            right.scheduler().iter_entries().collect::<Vec<_>>()
        );
    }
}

fn activate_native(world: &mut PlotWorld, position: BlockPos) {
    let Block::StoneButton { mut button } = world.get_block(position) else {
        panic!("start button changed")
    };
    assert!(!button.powered);
    button.powered = true;
    world.set_block(position, Block::StoneButton { button });
    world.schedule_tick(position, 10, TickPriority::Normal);
    crate::redstone::update_surrounding_blocks(world, position);
    let support = match button.face {
        ButtonFace::Floor => BlockFace::Bottom,
        ButtonFace::Ceiling => BlockFace::Top,
        ButtonFace::Wall => button.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(support));
}

fn assert_native(candidate: &GraphBackend, native: &PlotWorld, tick: usize) {
    for (index, spec) in candidate.program.nodes().iter().enumerate() {
        let node = NodeId::new(index as u32);
        let position = spec.position.unwrap();
        assert_eq!(
            candidate.node_block(node),
            native.get_block(position),
            "native block at tick {tick}, {position:?}"
        );
        if matches!(spec.kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })) {
            let actual = match native.get_block_entity(position) {
                Some(BlockEntity::Comparator { output_strength }) => *output_strength,
                _ => 0,
            };
            assert_eq!(
                candidate.node_state(node).output_strength,
                actual,
                "native comparator at tick {tick}, {position:?}"
            );
        }
    }
    let native_ticks = native.scheduler().iter_entries().collect::<Vec<_>>();
    assert!(candidate.scheduled_ticks() == native_ticks, "native future at tick {tick}");
}

#[test]
#[ignore = "research: full CHUNGUS lowering and paired execution with optional independent interpreter"]
fn fixed_classic_whole_chungus_compilation_and_execution() {
    let ticks = std::env::var("MCHPRS_CHUNGUS_TICKS")
        .ok()
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(1024);
    let interpreter = std::env::var("MCHPRS_FIXED_CLASSIC_ORACLE").unwrap_or_else(|_| "interpreter".into()) == "interpreter";
    let mut world = super::chungus_census::load_chungus_world();
    let started = Instant::now();
    let mut reference = compile(&world, world.get_corners());
    let general_compile_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let mut candidate = compile_mode(&world, world.get_corners(), true);
    let fixed_compile_seconds = started.elapsed().as_secs_f64();
    let program = candidate.fixed_classic.as_ref().unwrap();
    assert_eq!(program.nodes.len(), reference.program.nodes().len());
    assert_eq!(program.wires.len(), reference.program.wires().len());
    let table_bytes = std::mem::size_of_val(&*program.nodes) + std::mem::size_of_val(&*program.wires)
        + std::mem::size_of_val(&*program.sites) + std::mem::size_of_val(&*program.routes)
        + std::mem::size_of_val(&*program.ops) + std::mem::size_of_val(&*program.operands);
    let inventory = json!({"nodes":program.nodes.len(),"wires":program.wires.len(),"routes":program.routes.len(),"route_ops":program.ops.len(),"signal_operands":program.operands.len(),"capture_aliases":program.sites.len(),"executable_table_bytes":table_bytes,"wire_coverage":1.0,"general_dust_fallback":false});
    println!("FIXED CLASSIC LOWERED {inventory}, total compile seconds={fixed_compile_seconds}");
    let mut execution = [Duration::ZERO; 2];
    let mut activation = [Duration::ZERO; 2];
    let button = super::chungus_census::START_BUTTON;
    if interpreter {
        activate_native(&mut world, button);
    }
    for (index, engine) in [&mut reference, &mut candidate].into_iter().enumerate() {
        let started = Instant::now();
        engine.use_block(button).unwrap();
        activation[index] = started.elapsed();
    }
    for tick in 0..=ticks {
        if tick > 0 {
            if interpreter {
                world.tick_interpreted();
            }
            for index in if tick % 2 == 0 { [0, 1] } else { [1, 0] } {
                let started = Instant::now();
                if index == 0 {
                    reference.tick().unwrap();
                } else {
                    candidate.tick().unwrap();
                }
                execution[index] += started.elapsed();
            }
        }
        assert_equivalent(&reference, &candidate, tick);
        if interpreter {
            assert_native(&candidate, &world, tick);
        }
    }
    assert!(candidate.fixed_classic_counts.seed_calls > 0);
    assert!(candidate.fixed_classic_counts.wire_steps > 0);
    let report = json!({"experiment":"whole fixed classic circuit, exact ordered compiled propagation; not wholly queue-free weighted collapse", "ticks":ticks,"checked_boundaries":ticks+1,"independent_interpreter":interpreter,
        "profile":if cfg!(debug_assertions){"debug"}else{"release"},"inventory":inventory,
        "general_compile_seconds":general_compile_seconds,"fixed_compile_seconds":fixed_compile_seconds,
        "general_seconds":execution[0].as_secs_f64(),"fixed_seconds":execution[1].as_secs_f64(),
        "speed_ratio_general_over_fixed":execution[0].as_secs_f64()/execution[1].as_secs_f64(),
        "activation_seconds":[activation[0].as_secs_f64(),activation[1].as_secs_f64()],
        "general_counts":format!("{:?}",reference.execution_counts()),"fixed_counts":format!("{:?}",candidate.fixed_classic_counts),
        "fixed_general_counts":format!("{:?}",candidate.execution_counts()),
        "limits":"normal opt-in O2 pipeline; shared scheduler and component state; original Graph retained for oracle/projection; unit-test instrumentation enabled; ordered dust frontiers retained"});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/chungus-whole-fixed-classic-{ticks}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("FIXED CLASSIC RESULT {report}");
}
