//! Exact read refinement: held emitters, live analog/conductor reads and contacts.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::world::storage::Chunk;
use mchprs_blocks::{
    BlockDirection, BlockFacing,
    blocks::{Lever, LeverFace, RedstoneComparator},
};
use std::time::Instant;

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

const ROOT: BlockPos = BlockPos::new(8, 20, 8);
const LEVER: BlockPos = BlockPos::new(7, 20, 8);
const COMPARATOR: BlockPos = BlockPos::new(8, 20, 9);
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));

fn fixture() -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for pos in [ROOT, ROOT + BlockPos::new(1, 0, 0)] {
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(
        LEVER,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        COMPARATOR,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false),
        },
    );
    world.set_block_entity(COMPARATOR, BlockEntity::Comparator { output_strength: 0 });
    for pos in [ROOT, ROOT + BlockPos::new(1, 0, 0), LEVER, COMPARATOR] {
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    world
}

fn compile(world: &PlotWorld, bounds: (BlockPos, BlockPos), mixed: bool) -> GraphBackend {
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline.entries.retain(|entry| {
        !matches!(&entry.pass, passes::Pass::Build(builder)
        if builder.name() == "compile-fixed-classic" || !mixed && builder.name() == "compile-mixed-classic")
    });
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    GraphBackend::compile_with_pipeline(world, bounds, world.scheduler().iter_entries().collect(), &options, &pipeline, monitor).unwrap()
}

