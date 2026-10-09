use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::compile_graph::{
    CompileGraph, CompileLink, CompileNode, LinkType, NodeState, NodeType,
};
use crate::redpiler::{Compiler, CompilerOptions};
use crate::redstone;
use crate::world::{storage::Chunk, World};
use mchprs_blocks::blocks::{
    Block, ComparatorMode, Instrument, Lever, LeverFace, RedstoneComparator, RedstoneRepeater,
    RedstoneWire, StoneButton,
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
fn coalescing_retains_transitive_physical_aliases() {
    let mut graph = CompileGraph::new();
    let source = graph.add_node(CompileNode {
        native: false,
        ty: NodeType::Lever,
        block: None,
        block_aliases: Vec::new(),
        state: NodeState::default(),
        is_input: true,
        is_output: false,
    });
    let positions = [
        BlockPos::new(1, 30, 1),
        BlockPos::new(2, 30, 1),
        BlockPos::new(3, 30, 1),
    ];
    let alias = BlockPos::new(4, 30, 1);
    for (i, pos) in positions.into_iter().enumerate() {
        let id = graph.add_node(CompileNode {
            native: false,
            ty: NodeType::Repeater {
                delay: 2,
                facing_diode: false,
            },
            block: Some((pos, 0)),
            block_aliases: if i == 0 { vec![(alias, 0)] } else { Vec::new() },
            state: NodeState::default(),
            is_input: false,
            is_output: false,
        });
        graph.add_edge(source, id, CompileLink::new(LinkType::Default, 0));
    }
    super::coalesce::run(&mut graph).unwrap();
    assert_eq!(graph.node_count(), 2);
    let repeater = graph.node_weights().find(|node| !node.is_input).unwrap();
    let mut restored = repeater
        .block
        .into_iter()
        .chain(repeater.block_aliases.iter().copied())
        .map(|(pos, _)| pos)
        .collect::<Vec<_>>();
    restored.sort_by_key(|pos| pos.x);
    assert_eq!(
        restored,
        positions.into_iter().chain([alias]).collect::<Vec<_>>()
    );
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
fn comparator_pruning_preserves_a_larger_saved_output() {
    let mut graph = CompileGraph::new();
    let node = |ty, strength| CompileNode {
        native: false,
        ty,
        block: None,
        block_aliases: Vec::new(),
        state: NodeState::comparator(strength > 0, strength),
        is_input: false,
        is_output: true,
    };
    let side = graph.add_node(node(NodeType::Constant, 15));
    let comparator = graph.add_node(node(
        NodeType::Comparator {
            mode: ComparatorMode::Subtract,
            far_input: None,
            facing_diode: false,
        },
        5,
    ));
    let lamp = graph.add_node(node(NodeType::Lamp, 0));
    graph.add_edge(side, comparator, CompileLink::new(LinkType::Side, 0));
    let live = graph.add_edge(comparator, lamp, CompileLink::new(LinkType::Default, 4));
    let dead = graph.add_edge(comparator, lamp, CompileLink::new(LinkType::Default, 5));
    super::unreachable_output::run(&mut graph).unwrap();
    assert!(
        graph.edge_weight(live).is_some(),
        "saved strength 5 still supplies 1"
    );
    assert!(
        graph.edge_weight(dead).is_none(),
        "attenuation 5 cannot carry power"
    );
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
fn already_due_callback_preserves_subsequent_repeater_deadline() {
    let button = BlockPos::new(8, 30, 8);
    let repeater = BlockPos::new(7, 30, 8);
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
        world.set_block(
            repeater,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 1,
                    facing: BlockDirection::East,
                    powered: true,
                    locked: false,
                },
            },
        );
        world.schedule_half_tick(button, 0, TickPriority::NanoTick);
        let ticks = world.scheduler().iter_entries().collect();
        let mut compiler = compiled.then(|| start(&world, true, ticks));
        if compiled {
            world.clear_scheduled_ticks();
        }
        let mut trace = Vec::new();
        for _ in 0..3 {
            if let Some(compiler) = &mut compiler {
                advance(compiler, &mut world, 1);
            } else {
                world.tick_interpreted();
            }
            trace.push(matches!(
                world.get_block(repeater),
                Block::RedstoneRepeater { repeater } if repeater.powered
            ));
        }
        trace
    };
    let native = run(false);
    assert_eq!(native, vec![true, false, false]);
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
    Compiler::default()
        .compile(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(31, 65, 31)),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap();
}

#[test]
fn graph_coalescing_preserves_input_channel_and_attenuation() {
    // These are graph counterexamples; classic physical-layout reachability is unverified.
    let repeater = NodeType::Repeater {
        delay: 1,
        facing_diode: false,
    };
    for (source_type, target_type, channel, attenuation, should_merge) in [
        (repeater.clone(), repeater.clone(), LinkType::Side, 0, false),
        (
            NodeType::InstantOutput { port: 0 },
            repeater.clone(),
            LinkType::Default,
            1,
            false,
        ),
        (
            NodeType::InstantOutput { port: 0 },
            repeater.clone(),
            LinkType::Default,
            0,
            true,
        ),
        (
            NodeType::Lever,
            repeater.clone(),
            LinkType::Default,
            14,
            true,
        ),
        (NodeType::Lever, repeater, LinkType::Default, 15, false),
        (NodeType::Lever, NodeType::Wire, LinkType::Default, 1, false),
    ] {
        let mut graph = CompileGraph::new();
        let node = |ty, is_input| CompileNode {
            native: false,
            ty,
            block: None,
            block_aliases: Vec::new(),
            state: NodeState::default(),
            is_input,
            is_output: false,
        };
        let source = graph.add_node(node(source_type, true));
        let first = graph.add_node(node(target_type.clone(), false));
        let second = graph.add_node(node(target_type, false));
        graph.add_edge(source, first, CompileLink::new(LinkType::Default, 0));
        graph.add_edge(source, second, CompileLink::new(channel, attenuation));
        super::coalesce::run(&mut graph).unwrap();
        if should_merge {
            assert_eq!(graph.node_count(), 2, "equivalent input should still merge");
        } else {
            assert!(
                graph.contains_node(first) && graph.contains_node(second),
                "distinct {channel:?} input with attenuation {attenuation} was merged"
            );
        }
    }
}
