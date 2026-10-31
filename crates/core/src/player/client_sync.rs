//! Teleport confirmation is a fence against movement queued at the old position.
use std::time::{Duration, Instant};

const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct TeleportState {
    next_id: i32,
    pending: Option<(i32, Instant)>,
}

impl TeleportState {
    pub fn begin(&mut self, now: Instant) -> i32 {
        self.next_id = if self.next_id == i32::MAX { 0 } else { self.next_id + 1 };
        self.pending = Some((self.next_id, now));
        self.next_id
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn confirm(&mut self, id: i32) -> bool {
        if self.pending.is_some_and(|(expected, _)| id == expected) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub fn retry(&mut self, now: Instant) -> Option<i32> {
        let (id, sent) = self.pending.as_mut()?;
        if now.duration_since(*sent) < RETRY_INTERVAL {
            return None;
        }
        *sent = now;
        Some(*id)
    }
}

#[cfg(test)]
#[path = "client_sync_unit_tests.rs"]
mod tests;
