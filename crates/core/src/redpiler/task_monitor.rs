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
}

impl TaskMonitor {
    pub fn graph_statistics(&self) -> GraphStatistics {
        self.graph_statistics.lock().unwrap().clone()
    }

    pub(crate) fn clear_graph_statistics(&self) {
        *self.graph_statistics.lock().unwrap() = GraphStatistics::default();
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
