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
        self.next_id = if self.next_id == i32::MAX {
            0
        } else {
            self.next_id + 1
        };
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
mod tests {
    use super::*;

    #[test]
    fn old_and_duplicate_confirmations_cannot_release_a_new_teleport() {
        let now = Instant::now();
        let mut state = TeleportState::default();
        let first = state.begin(now);
        let second = state.begin(now);
        assert_ne!(first, second);
        assert!(!state.confirm(first));
        assert!(state.pending());
        assert!(state.confirm(second));
        assert!(!state.pending());
        assert!(!state.confirm(second));
        let third = state.begin(now);
        assert!(!state.confirm(second));
        assert!(state.pending());
        assert!(state.confirm(third));
    }

    #[test]
    fn retry_keeps_the_pending_id_and_stops_after_confirmation() {
        let now = Instant::now();
        let mut state = TeleportState::default();
        let id = state.begin(now);
        assert_eq!(state.retry(now + Duration::from_millis(999)), None);
        assert_eq!(state.retry(now + RETRY_INTERVAL), Some(id));
        assert!(state.pending());
        assert_eq!(state.retry(now + RETRY_INTERVAL), None);
        assert!(state.confirm(id));
        assert_eq!(state.retry(now + RETRY_INTERVAL * 2), None);
    }

    #[test]
    fn teleport_ids_wrap_without_becoming_negative() {
        let mut state = TeleportState {
            next_id: i32::MAX - 1,
            ..Default::default()
        };
        let now = Instant::now();
        assert_eq!(state.begin(now), i32::MAX);
        assert_eq!(state.begin(now), 0);
        assert!(!state.confirm(i32::MAX));
        assert!(state.confirm(0));
    }
}
