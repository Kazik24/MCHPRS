use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::compile_graph::{
    CompileGraph, CompileLink, CompileNode, LinkType, NodeState, NodeType,
};
use crate::redpiler::{CompileError, Compiler, CompilerOptions};
use crate::redstone;
use crate::world::{storage::Chunk, World};
use mchprs_blocks::blocks::{
    Block, ComparatorMode, Instrument, Lever, LeverFace, RedstoneComparator, RedstoneWire,
    StoneButton,
};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

fn start(world: &PlotWorld, optimize: bool, ticks: Vec<TickEntry>) -> Compiler {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            CompilerOptions {
                optimize,
                ..Default::default()
            },
            ticks,
            Default::default(),
        )
        .unwrap();
    compiler
}

fn advance(compiler: &mut Compiler, world: &mut PlotWorld, ticks: usize) {
    for _ in 0..ticks {
        compiler.tick_with_world(world);
    }
    compiler.flush(world);
}

#[test]
fn attenuated_constant_side_preserves_comparator_lamp_output() {
    let run = |optimize| {
        let mut world = world();
        let comparator = BlockPos::new(8, 30, 8);
        let control = BlockPos::new(9, 30, 8);
        let lamp = BlockPos::new(7, 30, 8);
        world.set_block(
            comparator,
            Block::RedstoneComparator {
                comparator: RedstoneComparator {
                    mode: ComparatorMode::Subtract,
                    facing: BlockDirection::East,
                    powered: false,
                },
            },
        );
        world.set_block(
            control,
            Block::Lever {
                lever: Lever::default(),
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        for z in 5..=7 {
            let pos = BlockPos::new(8, 30, z);
            world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                pos,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
        }
        world.set_block(BlockPos::new(8, 30, 4), Block::RedstoneBlock);
        for z in 5..=7 {
            let pos = BlockPos::new(8, 30, z);
            let wire = redstone::wire::get_state_for_placement(&world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        assert!(matches!(
            world.get_block(BlockPos::new(8, 30, 7)),
            Block::RedstoneWire { wire } if wire.power == 13
        ));
        let mut compiler = start(&world, optimize, vec![]);
        compiler.on_use_block(control);
        advance(&mut compiler, &mut world, 2);
        world.get_block(lamp)
    };
    let unoptimized = run(false);
    assert_eq!(unoptimized, Block::RedstoneLamp { lit: true });
    assert_eq!(run(true), unoptimized);
}

#[test]
fn repeated_lamp_off_requests_preserve_the_next_off_deadline() {
    let run = |compiled: bool| {
        let mut world = world();
        let control = BlockPos::new(8, 30, 8);
        let lamp = BlockPos::new(7, 30, 8);
        world.set_block(
            control,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: true });
        let mut compiler = compiled.then(|| start(&world, false, vec![]));
        let toggle = |world: &mut PlotWorld, compiler: &mut Option<Compiler>| {
            if let Some(compiler) = compiler {
                compiler.on_use_block(control);
            } else {
                let Block::Lever { mut lever } = world.get_block(control) else {
                    unreachable!()
                };
                lever.powered = !lever.powered;
                world.set_block(control, Block::Lever { lever });
                redstone::update_surrounding_blocks(world, control);
                redstone::update_surrounding_blocks(world, control.offset(BlockFace::Bottom));
            }
        };
        let tick = |world: &mut PlotWorld, compiler: &mut Option<Compiler>| {
            if let Some(compiler) = compiler {
                advance(compiler, world, 1);
            } else {
                world.tick_interpreted();
            }
        };
        toggle(&mut world, &mut compiler); // Off at t=0, deadline t=4.
        tick(&mut world, &mut compiler);
        toggle(&mut world, &mut compiler); // On at t=1.
        tick(&mut world, &mut compiler);
        toggle(&mut world, &mut compiler); // Off at t=2; keep the existing deadline.
        for _ in 0..3 {
            tick(&mut world, &mut compiler);
        }
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
        toggle(&mut world, &mut compiler); // On at t=5.
        toggle(&mut world, &mut compiler); // Off at t=5, new deadline t=9.
        let mut trace = Vec::new();
        for _ in 0..4 {
            tick(&mut world, &mut compiler);
            trace.push(world.get_block(lamp));
        }
        trace
    };
    let native = run(false);
    assert_eq!(
        native,
        vec![
            Block::RedstoneLamp { lit: true },
            Block::RedstoneLamp { lit: true },
            Block::RedstoneLamp { lit: true },
            Block::RedstoneLamp { lit: false },
        ]
    );
    assert_eq!(run(true), native);
}

#[test]
fn already_due_imported_tick_runs_on_the_first_compiled_step() {
    let button = BlockPos::new(8, 30, 8);
    let run = |compiled: bool| {
        let mut world = world();
        world.set_block(
            button,
            Block::StoneButton {
                button: StoneButton {
                    powered: true,
                    ..Default::default()
                },
            },
        );
        world.schedule_half_tick(button, 0, TickPriority::NanoTick);
        if compiled {
            let ticks = world.scheduler().iter_entries().collect();
            let mut compiler = start(&world, false, ticks);
            world.clear_scheduled_ticks();
            advance(&mut compiler, &mut world, 1);
        } else {
            world.tick_interpreted();
        }
        world.get_block(button)
    };
    let native = run(false);
    assert!(matches!(native, Block::StoneButton { button } if !button.powered));
    assert_eq!(run(true), native);
}

#[test]
fn compiling_more_than_u16_note_blocks_returns_without_panicking() {
    let mut world = world();
    for i in 0..=65536 {
        world.set_block(
            BlockPos::new(i % 32, 1 + i / 1024, (i / 32) % 32),
            Block::NoteBlock {
                instrument: Instrument::Harp,
                note: 0,
                powered: false,
            },
        );
    }
    let result = Compiler::default().compile(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(31, 65, 31)),
        Default::default(),
        vec![],
        Default::default(),
    );
    // Widening the ID or explicitly rejecting its capacity both prevent the panic.
    assert!(
        matches!(&result, Ok(()) | Err(CompileError::Backend(_))),
        "unexpected compile failure: {result:?}"
    );
}

#[test]
fn graph_coalescing_preserves_input_channel_and_attenuation() {
    // These are graph counterexamples; classic physical-layout reachability is unverified.
    for (source_type, channel, attenuation) in [
        (
            NodeType::Repeater {
                delay: 1,
                facing_diode: false,
            },
            LinkType::Side,
            0,
        ),
        (NodeType::InstantOutput { port: 0 }, LinkType::Default, 1),
    ] {
        let mut graph = CompileGraph::new();
        let node = |ty, is_input| CompileNode {
            ty,
            block: None,
            state: NodeState::default(),
            is_input,
            is_output: false,
        };
        let source = graph.add_node(node(source_type, true));
        let repeater = NodeType::Repeater {
            delay: 1,
            facing_diode: false,
        };
        let first = graph.add_node(node(repeater.clone(), false));
        let second = graph.add_node(node(repeater, false));
        graph.add_edge(source, first, CompileLink::new(LinkType::Default, 0));
        graph.add_edge(source, second, CompileLink::new(channel, attenuation));
        super::coalesce::run(&mut graph).unwrap();
        assert!(
            graph.contains_node(first) && graph.contains_node(second),
            "distinct {channel:?} input with attenuation {attenuation} was merged"
        );
    }
}
