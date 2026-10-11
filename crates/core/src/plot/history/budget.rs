//! History-specific limits backed by the shared plot memory budget.
use crate::config::CONFIG;
use crate::messages;
use crate::plot::memory_budget;
use once_cell::sync::Lazy;
use std::sync::Arc;

pub(super) static STORED: Lazy<Arc<Budget>> = Lazy::new(|| {
    let limit =
        mib_to_bytes(CONFIG.rhistory_memory_limit_mib).expect("invalid rhistory_memory_limit_mib");
    memory_budget::Budget::new(limit)
});

pub(super) static WORK: Lazy<Arc<Budget>> = Lazy::new(|| {
    let limit = mib_to_bytes(CONFIG.rhistory_work_memory_limit_mib)
        .expect("invalid rhistory_work_memory_limit_mib");
    memory_budget::Budget::new(limit)
});

pub(super) type Budget = memory_budget::Budget;
pub(super) type Reservation = memory_budget::Reservation;

pub(super) fn mib_to_bytes(mib: i64) -> Result<usize, String> {
    usize::try_from(mib)
        .ok()
        .and_then(|n| n.checked_mul(1024 * 1024))
        .ok_or_else(|| messages::HISTORY_INVALID_MEMORY_LIMIT.into())
}

pub(super) struct Bytes {
    pub data: Vec<u8>,
    _reservation: Reservation,
}

impl Bytes {
    pub fn zeroed(budget: &Arc<Budget>, size: usize) -> Result<Self, String> {
        let reservation = budget
            .reserve(size)
            .ok_or_else(|| messages::HISTORY_MEMORY_LIMIT_REACHED.to_owned())?;
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
