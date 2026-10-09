use crate::redpiler::compile_graph::{CompileGraph, LinkType, NodeIdx};
use crate::redpiler::CompilerOptions;
use itertools::Itertools;
use mchprs_blocks::blocks::{Block, Instrument};
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use tracing::trace;

use super::node::{ForwardLink, Node, NodeId, NodeInput, NodeType, Nodes, NonMaxU8};
use super::DirectBackend;
use crate::redpiler::backend::BackendError;

const MAX_INPUTS: usize = u8::MAX as usize;

#[derive(Debug, Default)]
struct FinalGraphStats {
    update_link_count: usize,
    side_link_count: usize,
    default_link_count: usize,
    nodes_bytes: usize,
}

fn compile_node(
    graph: &CompileGraph,
    node_idx: NodeIdx,
    nodes_len: usize,
    nodes_map: &FxHashMap<NodeIdx, usize>,
    noteblock_info: &mut Vec<(BlockPos, Instrument, u32)>,
    stats: &mut FinalGraphStats,
) -> Node {
    let node = &graph[node_idx];

    let mut default_input_count = 0;
    let mut side_input_count = 0;

    let mut default_inputs = NodeInput {
        strength_counts: [0; 16],
    };
    let mut side_inputs = NodeInput {
        strength_counts: [0; 16],
    };
    // Strengths and channel sizes were validated before lowering.
    for edge in graph.edges_directed(node_idx, Direction::Incoming) {
        let weight = edge.weight();
        let distance = weight.attenuation;
        let source = edge.source();
        let strength = graph[source].state.output_strength.saturating_sub(distance);
        match weight.ty {
            LinkType::Default => {
                default_input_count += 1;
                default_inputs.strength_counts[strength as usize] += 1;
            }
            LinkType::Side => {
                side_input_count += 1;
                side_inputs.strength_counts[strength as usize] += 1;
            }
        }
    }
    stats.default_link_count += default_input_count;
    stats.side_link_count += side_input_count;

    use crate::redpiler::compile_graph::NodeType as CNodeType;
    let updates = if node.ty != CNodeType::Constant {
        graph
            .edges_directed(node_idx, Direction::Outgoing)
            .sorted_by_key(|edge| nodes_map[&edge.target()])
            .into_group_map_by(|edge| std::mem::discriminant(&graph[edge.target()].ty))
            .into_values()
            .flatten()
            .map(|edge| unsafe {
                let idx = edge.target();
                let idx = nodes_map[&idx];
                assert!(idx < nodes_len);
                // Safety: bounds checked
                let target_id = NodeId::from_index(idx);

                let weight = edge.weight();
                ForwardLink::new(target_id, weight.ty == LinkType::Side, weight.attenuation)
            })
            .collect()
    } else {
        SmallVec::new()
    };
    stats.update_link_count += updates.len();

    let ty = match &node.ty {
        CNodeType::Repeater {
            delay,
            facing_diode,
        } => NodeType::Repeater {
            delay: *delay,
            facing_diode: *facing_diode,
        },
        CNodeType::Torch => NodeType::Torch,
        CNodeType::Observer { .. } => NodeType::Observer,
        CNodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => NodeType::Comparator {
            mode: *mode,
            far_input: far_input.map(|value| NonMaxU8::new(value).unwrap()),
            facing_diode: *facing_diode,
        },
        CNodeType::Lamp => NodeType::Lamp,
        CNodeType::CopperBulb => NodeType::CopperBulb,
        CNodeType::Button => NodeType::Button,
        CNodeType::Lever => NodeType::Lever,
        CNodeType::PressurePlate => NodeType::PressurePlate,
        CNodeType::Trapdoor => NodeType::Trapdoor,
        CNodeType::Wire => NodeType::Wire,
        CNodeType::Constant => NodeType::Constant,
        CNodeType::CommandBlock {
            repeating,
            chain,
            automatic,
            ..
        } => NodeType::CommandBlock {
            repeating: *repeating,
            chain: *chain,
            automatic: *automatic,
        },
        CNodeType::MobileSource { .. } | CNodeType::InstantOutput { .. } => NodeType::InstantSource,
        CNodeType::InstantInput { .. } => {
            unreachable!("boundary nodes rejected before lowering")
        }
        CNodeType::NoteBlock { instrument, note } => {
            let noteblock_id = noteblock_info.len().try_into().unwrap();
            noteblock_info.push((node.block.unwrap().0, *instrument, *note));
            NodeType::NoteBlock { noteblock_id }
        }
    };

    Node {
        ty,
        default_inputs,
        side_inputs,
        updates,
        powered: node.state.powered,
        output_power: node.state.output_strength,
        locked: node.state.repeater_locked,
        pending_tick: false,
        changed: false,
        is_io: node.is_input || node.is_output,
    }
}

