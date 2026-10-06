use super::super::BackendDispatcher;
use super::*;
use crate::redpiler::{CompileError, Compiler};
use crate::redstone::piston::trace::{self, Operation};
use crate::world::storage::Chunk;
use mchprs_blocks::BlockFacing;

mod bud_reference;
#[path = "../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

fn empty() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

fn compile(world: &PlotWorld) -> Compiler {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            world,
            world.get_corners(),
            CompilerOptions::parse("--piston-events --io-only"),
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    compiler
}

fn toggle(world: &mut PlotWorld, pos: BlockPos) {
    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("missing lever at {pos:?}")
    };
    lever.powered = !lever.powered;
    world.set_block(pos, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, pos);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, pos.offset(support));
}

#[test]
fn independent_bud_data_sampling_and_resampling_survive_event_execution() {
    let name = "instant-pistons-io/BUD_NonInstantInputs.schem";
    let mut interpreted = cpus::load(name);
    let mut displayed = cpus::load(name);
    let mut compiler = compile(&displayed);
    let origin = BlockPos::new(8, 8, 8);
    let data = origin + BlockPos::new(0, 7, 0);
    let update = origin + BlockPos::new(1, 4, 0);
    let memory = origin + BlockPos::new(0, 3, 3);
    let mut accepted = 0;
    for tick in 0..160 {
        // Prepare one, sample, hold another data value, then resample zero.
        if let Some(pos) = match tick {
            0 | 72 => Some(data),
            32 | 64 | 104 => Some(update),
            _ => None,
        } {
            assert_eq!(
                trace::capture(|| toggle(&mut interpreted, pos)),
                trace::capture(|| compiler.on_use_block(pos)),
                "input at tick {tick}"
            );
        }
        let expected = trace::capture(|| interpreted.tick_interpreted());
        let actual = trace::capture(|| compiler.tick());
        accepted += expected
            .iter()
            .filter(|e| matches!(e.operation, Operation::Applied(_)))
            .count();
        assert_eq!(actual, expected, "BUD operation order at tick {tick}");
        compiler.flush(&mut displayed);
        for local in [
            BlockPos::new(0, 1, 4),
            BlockPos::new(0, 7, 0),
            BlockPos::new(1, 4, 0),
        ] {
            // The ordinary repeater is internal presentation in this mode;
            // compare its authoritative state inside the private execution.
            let BackendDispatcher::EventBackend(backend) = compiler.jit.as_ref().unwrap() else {
                unreachable!()
            };
            assert_eq!(
                backend.world.get_block(origin + local),
                interpreted.get_block(origin + local)
            );
        }
        if tick == 24 {
            assert!(
                matches!(interpreted.get_block(memory), Block::Piston { piston } if piston.extended),
                "data alone must not write BUD storage"
            );
        }
        if tick == 60 || tick == 96 {
            assert!(
                matches!(interpreted.get_block(memory), Block::Piston { piston } if !piston.extended),
                "the stored one must hold through data changes"
            );
        }
    }
    assert!(accepted > 0);
    compiler.reset(&mut displayed, interpreted.get_corners());
    assert_eq!(
        cpus::checkpoint(&displayed, 160, &[]),
        cpus::checkpoint(&interpreted, 160, &[])
    );
}

#[test]
fn event_handoff_preserves_motion_and_expected_tick_types() {
    for elapsed in 0..8 {
        let mut interpreted = cpus::load("instant-pistons-io/BUD_PistonUpdate.schem");
        let data = BlockPos::new(8, 15, 9);
        let update = BlockPos::new(9, 11, 8);
        toggle(&mut interpreted, data);
        for _ in 0..32 {
            interpreted.tick_interpreted();
        }
        toggle(&mut interpreted, update);
        for _ in 0..elapsed {
            interpreted.tick_interpreted();
        }
        let mut compiler = compile(&interpreted);
        for _ in 0..3 {
            compiler.tick();
        }
        let mut expected = EventBackend::prepare(
            &interpreted,
            interpreted.get_corners(),
            interpreted.scheduler().iter_entries().collect(),
            &CompilerOptions::parse("--piston-events"),
            &TaskMonitor::default(),
        )
        .unwrap()
        .world;
        for _ in 0..3 {
            expected.tick_interpreted();
        }
        compiler.reset(&mut interpreted, expected.get_corners());
        assert_eq!(
            cpus::checkpoint(&interpreted, 0, &[]),
            cpus::checkpoint(&expected, 0, &[]),
            "handoff after {elapsed} ticks"
        );
        for _ in 0..20 {
            interpreted.tick_interpreted();
            expected.tick_interpreted();
        }
        assert_eq!(
            cpus::checkpoint(&interpreted, 20, &[]),
            cpus::checkpoint(&expected, 20, &[])
        );
    }
}

