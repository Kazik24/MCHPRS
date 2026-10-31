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
