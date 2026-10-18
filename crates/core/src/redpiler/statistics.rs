//! Graph sizes, work summaries, and compilation timings.

use super::{BackendKind, program_graph};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
/// Current execution ownership; counts describe circuit structure, not CPU time.
pub struct GraphExecutionCoverage {
    /// Electrical, wire, observer, reset and piston nodes; geometry-only contacts are excluded.
    pub component_nodes: usize,
    /// Original dust state owners, including wires executed by a compiled classic net.
    pub wire_nodes: usize,
    /// Original piston state owners, including actors with a private occupancy shortcut.
    pub piston_nodes: usize,
    /// Wire nodes currently owned by an active compiled classic net.
    pub compiled_classic_wires: usize,
    /// Wire nodes with a classic certificate, including nets that have deoptimized.
    pub certified_classic_wires: usize,
    /// Nodes executed by the whole fixed program, including its ordered propagation.
    pub fixed_classic_nodes: usize,
    /// Wire state owners executed by that program; disjoint from local classic nets.
    pub fixed_classic_wires: usize,
    /// Nodes whose internal execution is replaced by a whole instant kernel; currently zero.
    pub instant_kernel_nodes: usize,
    /// Nodes executed through a native fallback; zero in the Graph-only compiler.
    pub native_nodes: usize,
    /// Actors with a private resource-occupancy shortcut; their ordinary actions still execute.
    pub private_proxy_pistons: usize,
    /// Wire entry points with a certified reset-transfer shortcut; not whole-region ownership.
    pub reset_transfer_entries: usize,
    /// Routes with compiled instant instructions; these retain externally observable dispatch.
    pub instant_instruction_routes: usize,
    /// Observer state owners using the built-in reset pulse handler instead of ordinary observer dispatch.
    pub builtin_reset_clocks: usize,
}

impl GraphExecutionCoverage {
    pub fn general_graph_nodes(&self) -> usize {
        self.component_nodes - self.compiled_classic_wires - self.fixed_classic_nodes - self.instant_kernel_nodes - self.native_nodes
    }

    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!(
                "Execution ownership (component nodes): instant kernels {}; compiled classic wires {}; whole fixed program {}; general Graph {}; native fallback {}.",
                share(self.instant_kernel_nodes, self.component_nodes),
                share(self.compiled_classic_wires, self.component_nodes),
                share(self.fixed_classic_nodes, self.component_nodes),
                share(self.general_graph_nodes(), self.component_nodes),
                share(self.native_nodes, self.component_nodes),
            ),
            format!(
                "Wire ownership: compiled classic {}; whole fixed program {}; general Graph {}; {} certified wires currently deoptimized.",
                share(self.compiled_classic_wires, self.wire_nodes),
                share(self.fixed_classic_wires, self.wire_nodes),
                share(self.wire_nodes - self.compiled_classic_wires - self.fixed_classic_wires, self.wire_nodes),
                self.certified_classic_wires - self.compiled_classic_wires,
            ),
            format!(
                "Partial instant shortcuts (overlap Graph ownership): private piston checks {}; reset-transfer wire entries {}; {} instruction routes; {} built-in reset clocks. Coverage is not CPU-time share.",
                share(self.private_proxy_pistons, self.piston_nodes),
                share(self.reset_transfer_entries, self.wire_nodes),
                self.instant_instruction_routes,
                self.builtin_reset_clocks,
            ),
        ]
    }
}

