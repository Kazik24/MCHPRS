use crate::redpiler::{compile_graph::CompileGraph, instant::program::PreparedInstant};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use std::{fmt::Write, fs, io};

pub(super) fn write(graph: &CompileGraph, assemblies: &[PreparedInstant]) -> io::Result<()> {
    fs::write("redpiler_nodes.txt", format(graph, assemblies))
}

fn format(graph: &CompileGraph, assemblies: &[PreparedInstant]) -> String {
    let mut text = String::new();
    let instant_nodes = graph
        .node_weights()
        .filter(|node| {
            matches!(
                &node.ty,
                crate::redpiler::compile_graph::NodeType::InstantInput { .. }
                    | crate::redpiler::compile_graph::NodeType::MobileSource { .. }
                    | crate::redpiler::compile_graph::NodeType::InstantOutput { .. }
            )
        })
        .count();
    writeln!(
        text,
        "graph_nodes {} ordinary_nodes {} instant_interface_nodes {}",
        graph.node_count(),
        graph.node_count() - instant_nodes,
        instant_nodes,
    )
    .unwrap();
    for id in graph.node_indices() {
        let node = &graph[id];
        writeln!(
            text,
            "node {} type={:?} native={} input={} output={} state={:?} block={:?} aliases={:?}",
            id.index(),
            node.ty,
            node.native,
            node.is_input,
            node.is_output,
            node.state,
            node.block,
            node.block_aliases,
        )
        .unwrap();
    }
    writeln!(text, "graph_edges {}", graph.edge_count()).unwrap();
    for edge in graph.edge_references() {
        writeln!(
            text,
            "edge {} -> {} type={:?} attenuation={}",
            edge.source().index(),
            edge.target().index(),
            edge.weight().ty,
            edge.weight().attenuation,
        )
        .unwrap();
    }

    for (region, program) in assemblies.iter().enumerate() {
        let private_wires = program
            .logic
            .wires
            .difference(&program.propagation_wires)
            .count();
        writeln!(
            text,
            "\nassembly {} pistons={} candidate_wires={} private_wires={} retained_wires={} decisions={} responses={} outputs={}",
            region, program.pistons.len(), program.logic.wires.len(), private_wires,
            program.propagation_wires.len(), program.logic.arena.nodes.len(),
            program.logic.responses.len(), program.logic.outputs.len(),
        )
        .unwrap();

        let mut private_positions: Vec<_> = program
            .logic
            .wires
            .difference(&program.propagation_wires)
            .copied()
            .collect();
        private_positions.sort_by_key(|pos| (pos.y, pos.z, pos.x));
        for pos in private_positions {
            writeln!(text, "private_wire {pos:?}").unwrap();
        }
        let mut retained_positions: Vec<_> = program.propagation_wires.iter().copied().collect();
        retained_positions.sort_by_key(|pos| (pos.y, pos.z, pos.x));
        for pos in retained_positions {
            writeln!(text, "retained_wire {pos:?}").unwrap();
        }

        for (actor, piston) in program.pistons.iter().enumerate() {
            writeln!(
                text,
                "piston {actor} base={:?} head={:?}",
                piston.pos, piston.head
            )
            .unwrap();
        }
        for (actor, &root) in program.logic.responses.iter().enumerate() {
            writeln!(
                text,
                "response {actor} base={:?} expr={root}",
                program.pistons[actor].pos,
            )
            .unwrap();
        }
        for (index, decision) in program.logic.arena.nodes.iter().enumerate() {
            writeln!(
                text,
                "decision {} variable={:?} low={} high={}",
                index + 2,
                decision.variable,
                decision.low,
                decision.high,
            )
            .unwrap();
        }
        for (index, source) in program.logic.sources.iter().enumerate() {
            writeln!(text, "source {index} pos={source:?}").unwrap();
        }
        for (index, output) in program.logic.outputs.iter().enumerate() {
            writeln!(
                text,
                "output {} consumer={:?} input={:?} initial={} terms={}",
                program.output_offset + index,
                output.consumer,
                output.input,
                output.initial_strength,
                output.terms.len(),
            )
            .unwrap();
            for (term, power) in output.terms.iter().enumerate() {
                writeln!(
                    text,
                    "output_term {index}.{term} guard={} source={:?} attenuation={}",
                    power.guard, power.source, power.attenuation,
                )
                .unwrap();
            }
        }

        let mut links: Vec<_> = program.logic.wire_links.iter().copied().collect();
        links.sort_by_key(|(a, b)| (a.y, a.z, a.x, b.y, b.z, b.x));
        for (from, to) in links {
            writeln!(text, "wire_link {from:?} -> {to:?}").unwrap();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::compile_graph::{CompileLink, CompileNode, LinkType, NodeState, NodeType};

    #[test]
    fn listing_contains_nodes_and_edges() {
        let mut graph = CompileGraph::new();
        let node = |ty| CompileNode {
            native: false,
            ty,
            block: None,
            block_aliases: Vec::new(),
            state: NodeState::default(),
            is_input: false,
            is_output: false,
        };
        let source = graph.add_node(node(NodeType::Lever));
        let target = graph.add_node(node(NodeType::Lamp));
        graph.add_edge(source, target, CompileLink::new(LinkType::Default, 0));
        let listing = format(&graph, &[]);
        assert!(listing.contains("graph_nodes 2 ordinary_nodes 2 instant_interface_nodes 0"));
        assert!(listing.contains("node 0 type=Lever"));
        assert!(listing.contains("node 1 type=Lamp"));
        assert!(listing.contains("edge 0 -> 1"));
    }
}
