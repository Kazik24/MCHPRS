use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct TaskMonitor {
    cancelled: AtomicBool,
}

impl TaskMonitor {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