#[test]
fn event_admission_rejects_partial_plots_commands_and_incompatible_flags_transactionally() {
    let mut world = empty();
    world.set_block(
        BlockPos::new(20, 20, 20),
        Block::from_name("command_block").unwrap(),
    );
    world.schedule_tick(BlockPos::new(20, 20, 20), 2, TickPriority::Normal);
    let before = cpus::checkpoint(&world, 0, &[]);
    let mut compiler = Compiler::default();
    for (bounds, flags) in [
        (world.get_corners(), "--piston-events"),
        (
            (BlockPos::new(10, 10, 10), BlockPos::new(30, 30, 30)),
            "--piston-events",
        ),
        (world.get_corners(), "--piston-events --optimize"),
        (world.get_corners(), "--piston-events --export"),
    ] {
        assert!(matches!(
            compiler.compile(
                &world,
                bounds,
                CompilerOptions::parse(flags),
                world.scheduler().iter_entries().collect(),
                Default::default()
            ),
            Err(CompileError::PistonEvents(_))
        ));
        assert!(!compiler.is_active());
        assert_eq!(before, cpus::checkpoint(&world, 0, &[]));
    }
}

fn plate(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    let block = world
        .get_block(pos)
        .with_pressure_plate_power(powered)
        .unwrap();
    world.set_block(pos, block);
    redstone::update_surrounding_blocks(world, pos);
    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
}

#[test]
fn anpu_warm_entry_player_inputs_and_mid_run_handoffs_preserve_writes_and_screen() {
    let cpu = cpus::CPUS[1];
    let mut interpreted = cpus::load_cpu(cpu);
    let mut displayed = cpus::load_cpu(cpu);
    cpus::click(&mut interpreted, cpu.start);
    cpus::click(&mut displayed, cpu.start);
    for _ in 0..125 {
        interpreted.tick_interpreted();
        displayed.tick_interpreted();
    }
    let mut compiler = compile(&displayed);
    let left = BlockPos::new(121, 68, 53) + BlockPos::new(8, 8, 8);
    let right = BlockPos::new(123, 68, 53) + BlockPos::new(8, 8, 8);
    let mut active_handoffs = 0;
    for tick in 126..=800 {
        if let Some((pos, powered)) = match tick {
            126 => Some((left, true)),
            140 => Some((left, false)),
            200 | 201 => Some((right, true)), // Repeat a same-strength notification.
            250 => Some((right, false)),
            390 => Some((left, true)),
            450 => Some((left, false)),
            _ => None,
        } {
            assert_eq!(
                trace::capture(|| plate(&mut interpreted, pos, powered)),
                trace::capture(|| compiler.set_pressure_plate(pos, powered)),
                "paddle input at {tick}"
            );
        }
        assert_eq!(
            trace::capture(|| interpreted.tick_interpreted()),
            trace::capture(|| compiler.tick()),
            "warm ANPU storage operations at {tick}"
        );
        compiler.flush(&mut displayed);
        assert_eq!(
            cpus::screen(&displayed),
            cpus::screen(&interpreted),
            "warm ANPU screen at {tick}"
        );
        if [225, 350, 512, 800].contains(&tick) {
            active_handoffs += usize::from(!interpreted.piston_state().motions.is_empty());
            compiler.reset(&mut displayed, interpreted.get_corners());
            assert!(
                compiler.jit.is_none(),
                "reset must release its private plot"
            );
            assert_eq!(
                cpus::checkpoint(&displayed, tick, &[]),
                cpus::checkpoint(&interpreted, tick, &[]),
                "complete ANPU handoff at {tick}"
            );
            compiler = compile(&displayed);
        }
    }
    assert!(
        active_handoffs > 0,
        "handoff coverage must include live piston motions"
    );
}