pub fn compile(
    backend: &mut DirectBackend,
    graph: CompileGraph,
    ticks: Vec<TickEntry>,
    options: &CompilerOptions,
    instant: Vec<crate::redpiler::instant::program::PreparedInstant>,
) -> Result<(), BackendError> {
    // Geometry observation reads settled blocks, never electrical alias power.
    let geometry_positions: FxHashSet<_> = instant
        .iter()
        .flat_map(|program| {
            program
                .pistons
                .iter()
                .flat_map(|piston| [piston.pos, piston.head])
                .chain(program.aliases.iter().filter_map(|&(group, pos, _)| {
                    (program.payloads[group] != Block::Air).then_some(pos)
                }))
        })
        .collect();
    if graph.node_weights().any(|n| {
        matches!(
            n.ty,
            crate::redpiler::compile_graph::NodeType::InstantInput { .. }
                | crate::redpiler::compile_graph::NodeType::MobileSource { .. }
                | crate::redpiler::compile_graph::NodeType::InstantOutput { .. }
        )
    }) && instant.is_empty()
    {
        return Err(BackendError::InstantRuntimeUnavailable);
    }
    // Validate before filling packed input counters or creating unchecked
    // runtime references. Failure must leave the staged backend untouched.
    for id in graph.node_indices() {
        let node = &graph[id];
        let pos = node.block.map(|(pos, _)| pos);
        let far_input = match node.ty {
            crate::redpiler::compile_graph::NodeType::Comparator { far_input, .. } => far_input,
            _ => None,
        };
        for strength in std::iter::once(node.state.output_strength).chain(far_input) {
            if strength > 15 {
                return Err(BackendError::InvalidStrength { pos, strength });
            }
        }
        let mut default_inputs = 0;
        let mut side_inputs = 0;
        for edge in graph.edges_directed(id, Direction::Incoming) {
            match edge.weight().ty {
                LinkType::Default => default_inputs += 1,
                LinkType::Side => side_inputs += 1,
            }
        }
        if default_inputs > MAX_INPUTS || side_inputs > MAX_INPUTS {
            return Err(BackendError::TooManyInputs {
                pos,
                default_inputs,
                side_inputs,
            });
        }
    }
    // Create a mapping from compile to backend node indices
    let mut nodes_map = FxHashMap::with_capacity_and_hasher(graph.node_count(), Default::default());
    for node in graph.node_indices() {
        nodes_map.insert(node, nodes_map.len());
    }
    let nodes_len = nodes_map.len();

    // Lower nodes
    let mut stats = FinalGraphStats::default();
    let nodes = graph
        .node_indices()
        .map(|idx| {
            compile_node(
                &graph,
                idx,
                nodes_len,
                &nodes_map,
                &mut backend.noteblock_info,
                &mut stats,
            )
        })
        .collect();
    stats.nodes_bytes = nodes_len * std::mem::size_of::<Node>();
    trace!("{:#?}", stats);

    backend.blocks = graph
        .node_weights()
        .map(|node| node.block.map(|(pos, id)| (pos, Block::from_id(id))))
        .collect();
    backend.nodes = Nodes::new(nodes);
    if !instant.is_empty() {
        let bindings = graph
            .node_indices()
            .filter_map(|idx| match graph[idx].ty {
                crate::redpiler::compile_graph::NodeType::MobileSource { alias, .. } => {
                    Some((alias, backend.nodes.get(nodes_map[&idx])))
                }
                _ => graph[idx]
                    .block
                    .map(|(pos, _)| (pos, backend.nodes.get(nodes_map[&idx]))),
            })
            .collect::<FxHashMap<_, _>>();
        let outputs = graph
            .node_indices()
            .filter_map(|idx| match graph[idx].ty {
                crate::redpiler::compile_graph::NodeType::InstantOutput { port } => {
                    Some((port, backend.nodes.get(nodes_map[&idx])))
                }
                _ => None,
            })
            .collect::<FxHashMap<_, _>>();
        for program in instant {
            backend.instant.push(super::instant::Runtime::bind(
                program,
                &bindings,
                &outputs,
                &backend.nodes,
            )?);
        }
        backend.instant_dirty = vec![true; backend.instant.len()];
        for (region, runtime) in backend.instant.iter().enumerate() {
            for source in runtime.source_nodes() {
                backend
                    .instant_dependencies
                    .entry(source)
                    .or_default()
                    .push(region);
            }
        }
        for regions in backend.instant_dependencies.values_mut() {
            regions.sort_unstable();
            regions.dedup();
        }
    }

    // Create a mapping from block pos to backend NodeId
    for i in 0..backend.blocks.len() {
        if let Some((pos, _)) = backend.blocks[i] {
            backend.pos_map.insert(pos, backend.nodes.get(i));
        }
    }

    for idx in graph.node_indices() {
        let crate::redpiler::compile_graph::NodeType::Observer { watched } = graph[idx].ty else {
            continue;
        };
        let observer = backend.nodes.get(nodes_map[&idx]);
        if geometry_positions.contains(&watched) {
            backend
                .instant_observers
                .entry(watched)
                .or_default()
                .push(observer);
        } else if let Some(&source) = backend.pos_map.get(&watched) {
            backend
                .observer_watchers
                .entry(source)
                .or_default()
                .push(observer);
        }
        // A fixed cell without a node cannot change while compilation is active.
    }
    for observers in backend
        .observer_watchers
        .values_mut()
        .chain(backend.instant_observers.values_mut())
    {
        observers.sort_unstable_by_key(|id| id.index());
        observers.dedup();
    }
    for &pos in backend.instant_observers.keys() {
        let bindings = backend
            .instant
            .iter_mut()
            .map(|runtime| usize::from(runtime.watch_geometry(pos)))
            .sum::<usize>();
        if bindings != 1 {
            return Err(BackendError::ObserverGeometryBinding { pos, bindings });
        }
    }

    // Dynamic analog overrides can be read through a conducting rear block.
    for (i, block) in backend.blocks.iter().enumerate() {
        let Some((pos, Block::RedstoneComparator { comparator })) = block else {
            continue;
        };
        let id = backend.nodes.get(i);
        if !matches!(
            backend.nodes[id].ty,
            NodeType::Comparator {
                far_input: Some(_),
                ..
            }
        ) {
            continue;
        }
        let far = pos
            .offset(comparator.facing.block_face())
            .offset(comparator.facing.block_face());
        if let Some(&source) = backend.pos_map.get(&far) {
            if matches!(
                backend.nodes[source].ty,
                NodeType::CommandBlock { .. } | NodeType::CopperBulb
            ) {
                backend.far_comparators.entry(source).or_default().push(id);
            }
        }
    }

    // Preserve pending tick deadlines, priorities and input order.
    for entry in ticks {
        if let Some(node) = backend.pos_map.get(&entry.pos) {
            backend.scheduler.schedule_half_tick(
                *node,
                entry.ticks_left as usize,
                entry.tick_priority,
            );
            backend.nodes[*node].pending_tick = true;
        }
    }

    // A powered saved observer with no surviving off deadline still needs to
    // finish its pulse. Imported pending deadlines take precedence.
    for i in 0..backend.nodes.inner().len() {
        let id = backend.nodes.get(i);
        let node = &mut backend.nodes[id];
        if matches!(node.ty, NodeType::Observer) && node.powered && !node.pending_tick {
            super::schedule_tick(
                &mut backend.scheduler,
                id,
                node,
                1,
                mchprs_world::TickPriority::Normal,
            );
        }
    }

    // Initialize command-block power and automatic execution requests.
    for idx in graph.node_indices() {
        if let crate::redpiler::compile_graph::NodeType::CommandBlock {
            initial_tick,
            chain,
            ..
        } = graph[idx].ty
        {
            let id = backend.nodes.get(nodes_map[&idx]);
            update_command_output(backend, id, initial_tick && !chain);
        }
    }

    if options.export_dot_graph {
        std::fs::write("backend_graph.dot", format!("{}", backend)).unwrap();
    }
    Ok(())
}

