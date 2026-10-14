//! Shared byte reservations for bounded plot memory work.
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Usage {
    used: usize,
    limit: usize,
}

pub(crate) struct Budget {
    usage: Mutex<Usage>,
}

impl Budget {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            usage: Mutex::new(Usage { used: 0, limit }),
        })
    }

    pub(crate) fn stats(&self) -> (usize, usize) {
        let usage = self.usage.lock().unwrap();
        (usage.used, usage.limit)
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Reservation> {
        let mut usage = self.usage.lock().unwrap();
        let total = usage.used.checked_add(bytes)?;
        if total > usage.limit {
            return None;
        }
        usage.used = total;
        Some(Reservation { budget: self.clone(), bytes })
    }

    pub(crate) fn set_limit<T>(
        &self,
        limit: usize,
        too_small: impl FnOnce(usize) -> String,
        before_change: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut usage = self.usage.lock().unwrap();
        if limit < usage.used {
            return Err(too_small(usage.used));
        }
        let result = before_change()?;
        usage.limit = limit;
        Ok(result)
    }
}

pub(crate) struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Reservation {
    pub(crate) fn grow(&mut self, bytes: usize) -> bool {
        let Some(total) = self.bytes.checked_add(bytes) else {
            return false;
        };
        let mut usage = self.budget.usage.lock().unwrap();
        let Some(used) = usage.used.checked_add(bytes) else {
            return false;
        };
        if used > usage.limit {
            return false;
        }
        usage.used = used;
        self.bytes = total;
        true
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.usage.lock().unwrap().used -= self.bytes;
    }
}