#[test]
#[ignore = "ANPU 50,000-tick paired memory/screen replay; run with --release -- --ignored"]
fn anpu_event_execution_matches_ordered_writes_and_frozen_screen() {
    let cpu = cpus::CPUS[1];
    let reference = cpus::reference(cpu);
    let frozen_screen: Vec<cpus::ScreenFrame> = serde_json::from_str(include_str!(
        "../../../../../../test_data/cpu-references/anpu_screen.json"
    ))
    .unwrap();
    let mut interpreted = cpus::load_cpu(cpu);
    let mut displayed = cpus::load_cpu(cpu);
    let mut compiler = compile(&displayed);
    let mut memory = FxHashSet::default();
    for_each_block_optimized(
        &interpreted,
        interpreted.get_corners().0,
        interpreted.get_corners().1,
        |pos| {
            if matches!(interpreted.get_block(pos), Block::Piston { piston } if piston.sticky && piston.facing == BlockFacing::Down)
                && matches!(
                    interpreted.get_block(pos.offset(BlockFace::Top)),
                    Block::NoteBlock { .. }
                )
            {
                memory.insert(pos);
            }
        },
    );
    assert_eq!(memory.len(), 896, "ANPU note-block BUD bank inventory");
    let frozen_bud = bud_reference::read(&memory);
    let mut memory_trace = Vec::new();
    let mut frames = Vec::new();
    cpus::collect_screen(&displayed, 0, &mut frames);
    let start = trace::capture(|| cpus::click(&mut interpreted, cpu.start));
    assert_eq!(
        start,
        trace::capture(|| compiler.on_use_block(cpu.start + BlockPos::new(8, 8, 8)))
    );
    if let Some(entry) = bud_reference::sample(0, &start, &memory) {
        memory_trace.push(entry);
    }
    let mut samples = 0;
    let mut writes = 0;
    let mut bud_writes = 0;
    for tick in 1..=50_000 {
        let expected = trace::capture(|| interpreted.tick_interpreted());
        let actual = trace::capture(|| compiler.tick());
        for entry in &expected {
            match entry.operation {
                Operation::Sample { .. } => samples += 1,
                Operation::Applied(event) => {
                    writes += 1;
                    bud_writes += usize::from(memory.contains(&event.pos));
                }
            }
        }
        assert_eq!(
            actual, expected,
            "ANPU ordered storage samples/movements at tick {tick}"
        );
        if let Some(entry) = bud_reference::sample(tick, &actual, &memory) {
            memory_trace.push(entry);
        }
        compiler.flush(&mut displayed);
        cpus::collect_screen(&displayed, tick, &mut frames);
        if let Some(checkpoint) = reference.checkpoints.iter().find(|c| c.tick == tick) {
            assert_eq!(
                cpus::checkpoint(&interpreted, tick, &[]),
                *checkpoint,
                "ANPU interpreter baseline at {tick}"
            );
            let super::super::BackendDispatcher::EventBackend(backend) =
                compiler.jit.as_ref().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(
                cpus::checkpoint(&backend.world, tick, &[]),
                *checkpoint,
                "ANPU private execution baseline at {tick}"
            );
        }
    }
    assert_eq!(
        frames, frozen_screen,
        "exact tick-stamped ANPU screen frames"
    );
    assert_eq!(
        memory_trace, frozen_bud.updates,
        "frozen ANPU BUD sampling/write pattern"
    );
    assert!(
        samples > writes && bud_writes > 0,
        "the CPU must perform storage operations"
    );
    println!("ANPU matched {samples} samples, {writes} accepted piston operations ({bud_writes} in the 896-cell note-block BUD bank) and {} screen frames through 50,000 game ticks", frames.len());
    compiler.reset(&mut displayed, interpreted.get_corners());
    assert_eq!(
        cpus::checkpoint(&displayed, 50_000, &[]),
        cpus::checkpoint(&interpreted, 50_000, &[]),
        "ANPU complete physical handoff"
    );
}
