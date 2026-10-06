//! Shared byte reservations. Stored history and temporary work have separate limits.
use crate::config::CONFIG;
use crate::messages;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};

pub(super) static STORED: Lazy<Arc<Budget>> = Lazy::new(|| {
    let limit =
        mib_to_bytes(CONFIG.rhistory_memory_limit_mib).expect("invalid rhistory_memory_limit_mib");
    Budget::new(limit)
});

pub(super) static WORK: Lazy<Arc<Budget>> = Lazy::new(|| {
    let limit = mib_to_bytes(CONFIG.rhistory_work_memory_limit_mib)
        .expect("invalid rhistory_work_memory_limit_mib");
    Budget::new(limit)
});

pub(super) fn mib_to_bytes(mib: i64) -> Result<usize, String> {
    usize::try_from(mib)
        .ok()
        .and_then(|n| n.checked_mul(1024 * 1024))
        .ok_or_else(|| messages::HISTORY_INVALID_MEMORY_LIMIT.into())
}

#[derive(Default)]
struct Usage {
    used: usize,
    limit: usize,
}

pub(super) struct Budget {
    usage: Mutex<Usage>,
}

impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            usage: Mutex::new(Usage { used: 0, limit }),
        })
    }

    pub fn stats(&self) -> (usize, usize) {
        let usage = self.usage.lock().unwrap();
        (usage.used, usage.limit)
    }

    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, String> {
        let mut usage = self.usage.lock().unwrap();
        let total = usage
            .used
            .checked_add(bytes)
            .ok_or_else(|| messages::HISTORY_MEMORY_LIMIT_REACHED.to_owned())?;
        if total > usage.limit {
            return Err(messages::HISTORY_MEMORY_LIMIT_REACHED.to_owned());
        }

        usage.used = total;
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }

    pub fn set_limit(
        &self,
        limit: usize,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut usage = self.usage.lock().unwrap();
        if limit < usage.used {
            return Err(messages::history_free_buffers_first(super::format_memory(
                usage.used,
            )));
        }

        persist()?;
        usage.limit = limit;
        Ok(())
    }
}

pub(super) struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.usage.lock().unwrap().used -= self.bytes;
    }
}

// Field order ensures the allocation is freed before its reservation is released.
pub(super) struct Bytes {
    pub data: Vec<u8>,
    _reservation: Reservation,
}

impl Bytes {
    pub fn zeroed(budget: &Arc<Budget>, size: usize) -> Result<Self, String> {
        let reservation = budget.reserve(size)?;
        let mut data = Vec::new();
        data.try_reserve_exact(size)
            .map_err(|_| messages::HISTORY_ALLOCATION_FAILED.to_owned())?;
        debug_assert_eq!(data.capacity(), size);
        data.resize(size, 0);

        Ok(Self {
            data,
            _reservation: reservation,
        })
    }

    pub fn copy(budget: &Arc<Budget>, data: &[u8]) -> Result<Self, String> {
        let mut bytes = Self::zeroed(budget, data.len())?;
        bytes.data.copy_from_slice(data);
        Ok(bytes)
    }
}
