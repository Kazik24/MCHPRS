use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Default)]
struct CompileStatus {
    phase: &'static str,
    phase_number: usize,
    phase_started: Option<Instant>,
    detail: Option<(&'static str, usize)>,
    progress: Option<(usize, usize, &'static str)>,
    counts: Option<(usize, usize, usize)>,
}

#[derive(Default)]
pub struct TaskMonitor {
    /// shared cancellation flag observed by compilation workers.
    cancelled: AtomicBool,
    status: Mutex<CompileStatus>,
    deadline: Option<Instant>,
}

impl TaskMonitor {
    pub(crate) fn with_deadline(deadline: Option<Instant>) -> Self {
        Self { deadline, ..Self::default() }
    }

    pub(crate) fn budget_expired(&self) -> bool {
        !self.cancelled.load(Ordering::Relaxed) && self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub(crate) fn set_phase(&self, phase: &'static str) {
        let mut status = self.status.lock().unwrap();
        status.phase = phase;
        status.phase_number += 1;
        status.phase_started = Some(Instant::now());
        status.detail = None;
        status.progress = None;
    }

    pub(crate) fn set_detail(&self, step: &'static str, item: usize) {
        self.status.lock().unwrap().detail = Some((step, item));
    }

    pub(crate) fn set_progress(&self, completed: usize, total: usize, unit: &'static str) {
        self.status.lock().unwrap().progress = Some((completed, total, unit));
    }

    pub(crate) fn advance_progress(&self) {
        if let Some((completed, _, _)) = &mut self.status.lock().unwrap().progress {
            *completed += 1;
        }
    }

    pub(crate) fn set_counts(&self, nodes: usize, wires: usize, pistons: usize) {
        self.status.lock().unwrap().counts = Some((nodes, wires, pistons));
    }

    pub(crate) fn status(&self) -> String {
        let status = self.status.lock().unwrap();
        let mut text = match status.phase_started {
            Some(started) => format!(
                "phase {}: {} ({:.1}s in this phase)",
                status.phase_number,
                status.phase,
                started.elapsed().as_secs_f64()
            ),
            None => "preparing snapshot".to_owned(),
        };
        if let Some((step, _)) = status.detail {
            text.push_str(&format!("; {step}"));
        }
        if let Some((completed, total, unit)) = status.progress {
            text.push_str(&format!("; {completed}/{total} {unit}"));
        }
        if let Some((nodes, wires, pistons)) = status.counts {
            text.push_str(&format!("; {nodes} nodes, {wires} wires, {pistons} pistons"));
        }
        text
    }

    pub(crate) fn phase_number(&self) -> usize {
        self.status.lock().unwrap().phase_number
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.budget_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_cancels_work_without_confusing_manual_cancellation() {
        let monitor = TaskMonitor::with_deadline(Some(Instant::now()));
        assert!(monitor.budget_expired());
        assert!(monitor.cancelled());
        monitor.cancel();
        assert!(!monitor.budget_expired());
        assert!(monitor.cancelled());
        let monitor = TaskMonitor::with_deadline(None);
        assert!(!monitor.budget_expired());
        assert!(!monitor.cancelled());
    }

    #[test]
    fn progress_changes_phases_retains_counts_and_tracks_parallel_jobs() {
        let monitor = TaskMonitor::default();
        monitor.set_counts(12, 5, 2);
        monitor.set_phase("sampling");
        monitor.set_progress(0, 8, "probes");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| monitor.advance_progress());
            }
        });
        assert!(monitor.status().starts_with("phase 1: sampling ("));
        assert!(monitor.status().contains("8/8 probes; 12 nodes, 5 wires, 2 pistons"));
        monitor.set_phase("finalizing");
        assert!(monitor.status().starts_with("phase 2: finalizing ("));
        assert!(monitor.status().contains("12 nodes, 5 wires, 2 pistons"));
        assert!(!monitor.status().contains("probes"));
        monitor.cancel();
        assert!(monitor.cancelled());
    }
}
