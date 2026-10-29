use super::{GraphStatistics, PassStatistics, PreparationTableStatistics};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
/// Tracks cancellation, progress, messages, and graph statistics during compilation.
pub struct TaskMonitor {
    cancelled: Arc<AtomicBool>,
    max_progress: AtomicUsize,
    progress: AtomicUsize,
    message: Mutex<Option<Arc<String>>>,
    budget_multiplier: AtomicUsize,
    graph_statistics: Mutex<GraphStatistics>,
    warnings: Mutex<Vec<String>>,
    omitted_warnings: AtomicUsize,
}

#[cfg(test)]
mod tests {
    use super::TaskMonitor;

    #[test]
    fn warnings_are_bounded_and_cleared_for_the_next_compilation() {
        let monitor = TaskMonitor::default();
        monitor.record_warning("warning 0".into());
        for i in 0..40 {
            monitor.record_warning(format!("warning {i}"));
        }
        let warnings = monitor.warnings();
        assert_eq!(warnings.len(), 33);
        assert_eq!(warnings[0], "warning 0");
        assert_eq!(warnings[31], "warning 31");
        assert!(warnings[32].contains("8 additional warnings omitted"));
        monitor.clear_graph_statistics();
        assert!(monitor.warnings().is_empty());
        monitor.record_warning("overlap at (1, 2, 3)".into());
        monitor.record_warning("overlap at (1, 2, 3)".into());
        monitor.record_warning("overlap at (4, 5, 6)".into());
        assert_eq!(monitor.warnings(), ["overlap at (1, 2, 3)", "overlap at (4, 5, 6)"]);
        monitor.clear_graph_statistics();
        for warning in ["scoped summary", "candidate 1", "candidate 2", "whole-plot budget decline"] {
            monitor.record_warning(warning.into());
        }
        assert_eq!(monitor.warnings().last().unwrap(), "whole-plot budget decline");
        monitor.clear_graph_statistics();
        assert!(monitor.warnings().is_empty());
    }
}

impl TaskMonitor {
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = self.warnings.lock().unwrap().clone();
        let omitted = self.omitted_warnings.load(Ordering::Relaxed);
        if omitted > 0 {
            warnings.push(format!("{omitted} additional warnings omitted. Use /rp inspect for block details."));
        }
        warnings
    }

    #[cfg(test)]
    pub(crate) fn record_warning(&self, warning: String) {
        let mut warnings = self.warnings.lock().unwrap();
        if warnings.contains(&warning) {
            return;
        }
        if warnings.len() < 32 {
            warnings.push(warning);
        } else {
            self.omitted_warnings.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn graph_statistics(&self) -> GraphStatistics {
        self.graph_statistics.lock().unwrap().clone()
    }

    pub(crate) fn clear_graph_statistics(&self) {
        *self.graph_statistics.lock().unwrap() = GraphStatistics::default();
        self.warnings.lock().unwrap().clear();
        self.omitted_warnings.store(0, Ordering::Relaxed);
    }

    pub(crate) fn begin_graph_statistics(&self) {
        *self.graph_statistics.lock().unwrap() = GraphStatistics::default();
    }

    pub(crate) fn record_pass(&self, pass: PassStatistics) {
        self.graph_statistics.lock().unwrap().passes.push(pass);
    }

    pub(crate) fn record_transformation(&self, pass: PassStatistics) {
        let mut statistics = self.graph_statistics.lock().unwrap();
        statistics.passes.push(pass.clone());
        statistics.transformations.push(pass);
    }

    pub(crate) fn record_preparation_tables(&self, tables: Vec<PreparationTableStatistics>) {
        self.graph_statistics.lock().unwrap().preparation_tables.extend(tables);
    }

    pub(crate) fn preparation_stage_estimate(&self, stage: &'static str) -> usize {
        let statistics = self.graph_statistics.lock().unwrap();
        statistics
            .preparation_tables
            .iter()
            .filter(|table| table.stage == stage)
            .fold(0usize, |total, table| total.saturating_add(table.estimated_bytes))
    }

    pub(crate) fn set_parser_complete(&self, counts: super::GraphCounts) {
        self.graph_statistics.lock().unwrap().parser_complete = Some(counts);
    }

    pub(crate) fn set_final_graph(&self, counts: super::GraphCounts) {
        self.graph_statistics.lock().unwrap().final_counts = Some(counts);
    }

    pub(crate) fn finish_graph_statistics(&self, duration: Duration) {
        self.graph_statistics.lock().unwrap().duration = duration;
    }

    pub(crate) fn record_lowering(&self, program: Duration) {
        let mut statistics = self.graph_statistics.lock().unwrap();
        statistics.program_lowering_duration = program;
    }

    pub fn set_budget_multiplier(&self, multiplier: usize) {
        self.budget_multiplier.store(multiplier.clamp(1, 8), Ordering::Relaxed);
    }

    pub fn budget_multiplier(&self) -> usize {
        self.budget_multiplier.load(Ordering::Relaxed).clamp(1, 8)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn set_progress(&self, progress: usize) {
        self.progress.store(progress, Ordering::Relaxed);
    }

    pub fn inc_progress(&self) {
        self.progress.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_max_progress(&self, max_progress: usize) {
        self.max_progress.store(max_progress, Ordering::Relaxed);
    }

    pub fn progress(&self) -> usize {
        self.progress.load(Ordering::Relaxed)
    }

    pub fn max_progress(&self) -> usize {
        self.max_progress.load(Ordering::Relaxed)
    }

    pub fn set_message(&self, message: String) {
        *self.message.lock().unwrap() = Some(Arc::new(message));
    }

    pub fn message(&self) -> Option<Arc<String>> {
        self.message.lock().unwrap().clone()
    }
}