fn update_command_output(backend: &mut DirectBackend, id: NodeId, initial_tick: bool) {
    super::update::update_node(
        &mut backend.scheduler,
        &mut backend.events,
        &mut backend.nodes,
        id,
    );
    if initial_tick && !backend.nodes[id].pending_tick {
        backend.nodes[id].pending_tick = true;
        backend
            .scheduler
            .schedule_half_tick(id, 1, mchprs_world::TickPriority::Normal);
        backend.events.push(super::Event::CommandBlockPower {
            node_id: id,
            powered: backend.nodes[id].powered,
            capture_condition: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::compile_graph::{CompileLink, CompileNode, NodeState, NodeType};

    fn node(ty: NodeType, strength: u8) -> CompileNode {
        CompileNode {
            ty,
            block: None,
            state: NodeState::with_strength(strength),
            is_input: false,
            is_output: false,
        }
    }

    #[test]
    fn invalid_strength_is_rejected_before_packed_input_initialization() {
        for strength in [16, 255] {
            let mut graph = CompileGraph::new();
            graph.add_node(node(NodeType::Constant, strength));
            let mut backend = DirectBackend::default();
            assert_eq!(
                backend.compile(graph, vec![], &Default::default(), Default::default()),
                Err(BackendError::InvalidStrength {
                    pos: None,
                    strength
                })
            );
            assert!(backend.nodes.inner().is_empty());
        }
    }

    #[test]
    fn excessive_fan_in_is_rejected_without_overflowing_counters() {
        let mut graph = CompileGraph::new();
        let target = graph.add_node(node(NodeType::Lamp, 0));
        for _ in 0..256 {
            let source = graph.add_node(node(NodeType::Constant, 15));
            graph.add_edge(source, target, CompileLink::new(LinkType::Default, 0));
        }
        let mut backend = DirectBackend::default();
        assert_eq!(
            backend.compile(graph, vec![], &Default::default(), Default::default()),
            Err(BackendError::TooManyInputs {
                pos: None,
                default_inputs: 256,
                side_inputs: 0
            })
        );
        assert!(backend.nodes.inner().is_empty());
    }

    #[test]
    fn source_changes_publish_all_inputs_before_notifying_comparators() {
        for side_first in [false, true] {
            let mut graph = CompileGraph::new();
            let source = graph.add_node(node(NodeType::Lever, 0));
            let rear_source = graph.add_node(node(NodeType::Lever, 0));
            let comparator = graph.add_node(node(
                NodeType::Comparator {
                    mode: mchprs_blocks::blocks::ComparatorMode::Compare,
                    far_input: None,
                    facing_diode: false,
                },
                0,
            ));
            graph.add_edge(source, comparator, CompileLink::new(LinkType::Default, 3));
            graph.add_edge(source, comparator, CompileLink::new(LinkType::Side, 1));
            graph.add_edge(
                rear_source,
                comparator,
                CompileLink::new(LinkType::Default, 1),
            );
            let mut backend = DirectBackend::default();
            backend
                .compile(graph, vec![], &Default::default(), vec![])
                .unwrap();
            let source = backend.nodes.get(0);
            let rear_source = backend.nodes.get(1);
            let comparator = backend.nodes.get(2);
            backend.nodes[source]
                .updates
                .sort_by_key(|link| link.side());
            if side_first {
                backend.nodes[source].updates.reverse();
            }

            backend.set_node(source, true, 15);
            assert_eq!(
                super::super::input_strengths(&backend.nodes[comparator]),
                (12, 14)
            );
            assert!(
                !backend.nodes[comparator].pending_tick,
                "side_first={side_first}"
            );
            backend.tick();
            backend.tick();
            assert_eq!(backend.nodes[comparator].output_power, 0);

            backend.set_node(rear_source, true, 15);
            assert!(backend.nodes[comparator].pending_tick);
            backend.tick();
            assert_eq!(backend.nodes[comparator].output_power, 0);
            backend.tick();
            assert_eq!(backend.nodes[comparator].output_power, 14);

            backend.set_node(rear_source, false, 0);
            backend.tick();
            backend.tick();
            assert_eq!(backend.nodes[comparator].output_power, 0);
            backend.set_node(source, false, 0);
            assert_eq!(
                super::super::input_strengths(&backend.nodes[comparator]),
                (0, 0)
            );
            assert!(
                !backend.nodes[comparator].pending_tick,
                "side_first={side_first}"
            );
        }
    }

    fn observer_chain(
        ticks: Vec<TickEntry>,
        powered: bool,
    ) -> (DirectBackend, [super::super::node::NodeId; 3]) {
        use mchprs_blocks::blocks::{Lever, RedstoneObserver};
        use mchprs_blocks::BlockFacing;
        let positions = [
            BlockPos::new(4, 30, 4),
            BlockPos::new(5, 30, 4),
            BlockPos::new(6, 30, 4),
        ];
        let mut graph = CompileGraph::new();
        let mut lever = node(NodeType::Lever, 0);
        lever.block = Some((
            positions[0],
            Block::Lever {
                lever: Lever::default(),
            }
            .get_id(),
        ));
        graph.add_node(lever);
        for i in 1..3 {
            let mut observer = node(
                NodeType::Observer {
                    watched: positions[i - 1],
                },
                0,
            );
            observer.block = Some((
                positions[i],
                Block::Observer {
                    observer: RedstoneObserver {
                        facing: BlockFacing::West,
                        powered: powered && i == 1,
                    },
                }
                .get_id(),
            ));
            observer.state = NodeState::simple(powered && i == 1);
            graph.add_node(observer);
        }
        let mut backend = DirectBackend::default();
        backend
            .compile(graph, ticks, &Default::default(), vec![])
            .unwrap();
        let ids = positions.map(|pos| backend.pos_map[&pos]);
        (backend, ids)
    }

    #[test]
    fn observer_watch_chain_preserves_pulse_delays_and_ignores_pending_retriggers() {
        let (mut backend, [lever, first, second]) = observer_chain(vec![], false);
        backend.set_node(lever, true, 15);
        backend.set_node(lever, false, 0); // Changes during the pending rise do not extend it.
        assert!(backend.nodes[first].pending_tick);
        for (tick, expected) in [
            (1, (0, 0)),
            (2, (15, 0)),
            (3, (15, 0)),
            (4, (0, 15)),
            (5, (0, 15)),
            (6, (0, 0)),
        ] {
            backend.tick();
            assert_eq!(
                (
                    backend.nodes[first].output_power,
                    backend.nodes[second].output_power
                ),
                expected,
                "half tick {tick}"
            );
            if tick == 2 {
                backend.set_node(lever, true, 15); // The powered pulse also does not retrigger.
            }
        }
        for _ in 0..4 {
            backend.tick();
        }
        assert!(!backend.nodes[first].pending_tick && !backend.nodes[second].pending_tick);
        backend.set_node(lever, false, 0);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 0);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 15);
    }

    #[test]
    fn observer_imported_deadline_and_powered_entry_complete_without_retrigger() {
        let entry = TickEntry {
            block_type: None,
            pos: BlockPos::new(5, 30, 4),
            ticks_left: 1,
            tick_priority: mchprs_world::TickPriority::High,
        };
        let (mut backend, [lever, first, _]) = observer_chain(vec![entry], false);
        backend.set_node(lever, true, 15);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 15);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 15);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 0);

        let (mut backend, [_, first, _]) = observer_chain(vec![], true);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 15);
        backend.tick();
        assert_eq!(backend.nodes[first].output_power, 0);
    }

    #[test]
    fn observer_sees_comparator_block_state_but_not_entity_strength_alone() {
        let source_pos = BlockPos::new(4, 30, 4);
        let observer_pos = BlockPos::new(5, 30, 4);
        let mut graph = CompileGraph::new();
        let mut source = node(
            NodeType::Comparator {
                mode: mchprs_blocks::blocks::ComparatorMode::Compare,
                far_input: None,
                facing_diode: false,
            },
            5,
        );
        source.state.powered = true;
        source.block = Some((
            source_pos,
            Block::RedstoneComparator {
                comparator: mchprs_blocks::blocks::RedstoneComparator {
                    powered: true,
                    ..Default::default()
                },
            }
            .get_id(),
        ));
        graph.add_node(source);
        let mut observer = node(
            NodeType::Observer {
                watched: source_pos,
            },
            0,
        );
        observer.block = Some((
            observer_pos,
            Block::Observer {
                observer: mchprs_blocks::blocks::RedstoneObserver::default(),
            }
            .get_id(),
        ));
        graph.add_node(observer);
        let mut backend = DirectBackend::default();
        backend
            .compile(graph, vec![], &Default::default(), vec![])
            .unwrap();
        let source = backend.pos_map[&source_pos];
        let observer = backend.pos_map[&observer_pos];
        backend.set_node(source, true, 8);
        assert!(!backend.nodes[observer].pending_tick);
        backend.set_node(source, false, 0);
        assert!(backend.nodes[observer].pending_tick);
        backend.tick();
        backend.tick();
        assert_eq!(backend.nodes[observer].output_power, 15);
    }
}