fn same(left: &GraphBackend, right: &GraphBackend) {
    assert_eq!(left.state, right.state);
    assert!(
        left.future.scheduler.iter().eq(right.future.scheduler.iter()),
        "typed deadline/priority/FIFO"
    );
    assert_eq!(left.future.pending, right.future.pending);
    assert_eq!(left.future.pending_nodes, right.future.pending_nodes);
    assert_eq!(
        (
            left.future.clock.logical_tick,
            left.future.clock.phase,
            left.future.clock.scheduled_advanced
        ),
        (
            right.future.clock.logical_tick,
            right.future.clock.phase,
            right.future.clock.scheduled_advanced
        )
    );
    let requests = |b: &GraphBackend| b.future.piston_requests.iter().map(|r| (r.piston, r.action)).collect::<Vec<_>>();
    assert_eq!(requests(left), requests(right));
    let deferred = |b: &GraphBackend| {
        b.future
            .deferred_pistons
            .iter()
            .map(|e| {
                (
                    e.piston,
                    e.revision,
                    e.resource.map(|r| (r.resource, r.revision, r.contact)),
                    e.due_tick,
                    e.extending,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(deferred(left), deferred(right));
    assert_eq!(left.execution_counts(), right.execution_counts());
}

fn set(backend: &mut GraphBackend, pos: BlockPos, strength: u8) {
    let node = backend.positions[&pos];
    let ComponentState::Electrical(state) = &mut backend.state.components[node.index()] else {
        panic!("electrical input")
    };
    state.output_strength = strength;
    state.powered = strength != 0;
}

#[test]
fn mixed_classic_keeps_held_binary_emitters_and_live_analog_reads_distinct() {
    let mut world = fixture();
    world.set_block(LEVER, Block::RedstoneTorch { lit: false });
    let mut control = compile(&world, BOUNDS, false);
    let mut candidate = compile(&world, BOUNDS, true);
    assert!(!candidate.program.mixed_classic().wires.is_empty());
    assert!(
        candidate
            .program
            .mixed_classic()
            .operands
            .iter()
            .any(|o| matches!(o.source, FixedClassicSource::CapturedPowered { .. }))
    );
    assert!(
        candidate
            .program
            .mixed_classic()
            .operands
            .iter()
            .any(|o| matches!(o.source, FixedClassicSource::LiveOutput(_)))
    );
    for old in 0..=15 {
        for new in 0..=15 {
            let mut results = Vec::new();
            for backend in [&mut control, &mut candidate] {
                set(backend, LEVER, old);
                set(backend, COMPARATOR, old);
                let plan = backend.wire_plan(backend.positions[&ROOT]).clone();
                let mut walk = WireWalk::new();
                let neighbors = backend.identify_wire_neighbors(plan.node, &mut walk);
                set(backend, LEVER, new);
                set(backend, COMPARATOR, new);
                results.push((backend.seed_power(&plan), backend.turbo_wire_power(&plan, &neighbors, &walk)));
            }
            assert_eq!(results[0], results[1], "held {old}, live {new}");
            assert_eq!(results[1].1.block_power, if old > 0 { 15 } else { new });
        }
    }
}

#[test]
fn mixed_classic_keeps_control_flags_live_when_a_walk_holds_old_properties() {
    let world = fixture();
    let mut control = compile(&world, BOUNDS, false);
    let mut candidate = compile(&world, BOUNDS, true);
    for old in [0, 15] {
        for new in [0, 15] {
            let mut results = Vec::new();
            for backend in [&mut control, &mut candidate] {
                set(backend, LEVER, old);
                let plan = backend.wire_plan(backend.positions[&ROOT]).clone();
                let mut walk = WireWalk::new();
                let neighbors = backend.identify_wire_neighbors(plan.node, &mut walk);
                set(backend, LEVER, new);
                results.push(backend.turbo_wire_power(&plan, &neighbors, &walk));
            }
            assert_eq!(results[0], results[1]);
            assert_eq!(results[1].block_power, new);
        }
    }
}

#[test]
fn mixed_classic_preserves_contact_selection_and_captured_payloads() {
    let mut world = fixture();
    let base = BlockPos::new(10, 20, 6);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                extended: false,
                sticky: true,
            },
        },
    );
    world.set_block(base + BlockPos::new(0, 0, 1), Block::RedstoneBlock);
    world.set_block(base + BlockPos::new(0, 0, 2), Block::Air);
    let root = base + BlockPos::new(1, 0, 1);
    world.set_block(root, Block::RedstoneWire { wire: Default::default() });
    world.set_block(root.offset(BlockFace::Bottom), Block::Stone {});
    let mut control = compile(&world, BOUNDS, false);
    let mut candidate = compile(&world, BOUNDS, true);
    assert!(candidate.program.mixed_classic().wires.iter().any(|w| w.general_faces != 0));
    for enabled in [false, true] {
        for far in [false, true] {
            let mut results = Vec::new();
            for backend in [&mut control, &mut candidate] {
                let actor = backend.program.pistons().iter().position(|p| p.position == base).unwrap();
                let resource = backend.program.resource_for_piston(PistonId::new(actor as u32)).unwrap();
                let spec = backend.program.pistons()[actor];
                backend.state.resources[resource.index()].at = if far { spec.far } else { spec.near };
                backend.state.resources[resource.index()].enabled = enabled;
                let plan = backend.wire_plan(backend.positions[&root]).clone();
                let mut walk = WireWalk::new();
                let neighbors = backend.identify_wire_neighbors(plan.node, &mut walk);
                backend.state.resources[resource.index()].enabled = !enabled;
                results.push((backend.seed_power(&plan), backend.turbo_wire_power(&plan, &neighbors, &walk)));
            }
            assert_eq!(results[0], results[1]);
        }
    }
}

#[test]
fn mixed_classic_rejects_stale_queries_and_honors_cancellation() {
    let world = fixture();
    let backend = compile(&world, BOUNDS, true);
    assert!(!backend.program.mixed_classic().wires.is_empty());
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    prepared.graph.plans.fixed_signals = infer_fixed_signals(&prepared.graph, &monitor).unwrap();
    let plan = MixedClassicProgram::compile(&prepared.graph, &monitor).unwrap();
    plan.validate(&prepared.graph, &monitor).unwrap();
    let mut bad = plan.clone();
    bad.wires[0].general_faces ^= 1;
    assert!(bad.validate(&prepared.graph, &monitor).is_err());
    let mut bad = plan;
    bad.wires[0].fixed.constant ^= 1;
    assert!(bad.validate(&prepared.graph, &monitor).is_err());
    monitor.cancel();
    assert!(MixedClassicProgram::compile(&prepared.graph, &monitor).is_err());
}

#[test]
fn mixed_classic_keeps_ordered_propagation_and_pending_future() {
    let world = fixture();
    let mut control = compile(&world, BOUNDS, false);
    let mut candidate = compile(&world, BOUNDS, true);
    control.set_test_recording(true);
    candidate.set_test_recording(true);
    for _ in 0..16 {
        control.use_block(LEVER).unwrap();
        candidate.use_block(LEVER).unwrap();
        for _ in 0..4 {
            same(&control, &candidate);
            assert_eq!(control.trace(), candidate.trace());
            assert_eq!(control.observed_operations, candidate.observed_operations);
            control.tick().unwrap();
            candidate.tick().unwrap();
        }
    }
}

#[test]
#[ignore = "research: active ANPU mixed-classic full state/future, independent native state and reset continuation"]
fn mixed_classic_anpu_differential() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let mut native = cpus::load_cpu(cpu);
    native.disable_command_output_limits_for_replay();
    let mut projected = cpus::load_cpu(cpu);
    projected.disable_command_output_limits_for_replay();
    let mut control_world = cpus::load_cpu(cpu);
    let mut control = compile(&native, native.get_corners(), false);
    let mut candidate = compile(&native, native.get_corners(), true);
    let wires = candidate.program.mixed_classic().wires.len();
    let fixed_faces: usize = candidate
        .program
        .mixed_classic()
        .wires
        .iter()
        .map(|w| 6 - w.general_faces.count_ones() as usize)
        .sum();
    assert!(wires > 0 && fixed_faces > 0);
    let positions: Vec<_> = candidate.program.nodes().iter().filter_map(|n| n.position).collect();
    let comparators: Vec<_> = candidate
        .program
        .nodes()
        .iter()
        .filter(|n| matches!(n.kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })))
        .filter_map(|n| n.position)
        .collect();
    let ticks = 1024;
    cpus::click(&mut native, cpu.start);
    control.use_block(cpu.origin + cpu.start).unwrap();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    for tick in 0..=ticks {
        if tick > 0 {
            native.tick_interpreted();
            control.tick_with_world(&mut control_world).unwrap();
            candidate.tick_with_world(&mut projected).unwrap();
        }
        same(&control, &candidate);
        projected.clear_scheduled_ticks();
        candidate.project_future(&mut projected);
        for &pos in &positions {
            assert_eq!(native.get_block(pos), projected.get_block(pos), "native block {pos:?} tick {tick}");
        }
        let strength = |world: &PlotWorld, pos| match world.get_block_entity(pos) {
            Some(BlockEntity::Comparator { output_strength }) => *output_strength,
            _ => panic!("comparator entity"),
        };
        for &pos in &comparators {
            assert_eq!(strength(&native, pos), strength(&projected, pos), "native comparator {pos:?} tick {tick}");
        }
        assert_eq!(
            native.scheduler().iter_entries().collect::<Vec<_>>(),
            projected.scheduler().iter_entries().collect::<Vec<_>>(),
            "native scheduled future tick {tick}"
        );
        assert_eq!(
            native.command_output().collect::<Vec<_>>(),
            projected.command_output().collect::<Vec<_>>(),
            "native ordered output tick {tick}"
        );
        assert_eq!(cpus::screen(&native), cpus::screen(&projected), "screen tick {tick}");
    }
    projected.clear_scheduled_ticks();
    candidate.reset(&mut projected);
    let checkpoint = |world: &PlotWorld, tick| {
        let mut value = cpus::checkpoint(world, tick, &[]);
        // Motion identity allocation is interpreter-private; blocks, entities,
        // scheduler and continuation are checked independently of that encoding.
        value.pistons.clear();
        value
    };
    assert_eq!(checkpoint(&native, ticks), checkpoint(&projected, ticks));
    for _ in 0..32 {
        native.tick_interpreted();
        projected.tick_interpreted();
    }
    assert_eq!(checkpoint(&native, ticks + 32), checkpoint(&projected, ticks + 32));
    let report = serde_json::json!({"cpu":cpu.name,"ticks":ticks,"matched_boundaries":ticks+1,"native_continuation_ticks":32,"wires":wires,"compiled_faces":fixed_faces,"all_faces":wires*6,"counts":format!("{:?}",candidate.execution_counts())});
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mixed-classic-anpu-differential.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}

