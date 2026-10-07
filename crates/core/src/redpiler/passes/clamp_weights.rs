use crate::redpiler::compile_graph::CompileGraph;

pub(super) fn run(graph: &mut CompileGraph) -> Result<(), super::GraphError> {
    graph.retain_edges(|g, edge| g[edge].attenuation < 15);
    Ok(())
}