fn share(count: usize, total: usize) -> String {
    if total == 0 {
        format!("{count}/{total} (n/a)")
    } else {
        format!("{count}/{total} ({:.1}%)", 100.0 * count as f64 / total as f64)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Node and link counts for one compiler graph snapshot.
pub struct GraphCounts {
    pub nodes: usize,
    pub links: usize,
}

impl GraphCounts {
    pub const fn new(nodes: usize, links: usize) -> Self {
        Self { nodes, links }
    }

    pub(crate) fn of(graph: &program_graph::CompileGraph) -> Self {
        Self::new(
            graph.nodes.len(),
            graph.links.len() + graph.power_links.len() + graph.wire_relations.len(),
        )
    }
}

#[derive(Clone, Debug)]
/// Timing and size results for one graph pass.
pub struct PassStatistics {
    pub name: &'static str,
    pub enabled: bool,
    pub before: GraphCounts,
    pub after: GraphCounts,
    pub duration: Duration,
}

impl PassStatistics {
    pub(crate) fn disabled(name: &'static str, counts: GraphCounts) -> Self {
        Self {
            name,
            enabled: false,
            before: counts,
            after: counts,
            duration: Duration::ZERO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Retained-capacity estimate for one preparation table.
pub struct PreparationTableStatistics {
    pub stage: &'static str,
    pub table: String,
    pub entries: usize,
    pub capacity: usize,
    /// Estimates retained element storage and known nested slices, excluding maps and allocator overhead.
    pub estimated_bytes: usize,
}

#[derive(Clone, Debug, Default)]
/// Graph sizes, pass timings, and preparation memory estimates from compilation.
pub struct GraphStatistics {
    pub passes: Vec<PassStatistics>,
    /// This contains only optional transformations, not mandatory parser stages.
    pub transformations: Vec<PassStatistics>,
    /// Retained table capacities sampled after each required preparation stage.
    pub preparation_tables: Vec<PreparationTableStatistics>,
    /// This records the graph after required parsing and before transformations.
    pub parser_complete: Option<GraphCounts>,
    /// This records the final graph independently of the supplied pass count.
    pub final_counts: Option<GraphCounts>,
    pub duration: Duration,
}

impl GraphStatistics {
    pub fn baseline(&self) -> Option<GraphCounts> {
        self.parser_complete
    }

    pub fn final_graph(&self) -> Option<GraphCounts> {
        self.final_counts
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let (Some(before), Some(after)) = (self.baseline(), self.final_graph()) {
            lines.push(format!(
                "Graph after required preparation: {} nodes / {} links; after passes: {} nodes / {} links (change {:+} nodes / {:+} links)",
                before.nodes,
                before.links,
                after.nodes,
                after.links,
                after.nodes as i128 - before.nodes as i128,
                after.links as i128 - before.links as i128,
            ));
            lines.push("Construction: all parsed wire nodes are retained.".into());
        }
        if let Some(summary) = self.preparation_table_summary() {
            lines.push(summary);
        }
        let enabled = self.transformations.iter().filter(|pass| pass.enabled).count();
        let skipped = self.transformations.len() - enabled;
        lines.push(if self.transformations.is_empty() {
            "Transformations: none configured.".into()
        } else {
            format!("Transformations: {enabled} enabled, {skipped} skipped.")
        });
        lines.extend(self.passes.iter().map(|pass| {
            format!(
                "{} [{}]: {} -> {} nodes, {} -> {} links, {:.3} ms",
                pass.name,
                if pass.enabled { "enabled" } else { "skipped" },
                pass.before.nodes,
                pass.after.nodes,
                pass.before.links,
                pass.after.links,
                pass.duration.as_secs_f64() * 1000.0,
            )
        }));
        lines
    }

    fn preparation_table_summary(&self) -> Option<String> {
        let stage = self.preparation_tables.last()?.stage;
        let mut tables: Vec<_> = self.preparation_tables.iter().filter(|table| table.stage == stage).collect();
        tables.sort_by_key(|table| std::cmp::Reverse(table.estimated_bytes));
        let estimate = tables.iter().map(|table| table.estimated_bytes).sum::<usize>();
        let largest = tables
            .iter()
            .take(3)
            .map(|table| {
                format!(
                    "{} {} entries, capacity {}, {:.1} MiB",
                    table.table,
                    table.entries,
                    table.capacity,
                    table.estimated_bytes as f64 / (1024.0 * 1024.0),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "Graph preparation table estimate at {stage}: {:.1} MiB retained capacity; largest: {largest}",
            estimate as f64 / (1024.0 * 1024.0),
        ))
    }
}

#[derive(Clone, Debug)]
/// Timing and backend size results for one successful compilation.
pub struct CompileStatistics {
    pub graph: GraphStatistics,
    pub backend: BackendKind,
    pub backend_nodes: usize,
    /// Structural ownership at activation; later deoptimization is visible through Compiler's live snapshot.
    pub execution_coverage: GraphExecutionCoverage,
    pub analysis_duration: Duration,
    pub backend_duration: Duration,
    pub total_duration: Duration,
}

impl CompileStatistics {
    pub fn summary_lines(&self) -> Vec<String> {
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let mut lines = vec![format!(
            "Compile: {:.3} ms total; analysis {:.3}, graph {:.3}, backend {:.3} ms; {} backend nodes",
            ms(self.total_duration),
            ms(self.analysis_duration),
            ms(self.graph.duration),
            ms(self.backend_duration),
            self.backend_nodes,
        )];
        lines.extend(self.graph.summary_lines().into_iter().take(3));
        lines.extend(self.execution_coverage.summary_lines());
        lines
    }
}
