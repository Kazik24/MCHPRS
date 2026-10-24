//! Read-only candidate graph diagnostics; these describe structural risks, not bugs.
use crate::redpiler::program_graph::CompileGraph;
use crate::redpiler::TaskMonitor;

pub(crate) mod timing;
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

pub(crate) fn report(
    graph: &CompileGraph,
    options: DiagnosticOptions,
    timing_probe: Option<&timing::TimingProbeReport>,
    monitor: &TaskMonitor,
) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    if options.updates {
        lines.extend(updates::report(graph, monitor)?);
    }
    if options.timing {
        let probe = timing_probe.ok_or("timing diagnostics require a cold replay result")?;
        lines.extend(timing::report(probe, monitor)?);
    }
    if options.unproven {
        lines.extend(unproven::report(graph, monitor)?);
    }
    if options.updates || options.unproven {
        lines.push(match (options.updates, options.unproven) {
            (true, true) => "Candidates are not confirmed faults; unproven does not mean non-instant.".into(),
            (true, false) => "Candidates are not confirmed faults.".into(),
            (false, true) => "Local evidence does not prove instant behavior.".into(),
            (false, false) => unreachable!(),
        });
    }
    if monitor.cancelled() {
        return Err("diagnostics cancelled".into());
    }
    Ok(lines)
}
