//! Fallible allocation helpers for native compiler buffers.
use super::scratch::ScratchError;

const LOW_MEMORY: &str = "Redpiler compilation stopped: insufficient server memory; free memory or compile a smaller circuit";

fn allocation_error() -> ScratchError {
    ScratchError {
        position: None,
        reason: LOW_MEMORY,
    }
}

pub(super) fn reserve<T>(values: &mut Vec<T>, additional_elements: usize) -> Result<(), ScratchError> {
    values.try_reserve_exact(additional_elements).map_err(|_| allocation_error())
}

pub(super) fn zeroed<T: Default + Clone>(length: usize) -> Result<Vec<T>, ScratchError> {
    let mut values = Vec::new();
    reserve(&mut values, length)?;
    values.resize(length, T::default());
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_overflow_preserves_existing_values() {
        assert_eq!(zeroed::<u64>(usize::MAX).unwrap_err().reason, LOW_MEMORY);
        let mut values = vec![1u64, 2];
        assert_eq!(reserve(&mut values, usize::MAX).unwrap_err().reason, LOW_MEMORY);
        assert_eq!(values, [1, 2]);
    }
}
