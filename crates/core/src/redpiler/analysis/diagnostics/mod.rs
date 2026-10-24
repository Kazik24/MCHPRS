//! Read-only candidate graph diagnostics; these describe structural risks, not bugs.
use crate::redpiler::program_graph::CompileGraph;
use crate::redpiler::TaskMonitor;

mod timing;
mod unproven;
mod updates;

pub(crate) const MAX_WITNESSES: usize = 8;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiagnosticOptions {
    pub updates: bool,
    pub timing: bool,
    pub unproven: bool,
}

impl DiagnosticOptions {
    pub(crate) fn any(self) -> bool {
        self.updates || self.timing || self.unproven
    }
}

pub(crate) fn report(graph: &CompileGraph, options: DiagnosticOptions, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    if options.updates {
        lines.extend(updates::report(graph, monitor)?);
    }
    if options.timing {
        lines.extend(timing::report(graph, monitor)?);
    }
    if options.unproven {
        lines.extend(unproven::report(graph, monitor)?);
    }
    if monitor.cancelled() {
        return Err("diagnostics cancelled".into());
    }
    Ok(lines)
}
