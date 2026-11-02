use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct CompileStatus {
    phase: &'static str,
    detail: Option<(&'static str, usize)>,
    progress: Option<(usize, usize)>,
    counts: Option<(usize, usize, usize)>,
}

#[derive(Default)]
pub struct TaskMonitor {
    /// shared cancellation flag observed by compilation workers.
    cancelled: AtomicBool,
    status: Mutex<CompileStatus>,
}

impl TaskMonitor {
    pub(crate) fn set_phase(&self, phase: &'static str) {
        let mut status = self.status.lock().unwrap();
        status.phase = phase;
        status.detail = None;
        status.progress = None;
    }

    pub(crate) fn set_detail(&self, step: &'static str, item: usize) {
        self.status.lock().unwrap().detail = Some((step, item));
    }

    pub(crate) fn set_progress(&self, completed: usize, total: usize) {
        self.status.lock().unwrap().progress = Some((completed, total));
    }

    pub(crate) fn advance_progress(&self) {
        if let Some((completed, _)) = &mut self.status.lock().unwrap().progress {
            *completed += 1;
        }
    }

    pub(crate) fn set_counts(&self, nodes: usize, wires: usize, pistons: usize) {
        self.status.lock().unwrap().counts = Some((nodes, wires, pistons));
    }

    pub(crate) fn status(&self) -> String {
        let status = self.status.lock().unwrap();
        let mut text = if status.phase.is_empty() { "preparing snapshot" } else { status.phase }.to_owned();
        if let Some((step, item)) = status.detail { text.push_str(&format!("; {step} (component {item})")); }
        if let Some((completed, total)) = status.progress { text.push_str(&format!("; {completed}/{total} completed")); }
        if let Some((nodes, wires, pistons)) = status.counts { text.push_str(&format!("; {nodes} nodes, {wires} wires, {pistons} pistons")); }
        text
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_changes_phases_retains_counts_and_tracks_parallel_jobs() {
        let monitor = TaskMonitor::default();
        monitor.set_counts(12, 5, 2);
        monitor.set_phase("sampling");
        monitor.set_progress(0, 8);
        std::thread::scope(|scope| {
            for _ in 0..8 { scope.spawn(|| monitor.advance_progress()); }
        });
        assert_eq!(monitor.status(), "sampling; 8/8 completed; 12 nodes, 5 wires, 2 pistons");
        monitor.set_phase("finalizing");
        assert_eq!(monitor.status(), "finalizing; 12 nodes, 5 wires, 2 pistons");
        monitor.cancel();
        assert!(monitor.cancelled());
    }
}
