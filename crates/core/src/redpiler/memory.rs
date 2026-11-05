//! Linux headroom checks for compiler buffers and long-running probes.
// shortcut: concurrent allocations can outpace these checks; use worker processes for hard OOM/native-fault isolation.
use super::scratch::ScratchError;

pub(super) const LOW_MEMORY: &str = "Redpiler compilation stopped: insufficient server memory; free memory or compile a smaller circuit";
#[cfg(target_os = "linux")]
mod linux;

#[cfg(any(target_os = "linux", test))]
const HEADROOM: usize = 512 * 1024 * 1024;

#[cfg(any(target_os = "linux", test))]
fn admit(available_bytes: usize, additional_bytes: usize) -> Result<(), ScratchError> {
    if additional_bytes.checked_add(HEADROOM).is_none_or(|needed| needed > available_bytes) {
        Err(allocation_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn check(additional_bytes: usize) -> Result<(), ScratchError> {
    let available_bytes = linux::available_memory().map_err(|error| {
        tracing::warn!(%error, "cannot read Redpiler memory limits");
        ScratchError {
            position: None,
            reason: "Redpiler cannot read server memory limits; see server log",
        }
    })?;
    admit(available_bytes, additional_bytes)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn check(_additional_bytes: usize) -> Result<(), ScratchError> {
    Ok(())
}

pub(super) fn allocation_error() -> ScratchError {
    ScratchError { position: None, reason: LOW_MEMORY }
}

pub(super) fn reserve<T>(values: &mut Vec<T>, additional_elements: usize) -> Result<(), ScratchError> {
    let additional_bytes = additional_elements.checked_mul(std::mem::size_of::<T>()).ok_or_else(allocation_error)?;
    check(additional_bytes)?;
    values.try_reserve_exact(additional_elements).map_err(|_| allocation_error())
}

pub(super) fn zeroed<T: Default + Clone>(length: usize) -> Result<Vec<T>, ScratchError> {
    let mut values = Vec::new();
    reserve(&mut values, length)?;
    values.resize(length, T::default());
    Ok(values)
}

#[cfg(target_os = "linux")]
pub(super) fn check_periodic() -> Result<(), ScratchError> {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };
    static LAST: Mutex<Option<(Instant, Result<(), ScratchError>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((at, result)) = &*last
        && at.elapsed() < Duration::from_millis(250)
    {
        return result.clone();
    }
    let result = check(0);
    *last = Some((Instant::now(), result.clone()));
    result
}

#[cfg(not(target_os = "linux"))]
pub(super) fn check_periodic() -> Result<(), ScratchError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_keeps_headroom_and_rejects_overflow() {
        assert!(admit(HEADROOM + 100, 100).is_ok());
        assert_eq!(admit(HEADROOM + 99, 100).unwrap_err().reason, LOW_MEMORY);
        assert!(admit(usize::MAX, usize::MAX).is_err());
        assert_eq!(zeroed::<u64>(usize::MAX).unwrap_err().reason, LOW_MEMORY);
    }
}
