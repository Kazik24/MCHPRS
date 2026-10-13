use crate::redpiler::compile_graph::LegacyCompileGraph;

pub(super) fn run(graph: &mut LegacyCompileGraph) -> Result<(), super::GraphError> {
    graph.retain_edges(|g, edge| g[edge].attenuation < 15);
    Ok(())
}