#[test]
#[ignore = "research: alternating optimized-test ANPU execution samples; run a frozen executable with no competing builds"]
fn mixed_classic_anpu_timing() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let mut samples = Vec::new();
    for pair in 0..3 {
        let mut endpoints = Vec::new();
        for mixed in if pair % 2 == 0 { [false, true] } else { [true, false] } {
            let mut world = cpus::load_cpu(cpu);
            let started = Instant::now();
            let mut backend = compile(&world, world.get_corners(), mixed);
            let compile_seconds = started.elapsed().as_secs_f64();
            let started = Instant::now();
            backend.use_block(cpu.origin + cpu.start).unwrap();
            for _ in 0..cpu.active_ticks {
                backend.tick_with_world(&mut world).unwrap();
            }
            let seconds = started.elapsed().as_secs_f64();
            let report = serde_json::json!({"pair":pair,"mixed":mixed,"ticks":cpu.active_ticks,"seconds":seconds,
                "compile_seconds":compile_seconds,"counts":format!("{:?}",backend.execution_counts())});
            println!("{report}");
            samples.push(report);
            endpoints.push((backend, world));
        }
        same(&endpoints[0].0, &endpoints[1].0);
        let mut checkpoints = Vec::new();
        for (mut backend, mut world) in endpoints {
            world.clear_scheduled_ticks();
            backend.reset(&mut world);
            let reset = cpus::checkpoint(&world, cpu.active_ticks, &[]);
            for _ in 0..32 {
                world.tick_interpreted();
            }
            checkpoints.push((
                reset,
                cpus::checkpoint(&world, cpu.active_ticks + 32, &[]),
                world.command_output().map(str::to_owned).collect::<Vec<_>>(),
            ));
        }
        assert_eq!(checkpoints[0], checkpoints[1], "reset and native continuation");
    }
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mixed-classic-anpu-timing.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"profile":if cfg!(debug_assertions) {"dev optimized test"} else {"release test"},"samples":samples}),
        )
        .unwrap(),
    )
    .unwrap();
}
